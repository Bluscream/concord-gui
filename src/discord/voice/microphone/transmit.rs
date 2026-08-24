#[cfg(feature = "voice-playback")]
use super::noise::VoiceNoiseSuppressor;
use super::*;

#[cfg(feature = "voice-playback")]
async fn stop_voice_transmission(
    context: &VoiceUdpTransmitContext,
    sender: &mut VoiceOutboundSendState,
    transmit_stats: &mut VoiceUdpTransmitStats,
) {
    let outcome = sender.stop_speaking_with_dave(&mut *context.dave_state.lock().await);
    if let Err(error) = flush_voice_outbound_events(
        &context.udp_socket,
        &context.writer,
        outcome,
        sender,
        transmit_stats,
    )
    .await
    {
        logging::error("voice", error);
    }
}

#[cfg(feature = "voice-playback")]
async fn silence_voice_transmission(
    context: &VoiceUdpTransmitContext,
    sender: &mut VoiceOutboundSendState,
    transmit_stats: &mut VoiceUdpTransmitStats,
) {
    stop_voice_transmission(context, sender, transmit_stats).await;
    sender.set_capture_gate(false, false);
}

#[cfg(feature = "voice-playback")]
pub(crate) fn publish_local_speaking_edge(
    local_speaking_tx: &mpsc::UnboundedSender<bool>,
    local_speaking: &mut bool,
    speaking: bool,
) {
    if *local_speaking == speaking {
        return;
    }
    *local_speaking = speaking;
    let _ = local_speaking_tx.send(speaking);
}

#[cfg(any(test, feature = "voice-playback"))]
pub(crate) fn advance_voice_media_clock(
    sender: &mut VoiceOutboundSendState,
    previous_frame_at: &mut Option<Instant>,
    captured_at: Instant,
) {
    let elapsed_frames = previous_frame_at
        .map(|previous| {
            let elapsed_us = captured_at.saturating_duration_since(previous).as_micros();
            let frame_us = DISCORD_OPUS_FRAME_DURATION.as_micros();
            let rounded_frames = elapsed_us.saturating_add(frame_us / 2) / frame_us;
            u32::try_from(rounded_frames).unwrap_or(u32::MAX).max(1)
        })
        .unwrap_or(1);
    sender.advance_media_clock_frames(elapsed_frames);
    *previous_frame_at = Some(captured_at);
}

#[cfg(feature = "voice-playback")]
async fn send_voice_trailing_silence_frame(
    context: &VoiceUdpTransmitContext,
    sender: &mut VoiceOutboundSendState,
    transmit_stats: &mut VoiceUdpTransmitStats,
    trailing_silence: &mut VoiceTrailingSilence,
) -> Result<(), String> {
    let Some(finish_talkspurt) = trailing_silence.take_frame() else {
        return Ok(());
    };

    let mut dave_state = context.dave_state.lock().await;
    let outcome = sender.send_trailing_silence_frame_with_dave(&mut dave_state, finish_talkspurt);
    drop(dave_state);
    flush_voice_outbound_events(
        &context.udp_socket,
        &context.writer,
        outcome,
        sender,
        transmit_stats,
    )
    .await?;

    if !sender.speaking {
        trailing_silence.cancel();
    }
    Ok(())
}

#[cfg(feature = "voice-playback")]
pub(crate) async fn run_voice_udp_transmit(
    mut pcm_rx: mpsc::Receiver<VoiceMicrophoneFrame>,
    mut gate_rx: watch::Receiver<VoiceCaptureGate>,
    context: VoiceUdpTransmitContext,
) -> Result<(), String> {
    let rtp = VoiceOutboundRtpState {
        sequence: 0,
        timestamp: 0,
        ssrc: context.ssrc,
    };
    let mut sender = match VoiceOutboundSendState::new(
        &context.description.mode,
        &context.description.secret_key,
        rtp,
        0,
    ) {
        Ok(sender) => sender,
        Err(error) => {
            let _ = context.local_speaking_tx.send(false);
            return Err(format!("voice UDP transmit init failed: {error}"));
        }
    };
    let initial_gate = *gate_rx.borrow();
    sender.set_capture_gate(initial_gate.transmit_enabled, false);
    let mut encoder = match VoiceOpusEncode::new() {
        Ok(encoder) => encoder,
        Err(error) => {
            let _ = context.local_speaking_tx.send(false);
            return Err(error);
        }
    };
    let transmit_started_at = Instant::now();
    let mut transmit_stats = VoiceUdpTransmitStats::default();
    let mut microphone_gate = VoiceMicrophoneGateState::default();
    let mut trailing_silence = VoiceTrailingSilence::default();
    let mut noise_suppressor = VoiceNoiseSuppressor::new();
    let mut noise_suppression_enabled = initial_gate.noise_suppression;
    let mut previous_microphone_frame_at = None;
    let mut next_stats_log_at = transmit_started_at + VOICE_TRANSMIT_STATS_LOG_INTERVAL;
    let mut local_speaking = false;

    let result = loop {
        tokio::select! {
            changed = gate_rx.changed() => {
                if changed.is_err() {
                    drain_voice_microphone_pcm_queue(&mut pcm_rx);
                    silence_voice_transmission(&context, &mut sender, &mut transmit_stats).await;
                    break Ok(());
                }
                let gate = *gate_rx.borrow();
                let was_enabled = sender.capture_gate_enabled();
                if gate.transmit_enabled != was_enabled {
                    drain_voice_microphone_pcm_queue(&mut pcm_rx);
                    microphone_gate.reset();
                    noise_suppressor.reset();
                }
                if gate.noise_suppression != noise_suppression_enabled {
                    if gate.noise_suppression {
                        noise_suppressor.reset();
                    }
                    noise_suppression_enabled = gate.noise_suppression;
                }
                if !gate.transmit_enabled {
                    publish_local_speaking_edge(
                        &context.local_speaking_tx,
                        &mut local_speaking,
                        false,
                    );
                    if gate.capture_enabled {
                        trailing_silence.start(sender.speaking);
                    } else {
                        trailing_silence.cancel();
                        stop_voice_transmission(&context, &mut sender, &mut transmit_stats).await;
                    }
                    microphone_gate.reset();
                } else {
                    trailing_silence.cancel();
                }
                sender.set_capture_gate(gate.transmit_enabled, false);
            }
            received = pcm_rx.recv() => {
                let Some(frame) = received else {
                    silence_voice_transmission(&context, &mut sender, &mut transmit_stats).await;
                    microphone_gate.reset();
                    break Ok(());
                };
                transmit_stats.max_microphone_queue_depth = transmit_stats
                    .max_microphone_queue_depth
                    .max(pcm_rx.len().saturating_add(1));
                let (frame, stale_frames_dropped) = select_fresh_voice_microphone_frame(
                    frame,
                    &mut pcm_rx,
                    Instant::now(),
                );
                transmit_stats.stale_microphone_frames_dropped = transmit_stats
                    .stale_microphone_frames_dropped
                    .saturating_add(stale_frames_dropped);
                if stale_frames_dropped > 0 {
                    microphone_gate.reset();
                }
                let Some(mut frame) = frame else {
                    continue;
                };
                advance_voice_media_clock(
                    &mut sender,
                    &mut previous_microphone_frame_at,
                    frame.captured_at,
                );
                let gate = *gate_rx.borrow();
                if !gate.transmit_enabled {
                    publish_local_speaking_edge(
                        &context.local_speaking_tx,
                        &mut local_speaking,
                        false,
                    );
                    microphone_gate.reset();
                    if let Err(error) = send_voice_trailing_silence_frame(
                        &context,
                        &mut sender,
                        &mut transmit_stats,
                        &mut trailing_silence,
                    )
                    .await
                    {
                        break Err(error);
                    }
                } else {
                    if gate.noise_suppression {
                        let processing_started_at = Instant::now();
                        if noise_suppressor.process_20ms_stereo(&mut frame.samples) {
                            transmit_stats.noise_suppressed_frames = transmit_stats
                                .noise_suppressed_frames
                                .saturating_add(1);
                            transmit_stats.max_noise_suppression_processing_us = transmit_stats
                                .max_noise_suppression_processing_us
                                .max(processing_started_at.elapsed().as_micros());
                        }
                    }
                    let microphone_active =
                        voice_microphone_frame_is_active(gate, &mut microphone_gate, &frame.samples);
                    publish_local_speaking_edge(
                        &context.local_speaking_tx,
                        &mut local_speaking,
                        microphone_active,
                    );
                    if !microphone_active {
                        trailing_silence.start(sender.speaking);
                        if let Err(error) = send_voice_trailing_silence_frame(
                            &context,
                            &mut sender,
                            &mut transmit_stats,
                            &mut trailing_silence,
                        )
                        .await
                        {
                            break Err(error);
                        }
                    } else {
                        trailing_silence.cancel();
                        condition_voice_microphone_frame(
                            &mut frame.samples,
                            gate,
                            &mut microphone_gate,
                            &mut transmit_stats,
                        );
                        let opus = match encoder.encode_20ms_i16(&frame.samples) {
                            Ok(opus) => Some(opus),
                            Err(error) => {
                                logging::debug("voice", error);
                                None
                            }
                        };
                        if let Some(opus) = opus {
                            let now = Instant::now();
                            let frame_age = now.saturating_duration_since(frame.captured_at);
                            transmit_stats.max_microphone_queue_depth = transmit_stats
                                .max_microphone_queue_depth
                                .max(pcm_rx.len().saturating_add(1));
                            if frame_age > VOICE_MIC_MAX_FRAME_AGE
                                || pcm_rx.len().saturating_add(1) > VOICE_MIC_MAX_LIVE_FRAMES
                            {
                                transmit_stats.stale_microphone_frames_dropped = transmit_stats
                                    .stale_microphone_frames_dropped
                                    .saturating_add(1);
                                microphone_gate.reset();
                            } else {
                                transmit_stats.max_microphone_frame_age_ms = transmit_stats
                                    .max_microphone_frame_age_ms
                                    .max(frame_age.as_millis());
                                record_voice_transmit_frame(&mut transmit_stats, now);
                                let mut dave_state = context.dave_state.lock().await;
                                let outcome =
                                    sender.send_opus_frame_with_dave(&opus, &mut dave_state);
                                drop(dave_state);
                                if let Err(error) = flush_voice_outbound_events(
                                    &context.udp_socket,
                                    &context.writer,
                                    outcome,
                                    &mut sender,
                                    &mut transmit_stats,
                                )
                                .await
                                {
                                    break Err(error);
                                }
                            }
                        }
                    }
                }
                let now = Instant::now();
                if now >= next_stats_log_at {
                    log_voice_transmit_stats(
                        "voice UDP transmit stats",
                        &transmit_stats,
                        transmit_started_at,
                        sender.rtp.timestamp,
                    );
                    next_stats_log_at = now + VOICE_TRANSMIT_STATS_LOG_INTERVAL;
                }
            }
        }
    };
    publish_local_speaking_edge(&context.local_speaking_tx, &mut local_speaking, false);
    sender.set_capture_gate(false, false);
    log_voice_transmit_stats(
        "voice UDP transmit stopped",
        &transmit_stats,
        transmit_started_at,
        sender.rtp.timestamp,
    );
    result
}

#[cfg(feature = "voice-playback")]
pub(crate) fn drain_voice_microphone_pcm_queue(pcm_rx: &mut mpsc::Receiver<VoiceMicrophoneFrame>) {
    while pcm_rx.try_recv().is_ok() {}
}

#[cfg(feature = "voice-playback")]
pub(super) async fn flush_voice_outbound_events(
    udp_socket: &UdpSocket,
    writer: &VoiceWriter,
    outcome: Result<VoiceOutboundSendOutcome, String>,
    sender: &mut VoiceOutboundSendState,
    transmit_stats: &mut VoiceUdpTransmitStats,
) -> Result<(), String> {
    match outcome? {
        VoiceOutboundSendOutcome::Sent => {
            for event in sender.take_events() {
                match event {
                    VoiceOutboundSendEvent::Speaking { speaking, ssrc } => {
                        send_voice_text(writer, voice_speaking_payload(ssrc, speaking)).await?;
                    }
                    VoiceOutboundSendEvent::Packet { bytes } => {
                        udp_socket
                            .send(&bytes)
                            .await
                            .map_err(|error| format!("voice UDP transmit failed: {error}"))?;
                        transmit_stats.sent_packets += 1;
                    }
                }
            }
            if let Some(reason) = sender.take_logged_block_reason() {
                logging::debug(
                    "voice",
                    format!("voice UDP transmit resumed after block: {reason:?}"),
                );
            }
        }
        VoiceOutboundSendOutcome::Noop => {
            let _ = sender.take_logged_block_reason();
        }
        VoiceOutboundSendOutcome::Blocked(reason) => {
            if sender.record_blocked_transmit(reason) {
                logging::debug("voice", format!("voice UDP transmit blocked: {reason:?}"));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "voice-playback")]
pub(super) fn record_voice_transmit_frame(stats: &mut VoiceUdpTransmitStats, now: Instant) {
    if let Some(last_frame_at) = stats.last_frame_at {
        stats.max_frame_gap_ms = stats
            .max_frame_gap_ms
            .max(now.duration_since(last_frame_at).as_millis());
    }
    stats.last_frame_at = Some(now);
}

#[cfg(feature = "voice-playback")]
pub(super) fn log_voice_transmit_stats(
    label: &str,
    stats: &VoiceUdpTransmitStats,
    started_at: Instant,
    rtp_timestamp: u32,
) {
    let elapsed_ms = started_at.elapsed().as_millis();
    let rtp_elapsed_ms =
        (u128::from(rtp_timestamp) * 1_000) / u128::from(DISCORD_VOICE_SAMPLE_RATE);
    logging::debug(
        "voice",
        format!(
            "{label}: elapsed_ms={} sent_packets={} rtp_timestamp={} rtp_elapsed_ms={} stale_microphone_frames_dropped={} max_microphone_queue_depth={} max_microphone_frame_age_ms={} noise_suppressed_frames={} max_noise_suppression_processing_us={} overload_smoothed_frames={} limited_samples={} max_frame_gap_ms={}",
            elapsed_ms,
            stats.sent_packets,
            rtp_timestamp,
            rtp_elapsed_ms,
            stats.stale_microphone_frames_dropped,
            stats.max_microphone_queue_depth,
            stats.max_microphone_frame_age_ms,
            stats.noise_suppressed_frames,
            stats.max_noise_suppression_processing_us,
            stats.overload_smoothed_frames,
            stats.limited_samples,
            stats.max_frame_gap_ms,
        ),
    );
}
