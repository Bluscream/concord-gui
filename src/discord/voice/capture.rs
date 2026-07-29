use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use fast_image_resize::{
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
    images::{Image, ImageRef},
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
use xcap::{Frame, Monitor, VideoRecorder, Window};
use yuv::{
    YuvChromaSubsampling, YuvConversionMode, YuvPlanarImageMut, YuvRange, YuvStandardMatrix,
    rgba_to_yuv420,
};

use super::{StreamCaptureTarget, StreamCaptureTargetKind};
use crate::logging;

pub(super) const STREAM_CAPTURE_WIDTH: u32 = 1280;
pub(super) const STREAM_CAPTURE_HEIGHT: u32 = 720;
pub(super) const STREAM_CAPTURE_FPS: u32 = 30;
pub(super) const STREAM_CAPTURE_BITRATE: u32 = 6_000_000;
// Explicit feedback can still request immediate recovery frames, so the
// fallback GOP can avoid a large encoded-frame burst every second.
const STREAM_INTRA_FRAME_PERIOD_FRAMES: u32 = STREAM_CAPTURE_FPS * 2;
const STREAM_CAPTURE_FRAME_INTERVAL: Duration =
    Duration::from_nanos(1_000_000_000 / STREAM_CAPTURE_FPS as u64);
const STREAM_CAPTURE_STATS_INTERVAL: Duration = Duration::from_secs(5);
const STREAM_RECORDER_POLL_INTERVAL: Duration = Duration::from_millis(100);

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

enum CaptureSource {
    Display {
        recorder: VideoRecorder,
        frames: Receiver<Frame>,
    },
    Window(Window),
}

enum CaptureFrameOutcome {
    Queued,
    QueueFull,
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
        match self {
            Self::Display { frames, .. } => loop {
                match frames.recv_timeout(STREAM_RECORDER_POLL_INTERVAL) {
                    Ok(mut frame) => {
                        while let Ok(newer_frame) = frames.try_recv() {
                            frame = newer_frame;
                        }
                        return RgbaImage::from_raw(frame.width, frame.height, frame.raw)
                            .map(Some)
                            .ok_or_else(|| {
                                "display recorder returned an invalid RGBA frame".to_owned()
                            });
                    }
                    Err(RecvTimeoutError::Timeout) if stop.load(Ordering::Acquire) => {
                        return Ok(None);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err("display recorder stopped unexpectedly".to_owned());
                    }
                }
            },
            Self::Window(window) => window
                .capture_image()
                .map(Some)
                .map_err(|error| format!("window capture failed: {error}")),
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
        let scaled_width =
            ((source_width as f64 * scale).round() as u32).clamp(2, STREAM_CAPTURE_WIDTH) & !1;
        let scaled_height =
            ((source_height as f64 * scale).round() as u32).clamp(2, STREAM_CAPTURE_HEIGHT) & !1;

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

struct StreamFrameProcessor {
    resizer: Resizer,
    resize_options: ResizeOptions,
    resized: Image<'static>,
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
            resized: Image::new(2, 2, PixelType::U8x4),
            rgba,
            yuv: YuvPlanarImageMut::alloc(
                STREAM_CAPTURE_WIDTH,
                STREAM_CAPTURE_HEIGHT,
                YuvChromaSubsampling::Yuv420,
            ),
            geometry: None,
        }
    }

    fn prepare(&mut self, image: RgbaImage) -> Result<FramePreparationTimings, String> {
        let resize_started_at = Instant::now();
        let geometry = StreamFrameGeometry::for_source(image.dimensions())?;
        self.update_geometry(geometry);

        let source = ImageRef::new(
            geometry.source_dimensions.0,
            geometry.source_dimensions.1,
            image.as_raw(),
            PixelType::U8x4,
        )
        .map_err(|error| format!("captured RGBA frame is invalid: {error}"))?;

        if geometry.scaled_dimensions == (STREAM_CAPTURE_WIDTH, STREAM_CAPTURE_HEIGHT) {
            self.resizer
                .resize(&source, &mut self.rgba, &self.resize_options)
                .map_err(|error| format!("stream frame resize failed: {error}"))?;
        } else {
            self.resizer
                .resize(&source, &mut self.resized, &self.resize_options)
                .map_err(|error| format!("stream frame resize failed: {error}"))?;
            copy_resized_frame(
                self.resized.buffer(),
                geometry.scaled_dimensions,
                self.rgba.buffer_mut(),
                geometry.offsets,
            );
        }
        let resize = resize_started_at.elapsed();

        let color_convert_started_at = Instant::now();
        // The previous OpenH264 conversion also used low-precision BT.601
        // limited-range math. Fast mode preserves that behavior and selects
        // SIMD instructions at runtime on supported x86 and Arm processors.
        rgba_to_yuv420(
            &mut self.yuv,
            self.rgba.buffer(),
            STREAM_CAPTURE_WIDTH * 4,
            YuvRange::Limited,
            YuvStandardMatrix::Bt601,
            YuvConversionMode::Fast,
        )
        .map_err(|error| format!("RGBA to YUV conversion failed: {error}"))?;

        Ok(FramePreparationTimings {
            resize,
            color_convert: color_convert_started_at.elapsed(),
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

        if geometry.scaled_dimensions != (STREAM_CAPTURE_WIDTH, STREAM_CAPTURE_HEIGHT) {
            self.resized = Image::new(
                geometry.scaled_dimensions.0,
                geometry.scaled_dimensions.1,
                PixelType::U8x4,
            );
            fill_opaque_black(self.rgba.buffer_mut());
        }
        self.geometry = Some(geometry);
    }
}

fn fill_opaque_black(rgba: &mut [u8]) {
    rgba.fill(0);
    for alpha in rgba.iter_mut().skip(3).step_by(4) {
        *alpha = 255;
    }
}

fn copy_resized_frame(
    source: &[u8],
    source_dimensions: (u32, u32),
    destination: &mut [u8],
    offsets: (u32, u32),
) {
    let source_stride = source_dimensions.0 as usize * 4;
    let destination_stride = STREAM_CAPTURE_WIDTH as usize * 4;
    let destination_x = offsets.0 as usize * 4;
    let destination_y = offsets.1 as usize;

    for (row, source_row) in source
        .chunks_exact(source_stride)
        .take(source_dimensions.1 as usize)
        .enumerate()
    {
        let start = (destination_y + row) * destination_stride + destination_x;
        destination[start..start + source_stride].copy_from_slice(source_row);
    }
}

impl Drop for CaptureSource {
    fn drop(&mut self) {
        if let Self::Display { recorder, .. } = self
            && let Err(error) = recorder.stop()
        {
            logging::debug(
                "stream",
                format!("display recorder stop failed during shutdown: {error}"),
            );
        }
    }
}

impl Drop for StreamCaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take()
            && let Err(error) = worker.join()
        {
            logging::debug(
                "stream",
                format!("stream capture worker panicked during shutdown: {error:?}"),
            );
        }
    }
}

impl StreamCaptureHandle {
    pub(super) fn request_keyframe(&self) {
        self.force_keyframe.store(true, Ordering::Release);
    }
}

pub(crate) fn list_stream_capture_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    let mut targets = Vec::new();

    let monitors = Monitor::all().map_err(|error| format!("screen enumeration failed: {error}"))?;
    for (index, monitor) in monitors.into_iter().enumerate() {
        let id = monitor
            .id()
            .map_err(|error| format!("screen id lookup failed: {error}"))?;
        let name = monitor
            .friendly_name()
            .or_else(|_| monitor.name())
            .unwrap_or_else(|_| format!("Display {}", index + 1));
        let dimensions = monitor
            .width()
            .and_then(|width| monitor.height().map(|height| (width, height)))
            .ok();
        let title = match dimensions {
            Some((width, height)) => format!("{name} ({width}x{height})"),
            None => name,
        };
        targets.push(StreamCaptureTarget {
            kind: StreamCaptureTargetKind::Display,
            id,
            title: format!("Screen: {title}"),
        });
    }

    let windows = Window::all().map_err(|error| format!("window enumeration failed: {error}"))?;
    for window in windows {
        if window.is_minimized().unwrap_or(true) {
            continue;
        }
        let width = window.width().unwrap_or_default();
        let height = window.height().unwrap_or_default();
        if width < 2 || height < 2 {
            continue;
        }
        let title = window.title().unwrap_or_default().trim().to_owned();
        if title.is_empty() {
            continue;
        }
        let app_name = window.app_name().unwrap_or_default();
        let label = if app_name.is_empty() || title.starts_with(&app_name) {
            title
        } else {
            format!("{app_name}: {title}")
        };
        let id = window
            .id()
            .map_err(|error| format!("window id lookup failed: {error}"))?;
        targets.push(StreamCaptureTarget {
            kind: StreamCaptureTargetKind::Window,
            id,
            title: format!("Window: {label}"),
        });
    }

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

pub(super) fn start_stream_capture(
    target: StreamCaptureTarget,
    frames_tx: mpsc::Sender<Result<EncodedStreamFrame, String>>,
) -> Result<StreamCaptureHandle, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let force_keyframe = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker_force_keyframe = Arc::clone(&force_keyframe);
    let worker = thread::Builder::new()
        .name("stream-capture".to_owned())
        .spawn(move || {
            run_capture_worker(target, frames_tx, worker_stop, worker_force_keyframe);
        })
        .map_err(|error| format!("stream capture worker spawn failed: {error}"))?;
    Ok(StreamCaptureHandle {
        stop,
        force_keyframe,
        worker: Some(worker),
    })
}

fn run_capture_worker(
    target: StreamCaptureTarget,
    frames_tx: mpsc::Sender<Result<EncodedStreamFrame, String>>,
    stop: Arc<AtomicBool>,
    force_keyframe: Arc<AtomicBool>,
) {
    if let Err(error) = run_capture_loop(&target, &frames_tx, &stop, &force_keyframe) {
        let _ = frames_tx.blocking_send(Err(error));
    }
}

fn run_capture_loop(
    target: &StreamCaptureTarget,
    frames_tx: &mpsc::Sender<Result<EncodedStreamFrame, String>>,
    stop: &AtomicBool,
    force_keyframe: &AtomicBool,
) -> Result<(), String> {
    let source = resolve_capture_source(target)?;
    let mut encoder = Encoder::with_api_config(OpenH264API::from_source(), stream_encoder_config())
        .map_err(|error| format!("H264 encoder creation failed: {error}"))?;
    let started_at = Instant::now();
    let mut stats = CapturePerformanceStats::new();
    let mut frame_processor = StreamFrameProcessor::new();

    while !stop.load(Ordering::Acquire) {
        let frame_started_at = Instant::now();

        let capture_started_at = Instant::now();
        let Some(image) = source.capture_image(stop)? else {
            return Ok(());
        };
        let capture_time = capture_started_at.elapsed();

        let prepare_started_at = Instant::now();
        let preparation_timings = frame_processor.prepare(image)?;
        let prepare_time = prepare_started_at.elapsed();

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
            match frames_tx.try_send(Ok(EncodedStreamFrame {
                timestamp,
                annex_b,
                is_keyframe,
            })) {
                Ok(()) => CaptureFrameOutcome::Queued,
                Err(mpsc::error::TrySendError::Full(_)) => CaptureFrameOutcome::QueueFull,
                Err(mpsc::error::TrySendError::Closed(_)) => return Ok(()),
            }
        };
        stats.record_frame(
            outcome,
            encoded_bytes,
            CaptureFrameTimings {
                capture: capture_time,
                prepare: prepare_time,
                resize: preparation_timings.resize,
                color_convert: preparation_timings.color_convert,
                encode: encode_time,
                total: frame_started_at.elapsed(),
            },
            &target.title,
        );

        if let Some(remaining) =
            STREAM_CAPTURE_FRAME_INTERVAL.checked_sub(frame_started_at.elapsed())
        {
            thread::sleep(remaining);
        }
    }
    Ok(())
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
        .vui(VuiConfig::bt709_full())
}

fn resolve_capture_source(target: &StreamCaptureTarget) -> Result<CaptureSource, String> {
    match target.kind {
        StreamCaptureTargetKind::Display => Monitor::all()
            .map_err(|error| format!("screen enumeration failed: {error}"))?
            .into_iter()
            .find(|monitor| monitor.id().ok() == Some(target.id))
            .ok_or_else(|| format!("screen is no longer available: {}", target.title))
            .and_then(start_display_recorder),
        StreamCaptureTargetKind::Window => Window::all()
            .map_err(|error| format!("window enumeration failed: {error}"))?
            .into_iter()
            .find(|window| window.id().ok() == Some(target.id))
            .map(CaptureSource::Window)
            .ok_or_else(|| format!("window is no longer available: {}", target.title)),
    }
}

fn start_display_recorder(monitor: Monitor) -> Result<CaptureSource, String> {
    let (recorder, frames) = monitor
        .video_recorder()
        .map_err(|error| format!("display recorder creation failed: {error}"))?;
    recorder
        .start()
        .map_err(|error| format!("display recorder start failed: {error}"))?;
    logging::debug("stream", "continuous display recorder started");
    Ok(CaptureSource::Display { recorder, frames })
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
                .prepare(image)
                .expect("stream frame should be prepared");

            assert_eq!(
                rgba_pixel(processor.rgba.buffer(), 640, 360),
                [255, 0, 0, 255]
            );
            assert_eq!(
                rgba_pixel(processor.rgba.buffer(), content_edge.0, content_edge.1),
                [255, 0, 0, 255]
            );
            if let Some(black_bar) = black_bar {
                assert_eq!(
                    rgba_pixel(processor.rgba.buffer(), black_bar.0, black_bar.1),
                    [0, 0, 0, 255]
                );
            }
        }
    }

    #[test]
    fn simd_color_conversion_preserves_bt601_limited_output() {
        let mut output = YuvPlanarImageMut::alloc(2, 2, YuvChromaSubsampling::Yuv420);
        rgba_to_yuv420(
            &mut output,
            &[255, 0, 0, 255].repeat(4),
            8,
            YuvRange::Limited,
            YuvStandardMatrix::Bt601,
            YuvConversionMode::Fast,
        )
        .expect("RGBA frame should convert to YUV");

        for value in output.y_plane.borrow() {
            assert!(value.abs_diff(81) <= 1);
        }
        assert!(output.u_plane.borrow()[0].abs_diff(90) <= 1);
        assert!(output.v_plane.borrow()[0].abs_diff(239) <= 1);
    }

    #[test]
    fn frame_processor_reuses_working_buffers_for_stable_dimensions() {
        let mut processor = StreamFrameProcessor::new();
        let image = RgbaImage::from_pixel(800, 600, Rgba([12, 34, 56, 255]));
        processor
            .prepare(image.clone())
            .expect("first stream frame should be prepared");
        let addresses = (
            processor.resized.buffer().as_ptr(),
            processor.rgba.buffer().as_ptr(),
            processor.yuv.y_plane.borrow().as_ptr(),
            processor.yuv.u_plane.borrow().as_ptr(),
            processor.yuv.v_plane.borrow().as_ptr(),
        );

        processor
            .prepare(image)
            .expect("second stream frame should be prepared");

        assert_eq!(
            addresses,
            (
                processor.resized.buffer().as_ptr(),
                processor.rgba.buffer().as_ptr(),
                processor.yuv.y_plane.borrow().as_ptr(),
                processor.yuv.u_plane.borrow().as_ptr(),
                processor.yuv.v_plane.borrow().as_ptr(),
            )
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

    fn rgba_pixel(buffer: &[u8], x: u32, y: u32) -> [u8; 4] {
        let start = ((y * STREAM_CAPTURE_WIDTH + x) * 4) as usize;
        buffer[start..start + 4]
            .try_into()
            .expect("RGBA pixel has four channels")
    }
}
