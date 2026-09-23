use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};

use rand::random;
use tokio::{
    net::UdpSocket,
    sync::{Mutex, mpsc, oneshot, watch},
    task::JoinHandle,
    time::{Instant as TokioInstant, sleep_until},
};

use super::super::media::{build_rtcp_sender_report, current_unix_time};
use super::super::{
    DISCORD_OPUS_TIMESTAMP_INCREMENT, RTP_AEAD_NONCE_SUFFIX_BYTES, RTP_AEAD_TAG_BYTES,
    RTP_HEADER_EXTENSION_BYTES, RTP_HEADER_MIN_LEN, VoiceConnectionEnd, VoiceDaveState,
    VoiceRuntimeEvent, VoiceSessionDescription, VoiceStatusPublisher, capture,
    dave::VoiceDaveOutboundPayload,
    gateway,
    opus::VoiceOpusEncode,
    preview::StreamPreviewUploader,
    rtp::{VoiceRtpDecryptor, build_voice_rtp_packet_with_marker, looks_like_rtcp_packet},
    system_audio::{self, SYSTEM_AUDIO_FRAME_QUEUE},
};
use super::*;

use crate::{discord::StreamCaptureTarget, logging};

impl BroadcastAudioTransport {
    pub fn new(
        audio_ssrc: u32,
        packet_encryptor: Arc<BroadcastPacketEncryptor>,
        stats: SharedBroadcastSendStats,
    ) -> Self {
        Self {
            audio_ssrc,
            packet_encryptor,
            stats,
            sequence: random(),
            timestamp: random(),
            previous_frame_index: None,
            started: false,
            sent_packets: 0,
            sent_octets: 0,
            next_sender_report_at: Instant::now() + RTCP_SENDER_REPORT_INTERVAL,
        }
    }

    pub fn observe_frame(&mut self, frame_index: u64) {
        let elapsed_frames = broadcast_audio_elapsed_frames(self.previous_frame_index, frame_index);
        self.previous_frame_index = Some(frame_index);
        self.timestamp = self
            .timestamp
            .wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT.wrapping_mul(elapsed_frames));
    }

    pub async fn send(&mut self, socket: &UdpSocket, opus: &[u8]) -> Result<(), String> {
        let packet = build_voice_rtp_packet_with_marker(
            self.sequence,
            self.timestamp,
            self.audio_ssrc,
            !self.started,
            opus,
        )?;
        let encrypted = self.packet_encryptor.encrypt_media_packet(&packet)?;
        socket
            .send(&encrypted)
            .await
            .map_err(|error| format!("broadcast audio UDP send failed: {error}"))?;
        self.sequence = self.sequence.wrapping_add(1);
        self.started = true;
        self.sent_packets = self.sent_packets.wrapping_add(1);
        self.sent_octets = self.sent_octets.wrapping_add(
            u32::try_from(opus.len().min(u32::MAX as usize)).expect("bounded Opus length fits u32"),
        );
        update_broadcast_send_stats(&self.stats, |stats| {
            stats.observe_sent_audio(encrypted.len());
        });
        self.send_sender_report_if_due(socket).await
    }

    pub async fn send_sender_report_if_due(&mut self, socket: &UdpSocket) -> Result<(), String> {
        if Instant::now() < self.next_sender_report_at {
            return Ok(());
        }

        let sender_report = build_rtcp_sender_report(
            self.audio_ssrc,
            current_unix_time(),
            self.timestamp,
            self.sent_packets,
            self.sent_octets,
        );
        let sender_report = self
            .packet_encryptor
            .encrypt_rtcp_packet(&sender_report, "audio RTCP")?;
        socket
            .send(&sender_report)
            .await
            .map_err(|error| format!("broadcast audio RTCP sender report failed: {error}"))?;
        self.next_sender_report_at = Instant::now() + RTCP_SENDER_REPORT_INTERVAL;
        Ok(())
    }
}

pub struct BroadcastVideoTransport {
    pub video: BroadcastVideoSsrcs,
    pub packet_encryptor: Arc<BroadcastPacketEncryptor>,
    pub decryptor: VoiceRtpDecryptor,
    pub video_sequence: u16,
    pub rtx_sequence: u16,
    pub transport_sequence: u16,
    pub video_sent_packets: u32,
    pub video_sent_octets: u32,
    pub pacer: BroadcastRtpPacer,
    pub next_video_sender_report_at: Instant,
    pub last_reported_packet_loss: HashMap<u32, i32>,
    pub history: BroadcastRtpHistory,
    pub stats: SharedBroadcastSendStats,
}

impl BroadcastVideoTransport {
    pub fn new(
        description: &VoiceSessionDescription,
        video: BroadcastVideoSsrcs,
        packet_encryptor: Arc<BroadcastPacketEncryptor>,
        stats: SharedBroadcastSendStats,
    ) -> Result<Self, String> {
        let now = Instant::now();
        Ok(Self {
            video,
            packet_encryptor,
            decryptor: VoiceRtpDecryptor::new(&description.mode, &description.secret_key)?,
            video_sequence: random(),
            rtx_sequence: random(),
            transport_sequence: random(),
            video_sent_packets: 0,
            video_sent_octets: 0,
            pacer: BroadcastRtpPacer::new(now),
            next_video_sender_report_at: now + RTCP_SENDER_REPORT_INTERVAL,
            last_reported_packet_loss: HashMap::new(),
            history: BroadcastRtpHistory::new(),
            stats,
        })
    }

    pub async fn send_frame(
        &mut self,
        socket: &UdpSocket,
        frame: &[u8],
        timestamp: u32,
        is_keyframe: bool,
    ) -> Result<(), String> {
        let packets = packetize_discord_h264_frame(
            frame,
            timestamp,
            self.video.video_ssrc,
            &mut self.video_sequence,
            &mut self.transport_sequence,
        );
        let packet_count = packets.len();
        let estimated_wire_bytes = packets.iter().fold(0usize, |total, packet| {
            total.saturating_add(packet.len() + RTP_AEAD_TAG_BYTES + RTP_AEAD_NONCE_SUFFIX_BYTES)
        });
        let pacing_interval =
            self.pacer
                .pacing_interval(packet_count, estimated_wire_bytes, Instant::now());
        let pacing_started_at = TokioInstant::now();
        let mut wire_bytes = 0usize;

        for (index, packet) in packets.into_iter().enumerate() {
            self.history.remember(packet.clone())?;
            wire_bytes = wire_bytes.saturating_add(self.send_encrypted_rtp(socket, &packet).await?);
            self.video_sent_packets = self.video_sent_packets.wrapping_add(1);

            if let Some(interval) = pacing_interval
                && index + 1 < packet_count
            {
                let gap_count = u32::try_from(index + 1).unwrap_or(u32::MAX);
                sleep_until(pacing_started_at + interval * gap_count).await;
            }
        }

        self.video_sent_octets = self.video_sent_octets.wrapping_add(
            u32::try_from(frame.len().min(u32::MAX as usize))
                .expect("bounded frame length fits u32"),
        );
        update_broadcast_send_stats(&self.stats, |stats| {
            stats.observe_sent_frame(packet_count, wire_bytes, is_keyframe);
        });
        self.send_video_sender_report_if_due(socket, timestamp)
            .await
    }

    pub async fn handle_udp_packet(
        &mut self,
        socket: &UdpSocket,
        packet: &[u8],
    ) -> Result<bool, String> {
        if gateway::parse_udp_ping_response(packet).is_some() {
            return Ok(false);
        }
        if !looks_like_rtcp_packet(packet) {
            return Ok(false);
        }

        let decrypted = match self.decryptor.decrypt_rtcp_feedback(packet) {
            Ok(decrypted) => decrypted,
            Err(error) => {
                logging::debug(
                    "stream",
                    format!("ignoring invalid broadcast RTCP packet: {error}"),
                );
                return Ok(false);
            }
        };
        let feedback = match parse_broadcast_rtcp_feedback(&decrypted, self.video.video_ssrc) {
            Ok(feedback) => feedback,
            Err(error) => {
                logging::debug(
                    "stream",
                    format!("ignoring malformed broadcast RTCP feedback: {error}"),
                );
                return Ok(false);
            }
        };

        let mut new_reported_loss = false;
        for report in &feedback.receiver_reports {
            new_reported_loss |= receiver_report_has_new_loss(
                &mut self.last_reported_packet_loss,
                report.reporter_ssrc,
                report.cumulative_lost,
            );
            if report.fraction_lost > 0 || report.cumulative_lost > 0 {
                logging::debug(
                    "stream",
                    format!(
                        "broadcast receiver report: reporter_ssrc={} fraction_lost={} cumulative_lost={}",
                        report.reporter_ssrc, report.fraction_lost, report.cumulative_lost,
                    ),
                );
            }
        }

        let mut retransmitted_packets = 0usize;
        let mut history_misses = 0usize;
        let retransmission_count = feedback
            .nack_sequences
            .len()
            .min(STREAM_RTX_MAX_RETRANSMISSIONS_PER_FEEDBACK);
        let estimated_wire_bytes = retransmission_count.saturating_mul(
            STREAM_RTP_MAX_PAYLOAD_BYTES
                + RTP_HEADER_MIN_LEN
                + RTP_HEADER_EXTENSION_BYTES
                + STREAM_RTP_EXTENSION_BODY_BYTES
                + STREAM_RTX_ORIGINAL_SEQUENCE_BYTES
                + RTP_AEAD_TAG_BYTES
                + RTP_AEAD_NONCE_SUFFIX_BYTES,
        );
        let pacing_interval =
            self.pacer
                .pacing_interval(retransmission_count, estimated_wire_bytes, Instant::now());
        let pacing_started_at = TokioInstant::now();
        for (index, original_sequence) in feedback
            .nack_sequences
            .iter()
            .copied()
            .take(retransmission_count)
            .enumerate()
        {
            let Some(original) = self.history.get(original_sequence) else {
                history_misses += 1;
                continue;
            };
            let rtx_packet = build_discord_video_rtx_packet(
                original,
                self.video.rtx_ssrc,
                self.rtx_sequence,
                self.transport_sequence,
            )?;
            self.rtx_sequence = self.rtx_sequence.wrapping_add(1);
            self.transport_sequence = self.transport_sequence.wrapping_add(1);
            self.send_encrypted_rtp(socket, &rtx_packet).await?;
            retransmitted_packets += 1;

            if let Some(interval) = pacing_interval
                && index + 1 < retransmission_count
            {
                let gap_count = u32::try_from(index + 1).unwrap_or(u32::MAX);
                sleep_until(pacing_started_at + interval * gap_count).await;
            }
        }

        let request_keyframe = feedback.request_keyframe
            || history_misses > 0
            || new_reported_loss
            || feedback.nack_sequences.len() > retransmission_count;
        update_broadcast_send_stats(&self.stats, |stats| {
            stats.observe_feedback(
                feedback.nack_sequences.len(),
                retransmitted_packets,
                history_misses,
                request_keyframe,
            );
        });
        Ok(request_keyframe)
    }

    pub async fn send_encrypted_rtp(
        &self,
        socket: &UdpSocket,
        packet: &[u8],
    ) -> Result<usize, String> {
        let encrypted = self.packet_encryptor.encrypt_media_packet(packet)?;
        socket
            .send(&encrypted)
            .await
            .map_err(|error| format!("broadcast UDP send failed: {error}"))?;
        Ok(encrypted.len())
    }

    pub async fn send_video_sender_report_if_due(
        &mut self,
        socket: &UdpSocket,
        timestamp: u32,
    ) -> Result<(), String> {
        if Instant::now() < self.next_video_sender_report_at {
            return Ok(());
        }

        let sender_report = build_rtcp_sender_report(
            self.video.video_ssrc,
            current_unix_time(),
            timestamp,
            self.video_sent_packets,
            self.video_sent_octets,
        );
        let sender_report = self
            .packet_encryptor
            .encrypt_rtcp_packet(&sender_report, "video RTCP")?;
        socket
            .send(&sender_report)
            .await
            .map_err(|error| format!("broadcast RTCP sender report failed: {error}"))?;
        self.next_video_sender_report_at = Instant::now() + RTCP_SENDER_REPORT_INTERVAL;
        Ok(())
    }
}

pub struct BroadcastAudioTask {
    pub task: Option<JoinHandle<()>>,
    pub capture: Option<system_audio::SystemAudioCapture>,
}

impl BroadcastAudioTask {
    pub fn disabled() -> Self {
        Self {
            task: None,
            capture: None,
        }
    }

    pub fn start(
        target: &StreamCaptureTarget,
        socket: Arc<UdpSocket>,
        dave_state: Arc<Mutex<VoiceDaveState>>,
        audio_ssrc: u32,
        packet_encryptor: Arc<BroadcastPacketEncryptor>,
        stats: SharedBroadcastSendStats,
    ) -> Result<Self, String> {
        let encoder = VoiceOpusEncode::new_system_audio()
            .map_err(|error| format!("system audio encoder failed: {error}"))?;
        let (frames_tx, frames_rx) = mpsc::channel(SYSTEM_AUDIO_FRAME_QUEUE);
        let capture = system_audio::start_system_audio_capture(target, frames_tx)
            .map_err(|error| format!("system audio capture failed: {error}"))?;
        let capture_stats = capture.stats();
        let transport = BroadcastAudioTransport::new(audio_ssrc, packet_encryptor, stats);
        let task = tokio::spawn(async move {
            if let Err(error) = run_stream_broadcast_audio(
                socket,
                dave_state,
                capture_stats,
                frames_rx,
                encoder,
                transport,
            )
            .await
            {
                logging::error("stream", format!("system audio sender stopped: {error}"));
            }
        });
        Ok(Self {
            task: Some(task),
            capture: Some(capture),
        })
    }

    pub async fn shutdown(&mut self) {
        let Some(task) = self.task.take() else {
            if let Some(capture) = self.capture.take() {
                capture.shutdown().await;
            }
            return;
        };
        task.abort();
        let _ = task.await;
        if let Some(capture) = self.capture.take() {
            capture.shutdown().await;
        }
    }

    pub async fn completion(&mut self) -> Result<(), String> {
        let Some(task) = self.task.as_mut() else {
            return std::future::pending().await;
        };
        let result = task.await;
        self.task.take();
        if let Some(capture) = self.capture.take() {
            capture.shutdown().await;
        }
        match result {
            Ok(()) => Err("system audio sender stopped unexpectedly".to_owned()),
            Err(error) => Err(format!("system audio sender task failed: {error}")),
        }
    }

    pub fn abort(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Some(capture) = self.capture.take() {
            capture.shutdown_in_background();
        }
    }
}

impl Drop for BroadcastAudioTask {
    fn drop(&mut self) {
        self.abort();
    }
}

pub async fn report_system_audio_fallback(
    status_publisher: &VoiceStatusPublisher,
    error: impl AsRef<str>,
) {
    let message = format!(
        "System audio is unavailable. The broadcast will continue with video only. {}",
        error.as_ref()
    );
    logging::error("stream", &message);
    status_publisher
        .publish_stream_broadcast_audio_unavailable(message)
        .await;
}

pub async fn run_stream_broadcast_audio(
    socket: Arc<UdpSocket>,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    capture_stats: Arc<system_audio::SystemAudioCaptureStats>,
    mut frames_rx: mpsc::Receiver<system_audio::SystemAudioFrame>,
    mut encoder: VoiceOpusEncode,
    mut transport: BroadcastAudioTransport,
) -> Result<(), String> {
    // Keep every 20 ms frame in capture order. This task is separate from
    // video packet pacing, so high-motion frames cannot collapse the audio queue.
    while let Some(frame) = frames_rx.recv().await {
        let queue_depth = frames_rx.len();
        let queue_delay = Instant::now().saturating_duration_since(frame.captured_at);
        let capture_drops = capture_stats.dropped_frames();
        update_broadcast_send_stats(&transport.stats, |stats| {
            stats.observe_audio_queue(queue_depth, queue_delay, capture_drops);
        });

        transport.observe_frame(frame.frame_index);
        let opus = match encoder.encode_20ms_i16(&frame.samples) {
            Ok(opus) => opus,
            Err(error) => {
                logging::debug("stream", error);
                continue;
            }
        };
        let dave_payload = dave_state.lock().await.prepare_outbound_opus(&opus);
        let opus = match dave_payload {
            VoiceDaveOutboundPayload::Plain(opus) | VoiceDaveOutboundPayload::Encrypted(opus) => {
                opus
            }
            VoiceDaveOutboundPayload::Blocked(_) => {
                update_broadcast_send_stats(&transport.stats, |stats| {
                    stats.observe_blocked_audio();
                });
                continue;
            }
        };
        transport.send(&socket, &opus).await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_stream_broadcast_media(
    socket: Arc<UdpSocket>,
    description: VoiceSessionDescription,
    mut keyframe_interval_rx: watch::Receiver<Option<u64>>,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    target: StreamCaptureTarget,
    audio_ssrc: u32,
    video: BroadcastVideoSsrcs,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    connection_id: u64,
    stream_key: String,
    status_publisher: VoiceStatusPublisher,
    stream_preview_uploader: StreamPreviewUploader,
    broadcast_captures: StreamBroadcastCaptureRegistry,
    mut stop_rx: oneshot::Receiver<()>,
) -> Result<(), BroadcastConnectionFailure> {
    let mut prepared_capture = broadcast_captures.take(&stream_key).ok_or_else(|| {
        BroadcastConnectionFailure::stop("prepared stream capture is unavailable")
    })?;
    prepared_capture
        .capture
        .handle
        .set_keyframe_interval(*keyframe_interval_rx.borrow_and_update());
    let preview_task = match prepared_capture.preview_task.take() {
        Some(preview_task) => preview_task,
        None => {
            let preview_frames =
                prepared_capture
                    .capture
                    .preview_frames
                    .take()
                    .ok_or_else(|| {
                        BroadcastConnectionFailure::stop("stream preview capture is unavailable")
                    })?;
            stream_preview_uploader.start(stream_key.clone(), preview_frames)
        }
    };
    let packet_encryptor = Arc::new(
        BroadcastPacketEncryptor::new(&description).map_err(BroadcastConnectionFailure::stop)?,
    );
    let stats = Arc::new(StdMutex::new(BroadcastSendStats::new()));
    let mut transport = BroadcastVideoTransport::new(
        &description,
        video,
        Arc::clone(&packet_encryptor),
        Arc::clone(&stats),
    )
    .map_err(BroadcastConnectionFailure::stop)?;
    let mut audio_task = match BroadcastAudioTask::start(
        &target,
        Arc::clone(&socket),
        Arc::clone(&dave_state),
        audio_ssrc,
        Arc::clone(&packet_encryptor),
        Arc::clone(&stats),
    ) {
        Ok(audio_task) => audio_task,
        Err(error) => {
            report_system_audio_fallback(&status_publisher, error).await;
            BroadcastAudioTask::disabled()
        }
    };
    let mut received_packet = vec![0u8; STREAM_UDP_RECEIVE_PACKET_BYTES];
    // Capture and the video transport are ready at this point. System audio is
    // optional because permissions and platform backends may be unavailable.
    // Do not wait for the first video frame to pass DAVE because it can hold
    // outbound media until another participant joins the encrypted session.
    let _ = events_tx.send(VoiceRuntimeEvent::BroadcastStreamConnectionEstablished {
        connection_id,
        stream_key: stream_key.clone(),
    });
    let mut stable_deadline: Option<TokioInstant> = None;
    let mut stable = false;
    let mut keyframe_interval_updates_open = true;

    let result = async {
        loop {
            tokio::select! {
                _ = &mut stop_rx => return Ok(()),
                update = keyframe_interval_rx.changed(), if keyframe_interval_updates_open => {
                    if update.is_err() {
                        keyframe_interval_updates_open = false;
                        continue;
                    }
                    prepared_capture.capture.handle.set_keyframe_interval(
                        *keyframe_interval_rx.borrow_and_update(),
                    );
                }
                audio_result = audio_task.completion() => {
                    if let Err(error) = audio_result {
                        report_system_audio_fallback(&status_publisher, error).await;
                    }
                }
                _ = sleep_until(stable_deadline.unwrap_or_else(TokioInstant::now)),
                    if stable_deadline.is_some() =>
                {
                    stable_deadline = None;
                    stable = true;
                    let _ = events_tx.send(VoiceRuntimeEvent::BroadcastStreamConnectionStable {
                        connection_id,
                        stream_key: stream_key.clone(),
                    });
                }
                frame = prepared_capture.capture.frames.recv() => {
                    let Some(frame) = frame else {
                        return capture_completion_after_frame_channel_closed(
                            &mut prepared_capture.capture.errors,
                        );
                    };
                    let frame = frame.map_err(BroadcastConnectionFailure::stop)?;
                    if !stable && stable_deadline.is_none() {
                        stable_deadline =
                            Some(TokioInstant::now() + STREAM_BROADCAST_CONNECTION_STABLE_INTERVAL);
                    }
                    let dave_payload = dave_state
                        .lock()
                        .await
                        .prepare_outbound_h264(&frame.annex_b);
                    let encrypted_frame = match dave_payload {
                        VoiceDaveOutboundPayload::Plain(frame)
                        | VoiceDaveOutboundPayload::Encrypted(frame) => frame,
                        VoiceDaveOutboundPayload::Blocked(_) => {
                            update_broadcast_send_stats(&stats, |stats| {
                                stats.observe_blocked_frame();
                            });
                            continue;
                        }
                    };
                    transport
                        .send_frame(
                            &socket,
                            &encrypted_frame,
                            frame.timestamp,
                            frame.is_keyframe,
                        )
                        .await?;
                }
                error = prepared_capture.capture.errors.recv() => {
                    return match error {
                        Some(error) => Err(BroadcastConnectionFailure::stop(error)),
                        None => Ok(()),
                    };
                }
                received = socket.recv(&mut received_packet) => {
                    let length = received
                        .map_err(|error| format!("broadcast UDP receive failed: {error}"))?;
                    if transport
                        .handle_udp_packet(&socket, &received_packet[..length])
                        .await?
                    {
                        prepared_capture.capture.handle.request_keyframe();
                    }
                }
            }
        }
    }
    .await;

    audio_task.shutdown().await;
    prepared_capture.preview_task = Some(preview_task);
    let keep_capture = match &result {
        Ok(()) => true,
        Err(error) => error.outcome == VoiceConnectionEnd::Reconnect,
    };
    if keep_capture {
        if let Err(prepared_capture) = broadcast_captures.restore(stream_key, prepared_capture) {
            shutdown_prepared_broadcast_capture(prepared_capture).await;
        }
    } else {
        shutdown_prepared_broadcast_capture(prepared_capture).await;
    }
    result
}

pub async fn shutdown_prepared_broadcast_capture(mut prepared: PreparedBroadcastCapture) {
    if let Some(preview_task) = prepared.preview_task.take() {
        preview_task.shutdown().await;
    }
    prepared.capture.handle.shutdown().await;
}

pub fn broadcast_audio_elapsed_frames(previous: Option<u64>, frame_index: u64) -> u32 {
    previous
        .map(|previous| {
            u32::try_from(frame_index.saturating_sub(previous))
                .unwrap_or(u32::MAX)
                .max(1)
        })
        .unwrap_or(1)
}

pub struct BroadcastRtpPacer {
    pub debt: Duration,
    pub updated_at: Instant,
}

impl BroadcastRtpPacer {
    pub fn new(now: Instant) -> Self {
        Self {
            debt: Duration::ZERO,
            updated_at: now,
        }
    }

    pub fn pacing_interval(
        &mut self,
        packet_count: usize,
        estimated_wire_bytes: usize,
        now: Instant,
    ) -> Option<Duration> {
        // Unused transport time becomes bounded burst credit. This lets an IDR
        // use bandwidth saved by smaller frames without weakening the rolling
        // bitrate limit or accumulating latency in the encoded-frame queue.
        self.debt = self
            .debt
            .saturating_sub(now.saturating_duration_since(self.updated_at));
        self.updated_at = now;
        let frame_budget = duration_for_bitrate(
            estimated_wire_bytes,
            u64::from(capture::STREAM_TRANSPORT_BITRATE),
        );
        self.debt = self.debt.saturating_add(frame_budget);
        let rolling_budget = self.debt.saturating_sub(STREAM_RTP_BURST_CREDIT);
        // A separate burst ceiling prevents saved credit from releasing a large
        // IDR fast enough to cause loss and another keyframe request.
        let burst_budget = duration_for_bitrate(estimated_wire_bytes, STREAM_RTP_MAX_BURST_BITRATE);

        packet_pacing_interval(packet_count, rolling_budget.max(burst_budget))
    }
}

pub fn packet_pacing_interval(
    packet_count: usize,
    rate_limited_budget: Duration,
) -> Option<Duration> {
    let gap_count = u32::try_from(packet_count.checked_sub(1)?).unwrap_or(u32::MAX);
    if gap_count == 0 {
        return None;
    }

    // Small frames keep the prior low-latency spacing. Large hardware IDRs
    // need a longer budget so they do not turn the advertised stream bitrate
    // into a short network burst that loses packets before DAVE decryption.
    let low_latency_interval =
        (STREAM_RTP_SMALL_FRAME_PACING_BUDGET / gap_count).min(STREAM_RTP_MAX_PACKET_SPACING);
    let rate_limited_interval = duration_div_ceil(rate_limited_budget, gap_count);
    Some(low_latency_interval.max(rate_limited_interval))
}
