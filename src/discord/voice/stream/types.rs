use std::sync::atomic::AtomicBool;

use rand::random;

use super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS;
use super::*;

pub const STREAM_RTP_PACKET_BYTES: usize = 4096;
pub const LOCAL_H264_MAX_PAYLOAD_BYTES: usize = 1200;
pub const STREAM_STARTUP_BUFFER_MAX_FRAMES: usize = 180;
pub const STREAM_STARTUP_BUFFER_MAX_BYTES: usize = 32 * 1024 * 1024;
pub const STREAM_H264_ACCESS_UNIT_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const STREAM_H264_ACCESS_UNIT_MAX_PACKETS: usize = 4096;
pub const STREAM_STARTUP_REPLAY_FRAME_TICKS: u32 = 90;
pub const STREAM_PLAYER_READY_TIMEOUT: Duration = Duration::from_secs(10);
pub const STREAM_PLAYER_AUDIO_ENABLE_TIMEOUT: Duration = Duration::from_secs(1);
pub const STREAM_PLAYER_INPUT_CONFIG: &str = "SPACE ignore\np ignore\nPAUSE ignore\nPLAYPAUSE ignore\nPAUSEONLY ignore\nXF86_PAUSE ignore\n. ignore\n, ignore\n";
pub const OPUS_RTP_CLOCK_RATE: u32 = 48_000;
pub const VIDEO_RTP_CLOCK_RATE: u32 = 90_000;
pub const STREAM_PRESENTATION_CLOCK_MAX_CORRECTION: Duration = Duration::from_millis(250);
// Allow normal packet jitter and short Opus duration changes, but do not carry
// multi-second source clock resets into mpv's local playback timeline.
pub const STREAM_AUDIO_CLOCK_DRIFT_TOLERANCE_TICKS: u32 = DISCORD_OPUS_TIMESTAMP_INCREMENT * 6;
pub const STREAM_AUDIO_REORDER_DELAY: Duration = Duration::from_millis(100);
pub const STREAM_AUDIO_REORDER_INTERVAL: Duration = Duration::from_millis(20);
pub const STREAM_AUDIO_MAX_PENDING_PACKETS: usize = 64;
pub const STREAM_KEYFRAME_REQUEST_INTERVAL: Duration = Duration::from_secs(1);
pub const STREAM_VIDEO_NACK_INTERVAL: Duration = Duration::from_millis(100);
pub const STREAM_VIDEO_GAP_TIMEOUT: Duration = Duration::from_millis(500);
pub const STREAM_VIDEO_MAX_PENDING_PACKETS: usize = 2_048;
pub const STREAM_VIDEO_MAX_PENDING_BYTES: usize = 4 * 1024 * 1024;
pub const STREAM_VIDEO_MAX_NACK_SEQUENCES: usize = 64;
pub const STREAM_RTCP_RECEIVER_REPORT_INTERVAL: Duration = Duration::from_secs(5);
pub const STREAM_TRANSPORT_FEEDBACK_INTERVAL: Duration = Duration::from_millis(50);
pub const STREAM_TRANSPORT_FEEDBACK_MAX_STATUSES: usize = 512;
pub const LOCAL_RTCP_REPORT_INTERVAL: Duration = Duration::from_secs(1);
pub const STREAM_CONNECTION_STABLE_INTERVAL: Duration = Duration::from_secs(10);
pub const STREAM_WATCH_RECONNECT_BASE_DELAY: Duration = Duration::from_millis(250);
pub const STREAM_WATCH_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(2);
pub const RTCP_SENDER_REPORT: u8 = 200;
pub const RTCP_TRANSPORT_LAYER_FEEDBACK: u8 = 205;
pub const RTCP_GENERIC_NACK_FORMAT: u8 = 1;
pub const RTCP_TRANSPORT_WIDE_FEEDBACK_FORMAT: u8 = 15;
pub const RTCP_RECEIVER_REPORT: u8 = 201;
pub const RTCP_SOURCE_DESCRIPTION: u8 = 202;
pub const RTCP_SDES_CNAME: u8 = 1;
pub const RTCP_PAYLOAD_SPECIFIC_FEEDBACK: u8 = 206;
pub const RTCP_PLI_FORMAT: u8 = 1;
pub const RTCP_PLI_LENGTH_WORDS_MINUS_ONE: u16 = 2;
pub const RTP_ONE_BYTE_EXTENSION_PROFILE: u16 = 0xbede;
pub const DISCORD_TRANSPORT_SEQUENCE_EXTENSION_ID: u8 = 5;
pub const TRANSPORT_FEEDBACK_REFERENCE_TIME_MICROS: i64 = 64_000;
pub const TRANSPORT_FEEDBACK_DELTA_MICROS: i64 = 250;
pub const TRANSPORT_PACKET_NOT_RECEIVED: u8 = 0;
pub const TRANSPORT_PACKET_RECEIVED_SMALL_DELTA: u8 = 1;
pub const TRANSPORT_PACKET_RECEIVED_LARGE_DELTA: u8 = 2;

#[derive(Clone, Eq, PartialEq)]
pub struct StreamGatewaySession {
    pub connection_id: u64,
    pub request: StreamWatchRequest,
    pub current_user_id: Id<UserMarker>,
    pub session_id: String,
    pub rtc_server_id: String,
    pub rtc_channel_id: Id<ChannelMarker>,
    pub endpoint: String,
    pub token: String,
    pub reconnect_delay: Duration,
}

impl std::fmt::Debug for StreamGatewaySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamGatewaySession")
            .field("connection_id", &self.connection_id)
            .field("request", &self.request)
            .field("current_user_id", &self.current_user_id)
            .field("session_id", &self.session_id)
            .field("rtc_server_id", &self.rtc_server_id)
            .field("rtc_channel_id", &self.rtc_channel_id)
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .field("reconnect_delay", &self.reconnect_delay)
            .finish()
    }
}

pub struct StreamPlayerLogTasks {
    pub tasks: Vec<JoinHandle<()>>,
}

impl StreamPlayerLogTasks {
    pub fn new(tasks: [JoinHandle<()>; 2]) -> Self {
        Self {
            tasks: Vec::from(tasks),
        }
    }

    pub async fn finish(mut self) {
        for task in self.tasks.drain(..) {
            let _ = task.await;
        }
    }
}

impl Drop for StreamPlayerLogTasks {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedStreamVoiceState {
    pub scope: VoiceScope,
    pub channel_id: Id<ChannelMarker>,
    pub session_id: String,
}

#[derive(Clone)]
pub struct StreamPlayerReadySignal {
    pub player_ready: Arc<AtomicBool>,
    pub ready_tx: mpsc::UnboundedSender<u64>,
    pub media_generation: u64,
    pub display_name: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct StreamConnectionFailure {
    pub message: String,
    pub outcome: VoiceConnectionEnd,
}

impl StreamConnectionFailure {
    pub fn reconnect(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: VoiceConnectionEnd::Reconnect,
        }
    }

    pub fn stop(message: impl Into<String>) -> Self {
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
pub struct StreamRuntimeState {
    pub current_user_id: Option<Id<UserMarker>>,
    pub current_voice: Option<ObservedStreamVoiceState>,
    pub requested: Option<StreamWatchRequest>,
    pub create: Option<StreamCreateInfo>,
    pub server: Option<StreamServerInfo>,
    pub active: Option<StreamGatewaySession>,
    pub reconnect_attempts: u8,
    pub next_connection_id: u64,
}

#[derive(Default)]
pub struct StreamRuntimeUpdate {
    pub close_stream_key: Option<String>,
    pub send_delete: bool,
    pub connect: Option<StreamGatewaySession>,
    pub playback_ended: Option<StreamPlaybackEnded>,
    pub error: Option<String>,
}

pub struct StreamPlaybackEnded {
    pub request: StreamWatchRequest,
    pub reconnecting: bool,
}

impl StreamRuntimeState {
    pub fn apply(&mut self, event: &VoiceRuntimeEvent) -> StreamRuntimeUpdate {
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
                    update.playback_ended =
                        self.requested.take().map(|request| StreamPlaybackEnded {
                            request,
                            reconnecting: false,
                        });
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
                    if self.active.as_ref().is_some_and(|active| {
                        !server.matches_connection(&active.endpoint, &active.token)
                    }) {
                        update.playback_ended =
                            self.requested
                                .as_ref()
                                .cloned()
                                .map(|request| StreamPlaybackEnded {
                                    request,
                                    reconnecting: true,
                                });
                        update.close_stream_key =
                            self.active.take().map(|active| active.request.stream_key);
                    }
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
                        update.playback_ended =
                            self.requested.take().map(|request| StreamPlaybackEnded {
                                request,
                                reconnecting: false,
                            });
                        self.create = None;
                        self.server = None;
                        self.reconnect_attempts = 0;
                        update.close_stream_key = Some(stream_key.clone());
                        update.send_delete = true;
                    } else {
                        self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
                        update.playback_ended =
                            self.requested
                                .as_ref()
                                .cloned()
                                .map(|request| StreamPlaybackEnded {
                                    request,
                                    reconnecting: true,
                                });
                    }
                }
            }
            VoiceRuntimeEvent::Shutdown => {
                update.playback_ended = self.requested.take().map(|request| StreamPlaybackEnded {
                    request,
                    reconnecting: false,
                });
                update.close_stream_key =
                    self.active.take().map(|active| active.request.stream_key);
                update.send_delete = update.close_stream_key.is_some();
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

    pub fn record_voice_state(&mut self, state: &VoiceStateInfo, update: &mut StreamRuntimeUpdate) {
        if self.current_user_id != Some(state.user_id) {
            return;
        }
        let Some(channel_id) = state.channel_id else {
            self.current_voice = None;
            update.playback_ended = self.requested.take().map(|request| StreamPlaybackEnded {
                request,
                reconnecting: false,
            });
            update.close_stream_key = self.active.take().map(|active| active.request.stream_key);
            update.send_delete = update.close_stream_key.is_some();
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
            update.playback_ended = self.requested.take().map(|request| StreamPlaybackEnded {
                request,
                reconnecting: false,
            });
            update.close_stream_key = self.active.take().map(|active| active.request.stream_key);
            update.send_delete = update.close_stream_key.is_some();
            self.create = None;
            self.server = None;
        }
    }

    pub fn clear_matching(
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
            update.playback_ended = self.requested.take().map(|request| StreamPlaybackEnded {
                request,
                reconnecting: false,
            });
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

    pub fn connect_if_ready(&mut self) -> Option<StreamGatewaySession> {
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
            reconnect_delay: stream_watch_reconnect_delay(self.reconnect_attempts),
        };
        self.active = Some(session.clone());
        Some(session)
    }
}

pub fn stream_watch_reconnect_delay(reconnect_attempts: u8) -> Duration {
    if reconnect_attempts == 0 {
        return Duration::ZERO;
    }
    let multiplier = 1u32 << u32::from(reconnect_attempts.saturating_sub(1).min(3));
    let base_delay = STREAM_WATCH_RECONNECT_BASE_DELAY
        .saturating_mul(multiplier)
        .min(STREAM_WATCH_RECONNECT_MAX_DELAY);
    let jitter_limit_millis =
        u64::try_from((base_delay / 4).as_millis()).expect("bounded retry jitter fits u64");
    let jitter = Duration::from_millis(random::<u64>() % (jitter_limit_millis + 1));
    base_delay
        .saturating_add(jitter)
        .min(STREAM_WATCH_RECONNECT_MAX_DELAY)
}

pub async fn run_stream_gateway_session(
    session: StreamGatewaySession,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: VoiceStatusPublisher,
) {
    if !session.reconnect_delay.is_zero() {
        logging::debug(
            "stream",
            format!(
                "waiting {:?} before reconnecting stream watch",
                session.reconnect_delay
            ),
        );
        sleep(session.reconnect_delay).await;
    }
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
    let _ = events_tx.send(VoiceRuntimeEvent::StreamConnectionEnded {
        connection_id: session.connection_id,
        stream_key: session.request.stream_key.clone(),
        outcome,
    });
}
