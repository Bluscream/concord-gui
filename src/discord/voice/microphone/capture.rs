//! Microphone capture: opening the input stream, converting what the device
//! gives us into stereo i16, and the buffering that keeps the output timeline
//! steady when the device does not deliver on schedule.
//!
//! Ported from upstream's `voice/microphone.rs`, most recently at v2.5.21,
//! which added the bounded input channel in `input.rs` to tolerate capture
//! latency without dropping frames (#369).

use super::super::devices;
use super::*;

#[cfg(feature = "voice-playback")]
impl VoiceMicrophoneCapture {
    pub(crate) fn start(
        samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
        input_source: Option<&str>,
        microphone_buffer_ms: Option<MicrophoneBufferMs>,
    ) -> Result<Self, String> {
        let buffer_mode = microphone_buffer_ms.map_or(
            VoiceMicrophoneBufferMode::PlatformFixed,
            VoiceMicrophoneBufferMode::UserFixed,
        );
        Self::start_with_policy(samples_tx, input_source, buffer_mode)
    }

    fn start_with_policy(
        samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
        input_source: Option<&str>,
        buffer_mode: VoiceMicrophoneBufferMode,
    ) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let alsa_error_output = alsa::Output::local_error_handler().ok();

        let result = Self::start_with_cpal(samples_tx, input_source, buffer_mode);

        #[cfg(target_os = "linux")]
        log_captured_alsa_errors(&alsa_error_output);

        result
    }

    pub(crate) fn start_with_cpal(
        samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
        input_source: Option<&str>,
        buffer_mode: VoiceMicrophoneBufferMode,
    ) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = devices::resolve_input_device(&host, input_source)?;
        let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
        let input_stream = build_preferred_voice_input_stream(
            &device,
            Arc::clone(&stats),
            samples_tx.clone(),
            buffer_mode,
        )
        .or_else(|preferred_error| {
            logging::debug(
                "voice",
                format!("voice preferred microphone input stream failed: {preferred_error}"),
            );
            build_default_voice_input_stream(&device, Arc::clone(&stats), samples_tx, buffer_mode)
        })?;
        input_stream
            .stream
            .play()
            .map_err(|error| format!("voice microphone input stream start failed: {error}"))?;
        let stream_reported_buffer_ms = input_stream
            .stream_reported_buffer_frames
            .map(|frames| voice_buffer_duration_ms(frames, input_stream.stream_config.sample_rate));
        logging::debug(
            "voice",
            format!(
                "voice microphone capture started: host={} sample_rate={} channels={} format={:?} buffer_mode={:?} requested_buffer={:?} stream_reported_buffer_frames={} stream_reported_buffer_ms={}",
                host.id(),
                input_stream.stream_config.sample_rate,
                input_stream.stream_config.channels,
                input_stream.sample_format,
                input_stream.buffer_mode,
                input_stream.stream_config.buffer_size,
                input_stream
                    .stream_reported_buffer_frames
                    .map_or_else(|| "unknown".to_owned(), |frames| frames.to_string()),
                stream_reported_buffer_ms
                    .map_or_else(|| "unknown".to_owned(), |millis| millis.to_string()),
            ),
        );
        Ok(Self {
            _stream: input_stream.stream,
            _processor: input_stream.processor,
            stats,
        })
    }
}

#[cfg(feature = "voice-playback")]
pub(crate) fn build_preferred_voice_input_stream(
    device: &cpal::Device,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    buffer_mode: VoiceMicrophoneBufferMode,
) -> Result<VoiceMicrophoneInputStream, String> {
    let supported_config = select_voice_input_config(device)?;
    let supported_buffer_size = *supported_config.buffer_size();
    let sample_format = supported_config.sample_format();
    let mut stream_config = supported_config.config();
    build_configured_voice_input_stream(
        device,
        &mut stream_config,
        &supported_buffer_size,
        sample_format,
        stats,
        samples_tx,
        buffer_mode,
    )
}

#[cfg(feature = "voice-playback")]
pub(crate) fn build_default_voice_input_stream(
    device: &cpal::Device,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    buffer_mode: VoiceMicrophoneBufferMode,
) -> Result<VoiceMicrophoneInputStream, String> {
    let supported_config = device
        .default_input_config()
        .map_err(|error| format!("voice microphone default input config failed: {error}"))?;
    let supported_buffer_size = *supported_config.buffer_size();
    let sample_format = supported_config.sample_format();
    let mut stream_config = supported_config.config();
    build_configured_voice_input_stream(
        device,
        &mut stream_config,
        &supported_buffer_size,
        sample_format,
        stats,
        samples_tx,
        buffer_mode,
    )
}

#[cfg(feature = "voice-playback")]
fn build_configured_voice_input_stream(
    device: &cpal::Device,
    stream_config: &mut cpal::StreamConfig,
    supported_buffer_size: &cpal::SupportedBufferSize,
    sample_format: cpal::SampleFormat,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    buffer_mode: VoiceMicrophoneBufferMode,
) -> Result<VoiceMicrophoneInputStream, String> {
    match buffer_mode {
        VoiceMicrophoneBufferMode::UserFixed(duration) => {
            stream_config.buffer_size =
                voice_input_buffer_size(duration, stream_config.sample_rate);
            build_voice_input_stream_with_mode(
                device,
                stream_config,
                sample_format,
                stats,
                samples_tx,
                buffer_mode,
            )
        }
        VoiceMicrophoneBufferMode::PlatformFixed => {
            let Some(buffer_size) =
                automatic_voice_input_buffer_size(supported_buffer_size, stream_config.sample_rate)
            else {
                logging::debug(
                    "voice",
                    "voice microphone buffer range is unknown, using host default buffer",
                );
                return build_host_default_voice_input_stream(
                    device,
                    stream_config,
                    sample_format,
                    stats,
                    samples_tx,
                );
            };

            stream_config.buffer_size = buffer_size;
            match build_voice_input_stream_with_mode(
                device,
                stream_config,
                sample_format,
                Arc::clone(&stats),
                samples_tx.clone(),
                VoiceMicrophoneBufferMode::PlatformFixed,
            ) {
                Ok(input_stream) => Ok(input_stream),
                Err(fixed_error) => {
                    logging::debug(
                        "voice",
                        format!(
                            "voice fixed microphone input buffer failed, retrying host default buffer: {fixed_error}"
                        ),
                    );
                    build_host_default_voice_input_stream(
                        device,
                        stream_config,
                        sample_format,
                        stats,
                        samples_tx,
                    )
                    .map_err(|default_error| {
                        format!(
                            "voice fixed microphone input buffer failed ({fixed_error}); host default fallback failed ({default_error})"
                        )
                    })
                }
            }
        }
        VoiceMicrophoneBufferMode::HostDefaultFallback => build_host_default_voice_input_stream(
            device,
            stream_config,
            sample_format,
            stats,
            samples_tx,
        ),
    }
}

#[cfg(feature = "voice-playback")]
fn build_host_default_voice_input_stream(
    device: &cpal::Device,
    stream_config: &mut cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
) -> Result<VoiceMicrophoneInputStream, String> {
    stream_config.buffer_size = cpal::BufferSize::Default;
    build_voice_input_stream_with_mode(
        device,
        stream_config,
        sample_format,
        stats,
        samples_tx,
        VoiceMicrophoneBufferMode::HostDefaultFallback,
    )
}

#[cfg(feature = "voice-playback")]
fn build_voice_input_stream_with_mode(
    device: &cpal::Device,
    stream_config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    buffer_mode: VoiceMicrophoneBufferMode,
) -> Result<VoiceMicrophoneInputStream, String> {
    let (stream, processor) =
        build_voice_input_stream(device, stream_config, sample_format, stats, samples_tx)?;
    let stream_reported_buffer_frames = voice_stream_reported_buffer_frames(&stream);
    Ok(VoiceMicrophoneInputStream {
        stream,
        processor,
        stream_config: *stream_config,
        sample_format,
        buffer_mode,
        stream_reported_buffer_frames,
    })
}

#[cfg(feature = "voice-playback")]
pub(crate) fn select_voice_input_config(
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
pub(crate) fn voice_input_config_rank(config: &cpal::SupportedStreamConfigRange) -> (u8, u8) {
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
pub(crate) fn voice_input_buffer_size(
    microphone_buffer_ms: MicrophoneBufferMs,
    sample_rate: u32,
) -> cpal::BufferSize {
    cpal::BufferSize::Fixed(microphone_buffer_ms.frames(sample_rate))
}

#[cfg(feature = "voice-playback")]
pub(crate) fn automatic_voice_input_buffer_size(
    supported: &cpal::SupportedBufferSize,
    sample_rate: u32,
) -> Option<cpal::BufferSize> {
    match supported {
        cpal::SupportedBufferSize::Range { min, max } => {
            let callback_period_frames = sample_rate
                .checked_div(VOICE_MIC_AUTOMATIC_CALLBACKS_PER_SECOND)
                .unwrap_or(0)
                .max(1);
            Some(cpal::BufferSize::Fixed(
                callback_period_frames.clamp(*min, *max),
            ))
        }
        cpal::SupportedBufferSize::Unknown => None,
    }
}

#[cfg(feature = "voice-playback")]
fn duration_for_audio_frames(frames: u64, sample_rate: u32) -> Duration {
    if sample_rate == 0 {
        return Duration::ZERO;
    }
    let nanos = u128::from(frames).saturating_mul(1_000_000_000) / u128::from(sample_rate);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

#[cfg(feature = "voice-playback")]
fn voice_buffer_duration_ms(frames: u32, sample_rate: u32) -> u128 {
    duration_for_audio_frames(u64::from(frames), sample_rate).as_millis()
}

#[cfg(feature = "voice-playback")]
fn voice_stream_reported_buffer_frames(stream: &cpal::Stream) -> Option<u32> {
    match stream.buffer_size() {
        Ok(frames) => Some(frames),
        Err(error) => {
            logging::debug(
                "voice",
                format!("voice microphone reported buffer query failed: {error}"),
            );
            None
        }
    }
}

#[cfg(feature = "voice-playback")]
impl Default for VoiceMicrophoneCaptureStats {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            chunks: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            min_callback_frames: AtomicU64::new(u64::MAX),
            max_callback_frames: AtomicU64::new(0),
            queued_frames: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            peak_sample: AtomicU64::new(0),
            clipped_samples: AtomicU64::new(0),
            last_callback_elapsed_us: AtomicU64::new(0),
            max_callback_gap_ms: AtomicU64::new(0),
            input_queue_dropped_blocks: AtomicU64::new(0),
            stale_input_blocks: AtomicU64::new(0),
            stream_errors: AtomicU64::new(0),
            stream_xruns: AtomicU64::new(0),
            max_capture_latency_us: AtomicU64::new(0),
            max_capture_delivery_age_us: AtomicU64::new(0),
        }
    }
}

#[cfg(feature = "voice-playback")]
impl Drop for VoiceMicrophoneInputProcessor {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take()
            && let Err(error) = worker.join()
        {
            logging::debug(
                "voice",
                format!("voice microphone input processor panicked: {error:?}"),
            );
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
            output_pending_at: None,
            next_source_frame: 0.0,
            source_end: None,
            generation: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Returns the oldest completed frame timestamp, including frames dropped by a full queue.
    pub(crate) fn push_stereo_samples(
        &mut self,
        samples: &[i16],
        captured_at: Instant,
    ) -> Option<Instant> {
        if !self.generation.load(Ordering::Acquire)
            || self.source_end.is_some_and(|end| {
                captured_at.saturating_duration_since(end) > Duration::from_millis(20)
                    || end.saturating_duration_since(captured_at) > Duration::from_millis(20)
            })
        {
            self.reset();
        }
        self.align_output_timeline(captured_at);
        self.source_end = captured_at.checked_add(duration_for_audio_frames(
            (samples.len() / DISCORD_VOICE_CHANNELS_USIZE) as u64,
            self.source_sample_rate,
        ));
        if self.source_sample_rate == DISCORD_VOICE_SAMPLE_RATE {
            self.output_pending.extend_from_slice(samples);
        } else {
            self.source_pending.extend_from_slice(samples);
            self.resample_pending_source();
        }
        self.flush_output_frames()
    }

    pub(crate) fn reset(&mut self) {
        self.generation.store(false, Ordering::Release);
        self.generation = Arc::new(AtomicBool::new(true));
        self.source_pending.clear();
        self.output_pending.clear();
        self.output_pending_at = None;
        self.next_source_frame = 0.0;
        self.source_end = None;
    }

    fn align_output_timeline(&mut self, captured_at: Instant) {
        let pending_output_frames = self.output_pending.len() / DISCORD_VOICE_CHANNELS_USIZE;
        let pending_source_frames = self.source_pending.len() / DISCORD_VOICE_CHANNELS_USIZE;
        let pending_source_duration = duration_for_audio_frames(
            u64::try_from(pending_source_frames).unwrap_or(u64::MAX),
            self.source_sample_rate,
        );
        let pending_output_duration = duration_for_audio_frames(
            u64::try_from(pending_output_frames).unwrap_or(u64::MAX),
            DISCORD_VOICE_SAMPLE_RATE,
        );
        let pending_duration = pending_source_duration.saturating_add(pending_output_duration);
        let pending_started_at = captured_at
            .checked_sub(pending_duration)
            .unwrap_or(captured_at);
        self.output_pending_at.get_or_insert(pending_started_at);
    }

    pub(crate) fn resample_pending_source(&mut self) {
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

    pub(crate) fn flush_output_frames(&mut self) -> Option<Instant> {
        let mut oldest_frame_at = None;
        while self.output_pending.len() >= DISCORD_OPUS_20MS_STEREO_SAMPLES {
            let frame = VoiceMicrophoneFrame {
                samples: self
                    .output_pending
                    .drain(..DISCORD_OPUS_20MS_STEREO_SAMPLES)
                    .collect(),
                captured_at: self.output_pending_at.unwrap_or_else(Instant::now),
                generation: Arc::clone(&self.generation),
            };
            oldest_frame_at.get_or_insert(frame.captured_at);
            self.output_pending_at = self
                .output_pending_at
                .and_then(|captured_at| captured_at.checked_add(DISCORD_OPUS_FRAME_DURATION));
            if self.frames_tx.try_send(frame).is_ok() {
                self.stats.queued_frames.fetch_add(1, Ordering::Relaxed);
            } else {
                let remaining = self.output_pending.len() / DISCORD_OPUS_20MS_STEREO_SAMPLES;
                self.stats
                    .dropped_frames
                    .fetch_add(1 + remaining as u64, Ordering::Relaxed);
                self.reset();
                break;
            }
        }
        if self.source_pending.is_empty() && self.output_pending.is_empty() {
            self.output_pending_at = None;
        }
        oldest_frame_at
    }
}

#[cfg(feature = "voice-playback")]
pub(crate) fn interpolate_i16(current: i16, next: i16, fraction: f64) -> i16 {
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
                "voice microphone capture stopped: chunks={} frames={} callback_frames_min={} callback_frames_max={} callback_max_gap_ms={} input_queue_dropped_blocks={} stale_input_blocks={} capture_latency_max_us={} capture_delivery_age_max_us={} stream_errors={} stream_xruns={} queued_20ms_frames={} dropped_20ms_frames={} peak_sample={} clipped_samples={}",
                self.stats.chunks.load(Ordering::Relaxed),
                self.stats.frames.load(Ordering::Relaxed),
                voice_microphone_min_callback_frames(&self.stats),
                self.stats.max_callback_frames.load(Ordering::Relaxed),
                self.stats.max_callback_gap_ms.load(Ordering::Relaxed),
                self.stats
                    .input_queue_dropped_blocks
                    .load(Ordering::Relaxed),
                self.stats.stale_input_blocks.load(Ordering::Relaxed),
                self.stats.max_capture_latency_us.load(Ordering::Relaxed),
                self.stats
                    .max_capture_delivery_age_us
                    .load(Ordering::Relaxed),
                self.stats.stream_errors.load(Ordering::Relaxed),
                self.stats.stream_xruns.load(Ordering::Relaxed),
                self.stats.queued_frames.load(Ordering::Relaxed),
                self.stats.dropped_frames.load(Ordering::Relaxed),
                self.stats.peak_sample.load(Ordering::Relaxed),
                self.stats.clipped_samples.load(Ordering::Relaxed),
            ),
        );
    }
}

#[cfg(feature = "voice-playback")]
pub(crate) fn build_voice_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
) -> Result<(cpal::Stream, VoiceMicrophoneInputProcessor), String> {
    match sample_format {
        cpal::SampleFormat::F32 => build_typed_voice_input_stream(
            device,
            config,
            stats,
            samples_tx,
            voice_input_f32_to_stereo_i16,
        ),
        cpal::SampleFormat::U8 => build_typed_voice_input_stream(
            device,
            config,
            stats,
            samples_tx,
            voice_input_u8_to_stereo_i16,
        ),
        cpal::SampleFormat::I16 => build_typed_voice_input_stream(
            device,
            config,
            stats,
            samples_tx,
            voice_input_i16_to_stereo_i16,
        ),
        cpal::SampleFormat::U16 => build_typed_voice_input_stream(
            device,
            config,
            stats,
            samples_tx,
            voice_input_u16_to_stereo_i16,
        ),
        other => Err(format!(
            "unsupported voice microphone input sample format: {other:?}"
        )),
    }
}

#[cfg(feature = "voice-playback")]
fn build_typed_voice_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    convert: fn(&[T], usize) -> Vec<i16>,
) -> Result<(cpal::Stream, VoiceMicrophoneInputProcessor), String>
where
    T: cpal::SizedSample + Copy + Send + 'static,
{
    let channels = usize::from(config.channels);
    let sample_rate = config.sample_rate;
    let (mut input_tx, mut input_rx) = input::channel::<T>(sample_rate, channels);
    let stopped = Arc::new(AtomicBool::new(false));
    let worker_stopped = Arc::clone(&stopped);
    let worker_stats = Arc::clone(&stats);
    let worker = std::thread::Builder::new()
        .name("voice-mic-input".to_owned())
        .spawn(move || {
            let mut pcm_frames =
                VoiceMicrophonePcmFrames::new(samples_tx, Arc::clone(&worker_stats), sample_rate);
            while !worker_stopped.load(Ordering::Acquire) {
                let chunk = match input_rx.recv_timeout(VOICE_MIC_SERVICE_INTERVAL) {
                    Ok(chunk) => chunk,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                };
                if chunk.discontinuity {
                    pcm_frames.reset();
                }
                if Instant::now().saturating_duration_since(chunk.captured_at)
                    > VOICE_MIC_MAX_PROCESSING_DELAY
                {
                    pcm_frames.reset();
                    worker_stats
                        .stale_input_blocks
                        .fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                let samples = convert(&chunk.samples, channels);
                record_voice_input_pcm_stats(&samples, &worker_stats);
                let oldest_frame_at = pcm_frames.push_stereo_samples(&samples, chunk.captured_at);
                record_voice_input_delivery_age(oldest_frame_at, Instant::now(), &worker_stats);
            }
        })
        .map_err(|error| format!("voice microphone input processor spawn failed: {error}"))?;
    let processor = VoiceMicrophoneInputProcessor {
        stopped,
        worker: Some(worker),
    };
    let error_stats = Arc::clone(&stats);
    let stream = device
        .build_input_stream(
            *config,
            move |input: &[T], info| {
                let callback_at = Instant::now();
                let captured_at = voice_input_capture_instant(info, callback_at);
                record_voice_input_chunk(input.len(), channels, captured_at, callback_at, &stats);
                let dropped = input_tx.push(input, captured_at);
                stats
                    .input_queue_dropped_blocks
                    .fetch_add(dropped, Ordering::Relaxed);
            },
            move |error| record_voice_input_stream_error(error, &error_stats),
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))?;
    Ok((stream, processor))
}

#[cfg(feature = "voice-playback")]
fn voice_input_capture_instant(info: &cpal::InputCallbackInfo, callback_at: Instant) -> Instant {
    let timestamp = info.timestamp();
    let capture_delay = timestamp
        .callback
        .saturating_duration_since(timestamp.capture);
    callback_at
        .checked_sub(capture_delay)
        .unwrap_or(callback_at)
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
pub(crate) fn voice_input_u16_to_stereo_i16(input: &[u16], channels: usize) -> Vec<i16> {
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
pub(crate) fn voice_input_to_stereo_i16<T>(
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
    captured_at: Instant,
    callback_at: Instant,
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

    let elapsed_us = u64::try_from(
        callback_at
            .saturating_duration_since(stats.started_at)
            .as_micros(),
    )
    .unwrap_or(u64::MAX);
    let previous_elapsed_us = stats
        .last_callback_elapsed_us
        .swap(elapsed_us.max(1), Ordering::Relaxed);
    let callback_gap = elapsed_us.saturating_sub(previous_elapsed_us);
    if previous_elapsed_us != 0 {
        stats
            .max_callback_gap_ms
            .fetch_max(callback_gap / 1_000, Ordering::Relaxed);
    }

    let capture_latency = callback_at.saturating_duration_since(captured_at);
    let capture_latency_us = u64::try_from(capture_latency.as_micros()).unwrap_or(u64::MAX);
    stats
        .max_capture_latency_us
        .fetch_max(capture_latency_us, Ordering::Relaxed);
}

#[cfg(feature = "voice-playback")]
fn record_voice_input_delivery_age(
    oldest_frame_at: Option<Instant>,
    ready_at: Instant,
    stats: &VoiceMicrophoneCaptureStats,
) {
    if let Some(captured_at) = oldest_frame_at {
        let age = ready_at.saturating_duration_since(captured_at);
        stats.max_capture_delivery_age_us.fetch_max(
            u64::try_from(age.as_micros()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }
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
pub(crate) fn record_voice_input_stream_error(
    error: cpal::Error,
    stats: &VoiceMicrophoneCaptureStats,
) {
    stats.stream_errors.fetch_add(1, Ordering::Relaxed);
    if error.kind() == cpal::ErrorKind::Xrun {
        stats.stream_xruns.fetch_add(1, Ordering::Relaxed);
        logging::debug(
            "voice",
            format!("voice microphone input stream reported an xrun: {error}"),
        );
    } else {
        logging::error(
            "voice",
            format!("voice microphone input stream failed: {error}"),
        );
    }
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
