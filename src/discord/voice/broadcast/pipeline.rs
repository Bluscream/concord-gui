use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use futures::StreamExt;
use rand::random;
use serde_json::{Value, json};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, mpsc, oneshot, watch},
    time::timeout,
};
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message as WsMessage};

use crate::support::tls;
use uuid::Uuid;

use super::super::media::GatewayChildTasks;
use super::super::{
    DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE,
    DISCORD_VOICE_PAYLOAD_TYPE, DiscoveredVoiceAddress, VOICE_OP_READY,
    VOICE_OP_SESSION_DESCRIPTION, VOICE_OP_SESSION_UPDATE, VOICE_OP_SPEAKING,
    VOICE_WEBSOCKET_CONNECT_TIMEOUT, VoiceConnectionEnd, VoiceRuntimeEvent,
    VoiceSessionDescription, VoiceStatusPublisher, capture, gateway,
    preview::StreamPreviewUploader,
    rtp::{VoiceRtpEncryptor, parse_rtp_header},
};
use super::*;

use crate::{
    discord::{StreamCaptureTarget, StreamCaptureTargetKind},
    logging,
};

pub async fn connect_stream_broadcast(
    session: &StreamBroadcastGatewaySession,
    events_tx: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: &VoiceStatusPublisher,
    stream_preview_uploader: StreamPreviewUploader,
    broadcast_captures: StreamBroadcastCaptureRegistry,
    mut stop_rx: oneshot::Receiver<()>,
) -> Result<VoiceConnectionEnd, BroadcastConnectionFailure> {
    let url = gateway::voice_gateway_url(&session.endpoint)?;
    logging::debug("stream", format!("connecting broadcast websocket: {url}"));
    let connector = tls::websocket_connector()?;
    let (ws, response) = timeout(
        VOICE_WEBSOCKET_CONNECT_TIMEOUT,
        connect_async_tls_with_config(&url, None, false, Some(connector)),
    )
    .await
    .map_err(|_| "broadcast websocket connect timed out after 10s".to_owned())?
    .map_err(|error| format!("broadcast websocket connect failed: {error}"))?;
    logging::debug(
        "stream",
        format!(
            "broadcast websocket connected: status={}",
            response.status()
        ),
    );
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let mut gateway_control = gateway::StreamVoiceGatewayControl::new(
        Arc::clone(&writer),
        session.current_user_id,
        &session.rtc_server_id,
    )?;
    let dave_state = gateway_control.dave_state();
    let (media_finished_tx, mut media_finished_rx) =
        mpsc::unbounded_channel::<(u64, Result<(), BroadcastConnectionFailure>)>();
    let mut child_tasks = GatewayChildTasks::default();
    let mut media_generation = 0u64;
    let mut udp_socket: Option<Arc<UdpSocket>> = None;
    let mut ready_audio_ssrc: Option<u32> = None;
    let mut ready_video: Option<BroadcastVideoSsrcs> = None;
    let mut current_description: Option<VoiceSessionDescription> = None;
    let mut keyframe_interval_tx: Option<watch::Sender<Option<u64>>> = None;

    gateway::send_voice_text(&writer, stream_broadcast_identify_payload(session)).await?;
    logging::debug("stream", "broadcast identify sent");

    let result: Result<VoiceConnectionEnd, BroadcastConnectionFailure> = loop {
        let frame = tokio::select! {
            _ = &mut stop_rx => {
                break Ok(VoiceConnectionEnd::Stop);
            }
            _ = gateway_control.heartbeat_timed_out() => {
                break Ok(VoiceConnectionEnd::Reconnect);
            }
            media_result = media_finished_rx.recv(), if child_tasks.has_media() => {
                match media_result {
                    Some((generation, result)) => {
                        let Some(result) = broadcast_media_result_for_generation(
                            media_generation,
                            generation,
                            result,
                        ) else {
                            continue;
                        };
                        match result {
                            Ok(()) => break Ok(VoiceConnectionEnd::Stop),
                            Err(error) => break Err(error),
                        }
                    }
                    None => break Ok(VoiceConnectionEnd::Reconnect),
                }
            }
            frame = reader.next() => frame,
        };
        let Some(frame) = frame else {
            break Ok(VoiceConnectionEnd::Reconnect);
        };
        let frame = frame.map_err(|error| format!("broadcast websocket read failed: {error}"))?;
        match gateway_control.frame_action(&frame).await? {
            gateway::StreamVoiceGatewayFrameAction::Payload => {}
            gateway::StreamVoiceGatewayFrameAction::Continue => continue,
            gateway::StreamVoiceGatewayFrameAction::End(outcome) => break Ok(outcome),
        }
        match frame {
            WsMessage::Text(text) => {
                let value: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("broadcast websocket JSON parse failed: {error}"))?;
                gateway_control.record_sequence(&value).await;
                // `as u8` silently truncated, so `op: 258` arrived as opcode 2
                // and was handled as a different message. Upstream added this
                // guard in v2.5.18; the helper was already here, unused.
                let Some(opcode) = gateway::voice_gateway_opcode(&value) else {
                    logging::debug(
                        "stream",
                        "ignored broadcast gateway payload with invalid opcode",
                    );
                    continue;
                };
                match opcode {
                    VOICE_OP_READY => {
                        let ready = gateway::parse_voice_ready_payload(&value)?;
                        let video = parse_broadcast_video_ssrcs(&value)?;
                        let mode = gateway::choose_encryption_mode(&ready.modes)?;
                        let (socket, discovered) =
                            gateway::discover_voice_udp_address(&ready).await?;
                        gateway::send_voice_text(
                            &writer,
                            stream_broadcast_select_protocol_payload(&discovered, &mode),
                        )
                        .await?;
                        gateway::send_voice_text(
                            &writer,
                            stream_broadcast_speaking_payload(ready.ssrc),
                        )
                        .await?;
                        gateway::send_voice_text(
                            &writer,
                            stream_broadcast_video_payload(ready.ssrc, video),
                        )
                        .await?;
                        {
                            let mut dave = dave_state.lock().await;
                            dave.record_ssrc_user(ready.ssrc, session.current_user_id);
                            dave.record_ssrc_user(video.video_ssrc, session.current_user_id);
                            dave.record_ssrc_user(video.rtx_ssrc, session.current_user_id);
                        }
                        udp_socket = Some(socket);
                        ready_audio_ssrc = Some(ready.ssrc);
                        ready_video = Some(video);
                    }
                    VOICE_OP_SESSION_DESCRIPTION => {
                        let description = gateway::parse_voice_session_description(&value)?;
                        validate_broadcast_video_codec(&description)?;
                        dave_state
                            .lock()
                            .await
                            .apply_protocol_version(description.dave_protocol_version)?;
                        let socket = udp_socket
                            .as_ref()
                            .ok_or_else(|| {
                                "broadcast session description arrived before UDP ready".to_owned()
                            })?
                            .clone();
                        let video = ready_video.ok_or_else(|| {
                            "broadcast session description arrived before video SSRCs".to_owned()
                        })?;
                        let audio_ssrc = ready_audio_ssrc.ok_or_else(|| {
                            "broadcast session description arrived before audio SSRC".to_owned()
                        })?;
                        if current_description.as_ref() == Some(&description) {
                            continue;
                        }
                        let finished = media_finished_tx.clone();
                        let target = session.request.target.clone();
                        let dave_for_media = Arc::clone(&dave_state);
                        let events_for_media = events_tx.clone();
                        let stream_key = session.request.stream_key.clone();
                        let connection_id = session.connection_id;
                        let media_description = description.clone();
                        let media_status_publisher = status_publisher.clone();
                        let preview_uploader = stream_preview_uploader.clone();
                        let captures = broadcast_captures.clone();
                        let (next_keyframe_interval_tx, keyframe_interval_rx) =
                            watch::channel(description.keyframe_interval);
                        media_generation = media_generation.wrapping_add(1).max(1);
                        let generation = media_generation;
                        // The previous media task owns the prepared capture until
                        // its cleanup restores it to the registry.
                        child_tasks.shutdown_media().await;
                        let (media_stop_tx, media_stop_rx) = oneshot::channel();
                        let media_task = tokio::spawn(async move {
                            let result = run_stream_broadcast_media(
                                socket,
                                media_description,
                                keyframe_interval_rx,
                                dave_for_media,
                                target,
                                audio_ssrc,
                                video,
                                events_for_media,
                                connection_id,
                                stream_key,
                                media_status_publisher,
                                preview_uploader,
                                captures,
                                media_stop_rx,
                            )
                            .await;
                            let _ = finished.send((generation, result));
                        });
                        child_tasks.install_media_gracefully(media_task, media_stop_tx);
                        keyframe_interval_tx = Some(next_keyframe_interval_tx);
                        child_tasks
                            .replace_udp_ping(tokio::spawn(gateway::run_voice_udp_ping(
                                Arc::clone(
                                    udp_socket
                                        .as_ref()
                                        .expect("UDP socket exists after readiness check"),
                                ),
                            )))
                            .await;
                        current_description = Some(description);
                    }
                    VOICE_OP_SESSION_UPDATE => {
                        let Some(description) = current_description.as_mut() else {
                            break Err(BroadcastConnectionFailure::reconnect(
                                "broadcast session update arrived before session description",
                            ));
                        };
                        let Some(keyframe_interval_tx) = keyframe_interval_tx.as_ref() else {
                            break Err(BroadcastConnectionFailure::reconnect(
                                "broadcast session update arrived before media startup",
                            ));
                        };
                        apply_broadcast_session_update(&value, description, keyframe_interval_tx)?;
                        logging::debug(
                            "stream",
                            format!("broadcast session updated: {description:?}"),
                        );
                    }
                    other => {
                        if !gateway_control
                            .handle_json_op(other, &value, &mut child_tasks)
                            .await?
                        {
                            logging::debug(
                                "stream",
                                format!("unhandled broadcast gateway op={other}"),
                            );
                        }
                    }
                }
            }
            WsMessage::Binary(payload) => {
                gateway_control.handle_binary(&payload).await?;
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Close(_) | WsMessage::Frame(_) => {
                unreachable!("gateway control frames are handled first")
            }
        }
    };

    child_tasks.shutdown().await;
    result
}

fn validate_broadcast_video_codec(
    description: &VoiceSessionDescription,
) -> Result<(), BroadcastConnectionFailure> {
    if description
        .video_codec
        .as_deref()
        .is_some_and(|codec| !codec.eq_ignore_ascii_case("H264"))
    {
        return Err(BroadcastConnectionFailure::stop(format!(
            "stream selected unsupported video codec: {}",
            description.video_codec.as_deref().unwrap_or("none")
        )));
    }
    Ok(())
}

pub(super) fn apply_broadcast_session_update(
    value: &Value,
    description: &mut VoiceSessionDescription,
    keyframe_interval_tx: &watch::Sender<Option<u64>>,
) -> Result<(), BroadcastConnectionFailure> {
    gateway::apply_voice_session_update(value, description)?;
    validate_broadcast_video_codec(description)?;
    keyframe_interval_tx.send_replace(description.keyframe_interval);
    Ok(())
}

pub fn broadcast_media_result_for_generation(
    current_generation: u64,
    result_generation: u64,
    result: Result<(), BroadcastConnectionFailure>,
) -> Option<Result<(), BroadcastConnectionFailure>> {
    (current_generation == result_generation).then_some(result)
}

pub fn capture_completion_after_frame_channel_closed(
    errors_rx: &mut mpsc::UnboundedReceiver<String>,
) -> Result<(), BroadcastConnectionFailure> {
    // The capture worker sends its error before dropping the frame sender.
    // Preserve it when both channel events become ready at the same time.
    match errors_rx.try_recv() {
        Ok(error) => Err(BroadcastConnectionFailure::stop(error)),
        Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => Ok(()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BroadcastVideoSsrcs {
    pub video_ssrc: u32,
    pub rtx_ssrc: u32,
}

pub struct BroadcastSendStats {
    pub window_started_at: Instant,
    pub sent_audio_frames: u64,
    pub blocked_audio_frames: u64,
    pub sent_audio_bytes: u64,
    pub max_audio_queue_depth: usize,
    pub max_audio_queue_delay: Duration,
    pub audio_capture_drops: u64,
    pub sent_frames: u64,
    pub sent_keyframes: u64,
    pub blocked_frames: u64,
    pub sent_packets: u64,
    pub sent_bytes: u64,
    pub retransmitted_packets: u64,
    pub feedback_packets: u64,
    pub nack_requests: u64,
    pub keyframe_requests: u64,
    pub history_misses: u64,
    pub max_frame_packets: usize,
}

impl BroadcastSendStats {
    pub fn new() -> Self {
        Self {
            window_started_at: Instant::now(),
            sent_audio_frames: 0,
            blocked_audio_frames: 0,
            sent_audio_bytes: 0,
            max_audio_queue_depth: 0,
            max_audio_queue_delay: Duration::ZERO,
            audio_capture_drops: 0,
            sent_frames: 0,
            sent_keyframes: 0,
            blocked_frames: 0,
            sent_packets: 0,
            sent_bytes: 0,
            retransmitted_packets: 0,
            feedback_packets: 0,
            nack_requests: 0,
            keyframe_requests: 0,
            history_misses: 0,
            max_frame_packets: 0,
        }
    }

    pub fn observe_sent_frame(&mut self, packets: usize, bytes: usize, is_keyframe: bool) {
        self.sent_frames += 1;
        self.sent_keyframes += u64::from(is_keyframe);
        self.sent_packets = self.sent_packets.saturating_add(packets as u64);
        self.sent_bytes = self.sent_bytes.saturating_add(bytes as u64);
        self.max_frame_packets = self.max_frame_packets.max(packets);
        self.log_if_due();
    }

    pub fn observe_sent_audio(&mut self, bytes: usize) {
        self.sent_audio_frames += 1;
        self.sent_audio_bytes = self.sent_audio_bytes.saturating_add(bytes as u64);
        self.log_if_due();
    }

    pub fn observe_audio_queue(
        &mut self,
        queue_depth: usize,
        queue_delay: Duration,
        capture_drops: u64,
    ) {
        self.max_audio_queue_depth = self.max_audio_queue_depth.max(queue_depth);
        self.max_audio_queue_delay = self.max_audio_queue_delay.max(queue_delay);
        self.audio_capture_drops = capture_drops;
        self.log_if_due();
    }

    pub fn observe_blocked_audio(&mut self) {
        self.blocked_audio_frames += 1;
        self.log_if_due();
    }

    pub fn observe_blocked_frame(&mut self) {
        self.blocked_frames += 1;
        self.log_if_due();
    }

    pub fn observe_feedback(
        &mut self,
        nack_requests: usize,
        retransmitted_packets: usize,
        history_misses: usize,
        requested_keyframe: bool,
    ) {
        self.feedback_packets += 1;
        self.nack_requests = self.nack_requests.saturating_add(nack_requests as u64);
        self.retransmitted_packets = self
            .retransmitted_packets
            .saturating_add(retransmitted_packets as u64);
        self.history_misses = self.history_misses.saturating_add(history_misses as u64);
        self.keyframe_requests += u64::from(requested_keyframe);
        self.log_if_due();
    }

    pub fn log_if_due(&mut self) {
        let elapsed = self.window_started_at.elapsed();
        if elapsed < BROADCAST_SEND_STATS_INTERVAL {
            return;
        }

        let seconds = elapsed.as_secs_f64().max(f64::EPSILON);
        logging::debug(
            "stream",
            format!(
                "broadcast send stats: elapsed_ms={} audio_fps={:.1} audio_blocked={} audio_kbps={:.1} audio_queue_max={} audio_queue_max_ms={:.1} audio_capture_drops={} video_fps={:.1} keyframes={} video_blocked={} video_packets_per_second={:.1} video_wire_mbps={:.2} max_frame_packets={} feedback_packets={} nack_requests={} rtx_packets={} history_misses={} keyframe_requests={}",
                elapsed.as_millis(),
                self.sent_audio_frames as f64 / seconds,
                self.blocked_audio_frames,
                self.sent_audio_bytes as f64 * 8.0 / seconds / 1_000.0,
                self.max_audio_queue_depth,
                self.max_audio_queue_delay.as_secs_f64() * 1_000.0,
                self.audio_capture_drops,
                self.sent_frames as f64 / seconds,
                self.sent_keyframes,
                self.blocked_frames,
                self.sent_packets as f64 / seconds,
                self.sent_bytes as f64 * 8.0 / seconds / 1_000_000.0,
                self.max_frame_packets,
                self.feedback_packets,
                self.nack_requests,
                self.retransmitted_packets,
                self.history_misses,
                self.keyframe_requests,
            ),
        );

        *self = Self::new();
    }
}

pub type SharedBroadcastSendStats = Arc<StdMutex<BroadcastSendStats>>;

pub fn update_broadcast_send_stats(
    stats: &SharedBroadcastSendStats,
    update: impl FnOnce(&mut BroadcastSendStats),
) {
    let mut stats = stats
        .lock()
        .expect("broadcast send statistics lock is not poisoned");
    update(&mut stats);
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct BroadcastRtcpFeedback {
    pub nack_sequences: Vec<u16>,
    pub request_keyframe: bool,
    pub receiver_reports: Vec<BroadcastReceiverReport>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct BroadcastReceiverReport {
    pub reporter_ssrc: u32,
    pub fraction_lost: u8,
    pub cumulative_lost: i32,
}

pub struct BroadcastRtpHistory {
    pub packets: VecDeque<(u16, Vec<u8>)>,
}

impl BroadcastRtpHistory {
    pub fn new() -> Self {
        Self {
            packets: VecDeque::with_capacity(STREAM_RTP_HISTORY_CAPACITY),
        }
    }

    pub fn remember(&mut self, packet: Vec<u8>) -> Result<(), String> {
        let sequence = parse_rtp_header(&packet)?.sequence;
        if self.packets.len() == STREAM_RTP_HISTORY_CAPACITY {
            self.packets.pop_front();
        }
        self.packets.push_back((sequence, packet));
        Ok(())
    }

    pub fn get(&self, sequence: u16) -> Option<&[u8]> {
        let (base_sequence, _) = self.packets.front()?;
        let offset = usize::from(sequence.wrapping_sub(*base_sequence));
        self.packets
            .get(offset)
            .filter(|(stored_sequence, _)| *stored_sequence == sequence)
            .map(|(_, packet)| packet.as_slice())
    }
}

pub fn parse_broadcast_video_ssrcs(value: &Value) -> Result<BroadcastVideoSsrcs, String> {
    let streams = value
        .get("d")
        .and_then(|data| data.get("streams"))
        .and_then(Value::as_array)
        .ok_or_else(|| "broadcast ready missing video streams".to_owned())?;
    let stream = streams
        .iter()
        .find(|stream| stream.get("rid").and_then(Value::as_str) == Some(STREAM_RID))
        .or_else(|| streams.first())
        .ok_or_else(|| "broadcast ready missing video stream".to_owned())?;
    let video_ssrc = stream
        .get("ssrc")
        .and_then(Value::as_u64)
        .and_then(|ssrc| u32::try_from(ssrc).ok())
        .ok_or_else(|| "broadcast ready missing video SSRC".to_owned())?;
    let rtx_ssrc = stream
        .get("rtx_ssrc")
        .and_then(Value::as_u64)
        .and_then(|ssrc| u32::try_from(ssrc).ok())
        .or_else(|| video_ssrc.checked_add(1))
        .ok_or_else(|| "broadcast ready has no usable RTX SSRC".to_owned())?;
    Ok(BroadcastVideoSsrcs {
        video_ssrc,
        rtx_ssrc,
    })
}

/// What Discord calls the stream being published.
///
/// A camera and a shared screen are different types on the wire, and sending
/// "screen" for a camera announces the wrong thing about the feed - Discord's
/// own clients decide layout and quality from it. Taken from the capture
/// target rather than fixed, which it used to be.
pub fn stream_kind(target: &StreamCaptureTarget) -> &'static str {
    match target.kind {
        StreamCaptureTargetKind::Camera => "video",
        _ => "screen",
    }
}

pub fn stream_broadcast_identify_payload(session: &StreamBroadcastGatewaySession) -> String {
    json!({
        "op": 0,
        "d": {
            "server_id": session.rtc_server_id,
            "user_id": session.current_user_id.to_string(),
            "channel_id": session.rtc_channel_id.to_string(),
            "session_id": session.session_id,
            "token": session.token,
            "video": true,
            "streams": [{
                "type": stream_kind(&session.request.target),
                "rid": STREAM_RID,
                "quality": 100,
            }],
            "max_dave_protocol_version": davey::DAVE_PROTOCOL_VERSION,
        },
    })
    .to_string()
}

pub fn stream_broadcast_select_protocol_payload(
    discovered: &DiscoveredVoiceAddress,
    mode: &str,
) -> String {
    json!({
        "op": 1,
        "d": {
            "protocol": "udp",
            "data": {
                "address": discovered.address,
                "port": discovered.port,
                "mode": mode,
            },
            "codecs": [
                {
                    "name": "opus",
                    "type": "audio",
                    "priority": 1000,
                    "payload_type": DISCORD_VOICE_PAYLOAD_TYPE,
                    "encode": true,
                    "decode": false,
                },
                {
                    "name": "H264",
                    "type": "video",
                    "priority": 1000,
                    "payload_type": DISCORD_STREAM_VIDEO_PAYLOAD_TYPE,
                    "rtx_payload_type": DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE,
                    "encode": true,
                    "decode": false,
                },
            ],
            "rtc_connection_id": Uuid::new_v4().to_string(),
        },
    })
    .to_string()
}

pub fn stream_broadcast_video_payload(audio_ssrc: u32, video: BroadcastVideoSsrcs) -> String {
    json!({
        "op": 12,
        "d": {
            "audio_ssrc": audio_ssrc,
            "video_ssrc": video.video_ssrc,
            "rtx_ssrc": video.rtx_ssrc,
            "streams": [{
                "type": "video",
                "rid": STREAM_RID,
                "ssrc": video.video_ssrc,
                "rtx_ssrc": video.rtx_ssrc,
                "active": true,
                "quality": 100,
                "max_bitrate": capture::STREAM_TRANSPORT_BITRATE,
                "max_framerate": capture::STREAM_CAPTURE_FPS,
                "max_resolution": {
                    "type": "fixed",
                    "width": capture::STREAM_CAPTURE_WIDTH,
                    "height": capture::STREAM_CAPTURE_HEIGHT,
                },
            }],
        },
    })
    .to_string()
}

pub fn stream_broadcast_speaking_payload(audio_ssrc: u32) -> String {
    json!({
        "op": VOICE_OP_SPEAKING,
        "d": {
            "speaking": SOUNDSHARE_SPEAKING_FLAG,
            "delay": 0,
            "ssrc": audio_ssrc,
        },
    })
    .to_string()
}

pub struct BroadcastPacketEncryptor {
    pub encryptor: VoiceRtpEncryptor,
    // Audio, video, retransmission, and RTCP packets share one key. A single
    // atomic sequence prevents nonce reuse when their tasks send concurrently.
    pub nonce_suffix: AtomicU32,
}

impl BroadcastPacketEncryptor {
    pub fn new(description: &VoiceSessionDescription) -> Result<Self, String> {
        Self::with_nonce(&description.mode, &description.secret_key, random::<u32>())
    }

    pub fn with_nonce(mode: &str, secret_key: &[u8], nonce_suffix: u32) -> Result<Self, String> {
        Ok(Self {
            encryptor: VoiceRtpEncryptor::new(mode, secret_key)?,
            nonce_suffix: AtomicU32::new(nonce_suffix),
        })
    }

    pub fn encrypt_media_packet(&self, packet: &[u8]) -> Result<Vec<u8>, String> {
        self.encryptor
            .encrypt_media_packet(packet, self.take_nonce("RTP")?)
    }

    pub fn encrypt_rtcp_packet(&self, packet: &[u8], packet_kind: &str) -> Result<Vec<u8>, String> {
        self.encryptor
            .encrypt_rtcp_feedback(packet, self.take_nonce(packet_kind)?)
    }

    pub fn take_nonce(&self, packet_kind: &str) -> Result<[u8; 4], String> {
        self.nonce_suffix
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |nonce| {
                nonce.checked_add(1)
            })
            .map(u32::to_be_bytes)
            .map_err(|_| format!("broadcast {packet_kind} nonce exhausted"))
    }
}

pub struct BroadcastAudioTransport {
    pub audio_ssrc: u32,
    pub packet_encryptor: Arc<BroadcastPacketEncryptor>,
    pub stats: SharedBroadcastSendStats,
    pub sequence: u16,
    pub timestamp: u32,
    pub previous_frame_index: Option<u64>,
    pub started: bool,
    pub sent_packets: u32,
    pub sent_octets: u32,
    pub next_sender_report_at: Instant,
}
