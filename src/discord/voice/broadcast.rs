use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::{SinkExt, StreamExt};
use rand::random;
use serde_json::{Value, json};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, mpsc, oneshot},
    task::JoinHandle,
    time::{Instant as TokioInstant, sleep_until, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use uuid::Uuid;

use super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS;
use super::{
    DISCORD_OPUS_FRAME_DURATION, DISCORD_OPUS_TIMESTAMP_INCREMENT,
    DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE,
    DISCORD_VOICE_PAYLOAD_TYPE, DiscoveredVoiceAddress, RTP_HEADER_MIN_LEN, RTP_VERSION,
    StreamBroadcastRequest, StreamCreateInfo, StreamServerInfo, VOICE_OP_CLIENT_DISCONNECT,
    VOICE_OP_CLIENT_FLAGS, VOICE_OP_CLIENT_PLATFORM, VOICE_OP_CLIENTS_CONNECT,
    VOICE_OP_DAVE_EXECUTE_TRANSITION, VOICE_OP_DAVE_PREPARE_EPOCH,
    VOICE_OP_DAVE_PREPARE_TRANSITION, VOICE_OP_HEARTBEAT_ACK, VOICE_OP_HELLO,
    VOICE_OP_MEDIA_SINK_WANTS, VOICE_OP_READY, VOICE_OP_SESSION_DESCRIPTION, VOICE_OP_SPEAKING,
    VOICE_WEBSOCKET_CONNECT_TIMEOUT, VoiceConnectionEnd, VoiceDaveState, VoiceHeartbeatAckState,
    VoiceHeartbeatTimeout, VoiceRuntimeEvent, VoiceScope, VoiceSessionDescription,
    VoiceStatusPublisher, capture,
    dave::VoiceDaveOutboundPayload,
    gateway,
    opus::VoiceOpusEncode,
    rtp::{
        VoiceRtpDecryptor, VoiceRtpEncryptor, build_voice_rtp_packet_with_marker,
        looks_like_rtcp_packet, parse_rtp_header,
    },
    system_audio::{self, SYSTEM_AUDIO_FRAME_QUEUE},
};
use crate::{
    discord::{
        ids::{
            Id,
            marker::{ChannelMarker, UserMarker},
        },
        voice::VoiceStateInfo,
    },
    logging,
};

const STREAM_RID: &str = "100";
const STREAM_RTP_MAX_PAYLOAD_BYTES: usize = 1_100;
const RTP_EXTENSION_PROFILE_ONE_BYTE: u16 = 0xbede;
const RTP_EXTENSION_TRANSPORT_SEQUENCE: u8 = 5;
const RTP_EXTENSION_PLAYOUT_DELAY: u8 = 6;
const RTP_EXTENSION_VIDEO_CONTENT_TYPE: u8 = 7;
const RTP_EXTENSION_RID: u8 = 11;
const VIDEO_CONTENT_TYPE_SCREEN: u8 = 1;
const RTCP_SENDER_REPORT_INTERVAL: Duration = Duration::from_secs(5);
const BROADCAST_SEND_STATS_INTERVAL: Duration = Duration::from_secs(5);
const STREAM_RTP_PACING_BUDGET: Duration = Duration::from_millis(25);
const STREAM_RTP_MAX_PACKET_SPACING: Duration = Duration::from_millis(2);
const STREAM_RTP_HISTORY_CAPACITY: usize = 2_048;
const STREAM_RTX_MAX_RETRANSMISSIONS_PER_FEEDBACK: usize = 128;
const STREAM_UDP_RECEIVE_PACKET_BYTES: usize = 2_048;
const SOUNDSHARE_SPEAKING_FLAG: u8 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StreamBroadcastGatewaySession {
    pub(super) connection_id: u64,
    pub(super) request: StreamBroadcastRequest,
    pub(super) current_user_id: Id<UserMarker>,
    pub(super) session_id: String,
    pub(super) rtc_server_id: String,
    pub(super) rtc_channel_id: Id<ChannelMarker>,
    pub(super) endpoint: String,
    pub(super) token: String,
}

#[derive(Default)]
struct BroadcastChildTasks {
    heartbeat: Option<JoinHandle<()>>,
    keepalive: Option<JoinHandle<()>>,
    media: Option<JoinHandle<()>>,
}

impl BroadcastChildTasks {
    async fn replace_heartbeat(&mut self, task: JoinHandle<()>) {
        Self::replace(&mut self.heartbeat, task).await;
    }

    async fn replace_keepalive(&mut self, task: JoinHandle<()>) {
        Self::replace(&mut self.keepalive, task).await;
    }

    async fn replace_media(&mut self, task: JoinHandle<()>) {
        Self::replace(&mut self.media, task).await;
    }

    async fn replace(slot: &mut Option<JoinHandle<()>>, task: JoinHandle<()>) {
        if let Some(previous) = slot.take() {
            previous.abort();
            let _ = previous.await;
        }
        *slot = Some(task);
    }

    async fn shutdown(&mut self) {
        let tasks = [
            self.heartbeat.take(),
            self.keepalive.take(),
            self.media.take(),
        ];
        for task in tasks.iter().flatten() {
            task.abort();
        }
        for task in tasks.into_iter().flatten() {
            let _ = task.await;
        }
    }

    fn abort_all(&mut self) {
        for task in [
            self.heartbeat.take(),
            self.keepalive.take(),
            self.media.take(),
        ]
        .into_iter()
        .flatten()
        {
            task.abort();
        }
    }
}

impl Drop for BroadcastChildTasks {
    fn drop(&mut self) {
        // Normal shutdown awaits every child. This fallback also stops children
        // if the parent broadcast future itself is forcefully cancelled.
        self.abort_all();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedBroadcastVoiceState {
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    session_id: String,
}

#[derive(Default)]
pub(super) struct StreamBroadcastRuntimeState {
    current_user_id: Option<Id<UserMarker>>,
    current_voice: Option<ObservedBroadcastVoiceState>,
    requested: Option<StreamBroadcastRequest>,
    create: Option<StreamCreateInfo>,
    server: Option<StreamServerInfo>,
    active: Option<StreamBroadcastGatewaySession>,
    reconnect_attempts: u8,
    next_connection_id: u64,
}

#[derive(Default)]
pub(super) struct StreamBroadcastRuntimeUpdate {
    pub(super) close_stream_key: Option<String>,
    pub(super) send_delete: bool,
    pub(super) connect: Option<StreamBroadcastGatewaySession>,
    pub(super) error: Option<String>,
}

impl StreamBroadcastRuntimeState {
    pub(super) fn apply(&mut self, event: &VoiceRuntimeEvent) -> StreamBroadcastRuntimeUpdate {
        let mut update = StreamBroadcastRuntimeUpdate::default();
        match event {
            VoiceRuntimeEvent::CurrentUserReady(user_id) => self.current_user_id = *user_id,
            VoiceRuntimeEvent::VoiceState(state) => self.record_voice_state(state, &mut update),
            VoiceRuntimeEvent::BroadcastStreamRequested(request) => {
                if self
                    .requested
                    .as_ref()
                    .is_some_and(|current| current.stream_key != request.stream_key)
                {
                    update.close_stream_key =
                        self.active.take().map(|active| active.request.stream_key);
                    update.send_delete = update.close_stream_key.is_some();
                }
                self.reconnect_attempts = 0;
                self.requested = Some(request.clone());
                self.create = None;
                self.server = None;
            }
            VoiceRuntimeEvent::BroadcastStreamCancelled { stream_key } => {
                self.clear_matching(stream_key, &mut update, false);
            }
            VoiceRuntimeEvent::BroadcastStreamStopRequested { stream_key } => {
                self.clear_matching(stream_key, &mut update, true);
                if update.close_stream_key.is_none() {
                    update.close_stream_key = Some(stream_key.clone());
                    update.send_delete = true;
                }
            }
            VoiceRuntimeEvent::StreamCreate(stream) => {
                if self
                    .requested
                    .as_ref()
                    .is_some_and(|request| request.stream_key == stream.stream_key)
                {
                    self.create = Some(stream.clone());
                }
            }
            VoiceRuntimeEvent::StreamServer(server) => {
                if self
                    .requested
                    .as_ref()
                    .is_some_and(|request| request.stream_key == server.stream_key)
                {
                    if self.active.as_ref().is_some_and(|active| {
                        !server.matches_connection(&active.endpoint, &active.token)
                    }) {
                        update.close_stream_key =
                            self.active.take().map(|active| active.request.stream_key);
                    }
                    self.server = Some(server.clone());
                }
            }
            VoiceRuntimeEvent::StreamDelete(stream) => {
                if self
                    .requested
                    .as_ref()
                    .is_some_and(|request| request.stream_key == stream.stream_key)
                    && (!stream.reason.is_empty() || stream.unavailable)
                {
                    let reason = if stream.reason.is_empty() {
                        "stream unavailable"
                    } else {
                        stream.reason.as_str()
                    };
                    update.error = Some(format!("Could not broadcast stream: {reason}"));
                }
                self.clear_matching(&stream.stream_key, &mut update, false);
            }
            VoiceRuntimeEvent::BroadcastStreamConnectionEstablished {
                connection_id,
                stream_key,
            } => {
                if self.active.as_ref().is_some_and(|active| {
                    active.connection_id == *connection_id
                        && active.request.stream_key == *stream_key
                }) {
                    self.reconnect_attempts = 0;
                }
            }
            VoiceRuntimeEvent::BroadcastStreamConnectionEnded {
                connection_id,
                stream_key,
                outcome,
            } => {
                if self.active.as_ref().is_some_and(|active| {
                    active.connection_id == *connection_id
                        && active.request.stream_key == *stream_key
                }) {
                    self.active = None;
                    if *outcome == VoiceConnectionEnd::Stop
                        || self.reconnect_attempts >= MAX_VOICE_RECONNECT_ATTEMPTS
                    {
                        self.requested = None;
                        self.create = None;
                        self.server = None;
                        self.reconnect_attempts = 0;
                        update.close_stream_key = Some(stream_key.clone());
                        update.send_delete = true;
                    } else {
                        self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
                    }
                }
            }
            VoiceRuntimeEvent::Shutdown => {
                update.close_stream_key = self
                    .active
                    .take()
                    .map(|active| active.request.stream_key)
                    .or_else(|| {
                        self.requested
                            .as_ref()
                            .map(|request| request.stream_key.clone())
                    });
                update.send_delete = update.close_stream_key.is_some();
                self.requested = None;
                self.create = None;
                self.server = None;
                self.reconnect_attempts = 0;
            }
            _ => {}
        }

        if self.active.is_none() {
            update.connect = self.connect_if_ready();
        }
        update
    }

    fn record_voice_state(
        &mut self,
        state: &VoiceStateInfo,
        update: &mut StreamBroadcastRuntimeUpdate,
    ) {
        if self.current_user_id != Some(state.user_id) {
            return;
        }
        let Some(channel_id) = state.channel_id else {
            self.current_voice = None;
            update.close_stream_key = self
                .active
                .take()
                .map(|active| active.request.stream_key)
                .or_else(|| {
                    self.requested
                        .as_ref()
                        .map(|request| request.stream_key.clone())
                });
            update.send_delete = update.close_stream_key.is_some();
            self.requested = None;
            self.create = None;
            self.server = None;
            self.reconnect_attempts = 0;
            return;
        };
        let Some(scope) = state.scope() else {
            return;
        };
        let Some(session_id) = state
            .session_id
            .as_ref()
            .filter(|session_id| !session_id.is_empty())
        else {
            return;
        };
        self.current_voice = Some(ObservedBroadcastVoiceState {
            scope,
            channel_id,
            session_id: session_id.clone(),
        });
        if self
            .requested
            .as_ref()
            .is_some_and(|request| request.scope != scope || request.channel_id != channel_id)
        {
            update.close_stream_key = self
                .active
                .take()
                .map(|active| active.request.stream_key)
                .or_else(|| {
                    self.requested
                        .as_ref()
                        .map(|request| request.stream_key.clone())
                });
            update.send_delete = update.close_stream_key.is_some();
            self.requested = None;
            self.create = None;
            self.server = None;
            self.reconnect_attempts = 0;
        }
    }

    fn clear_matching(
        &mut self,
        stream_key: &str,
        update: &mut StreamBroadcastRuntimeUpdate,
        send_delete: bool,
    ) {
        let matches_requested = self
            .requested
            .as_ref()
            .is_some_and(|request| request.stream_key == stream_key);
        if matches_requested {
            self.requested = None;
            self.create = None;
            self.server = None;
            self.reconnect_attempts = 0;
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.request.stream_key == stream_key)
        {
            self.active = None;
        }
        if matches_requested || send_delete {
            update.close_stream_key = Some(stream_key.to_owned());
            update.send_delete = send_delete;
        }
    }

    fn connect_if_ready(&mut self) -> Option<StreamBroadcastGatewaySession> {
        let request = self.requested.as_ref()?;
        let current_voice = self.current_voice.as_ref()?;
        if request.scope != current_voice.scope || request.channel_id != current_voice.channel_id {
            return None;
        }
        let create = self.create.as_ref()?;
        let server = self.server.as_ref()?;
        if create.stream_key != request.stream_key || server.stream_key != request.stream_key {
            return None;
        }
        let endpoint = server.endpoint.as_ref()?.trim_end_matches('/').to_owned();
        if endpoint.is_empty() || server.token.is_empty() {
            return None;
        }

        self.next_connection_id = self.next_connection_id.wrapping_add(1).max(1);
        let session = StreamBroadcastGatewaySession {
            connection_id: self.next_connection_id,
            request: request.clone(),
            current_user_id: self.current_user_id?,
            session_id: current_voice.session_id.clone(),
            rtc_server_id: create.rtc_server_id.clone(),
            rtc_channel_id: create.rtc_channel_id,
            endpoint,
            token: server.token.clone(),
        };
        self.active = Some(session.clone());
        Some(session)
    }
}

pub(super) async fn run_stream_broadcast_session(
    session: StreamBroadcastGatewaySession,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: VoiceStatusPublisher,
    stop_rx: oneshot::Receiver<()>,
) {
    let outcome = match connect_stream_broadcast(&session, &events_tx, stop_rx).await {
        Ok(outcome) => outcome,
        Err(error) => {
            logging::error("stream", &error);
            status_publisher
                .publish_error(format!("Could not broadcast stream: {error}"))
                .await;
            VoiceConnectionEnd::Stop
        }
    };
    let _ = events_tx.send(VoiceRuntimeEvent::BroadcastStreamConnectionEnded {
        connection_id: session.connection_id,
        stream_key: session.request.stream_key.clone(),
        outcome,
    });
}

async fn connect_stream_broadcast(
    session: &StreamBroadcastGatewaySession,
    events_tx: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    mut stop_rx: oneshot::Receiver<()>,
) -> Result<VoiceConnectionEnd, String> {
    let url = gateway::voice_gateway_url(&session.endpoint)?;
    logging::debug("stream", format!("connecting broadcast websocket: {url}"));
    let (ws, response) = timeout(VOICE_WEBSOCKET_CONNECT_TIMEOUT, connect_async(&url))
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
    let last_sequence = Arc::new(Mutex::new(None));
    let heartbeat_ack = Arc::new(Mutex::new(VoiceHeartbeatAckState::default()));
    let (heartbeat_timeout_tx, mut heartbeat_timeout_rx) =
        mpsc::unbounded_channel::<VoiceHeartbeatTimeout>();
    let (media_finished_tx, mut media_finished_rx) =
        mpsc::unbounded_channel::<Result<(), String>>();
    let mut child_tasks = BroadcastChildTasks::default();
    let dave_group_id = session
        .rtc_server_id
        .parse::<u64>()
        .ok()
        .and_then(|server_id| server_id.checked_sub(1))
        .ok_or_else(|| "stream RTC server id is not a valid DAVE group id".to_owned())?;
    let dave_state = Arc::new(Mutex::new(VoiceDaveState::new_for_identity(
        session.current_user_id,
        dave_group_id,
    )));
    let mut udp_socket: Option<Arc<UdpSocket>> = None;
    let mut ready_audio_ssrc: Option<u32> = None;
    let mut ready_video: Option<BroadcastVideoSsrcs> = None;
    let mut current_description: Option<VoiceSessionDescription> = None;

    gateway::send_voice_text(&writer, stream_broadcast_identify_payload(session)).await?;
    logging::debug("stream", "broadcast identify sent");

    let result = loop {
        let frame = tokio::select! {
            _ = &mut stop_rx => {
                break Ok(VoiceConnectionEnd::Stop);
            }
            _ = heartbeat_timeout_rx.recv() => {
                break Ok(VoiceConnectionEnd::Reconnect);
            }
            media_result = media_finished_rx.recv(), if child_tasks.media.is_some() => {
                match media_result {
                    Some(Ok(())) => break Ok(VoiceConnectionEnd::Stop),
                    Some(Err(error)) => break Err(error),
                    None => break Ok(VoiceConnectionEnd::Reconnect),
                }
            }
            frame = reader.next() => frame,
        };
        let Some(frame) = frame else {
            break Ok(VoiceConnectionEnd::Reconnect);
        };
        let frame = frame.map_err(|error| format!("broadcast websocket read failed: {error}"))?;
        match frame {
            WsMessage::Text(text) => {
                let value: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("broadcast websocket JSON parse failed: {error}"))?;
                if let Some(sequence) = value.get("seq").and_then(Value::as_i64) {
                    *last_sequence.lock().await = Some(sequence);
                }
                let opcode = value.get("op").and_then(Value::as_u64).unwrap_or_default() as u8;
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
                        if description
                            .video_codec
                            .as_deref()
                            .is_some_and(|codec| !codec.eq_ignore_ascii_case("H264"))
                        {
                            break Err(format!(
                                "stream selected unsupported video codec: {}",
                                description.video_codec.as_deref().unwrap_or("none")
                            ));
                        }
                        if let Some(version) = description.dave_protocol_version {
                            let version = u16::try_from(version)
                                .map_err(|_| "DAVE protocol version does not fit u16".to_owned())?;
                            dave_state.lock().await.reinit(version)?;
                        }
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
                        child_tasks
                            .replace_media(tokio::spawn(async move {
                                let result = run_stream_broadcast_media(
                                    socket,
                                    media_description,
                                    dave_for_media,
                                    target,
                                    audio_ssrc,
                                    video,
                                    events_for_media,
                                    connection_id,
                                    stream_key,
                                )
                                .await;
                                let _ = finished.send(result);
                            }))
                            .await;
                        child_tasks
                            .replace_keepalive(tokio::spawn(gateway::run_voice_udp_keepalive(
                                Arc::clone(
                                    udp_socket
                                        .as_ref()
                                        .expect("UDP socket exists after readiness check"),
                                ),
                            )))
                            .await;
                        current_description = Some(description);
                    }
                    VOICE_OP_HEARTBEAT_ACK => {
                        heartbeat_ack.lock().await.mark_acknowledged();
                    }
                    VOICE_OP_HELLO => {
                        let interval = value
                            .get("d")
                            .and_then(|data| data.get("heartbeat_interval"))
                            .and_then(Value::as_u64)
                            .map(Duration::from_millis)
                            .ok_or_else(|| {
                                "broadcast hello missing heartbeat interval".to_owned()
                            })?;
                        heartbeat_ack.lock().await.reset();
                        child_tasks
                            .replace_heartbeat(tokio::spawn(gateway::run_voice_heartbeat(
                                Arc::clone(&writer),
                                interval,
                                Arc::clone(&last_sequence),
                                Arc::clone(&heartbeat_ack),
                                heartbeat_timeout_tx.clone(),
                                0,
                            )))
                            .await;
                    }
                    VOICE_OP_SPEAKING => {
                        dave_state.lock().await.handle_speaking_op(&value);
                    }
                    VOICE_OP_CLIENTS_CONNECT
                    | VOICE_OP_CLIENT_DISCONNECT
                    | VOICE_OP_MEDIA_SINK_WANTS
                    | VOICE_OP_CLIENT_FLAGS
                    | VOICE_OP_CLIENT_PLATFORM
                    | VOICE_OP_DAVE_PREPARE_TRANSITION
                    | VOICE_OP_DAVE_EXECUTE_TRANSITION
                    | VOICE_OP_DAVE_PREPARE_EPOCH => {
                        dave_state
                            .lock()
                            .await
                            .handle_json_op(&writer, opcode, &value)
                            .await?;
                    }
                    other => {
                        logging::debug("stream", format!("unhandled broadcast gateway op={other}"));
                    }
                }
            }
            WsMessage::Binary(payload) => {
                let frame = gateway::parse_voice_binary_frame(&payload)?;
                *last_sequence.lock().await = Some(frame.sequence);
                dave_state
                    .lock()
                    .await
                    .handle_binary_frame(&writer, frame)
                    .await?;
            }
            WsMessage::Ping(payload) => {
                writer
                    .lock()
                    .await
                    .send(WsMessage::Pong(payload))
                    .await
                    .map_err(|error| format!("broadcast websocket pong failed: {error}"))?;
            }
            WsMessage::Close(frame) => {
                let action = frame
                    .as_ref()
                    .map(|frame| gateway::voice_close_action(u16::from(frame.code)))
                    .unwrap_or(gateway::VoiceCloseAction::Reconnect);
                break Ok(match action {
                    gateway::VoiceCloseAction::Stop => VoiceConnectionEnd::Stop,
                    gateway::VoiceCloseAction::Resume | gateway::VoiceCloseAction::Reconnect => {
                        VoiceConnectionEnd::Reconnect
                    }
                });
            }
            WsMessage::Pong(_) | WsMessage::Frame(_) => {}
        }
    };

    child_tasks.shutdown().await;
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BroadcastVideoSsrcs {
    video_ssrc: u32,
    rtx_ssrc: u32,
}

struct BroadcastSendStats {
    window_started_at: Instant,
    sent_audio_frames: u64,
    blocked_audio_frames: u64,
    sent_audio_bytes: u64,
    max_audio_queue_depth: usize,
    max_audio_queue_delay: Duration,
    audio_capture_drops: u64,
    sent_frames: u64,
    sent_keyframes: u64,
    blocked_frames: u64,
    sent_packets: u64,
    sent_bytes: u64,
    retransmitted_packets: u64,
    feedback_packets: u64,
    nack_requests: u64,
    keyframe_requests: u64,
    history_misses: u64,
    max_frame_packets: usize,
}

impl BroadcastSendStats {
    fn new() -> Self {
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

    fn observe_sent_frame(&mut self, packets: usize, bytes: usize, is_keyframe: bool) {
        self.sent_frames += 1;
        self.sent_keyframes += u64::from(is_keyframe);
        self.sent_packets = self.sent_packets.saturating_add(packets as u64);
        self.sent_bytes = self.sent_bytes.saturating_add(bytes as u64);
        self.max_frame_packets = self.max_frame_packets.max(packets);
        self.log_if_due();
    }

    fn observe_sent_audio(&mut self, bytes: usize) {
        self.sent_audio_frames += 1;
        self.sent_audio_bytes = self.sent_audio_bytes.saturating_add(bytes as u64);
        self.log_if_due();
    }

    fn observe_audio_queue(
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

    fn observe_blocked_audio(&mut self) {
        self.blocked_audio_frames += 1;
        self.log_if_due();
    }

    fn observe_blocked_frame(&mut self) {
        self.blocked_frames += 1;
        self.log_if_due();
    }

    fn observe_feedback(
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

    fn log_if_due(&mut self) {
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

type SharedBroadcastSendStats = Arc<StdMutex<BroadcastSendStats>>;

fn update_broadcast_send_stats(
    stats: &SharedBroadcastSendStats,
    update: impl FnOnce(&mut BroadcastSendStats),
) {
    let mut stats = stats
        .lock()
        .expect("broadcast send statistics lock is not poisoned");
    update(&mut stats);
}

#[derive(Debug, Default, Eq, PartialEq)]
struct BroadcastRtcpFeedback {
    nack_sequences: Vec<u16>,
    request_keyframe: bool,
    receiver_reports: Vec<BroadcastReceiverReport>,
}

#[derive(Debug, Eq, PartialEq)]
struct BroadcastReceiverReport {
    reporter_ssrc: u32,
    fraction_lost: u8,
    cumulative_lost: i32,
}

struct BroadcastRtpHistory {
    packets: VecDeque<(u16, Vec<u8>)>,
}

impl BroadcastRtpHistory {
    fn new() -> Self {
        Self {
            packets: VecDeque::with_capacity(STREAM_RTP_HISTORY_CAPACITY),
        }
    }

    fn remember(&mut self, packet: Vec<u8>) -> Result<(), String> {
        let sequence = parse_rtp_header(&packet)?.sequence;
        if self.packets.len() == STREAM_RTP_HISTORY_CAPACITY {
            self.packets.pop_front();
        }
        self.packets.push_back((sequence, packet));
        Ok(())
    }

    fn get(&self, sequence: u16) -> Option<Vec<u8>> {
        self.packets
            .iter()
            .rev()
            .find(|(stored_sequence, _)| *stored_sequence == sequence)
            .map(|(_, packet)| packet.clone())
    }
}

fn parse_broadcast_video_ssrcs(value: &Value) -> Result<BroadcastVideoSsrcs, String> {
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

fn stream_broadcast_identify_payload(session: &StreamBroadcastGatewaySession) -> String {
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
                "type": "screen",
                "rid": STREAM_RID,
                "quality": 100,
            }],
            "max_dave_protocol_version": davey::DAVE_PROTOCOL_VERSION,
        },
    })
    .to_string()
}

fn stream_broadcast_select_protocol_payload(
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

fn stream_broadcast_video_payload(audio_ssrc: u32, video: BroadcastVideoSsrcs) -> String {
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
                "max_bitrate": capture::STREAM_CAPTURE_BITRATE,
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

fn stream_broadcast_speaking_payload(audio_ssrc: u32) -> String {
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

struct BroadcastPacketEncryptor {
    encryptor: VoiceRtpEncryptor,
    // Audio, video, retransmission, and RTCP packets share one key. A single
    // atomic sequence prevents nonce reuse when their tasks send concurrently.
    nonce_suffix: AtomicU32,
}

impl BroadcastPacketEncryptor {
    fn new(description: &VoiceSessionDescription) -> Result<Self, String> {
        Self::with_nonce(&description.mode, &description.secret_key, random::<u32>())
    }

    fn with_nonce(mode: &str, secret_key: &[u8], nonce_suffix: u32) -> Result<Self, String> {
        Ok(Self {
            encryptor: VoiceRtpEncryptor::new(mode, secret_key)?,
            nonce_suffix: AtomicU32::new(nonce_suffix),
        })
    }

    fn encrypt_media_packet(&self, packet: &[u8]) -> Result<Vec<u8>, String> {
        self.encryptor
            .encrypt_media_packet(packet, self.take_nonce("RTP")?)
    }

    fn encrypt_rtcp_packet(&self, packet: &[u8], packet_kind: &str) -> Result<Vec<u8>, String> {
        self.encryptor
            .encrypt_rtcp_feedback(packet, self.take_nonce(packet_kind)?)
    }

    fn take_nonce(&self, packet_kind: &str) -> Result<[u8; 4], String> {
        self.nonce_suffix
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |nonce| {
                nonce.checked_add(1)
            })
            .map(u32::to_be_bytes)
            .map_err(|_| format!("broadcast {packet_kind} nonce exhausted"))
    }
}

struct BroadcastAudioTransport {
    audio_ssrc: u32,
    packet_encryptor: Arc<BroadcastPacketEncryptor>,
    stats: SharedBroadcastSendStats,
    sequence: u16,
    timestamp: u32,
    previous_frame_at: Option<Instant>,
    started: bool,
    sent_packets: u32,
    sent_octets: u32,
    next_sender_report_at: Instant,
}

impl BroadcastAudioTransport {
    fn new(
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
            previous_frame_at: None,
            started: false,
            sent_packets: 0,
            sent_octets: 0,
            next_sender_report_at: Instant::now() + RTCP_SENDER_REPORT_INTERVAL,
        }
    }

    fn observe_frame(&mut self, captured_at: Instant) {
        let elapsed_frames = broadcast_audio_elapsed_frames(self.previous_frame_at, captured_at);
        self.previous_frame_at = Some(captured_at);
        self.timestamp = self
            .timestamp
            .wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT.wrapping_mul(elapsed_frames));
    }

    async fn send(&mut self, socket: &UdpSocket, opus: &[u8]) -> Result<(), String> {
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

    async fn send_sender_report_if_due(&mut self, socket: &UdpSocket) -> Result<(), String> {
        if Instant::now() < self.next_sender_report_at {
            return Ok(());
        }

        let sender_report = build_rtcp_sender_report(
            self.audio_ssrc,
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

struct BroadcastVideoTransport {
    video: BroadcastVideoSsrcs,
    packet_encryptor: Arc<BroadcastPacketEncryptor>,
    decryptor: VoiceRtpDecryptor,
    video_sequence: u16,
    rtx_sequence: u16,
    transport_sequence: u16,
    video_sent_packets: u32,
    video_sent_octets: u32,
    next_video_sender_report_at: Instant,
    last_reported_packet_loss: HashMap<u32, i32>,
    history: BroadcastRtpHistory,
    stats: SharedBroadcastSendStats,
}

impl BroadcastVideoTransport {
    fn new(
        description: &VoiceSessionDescription,
        video: BroadcastVideoSsrcs,
        packet_encryptor: Arc<BroadcastPacketEncryptor>,
        stats: SharedBroadcastSendStats,
    ) -> Result<Self, String> {
        Ok(Self {
            video,
            packet_encryptor,
            decryptor: VoiceRtpDecryptor::new(&description.mode, &description.secret_key)?,
            video_sequence: random(),
            rtx_sequence: random(),
            transport_sequence: random(),
            video_sent_packets: 0,
            video_sent_octets: 0,
            next_video_sender_report_at: Instant::now() + RTCP_SENDER_REPORT_INTERVAL,
            last_reported_packet_loss: HashMap::new(),
            history: BroadcastRtpHistory::new(),
            stats,
        })
    }

    async fn send_frame(
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
        let pacing_interval = rtp_packet_pacing_interval(packet_count);
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

    async fn handle_udp_packet(
        &mut self,
        socket: &UdpSocket,
        packet: &[u8],
    ) -> Result<bool, String> {
        if gateway::parse_udp_keepalive_response(packet).is_some() {
            return Ok(false);
        }
        if !looks_like_rtcp_packet(packet) {
            return Ok(false);
        }

        let decrypted = if is_plain_rtcp_compound_packet(packet) {
            packet.to_vec()
        } else {
            match self.decryptor.decrypt_rtcp_feedback(packet) {
                Ok(decrypted) => decrypted,
                Err(error) => {
                    logging::debug(
                        "stream",
                        format!("ignoring invalid broadcast RTCP packet: {error}"),
                    );
                    return Ok(false);
                }
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
        let pacing_interval = rtp_packet_pacing_interval(retransmission_count);
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
                &original,
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

    async fn send_encrypted_rtp(&self, socket: &UdpSocket, packet: &[u8]) -> Result<usize, String> {
        let encrypted = self.packet_encryptor.encrypt_media_packet(packet)?;
        socket
            .send(&encrypted)
            .await
            .map_err(|error| format!("broadcast UDP send failed: {error}"))?;
        Ok(encrypted.len())
    }

    async fn send_video_sender_report_if_due(
        &mut self,
        socket: &UdpSocket,
        timestamp: u32,
    ) -> Result<(), String> {
        if Instant::now() < self.next_video_sender_report_at {
            return Ok(());
        }

        let sender_report = build_rtcp_sender_report(
            self.video.video_ssrc,
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

#[derive(Default)]
struct BroadcastAudioTask {
    task: Option<JoinHandle<()>>,
}

impl BroadcastAudioTask {
    fn start(
        socket: Arc<UdpSocket>,
        dave_state: Arc<Mutex<VoiceDaveState>>,
        audio_ssrc: u32,
        packet_encryptor: Arc<BroadcastPacketEncryptor>,
        stats: SharedBroadcastSendStats,
    ) -> Self {
        let (frames_tx, frames_rx) = mpsc::channel(SYSTEM_AUDIO_FRAME_QUEUE);
        let capture = match system_audio::start_system_audio_capture(frames_tx) {
            Ok(capture) => capture,
            Err(error) => {
                logging::error(
                    "stream",
                    format!("system audio unavailable, continuing video-only: {error}"),
                );
                return Self::default();
            }
        };
        let encoder = match VoiceOpusEncode::new_system_audio() {
            Ok(encoder) => encoder,
            Err(error) => {
                logging::error(
                    "stream",
                    format!("system audio encoder unavailable, continuing video-only: {error}"),
                );
                return Self::default();
            }
        };
        let transport = BroadcastAudioTransport::new(audio_ssrc, packet_encryptor, stats);
        let task = tokio::spawn(async move {
            if let Err(error) = run_stream_broadcast_audio(
                socket,
                dave_state,
                Some(capture),
                frames_rx,
                encoder,
                transport,
            )
            .await
            {
                logging::error(
                    "stream",
                    format!("system audio sender stopped, continuing video-only: {error}"),
                );
            }
        });
        Self { task: Some(task) }
    }

    async fn shutdown(&mut self) {
        let Some(task) = self.task.take() else {
            return;
        };
        task.abort();
        let _ = task.await;
    }

    fn abort(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Drop for BroadcastAudioTask {
    fn drop(&mut self) {
        self.abort();
    }
}

async fn run_stream_broadcast_audio(
    socket: Arc<UdpSocket>,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    capture: Option<system_audio::SystemAudioCapture>,
    mut frames_rx: mpsc::Receiver<system_audio::SystemAudioFrame>,
    mut encoder: VoiceOpusEncode,
    mut transport: BroadcastAudioTransport,
) -> Result<(), String> {
    // Keep every 20 ms frame in capture order. This task is separate from
    // video packet pacing, so high-motion frames cannot collapse the audio queue.
    while let Some(frame) = frames_rx.recv().await {
        let queue_depth = frames_rx.len();
        let queue_delay = Instant::now().saturating_duration_since(frame.captured_at);
        let capture_drops = capture
            .as_ref()
            .map_or(0, system_audio::SystemAudioCapture::dropped_frames);
        update_broadcast_send_stats(&transport.stats, |stats| {
            stats.observe_audio_queue(queue_depth, queue_delay, capture_drops);
        });

        transport.observe_frame(frame.captured_at);
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
async fn run_stream_broadcast_media(
    socket: Arc<UdpSocket>,
    description: VoiceSessionDescription,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    target: super::StreamCaptureTarget,
    audio_ssrc: u32,
    video: BroadcastVideoSsrcs,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    connection_id: u64,
    stream_key: String,
) -> Result<(), String> {
    let (frames_tx, mut frames_rx) = mpsc::channel(2);
    let capture = capture::start_stream_capture(target, frames_tx)?;
    let packet_encryptor = Arc::new(BroadcastPacketEncryptor::new(&description)?);
    let stats = Arc::new(StdMutex::new(BroadcastSendStats::new()));
    let mut audio_task = BroadcastAudioTask::start(
        Arc::clone(&socket),
        Arc::clone(&dave_state),
        audio_ssrc,
        Arc::clone(&packet_encryptor),
        Arc::clone(&stats),
    );
    let mut transport =
        BroadcastVideoTransport::new(&description, video, packet_encryptor, Arc::clone(&stats))?;
    let mut received_packet = vec![0u8; STREAM_UDP_RECEIVE_PACKET_BYTES];
    let mut ready_sent = false;

    let result = async {
        loop {
            tokio::select! {
                frame = frames_rx.recv() => {
                    let Some(frame) = frame else {
                        return Ok(());
                    };
                    let frame = frame?;
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
                    if !ready_sent {
                        ready_sent = true;
                        let _ = events_tx.send(
                            VoiceRuntimeEvent::BroadcastStreamConnectionEstablished {
                                connection_id,
                                stream_key: stream_key.clone(),
                            },
                        );
                    }
                }
                received = socket.recv(&mut received_packet) => {
                    let length = received
                        .map_err(|error| format!("broadcast UDP receive failed: {error}"))?;
                    if transport
                        .handle_udp_packet(&socket, &received_packet[..length])
                        .await?
                    {
                        capture.request_keyframe();
                    }
                }
            }
        }
    }
    .await;

    audio_task.shutdown().await;
    result
}

fn build_rtcp_sender_report(
    ssrc: u32,
    rtp_timestamp: u32,
    packet_count: u32,
    octet_count: u32,
) -> Vec<u8> {
    const NTP_UNIX_EPOCH_OFFSET_SECONDS: u64 = 2_208_988_800;
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ntp_seconds = elapsed
        .as_secs()
        .wrapping_add(NTP_UNIX_EPOCH_OFFSET_SECONDS) as u32;
    let ntp_fraction = ((u64::from(elapsed.subsec_nanos())) << 32) / 1_000_000_000;

    let mut packet = Vec::with_capacity(28);
    packet.extend_from_slice(&[RTP_VERSION << 6, 200]);
    packet.extend_from_slice(&6u16.to_be_bytes());
    packet.extend_from_slice(&ssrc.to_be_bytes());
    packet.extend_from_slice(&ntp_seconds.to_be_bytes());
    packet.extend_from_slice(&(ntp_fraction as u32).to_be_bytes());
    packet.extend_from_slice(&rtp_timestamp.to_be_bytes());
    packet.extend_from_slice(&packet_count.to_be_bytes());
    packet.extend_from_slice(&octet_count.to_be_bytes());
    packet
}

fn broadcast_audio_elapsed_frames(previous: Option<Instant>, captured_at: Instant) -> u32 {
    previous
        .map(|previous| {
            let elapsed_us = captured_at.saturating_duration_since(previous).as_micros();
            let frame_us = DISCORD_OPUS_FRAME_DURATION.as_micros();
            let rounded_frames = elapsed_us.saturating_add(frame_us / 2) / frame_us;
            u32::try_from(rounded_frames).unwrap_or(u32::MAX).max(1)
        })
        .unwrap_or(1)
}

fn rtp_packet_pacing_interval(packet_count: usize) -> Option<Duration> {
    let gap_count = u32::try_from(packet_count.checked_sub(1)?).unwrap_or(u32::MAX);
    if gap_count == 0 {
        return None;
    }
    Some((STREAM_RTP_PACING_BUDGET / gap_count).min(STREAM_RTP_MAX_PACKET_SPACING))
}

fn receiver_report_has_new_loss(
    previous: &mut HashMap<u32, i32>,
    reporter_ssrc: u32,
    current: i32,
) -> bool {
    let previous = previous.insert(reporter_ssrc, current);
    current > 0 && previous.is_none_or(|previous| current > previous)
}

fn is_plain_rtcp_compound_packet(packet: &[u8]) -> bool {
    let mut offset = 0usize;
    while offset < packet.len() {
        let Ok(packet_len) = rtcp_packet_len(&packet[offset..]) else {
            return false;
        };
        offset = offset.saturating_add(packet_len);
    }
    offset == packet.len()
}

fn parse_broadcast_rtcp_feedback(
    packet: &[u8],
    video_ssrc: u32,
) -> Result<BroadcastRtcpFeedback, String> {
    let mut feedback = BroadcastRtcpFeedback::default();
    let mut offset = 0usize;

    while offset < packet.len() {
        let packet_len = rtcp_packet_len(&packet[offset..])?;
        let rtcp = &packet[offset..offset + packet_len];
        let feedback_format = rtcp[0] & 0x1f;

        match rtcp[1] {
            201 => parse_receiver_report(rtcp, video_ssrc, &mut feedback)?,
            205 if feedback_format == 1 => parse_generic_nack(rtcp, video_ssrc, &mut feedback)?,
            206 if feedback_format == 1 => {
                if rtcp.len() < 12 {
                    return Err("RTCP PLI packet is too short".to_owned());
                }
                let media_ssrc =
                    u32::from_be_bytes(rtcp[8..12].try_into().expect("validated PLI media SSRC"));
                feedback.request_keyframe |= media_ssrc == video_ssrc;
            }
            206 if feedback_format == 4 => {
                parse_full_intra_request(rtcp, video_ssrc, &mut feedback)?
            }
            _ => {}
        }
        offset += packet_len;
    }

    feedback.nack_sequences.sort_unstable();
    feedback.nack_sequences.dedup();
    Ok(feedback)
}

fn rtcp_packet_len(packet: &[u8]) -> Result<usize, String> {
    if packet.len() < 4 {
        return Err("RTCP packet is shorter than its header".to_owned());
    }
    if packet[0] >> 6 != RTP_VERSION {
        return Err("RTCP packet has unsupported version".to_owned());
    }
    if !(192..=223).contains(&packet[1]) {
        return Err("RTCP packet has invalid packet type".to_owned());
    }
    let packet_len = (usize::from(u16::from_be_bytes([packet[2], packet[3]])) + 1) * 4;
    if packet_len < 4 || packet_len > packet.len() {
        return Err("RTCP packet length exceeds received data".to_owned());
    }
    Ok(packet_len)
}

fn parse_receiver_report(
    packet: &[u8],
    video_ssrc: u32,
    feedback: &mut BroadcastRtcpFeedback,
) -> Result<(), String> {
    let report_count = usize::from(packet[0] & 0x1f);
    let expected_len = 8usize.saturating_add(report_count.saturating_mul(24));
    if packet.len() < expected_len {
        return Err("RTCP receiver report is shorter than its report blocks".to_owned());
    }

    let reporter_ssrc = u32::from_be_bytes(
        packet[4..8]
            .try_into()
            .expect("validated receiver report sender SSRC"),
    );
    for index in 0..report_count {
        let start = 8 + index * 24;
        let reported_ssrc = u32::from_be_bytes(
            packet[start..start + 4]
                .try_into()
                .expect("validated receiver report SSRC"),
        );
        if reported_ssrc != video_ssrc {
            continue;
        }

        let fraction_lost = packet[start + 4];
        let cumulative = u32::from(packet[start + 5]) << 16
            | u32::from(packet[start + 6]) << 8
            | u32::from(packet[start + 7]);
        let cumulative_lost = if cumulative & 0x80_0000 != 0 {
            (cumulative | 0xff00_0000) as i32
        } else {
            cumulative as i32
        };
        feedback.receiver_reports.push(BroadcastReceiverReport {
            reporter_ssrc,
            fraction_lost,
            cumulative_lost,
        });
    }
    Ok(())
}

fn parse_generic_nack(
    packet: &[u8],
    video_ssrc: u32,
    feedback: &mut BroadcastRtcpFeedback,
) -> Result<(), String> {
    if packet.len() < 12 || !(packet.len() - 12).is_multiple_of(4) {
        return Err("RTCP NACK packet has invalid feedback length".to_owned());
    }
    let media_ssrc =
        u32::from_be_bytes(packet[8..12].try_into().expect("validated NACK media SSRC"));
    if media_ssrc != video_ssrc {
        return Ok(());
    }

    for entry in packet[12..].chunks_exact(4) {
        let packet_id = u16::from_be_bytes([entry[0], entry[1]]);
        let bitmask = u16::from_be_bytes([entry[2], entry[3]]);
        feedback.nack_sequences.push(packet_id);
        for bit in 0..16 {
            if bitmask & (1 << bit) != 0 {
                feedback
                    .nack_sequences
                    .push(packet_id.wrapping_add(bit + 1));
            }
        }
    }
    Ok(())
}

fn parse_full_intra_request(
    packet: &[u8],
    video_ssrc: u32,
    feedback: &mut BroadcastRtcpFeedback,
) -> Result<(), String> {
    if packet.len() < 12 || !(packet.len() - 12).is_multiple_of(8) {
        return Err("RTCP FIR packet has invalid feedback length".to_owned());
    }
    feedback.request_keyframe |= packet[12..].chunks_exact(8).any(|entry| {
        u32::from_be_bytes(entry[..4].try_into().expect("validated FIR media SSRC")) == video_ssrc
    });
    Ok(())
}

fn packetize_discord_h264_frame(
    frame: &[u8],
    timestamp: u32,
    ssrc: u32,
    sequence: &mut u16,
    transport_sequence: &mut u16,
) -> Vec<Vec<u8>> {
    let mut payloads = Vec::new();
    for nal in annex_b_nals(frame) {
        if nal.len() <= STREAM_RTP_MAX_PAYLOAD_BYTES {
            payloads.push(nal.to_vec());
            continue;
        }
        let Some((&nal_header, body)) = nal.split_first() else {
            continue;
        };
        let fu_indicator = (nal_header & 0xe0) | 28;
        let nal_type = nal_header & 0x1f;
        let chunks = body.chunks(STREAM_RTP_MAX_PAYLOAD_BYTES - 2);
        let chunk_count = chunks.len();
        for (index, chunk) in chunks.enumerate() {
            let mut payload = Vec::with_capacity(chunk.len() + 2);
            payload.push(fu_indicator);
            payload.push(
                nal_type
                    | if index == 0 { 0x80 } else { 0 }
                    | if index + 1 == chunk_count { 0x40 } else { 0 },
            );
            payload.extend_from_slice(chunk);
            payloads.push(payload);
        }
    }
    let payload_count = payloads.len();
    payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            let packet = build_discord_video_rtp_packet(
                *sequence,
                timestamp,
                ssrc,
                index + 1 == payload_count,
                *transport_sequence,
                &payload,
            );
            *sequence = sequence.wrapping_add(1);
            *transport_sequence = transport_sequence.wrapping_add(1);
            packet
        })
        .collect()
}

fn build_discord_video_rtp_packet(
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    marker: bool,
    transport_sequence: u16,
    payload: &[u8],
) -> Vec<u8> {
    build_discord_video_rtp_packet_with_payload_type(
        sequence,
        timestamp,
        ssrc,
        marker,
        transport_sequence,
        DISCORD_STREAM_VIDEO_PAYLOAD_TYPE,
        payload,
    )
}

fn build_discord_video_rtx_packet(
    original: &[u8],
    rtx_ssrc: u32,
    rtx_sequence: u16,
    transport_sequence: u16,
) -> Result<Vec<u8>, String> {
    let header = parse_rtp_header(original)?;
    let original_payload = original
        .get(header.payload_offset..)
        .ok_or_else(|| "original RTP packet is missing media payload".to_owned())?;
    let mut rtx_payload = Vec::with_capacity(2 + original_payload.len());
    rtx_payload.extend_from_slice(&header.sequence.to_be_bytes());
    rtx_payload.extend_from_slice(original_payload);
    Ok(build_discord_video_rtp_packet_with_payload_type(
        rtx_sequence,
        header.timestamp,
        rtx_ssrc,
        header.marker,
        transport_sequence,
        DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE,
        &rtx_payload,
    ))
}

#[allow(clippy::too_many_arguments)]
fn build_discord_video_rtp_packet_with_payload_type(
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    marker: bool,
    transport_sequence: u16,
    payload_type: u8,
    payload: &[u8],
) -> Vec<u8> {
    let mut extensions = Vec::with_capacity(16);
    push_one_byte_extension(
        &mut extensions,
        RTP_EXTENSION_TRANSPORT_SEQUENCE,
        &transport_sequence.to_be_bytes(),
    );
    push_one_byte_extension(&mut extensions, RTP_EXTENSION_PLAYOUT_DELAY, &[0, 0, 0]);
    push_one_byte_extension(
        &mut extensions,
        RTP_EXTENSION_VIDEO_CONTENT_TYPE,
        &[VIDEO_CONTENT_TYPE_SCREEN],
    );
    push_one_byte_extension(&mut extensions, RTP_EXTENSION_RID, STREAM_RID.as_bytes());
    while extensions.len() % 4 != 0 {
        extensions.push(0);
    }

    let mut packet = Vec::with_capacity(RTP_HEADER_MIN_LEN + 4 + extensions.len() + payload.len());
    packet.push((RTP_VERSION << 6) | 0x10);
    packet.push((u8::from(marker) << 7) | payload_type);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&timestamp.to_be_bytes());
    packet.extend_from_slice(&ssrc.to_be_bytes());
    packet.extend_from_slice(&RTP_EXTENSION_PROFILE_ONE_BYTE.to_be_bytes());
    packet.extend_from_slice(
        &u16::try_from(extensions.len() / 4)
            .expect("RTP extension word count fits u16")
            .to_be_bytes(),
    );
    packet.extend_from_slice(&extensions);
    packet.extend_from_slice(payload);
    packet
}

fn push_one_byte_extension(output: &mut Vec<u8>, id: u8, value: &[u8]) {
    debug_assert!((1..=14).contains(&id));
    debug_assert!((1..=16).contains(&value.len()));
    output.push((id << 4) | (u8::try_from(value.len()).expect("extension length fits u8") - 1));
    output.extend_from_slice(value);
}

fn annex_b_nals(frame: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut index = 0usize;
    while index + 3 <= frame.len() {
        let start_len = if frame.get(index..index + 4) == Some(&[0, 0, 0, 1]) {
            4
        } else if frame.get(index..index + 3) == Some(&[0, 0, 1]) {
            3
        } else {
            index += 1;
            continue;
        };
        starts.push((index, start_len));
        index += start_len;
    }
    if starts.is_empty() {
        return (!frame.is_empty()).then_some(frame).into_iter().collect();
    }
    starts
        .iter()
        .enumerate()
        .filter_map(|(position, (start, start_len))| {
            let nal_start = start + start_len;
            let nal_end = starts
                .get(position + 1)
                .map(|(next, _)| *next)
                .unwrap_or(frame.len());
            (nal_start < nal_end).then_some(&frame[nal_start..nal_end])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::{
        ids::marker::GuildMarker,
        voice::{
            AEAD_XCHACHA20_POLY1305_RTPSIZE, DISCORD_OPUS_20MS_STEREO_SAMPLES, StreamCaptureTarget,
            StreamCaptureTargetKind, parse_rtp_header,
        },
    };

    fn request() -> StreamBroadcastRequest {
        StreamBroadcastRequest {
            stream_key: "guild:10:20:30".to_owned(),
            scope: VoiceScope::Guild(Id::new(10)),
            channel_id: Id::new(20),
            target: StreamCaptureTarget {
                kind: StreamCaptureTargetKind::Display,
                id: 1,
                title: "Screen: Display".to_owned(),
            },
        }
    }

    fn session() -> StreamBroadcastGatewaySession {
        StreamBroadcastGatewaySession {
            connection_id: 1,
            request: request(),
            current_user_id: Id::new(30),
            session_id: "session".to_owned(),
            rtc_server_id: "11".to_owned(),
            rtc_channel_id: Id::new(20),
            endpoint: "streams.example".to_owned(),
            token: "token".to_owned(),
        }
    }

    fn connected_broadcast_runtime() -> (StreamBroadcastRuntimeState, StreamBroadcastGatewaySession)
    {
        let mut state = StreamBroadcastRuntimeState::default();
        state.apply(&VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(30))));
        state.apply(&VoiceRuntimeEvent::VoiceState(VoiceStateInfo {
            guild_id: Some(Id::new(10)),
            channel_id: Some(Id::new(20)),
            user_id: Id::new(30),
            session_id: Some("session".to_owned()),
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        }));
        state.apply(&VoiceRuntimeEvent::BroadcastStreamRequested(request()));
        state.apply(&VoiceRuntimeEvent::StreamCreate(StreamCreateInfo {
            stream_key: request().stream_key,
            rtc_server_id: "11".to_owned(),
            rtc_channel_id: Id::new(20),
            viewer_ids: Vec::new(),
            paused: false,
        }));
        let update = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: request().stream_key,
            endpoint: Some("streams.example".to_owned()),
            token: "token".to_owned(),
        }));
        let session = update.connect.expect("broadcast session should be ready");
        (state, session)
    }

    fn broadcast_connection_ended(
        session: &StreamBroadcastGatewaySession,
        outcome: VoiceConnectionEnd,
    ) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::BroadcastStreamConnectionEnded {
            connection_id: session.connection_id,
            stream_key: session.request.stream_key.clone(),
            outcome,
        }
    }

    fn voice_description() -> VoiceSessionDescription {
        VoiceSessionDescription {
            mode: AEAD_XCHACHA20_POLY1305_RTPSIZE.to_owned(),
            secret_key: vec![9; 32],
            dave_protocol_version: None,
            video_codec: Some("H264".to_owned()),
        }
    }

    struct DropNotice(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropNotice {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    async fn broadcast_child_tasks_abort_when_their_owner_is_dropped() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _notice = DropNotice(Some(dropped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx
            .await
            .expect("test broadcast child task should start");
        let mut tasks = BroadcastChildTasks::default();
        tasks.replace_media(task).await;

        drop(tasks);

        timeout(Duration::from_secs(1), dropped_rx)
            .await
            .expect("broadcast child task should stop")
            .expect("broadcast child task should report cleanup");
    }

    #[tokio::test]
    async fn broadcast_audio_task_aborts_when_its_owner_is_dropped() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _notice = DropNotice(Some(dropped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx
            .await
            .expect("test broadcast audio task should start");
        let audio_task = BroadcastAudioTask { task: Some(task) };

        drop(audio_task);

        timeout(Duration::from_secs(1), dropped_rx)
            .await
            .expect("broadcast audio task should stop")
            .expect("broadcast audio task should report cleanup");
    }

    #[test]
    fn broadcast_runtime_connects_only_after_voice_create_and_server_state() {
        let mut state = StreamBroadcastRuntimeState::default();
        let guild_id = Id::<GuildMarker>::new(10);
        let channel_id = Id::<ChannelMarker>::new(20);
        let user_id = Id::<UserMarker>::new(30);

        state.apply(&VoiceRuntimeEvent::CurrentUserReady(Some(user_id)));
        state.apply(&VoiceRuntimeEvent::VoiceState(VoiceStateInfo {
            guild_id: Some(guild_id),
            channel_id: Some(channel_id),
            user_id,
            session_id: Some("session".to_owned()),
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        }));
        assert!(
            state
                .apply(&VoiceRuntimeEvent::BroadcastStreamRequested(request()))
                .connect
                .is_none()
        );
        assert!(
            state
                .apply(&VoiceRuntimeEvent::StreamCreate(StreamCreateInfo {
                    stream_key: request().stream_key,
                    rtc_server_id: "11".to_owned(),
                    rtc_channel_id: channel_id,
                    viewer_ids: Vec::new(),
                    paused: false,
                }))
                .connect
                .is_none()
        );

        let update = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: request().stream_key,
            endpoint: Some("streams.example".to_owned()),
            token: "token".to_owned(),
        }));

        let session = update.connect.expect("all broadcast state is ready");
        assert_eq!(session.current_user_id, user_id);
        assert_eq!(session.request.channel_id, channel_id);
        assert_eq!(session.endpoint, "streams.example");
    }

    #[test]
    fn broadcast_runtime_rotates_active_stream_servers() {
        let (mut state, initial) = connected_broadcast_runtime();

        let rotated = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: initial.request.stream_key.clone(),
            endpoint: Some("replacement.example.com".to_owned()),
            token: "replacement-token".to_owned(),
        }));
        assert_eq!(
            rotated.close_stream_key.as_deref(),
            Some(initial.request.stream_key.as_str())
        );
        assert!(!rotated.send_delete);
        let replacement = rotated
            .connect
            .expect("new stream server starts a replacement broadcast");
        assert_eq!(replacement.endpoint, "replacement.example.com");
        assert_eq!(replacement.token, "replacement-token");

        let unavailable = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: initial.request.stream_key.clone(),
            endpoint: None,
            token: "pending-token".to_owned(),
        }));
        assert_eq!(
            unavailable.close_stream_key.as_deref(),
            Some(initial.request.stream_key.as_str())
        );
        assert!(!unavailable.send_delete);
        assert!(unavailable.connect.is_none());

        let reallocated = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: initial.request.stream_key.clone(),
            endpoint: Some("reallocated.example.com".to_owned()),
            token: "reallocated-token".to_owned(),
        }));
        let active = reallocated
            .connect
            .expect("reallocated stream server reconnects broadcast");
        assert_ne!(active.connection_id, replacement.connection_id);

        let stale_end = state.apply(&VoiceRuntimeEvent::BroadcastStreamConnectionEnded {
            connection_id: replacement.connection_id,
            stream_key: replacement.request.stream_key,
            outcome: VoiceConnectionEnd::Stop,
        });
        assert!(stale_end.close_stream_key.is_none());
        assert!(stale_end.connect.is_none());
        assert_eq!(
            state
                .active
                .as_ref()
                .expect("reallocated broadcast remains active")
                .connection_id,
            active.connection_id
        );
    }

    #[test]
    fn broadcast_runtime_stops_after_bounded_reconnect_attempts() {
        let (mut state, mut active) = connected_broadcast_runtime();

        for attempt in 1..=MAX_VOICE_RECONNECT_ATTEMPTS {
            let update = state.apply(&broadcast_connection_ended(
                &active,
                VoiceConnectionEnd::Reconnect,
            ));
            assert!(
                update.close_stream_key.is_none(),
                "retry {attempt} should keep the broadcast request active"
            );
            active = update
                .connect
                .expect("retry within the limit should reconnect broadcast");
        }

        let stopped = state.apply(&broadcast_connection_ended(
            &active,
            VoiceConnectionEnd::Reconnect,
        ));
        assert!(stopped.connect.is_none());
        assert_eq!(
            stopped.close_stream_key.as_deref(),
            Some(active.request.stream_key.as_str())
        );
        assert!(stopped.send_delete);
    }

    #[test]
    fn broadcast_identify_declares_screen_stream() {
        let payload: Value = serde_json::from_str(&stream_broadcast_identify_payload(&session()))
            .expect("broadcast identify is valid json");
        assert_eq!(payload["op"], 0);
        assert_eq!(payload["d"]["video"], true);
        assert_eq!(payload["d"]["streams"][0]["type"], "screen");
        assert_eq!(payload["d"]["streams"][0]["rid"], STREAM_RID);
    }

    #[test]
    fn broadcast_ready_selects_requested_rid_and_derives_missing_rtx_ssrc() {
        let ready = json!({
            "d": {
                "streams": [
                    {"type": "video", "rid": "50", "ssrc": 50, "rtx_ssrc": 51},
                    {"type": "video", "rid": STREAM_RID, "ssrc": 100}
                ]
            }
        });

        assert_eq!(
            parse_broadcast_video_ssrcs(&ready),
            Ok(BroadcastVideoSsrcs {
                video_ssrc: 100,
                rtx_ssrc: 101,
            })
        );
    }

    #[test]
    fn broadcast_gateway_payloads_declare_outbound_h264_and_active_video() {
        let selected: Value = serde_json::from_str(&stream_broadcast_select_protocol_payload(
            &DiscoveredVoiceAddress {
                address: "127.0.0.1".to_owned(),
                port: 5000,
            },
            AEAD_XCHACHA20_POLY1305_RTPSIZE,
        ))
        .expect("broadcast select protocol payload is valid json");
        assert_eq!(selected["d"]["codecs"][0]["name"], "opus");
        assert_eq!(selected["d"]["codecs"][0]["encode"], true);
        assert_eq!(selected["d"]["codecs"][0]["decode"], false);
        assert_eq!(selected["d"]["codecs"][1]["name"], "H264");
        assert_eq!(
            selected["d"]["codecs"][1]["payload_type"],
            DISCORD_STREAM_VIDEO_PAYLOAD_TYPE
        );
        assert_eq!(
            selected["d"]["codecs"][1]["rtx_payload_type"],
            DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE
        );
        assert_eq!(selected["d"]["codecs"][1]["encode"], true);
        assert_eq!(selected["d"]["codecs"][1]["decode"], false);

        let video = BroadcastVideoSsrcs {
            video_ssrc: 100,
            rtx_ssrc: 101,
        };
        let announced: Value = serde_json::from_str(&stream_broadcast_video_payload(99, video))
            .expect("broadcast video payload is valid json");
        assert_eq!(announced["op"], 12);
        assert_eq!(announced["d"]["audio_ssrc"], 99);
        assert_eq!(announced["d"]["video_ssrc"], 100);
        assert_eq!(announced["d"]["rtx_ssrc"], 101);
        assert_eq!(announced["d"]["streams"][0]["type"], "video");
        assert_eq!(announced["d"]["streams"][0]["rid"], STREAM_RID);
        assert_eq!(announced["d"]["streams"][0]["ssrc"], 100);
        assert_eq!(announced["d"]["streams"][0]["rtx_ssrc"], 101);
        assert_eq!(announced["d"]["streams"][0]["active"], true);
        assert_eq!(
            announced["d"]["streams"][0]["max_bitrate"],
            capture::STREAM_CAPTURE_BITRATE
        );
        assert_eq!(
            announced["d"]["streams"][0]["max_framerate"],
            capture::STREAM_CAPTURE_FPS
        );
        assert_eq!(
            announced["d"]["streams"][0]["max_resolution"]["width"],
            capture::STREAM_CAPTURE_WIDTH
        );
        assert_eq!(
            announced["d"]["streams"][0]["max_resolution"]["height"],
            capture::STREAM_CAPTURE_HEIGHT
        );
    }

    #[test]
    fn broadcast_speaking_registers_audio_ssrc_as_soundshare() {
        let payload: Value = serde_json::from_str(&stream_broadcast_speaking_payload(1234))
            .expect("broadcast speaking payload is valid json");
        assert_eq!(payload["op"], VOICE_OP_SPEAKING);
        assert_eq!(payload["d"]["speaking"], SOUNDSHARE_SPEAKING_FLAG);
        assert_eq!(payload["d"]["ssrc"], 1234);
    }

    #[test]
    fn broadcast_audio_clock_preserves_capture_gaps() {
        let first = Instant::now();
        assert_eq!(broadcast_audio_elapsed_frames(None, first), 1);
        assert_eq!(
            broadcast_audio_elapsed_frames(Some(first), first + DISCORD_OPUS_FRAME_DURATION * 3),
            3
        );
        assert_eq!(
            broadcast_audio_elapsed_frames(Some(first), first + DISCORD_OPUS_FRAME_DURATION / 4),
            1
        );
    }

    #[test]
    fn broadcast_packet_encryptor_allocates_unique_nonces_concurrently() {
        let encryptor = Arc::new(
            BroadcastPacketEncryptor::with_nonce(AEAD_XCHACHA20_POLY1305_RTPSIZE, &[9; 32], 1)
                .expect("broadcast packet encryptor should build"),
        );
        let mut workers = Vec::new();
        for _ in 0..4 {
            let encryptor = Arc::clone(&encryptor);
            workers.push(std::thread::spawn(move || {
                (0..256)
                    .map(|_| {
                        u32::from_be_bytes(
                            encryptor
                                .take_nonce("test RTP")
                                .expect("test nonce should be available"),
                        )
                    })
                    .collect::<Vec<_>>()
            }));
        }

        let mut nonces = workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("nonce worker should finish"))
            .collect::<Vec<_>>();
        nonces.sort_unstable();
        nonces.dedup();

        assert_eq!(nonces.len(), 1_024);
        assert_eq!(nonces.first(), Some(&1));
        assert_eq!(nonces.last(), Some(&1_024));

        let exhausted = BroadcastPacketEncryptor::with_nonce(
            AEAD_XCHACHA20_POLY1305_RTPSIZE,
            &[9; 32],
            u32::MAX - 1,
        )
        .expect("broadcast packet encryptor should build near nonce exhaustion");
        assert_eq!(
            exhausted
                .take_nonce("test RTP")
                .expect("last safe nonce should be available"),
            (u32::MAX - 1).to_be_bytes()
        );
        assert_eq!(
            exhausted
                .take_nonce("test RTP")
                .expect_err("nonce allocation must stop before wrapping"),
            "broadcast test RTP nonce exhausted"
        );
    }

    #[tokio::test]
    async fn broadcast_audio_sender_preserves_queued_frame_order() {
        let receiver = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("test audio receiver should bind");
        let sender = Arc::new(
            UdpSocket::bind("127.0.0.1:0")
                .await
                .expect("test audio sender should bind"),
        );
        sender
            .connect(
                receiver
                    .local_addr()
                    .expect("test audio receiver should have an address"),
            )
            .await
            .expect("test audio sender should connect");

        let (frames_tx, frames_rx) = mpsc::channel(SYSTEM_AUDIO_FRAME_QUEUE);
        let first_captured_at = Instant::now();
        for index in 0..3 {
            frames_tx
                .send(system_audio::SystemAudioFrame {
                    samples: vec![index as i16; DISCORD_OPUS_20MS_STEREO_SAMPLES],
                    captured_at: first_captured_at + DISCORD_OPUS_FRAME_DURATION * index,
                })
                .await
                .expect("test audio frame should queue");
        }
        drop(frames_tx);

        let description = voice_description();
        let packet_encryptor =
            Arc::new(BroadcastPacketEncryptor::new(&description).expect("encryptor should build"));
        let stats = Arc::new(StdMutex::new(BroadcastSendStats::new()));
        let transport =
            BroadcastAudioTransport::new(42, Arc::clone(&packet_encryptor), Arc::clone(&stats));
        let initial_sequence = transport.sequence;
        let initial_timestamp = transport.timestamp;
        let dave_state = Arc::new(Mutex::new(VoiceDaveState::new_for_identity(
            Id::new(30),
            10,
        )));
        let encoder =
            VoiceOpusEncode::new_system_audio().expect("system audio Opus encoder should build");
        let sender_task = tokio::spawn(run_stream_broadcast_audio(
            Arc::clone(&sender),
            dave_state,
            None,
            frames_rx,
            encoder,
            transport,
        ));
        let decryptor = VoiceRtpDecryptor::new(&description.mode, &description.secret_key)
            .expect("test audio decryptor should build");

        let mut packet = vec![0u8; 2_048];
        for index in 0..3u32 {
            let length = timeout(Duration::from_secs(1), receiver.recv(&mut packet))
                .await
                .expect("ordered audio packet should arrive")
                .expect("ordered audio packet should receive");
            let header =
                parse_rtp_header(&packet[..length]).expect("audio RTP header should parse");
            let decrypted = decryptor
                .decrypt_packet(&packet[..length], &header)
                .expect("audio RTP packet should decrypt");

            assert_eq!(header.payload_type, DISCORD_VOICE_PAYLOAD_TYPE);
            assert_eq!(header.sequence, initial_sequence.wrapping_add(index as u16));
            assert_eq!(
                header.timestamp,
                initial_timestamp
                    .wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT.wrapping_mul(index + 1))
            );
            assert_eq!(header.ssrc, 42);
            assert_eq!(header.marker, index == 0);
            assert!(!decrypted.media_payload.is_empty());
        }

        timeout(Duration::from_secs(1), sender_task)
            .await
            .expect("audio sender task should finish")
            .expect("audio sender task should join")
            .expect("audio sender should preserve queued frame order");
    }

    #[test]
    fn sender_report_maps_video_clock_and_counters() {
        let packet = build_rtcp_sender_report(42, 90_000, 12, 34_567);
        assert_eq!(packet.len(), 28);
        assert_eq!(&packet[..4], &[0x80, 200, 0, 6]);
        assert_eq!(
            u32::from_be_bytes(packet[4..8].try_into().expect("SSRC")),
            42
        );
        assert_eq!(
            u32::from_be_bytes(packet[16..20].try_into().expect("RTP timestamp")),
            90_000
        );
        assert_eq!(
            u32::from_be_bytes(packet[20..24].try_into().expect("packet count")),
            12
        );
        assert_eq!(
            u32::from_be_bytes(packet[24..28].try_into().expect("octet count")),
            34_567
        );
    }

    #[test]
    fn h264_packetizer_marks_only_final_packet_and_adds_extensions() {
        let frame = [0, 0, 0, 1, 0x65]
            .into_iter()
            .chain(std::iter::repeat_n(0xaa, 3_000))
            .collect::<Vec<_>>();
        let mut sequence = 7;
        let mut transport_sequence = 99;
        let packets = packetize_discord_h264_frame(
            &frame,
            90_000,
            42,
            &mut sequence,
            &mut transport_sequence,
        );

        assert!(packets.len() > 1);
        for (index, packet) in packets.iter().enumerate() {
            let header = parse_rtp_header(packet).expect("broadcast RTP packet is valid");
            assert_eq!(header.payload_type, DISCORD_STREAM_VIDEO_PAYLOAD_TYPE);
            assert_eq!(header.timestamp, 90_000);
            assert_eq!(header.ssrc, 42);
            assert_eq!(header.marker, index + 1 == packets.len());
            assert!(header.encrypted_extension_body_len > 0);
        }
    }

    #[test]
    fn large_frames_are_paced_with_bounded_latency() {
        assert_eq!(rtp_packet_pacing_interval(1), None);

        let small_frame_interval =
            rtp_packet_pacing_interval(3).expect("multiple packets should be paced");
        assert!(small_frame_interval <= STREAM_RTP_MAX_PACKET_SPACING);

        let packet_count = 100;
        let large_frame_interval =
            rtp_packet_pacing_interval(packet_count).expect("large frame should be paced");
        let total_pacing = large_frame_interval * (packet_count as u32 - 1);
        assert!(total_pacing <= STREAM_RTP_PACING_BUDGET);
        assert!(total_pacing >= STREAM_RTP_PACING_BUDGET - Duration::from_millis(1));
    }

    #[test]
    fn rtcp_feedback_requests_retransmission_and_keyframe_recovery() {
        let sender_ssrc = 7u32;
        let video_ssrc = 42u32;
        let mut feedback = Vec::new();

        feedback.extend_from_slice(&[0x81, 201, 0, 7]);
        feedback.extend_from_slice(&sender_ssrc.to_be_bytes());
        feedback.extend_from_slice(&video_ssrc.to_be_bytes());
        feedback.extend_from_slice(&[64, 0, 0, 2]);
        feedback.extend_from_slice(&[0; 16]);

        feedback.extend_from_slice(&[0x81, 205, 0, 3]);
        feedback.extend_from_slice(&sender_ssrc.to_be_bytes());
        feedback.extend_from_slice(&video_ssrc.to_be_bytes());
        feedback.extend_from_slice(&1_000u16.to_be_bytes());
        feedback.extend_from_slice(&0b0000_0000_0000_0101u16.to_be_bytes());

        feedback.extend_from_slice(&[0x81, 206, 0, 2]);
        feedback.extend_from_slice(&sender_ssrc.to_be_bytes());
        feedback.extend_from_slice(&video_ssrc.to_be_bytes());

        let parsed =
            parse_broadcast_rtcp_feedback(&feedback, video_ssrc).expect("feedback should parse");

        assert_eq!(parsed.nack_sequences, vec![1_000, 1_001, 1_003]);
        assert!(parsed.request_keyframe);
        assert_eq!(
            parsed.receiver_reports,
            vec![BroadcastReceiverReport {
                reporter_ssrc: sender_ssrc,
                fraction_lost: 64,
                cumulative_lost: 2,
            }]
        );

        let mut previous_loss = HashMap::new();
        assert!(receiver_report_has_new_loss(
            &mut previous_loss,
            sender_ssrc,
            2
        ));
        assert!(!receiver_report_has_new_loss(
            &mut previous_loss,
            sender_ssrc,
            2
        ));
        assert!(receiver_report_has_new_loss(
            &mut previous_loss,
            sender_ssrc,
            3
        ));
        assert!(receiver_report_has_new_loss(
            &mut previous_loss,
            sender_ssrc + 1,
            1
        ));
    }

    #[test]
    fn rtx_packet_preserves_original_packet_identity_and_payload() {
        let original = build_discord_video_rtp_packet(10, 90_000, 42, true, 20, b"h264");
        let original_header =
            parse_rtp_header(&original).expect("original RTP packet should parse");
        let rtx = build_discord_video_rtx_packet(&original, 100, 30, 40)
            .expect("RTX packet should build");
        let rtx_header = parse_rtp_header(&rtx).expect("RTX packet should parse");

        assert_eq!(
            rtx_header.payload_type,
            DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE
        );
        assert_eq!(rtx_header.sequence, 30);
        assert_eq!(rtx_header.timestamp, original_header.timestamp);
        assert_eq!(rtx_header.ssrc, 100);
        assert_eq!(rtx_header.marker, original_header.marker);
        assert_eq!(
            &rtx[rtx_header.payload_offset..rtx_header.payload_offset + 2],
            &original_header.sequence.to_be_bytes()
        );
        assert_eq!(
            &rtx[rtx_header.payload_offset + 2..],
            &original[original_header.payload_offset..]
        );
    }
}
