use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use fast_image_resize::{
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
    images::{CroppedImageMut, Image, ImageRef},
};
use image::RgbaImage;
use openh264::{
    OpenH264API,
    encoder::{
        BitRate, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, Level, Profile,
        RateControlMode, UsageType, VuiConfig,
    },
    formats::YUVSlices,
};
use tokio::sync::mpsc;
use yuv::{
    YuvChromaSubsampling, YuvConversionMode, YuvPlanarImageMut, YuvRange, YuvStandardMatrix,
    rgba_to_yuv420,
};

use super::{
    StreamCaptureTarget,
    preview::{StreamPreviewCadence, StreamPreviewFrame},
};
use crate::logging;

#[cfg(target_os = "linux")]
#[path = "capture/linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "capture/macos.rs"]
mod platform;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
#[path = "capture/unsupported.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "capture/windows.rs"]
mod platform;

pub(super) const STREAM_CAPTURE_WIDTH: u32 = 1280;
pub(super) const STREAM_CAPTURE_HEIGHT: u32 = 720;
pub(super) const STREAM_CAPTURE_FPS: u32 = 30;
pub(super) const STREAM_CAPTURE_BITRATE: u32 = 8_000_000;
// Explicit feedback can still request immediate recovery frames, so the
// fallback GOP can avoid a large encoded-frame burst every second.
const STREAM_INTRA_FRAME_PERIOD_FRAMES: u32 = STREAM_CAPTURE_FPS * 2;
const STREAM_CAPTURE_FRAME_INTERVAL: Duration =
    Duration::from_nanos(1_000_000_000 / STREAM_CAPTURE_FPS as u64);
const STREAM_CAPTURE_STATS_INTERVAL: Duration = Duration::from_secs(5);
const STREAM_RECORDER_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STREAM_CAPTURE_GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const STREAM_CAPTURE_SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub(super) struct EncodedStreamFrame {
    pub(super) timestamp: u32,
    pub(super) annex_b: Vec<u8>,
    pub(super) is_keyframe: bool,
}

pub(super) struct StreamCaptureHandle {
    stop: Arc<AtomicBool>,
    force_keyframe: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

pub(super) struct PreparedStreamCapture {
    pub(super) handle: StreamCaptureHandle,
    pub(super) frames: mpsc::Receiver<Result<EncodedStreamFrame, String>>,
    pub(super) preview_frames: Option<mpsc::Receiver<StreamPreviewFrame>>,
    pub(super) errors: mpsc::UnboundedReceiver<String>,
}

pub(super) struct CaptureFrame {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba: Vec<u8>,
}

struct CaptureSource {
    session: platform::CaptureSession,
    frames: Receiver<Result<CaptureFrame, String>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CaptureFrameOutcome {
    Queued,
    QueueFull,
    QueueClosed,
    EncoderSkipped,
}

struct CaptureFrameTimings {
    capture: Duration,
    prepare: Duration,
    resize: Duration,
    color_convert: Duration,
    encode: Duration,
    total: Duration,
}

struct FramePreparationTimings {
    resize: Duration,
    color_convert: Duration,
}

struct PreparedStreamFrame {
    timings: FramePreparationTimings,
    preview: Option<StreamPreviewFrame>,
}

struct CapturePerformanceStats {
    window_started_at: Instant,
    captured_frames: u64,
    queued_frames: u64,
    queue_full_frames: u64,
    encoder_skipped_frames: u64,
    encoded_bytes: u64,
    capture_duration: Duration,
    prepare_duration: Duration,
    resize_duration: Duration,
    color_convert_duration: Duration,
    encode_duration: Duration,
    total_duration: Duration,
    max_frame_duration: Duration,
}

struct StreamFramePacer {
    next_deadline: Instant,
}

impl StreamFramePacer {
    fn new(started_at: Instant) -> Self {
        Self {
            next_deadline: started_at + STREAM_CAPTURE_FRAME_INTERVAL,
        }
    }

    fn wait_for_next_frame(&mut self) {
        let deadline = next_stream_frame_deadline(self.next_deadline, Instant::now());
        if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            thread::sleep(remaining);
        }
        self.next_deadline = deadline + STREAM_CAPTURE_FRAME_INTERVAL;
    }
}

fn next_stream_frame_deadline(current_deadline: Instant, now: Instant) -> Instant {
    let Some(overdue) = now.checked_duration_since(current_deadline) else {
        return current_deadline;
    };
    let missed_intervals = overdue.as_nanos() / STREAM_CAPTURE_FRAME_INTERVAL.as_nanos() + 1;
    let Ok(missed_intervals) = u32::try_from(missed_intervals) else {
        return now + STREAM_CAPTURE_FRAME_INTERVAL;
    };

    current_deadline
        .checked_add(STREAM_CAPTURE_FRAME_INTERVAL * missed_intervals)
        .unwrap_or(now + STREAM_CAPTURE_FRAME_INTERVAL)
}

impl CapturePerformanceStats {
    fn new() -> Self {
        Self {
            window_started_at: Instant::now(),
            captured_frames: 0,
            queued_frames: 0,
            queue_full_frames: 0,
            encoder_skipped_frames: 0,
            encoded_bytes: 0,
            capture_duration: Duration::ZERO,
            prepare_duration: Duration::ZERO,
            resize_duration: Duration::ZERO,
            color_convert_duration: Duration::ZERO,
            encode_duration: Duration::ZERO,
            total_duration: Duration::ZERO,
            max_frame_duration: Duration::ZERO,
        }
    }

    fn record_frame(
        &mut self,
        outcome: CaptureFrameOutcome,
        encoded_bytes: usize,
        timings: CaptureFrameTimings,
        target: &str,
    ) {
        self.captured_frames += 1;
        match outcome {
            CaptureFrameOutcome::Queued => self.queued_frames += 1,
            CaptureFrameOutcome::QueueFull => self.queue_full_frames += 1,
            CaptureFrameOutcome::QueueClosed => {}
            CaptureFrameOutcome::EncoderSkipped => self.encoder_skipped_frames += 1,
        }
        self.encoded_bytes = self.encoded_bytes.saturating_add(encoded_bytes as u64);
        self.capture_duration += timings.capture;
        self.prepare_duration += timings.prepare;
        self.resize_duration += timings.resize;
        self.color_convert_duration += timings.color_convert;
        self.encode_duration += timings.encode;
        self.total_duration += timings.total;
        self.max_frame_duration = self.max_frame_duration.max(timings.total);
        self.log_if_due(target);
    }

    fn log_if_due(&mut self, target: &str) {
        let elapsed = self.window_started_at.elapsed();
        if elapsed < STREAM_CAPTURE_STATS_INTERVAL {
            return;
        }

        logging::debug(
            "stream",
            format!(
                "broadcast capture stats: target={} elapsed_ms={} input_fps={:.1} queued_fps={:.1} queue_full_frames={} encoder_skipped_frames={} encoded_mbps={:.2} avg_capture_ms={:.1} avg_prepare_ms={:.1} avg_resize_ms={:.1} avg_color_convert_ms={:.1} avg_encode_ms={:.1} avg_frame_ms={:.1} max_frame_ms={:.1}",
                target,
                elapsed.as_millis(),
                rate_per_second(self.captured_frames, elapsed),
                rate_per_second(self.queued_frames, elapsed),
                self.queue_full_frames,
                self.encoder_skipped_frames,
                bits_per_second(self.encoded_bytes, elapsed) / 1_000_000.0,
                average_millis(self.capture_duration, self.captured_frames),
                average_millis(self.prepare_duration, self.captured_frames),
                average_millis(self.resize_duration, self.captured_frames),
                average_millis(self.color_convert_duration, self.captured_frames),
                average_millis(self.encode_duration, self.captured_frames),
                average_millis(self.total_duration, self.captured_frames),
                self.max_frame_duration.as_secs_f64() * 1_000.0,
            ),
        );

        *self = Self::new();
    }
}

impl CaptureSource {
    fn capture_image(&self, stop: &AtomicBool) -> Result<Option<RgbaImage>, String> {
        loop {
            match self.frames.recv_timeout(STREAM_RECORDER_POLL_INTERVAL) {
                Ok(frame) => {
                    let mut frame = frame?;
                    while let Ok(newer_frame) = self.frames.try_recv() {
                        frame = newer_frame?;
                    }
                    return RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
                        .map(Some)
                        .ok_or_else(|| {
                            "capture backend returned an invalid RGBA frame".to_owned()
                        });
                }
                Err(RecvTimeoutError::Timeout) if stop.load(Ordering::Acquire) => return Ok(None),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("capture backend stopped unexpectedly".to_owned());
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamFrameGeometry {
    source_dimensions: (u32, u32),
    scaled_dimensions: (u32, u32),
    offsets: (u32, u32),
}

impl StreamFrameGeometry {
    fn for_source(source_dimensions: (u32, u32)) -> Result<Self, String> {
        let (source_width, source_height) = source_dimensions;
        if source_width == 0 || source_height == 0 {
            return Err("capture source returned an empty frame".to_owned());
        }

        let scale = (STREAM_CAPTURE_WIDTH as f64 / source_width as f64)
            .min(STREAM_CAPTURE_HEIGHT as f64 / source_height as f64);
        // Multiples of four keep the scaled content and letterbox offsets
        // aligned to the YUV420 chroma grid used after RGBA resizing.
        let scaled_width = align_yuv420_dimension(
            (source_width as f64 * scale).round() as u32,
            STREAM_CAPTURE_WIDTH,
        );
        let scaled_height = align_yuv420_dimension(
            (source_height as f64 * scale).round() as u32,
            STREAM_CAPTURE_HEIGHT,
        );

        Ok(Self {
            source_dimensions,
            scaled_dimensions: (scaled_width, scaled_height),
            offsets: (
                (STREAM_CAPTURE_WIDTH - scaled_width) / 2,
                (STREAM_CAPTURE_HEIGHT - scaled_height) / 2,
            ),
        })
    }
}

fn align_yuv420_dimension(value: u32, maximum: u32) -> u32 {
    value.clamp(4, maximum) & !3
}

struct StreamFrameProcessor {
    resizer: Resizer,
    resize_options: ResizeOptions,
    rgba: Image<'static>,
    yuv: YuvPlanarImageMut<'static, u8>,
    geometry: Option<StreamFrameGeometry>,
}

impl StreamFrameProcessor {
    fn new() -> Self {
        let mut rgba = Image::new(STREAM_CAPTURE_WIDTH, STREAM_CAPTURE_HEIGHT, PixelType::U8x4);
        fill_opaque_black(rgba.buffer_mut());

        Self {
            resizer: Resizer::new(),
            // Box filtering is close to the previous thumbnail behavior and
            // avoids the higher cost of the library's default Lanczos filter.
            resize_options: ResizeOptions::new()
                .resize_alg(ResizeAlg::Convolution(FilterType::Box))
                .use_alpha(false),
            rgba,
            yuv: YuvPlanarImageMut::alloc(
                STREAM_CAPTURE_WIDTH,
                STREAM_CAPTURE_HEIGHT,
                YuvChromaSubsampling::Yuv420,
            ),
            geometry: None,
        }
    }

    fn prepare(
        &mut self,
        image: RgbaImage,
        include_preview: bool,
    ) -> Result<PreparedStreamFrame, String> {
        let original_dimensions = image.dimensions();
        let geometry = StreamFrameGeometry::for_source(image.dimensions())?;
        self.update_geometry(geometry);

        let resize_started_at = Instant::now();
        let source = ImageRef::new(
            geometry.source_dimensions.0,
            geometry.source_dimensions.1,
            image.as_raw(),
            PixelType::U8x4,
        )
        .map_err(|error| format!("captured RGBA frame is invalid: {error}"))?;
        let mut destination = CroppedImageMut::new(
            &mut self.rgba,
            geometry.offsets.0,
            geometry.offsets.1,
            geometry.scaled_dimensions.0,
            geometry.scaled_dimensions.1,
        )
        .map_err(|error| format!("stream RGBA destination crop is invalid: {error}"))?;
        self.resizer
            .resize(&source, &mut destination, &self.resize_options)
            .map_err(|error| format!("stream RGBA resize failed: {error}"))?;
        let resize = resize_started_at.elapsed();

        let color_convert_started_at = Instant::now();
        // The measured Apple Silicon path is faster when conversion runs on
        // the final 720p buffer instead of the full-resolution capture.
        rgba_to_yuv420(
            &mut self.yuv,
            self.rgba.buffer(),
            STREAM_CAPTURE_WIDTH * 4,
            YuvRange::Limited,
            YuvStandardMatrix::Bt709,
            YuvConversionMode::Fast,
        )
        .map_err(|error| format!("RGBA to YUV conversion failed: {error}"))?;
        let color_convert = color_convert_started_at.elapsed();

        Ok(PreparedStreamFrame {
            timings: FramePreparationTimings {
                resize,
                color_convert,
            },
            preview: include_preview.then(|| StreamPreviewFrame {
                width: original_dimensions.0,
                height: original_dimensions.1,
                rgba: image.into_raw(),
            }),
        })
    }

    fn yuv_source(&self) -> YUVSlices<'_> {
        YUVSlices::new(
            (
                self.yuv.y_plane.borrow(),
                self.yuv.u_plane.borrow(),
                self.yuv.v_plane.borrow(),
            ),
            (
                STREAM_CAPTURE_WIDTH as usize,
                STREAM_CAPTURE_HEIGHT as usize,
            ),
            (
                self.yuv.y_stride as usize,
                self.yuv.u_stride as usize,
                self.yuv.v_stride as usize,
            ),
        )
    }

    fn update_geometry(&mut self, geometry: StreamFrameGeometry) {
        if self.geometry == Some(geometry) {
            return;
        }

        fill_opaque_black(self.rgba.buffer_mut());
        self.geometry = Some(geometry);
    }
}

fn fill_opaque_black(rgba: &mut [u8]) {
    rgba.fill(0);
    for alpha in rgba.iter_mut().skip(3).step_by(4) {
        *alpha = 255;
    }
}

impl Drop for CaptureSource {
    fn drop(&mut self) {
        if let Err(error) = self.session.stop() {
            logging::debug(
                "stream",
                format!("capture backend stop failed during shutdown: {error}"),
            );
        }
    }
}

impl Drop for StreamCaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            reap_capture_worker(worker);
        }
    }
}

fn reap_capture_worker(worker: JoinHandle<()>) {
    if let Err(error) = thread::Builder::new()
        .name("stream-capture-reaper".to_owned())
        .spawn(move || {
            if let Err(error) = worker.join() {
                logging::debug(
                    "stream",
                    format!("stream capture worker panicked during shutdown: {error:?}"),
                );
            }
        })
    {
        // Dropping the join handle detaches the worker, so a reaper spawn
        // failure still leaves shutdown nonblocking.
        logging::debug(
            "stream",
            format!("stream capture reaper spawn failed: {error}"),
        );
    }
}

impl StreamCaptureHandle {
    pub(super) fn request_keyframe(&self) {
        self.force_keyframe.store(true, Ordering::Release);
    }

    pub(super) async fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return;
        };
        let deadline = Instant::now() + STREAM_CAPTURE_GRACEFUL_SHUTDOWN_TIMEOUT;
        while !worker.is_finished() {
            if Instant::now() >= deadline {
                // Native capture shutdown can wait for an operating-system
                // callback. Move ownership outside Tokio so runtime shutdown
                // stays bounded.
                logging::debug(
                    "stream",
                    "stream capture worker did not stop before graceful shutdown timeout; reaping outside Tokio",
                );
                reap_capture_worker(worker);
                return;
            }
            tokio::time::sleep(STREAM_CAPTURE_SHUTDOWN_POLL_INTERVAL).await;
        }
        if let Err(error) = worker.join() {
            logging::debug(
                "stream",
                format!("stream capture worker panicked during shutdown: {error:?}"),
            );
        }
    }
}

pub(crate) fn list_stream_capture_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    let mut targets = platform::list_targets()?;

    targets.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
    targets.dedup_by(|left, right| left.kind == right.kind && left.id == right.id);
    if targets.is_empty() {
        return Err("no capturable screens or windows were found".to_owned());
    }
    Ok(targets)
}

pub(super) fn prepare_stream_capture(
    target: StreamCaptureTarget,
) -> Result<PreparedStreamCapture, String> {
    let (frames_tx, frames) = mpsc::channel(2);
    let (preview_frames_tx, preview_frames) = mpsc::channel(1);
    let (errors_tx, errors) = mpsc::unbounded_channel();
    let (ready_tx, ready_rx) = sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    let force_keyframe = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker_force_keyframe = Arc::clone(&force_keyframe);
    let worker_ready_tx = ready_tx.clone();
    let worker = thread::Builder::new()
        .name("stream-capture".to_owned())
        .spawn(move || {
            run_capture_worker(
                target,
                frames_tx,
                preview_frames_tx,
                errors_tx,
                worker_stop,
                worker_force_keyframe,
                worker_ready_tx,
            );
        })
        .map_err(|error| format!("stream capture worker spawn failed: {error}"))?;
    let handle = StreamCaptureHandle {
        stop,
        force_keyframe,
        worker: Some(worker),
    };

    ready_rx
        .recv()
        .map_err(|_| "stream capture stopped before producing a frame".to_owned())??;
    Ok(PreparedStreamCapture {
        handle,
        frames,
        preview_frames: Some(preview_frames),
        errors,
    })
}

fn run_capture_worker(
    target: StreamCaptureTarget,
    frames_tx: mpsc::Sender<Result<EncodedStreamFrame, String>>,
    preview_frames_tx: mpsc::Sender<StreamPreviewFrame>,
    errors_tx: mpsc::UnboundedSender<String>,
    stop: Arc<AtomicBool>,
    force_keyframe: Arc<AtomicBool>,
    ready_tx: SyncSender<Result<(), String>>,
) {
    if let Err(error) = run_capture_loop(
        &target,
        &frames_tx,
        &preview_frames_tx,
        &stop,
        &force_keyframe,
        ready_tx.clone(),
    ) {
        let _ = ready_tx.send(Err(error.clone()));
        let _ = errors_tx.send(error);
    }
}

fn run_capture_loop(
    target: &StreamCaptureTarget,
    frames_tx: &mpsc::Sender<Result<EncodedStreamFrame, String>>,
    preview_frames_tx: &mpsc::Sender<StreamPreviewFrame>,
    stop: &AtomicBool,
    force_keyframe: &AtomicBool,
    ready_tx: SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let source = resolve_capture_source(target)?;
    let mut encoder = Encoder::with_api_config(OpenH264API::from_source(), stream_encoder_config())
        .map_err(|error| format!("H264 encoder creation failed: {error}"))?;
    let mut ready_tx = Some(ready_tx);
    let started_at = Instant::now();
    let mut frame_pacer = StreamFramePacer::new(started_at);
    let mut stats = CapturePerformanceStats::new();
    let mut frame_processor = StreamFrameProcessor::new();
    let mut preview_cadence = StreamPreviewCadence::default();

    while !stop.load(Ordering::Acquire) {
        let frame_started_at = Instant::now();

        let capture_started_at = Instant::now();
        let Some(image) = source.capture_image(stop)? else {
            return Ok(());
        };
        let capture_time = capture_started_at.elapsed();

        let preview_now = Instant::now();
        let preview_permit = preview_cadence
            .is_due(preview_now)
            .then(|| preview_frames_tx.try_reserve().ok())
            .flatten();
        let prepare_started_at = Instant::now();
        let prepared = frame_processor.prepare(image, preview_permit.is_some())?;
        let prepare_time = prepare_started_at.elapsed();
        if let (Some(permit), Some(preview)) = (preview_permit, prepared.preview) {
            permit.send(preview);
            preview_cadence.record_queued(preview_now);
        }

        let frame_slot = match try_reserve_encoded_frame_slot(frames_tx) {
            Ok(permit) => permit,
            Err(CaptureFrameOutcome::QueueFull) => {
                stats.record_frame(
                    CaptureFrameOutcome::QueueFull,
                    0,
                    CaptureFrameTimings {
                        capture: capture_time,
                        prepare: prepare_time,
                        resize: prepared.timings.resize,
                        color_convert: prepared.timings.color_convert,
                        encode: Duration::ZERO,
                        total: frame_started_at.elapsed(),
                    },
                    &target.title,
                );
                frame_pacer.wait_for_next_frame();
                continue;
            }
            Err(CaptureFrameOutcome::QueueClosed) => return Ok(()),
            Err(_) => unreachable!("frame reservation only reports queue availability"),
        };

        let encode_started_at = Instant::now();
        if force_keyframe.swap(false, Ordering::AcqRel) {
            encoder.force_intra_frame();
        }
        let yuv = frame_processor.yuv_source();
        let encoded = encoder
            .encode(&yuv)
            .map_err(|error| format!("H264 frame encoding failed: {error}"))?;
        let is_keyframe = matches!(encoded.frame_type(), FrameType::IDR | FrameType::I);
        let annex_b = encoded.to_vec();
        let encode_time = encode_started_at.elapsed();
        let encoded_bytes = annex_b.len();
        let outcome = if annex_b.is_empty() {
            CaptureFrameOutcome::EncoderSkipped
        } else {
            let timestamp = stream_rtp_timestamp(started_at.elapsed());
            frame_slot.send(Ok(EncodedStreamFrame {
                timestamp,
                annex_b,
                is_keyframe,
            }));
            if let Some(ready_tx) = ready_tx.take() {
                let _ = ready_tx.send(Ok(()));
            }
            CaptureFrameOutcome::Queued
        };
        stats.record_frame(
            outcome,
            encoded_bytes,
            CaptureFrameTimings {
                capture: capture_time,
                prepare: prepare_time,
                resize: prepared.timings.resize,
                color_convert: prepared.timings.color_convert,
                encode: encode_time,
                total: frame_started_at.elapsed(),
            },
            &target.title,
        );

        frame_pacer.wait_for_next_frame();
    }
    Ok(())
}

fn try_reserve_encoded_frame_slot(
    frames_tx: &mpsc::Sender<Result<EncodedStreamFrame, String>>,
) -> Result<mpsc::Permit<'_, Result<EncodedStreamFrame, String>>, CaptureFrameOutcome> {
    match frames_tx.try_reserve() {
        Ok(permit) => Ok(permit),
        Err(mpsc::error::TrySendError::Full(_)) => Err(CaptureFrameOutcome::QueueFull),
        Err(mpsc::error::TrySendError::Closed(_)) => Err(CaptureFrameOutcome::QueueClosed),
    }
}

fn stream_encoder_config() -> EncoderConfig {
    // OpenH264 enables these camera-oriented tools by default, but its
    // screen-content mode rejects them and writes warnings directly to stderr.
    EncoderConfig::new()
        .usage_type(UsageType::ScreenContentRealTime)
        .skip_frames(true)
        .adaptive_quantization(false)
        .background_detection(false)
        .rate_control_mode(RateControlMode::Bitrate)
        .bitrate(BitRate::from_bps(STREAM_CAPTURE_BITRATE))
        .max_frame_rate(FrameRate::from_hz(STREAM_CAPTURE_FPS as f32))
        .profile(Profile::Baseline)
        .level(Level::Level_3_1)
        .intra_frame_period(IntraFramePeriod::from_num_frames(
            STREAM_INTRA_FRAME_PERIOD_FRAMES,
        ))
        .vui(VuiConfig::bt709())
}

fn resolve_capture_source(target: &StreamCaptureTarget) -> Result<CaptureSource, String> {
    let (session, frames) = platform::start_capture(target)?;
    logging::debug("stream", "native continuous capture started");
    Ok(CaptureSource { session, frames })
}

fn stream_rtp_timestamp(elapsed: Duration) -> u32 {
    let ticks = elapsed.as_micros().saturating_mul(90) / 1_000;
    ticks as u32
}

fn rate_per_second(count: u64, elapsed: Duration) -> f64 {
    count as f64 / elapsed.as_secs_f64().max(f64::EPSILON)
}

fn bits_per_second(bytes: u64, elapsed: Duration) -> f64 {
    rate_per_second(bytes.saturating_mul(8), elapsed)
}

fn average_millis(duration: Duration, samples: u64) -> f64 {
    duration.as_secs_f64() * 1_000.0 / samples.max(1) as f64
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    #[test]
    fn screen_content_encoder_configuration_initializes_cleanly() {
        let _encoder =
            Encoder::with_api_config(OpenH264API::from_source(), stream_encoder_config())
                .expect("screen content encoder configuration should initialize");
    }

    #[test]
    fn screen_content_encoder_uses_a_two_second_intra_period() {
        let config = format!("{:?}", stream_encoder_config());

        assert!(
            config.contains("intra_frame_period: IntraFramePeriod(60)"),
            "unexpected stream encoder configuration: {config}"
        );
    }

    #[test]
    fn screen_content_encoder_targets_eight_megabits_per_second() {
        let config = format!("{:?}", stream_encoder_config());

        assert!(
            config.contains("bitrate: BitRate(8000000)"),
            "unexpected stream encoder configuration: {config}"
        );
    }

    #[test]
    fn capture_handle_coalesces_keyframe_requests() {
        let mut handle = StreamCaptureHandle {
            stop: Arc::new(AtomicBool::new(false)),
            force_keyframe: Arc::new(AtomicBool::new(false)),
            worker: None,
        };

        handle.request_keyframe();
        handle.request_keyframe();

        assert!(handle.force_keyframe.swap(false, Ordering::AcqRel));
        assert!(!handle.force_keyframe.swap(false, Ordering::AcqRel));
        handle.worker = None;
    }

    #[test]
    fn capture_handle_drop_does_not_wait_for_worker_shutdown() {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = release_rx.recv();
        });
        let handle = StreamCaptureHandle {
            stop: Arc::new(AtomicBool::new(false)),
            force_keyframe: Arc::new(AtomicBool::new(false)),
            worker: Some(worker),
        };
        let (dropped_tx, dropped_rx) = std::sync::mpsc::channel();
        let dropper = std::thread::spawn(move || {
            drop(handle);
            let _ = dropped_tx.send(());
        });

        dropped_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("capture handle drop must not wait for the worker");
        release_tx
            .send(())
            .expect("test capture worker should be released");
        dropper.join().expect("test handle dropper should finish");
    }

    #[test]
    fn capture_error_uses_a_separate_nonblocking_channel() {
        let (frames_tx, mut frames_rx) = mpsc::channel::<Result<EncodedStreamFrame, String>>(1);
        frames_tx
            .try_send(Ok(EncodedStreamFrame {
                timestamp: 1,
                annex_b: vec![1],
                is_keyframe: false,
            }))
            .expect("test frame should fill the queue");
        let (errors_tx, mut errors_rx) = mpsc::unbounded_channel();

        errors_tx
            .send("capture failed".to_owned())
            .expect("capture error should not depend on frame queue capacity");

        let queued = frames_rx
            .try_recv()
            .expect("the queued frame should remain available");
        assert!(queued.is_ok());
        assert_eq!(
            errors_rx
                .try_recv()
                .expect("capture error should remain available"),
            "capture failed"
        );
    }

    #[test]
    fn full_encoded_frame_queue_skips_encoding_without_consuming_keyframe_request() {
        let (frames_tx, mut frames_rx) = mpsc::channel(1);
        let force_keyframe = AtomicBool::new(true);
        frames_tx
            .try_send(Ok(EncodedStreamFrame {
                timestamp: 1,
                annex_b: vec![1],
                is_keyframe: false,
            }))
            .expect("first test frame should fill the queue");

        match try_reserve_encoded_frame_slot(&frames_tx) {
            Err(outcome) => assert_eq!(outcome, CaptureFrameOutcome::QueueFull),
            Ok(_) => panic!("a full frame queue must not reserve another slot"),
        }
        assert!(force_keyframe.load(Ordering::Acquire));
        assert_eq!(
            frames_rx
                .try_recv()
                .expect("the older queued frame should remain")
                .expect("the older queued frame should be valid")
                .timestamp,
            1
        );

        let permit = try_reserve_encoded_frame_slot(&frames_tx)
            .expect("a drained frame queue should reserve a slot");
        permit.send(Ok(EncodedStreamFrame {
            timestamp: 2,
            annex_b: vec![2],
            is_keyframe: true,
        }));
        assert_eq!(
            frames_rx
                .try_recv()
                .expect("the reserved frame should be queued")
                .expect("the reserved frame should be valid")
                .timestamp,
            2
        );
    }

    #[test]
    fn letterbox_preserves_source_aspect_ratio_for_common_shapes() {
        let cases = [
            ((800, 600), Some((159, 0)), (160, 0)),
            ((1280, 720), None, (0, 0)),
            ((1600, 600), Some((0, 119)), (0, 120)),
        ];

        for (dimensions, black_bar, content_edge) in cases {
            let image = RgbaImage::from_pixel(dimensions.0, dimensions.1, Rgba([255, 0, 0, 255]));
            let mut processor = StreamFrameProcessor::new();
            processor
                .prepare(image, false)
                .expect("stream frame should be prepared");

            assert!(luma_pixel(&processor, 640, 360).abs_diff(63) <= 1);
            assert!(luma_pixel(&processor, content_edge.0, content_edge.1).abs_diff(63) <= 1);
            if let Some(black_bar) = black_bar {
                assert_eq!(luma_pixel(&processor, black_bar.0, black_bar.1), 16);
            }
        }
    }

    #[test]
    fn broadcast_color_conversion_and_vui_use_bt709_limited() {
        let mut processor = StreamFrameProcessor::new();
        processor
            .prepare(
                RgbaImage::from_pixel(
                    STREAM_CAPTURE_WIDTH,
                    STREAM_CAPTURE_HEIGHT,
                    Rgba([255, 0, 0, 255]),
                ),
                false,
            )
            .expect("red stream frame should be prepared");

        for value in processor.yuv.y_plane.borrow() {
            assert!(
                value.abs_diff(63) <= 1,
                "unexpected BT.709 limited red luma: {value}"
            );
        }
        let u = processor.yuv.u_plane.borrow()[0];
        let v = processor.yuv.v_plane.borrow()[0];
        assert!(u.abs_diff(102) <= 1, "unexpected BT.709 limited red U: {u}");
        assert!(v.abs_diff(240) <= 1, "unexpected BT.709 limited red V: {v}");

        let config = format!("{:?}", stream_encoder_config());
        assert!(
            config.contains("matrix_coefficients: Bt709") && config.contains("full_range: false"),
            "unexpected stream VUI configuration: {config}"
        );
    }

    #[test]
    fn frame_processor_reuses_working_buffers_for_stable_dimensions() {
        let mut processor = StreamFrameProcessor::new();
        let image = RgbaImage::from_pixel(800, 600, Rgba([12, 34, 56, 255]));
        processor
            .prepare(image.clone(), false)
            .expect("first stream frame should be prepared");
        let addresses = (
            processor.rgba.buffer().as_ptr(),
            processor.yuv.y_plane.borrow().as_ptr(),
            processor.yuv.u_plane.borrow().as_ptr(),
            processor.yuv.v_plane.borrow().as_ptr(),
        );

        processor
            .prepare(image, false)
            .expect("second stream frame should be prepared");

        assert_eq!(
            addresses,
            (
                processor.rgba.buffer().as_ptr(),
                processor.yuv.y_plane.borrow().as_ptr(),
                processor.yuv.u_plane.borrow().as_ptr(),
                processor.yuv.v_plane.borrow().as_ptr(),
            )
        );
    }

    #[test]
    fn preview_reuses_the_owned_capture_buffer() {
        let image = RgbaImage::from_pixel(800, 600, Rgba([12, 34, 56, 255]));
        let capture_buffer = image.as_raw().as_ptr();
        let mut processor = StreamFrameProcessor::new();

        let prepared = processor
            .prepare(image, true)
            .expect("stream frame should be prepared");
        let preview = prepared.preview.expect("preview should be returned");

        assert_eq!(preview.rgba.as_ptr(), capture_buffer);
        assert_eq!((preview.width, preview.height), (800, 600));
    }

    #[test]
    fn odd_capture_dimensions_keep_yuv420_output_aligned() {
        let geometry =
            StreamFrameGeometry::for_source((2057, 1329)).expect("source should be accepted");
        let mut processor = StreamFrameProcessor::new();
        processor
            .prepare(
                RgbaImage::from_pixel(801, 601, Rgba([12, 34, 56, 255])),
                false,
            )
            .expect("odd-sized stream frame should be prepared");

        assert_eq!(geometry.source_dimensions, (2057, 1329));
        assert_eq!(geometry.scaled_dimensions.0 % 4, 0);
        assert_eq!(geometry.scaled_dimensions.1 % 4, 0);
        assert_eq!(geometry.offsets.0 % 2, 0);
        assert_eq!(geometry.offsets.1 % 2, 0);
    }

    #[test]
    fn frame_deadline_corrects_sleep_overshoot_without_drift() {
        let started_at = Instant::now();
        let deadline = started_at + STREAM_CAPTURE_FRAME_INTERVAL;
        let woke_late = deadline + Duration::from_millis(4);

        assert_eq!(
            next_stream_frame_deadline(deadline, woke_late),
            deadline + STREAM_CAPTURE_FRAME_INTERVAL
        );
    }

    #[test]
    fn frame_deadline_skips_missed_intervals_without_catch_up_bursts() {
        let started_at = Instant::now();
        let deadline = started_at + STREAM_CAPTURE_FRAME_INTERVAL;
        let second_deadline = deadline + STREAM_CAPTURE_FRAME_INTERVAL;
        let third_deadline = second_deadline + STREAM_CAPTURE_FRAME_INTERVAL;
        let finished_after_third_deadline = third_deadline + Duration::from_millis(1);

        assert_eq!(
            next_stream_frame_deadline(deadline, finished_after_third_deadline),
            third_deadline + STREAM_CAPTURE_FRAME_INTERVAL
        );
    }

    #[test]
    fn performance_rates_use_the_observed_interval() {
        let elapsed = Duration::from_secs(5);

        assert_eq!(rate_per_second(150, elapsed), 30.0);
        assert_eq!(bits_per_second(3_750_000, elapsed), 6_000_000.0);
        assert_eq!(average_millis(Duration::from_millis(150), 10), 15.0);
    }

    #[test]
    fn rtp_timestamp_uses_video_clock() {
        assert_eq!(stream_rtp_timestamp(Duration::from_millis(500)), 45_000);
        assert_eq!(stream_rtp_timestamp(Duration::from_secs(1)), 90_000);
    }

    fn luma_pixel(processor: &StreamFrameProcessor, x: u32, y: u32) -> u8 {
        processor.yuv.y_plane.borrow()[(y * processor.yuv.y_stride + x) as usize]
    }
}
