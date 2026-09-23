use super::*;

pub(super) struct ManagedTask {
    pub(super) label: &'static str,
    pub(super) task: Option<JoinHandle<()>>,
}

impl ManagedTask {
    pub(super) const fn new(label: &'static str) -> Self {
        Self { label, task: None }
    }

    pub(super) fn replace(&mut self, task: JoinHandle<()>) {
        if let Some(previous) = self.task.replace(task) {
            logging::debug("voice", format!("aborting previous {}", self.label));
            previous.abort();
        }
    }

    pub(super) fn abort(&mut self) {
        if let Some(task) = self.task.take() {
            logging::debug("voice", format!("aborting {}", self.label));
            task.abort();
        }
    }
}

pub(super) struct VoiceChildTasks {
    pub(super) heartbeat: ManagedTask,
    pub(super) udp_ping: ManagedTask,
    pub(super) udp_receive: ManagedTask,
    #[cfg(feature = "voice-playback")]
    pub(super) udp_transmit: Option<JoinHandle<()>>,
    #[cfg(feature = "voice-playback")]
    pub(super) transmit_gate: Option<watch::Sender<VoiceCaptureGate>>,
    #[cfg(feature = "voice-playback")]
    pub(super) playback_enabled: Option<Arc<AtomicBool>>,
    #[cfg(feature = "voice-playback")]
    pub(super) playback_volume: Option<Arc<AtomicU8>>,
    #[cfg(feature = "voice-playback")]
    pub(super) microphone_pcm_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
    pub(super) opus_decode: ManagedTask,
    #[cfg(feature = "voice-playback")]
    pub(super) audio_output: Option<VoiceAudioOutput>,
    #[cfg(feature = "voice-playback")]
    pub(super) decoded_audio_output: Option<VoiceDecodedAudioOutput>,
    #[cfg(feature = "voice-playback")]
    pub(super) microphone_capture: Option<VoiceMicrophoneCapture>,
    #[cfg(feature = "voice-playback")]
    pub(super) microphone_source: Option<String>,
    pub(super) microphone_buffer_ms: Option<MicrophoneBufferMs>,
    #[cfg(feature = "voice-playback")]
    pub(super) output_source: Option<String>,
    pub(super) audio_runtime: Option<VoiceAudioRuntime>,
}

#[derive(Default)]
pub(super) struct VoiceHeartbeatAckState {
    pub(super) awaiting_ack: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VoiceHeartbeatTimeout {
    pub(super) generation: u64,
}

impl VoiceHeartbeatAckState {
    pub(super) fn mark_sent(&mut self) -> bool {
        if self.awaiting_ack {
            return false;
        }
        self.awaiting_ack = true;
        true
    }

    pub(super) fn mark_acknowledged(&mut self) {
        self.awaiting_ack = false;
    }

    pub(super) fn reset(&mut self) {
        self.awaiting_ack = false;
    }
}

impl Default for VoiceChildTasks {
    fn default() -> Self {
        Self {
            heartbeat: ManagedTask::new("voice heartbeat task"),
            udp_ping: ManagedTask::new("voice UDP ping task"),
            udp_receive: ManagedTask::new("voice UDP receive task"),
            #[cfg(feature = "voice-playback")]
            udp_transmit: None,
            #[cfg(feature = "voice-playback")]
            transmit_gate: None,
            #[cfg(feature = "voice-playback")]
            playback_enabled: None,
            #[cfg(feature = "voice-playback")]
            playback_volume: None,
            #[cfg(feature = "voice-playback")]
            microphone_pcm_tx: None,
            opus_decode: ManagedTask::new("voice Opus decode task"),
            #[cfg(feature = "voice-playback")]
            audio_output: None,
            #[cfg(feature = "voice-playback")]
            decoded_audio_output: None,
            #[cfg(feature = "voice-playback")]
            microphone_capture: None,
            #[cfg(feature = "voice-playback")]
            microphone_source: None,
            microphone_buffer_ms: None,
            #[cfg(feature = "voice-playback")]
            output_source: None,
            audio_runtime: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VoiceCaptureGate {
    /// Preserve transmit transitions even when watch coalesces mute and unmute.
    pub(super) transmit_epoch: u64,
    pub(super) capture_enabled: bool,
    pub(super) transmit_enabled: bool,
    pub(super) use_voice_activity: bool,
    pub(super) noise_suppression: bool,
    pub(super) microphone_buffer_ms: Option<MicrophoneBufferMs>,
    pub(super) microphone_sensitivity: MicrophoneSensitivityDb,
    pub(super) microphone_volume: VoiceVolumePercent,
}

pub(super) struct VoiceGatewayControls {
    pub(super) audio_sources_rx: watch::Receiver<VoiceAudioSourceSelection>,
    pub(super) initial_capture_gate: VoiceCaptureGate,
    pub(super) capture_gate_rx: mpsc::UnboundedReceiver<VoiceCaptureGate>,
    pub(super) initial_playback_gate: VoicePlaybackGate,
    pub(super) playback_gate_rx: mpsc::UnboundedReceiver<VoicePlaybackGate>,
    pub(super) participant_playback_rx:
        watch::Receiver<HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>>,
}

#[cfg(feature = "voice-playback")]
pub(super) struct VoiceUdpTransmitContext {
    pub(super) udp_socket: Arc<UdpSocket>,
    pub(super) writer: VoiceWriter,
    pub(super) description: VoiceSessionDescription,
    pub(super) ssrc: u32,
    pub(super) dave_state: Arc<Mutex<VoiceDaveState>>,
    pub(super) local_speaking_tx: mpsc::UnboundedSender<bool>,
}

#[cfg(feature = "voice-playback")]
pub(super) struct VoiceMicrophoneInputStream {
    pub(super) stream: cpal::Stream,
    pub(super) processor: VoiceMicrophoneInputProcessor,
    pub(super) stream_config: cpal::StreamConfig,
    pub(super) sample_format: cpal::SampleFormat,
    pub(super) buffer_mode: VoiceMicrophoneBufferMode,
    pub(super) stream_reported_buffer_frames: Option<u32>,
}

#[cfg(feature = "voice-playback")]
pub(super) struct VoiceMicrophoneInputProcessor {
    pub(super) stopped: Arc<std::sync::atomic::AtomicBool>,
    pub(super) worker: Option<std::thread::JoinHandle<()>>,
}

/// How the microphone stream's buffer size is chosen. A reported range does
/// not mean the device and audio server can hold it, so the platform default
/// is the starting point and the host default the fallback.
#[cfg(feature = "voice-playback")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VoiceMicrophoneBufferMode {
    PlatformFixed,
    UserFixed(MicrophoneBufferMs),
    HostDefaultFallback,
}

#[cfg(feature = "voice-playback")]
pub(super) struct VoiceMicrophonePcmFrames {
    pub(super) frames_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    pub(super) stats: Arc<VoiceMicrophoneCaptureStats>,
    pub(super) source_sample_rate: u32,
    pub(super) source_pending: Vec<i16>,
    pub(super) output_pending: Vec<i16>,
    pub(super) output_pending_at: Option<Instant>,
    pub(super) next_source_frame: f64,
    pub(super) source_end: Option<Instant>,
    pub(super) generation: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(feature = "voice-playback")]
#[derive(Debug)]
pub(super) struct VoiceMicrophoneFrame {
    pub(super) samples: Vec<i16>,
    /// The oldest sample, including time spent in the audio backend.
    pub(super) captured_at: Instant,
    /// Either stage can retire all queued and partial audio from this
    /// generation.
    pub(super) generation: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(feature = "voice-playback")]
pub(super) struct VoiceMicrophoneCaptureStats {
    pub(super) started_at: Instant,
    pub(super) chunks: AtomicU64,
    pub(super) frames: AtomicU64,
    pub(super) min_callback_frames: AtomicU64,
    pub(super) max_callback_frames: AtomicU64,
    pub(super) queued_frames: AtomicU64,
    pub(super) dropped_frames: AtomicU64,
    pub(super) peak_sample: AtomicU64,
    pub(super) clipped_samples: AtomicU64,
    pub(super) last_callback_elapsed_us: AtomicU64,
    pub(super) max_callback_gap_ms: AtomicU64,
    pub(super) input_queue_dropped_blocks: AtomicU64,
    pub(super) stale_input_blocks: AtomicU64,
    pub(super) stream_errors: AtomicU64,
    pub(super) stream_xruns: AtomicU64,
    pub(super) max_capture_latency_us: AtomicU64,
    pub(super) max_capture_delivery_age_us: AtomicU64,
}

#[cfg(feature = "voice-playback")]
#[derive(Default)]
pub(super) struct VoiceUdpTransmitStats {
    pub(super) sent_packets: u64,
    pub(super) stale_microphone_frames_dropped: u64,
    pub(super) noise_suppressed_frames: u64,
    pub(super) max_noise_suppression_processing_us: u128,
    pub(super) overload_smoothed_frames: u64,
    pub(super) limited_samples: u64,
    pub(super) max_microphone_queue_depth: usize,
    pub(super) max_microphone_frame_age_ms: u128,
    pub(super) max_frame_gap_ms: u128,
    pub(super) last_frame_at: Option<Instant>,
}

#[cfg(any(test, feature = "voice-playback"))]
#[derive(Default)]
pub(super) struct VoiceTrailingSilence {
    pub(super) remaining_frames: usize,
}

#[cfg(any(test, feature = "voice-playback"))]
impl VoiceTrailingSilence {
    pub(super) fn start(&mut self, speaking: bool) {
        if speaking && self.remaining_frames == 0 {
            self.remaining_frames = DISCORD_TRAILING_SILENCE_FRAMES;
        }
    }

    pub(super) fn cancel(&mut self) {
        self.remaining_frames = 0;
    }

    pub(super) fn take_frame(&mut self) -> Option<bool> {
        if self.remaining_frames == 0 {
            return None;
        }
        self.remaining_frames -= 1;
        Some(self.remaining_frames == 0)
    }
}

#[cfg(any(test, feature = "voice-playback"))]
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VoiceMicrophoneOverloadKind {
    HandlingNoise,
    Transient,
    Attenuated,
    Recovery,
}

#[cfg(any(test, feature = "voice-playback"))]
#[derive(Clone, Copy, Debug)]
pub(super) struct VoiceMicrophoneOverloadDecision {
    pub(super) kind: VoiceMicrophoneOverloadKind,
    pub(super) gain: f32,
}

#[cfg(feature = "voice-playback")]
#[derive(Default)]
pub(super) struct VoiceMicrophoneGateState {
    pub(super) hangover_frames: u8,
    pub(super) overload_recovery_frames: u8,
    pub(super) handling_noise_suppression_frames: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VoiceBinaryFrame<'a> {
    pub(super) sequence: i64,
    pub(super) opcode: u8,
    pub(super) payload: &'a [u8],
}

impl VoiceChildTasks {
    pub(super) fn replace_heartbeat(&mut self, task: JoinHandle<()>) {
        self.heartbeat.replace(task);
    }

    pub(super) fn replace_udp_receive(&mut self, task: JoinHandle<()>) {
        self.udp_receive.replace(task);
    }

    pub(super) fn replace_udp_ping(&mut self, task: JoinHandle<()>) {
        self.udp_ping.replace(task);
    }

    #[cfg(feature = "voice-playback")]
    pub(super) fn install_udp_transmit(
        &mut self,
        task: JoinHandle<()>,
        gate: watch::Sender<VoiceCaptureGate>,
        microphone_pcm_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    ) {
        debug_assert!(self.udp_transmit.is_none());
        self.udp_transmit = Some(task);
        self.transmit_gate = Some(gate);
        self.microphone_pcm_tx = Some(microphone_pcm_tx);
    }

    #[cfg(feature = "voice-playback")]
    pub(super) fn signal_udp_transmit_stop(&mut self) {
        if let Some(gate) = self.transmit_gate.as_ref() {
            let _ = gate.send(VoiceCaptureGate {
                transmit_epoch: 0,
                capture_enabled: false,
                transmit_enabled: false,
                use_voice_activity: true,
                noise_suppression: false,
                microphone_buffer_ms: None,
                microphone_sensitivity: MicrophoneSensitivityDb::default(),
                microphone_volume: VoiceVolumePercent::default(),
            });
        }
        self.microphone_capture = None;
        self.microphone_pcm_tx = None;
        self.transmit_gate = None;
    }

    #[cfg(feature = "voice-playback")]
    pub(super) async fn stop_udp_transmit_gracefully(&mut self, label: &str) -> bool {
        let Some(mut task) = self.udp_transmit.take() else {
            self.signal_udp_transmit_stop();
            return false;
        };
        logging::debug("voice", label);
        self.signal_udp_transmit_stop();
        match timeout(VOICE_TRANSMIT_SHUTDOWN_TIMEOUT, &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                logging::debug("voice", format!("voice UDP transmit task ended: {error}"));
            }
            Err(_) => {
                logging::debug("voice", "voice UDP transmit graceful stop timed out");
                task.abort();
                let _ = task.await;
            }
        }
        true
    }

    pub(super) fn replace_opus_decode(&mut self, opus_decode: VoiceOpusDecode) {
        #[cfg(feature = "voice-playback")]
        {
            if let Some(decoded_audio_output) = self.decoded_audio_output.take() {
                decoded_audio_output.replace(None);
            }
            self.audio_output = opus_decode.audio_output;
            self.decoded_audio_output = Some(opus_decode.decoded_audio_output);
            self.playback_enabled = Some(opus_decode.playback_enabled);
            self.playback_volume = Some(opus_decode.playback_volume);
        }
        self.opus_decode.replace(opus_decode.task);
    }

    pub(super) fn abort_all(&mut self) {
        self.heartbeat.abort();
        self.udp_ping.abort();
        self.udp_receive.abort();
        #[cfg(feature = "voice-playback")]
        if let Some(task) = self.udp_transmit.take() {
            logging::debug("voice", "stopping voice UDP transmit task");
            self.signal_udp_transmit_stop();
            task.abort();
        }
        self.opus_decode.abort();
        #[cfg(feature = "voice-playback")]
        {
            if let Some(decoded_audio_output) = self.decoded_audio_output.take() {
                decoded_audio_output.replace(None);
            }
            self.audio_output = None;
            self.playback_enabled = None;
            self.playback_volume = None;
            self.microphone_capture = None;
        }
    }

    pub(super) async fn shutdown_all(&mut self) {
        #[cfg(feature = "voice-playback")]
        let _ = self
            .stop_udp_transmit_gracefully("stopping voice UDP transmit task")
            .await;
        self.abort_all();
    }

    #[allow(dead_code)]
    pub(super) fn set_microphone_capture_enabled(
        &mut self,
        enabled: bool,
        microphone_buffer_ms: Option<MicrophoneBufferMs>,
    ) {
        #[cfg(feature = "voice-playback")]
        {
            let buffer_changed = self.microphone_buffer_ms != microphone_buffer_ms;
            let samples_tx = if enabled {
                self.microphone_pcm_tx.clone()
            } else {
                None
            };
            match (
                samples_tx,
                self.microphone_capture.is_some(),
                buffer_changed,
            ) {
                (Some(samples_tx), false, _) => {
                    match VoiceMicrophoneCapture::start(
                        samples_tx,
                        self.microphone_source.as_deref(),
                        microphone_buffer_ms,
                    ) {
                        Ok(capture) => {
                            self.microphone_capture = Some(capture);
                            self.microphone_buffer_ms = microphone_buffer_ms;
                        }
                        Err(error) => logging::error(
                            "voice",
                            format!("voice microphone capture unavailable: {error}"),
                        ),
                    }
                }
                (Some(samples_tx), true, true) => {
                    logging::debug(
                        "voice",
                        "starting replacement voice microphone capture for new buffer setting",
                    );
                    match VoiceMicrophoneCapture::start(
                        samples_tx,
                        self.microphone_source.as_deref(),
                        microphone_buffer_ms,
                    ) {
                        Ok(capture) => {
                            self.microphone_capture = Some(capture);
                            self.microphone_buffer_ms = microphone_buffer_ms;
                            logging::debug(
                                "voice",
                                "replaced voice microphone capture for new buffer setting",
                            );
                        }
                        Err(error) => logging::error(
                            "voice",
                            format!(
                                "voice microphone buffer change failed; previous capture remains active: {error}"
                            ),
                        ),
                    }
                }
                (None, true, _) => {
                    logging::debug("voice", "stopping voice microphone capture");
                    self.microphone_capture = None;
                }
                _ => {}
            }
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = (enabled, microphone_buffer_ms);
        }
    }

    pub(super) fn set_voice_transmit_gate(&mut self, capture_gate: VoiceCaptureGate) {
        #[cfg(feature = "voice-playback")]
        {
            if let Some(gate) = self.transmit_gate.as_ref() {
                let _ = gate.send(capture_gate);
            }
            self.set_microphone_capture_enabled(
                capture_gate.capture_enabled,
                capture_gate.microphone_buffer_ms,
            );
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = capture_gate;
        }
    }

    pub(super) fn set_voice_playback_gate(&mut self, playback_gate: VoicePlaybackGate) {
        #[cfg(feature = "voice-playback")]
        {
            if let Some(playback_enabled) = self.playback_enabled.as_ref() {
                playback_enabled.store(playback_gate.enabled, Ordering::Relaxed);
            }
            if let Some(playback_volume) = self.playback_volume.as_ref() {
                playback_volume.store(playback_gate.volume.value(), Ordering::Relaxed);
            }
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = playback_gate;
        }
    }

    pub(super) fn set_voice_audio_sources(
        &mut self,
        sources: VoiceAudioSources,
        capture_gate: VoiceCaptureGate,
    ) -> VoiceAudioSourcesApplyOutcome {
        #[cfg(feature = "voice-playback")]
        {
            let mut errors = Vec::new();
            if self.microphone_source != sources.input {
                if let Some(samples_tx) = self.microphone_pcm_tx.clone()
                    && (self.microphone_capture.is_some() || capture_gate.capture_enabled)
                {
                    logging::debug(
                        "voice",
                        "starting replacement voice microphone capture for new source",
                    );
                    match VoiceMicrophoneCapture::start(
                        samples_tx,
                        sources.input.as_deref(),
                        capture_gate.microphone_buffer_ms,
                    ) {
                        Ok(capture) => {
                            self.microphone_capture = Some(capture);
                            self.microphone_source = sources.input;
                            logging::debug(
                                "voice",
                                "replaced voice microphone capture for new source",
                            );
                        }
                        Err(error) => errors.push(format!(
                            "Could not switch voice input source. The previous source remains active: {error}"
                        )),
                    }
                } else {
                    self.microphone_source = sources.input;
                }
            }
            if self.output_source != sources.output {
                if let Err(error) = self.replace_voice_audio_output(sources.output.as_deref()) {
                    errors.push(format!(
                        "Could not switch voice output source. The previous source remains active: {error}"
                    ));
                } else {
                    self.output_source = sources.output;
                }
            }
            VoiceAudioSourcesApplyOutcome {
                active_sources: VoiceAudioSources {
                    input: self.microphone_source.clone(),
                    output: self.output_source.clone(),
                },
                error: (!errors.is_empty()).then(|| errors.join(" ")),
            }
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = capture_gate;
            VoiceAudioSourcesApplyOutcome {
                active_sources: sources,
                error: None,
            }
        }
    }

    #[cfg(feature = "voice-playback")]
    pub(super) fn replace_voice_audio_output(
        &mut self,
        output_source: Option<&str>,
    ) -> Result<(), String> {
        let Some(decoded_audio_output) = self.decoded_audio_output.clone() else {
            return Ok(());
        };
        let (Some(playback_enabled), Some(playback_volume)) =
            (self.playback_enabled.clone(), self.playback_volume.clone())
        else {
            return Ok(());
        };

        let audio_output =
            VoiceAudioOutput::start(playback_enabled, playback_volume, output_source)?;
        decoded_audio_output.replace(Some(&audio_output));
        self.audio_output = Some(audio_output);
        logging::debug("voice", "replaced voice audio output for new source");
        Ok(())
    }
}

impl Drop for VoiceChildTasks {
    fn drop(&mut self) {
        self.abort_all();
    }
}
