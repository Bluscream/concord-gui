use super::super::devices;
use super::*;

#[cfg(feature = "voice-playback")]
impl VoiceMicrophoneCapture {
    pub fn start(
        samples_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
        input_source: Option<&str>,
    ) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let alsa_error_output = alsa::Output::local_error_handler().ok();

        let result = Self::start_with_cpal(samples_tx, input_source);

        #[cfg(target_os = "linux")]
        log_captured_alsa_errors(&alsa_error_output);

        result
    }

    pub(super) fn start_with_cpal(
        samples_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
        input_source: Option<&str>,
    ) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = devices::resolve_input_device(&host, input_source)?;
        let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
        let (stream, stream_config, sample_format) =
            build_preferred_voice_input_stream(&device, Arc::clone(&stats), samples_tx.clone())
                .or_else(|preferred_error| {
                    logging::debug(
                        "voice",
                        format!(
                            "voice preferred microphone input stream failed: {preferred_error}"
                        ),
                    );
                    build_default_voice_input_stream(&device, Arc::clone(&stats), samples_tx)
                })?;
        stream
            .play()
            .map_err(|error| format!("voice microphone input stream start failed: {error}"))?;
        logging::debug(
            "voice",
            format!(
                "voice microphone capture started: host={} sample_rate={} channels={} format={:?} buffer_size={:?}",
                host.id(),
                stream_config.sample_rate,
                stream_config.channels,
                sample_format,
                stream_config.buffer_size,
            ),
        );
        Ok(Self {
            _stream: stream,
            stats,
        })
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_preferred_voice_input_stream(
    device: &cpal::Device,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
) -> Result<(cpal::Stream, cpal::StreamConfig, cpal::SampleFormat), String> {
    let supported_config = select_voice_input_config(device)?;
    let sample_format = supported_config.sample_format();
    let mut stream_config = supported_config.config();
    stream_config.buffer_size = voice_input_buffer_size(supported_config.buffer_size());

    match build_voice_input_stream(
        device,
        &stream_config,
        sample_format,
        Arc::clone(&stats),
        samples_tx.clone(),
    ) {
        Ok(stream) => Ok((stream, stream_config, sample_format)),
        Err(error) if stream_config.buffer_size != cpal::BufferSize::Default => {
            logging::debug(
                "voice",
                format!(
                    "voice fixed microphone input buffer failed, retrying default buffer: {error}"
                ),
            );
            stream_config.buffer_size = cpal::BufferSize::Default;
            build_voice_input_stream(device, &stream_config, sample_format, stats, samples_tx)
                .map(|stream| (stream, stream_config, sample_format))
        }
        Err(error) => Err(error),
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_default_voice_input_stream(
    device: &cpal::Device,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
) -> Result<(cpal::Stream, cpal::StreamConfig, cpal::SampleFormat), String> {
    let supported_config = device
        .default_input_config()
        .map_err(|error| format!("voice microphone default input config failed: {error}"))?;
    let sample_format = supported_config.sample_format();
    let stream_config = supported_config.config();
    build_voice_input_stream(device, &stream_config, sample_format, stats, samples_tx)
        .map(|stream| (stream, stream_config, sample_format))
}

#[cfg(feature = "voice-playback")]
pub(super) fn select_voice_input_config(
    device: &cpal::Device,
) -> Result<cpal::SupportedStreamConfig, String> {
    device
        .supported_input_configs()
        .map_err(|error| format!("voice microphone input config query failed: {error}"))?
        .filter(|config| {
            config.min_sample_rate() <= DISCORD_VOICE_SAMPLE_RATE
                && config.max_sample_rate() >= DISCORD_VOICE_SAMPLE_RATE
                && (config.channels() == 1 || config.channels() == DISCORD_VOICE_CHANNELS)
        })
        .min_by_key(voice_input_config_rank)
        .map(|config| config.with_sample_rate(DISCORD_VOICE_SAMPLE_RATE))
        .ok_or_else(|| "no Discord-friendly microphone input config found".to_owned())
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_config_rank(config: &cpal::SupportedStreamConfigRange) -> (u8, u8) {
    (
        voice_input_channel_rank(config.channels()),
        voice_input_sample_format_rank(config.sample_format()),
    )
}

#[cfg(feature = "voice-playback")]
pub(crate) fn voice_input_channel_rank(channels: u16) -> u8 {
    match channels {
        1 => 0,
        DISCORD_VOICE_CHANNELS => 1,
        _ => 2,
    }
}

#[cfg(feature = "voice-playback")]
pub(crate) fn voice_input_sample_format_rank(format: cpal::SampleFormat) -> u8 {
    match format {
        cpal::SampleFormat::F32 => 0,
        cpal::SampleFormat::I16 => 1,
        cpal::SampleFormat::U16 => 2,
        cpal::SampleFormat::U8 => 3,
        _ if format.is_uint() => 4,
        _ => 5,
    }
}

#[cfg(feature = "voice-playback")]
pub(crate) fn voice_input_buffer_size(supported: &cpal::SupportedBufferSize) -> cpal::BufferSize {
    match supported {
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed(VOICE_MIC_PREFERRED_BUFFER_FRAMES.clamp(*min, *max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    }
}

#[cfg(feature = "voice-playback")]
impl Default for VoiceMicrophoneCaptureStats {
    fn default() -> Self {
        Self {
            chunks: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            min_callback_frames: AtomicU64::new(u64::MAX),
            max_callback_frames: AtomicU64::new(0),
            queued_frames: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            peak_sample: AtomicU64::new(0),
            clipped_samples: AtomicU64::new(0),
        }
    }
}

#[cfg(feature = "voice-playback")]
impl VoiceMicrophonePcmFrames {
    pub(crate) fn new(
        frames_tx: mpsc::Sender<VoiceMicrophoneFrame>,
        stats: Arc<VoiceMicrophoneCaptureStats>,
        source_sample_rate: u32,
    ) -> Self {
        Self {
            frames_tx,
            stats,
            source_sample_rate,
            source_pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES),
            output_pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES),
            next_source_frame: 0.0,
        }
    }

    pub(crate) fn push_stereo_samples(&mut self, samples: &[i16]) {
        if self.source_sample_rate == DISCORD_VOICE_SAMPLE_RATE {
            self.output_pending.extend_from_slice(samples);
            self.flush_output_frames();
            return;
        }

        self.source_pending.extend_from_slice(samples);
        self.resample_pending_source();
        self.flush_output_frames();
    }

    pub(super) fn resample_pending_source(&mut self) {
        let source_frames = self.source_pending.len() / DISCORD_VOICE_CHANNELS_USIZE;
        if source_frames < 2 {
            return;
        }

        let source_step = f64::from(self.source_sample_rate) / f64::from(DISCORD_VOICE_SAMPLE_RATE);
        while self.next_source_frame + 1.0 < source_frames as f64 {
            let frame_index = self.next_source_frame.floor() as usize;
            let fraction = self.next_source_frame - frame_index as f64;
            let left = interpolate_i16(
                self.source_pending[frame_index * DISCORD_VOICE_CHANNELS_USIZE],
                self.source_pending[(frame_index + 1) * DISCORD_VOICE_CHANNELS_USIZE],
                fraction,
            );
            let right = interpolate_i16(
                self.source_pending[frame_index * DISCORD_VOICE_CHANNELS_USIZE + 1],
                self.source_pending[(frame_index + 1) * DISCORD_VOICE_CHANNELS_USIZE + 1],
                fraction,
            );
            self.output_pending.push(left);
            self.output_pending.push(right);
            self.next_source_frame += source_step;
        }

        let consumed_frames = self.next_source_frame.floor() as usize;
        if consumed_frames > 0 {
            self.source_pending
                .drain(..consumed_frames * DISCORD_VOICE_CHANNELS_USIZE);
            self.next_source_frame -= consumed_frames as f64;
        }
    }

    pub(super) fn flush_output_frames(&mut self) {
        while self.output_pending.len() >= DISCORD_OPUS_20MS_STEREO_SAMPLES {
            let frame = VoiceMicrophoneFrame {
                samples: self
                    .output_pending
                    .drain(..DISCORD_OPUS_20MS_STEREO_SAMPLES)
                    .collect(),
                captured_at: Instant::now(),
            };
            if self.frames_tx.try_send(frame).is_ok() {
                self.stats.queued_frames.fetch_add(1, Ordering::Relaxed);
            } else {
                self.stats.dropped_frames.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn interpolate_i16(current: i16, next: i16, fraction: f64) -> i16 {
    let value = f64::from(current) + (f64::from(next) - f64::from(current)) * fraction;
    value
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

#[cfg(feature = "voice-playback")]
impl Drop for VoiceMicrophoneCapture {
    fn drop(&mut self) {
        logging::debug(
            "voice",
            format!(
                "voice microphone capture stopped: chunks={} frames={} callback_frames_min={} callback_frames_max={} queued_20ms_frames={} dropped_20ms_frames={} peak_sample={} clipped_samples={}",
                self.stats.chunks.load(Ordering::Relaxed),
                self.stats.frames.load(Ordering::Relaxed),
                voice_microphone_min_callback_frames(&self.stats),
                self.stats.max_callback_frames.load(Ordering::Relaxed),
                self.stats.queued_frames.load(Ordering::Relaxed),
                self.stats.dropped_frames.load(Ordering::Relaxed),
                self.stats.peak_sample.load(Ordering::Relaxed),
                self.stats.clipped_samples.load(Ordering::Relaxed),
            ),
        );
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_voice_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
) -> Result<cpal::Stream, String> {
    match sample_format {
        cpal::SampleFormat::F32 => build_voice_input_stream_f32(device, config, stats, samples_tx),
        cpal::SampleFormat::U8 => build_voice_input_stream_u8(device, config, stats, samples_tx),
        cpal::SampleFormat::I16 => build_voice_input_stream_i16(device, config, stats, samples_tx),
        cpal::SampleFormat::U16 => build_voice_input_stream_u16(device, config, stats, samples_tx),
        other => Err(format!(
            "unsupported voice microphone input sample format: {other:?}"
        )),
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_voice_input_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            Arc::clone(&stats),
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            *config,
            move |input: &[f32], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_f32_to_stereo_i16(input, channels);
                    record_voice_input_pcm_stats(&samples, &stats);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_voice_input_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            Arc::clone(&stats),
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            *config,
            move |input: &[i16], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_i16_to_stereo_i16(input, channels);
                    record_voice_input_pcm_stats(&samples, &stats);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_voice_input_stream_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            Arc::clone(&stats),
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            *config,
            move |input: &[u16], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_u16_to_stereo_i16(input, channels);
                    record_voice_input_pcm_stats(&samples, &stats);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_voice_input_stream_u8(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            Arc::clone(&stats),
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            *config,
            move |input: &[u8], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_u8_to_stereo_i16(input, channels);
                    record_voice_input_pcm_stats(&samples, &stats);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
pub(crate) fn voice_input_f32_to_stereo_i16(input: &[f32], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| {
        (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
    })
}

#[cfg(feature = "voice-playback")]
pub(crate) fn voice_input_i16_to_stereo_i16(input: &[i16], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| sample)
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_u16_to_stereo_i16(input: &[u16], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| {
        let shifted = i32::from(sample) - 32768;
        shifted.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    })
}

#[cfg(feature = "voice-playback")]
pub(crate) fn voice_input_u8_to_stereo_i16(input: &[u8], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| (i16::from(sample) - 128) << 8)
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_to_stereo_i16<T>(
    input: &[T],
    channels: usize,
    mut convert: impl FnMut(T) -> i16,
) -> Vec<i16>
where
    T: Copy,
{
    if channels == 0 {
        return Vec::new();
    }
    let frames = input.len() / channels;
    let mut stereo = Vec::with_capacity(frames * usize::from(DISCORD_VOICE_CHANNELS));
    for frame in input.chunks_exact(channels) {
        let left = convert(frame[0]);
        let right = if channels == 1 {
            left
        } else {
            convert(frame[1])
        };
        stereo.push(left);
        stereo.push(right);
    }
    stereo
}

#[cfg(feature = "voice-playback")]
pub(crate) fn record_voice_input_chunk(
    sample_count: usize,
    channels: usize,
    stats: &VoiceMicrophoneCaptureStats,
) {
    let frames = sample_count / channels.max(1);
    stats.chunks.fetch_add(1, Ordering::Relaxed);
    stats
        .frames
        .fetch_add(u64::try_from(frames).unwrap_or(u64::MAX), Ordering::Relaxed);
    let frames = u64::try_from(frames).unwrap_or(u64::MAX);
    stats
        .min_callback_frames
        .fetch_min(frames, Ordering::Relaxed);
    stats
        .max_callback_frames
        .fetch_max(frames, Ordering::Relaxed);
}

#[cfg(feature = "voice-playback")]
pub(crate) fn record_voice_input_pcm_stats(samples: &[i16], stats: &VoiceMicrophoneCaptureStats) {
    let peak = samples
        .iter()
        .map(|sample| i32::from(*sample).unsigned_abs() as u64)
        .max()
        .unwrap_or(0);
    let clipped = samples
        .iter()
        .filter(|sample| i32::from(**sample).abs() >= i32::from(i16::MAX) - 1)
        .count();

    stats.peak_sample.fetch_max(peak, Ordering::Relaxed);
    stats.clipped_samples.fetch_add(
        u64::try_from(clipped).unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
}

#[cfg(feature = "voice-playback")]
pub(crate) fn voice_microphone_min_callback_frames(stats: &VoiceMicrophoneCaptureStats) -> u64 {
    let min = stats.min_callback_frames.load(Ordering::Relaxed);
    if min == u64::MAX { 0 } else { min }
}

#[cfg(feature = "voice-playback")]
pub(super) fn log_voice_input_stream_error(error: cpal::Error) {
    logging::error(
        "voice",
        format!("voice microphone input stream failed: {error}"),
    );
}

#[cfg(all(feature = "voice-playback", target_os = "linux"))]
pub(crate) fn log_captured_alsa_errors(
    alsa_error_output: &Option<std::rc::Rc<std::cell::RefCell<alsa::Output>>>,
) {
    let Some(output) = alsa_error_output else {
        return;
    };
    let message = output
        .borrow()
        .buffer_string(|bytes| String::from_utf8_lossy(bytes).replace('\0', ""));
    let message = message.trim();
    if message.is_empty() {
        return;
    }
    logging::error("voice", format!("captured ALSA diagnostics: {message}"));
}
