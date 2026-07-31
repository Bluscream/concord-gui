use std::fmt;
use std::time::Instant;

#[cfg(feature = "stream-broadcast")]
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
#[cfg(feature = "stream-broadcast")]
use std::thread::{self, JoinHandle};
#[cfg(feature = "stream-broadcast")]
use std::time::Duration;

#[cfg(feature = "stream-broadcast")]
use flexaudio_core::backend::{CaptureBackend, RawSink};
#[cfg(feature = "stream-broadcast")]
use flexaudio_core::normalizer::Normalizer;
#[cfg(feature = "stream-broadcast")]
use flexaudio_core::raw_ring::{RawConsumer, raw_ring};
#[cfg(feature = "stream-broadcast")]
use flexaudio_core::types::{Error as FlexAudioError, OutputFormat, ProcessMode};
use tokio::sync::mpsc;
#[cfg(feature = "stream-broadcast")]
use xcap::Window;

#[cfg(feature = "stream-broadcast")]
use crate::logging;
#[cfg(feature = "stream-broadcast")]
use crate::support::audio_output;

use super::StreamCaptureTarget;
#[cfg(feature = "stream-broadcast")]
use super::{DISCORD_OPUS_20MS_STEREO_SAMPLES, StreamCaptureTargetKind};

#[cfg(all(feature = "stream-broadcast", target_os = "linux"))]
#[path = "system_audio/linux.rs"]
mod platform;
#[cfg(all(feature = "stream-broadcast", target_os = "macos"))]
#[path = "system_audio/macos.rs"]
mod platform;
#[cfg(all(feature = "stream-broadcast", target_os = "windows"))]
#[path = "system_audio/windows.rs"]
mod platform;

pub(super) const SYSTEM_AUDIO_FRAME_QUEUE: usize = 8;

#[cfg(feature = "stream-broadcast")]
const SYSTEM_AUDIO_RAW_RING_SAMPLES: usize = 48_000;
#[cfg(feature = "stream-broadcast")]
const SYSTEM_AUDIO_POLL_INTERVAL: Duration = Duration::from_millis(2);

#[derive(Debug)]
pub(super) struct SystemAudioFrame {
    pub(super) samples: Vec<i16>,
    pub(super) captured_at: Instant,
}

pub(super) struct SystemAudioCapture {
    #[cfg(feature = "stream-broadcast")]
    stop: Arc<AtomicBool>,
    #[cfg(feature = "stream-broadcast")]
    worker: Option<JoinHandle<()>>,
    #[cfg(feature = "stream-broadcast")]
    stats: Arc<SystemAudioCaptureStats>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct SystemAudioCaptureError {
    message: String,
}

impl SystemAudioCaptureError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SystemAudioCaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[cfg(feature = "stream-broadcast")]
#[derive(Default)]
struct SystemAudioCaptureStats {
    source_samples: AtomicU64,
    queued_frames: AtomicU64,
    queue_dropped_frames: AtomicU64,
    raw_dropped_samples: AtomicU64,
}

#[cfg(feature = "stream-broadcast")]
impl SystemAudioCapture {
    pub(super) fn dropped_frames(&self) -> u64 {
        self.stats
            .queue_dropped_frames
            .load(Ordering::Relaxed)
            .saturating_add(
                self.stats.raw_dropped_samples.load(Ordering::Relaxed)
                    / DISCORD_OPUS_20MS_STEREO_SAMPLES as u64,
            )
    }
}

#[cfg(not(feature = "stream-broadcast"))]
impl SystemAudioCapture {
    pub(super) fn dropped_frames(&self) -> u64 {
        0
    }
}

#[cfg(all(
    feature = "stream-broadcast",
    any(target_os = "linux", target_os = "macos", target_os = "windows")
))]
pub(super) fn start_system_audio_capture(
    target: &StreamCaptureTarget,
    frames_tx: mpsc::Sender<SystemAudioFrame>,
) -> Result<SystemAudioCapture, SystemAudioCaptureError> {
    let scope = resolve_audio_scope(target)?;
    let mut backend = platform::capture_backend(target, scope.target_pid, scope.mode);
    let (sample_rate, channels) = backend.native_format();
    if sample_rate == 0 || channels == 0 {
        return Err(SystemAudioCaptureError::new(format!(
            "{} system audio backend returned an invalid format: sample_rate={sample_rate} channels={channels}",
            platform::BACKEND_NAME,
        )));
    }

    let normalizer = Normalizer::new(sample_rate, channels, OutputFormat::default())
        .map_err(system_audio_backend_error)?;
    let (raw_tx, raw_rx) = raw_ring(SYSTEM_AUDIO_RAW_RING_SAMPLES);
    backend
        .start(RawSink::new(raw_tx, sample_rate, channels))
        .map_err(system_audio_backend_error)?;

    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(SystemAudioCaptureStats::default());
    let worker_stop = Arc::clone(&stop);
    let worker_stats = Arc::clone(&stats);
    let scope_description = scope.description;
    let worker = thread::Builder::new()
        .name("stream-system-audio".to_owned())
        .spawn(move || {
            run_system_audio_capture(
                backend,
                raw_rx,
                normalizer,
                frames_tx,
                worker_stop,
                worker_stats,
            );
        })
        .map_err(|error| {
            SystemAudioCaptureError::new(format!("system audio worker spawn failed: {error}"))
        })?;

    logging::debug(
        "stream",
        format!(
            "system audio capture started: backend={} scope={scope_description} sample_rate={sample_rate} channels={channels}",
            platform::BACKEND_NAME,
        ),
    );
    Ok(SystemAudioCapture {
        stop,
        worker: Some(worker),
        stats,
    })
}

#[cfg(all(
    feature = "stream-broadcast",
    not(any(target_os = "linux", target_os = "macos", target_os = "windows"))
))]
pub(super) fn start_system_audio_capture(
    _target: &StreamCaptureTarget,
    frames_tx: mpsc::Sender<SystemAudioFrame>,
) -> Result<SystemAudioCapture, SystemAudioCaptureError> {
    drop(frames_tx);
    Err(SystemAudioCaptureError::new(
        "system audio capture is unsupported on this operating system",
    ))
}

#[cfg(not(feature = "stream-broadcast"))]
pub(super) fn start_system_audio_capture(
    _target: &StreamCaptureTarget,
    frames_tx: mpsc::Sender<SystemAudioFrame>,
) -> Result<SystemAudioCapture, SystemAudioCaptureError> {
    drop(frames_tx);
    Err(SystemAudioCaptureError::new(
        "system audio capture requires the stream-broadcast feature",
    ))
}

#[cfg(feature = "stream-broadcast")]
struct SystemAudioScope {
    target_pid: u32,
    mode: ProcessMode,
    description: String,
}

#[cfg(feature = "stream-broadcast")]
fn resolve_audio_scope(
    target: &StreamCaptureTarget,
) -> Result<SystemAudioScope, SystemAudioCaptureError> {
    match target.kind {
        StreamCaptureTargetKind::Display => Ok(SystemAudioScope {
            target_pid: std::process::id(),
            mode: ProcessMode::Exclude,
            description: format!("display excluding concord pid={}", std::process::id()),
        }),
        StreamCaptureTargetKind::Window => {
            let window = Window::all()
                .map_err(|error| {
                    SystemAudioCaptureError::new(format!(
                        "window enumeration for audio capture failed: {error}"
                    ))
                })?
                .into_iter()
                .find(|window| window.id().ok() == Some(target.id))
                .ok_or_else(|| {
                    SystemAudioCaptureError::new(format!(
                        "window is no longer available for audio capture: {}",
                        target.title
                    ))
                })?;
            let target_pid = window.pid().map_err(|error| {
                SystemAudioCaptureError::new(format!(
                    "window process lookup for audio capture failed: {error}"
                ))
            })?;
            if target_pid == std::process::id() {
                return Err(SystemAudioCaptureError::new(
                    "Concord cannot broadcast its own window audio",
                ));
            }
            Ok(SystemAudioScope {
                target_pid,
                mode: ProcessMode::Include,
                description: format!("window pid={target_pid}"),
            })
        }
    }
}

#[cfg(feature = "stream-broadcast")]
fn run_system_audio_capture(
    mut backend: Box<dyn CaptureBackend>,
    mut raw_rx: RawConsumer,
    mut normalizer: Normalizer,
    frames_tx: mpsc::Sender<SystemAudioFrame>,
    stop: Arc<AtomicBool>,
    stats: Arc<SystemAudioCaptureStats>,
) {
    let channels = usize::from(backend.native_format().1.max(1));
    let mut raw_samples = vec![0.0_f32; 4_096 * channels];
    let capture_started = Instant::now();

    while !stop.load(Ordering::Acquire) {
        let sample_count = raw_rx.pop_slice(&mut raw_samples);
        stats
            .raw_dropped_samples
            .store(raw_rx.overflow_count(), Ordering::Relaxed);
        if sample_count == 0 {
            thread::park_timeout(SYSTEM_AUDIO_POLL_INTERVAL);
            continue;
        }

        stats
            .source_samples
            .fetch_add(sample_count as u64, Ordering::Relaxed);
        let captured_ns = capture_started.elapsed().as_nanos().min(i64::MAX as u128) as i64;
        if let Err(error) = normalizer.push(&raw_samples[..sample_count], captured_ns) {
            logging::error(
                "stream",
                format!("system audio normalization failed: {error}"),
            );
            break;
        }

        while let Some((samples, _pts_ns)) = normalizer.pop_chunk() {
            let frame = SystemAudioFrame {
                samples: samples
                    .into_iter()
                    .map(audio_output::f32_sample_to_i16)
                    .collect(),
                captured_at: Instant::now(),
            };
            match frames_tx.try_send(frame) {
                Ok(()) => {
                    stats.queued_frames.fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    stats.queue_dropped_frames.fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    stop.store(true, Ordering::Release);
                    break;
                }
            }
        }
    }

    backend.stop();
}

#[cfg(feature = "stream-broadcast")]
fn system_audio_backend_error(error: FlexAudioError) -> SystemAudioCaptureError {
    let context = match error {
        FlexAudioError::PermissionDenied => "system audio permission denied".to_owned(),
        FlexAudioError::DeviceNotFound => "system audio process is not available".to_owned(),
        FlexAudioError::UnsupportedOsVersion => {
            "system audio capture is unsupported on this OS version".to_owned()
        }
        FlexAudioError::Unsupported => {
            "system audio capture is unsupported on this operating system".to_owned()
        }
        error => format!("system audio backend failed: {error}"),
    };
    SystemAudioCaptureError::new(context)
}

#[cfg(feature = "stream-broadcast")]
impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take()
            && let Err(error) = worker.join()
        {
            logging::debug(
                "stream",
                format!("system audio worker panicked during shutdown: {error:?}"),
            );
        }
        logging::debug(
            "stream",
            format!(
                "system audio capture stopped: source_samples={} queued_20ms_frames={} dropped_20ms_frames={} raw_dropped_samples={}",
                self.stats.source_samples.load(Ordering::Relaxed),
                self.stats.queued_frames.load(Ordering::Relaxed),
                self.stats.queue_dropped_frames.load(Ordering::Relaxed),
                self.stats.raw_dropped_samples.load(Ordering::Relaxed),
            ),
        );
    }
}
