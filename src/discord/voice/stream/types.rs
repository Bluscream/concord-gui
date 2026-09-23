use std::collections::BTreeMap;
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

#[cfg(test)]
impl StreamGatewaySession {
    pub fn for_test(stream_key: &str) -> Self {
        Self {
            connection_id: 1,
            request: StreamWatchRequest {
                stream_key: stream_key.to_owned(),
                scope: VoiceScope::Guild(Id::new(1)),
                channel_id: Id::new(2),
                owner_id: Id::new(3),
                display_name: "Streamer".to_owned(),
            },
            current_user_id: Id::new(4),
            session_id: "parent-session".to_owned(),
            rtc_server_id: "5".to_owned(),
            rtc_channel_id: Id::new(6),
            endpoint: "stream.example.com".to_owned(),
            token: "stream-token".to_owned(),
            reconnect_delay: Duration::ZERO,
        }
    }
}

#[derive(Default)]
pub struct StreamRuntimeState {
    current_user_id: Option<Id<UserMarker>>,
    current_voice: Option<ObservedStreamVoiceState>,
    pub(crate) watches: BTreeMap<String, StreamWatchState>,
    next_connection_id: u64,
}

pub(crate) struct StreamWatchState {
    pub(crate) request: StreamWatchRequest,
    pub(crate) create: Option<StreamCreateInfo>,
    pub(crate) server: Option<StreamServerInfo>,
    pub(crate) active: Option<StreamGatewaySession>,
    pub(crate) reconnect_attempts: u8,
}

impl StreamWatchState {
    fn new(request: StreamWatchRequest) -> Self {
        Self {
            request,
            create: None,
            server: None,
            active: None,
            reconnect_attempts: 0,
        }
    }
}

#[derive(Default)]
pub struct StreamRuntimeUpdate {
    pub close: Vec<StreamWatchClose>,
    pub connect: Vec<StreamGatewaySession>,
    pub playback_ended: Vec<StreamPlaybackEnded>,
    pub errors: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct StreamWatchClose {
    pub stream_key: String,
    pub send_delete: bool,
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
                self.watches
                    .entry(request.stream_key.clone())
                    .and_modify(|watch| watch.request = request.clone())
                    .or_insert_with(|| StreamWatchState::new(request.clone()));
            }
            VoiceRuntimeEvent::WatchStreamCancelled { stream_key } => {
                self.clear_matching(stream_key, &mut update, false);
            }
            VoiceRuntimeEvent::StreamCreate(stream) => {
                if let Some(watch) = self.watches.get_mut(&stream.stream_key) {
                    watch.create = Some(stream.clone());
                }
            }
            VoiceRuntimeEvent::StreamServer(server) => {
                if let Some(watch) = self.watches.get_mut(&server.stream_key) {
                    if watch.active.as_ref().is_some_and(|active| {
                        !server.matches_connection(&active.endpoint, &active.token)
                    }) {
                        update.playback_ended.push(StreamPlaybackEnded {
                            request: watch.request.clone(),
                            reconnecting: true,
                        });
                        watch.active = None;
                        update.close.push(StreamWatchClose {
                            stream_key: server.stream_key.clone(),
                            send_delete: false,
                        });
                    }
                    watch.server = Some(server.clone());
                }
            }
            VoiceRuntimeEvent::StreamDelete(stream) => {
                if let Some(request) = self
                    .watches
                    .get(&stream.stream_key)
                    .map(|watch| &watch.request)
                    && (!stream.reason.is_empty() || stream.unavailable)
                {
                    let reason = if stream.reason.is_empty() {
                        "stream unavailable"
                    } else {
                        stream.reason.as_str()
                    };
                    update.errors.push(format!(
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
                if let Some(watch) = self.watches.get_mut(stream_key)
                    && watch
                        .active
                        .as_ref()
                        .is_some_and(|active| active.connection_id == *connection_id)
                {
                    watch.reconnect_attempts = 0;
                }
            }
            VoiceRuntimeEvent::StreamConnectionEnded {
                connection_id,
                stream_key,
                outcome,
            } => {
                let terminal = if let Some(watch) = self.watches.get_mut(stream_key)
                    && watch
                        .active
                        .as_ref()
                        .is_some_and(|active| active.connection_id == *connection_id)
                {
                    watch.active = None;
                    if *outcome == VoiceConnectionEnd::Stop
                        || watch.reconnect_attempts >= MAX_VOICE_RECONNECT_ATTEMPTS
                    {
                        true
                    } else {
                        watch.reconnect_attempts = watch.reconnect_attempts.saturating_add(1);
                        update.playback_ended.push(StreamPlaybackEnded {
                            request: watch.request.clone(),
                            reconnecting: true,
                        });
                        false
                    }
                } else {
                    false
                };
                if terminal {
                    let watch = self
                        .watches
                        .remove(stream_key)
                        .expect("matched stream watch remains present");
                    update.playback_ended.push(StreamPlaybackEnded {
                        request: watch.request,
                        reconnecting: false,
                    });
                    update.close.push(StreamWatchClose {
                        stream_key: stream_key.clone(),
                        send_delete: true,
                    });
                }
            }
            VoiceRuntimeEvent::Shutdown => {
                self.clear_all(&mut update, true);
            }
            _ => {}
        }

        self.connect_ready_watches(&mut update);
        update
    }

    fn record_voice_state(&mut self, state: &VoiceStateInfo, update: &mut StreamRuntimeUpdate) {
        if self.current_user_id != Some(state.user_id) {
            return;
        }
        let Some(channel_id) = state.channel_id else {
            self.current_voice = None;
            self.clear_all(update, true);
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
        let stale_keys = self
            .watches
            .iter()
            .filter(|(_, watch)| {
                watch.request.scope != scope || watch.request.channel_id != channel_id
            })
            .map(|(stream_key, _)| stream_key.clone())
            .collect::<Vec<_>>();
        for stream_key in stale_keys {
            self.clear_matching(&stream_key, update, true);
        }
    }

    fn clear_all(&mut self, update: &mut StreamRuntimeUpdate, send_delete: bool) {
        let stream_keys = self.watches.keys().cloned().collect::<Vec<_>>();
        for stream_key in stream_keys {
            self.clear_matching(&stream_key, update, send_delete);
        }
    }

    fn clear_matching(
        &mut self,
        stream_key: &str,
        update: &mut StreamRuntimeUpdate,
        send_delete: bool,
    ) {
        let Some(watch) = self.watches.remove(stream_key) else {
            return;
        };
        update.playback_ended.push(StreamPlaybackEnded {
            request: watch.request,
            reconnecting: false,
        });
        if watch.active.is_some() {
            update.close.push(StreamWatchClose {
                stream_key: stream_key.to_owned(),
                send_delete,
            });
        }
    }

    fn connect_ready_watches(&mut self, update: &mut StreamRuntimeUpdate) {
        let stream_keys = self.watches.keys().cloned().collect::<Vec<_>>();
        for stream_key in stream_keys {
            if let Some(session) = self.connect_if_ready(&stream_key) {
                update.connect.push(session);
            }
        }
    }

    fn connect_if_ready(&mut self, stream_key: &str) -> Option<StreamGatewaySession> {
        let current_user_id = self.current_user_id?;
        let current_voice = self.current_voice.as_ref()?;
        let watch = self.watches.get(stream_key)?;
        if watch.active.is_some()
            || watch.request.scope != current_voice.scope
            || watch.request.channel_id != current_voice.channel_id
        {
            return None;
        }
        let create = watch.create.as_ref()?;
        let server = watch.server.as_ref()?;
        let endpoint = server.endpoint.as_ref()?.trim_end_matches('/').to_owned();
        if endpoint.is_empty() || server.token.is_empty() {
            return None;
        }
        let request = watch.request.clone();
        let rtc_server_id = create.rtc_server_id.clone();
        let rtc_channel_id = create.rtc_channel_id;
        let token = server.token.clone();
        let reconnect_delay = stream_watch_reconnect_delay(watch.reconnect_attempts);

        self.next_connection_id = self.next_connection_id.wrapping_add(1).max(1);
        let session = StreamGatewaySession {
            connection_id: self.next_connection_id,
            request,
            current_user_id,
            session_id: current_voice.session_id.clone(),
            rtc_server_id,
            rtc_channel_id,
            endpoint,
            token,
            reconnect_delay,
        };
        self.watches
            .get_mut(stream_key)
            .expect("stream watch remains present while connecting")
            .active = Some(session.clone());
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
