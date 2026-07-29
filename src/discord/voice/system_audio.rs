use std::time::Instant;

#[cfg(feature = "voice-playback")]
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[cfg(feature = "voice-playback")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::sync::mpsc;

#[cfg(feature = "voice-playback")]
use super::microphone::{
    voice_input_buffer_size, voice_input_f32_to_stereo_i16, voice_input_i16_to_stereo_i16,
    voice_input_u8_to_stereo_i16, voice_input_u16_to_stereo_i16,
};
#[cfg(any(test, feature = "voice-playback"))]
use super::{DISCORD_OPUS_20MS_STEREO_SAMPLES, DISCORD_VOICE_CHANNELS, DISCORD_VOICE_SAMPLE_RATE};
#[cfg(feature = "voice-playback")]
use crate::logging;

pub(super) const SYSTEM_AUDIO_FRAME_QUEUE: usize = 8;

#[derive(Debug)]
pub(super) struct SystemAudioFrame {
    pub(super) samples: Vec<i16>,
    pub(super) captured_at: Instant,
}

pub(super) struct SystemAudioCapture {
    #[cfg(feature = "voice-playback")]
    _stream: cpal::Stream,
    #[cfg(feature = "voice-playback")]
    stats: Arc<SystemAudioCaptureStats>,
}

#[cfg(feature = "voice-playback")]
impl SystemAudioCapture {
    pub(super) fn dropped_frames(&self) -> u64 {
        self.stats.dropped_frames.load(Ordering::Relaxed)
    }
}

#[cfg(not(feature = "voice-playback"))]
impl SystemAudioCapture {
    pub(super) fn dropped_frames(&self) -> u64 {
        0
    }
}

#[cfg(any(test, feature = "voice-playback"))]
struct SystemAudioPcmFrames {
    source_sample_rate: u32,
    source_pending: Vec<i16>,
    output_pending: Vec<i16>,
    next_source_frame: f64,
}

#[cfg(feature = "voice-playback")]
#[derive(Default)]
struct SystemAudioCaptureStats {
    callbacks: AtomicU64,
    source_frames: AtomicU64,
    queued_frames: AtomicU64,
    dropped_frames: AtomicU64,
}

#[cfg(any(test, feature = "voice-playback"))]
impl SystemAudioPcmFrames {
    fn new(source_sample_rate: u32) -> Self {
        Self {
            source_sample_rate,
            source_pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES),
            output_pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES),
            next_source_frame: 0.0,
        }
    }

    fn push_stereo_samples(&mut self, samples: &[i16], mut emit: impl FnMut(SystemAudioFrame)) {
        if self.source_sample_rate == DISCORD_VOICE_SAMPLE_RATE {
            self.output_pending.extend_from_slice(samples);
        } else {
            self.source_pending.extend_from_slice(samples);
            self.resample_pending_source();
        }

        while self.output_pending.len() >= DISCORD_OPUS_20MS_STEREO_SAMPLES {
            emit(SystemAudioFrame {
                samples: self
                    .output_pending
                    .drain(..DISCORD_OPUS_20MS_STEREO_SAMPLES)
                    .collect(),
                captured_at: Instant::now(),
            });
        }
    }

    fn resample_pending_source(&mut self) {
        let channels = usize::from(DISCORD_VOICE_CHANNELS);
        let source_frames = self.source_pending.len() / channels;
        if source_frames < 2 {
            return;
        }

        let source_step = f64::from(self.source_sample_rate) / f64::from(DISCORD_VOICE_SAMPLE_RATE);
        while self.next_source_frame + 1.0 < source_frames as f64 {
            let frame_index = self.next_source_frame.floor() as usize;
            let fraction = self.next_source_frame - frame_index as f64;
            let current = frame_index * channels;
            let next = (frame_index + 1) * channels;
            self.output_pending.push(interpolate_i16(
                self.source_pending[current],
                self.source_pending[next],
                fraction,
            ));
            self.output_pending.push(interpolate_i16(
                self.source_pending[current + 1],
                self.source_pending[next + 1],
                fraction,
            ));
            self.next_source_frame += source_step;
        }

        let consumed_frames = self.next_source_frame.floor() as usize;
        if consumed_frames > 0 {
            self.source_pending.drain(..consumed_frames * channels);
            self.next_source_frame -= consumed_frames as f64;
        }
    }
}

#[cfg(any(test, feature = "voice-playback"))]
fn interpolate_i16(current: i16, next: i16, fraction: f64) -> i16 {
    let value = f64::from(current) + (f64::from(next) - f64::from(current)) * fraction;
    value
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

#[cfg(feature = "voice-playback")]
pub(super) fn start_system_audio_capture(
    frames_tx: mpsc::Sender<SystemAudioFrame>,
) -> Result<SystemAudioCapture, String> {
    let (host, device, capture_output_device) = system_audio_device()?;
    let description = device
        .description()
        .map(|description| description.name().to_owned())
        .unwrap_or_else(|_| "unknown".to_owned());
    let supported_config = if capture_output_device {
        device
            .default_output_config()
            .map_err(|error| format!("system audio output config failed: {error}"))?
    } else {
        device
            .default_input_config()
            .map_err(|error| format!("system audio monitor config failed: {error}"))?
    };
    let sample_format = supported_config.sample_format();
    let mut stream_config = supported_config.config();
    stream_config.buffer_size = voice_input_buffer_size(supported_config.buffer_size());
    let stats = Arc::new(SystemAudioCaptureStats::default());
    let stream = build_system_audio_stream(
        &device,
        &stream_config,
        sample_format,
        Arc::clone(&stats),
        frames_tx.clone(),
    )
    .or_else(|preferred_error| {
        logging::debug(
            "stream",
            format!(
                "preferred system audio buffer failed, retrying default buffer: {preferred_error}"
            ),
        );
        stream_config.buffer_size = cpal::BufferSize::Default;
        build_system_audio_stream(
            &device,
            &stream_config,
            sample_format,
            Arc::clone(&stats),
            frames_tx,
        )
    })?;
    stream
        .play()
        .map_err(|error| format!("system audio capture start failed: {error}"))?;
    logging::debug(
        "stream",
        format!(
            "system audio capture started: host={} device={description} sample_rate={} channels={} format={sample_format:?} buffer_size={:?}",
            host.id(),
            stream_config.sample_rate,
            stream_config.channels,
            stream_config.buffer_size,
        ),
    );
    Ok(SystemAudioCapture {
        _stream: stream,
        stats,
    })
}

#[cfg(not(feature = "voice-playback"))]
pub(super) fn start_system_audio_capture(
    frames_tx: mpsc::Sender<SystemAudioFrame>,
) -> Result<SystemAudioCapture, String> {
    drop(frames_tx);
    Err("system audio capture requires the voice-playback feature".to_owned())
}

#[cfg(all(feature = "voice-playback", target_os = "linux"))]
fn system_audio_device() -> Result<(cpal::Host, cpal::Device, bool), String> {
    let host = cpal::host_from_id(cpal::HostId::PulseAudio)
        .map_err(|error| format!("PulseAudio system audio host unavailable: {error}"))?;
    let default_output = host
        .default_output_device()
        .ok_or_else(|| "no default system audio output device is available".to_owned())?;
    let sink_id = default_output
        .id()
        .map_err(|error| format!("default system audio output id failed: {error}"))?;
    let inputs = host
        .input_devices()
        .map_err(|error| format!("system audio monitor enumeration failed: {error}"))?;
    for device in inputs {
        let Ok(id) = device.id() else {
            continue;
        };
        if is_pulse_monitor_for_sink(sink_id.id(), id.id()) {
            return Ok((host, device, false));
        }
    }
    Err(format!(
        "no PulseAudio monitor source is available for output {}",
        sink_id.id()
    ))
}

#[cfg(any(test, all(feature = "voice-playback", target_os = "linux")))]
fn is_pulse_monitor_for_sink(sink_id: &str, input_id: &str) -> bool {
    input_id
        .strip_suffix(".monitor")
        .is_some_and(|source_id| source_id == sink_id)
}

#[cfg(all(feature = "voice-playback", not(target_os = "linux")))]
fn system_audio_device() -> Result<(cpal::Host, cpal::Device, bool), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default system audio output device is available".to_owned())?;

    // CPAL currently treats a duplex CoreAudio device as a normal input. Do
    // not risk broadcasting a microphone when the selected output is duplex.
    #[cfg(target_os = "macos")]
    if device.supports_input() {
        return Err(
            "the default macOS output is duplex, so CPAL cannot safely select system loopback"
                .to_owned(),
        );
    }

    Ok((host, device, true))
}

#[cfg(feature = "voice-playback")]
fn build_system_audio_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    stats: Arc<SystemAudioCaptureStats>,
    frames_tx: mpsc::Sender<SystemAudioFrame>,
) -> Result<cpal::Stream, String> {
    match sample_format {
        cpal::SampleFormat::F32 => build_system_audio_stream_for::<f32>(
            device,
            config,
            stats,
            frames_tx,
            voice_input_f32_to_stereo_i16,
        ),
        cpal::SampleFormat::I16 => build_system_audio_stream_for::<i16>(
            device,
            config,
            stats,
            frames_tx,
            voice_input_i16_to_stereo_i16,
        ),
        cpal::SampleFormat::U16 => build_system_audio_stream_for::<u16>(
            device,
            config,
            stats,
            frames_tx,
            voice_input_u16_to_stereo_i16,
        ),
        cpal::SampleFormat::U8 => build_system_audio_stream_for::<u8>(
            device,
            config,
            stats,
            frames_tx,
            voice_input_u8_to_stereo_i16,
        ),
        other => Err(format!(
            "unsupported system audio input sample format: {other:?}"
        )),
    }
}

#[cfg(feature = "voice-playback")]
fn build_system_audio_stream_for<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<SystemAudioCaptureStats>,
    frames_tx: mpsc::Sender<SystemAudioFrame>,
    convert: fn(&[T], usize) -> Vec<i16>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + 'static,
{
    let channels = usize::from(config.channels);
    let mut pcm_frames = SystemAudioPcmFrames::new(config.sample_rate);
    device
        .build_input_stream(
            *config,
            move |input: &[T], _| {
                stats.callbacks.fetch_add(1, Ordering::Relaxed);
                stats.source_frames.fetch_add(
                    u64::try_from(input.len() / channels.max(1)).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
                let samples = convert(input, channels);
                pcm_frames.push_stereo_samples(&samples, |frame| {
                    if frames_tx.try_send(frame).is_ok() {
                        stats.queued_frames.fetch_add(1, Ordering::Relaxed);
                    } else {
                        stats.dropped_frames.fetch_add(1, Ordering::Relaxed);
                    }
                });
            },
            log_system_audio_stream_error,
            None,
        )
        .map_err(|error| format!("system audio input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn log_system_audio_stream_error(error: cpal::Error) {
    logging::error("stream", format!("system audio capture failed: {error}"));
}

#[cfg(feature = "voice-playback")]
impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        logging::debug(
            "stream",
            format!(
                "system audio capture stopped: callbacks={} source_frames={} queued_20ms_frames={} dropped_20ms_frames={}",
                self.stats.callbacks.load(Ordering::Relaxed),
                self.stats.source_frames.load(Ordering::Relaxed),
                self.stats.queued_frames.load(Ordering::Relaxed),
                self.stats.dropped_frames.load(Ordering::Relaxed),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_monitor_selection_accepts_only_the_default_output() {
        assert!(is_pulse_monitor_for_sink(
            "alsa_output.default",
            "alsa_output.default.monitor"
        ));
        assert!(!is_pulse_monitor_for_sink(
            "alsa_output.default",
            "alsa_output.unrelated.monitor"
        ));
        assert!(!is_pulse_monitor_for_sink(
            "alsa_output.default",
            "alsa_output.default"
        ));
    }

    #[test]
    fn pcm_capture_emits_normalized_20ms_stereo_frames() {
        let mut native = SystemAudioPcmFrames::new(DISCORD_VOICE_SAMPLE_RATE);
        let mut native_frames = Vec::new();
        native.push_stereo_samples(&vec![7; DISCORD_OPUS_20MS_STEREO_SAMPLES * 2], |frame| {
            native_frames.push(frame)
        });
        assert_eq!(native_frames.len(), 2);
        assert!(
            native_frames
                .iter()
                .all(|frame| frame.samples.len() == DISCORD_OPUS_20MS_STEREO_SAMPLES)
        );

        let mut resampled = SystemAudioPcmFrames::new(44_100);
        let mut source = Vec::with_capacity(883 * usize::from(DISCORD_VOICE_CHANNELS));
        for index in 0..883 {
            source.push(index as i16);
            source.push(-(index as i16));
        }
        let mut resampled_frames = Vec::new();
        resampled.push_stereo_samples(&source, |frame| resampled_frames.push(frame));
        assert_eq!(resampled_frames.len(), 1);
        assert_eq!(
            resampled_frames[0].samples.len(),
            DISCORD_OPUS_20MS_STEREO_SAMPLES
        );
        assert_eq!(
            resampled_frames[0].samples[0],
            -resampled_frames[0].samples[1]
        );
    }
}
