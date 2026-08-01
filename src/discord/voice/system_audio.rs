use std::fmt;
use std::time::Instant;

#[cfg(feature = "stream-broadcast")]
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use tokio::sync::mpsc;

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
pub(super) const SYSTEM_AUDIO_SAMPLE_RATE: u32 = 48_000;
#[cfg(feature = "stream-broadcast")]
pub(super) const SYSTEM_AUDIO_CHANNELS: u16 = 2;

#[derive(Debug)]
pub(super) struct SystemAudioFrame {
    pub(super) samples: Vec<i16>,
    pub(super) captured_at: Instant,
}

pub(super) struct SystemAudioCapture {
    #[cfg(all(
        feature = "stream-broadcast",
        any(target_os = "linux", target_os = "macos", target_os = "windows")
    ))]
    session: platform::CaptureSession,
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
pub(super) struct SystemAudioCaptureStats {
    source_samples: AtomicU64,
    queued_frames: AtomicU64,
    queue_dropped_frames: AtomicU64,
}

#[cfg(feature = "stream-broadcast")]
pub(super) struct AudioFrameAssembler {
    pending: Vec<f32>,
    frames_tx: mpsc::Sender<SystemAudioFrame>,
    stats: Arc<SystemAudioCaptureStats>,
}

#[cfg(feature = "stream-broadcast")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AudioProcessMode {
    Include,
    Exclude,
}

#[cfg(feature = "stream-broadcast")]
impl AudioFrameAssembler {
    pub(super) fn new(
        frames_tx: mpsc::Sender<SystemAudioFrame>,
        stats: Arc<SystemAudioCaptureStats>,
    ) -> Self {
        Self {
            pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES * 2),
            frames_tx,
            stats,
        }
    }

    /// Accepts interleaved 48 kHz stereo float samples and emits exact 20 ms
    /// frames expected by the broadcast Opus encoder.
    pub(super) fn push(&mut self, samples: &[f32]) -> bool {
        self.stats
            .source_samples
            .fetch_add(samples.len() as u64, Ordering::Relaxed);
        self.pending.extend_from_slice(samples);

        let mut consumed = 0;
        while self.pending.len() - consumed >= DISCORD_OPUS_20MS_STEREO_SAMPLES {
            let end = consumed + DISCORD_OPUS_20MS_STEREO_SAMPLES;
            let frame = SystemAudioFrame {
                samples: self.pending[consumed..end]
                    .iter()
                    .copied()
                    .map(audio_output::f32_sample_to_i16)
                    .collect(),
                captured_at: Instant::now(),
            };
            consumed = end;

            match self.frames_tx.try_send(frame) {
                Ok(()) => {
                    self.stats.queued_frames.fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    self.stats
                        .queue_dropped_frames
                        .fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => return false,
            }
        }

        if consumed > 0 {
            let remaining = self.pending.len() - consumed;
            self.pending.copy_within(consumed.., 0);
            self.pending.truncate(remaining);
        }
        true
    }
}

#[cfg(feature = "stream-broadcast")]
impl SystemAudioCapture {
    pub(super) fn dropped_frames(&self) -> u64 {
        self.stats.queue_dropped_frames.load(Ordering::Relaxed)
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
    let stats = Arc::new(SystemAudioCaptureStats::default());
    let assembler = AudioFrameAssembler::new(frames_tx, Arc::clone(&stats));
    let session = platform::start_capture(target, scope.target_pid, scope.mode, assembler)
        .map_err(SystemAudioCaptureError::new)?;

    logging::debug(
        "stream",
        format!(
            "system audio capture started: backend={} scope={} sample_rate={} channels={}",
            platform::BACKEND_NAME,
            scope.description,
            SYSTEM_AUDIO_SAMPLE_RATE,
            SYSTEM_AUDIO_CHANNELS,
        ),
    );
    Ok(SystemAudioCapture { session, stats })
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
    mode: AudioProcessMode,
    description: String,
}

#[cfg(feature = "stream-broadcast")]
fn resolve_audio_scope(
    target: &StreamCaptureTarget,
) -> Result<SystemAudioScope, SystemAudioCaptureError> {
    match target.kind {
        StreamCaptureTargetKind::Display | StreamCaptureTargetKind::Portal => {
            Ok(SystemAudioScope {
                target_pid: std::process::id(),
                mode: AudioProcessMode::Exclude,
                description: format!("display excluding concord pid={}", std::process::id()),
            })
        }
        StreamCaptureTargetKind::Window => {
            let Some(target_pid) =
                platform::target_process_id(target).map_err(SystemAudioCaptureError::new)?
            else {
                return Ok(SystemAudioScope {
                    target_pid: std::process::id(),
                    mode: AudioProcessMode::Exclude,
                    description: format!("window excluding concord pid={}", std::process::id()),
                });
            };
            if target_pid == std::process::id() {
                return Err(SystemAudioCaptureError::new(
                    "Concord cannot broadcast its own window audio",
                ));
            }
            Ok(SystemAudioScope {
                target_pid,
                mode: AudioProcessMode::Include,
                description: format!("window pid={target_pid}"),
            })
        }
    }
}

#[cfg(all(
    feature = "stream-broadcast",
    any(target_os = "linux", target_os = "macos", target_os = "windows")
))]
impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        if let Err(error) = self.session.stop() {
            logging::debug(
                "stream",
                format!("system audio capture stop failed: {error}"),
            );
        }
        logging::debug(
            "stream",
            format!(
                "system audio capture stopped: source_samples={} queued_20ms_frames={} dropped_20ms_frames={}",
                self.stats.source_samples.load(Ordering::Relaxed),
                self.stats.queued_frames.load(Ordering::Relaxed),
                self.stats.queue_dropped_frames.load(Ordering::Relaxed),
            ),
        );
    }
}
