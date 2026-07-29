use std::{
    collections::VecDeque,
    io::Write,
    net::{Ipv4Addr, SocketAddrV4},
    path::Path,
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use tempfile::NamedTempFile;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
};
use uuid::Uuid;

use super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS;
use super::*;

const STREAM_RTP_PACKET_BYTES: usize = 4096;
const LOCAL_H264_MAX_PAYLOAD_BYTES: usize = 1200;
const STREAM_STARTUP_BUFFER_MAX_FRAMES: usize = 180;
const STREAM_STARTUP_REPLAY_FRAME_TICKS: u32 = 90;
const OPUS_RTP_CLOCK_RATE: u32 = 48_000;
const VIDEO_RTP_CLOCK_RATE: u32 = 90_000;
const STREAM_KEYFRAME_REQUEST_INTERVAL: Duration = Duration::from_secs(1);
const LOCAL_RTCP_REPORT_INTERVAL: Duration = Duration::from_secs(1);
const STREAM_CONNECTION_STABLE_INTERVAL: Duration = Duration::from_secs(10);
const NTP_UNIX_EPOCH_OFFSET_SECONDS: u64 = 2_208_988_800;
const RTCP_SENDER_REPORT_PACKET_TYPE: u8 = 200;
const RTCP_SENDER_REPORT_LENGTH_WORDS_MINUS_ONE: u16 = 6;
const RTCP_PAYLOAD_SPECIFIC_FEEDBACK: u8 = 206;
const RTCP_PLI_FORMAT: u8 = 1;
const RTCP_PLI_LENGTH_WORDS_MINUS_ONE: u16 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StreamGatewaySession {
    pub(super) connection_id: u64,
    pub(super) request: StreamWatchRequest,
    pub(super) current_user_id: Id<UserMarker>,
    pub(super) session_id: String,
    pub(super) rtc_server_id: String,
    pub(super) rtc_channel_id: Id<ChannelMarker>,
    pub(super) endpoint: String,
    pub(super) token: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedStreamVoiceState {
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    session_id: String,
}

#[derive(Clone)]
struct StreamPlayerReadySignal {
    player_ready: Arc<AtomicBool>,
    ready_tx: mpsc::UnboundedSender<u64>,
    media_generation: u64,
    status_publisher: VoiceStatusPublisher,
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    user_id: Id<UserMarker>,
}

#[derive(Debug, Eq, PartialEq)]
struct StreamConnectionFailure {
    message: String,
    outcome: VoiceConnectionEnd,
}

impl StreamConnectionFailure {
    fn reconnect(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: VoiceConnectionEnd::Reconnect,
        }
    }

    fn stop(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: VoiceConnectionEnd::Stop,
        }
    }
}

impl From<String> for StreamConnectionFailure {
    fn from(message: String) -> Self {
        Self::reconnect(message)
    }
}

#[derive(Default)]
pub(super) struct StreamRuntimeState {
    current_user_id: Option<Id<UserMarker>>,
    current_voice: Option<ObservedStreamVoiceState>,
    requested: Option<StreamWatchRequest>,
    create: Option<StreamCreateInfo>,
    server: Option<StreamServerInfo>,
    active: Option<StreamGatewaySession>,
    reconnect_attempts: u8,
    next_connection_id: u64,
}

#[derive(Default)]
pub(super) struct StreamRuntimeUpdate {
    pub(super) close_stream_key: Option<String>,
    pub(super) send_delete: bool,
    pub(super) connect: Option<StreamGatewaySession>,
    pub(super) error: Option<String>,
}

impl StreamRuntimeState {
    pub(super) fn apply(&mut self, event: &VoiceRuntimeEvent) -> StreamRuntimeUpdate {
        let mut update = StreamRuntimeUpdate::default();
        match event {
            VoiceRuntimeEvent::CurrentUserReady(user_id) => self.current_user_id = *user_id,
            VoiceRuntimeEvent::VoiceState(state) => self.record_voice_state(state, &mut update),
            VoiceRuntimeEvent::WatchStreamRequested(request) => {
                if self
                    .requested
                    .as_ref()
                    .is_none_or(|current| current.stream_key != request.stream_key)
                {
                    update.close_stream_key =
                        self.active.take().map(|active| active.request.stream_key);
                    update.send_delete = update.close_stream_key.is_some();
                    self.create = None;
                    self.server = None;
                    self.reconnect_attempts = 0;
                }
                self.requested = Some(request.clone());
            }
            VoiceRuntimeEvent::WatchStreamCancelled { stream_key } => {
                self.clear_matching(stream_key, &mut update, false);
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
                    self.server = Some(server.clone());
                }
            }
            VoiceRuntimeEvent::StreamDelete(stream) => {
                if let Some(request) = self
                    .requested
                    .as_ref()
                    .filter(|request| request.stream_key == stream.stream_key)
                    && (!stream.reason.is_empty() || stream.unavailable)
                {
                    let reason = if stream.reason.is_empty() {
                        "stream unavailable"
                    } else {
                        stream.reason.as_str()
                    };
                    update.error = Some(format!(
                        "Could not watch {}'s stream: {reason}",
                        request.display_name
                    ));
                }
                self.clear_matching(&stream.stream_key, &mut update, false);
            }
            VoiceRuntimeEvent::StreamConnectionEstablished {
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
            VoiceRuntimeEvent::StreamConnectionEnded {
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
                update.close_stream_key =
                    self.active.take().map(|active| active.request.stream_key);
                update.send_delete = update.close_stream_key.is_some();
                self.requested = None;
                self.create = None;
                self.server = None;
            }
            _ => {}
        }

        if self.active.is_none() {
            update.connect = self.connect_if_ready();
        }
        update
    }

    fn record_voice_state(&mut self, state: &VoiceStateInfo, update: &mut StreamRuntimeUpdate) {
        if self.current_user_id != Some(state.user_id) {
            return;
        }
        let Some(channel_id) = state.channel_id else {
            self.current_voice = None;
            update.close_stream_key = self.active.take().map(|active| active.request.stream_key);
            update.send_delete = update.close_stream_key.is_some();
            self.requested = None;
            self.create = None;
            self.server = None;
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
        self.current_voice = Some(ObservedStreamVoiceState {
            scope,
            channel_id,
            session_id: session_id.clone(),
        });
        if self
            .requested
            .as_ref()
            .is_some_and(|request| request.scope != scope || request.channel_id != channel_id)
        {
            update.close_stream_key = self.active.take().map(|active| active.request.stream_key);
            update.send_delete = update.close_stream_key.is_some();
            self.requested = None;
            self.create = None;
            self.server = None;
        }
    }

    fn clear_matching(
        &mut self,
        stream_key: &str,
        update: &mut StreamRuntimeUpdate,
        send_delete: bool,
    ) {
        if self
            .requested
            .as_ref()
            .is_some_and(|request| request.stream_key == stream_key)
        {
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
            update.close_stream_key = Some(stream_key.to_owned());
            update.send_delete = send_delete;
        }
    }

    fn connect_if_ready(&mut self) -> Option<StreamGatewaySession> {
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
        let session = StreamGatewaySession {
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

pub(super) async fn run_stream_gateway_session(
    session: StreamGatewaySession,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: VoiceStatusPublisher,
) {
    let outcome = match connect_stream_gateway(&session, &events_tx, &status_publisher).await {
        Ok(outcome) => outcome,
        Err(error) => {
            logging::error("stream", &error.message);
            status_publisher
                .publish_error(format!(
                    "Could not watch {}'s stream: {}",
                    session.request.display_name, error.message
                ))
                .await;
            error.outcome
        }
    };
    status_publisher
        .publish_stream_playback_ended(
            session.request.scope,
            session.request.channel_id,
            session.request.owner_id,
            outcome == VoiceConnectionEnd::Reconnect,
        )
        .await;
    let _ = events_tx.send(VoiceRuntimeEvent::StreamConnectionEnded {
        connection_id: session.connection_id,
        stream_key: session.request.stream_key.clone(),
        outcome,
    });
}

async fn connect_stream_gateway(
    session: &StreamGatewaySession,
    events_tx: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: &VoiceStatusPublisher,
) -> Result<VoiceConnectionEnd, StreamConnectionFailure> {
    let url = gateway::voice_gateway_url(&session.endpoint)?;
    logging::debug("stream", format!("connecting stream websocket: {url}"));
    let (ws, response) = timeout(VOICE_WEBSOCKET_CONNECT_TIMEOUT, connect_async(&url))
        .await
        .map_err(|_| "stream websocket connect timed out after 10s".to_owned())?
        .map_err(|error| format!("stream websocket connect failed: {error}"))?;
    logging::debug(
        "stream",
        format!("stream websocket connected: status={}", response.status()),
    );
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let last_sequence = Arc::new(Mutex::new(None));
    let heartbeat_ack = Arc::new(Mutex::new(VoiceHeartbeatAckState::default()));
    let (heartbeat_timeout_tx, mut heartbeat_timeout_rx) =
        mpsc::unbounded_channel::<VoiceHeartbeatTimeout>();
    let mut heartbeat_task: Option<JoinHandle<()>> = None;
    let mut keepalive_task: Option<JoinHandle<()>> = None;
    let mut media_task: Option<JoinHandle<()>> = None;
    let (media_finished_tx, mut media_finished_rx) =
        mpsc::unbounded_channel::<Result<(), StreamConnectionFailure>>();
    let (player_ready_tx, mut player_ready_rx) = mpsc::unbounded_channel::<u64>();
    let (video_source_tx, video_source_rx) = watch::channel(StreamVideoSource::default());
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
    let mut local_ssrc: Option<u32> = None;
    let mut current_description: Option<VoiceSessionDescription> = None;
    let mut media_generation = 0u64;
    let mut connection_stable_deadline: Option<Instant> = None;

    gateway::send_voice_text(&writer, stream_identify_payload(session)).await?;
    logging::debug("stream", "stream identify sent");

    let result = loop {
        let frame = tokio::select! {
            _ = heartbeat_timeout_rx.recv() => {
                break Ok(VoiceConnectionEnd::Reconnect);
            }
            media_result = media_finished_rx.recv(), if media_task.is_some() => {
                match media_result {
                    Some(Ok(())) => break Ok(VoiceConnectionEnd::Stop),
                    Some(Err(error)) => break Err(error),
                    None => break Ok(VoiceConnectionEnd::Reconnect),
                }
            }
            ready_generation = player_ready_rx.recv(), if media_task.is_some() => {
                if ready_generation == Some(media_generation) {
                    connection_stable_deadline =
                        Some(Instant::now() + STREAM_CONNECTION_STABLE_INTERVAL);
                }
                continue;
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(
                connection_stable_deadline.unwrap_or_else(Instant::now)
            )), if connection_stable_deadline.is_some() => {
                connection_stable_deadline = None;
                let _ = events_tx.send(VoiceRuntimeEvent::StreamConnectionEstablished {
                    connection_id: session.connection_id,
                    stream_key: session.request.stream_key.clone(),
                });
                continue;
            }
            frame = reader.next() => frame,
        };
        let Some(frame) = frame else {
            break Ok(VoiceConnectionEnd::Reconnect);
        };
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                break Err(StreamConnectionFailure::reconnect(format!(
                    "stream websocket read failed: {error}"
                )));
            }
        };
        match frame {
            WsMessage::Text(text) => {
                let value: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("stream websocket JSON parse failed: {error}"))?;
                if let Some(sequence) = value.get("seq").and_then(Value::as_i64) {
                    *last_sequence.lock().await = Some(sequence);
                }
                let opcode = value.get("op").and_then(Value::as_u64).unwrap_or_default() as u8;
                match opcode {
                    VOICE_OP_READY => {
                        let ready = gateway::parse_voice_ready_payload(&value)?;
                        let mode = gateway::choose_encryption_mode(&ready.modes)?;
                        let (socket, discovered) =
                            gateway::discover_voice_udp_address(&ready).await?;
                        gateway::send_voice_text(
                            &writer,
                            stream_select_protocol_payload(&discovered, &mode),
                        )
                        .await?;
                        gateway::send_voice_text(
                            &writer,
                            stream_receive_only_video_payload(ready.ssrc),
                        )
                        .await?;
                        local_ssrc = Some(ready.ssrc);
                        udp_socket = Some(socket);
                    }
                    VOICE_OP_SESSION_DESCRIPTION => {
                        let description = gateway::parse_voice_session_description(&value)?;
                        if description
                            .video_codec
                            .as_deref()
                            .is_some_and(|codec| !codec.eq_ignore_ascii_case("H264"))
                        {
                            break Err(StreamConnectionFailure::reconnect(format!(
                                "stream selected unsupported video codec: {}",
                                description.video_codec.as_deref().unwrap_or("none")
                            )));
                        }
                        if let Some(version) = description.dave_protocol_version {
                            let version = u16::try_from(version)
                                .map_err(|_| "DAVE protocol version does not fit u16".to_owned())?;
                            dave_state.lock().await.reinit(version)?;
                        }
                        let Some(socket) = udp_socket.as_ref() else {
                            break Err(StreamConnectionFailure::reconnect(
                                "stream session description arrived before UDP ready",
                            ));
                        };
                        let Some(local_ssrc) = local_ssrc else {
                            break Err(StreamConnectionFailure::reconnect(
                                "stream session description arrived before local SSRC",
                            ));
                        };
                        if current_description.as_ref() == Some(&description) {
                            continue;
                        }
                        if let Some(task) = media_task.take() {
                            task.abort();
                        }
                        if let Some(task) = keepalive_task.take() {
                            task.abort();
                        }
                        let socket_for_media = Arc::clone(socket);
                        let description_for_media = description.clone();
                        let dave_for_media = Arc::clone(&dave_state);
                        let source_for_media = video_source_rx.clone();
                        let finished = media_finished_tx.clone();
                        let owner_id = session.request.owner_id;
                        media_generation = media_generation.wrapping_add(1);
                        connection_stable_deadline = None;
                        let stream_player_ready = StreamPlayerReadySignal {
                            player_ready: Arc::new(AtomicBool::new(false)),
                            ready_tx: player_ready_tx.clone(),
                            media_generation,
                            status_publisher: status_publisher.clone(),
                            scope: session.request.scope,
                            channel_id: session.request.channel_id,
                            user_id: session.request.owner_id,
                        };
                        media_task = Some(tokio::spawn(async move {
                            let result = run_stream_media(
                                socket_for_media,
                                description_for_media,
                                dave_for_media,
                                source_for_media,
                                owner_id,
                                local_ssrc,
                                stream_player_ready,
                            )
                            .await;
                            let _ = finished.send(result);
                        }));
                        keepalive_task = Some(tokio::spawn(gateway::run_voice_udp_keepalive(
                            Arc::clone(socket),
                        )));
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
                            .ok_or_else(|| "stream hello missing heartbeat interval".to_owned())?;
                        if let Some(task) = heartbeat_task.take() {
                            task.abort();
                        }
                        heartbeat_ack.lock().await.reset();
                        heartbeat_task = Some(tokio::spawn(gateway::run_voice_heartbeat(
                            Arc::clone(&writer),
                            interval,
                            Arc::clone(&last_sequence),
                            Arc::clone(&heartbeat_ack),
                            heartbeat_timeout_tx.clone(),
                            0,
                        )));
                    }
                    VOICE_OP_VIDEO => {
                        if let Some(source) =
                            parse_stream_video_source(&value, session.request.owner_id)
                        {
                            logging::debug(
                                "stream",
                                format!(
                                    "stream video source selected: audio_ssrc={} video_ssrc={} rtx_ssrc={:?}",
                                    source.audio_ssrc, source.video_ssrc, source.rtx_ssrc
                                ),
                            );
                            {
                                let mut dave = dave_state.lock().await;
                                dave.record_ssrc_user(source.audio_ssrc, session.request.owner_id);
                                dave.record_ssrc_user(source.video_ssrc, session.request.owner_id);
                            }
                            gateway::send_voice_text(
                                &writer,
                                stream_media_sink_wants_payload(
                                    source.audio_ssrc,
                                    source.video_ssrc,
                                ),
                            )
                            .await?;
                            video_source_tx.send_replace(source);
                        }
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
                        logging::debug("stream", format!("unhandled stream gateway op={other}"))
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
                    .map_err(|error| format!("stream websocket pong failed: {error}"))?;
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

    if let Some(task) = heartbeat_task {
        task.abort();
    }
    if let Some(task) = keepalive_task {
        task.abort();
    }
    if let Some(task) = media_task {
        task.abort();
    }
    result
}

fn stream_identify_payload(session: &StreamGatewaySession) -> String {
    json!({
        "op": 0,
        "d": {
            "server_id": session.rtc_server_id,
            "user_id": session.current_user_id.to_string(),
            "channel_id": session.rtc_channel_id.to_string(),
            "session_id": session.session_id,
            "token": session.token,
            "video": true,
            "max_dave_protocol_version": davey::DAVE_PROTOCOL_VERSION,
        },
    })
    .to_string()
}

fn stream_select_protocol_payload(discovered: &DiscoveredVoiceAddress, mode: &str) -> String {
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
                    "encode": false,
                    "decode": true,
                },
                {
                    "name": "H264",
                    "type": "video",
                    "priority": 1000,
                    "payload_type": DISCORD_STREAM_VIDEO_PAYLOAD_TYPE,
                    "rtx_payload_type": DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE,
                    "encode": false,
                    "decode": true,
                },
            ],
            "rtc_connection_id": Uuid::new_v4().to_string(),
        },
    })
    .to_string()
}

fn stream_receive_only_video_payload(audio_ssrc: u32) -> String {
    json!({
        "op": VOICE_OP_VIDEO,
        "d": {
            "audio_ssrc": audio_ssrc,
            "video_ssrc": 0,
            "rtx_ssrc": 0,
            "streams": [],
        },
    })
    .to_string()
}

fn stream_media_sink_wants_payload(audio_ssrc: u32, video_ssrc: u32) -> String {
    let mut wants = serde_json::Map::new();
    if audio_ssrc != 0 {
        wants.insert(audio_ssrc.to_string(), Value::from(100));
    }
    if video_ssrc != 0 {
        wants.insert(video_ssrc.to_string(), Value::from(100));
    }
    wants.insert("any".to_owned(), Value::from(0));
    json!({
        "op": VOICE_OP_MEDIA_SINK_WANTS,
        "d": Value::Object(wants),
    })
    .to_string()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct StreamVideoSource {
    audio_ssrc: u32,
    video_ssrc: u32,
    rtx_ssrc: Option<u32>,
}

fn parse_stream_video_source(value: &Value, owner_id: Id<UserMarker>) -> Option<StreamVideoSource> {
    let data = value.get("d")?;
    if data.get("user_id").and_then(Value::as_str) != Some(owner_id.to_string().as_str()) {
        return None;
    }
    let audio_ssrc = data
        .get("audio_ssrc")
        .and_then(Value::as_u64)
        .and_then(|ssrc| u32::try_from(ssrc).ok())?;
    let fallback_video_ssrc = data
        .get("video_ssrc")
        .and_then(Value::as_u64)
        .and_then(|ssrc| u32::try_from(ssrc).ok())
        .filter(|ssrc| *ssrc != 0);
    let selected = data
        .get("streams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|stream| {
            stream
                .get("active")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        })
        .filter_map(|stream| {
            let ssrc = stream
                .get("ssrc")
                .and_then(Value::as_u64)
                .and_then(|ssrc| u32::try_from(ssrc).ok())?;
            let quality = stream
                .get("quality")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let rtx_ssrc = stream
                .get("rtx_ssrc")
                .and_then(Value::as_u64)
                .and_then(|ssrc| u32::try_from(ssrc).ok());
            Some((quality, ssrc, rtx_ssrc))
        })
        .max_by_key(|(quality, _, _)| *quality);
    let (video_ssrc, rtx_ssrc) = selected
        .map(|(_, ssrc, rtx)| (ssrc, rtx))
        .or_else(|| fallback_video_ssrc.map(|ssrc| (ssrc, Some(ssrc.wrapping_add(1)))))?;
    Some(StreamVideoSource {
        audio_ssrc,
        video_ssrc,
        rtx_ssrc,
    })
}

async fn run_stream_media(
    discord_socket: Arc<UdpSocket>,
    description: VoiceSessionDescription,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    video_source_rx: watch::Receiver<StreamVideoSource>,
    owner_id: Id<UserMarker>,
    local_ssrc: u32,
    stream_player_ready: StreamPlayerReadySignal,
) -> Result<(), StreamConnectionFailure> {
    let audio_ports = reserve_local_udp_port_pair()?;
    let video_ports = reserve_local_udp_port_pair()?;
    let audio_port = audio_ports.rtp_port;
    let audio_rtcp_port = audio_ports.rtcp_port;
    let video_port = video_ports.rtp_port;
    let video_rtcp_port = video_ports.rtcp_port;
    let mut sdp =
        NamedTempFile::new().map_err(|error| format!("create stream SDP failed: {error}"))?;
    sdp.write_all(stream_sdp(audio_port, audio_rtcp_port, video_port, video_rtcp_port).as_bytes())
        .map_err(|error| format!("write stream SDP failed: {error}"))?;
    sdp.flush()
        .map_err(|error| format!("flush stream SDP failed: {error}"))?;

    // Keep both RTP/RTCP pairs reserved until the SDP is complete, then
    // release them immediately before mpv binds its receive sockets.
    drop(audio_ports);
    drop(video_ports);
    let mut player = stream_player_command(sdp.path());
    let mut player = player.spawn().map_err(stream_player_spawn_failure)?;
    let player_id = player.id();
    let player_stdout = player
        .stdout
        .take()
        .ok_or_else(|| StreamConnectionFailure::stop("capture stream mpv stdout failed"))?;
    let player_stderr = player
        .stderr
        .take()
        .ok_or_else(|| StreamConnectionFailure::stop("capture stream mpv stderr failed"))?;
    let last_player_error = Arc::new(Mutex::new(None));
    let video_player_ready = Arc::clone(&stream_player_ready.player_ready);
    let player_log_tasks = [
        tokio::spawn(log_stream_player_output(
            "stream",
            "stdout",
            player_stdout,
            Arc::clone(&last_player_error),
            Some(stream_player_ready),
        )),
        tokio::spawn(log_stream_player_output(
            "stream",
            "stderr",
            player_stderr,
            Arc::clone(&last_player_error),
            None,
        )),
    ];
    logging::debug(
        "stream",
        format!(
            "stream mpv started: pid={player_id:?} audio_port={audio_port} audio_rtcp_port={audio_rtcp_port} video_port={video_port} video_rtcp_port={video_rtcp_port}"
        ),
    );

    let local_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| format!("bind local stream RTP socket failed: {error}"))?;
    let audio_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, audio_port);
    let audio_rtcp_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, audio_rtcp_port);
    let video_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, video_port);
    let video_rtcp_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, video_rtcp_port);
    let decryptor = VoiceRtpDecryptor::new(&description.mode, &description.secret_key)?;
    let encryptor = VoiceRtpEncryptor::new(&description.mode, &description.secret_key)?;
    let mut packet = [0u8; STREAM_RTP_PACKET_BYTES];
    let mut h264 = H264Depacketizer::default();
    let mut h264_startup = H264StartupGate::default();
    let mut h264_startup_buffer: VecDeque<BufferedH264Frame> = VecDeque::new();
    let mut local_audio_sequence = 0u16;
    let mut local_video_sequence = 0u16;
    let mut local_audio_packets = 0u32;
    let mut local_audio_octets = 0u32;
    let mut local_video_packets = 0u32;
    let mut local_video_octets = 0u32;
    let mut local_video_frames = 0u64;
    let mut last_local_video_timestamp = None;
    let mut rtcp_feedback_nonce = 0u32;
    let media_started_at = Instant::now();
    let mut local_audio_clock = LocalRtpClock::default();
    let mut local_video_clock = LocalRtpClock::default();
    let mut logged_first_audio = false;
    let mut logged_first_video_frame = false;
    let mut logged_first_video = false;
    let mut logged_video_before_player_ready = false;
    let mut logged_keyframe_request = false;
    let mut logged_local_sender_reports = false;
    let mut local_rtcp_report_ticks = 0u64;
    let mut keyframe_request_interval = tokio::time::interval(STREAM_KEYFRAME_REQUEST_INTERVAL);
    keyframe_request_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut local_rtcp_report_interval = tokio::time::interval(LOCAL_RTCP_REPORT_INTERVAL);
    local_rtcp_report_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = local_rtcp_report_interval.tick() => {
                let elapsed = media_started_at.elapsed();
                let source = *video_source_rx.borrow();
                let unix_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|error| format!("read local stream wall clock failed: {error}"))?;
                let mut sent_report = false;
                if source.audio_ssrc != 0 && local_audio_packets != 0 {
                    let report = build_local_rtcp_sender_report(
                        source.audio_ssrc,
                        unix_time,
                        elapsed_rtp_timestamp(elapsed, OPUS_RTP_CLOCK_RATE),
                        local_audio_packets,
                        local_audio_octets,
                    );
                    let _ = local_socket.send_to(&report, audio_rtcp_target).await;
                    sent_report = true;
                }
                if source.video_ssrc != 0 && local_video_packets != 0 {
                    let report = build_local_rtcp_sender_report(
                        source.video_ssrc,
                        unix_time,
                        elapsed_rtp_timestamp(elapsed, VIDEO_RTP_CLOCK_RATE),
                        local_video_packets,
                        local_video_octets,
                    );
                    let _ = local_socket.send_to(&report, video_rtcp_target).await;
                    sent_report = true;
                }
                if sent_report {
                    local_rtcp_report_ticks = local_rtcp_report_ticks.wrapping_add(1);
                    if !logged_local_sender_reports {
                        logged_local_sender_reports = true;
                        logging::debug(
                            "stream",
                            format!(
                                "local RTCP sender reports started: elapsed_ms={}",
                                elapsed.as_millis()
                            ),
                        );
                    } else if local_rtcp_report_ticks.is_multiple_of(10) {
                        logging::debug(
                            "stream",
                            format!(
                                "local stream RTP stats: elapsed_ms={} audio_packets={local_audio_packets} video_packets={local_video_packets} video_frames={local_video_frames} last_video_timestamp={last_local_video_timestamp:?}",
                                elapsed.as_millis()
                            ),
                        );
                    }
                }
            }
            _ = keyframe_request_interval.tick(), if !h264_startup.is_started() => {
                let source = *video_source_rx.borrow();
                if source.video_ssrc != 0 {
                    let feedback = build_rtcp_pli(local_ssrc, source.video_ssrc);
                    let encrypted = encryptor.encrypt_rtcp_feedback(
                        &feedback,
                        rtcp_feedback_nonce.to_be_bytes(),
                    )?;
                    rtcp_feedback_nonce = rtcp_feedback_nonce
                        .checked_add(1)
                        .ok_or_else(|| "stream RTCP feedback nonce exhausted".to_owned())?;
                    discord_socket
                        .send(&encrypted)
                        .await
                        .map_err(|error| format!("send stream RTCP PLI failed: {error}"))?;
                    if !logged_keyframe_request {
                        logged_keyframe_request = true;
                        logging::debug(
                            "stream",
                            format!(
                                "stream keyframe request sent: sender_ssrc={local_ssrc} media_ssrc={}",
                                source.video_ssrc
                            ),
                        );
                    }
                }
            }
            status = player.wait() => {
                let status = status.map_err(|error| {
                    StreamConnectionFailure::stop(format!(
                        "wait for stream mpv failed: {error}"
                    ))
                })?;
                for log_task in player_log_tasks {
                    let _ = log_task.await;
                }
                let last_error = last_player_error.lock().await.clone();
                logging::debug("stream", format!("stream mpv exited: status={status}"));
                if status.success() {
                    return Ok(());
                }
                return Err(StreamConnectionFailure::stop(match last_error {
                    Some(error) => format!("stream mpv exited with {status}: {error}"),
                    None => format!("stream mpv exited with {status}"),
                }));
            }
            received = discord_socket.recv(&mut packet) => {
                let received =
                    received.map_err(|error| format!("stream UDP receive failed: {error}"))?;
                let packet = &packet[..received];
                if looks_like_rtcp_packet(packet) {
                    continue;
                }
                let header = match parse_rtp_header(packet) {
                    Ok(header) => header,
                    Err(_) => continue,
                };
                let source = *video_source_rx.borrow();
                // TODO: Add RTCP NACK and RTX repair. The first viewer path
                // drops retransmission packets and waits for the next complete
                // H264 frame after loss.
                if header.payload_type == DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE
                    || source.rtx_ssrc == Some(header.ssrc)
                {
                    continue;
                }
                if header.payload_type != DISCORD_VOICE_PAYLOAD_TYPE
                    && header.payload_type != DISCORD_STREAM_VIDEO_PAYLOAD_TYPE
                {
                    continue;
                }
                let decrypted = match decryptor.decrypt_packet_any(packet, &header) {
                    Ok(decrypted) => decrypted,
                    Err(error) => {
                        logging::debug("stream", format!("stream RTP decrypt failed: {error}"));
                        continue;
                    }
                };
                if header.payload_type == DISCORD_VOICE_PAYLOAD_TYPE
                    && source.audio_ssrc != 0
                    && header.ssrc == source.audio_ssrc
                {
                    let media = dave_state
                        .lock()
                        .await
                        .unwrap_media_payload_for_ssrc(header.ssrc, &decrypted.media_payload);
                    let opus = match media {
                        VoiceMediaPayload::Plain(opus)
                        | VoiceMediaPayload::DaveDecrypted { opus, .. } => opus,
                        _ => continue,
                    };
                    let real_audio_at = media_started_at.elapsed();
                    let local_timestamp = local_audio_clock.rebase(
                        header.timestamp,
                        real_audio_at,
                        OPUS_RTP_CLOCK_RATE,
                    );
                    let packet = build_local_rtp_packet(
                        LOCAL_STREAM_AUDIO_PAYLOAD_TYPE,
                        header.marker,
                        local_audio_sequence,
                        local_timestamp,
                        source.audio_ssrc,
                        &opus,
                    );
                    local_audio_sequence = local_audio_sequence.wrapping_add(1);
                    let _ = local_socket.send_to(&packet, audio_target).await;
                    local_audio_packets = local_audio_packets.wrapping_add(1);
                    local_audio_octets = local_audio_octets.wrapping_add(opus.len() as u32);
                    if !logged_first_audio {
                        logged_first_audio = true;
                        logging::debug(
                            "stream",
                            format!(
                                "first stream audio forwarded: elapsed_ms={} source_timestamp={} local_timestamp={}",
                                media_started_at.elapsed().as_millis(),
                                header.timestamp,
                                local_timestamp
                            ),
                        );
                    }
                } else if header.payload_type == DISCORD_STREAM_VIDEO_PAYLOAD_TYPE
                    && source.video_ssrc != 0
                    && header.ssrc == source.video_ssrc
                    && let Some(frame) = h264.push(&header, &decrypted.media_payload)
                {
                    let frame = match dave_state
                        .lock()
                        .await
                        .decrypt_video_frame(owner_id, &frame)
                    {
                        Ok(Some(frame)) => frame,
                        Ok(None) => continue,
                        Err(error) => {
                            logging::debug("stream", error);
                            continue;
                        }
                    };
                    if !logged_first_video_frame {
                        logged_first_video_frame = true;
                        logging::debug(
                            "stream",
                            format!(
                                "first stream video frame decrypted: elapsed_ms={} nal_types={:?}",
                                media_started_at.elapsed().as_millis(),
                                h264_nal_types(&frame),
                            ),
                        );
                    }
                    let player_ready = video_player_ready.load(Ordering::Acquire);
                    if player_ready && !h264_startup_buffer.is_empty() {
                        let buffered_frames = h264_startup_buffer.len();
                        let replay_origin = elapsed_rtp_timestamp(
                            media_started_at.elapsed(),
                            VIDEO_RTP_CLOCK_RATE,
                        );
                        let mut replay_anchor = None;
                        for (index, buffered) in h264_startup_buffer.drain(..).enumerate() {
                            let local_timestamp = replay_origin.wrapping_add(
                                u32::try_from(index)
                                    .expect("startup buffer length is bounded")
                                    .wrapping_mul(STREAM_STARTUP_REPLAY_FRAME_TICKS),
                            );
                            let (packet_count, octet_count) = send_local_h264_frame(
                                &local_socket,
                                video_target,
                                &buffered.encoded,
                                local_timestamp,
                                source.video_ssrc,
                                &mut local_video_sequence,
                            )
                            .await;
                            local_video_packets = local_video_packets.wrapping_add(packet_count);
                            local_video_octets = local_video_octets.wrapping_add(octet_count);
                            local_video_frames = local_video_frames.wrapping_add(1);
                            last_local_video_timestamp = Some(local_timestamp);
                            replay_anchor = Some((buffered.source_timestamp, local_timestamp));
                            if !logged_first_video {
                                logged_first_video = true;
                                logging::debug(
                                    "stream",
                                    format!(
                                        "first buffered stream video forwarded: elapsed_ms={} source_timestamp={} local_timestamp={local_timestamp}",
                                        media_started_at.elapsed().as_millis(),
                                        buffered.source_timestamp,
                                    ),
                                );
                            }
                        }
                        if let Some((source_timestamp, local_timestamp)) = replay_anchor {
                            local_video_clock.anchor(source_timestamp, local_timestamp);
                        }
                        logging::debug(
                            "stream",
                            format!(
                                "buffered H264 startup replayed: elapsed_ms={} frames={buffered_frames}",
                                media_started_at.elapsed().as_millis()
                            ),
                        );
                    }
                    if !player_ready && !logged_video_before_player_ready {
                        logged_video_before_player_ready = true;
                        logging::debug(
                            "stream",
                            "buffering stream video until mpv opens its SDP input",
                        );
                    }
                    let waiting_for_keyframe = !h264_startup.is_started();
                    let frame = accept_or_buffer_h264(
                        player_ready,
                        &mut h264_startup,
                        &mut h264_startup_buffer,
                        frame,
                        header.timestamp,
                    );
                    if waiting_for_keyframe && h264_startup.is_started() {
                        logging::debug(
                            "stream",
                            format!(
                                "stream H264 keyframe {}: elapsed_ms={}",
                                if player_ready { "accepted" } else { "buffered" },
                                media_started_at.elapsed().as_millis()
                            ),
                        );
                    }
                    let Some(frame) = frame else {
                        continue;
                    };
                    let local_timestamp = local_video_clock.rebase(
                        frame.source_timestamp,
                        media_started_at.elapsed(),
                        VIDEO_RTP_CLOCK_RATE,
                    );
                    let (packet_count, octet_count) = send_local_h264_frame(
                        &local_socket,
                        video_target,
                        &frame.encoded,
                        local_timestamp,
                        source.video_ssrc,
                        &mut local_video_sequence,
                    )
                    .await;
                    local_video_packets = local_video_packets.wrapping_add(packet_count);
                    local_video_octets = local_video_octets.wrapping_add(octet_count);
                    local_video_frames = local_video_frames.wrapping_add(1);
                    last_local_video_timestamp = Some(local_timestamp);
                    if !logged_first_video {
                        logged_first_video = true;
                        logging::debug(
                            "stream",
                            format!(
                                "first stream video forwarded: elapsed_ms={} source_timestamp={} local_timestamp={}",
                                media_started_at.elapsed().as_millis(),
                                frame.source_timestamp,
                                local_timestamp
                            ),
                        );
                    }
                }
            }
        }
    }
}

fn stream_player_spawn_failure(error: std::io::Error) -> StreamConnectionFailure {
    let message = if error.kind() == std::io::ErrorKind::NotFound {
        "mpv is required to watch Discord streams; install mpv and make sure it is on PATH"
            .to_owned()
    } else {
        format!("start mpv for stream failed: {error}")
    };
    StreamConnectionFailure::stop(message)
}

fn stream_player_command(sdp_path: &Path) -> Command {
    let mut player = Command::new("mpv");
    player
        // Keep playback deterministic and prevent user cache settings from
        // turning this live input into a delayed stream.
        .arg("--no-config")
        // mpv disables terminal logs when its output is redirected. Force the
        // line-based log stream on so Concord can capture player lifecycle
        // events through stdout.
        .arg("--terminal=yes")
        // Built-in UI scripts delay SDP socket creation and are not needed for
        // the dedicated stream window.
        .arg("--load-scripts=no")
        // Keep normal output quiet, but include the lifecycle stages needed to
        // separate SDP, decoder, and display startup delay in a live log.
        .arg("--msg-level=all=warn,cplayer=v,lavf=v,vd=v,ad=v")
        .arg("--profile=low-latency")
        .arg("--cache=no")
        .arg("--demuxer-readahead-secs=0")
        .arg("--demuxer=lavf")
        .arg("--demuxer-lavf-format=sdp")
        .arg("--demuxer-lavf-probesize=32")
        .arg("--demuxer-lavf-buffersize=4096")
        // mpv uses commas between lavf key/value options. Square brackets keep
        // the protocol list together as one value.
        .arg("--demuxer-lavf-o=protocol_whitelist=[file,udp,rtp],max_delay=0,reorder_queue_size=0")
        .arg("--force-window=immediate")
        // Audio and video share one player so its volume and mute controls
        // apply to the complete broadcast.
        .arg("--video-sync=desync")
        // Skip late output frames instead of preserving live delay.
        // Decoder dropping can discard H264 reference frames.
        .arg("--framedrop=vo")
        .arg("--video-timing-offset=0")
        .arg("--")
        .arg(sdp_path)
        // Both output streams remain enabled so startup stages and failures
        // reach the Concord log. stdin is closed so mpv cannot consume TUI
        // input.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    player
}

#[derive(Default)]
struct LocalRtpClock {
    source_origin: Option<u32>,
    local_origin: u32,
}

impl LocalRtpClock {
    fn anchor(&mut self, source_timestamp: u32, local_timestamp: u32) {
        self.source_origin = Some(source_timestamp);
        self.local_origin = local_timestamp;
    }

    fn rebase(&mut self, source_timestamp: u32, elapsed: Duration, clock_rate: u32) -> u32 {
        let source_origin = *self.source_origin.get_or_insert_with(|| {
            self.local_origin = elapsed_rtp_timestamp(elapsed, clock_rate);
            source_timestamp
        });
        self.local_origin
            .wrapping_add(source_timestamp.wrapping_sub(source_origin))
    }
}

fn elapsed_rtp_timestamp(elapsed: Duration, clock_rate: u32) -> u32 {
    let whole = elapsed.as_secs().wrapping_mul(u64::from(clock_rate));
    let fractional =
        u64::from(elapsed.subsec_nanos()).wrapping_mul(u64::from(clock_rate)) / 1_000_000_000;
    whole.wrapping_add(fractional) as u32
}

fn build_rtcp_pli(sender_ssrc: u32, media_ssrc: u32) -> [u8; 12] {
    let mut packet = [0u8; 12];
    packet[0] = (RTP_VERSION << 6) | RTCP_PLI_FORMAT;
    packet[1] = RTCP_PAYLOAD_SPECIFIC_FEEDBACK;
    packet[2..4].copy_from_slice(&RTCP_PLI_LENGTH_WORDS_MINUS_ONE.to_be_bytes());
    packet[4..8].copy_from_slice(&sender_ssrc.to_be_bytes());
    packet[8..12].copy_from_slice(&media_ssrc.to_be_bytes());
    packet
}

fn build_local_rtcp_sender_report(
    sender_ssrc: u32,
    unix_time: Duration,
    rtp_timestamp: u32,
    packet_count: u32,
    octet_count: u32,
) -> [u8; 28] {
    let ntp_seconds = unix_time
        .as_secs()
        .wrapping_add(NTP_UNIX_EPOCH_OFFSET_SECONDS) as u32;
    let ntp_fraction = ((u64::from(unix_time.subsec_nanos()) << 32) / 1_000_000_000) as u32;
    let mut report = [0u8; 28];
    report[0] = RTP_VERSION << 6;
    report[1] = RTCP_SENDER_REPORT_PACKET_TYPE;
    report[2..4].copy_from_slice(&RTCP_SENDER_REPORT_LENGTH_WORDS_MINUS_ONE.to_be_bytes());
    report[4..8].copy_from_slice(&sender_ssrc.to_be_bytes());
    report[8..12].copy_from_slice(&ntp_seconds.to_be_bytes());
    report[12..16].copy_from_slice(&ntp_fraction.to_be_bytes());
    report[16..20].copy_from_slice(&rtp_timestamp.to_be_bytes());
    report[20..24].copy_from_slice(&packet_count.to_be_bytes());
    report[24..28].copy_from_slice(&octet_count.to_be_bytes());
    report
}

async fn log_stream_player_output(
    kind: &'static str,
    output: &'static str,
    stream: impl AsyncRead + Unpin,
    last_error: Arc<Mutex<Option<String>>>,
    player_ready: Option<StreamPlayerReadySignal>,
) {
    let mut lines = BufReader::new(stream).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) if !line.trim().is_empty() => {
                let input_ready = stream_player_input_is_ready(&line);
                logging::debug("stream", format!("mpv {kind} {output}: {line}"));
                *last_error.lock().await = Some(line);
                if let Some(player_ready) = player_ready.as_ref()
                    && input_ready
                    && !player_ready.player_ready.swap(true, Ordering::AcqRel)
                {
                    logging::debug("stream", format!("stream {kind} mpv input ready"));
                    let _ = player_ready.ready_tx.send(player_ready.media_generation);
                    player_ready
                        .status_publisher
                        .publish_stream_playback_ready(
                            player_ready.scope,
                            player_ready.channel_id,
                            player_ready.user_id,
                        )
                        .await;
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(error) => {
                logging::debug(
                    "stream",
                    format!("read mpv {kind} {output} failed: {error}"),
                );
                break;
            }
        }
    }
}

fn stream_player_input_is_ready(line: &str) -> bool {
    line.contains("[cplayer] Opening done:")
}

struct ReservedLocalUdpPortPair {
    _rtp_socket: std::net::UdpSocket,
    _rtcp_socket: std::net::UdpSocket,
    rtp_port: u16,
    rtcp_port: u16,
}

fn reserve_local_udp_port_pair() -> Result<ReservedLocalUdpPortPair, String> {
    for _ in 0..64 {
        let rtp_socket = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| format!("reserve local stream RTP port failed: {error}"))?;
        let rtp_port = rtp_socket
            .local_addr()
            .map_err(|error| format!("read local stream RTP port failed: {error}"))?
            .port();
        let Some(rtcp_port) = rtp_port
            .checked_add(1)
            .filter(|_| rtp_port.is_multiple_of(2))
        else {
            continue;
        };
        let Ok(rtcp_socket) = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, rtcp_port)) else {
            continue;
        };
        return Ok(ReservedLocalUdpPortPair {
            _rtp_socket: rtp_socket,
            _rtcp_socket: rtcp_socket,
            rtp_port,
            rtcp_port,
        });
    }
    Err("reserve adjacent local stream RTP and RTCP ports failed".to_owned())
}

fn stream_sdp(
    audio_port: u16,
    audio_rtcp_port: u16,
    video_port: u16,
    video_rtcp_port: u16,
) -> String {
    format!(
        "v=0\r\n\
         o=- 0 0 IN IP4 127.0.0.1\r\n\
         s=Concord Discord Stream\r\n\
         c=IN IP4 127.0.0.1\r\n\
         t=0 0\r\n\
         m=audio {audio_port} RTP/AVP {LOCAL_STREAM_AUDIO_PAYLOAD_TYPE}\r\n\
         a=rtcp:{audio_rtcp_port} IN IP4 127.0.0.1\r\n\
         a=rtpmap:{LOCAL_STREAM_AUDIO_PAYLOAD_TYPE} opus/48000/2\r\n\
         a=recvonly\r\n\
         m=video {video_port} RTP/AVP {LOCAL_STREAM_VIDEO_PAYLOAD_TYPE}\r\n\
         a=rtcp:{video_rtcp_port} IN IP4 127.0.0.1\r\n\
         a=rtpmap:{LOCAL_STREAM_VIDEO_PAYLOAD_TYPE} H264/90000\r\n\
         a=fmtp:{LOCAL_STREAM_VIDEO_PAYLOAD_TYPE} packetization-mode=1\r\n\
         a=recvonly\r\n"
    )
}

fn build_local_rtp_packet(
    payload_type: u8,
    marker: bool,
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut packet = Vec::with_capacity(RTP_HEADER_MIN_LEN + payload.len());
    packet.push(RTP_VERSION << 6);
    packet.push((u8::from(marker) << 7) | payload_type);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&timestamp.to_be_bytes());
    packet.extend_from_slice(&ssrc.to_be_bytes());
    packet.extend_from_slice(payload);
    packet
}

#[derive(Default)]
struct H264Depacketizer {
    timestamp: Option<u32>,
    expected_sequence: Option<u16>,
    frame: Vec<u8>,
    fragment_open: bool,
}

#[derive(Default)]
struct H264StartupGate {
    started: bool,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
}

struct BufferedH264Frame {
    encoded: Vec<u8>,
    source_timestamp: u32,
}

impl H264StartupGate {
    fn is_started(&self) -> bool {
        self.started
    }

    fn accept(&mut self, frame: Vec<u8>) -> Option<Vec<u8>> {
        let (has_idr, has_sps, has_pps) = {
            let mut has_idr = false;
            let mut has_sps = false;
            let mut has_pps = false;
            for nal in annex_b_nals(&frame) {
                match nal.first().map(|byte| byte & 0x1f) {
                    Some(5) => has_idr = true,
                    Some(7) => {
                        has_sps = true;
                        self.sps = Some(nal.to_vec());
                    }
                    Some(8) => {
                        has_pps = true;
                        self.pps = Some(nal.to_vec());
                    }
                    _ => {}
                }
            }
            (has_idr, has_sps, has_pps)
        };

        if self.started {
            return Some(frame);
        }
        if !has_idr {
            return None;
        }

        self.started = true;
        if (has_sps || self.sps.is_none()) && (has_pps || self.pps.is_none()) {
            return Some(frame);
        }

        let mut startup_frame = Vec::new();
        if !has_sps && let Some(sps) = self.sps.as_deref() {
            append_annex_b_nal(&mut startup_frame, sps);
        }
        if !has_pps && let Some(pps) = self.pps.as_deref() {
            append_annex_b_nal(&mut startup_frame, pps);
        }
        startup_frame.extend_from_slice(&frame);
        Some(startup_frame)
    }
}

fn accept_or_buffer_h264(
    player_ready: bool,
    startup: &mut H264StartupGate,
    startup_buffer: &mut VecDeque<BufferedH264Frame>,
    frame: Vec<u8>,
    source_timestamp: u32,
) -> Option<BufferedH264Frame> {
    let frame = BufferedH264Frame {
        encoded: startup.accept(frame)?,
        source_timestamp,
    };
    if player_ready {
        return Some(frame);
    }
    if startup_buffer.len() >= STREAM_STARTUP_BUFFER_MAX_FRAMES {
        startup_buffer.clear();
        *startup = H264StartupGate::default();
        return None;
    }
    startup_buffer.push_back(frame);
    None
}

fn append_annex_b_nal(frame: &mut Vec<u8>, nal: &[u8]) {
    frame.extend_from_slice(&[0, 0, 0, 1]);
    frame.extend_from_slice(nal);
}

fn h264_nal_types(frame: &[u8]) -> Vec<u8> {
    annex_b_nals(frame)
        .into_iter()
        .filter_map(|nal| nal.first().map(|byte| byte & 0x1f))
        .collect()
}

impl H264Depacketizer {
    fn push(&mut self, header: &RtpHeader, payload: &[u8]) -> Option<Vec<u8>> {
        if self.timestamp != Some(header.timestamp)
            || self
                .expected_sequence
                .is_some_and(|expected| expected != header.sequence)
        {
            self.reset(header.timestamp);
        }
        self.expected_sequence = Some(header.sequence.wrapping_add(1));
        let nal_type = payload.first().map(|byte| byte & 0x1f)?;
        let accepted = match nal_type {
            1..=23 => {
                self.append_nal(payload);
                true
            }
            24 => self.append_stap_a(payload),
            28 => self.append_fu_a(payload),
            _ => false,
        };
        if !accepted {
            self.reset(header.timestamp);
            return None;
        }
        if header.marker {
            if self.fragment_open || self.frame.is_empty() {
                self.reset(header.timestamp);
                return None;
            }
            self.timestamp = None;
            self.expected_sequence = None;
            return Some(std::mem::take(&mut self.frame));
        }
        None
    }

    fn reset(&mut self, timestamp: u32) {
        self.timestamp = Some(timestamp);
        self.expected_sequence = None;
        self.frame.clear();
        self.fragment_open = false;
    }

    fn append_nal(&mut self, nal: &[u8]) {
        self.frame.extend_from_slice(&[0, 0, 0, 1]);
        self.frame.extend_from_slice(nal);
        self.fragment_open = false;
    }

    fn append_stap_a(&mut self, payload: &[u8]) -> bool {
        let mut cursor = 1usize;
        let mut found = false;
        while cursor + 2 <= payload.len() {
            let size = usize::from(u16::from_be_bytes([payload[cursor], payload[cursor + 1]]));
            cursor += 2;
            let Some(nal) = payload.get(cursor..cursor.saturating_add(size)) else {
                return false;
            };
            if nal.is_empty() {
                return false;
            }
            self.append_nal(nal);
            cursor += size;
            found = true;
        }
        found && cursor == payload.len()
    }

    fn append_fu_a(&mut self, payload: &[u8]) -> bool {
        if payload.len() < 3 {
            return false;
        }
        let indicator = payload[0];
        let fu_header = payload[1];
        let start = fu_header & 0x80 != 0;
        let end = fu_header & 0x40 != 0;
        if start {
            self.frame.extend_from_slice(&[0, 0, 0, 1]);
            self.frame.push((indicator & 0xe0) | (fu_header & 0x1f));
            self.fragment_open = !end;
        } else if !self.fragment_open {
            return false;
        } else if end {
            self.fragment_open = false;
        }
        self.frame.extend_from_slice(&payload[2..]);
        true
    }
}

fn packetize_h264_frame(
    frame: &[u8],
    timestamp: u32,
    ssrc: u32,
    sequence: &mut u16,
) -> Vec<Vec<u8>> {
    let nals = annex_b_nals(frame);
    let mut payloads = Vec::new();
    for nal in nals {
        if nal.len() <= LOCAL_H264_MAX_PAYLOAD_BYTES {
            payloads.push(nal.to_vec());
            continue;
        }
        let Some((&nal_header, body)) = nal.split_first() else {
            continue;
        };
        let fu_indicator = (nal_header & 0xe0) | 28;
        let nal_type = nal_header & 0x1f;
        let chunks = body.chunks(LOCAL_H264_MAX_PAYLOAD_BYTES - 2);
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
            let packet = build_local_rtp_packet(
                LOCAL_STREAM_VIDEO_PAYLOAD_TYPE,
                index + 1 == payload_count,
                *sequence,
                timestamp,
                ssrc,
                &payload,
            );
            *sequence = sequence.wrapping_add(1);
            packet
        })
        .collect()
}

async fn send_local_h264_frame(
    socket: &UdpSocket,
    target: SocketAddrV4,
    frame: &[u8],
    timestamp: u32,
    ssrc: u32,
    sequence: &mut u16,
) -> (u32, u32) {
    let packets = packetize_h264_frame(frame, timestamp, ssrc, sequence);
    let packet_count = u32::try_from(packets.len()).expect("H264 packet count fits u32");
    let octet_count = packets.iter().fold(0u32, |total, packet| {
        total.wrapping_add(packet.len().saturating_sub(RTP_HEADER_MIN_LEN) as u32)
    });
    for packet in packets {
        let _ = socket.send_to(&packet, target).await;
    }
    (packet_count, octet_count)
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

    fn stream_request() -> StreamWatchRequest {
        StreamWatchRequest {
            stream_key: "guild:10:20:99".to_owned(),
            scope: VoiceScope::Guild(Id::new(10)),
            channel_id: Id::new(20),
            owner_id: Id::new(99),
            display_name: "Streamer".to_owned(),
        }
    }

    fn current_voice_state() -> VoiceStateInfo {
        VoiceStateInfo {
            guild_id: Some(Id::new(10)),
            channel_id: Some(Id::new(20)),
            user_id: Id::new(5),
            session_id: Some("parent-session".to_owned()),
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        }
    }

    fn connected_stream_runtime() -> (StreamRuntimeState, StreamGatewaySession) {
        let mut state = StreamRuntimeState::default();
        state.apply(&VoiceRuntimeEvent::WatchStreamRequested(stream_request()));
        state.apply(&VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(5))));
        state.apply(&VoiceRuntimeEvent::VoiceState(current_voice_state()));
        state.apply(&VoiceRuntimeEvent::StreamCreate(StreamCreateInfo {
            stream_key: "guild:10:20:99".to_owned(),
            rtc_server_id: "400".to_owned(),
            rtc_channel_id: Id::new(401),
        }));
        let update = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: "guild:10:20:99".to_owned(),
            endpoint: Some("stream.example.com".to_owned()),
            token: "stream-token".to_owned(),
        }));
        let session = update.connect.expect("stream session should be ready");
        (state, session)
    }

    fn stream_connection_ended(
        session: &StreamGatewaySession,
        outcome: VoiceConnectionEnd,
    ) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::StreamConnectionEnded {
            connection_id: session.connection_id,
            stream_key: session.request.stream_key.clone(),
            outcome,
        }
    }

    #[test]
    fn h264_fu_a_round_trips_through_local_packetizer() {
        let frame = [0, 0, 0, 1, 0x65]
            .into_iter()
            .chain(std::iter::repeat_n(0xaa, 3000))
            .collect::<Vec<_>>();
        let mut sequence = 7;
        let packets = packetize_h264_frame(&frame, 90_000, 42, &mut sequence);
        assert!(packets.len() > 1);

        let mut depacketizer = H264Depacketizer::default();
        let mut decoded = None;
        for packet in packets {
            let header = parse_rtp_header(&packet).expect("local RTP packet is valid");
            decoded = depacketizer.push(&header, &packet[header.payload_offset..]);
        }
        assert_eq!(decoded, Some(frame));
    }

    #[test]
    fn stream_video_starts_at_idr_with_cached_parameter_sets() {
        let parameter_sets = vec![0, 0, 0, 1, 0x67, 0x11, 0, 0, 0, 1, 0x68, 0x22];
        let predicted = vec![0, 0, 0, 1, 0x41, 0x33];
        let idr = vec![0, 0, 0, 1, 0x65, 0x44];
        let mut gate = H264StartupGate::default();

        assert_eq!(gate.accept(parameter_sets), None);
        assert_eq!(gate.accept(predicted.clone()), None);

        let startup = gate
            .accept(idr)
            .expect("IDR should start local video playback");
        assert_eq!(h264_nal_types(&startup), vec![7, 8, 5]);
        assert!(gate.is_started());
        assert_eq!(gate.accept(predicted.clone()), Some(predicted));
    }

    #[test]
    fn stream_video_replays_the_initial_gop_after_player_readiness() {
        let startup_frame = vec![
            0, 0, 0, 1, 0x67, 0x11, 0, 0, 0, 1, 0x68, 0x22, 0, 0, 0, 1, 0x65, 0x33,
        ];
        let predicted = vec![0, 0, 0, 1, 0x41, 0x44];
        let mut gate = H264StartupGate::default();
        let mut buffer = VecDeque::new();

        assert_eq!(
            accept_or_buffer_h264(false, &mut gate, &mut buffer, startup_frame.clone(), 90_000,)
                .map(|frame| frame.encoded),
            None
        );
        assert!(gate.is_started());
        assert_eq!(buffer.len(), 1);
        assert_eq!(
            accept_or_buffer_h264(false, &mut gate, &mut buffer, predicted.clone(), 93_000,)
                .map(|frame| frame.encoded),
            None
        );
        assert_eq!(buffer.len(), 2);
        assert_eq!(
            buffer
                .pop_front()
                .map(|frame| (frame.encoded, frame.source_timestamp)),
            Some((startup_frame, 90_000))
        );
        assert_eq!(
            buffer
                .pop_front()
                .map(|frame| (frame.encoded, frame.source_timestamp)),
            Some((predicted.clone(), 93_000))
        );
        assert_eq!(
            accept_or_buffer_h264(true, &mut gate, &mut buffer, predicted.clone(), 96_000)
                .map(|frame| (frame.encoded, frame.source_timestamp)),
            Some((predicted, 96_000))
        );

        let mut clock = LocalRtpClock::default();
        clock.anchor(93_000, 10_000);
        assert_eq!(
            clock.rebase(96_000, Duration::from_secs(10), VIDEO_RTP_CLOCK_RATE),
            13_000
        );
        assert!(stream_player_input_is_ready(
            "[cplayer] Opening done: /tmp/concord-video.sdp"
        ));
        assert!(!stream_player_input_is_ready(
            "[cplayer] Starting playback..."
        ));
    }

    #[test]
    fn stream_keyframe_request_encrypts_only_the_rtcp_feedback_body() {
        let sender_ssrc = 0x0102_0304;
        let media_ssrc = 0x0506_0708;
        let feedback = build_rtcp_pli(sender_ssrc, media_ssrc);
        assert_eq!(&feedback[..4], &[0x81, 206, 0, 2]);
        assert_eq!(&feedback[4..8], &sender_ssrc.to_be_bytes());
        assert_eq!(&feedback[8..], &media_ssrc.to_be_bytes());

        for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
            let key = [0x42; 32];
            let encryptor =
                VoiceRtpEncryptor::new(mode, &key).expect("feedback encryptor should initialize");
            let encrypted = encryptor
                .encrypt_rtcp_feedback(&feedback, 9u32.to_be_bytes())
                .expect("RTCP feedback should encrypt");
            assert_eq!(&encrypted[..8], &feedback[..8]);
            assert_eq!(
                encrypted.len(),
                8 + 4 + RTP_AEAD_TAG_BYTES + RTP_AEAD_NONCE_SUFFIX_BYTES
            );

            let decryptor =
                VoiceRtpDecryptor::new(mode, &key).expect("feedback decryptor should initialize");
            let decrypted = decryptor
                .decrypt_rtcp_feedback(&encrypted)
                .expect("RTCP feedback body should decrypt");
            assert_eq!(decrypted, feedback);
        }
    }

    #[test]
    fn stream_compound_rtcp_feedback_round_trips() {
        let mut feedback = build_rtcp_pli(0x0102_0304, 0x0506_0708).to_vec();
        feedback.extend_from_slice(&build_rtcp_pli(0x1112_1314, 0x1516_1718));

        for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
            let key = [0x42; 32];
            let encryptor =
                VoiceRtpEncryptor::new(mode, &key).expect("feedback encryptor should initialize");
            let encrypted = encryptor
                .encrypt_rtcp_feedback(&feedback, 10u32.to_be_bytes())
                .expect("compound RTCP feedback should encrypt");
            let decryptor =
                VoiceRtpDecryptor::new(mode, &key).expect("feedback decryptor should initialize");
            let decrypted = decryptor
                .decrypt_rtcp_feedback(&encrypted)
                .expect("compound RTCP feedback should decrypt");

            assert_eq!(decrypted, feedback);
        }
    }

    #[test]
    fn local_sender_report_maps_rtp_to_a_shared_ntp_clock() {
        let report = build_local_rtcp_sender_report(
            0x0102_0304,
            Duration::new(1, 500_000_000),
            90_000,
            30,
            45_000,
        );

        assert_eq!(&report[..4], &[0x80, 200, 0, 6]);
        assert_eq!(&report[4..8], &0x0102_0304u32.to_be_bytes());
        assert_eq!(
            &report[8..12],
            &((NTP_UNIX_EPOCH_OFFSET_SECONDS + 1) as u32).to_be_bytes()
        );
        assert_eq!(&report[12..16], &0x8000_0000u32.to_be_bytes());
        assert_eq!(&report[16..20], &90_000u32.to_be_bytes());
        assert_eq!(&report[20..24], &30u32.to_be_bytes());
        assert_eq!(&report[24..28], &45_000u32.to_be_bytes());
    }

    #[test]
    fn stream_sdp_keeps_audio_and_video_on_one_input() {
        let sdp = stream_sdp(50_000, 50_001, 50_002, 50_003);

        assert!(sdp.contains("m=audio 50000 RTP/AVP 111\r\n"));
        assert!(sdp.contains("a=rtcp:50001 IN IP4 127.0.0.1\r\n"));
        assert!(sdp.contains("m=video 50002 RTP/AVP 96\r\n"));
        assert!(sdp.contains("a=rtcp:50003 IN IP4 127.0.0.1\r\n"));
    }

    #[test]
    fn stream_video_source_prefers_highest_active_quality() {
        let value = json!({
            "op": 12,
            "d": {
                "user_id": "99",
                "audio_ssrc": 10,
                "video_ssrc": 20,
                "streams": [
                    {"ssrc": 20, "rtx_ssrc": 21, "quality": 50, "active": true},
                    {"ssrc": 30, "rtx_ssrc": 31, "quality": 100, "active": true}
                ]
            }
        });
        assert_eq!(
            parse_stream_video_source(&value, Id::new(99)),
            Some(StreamVideoSource {
                audio_ssrc: 10,
                video_ssrc: 30,
                rtx_ssrc: Some(31),
            })
        );
    }

    #[test]
    fn stream_runtime_waits_for_parent_voice_and_both_stream_events() {
        let mut state = StreamRuntimeState::default();
        assert!(
            state
                .apply(&VoiceRuntimeEvent::WatchStreamRequested(stream_request()))
                .connect
                .is_none()
        );
        state.apply(&VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(5))));
        state.apply(&VoiceRuntimeEvent::VoiceState(current_voice_state()));
        state.apply(&VoiceRuntimeEvent::StreamCreate(StreamCreateInfo {
            stream_key: "guild:10:20:99".to_owned(),
            rtc_server_id: "400".to_owned(),
            rtc_channel_id: Id::new(401),
        }));

        let update = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: "guild:10:20:99".to_owned(),
            endpoint: Some("stream.example.com".to_owned()),
            token: "stream-token".to_owned(),
        }));
        let session = update.connect.expect("stream session should now be ready");
        assert_eq!(session.session_id, "parent-session");
        assert_eq!(session.rtc_server_id, "400");
        assert_eq!(session.rtc_channel_id, Id::new(401));
        assert_eq!(session.request.owner_id, Id::new(99));
    }

    #[test]
    fn stream_runtime_stops_after_consecutive_pre_playback_failures() {
        let (mut state, mut active) = connected_stream_runtime();

        for attempt in 1..=MAX_VOICE_RECONNECT_ATTEMPTS {
            let update = state.apply(&stream_connection_ended(
                &active,
                VoiceConnectionEnd::Reconnect,
            ));
            assert!(
                update.close_stream_key.is_none(),
                "retry {attempt} should keep the watch request active"
            );
            active = update
                .connect
                .expect("retry within the limit should reconnect");
        }

        let stopped = state.apply(&stream_connection_ended(
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
    fn stream_runtime_stops_immediately_after_terminal_failure() {
        let (mut state, active) = connected_stream_runtime();

        let stopped = state.apply(&stream_connection_ended(&active, VoiceConnectionEnd::Stop));

        assert!(stopped.connect.is_none());
        assert_eq!(
            stopped.close_stream_key.as_deref(),
            Some(active.request.stream_key.as_str())
        );
        assert!(stopped.send_delete);
    }

    #[test]
    fn stream_runtime_resets_retries_only_after_stable_playback() {
        let (mut state, initial) = connected_stream_runtime();
        let first_retry = state.apply(&stream_connection_ended(
            &initial,
            VoiceConnectionEnd::Reconnect,
        ));
        let mut active = first_retry
            .connect
            .expect("the first transport failure should reconnect");

        state.apply(&VoiceRuntimeEvent::StreamConnectionEstablished {
            connection_id: active.connection_id,
            stream_key: active.request.stream_key.clone(),
        });

        for _ in 0..MAX_VOICE_RECONNECT_ATTEMPTS {
            active = state
                .apply(&stream_connection_ended(
                    &active,
                    VoiceConnectionEnd::Reconnect,
                ))
                .connect
                .expect("stable playback should restore the full retry budget");
        }
    }

    #[test]
    fn stream_failures_distinguish_player_and_transport_errors() {
        for error in [
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "not executable"),
        ] {
            let failure = stream_player_spawn_failure(error);
            assert_eq!(failure.outcome, VoiceConnectionEnd::Stop);
        }
        assert_eq!(
            StreamConnectionFailure::from("stream UDP receive failed".to_owned()).outcome,
            VoiceConnectionEnd::Reconnect
        );
    }

    #[test]
    fn stream_gateway_payloads_request_stream_audio_and_h264() {
        let session = StreamGatewaySession {
            connection_id: 1,
            request: stream_request(),
            current_user_id: Id::new(5),
            session_id: "parent-session".to_owned(),
            rtc_server_id: "400".to_owned(),
            rtc_channel_id: Id::new(401),
            endpoint: "stream.example.com".to_owned(),
            token: "stream-token".to_owned(),
        };
        let identify: Value =
            serde_json::from_str(&stream_identify_payload(&session)).expect("valid identify json");
        assert_eq!(identify["d"]["server_id"], "400");
        assert_eq!(identify["d"]["channel_id"], "401");
        assert_eq!(identify["d"]["video"], true);
        assert!(identify["d"].get("streams").is_none());

        let selected: Value = serde_json::from_str(&stream_select_protocol_payload(
            &DiscoveredVoiceAddress {
                address: "127.0.0.1".to_owned(),
                port: 5000,
            },
            AEAD_XCHACHA20_POLY1305_RTPSIZE,
        ))
        .expect("valid select protocol json");
        assert_eq!(selected["d"]["codecs"][0]["name"], "opus");
        assert_eq!(selected["d"]["codecs"][0]["payload_type"], 120);
        assert_eq!(selected["d"]["codecs"][0]["encode"], false);
        assert_eq!(selected["d"]["codecs"][0]["decode"], true);
        assert_eq!(selected["d"]["codecs"][1]["name"], "H264");
        assert_eq!(selected["d"]["codecs"][1]["payload_type"], 103);
        assert_eq!(selected["d"]["codecs"][1]["decode"], true);

        let wants: Value = serde_json::from_str(&stream_media_sink_wants_payload(800, 900))
            .expect("valid media sink wants json");
        assert_eq!(
            wants,
            json!({"op": 15, "d": {"800": 100, "900": 100, "any": 0}})
        );
    }

    #[test]
    fn stream_player_controls_the_complete_broadcast() {
        let player = stream_player_command(Path::new("/tmp/concord-stream.sdp"))
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            player,
            vec![
                "--no-config",
                "--terminal=yes",
                "--load-scripts=no",
                "--msg-level=all=warn,cplayer=v,lavf=v,vd=v,ad=v",
                "--profile=low-latency",
                "--cache=no",
                "--demuxer-readahead-secs=0",
                "--demuxer=lavf",
                "--demuxer-lavf-format=sdp",
                "--demuxer-lavf-probesize=32",
                "--demuxer-lavf-buffersize=4096",
                "--demuxer-lavf-o=protocol_whitelist=[file,udp,rtp],max_delay=0,reorder_queue_size=0",
                "--force-window=immediate",
                "--video-sync=desync",
                "--framedrop=vo",
                "--video-timing-offset=0",
                "--",
                "/tmp/concord-stream.sdp",
            ]
        );
    }

    #[test]
    fn local_rtp_clocks_share_live_time_without_source_clock_offsets() {
        let mut audio = LocalRtpClock::default();
        let mut video = LocalRtpClock::default();

        assert_eq!(
            audio.rebase(3_000_000, Duration::from_millis(100), OPUS_RTP_CLOCK_RATE),
            4_800
        );
        assert_eq!(
            video.rebase(90_000_000, Duration::from_millis(125), VIDEO_RTP_CLOCK_RATE),
            11_250
        );
        assert_eq!(
            audio.rebase(3_000_960, Duration::from_millis(120), OPUS_RTP_CLOCK_RATE),
            5_760
        );
        assert_eq!(
            video.rebase(90_003_000, Duration::from_millis(158), VIDEO_RTP_CLOCK_RATE),
            14_250
        );
    }
}
