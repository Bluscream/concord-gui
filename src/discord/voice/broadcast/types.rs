use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use rand::random;
use tokio::{
    sync::{mpsc, oneshot},
    time::sleep,
};

use super::super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS;
use super::super::{
    StreamBroadcastRequest, StreamCreateInfo, StreamServerInfo, VoiceConnectionEnd,
    VoiceRuntimeEvent, VoiceScope, VoiceStatusPublisher, capture,
    preview::{StreamPreviewUploadTask, StreamPreviewUploader},
};
use super::*;

use crate::{
    discord::{
        StreamCaptureTarget,
        ids::{
            Id,
            marker::{ChannelMarker, UserMarker},
        },
        voice::VoiceStateInfo,
    },
    logging,
};

pub const STREAM_RID: &str = "100";
pub const STREAM_RTP_MAX_PAYLOAD_BYTES: usize = 1_100;
pub const STREAM_RTP_EXTENSION_BODY_BYTES: usize = 16;
pub const STREAM_RTX_ORIGINAL_SEQUENCE_BYTES: usize = 2;
pub const RTP_EXTENSION_PROFILE_ONE_BYTE: u16 = 0xbede;
pub const RTP_EXTENSION_TRANSPORT_SEQUENCE: u8 = 5;
pub const RTP_EXTENSION_PLAYOUT_DELAY: u8 = 6;
pub const RTP_EXTENSION_VIDEO_CONTENT_TYPE: u8 = 7;
pub const RTP_EXTENSION_RID: u8 = 11;
pub const RTP_EXTENSION_REPAIRED_RID: u8 = 12;
pub const VIDEO_CONTENT_TYPE_SCREEN: u8 = 1;
pub const RTCP_SENDER_REPORT_INTERVAL: Duration = Duration::from_secs(5);
pub const BROADCAST_SEND_STATS_INTERVAL: Duration = Duration::from_secs(5);
pub const STREAM_RTP_SMALL_FRAME_PACING_BUDGET: Duration = Duration::from_millis(25);
pub const STREAM_RTP_MAX_PACKET_SPACING: Duration = Duration::from_millis(2);
pub const STREAM_RTP_BURST_CREDIT: Duration = Duration::from_millis(150);
pub const STREAM_RTP_MAX_BURST_BITRATE: u64 = 16_000_000;
pub const STREAM_RTP_HISTORY_CAPACITY: usize = 2_048;
pub const STREAM_RTX_MAX_RETRANSMISSIONS_PER_FEEDBACK: usize = 128;
pub const STREAM_UDP_RECEIVE_PACKET_BYTES: usize = 2_048;
pub const SOUNDSHARE_SPEAKING_FLAG: u8 = 2;
pub const STREAM_BROADCAST_CONNECTION_STABLE_INTERVAL: Duration = Duration::from_secs(10);
pub const STREAM_BROADCAST_RECONNECT_BASE_DELAY: Duration = Duration::from_millis(250);
pub const STREAM_BROADCAST_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone, Eq, PartialEq)]
pub struct StreamBroadcastGatewaySession {
    pub connection_id: u64,
    pub request: StreamBroadcastRequest,
    pub(super) current_user_id: Id<UserMarker>,
    pub(super) session_id: String,
    pub(super) rtc_server_id: String,
    pub(super) rtc_channel_id: Id<ChannelMarker>,
    pub(super) endpoint: String,
    pub(super) token: String,
    pub reconnect_delay: Duration,
}

impl std::fmt::Debug for StreamBroadcastGatewaySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamBroadcastGatewaySession")
            .field("connection_id", &self.connection_id)
            .field("request", &self.request)
            .field("current_user_id", &self.current_user_id)
            .field("session_id", &self.session_id)
            .field("rtc_server_id", &self.rtc_server_id)
            .field("rtc_channel_id", &self.rtc_channel_id)
            .field("endpoint", &self.endpoint)
            .field("reconnect_delay", &self.reconnect_delay)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct BroadcastConnectionFailure {
    pub message: String,
    pub outcome: VoiceConnectionEnd,
}

impl BroadcastConnectionFailure {
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

impl From<String> for BroadcastConnectionFailure {
    fn from(message: String) -> Self {
        Self::reconnect(message)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedBroadcastVoiceState {
    pub scope: VoiceScope,
    pub channel_id: Id<ChannelMarker>,
    pub session_id: String,
}

#[derive(Default)]
pub struct StreamBroadcastRuntimeState {
    pub current_user_id: Option<Id<UserMarker>>,
    pub current_voice: Option<ObservedBroadcastVoiceState>,
    pub requested: Option<StreamBroadcastRequest>,
    pub create: Option<StreamCreateInfo>,
    pub server: Option<StreamServerInfo>,
    pub active: Option<StreamBroadcastGatewaySession>,
    pub reconnect_attempts: u8,
    pub next_connection_id: u64,
}

#[derive(Default)]
pub struct StreamBroadcastRuntimeUpdate {
    pub close_stream_key: Option<String>,
    pub send_delete: bool,
    pub retain_capture: bool,
    pub connect: Option<StreamBroadcastGatewaySession>,
    pub broadcast_ended: Option<StreamBroadcastRequest>,
    pub error: Option<String>,
}

pub struct PreparedBroadcastCapture {
    pub request_id: u64,
    pub capture: capture::PreparedStreamCapture,
    pub preview_task: Option<StreamPreviewUploadTask>,
}

#[derive(Default)]
pub struct StreamBroadcastCaptureRegistryState {
    pub active_requests: HashMap<String, u64>,
    pub captures: HashMap<String, PreparedBroadcastCapture>,
}

#[derive(Clone, Default)]
pub struct StreamBroadcastCaptureRegistry {
    pub state: Arc<StdMutex<StreamBroadcastCaptureRegistryState>>,
}

impl StreamBroadcastCaptureRegistry {
    pub fn activate(&self, stream_key: String, request_id: u64) {
        let mut state = self
            .state
            .lock()
            .expect("stream capture registry lock is not poisoned");
        let previous_request_id = state.active_requests.insert(stream_key.clone(), request_id);
        logging::debug(
            "stream",
            format!(
                "activated stream capture request: stream_key={stream_key} request_id={request_id} previous_request_id={previous_request_id:?}"
            ),
        );
    }

    pub fn prepare(
        &self,
        stream_key: String,
        request_id: u64,
        target: StreamCaptureTarget,
        cancellation: capture::StreamCaptureCancellation,
    ) -> Result<bool, String> {
        logging::debug(
            "stream",
            format!(
                "starting stream capture preparation: stream_key={stream_key} request_id={request_id} target_kind={:?}",
                target.kind,
            ),
        );
        let capture = capture::prepare_stream_capture(target, cancellation)?;
        let prepared = PreparedBroadcastCapture {
            request_id,
            capture,
            preview_task: None,
        };
        let mut state = self
            .state
            .lock()
            .expect("stream capture registry lock is not poisoned");
        let active_request_id = state.active_requests.get(&stream_key).copied();
        if active_request_id != Some(request_id) {
            logging::debug(
                "stream",
                format!(
                    "discarding stale prepared stream capture: stream_key={stream_key} request_id={request_id} active_request_id={active_request_id:?}"
                ),
            );
            return Ok(false);
        }
        state.captures.insert(stream_key.clone(), prepared);
        logging::debug(
            "stream",
            format!(
                "stored prepared stream capture: stream_key={stream_key} request_id={request_id}"
            ),
        );
        Ok(true)
    }

    pub fn take(&self, stream_key: &str) -> Option<PreparedBroadcastCapture> {
        let mut state = self
            .state
            .lock()
            .expect("stream capture registry lock is not poisoned");
        let Some(active_request_id) = state.active_requests.get(stream_key).copied() else {
            logging::debug(
                "stream",
                format!("prepared stream capture has no active request: stream_key={stream_key}"),
            );
            return None;
        };
        let Some(prepared) = state.captures.remove(stream_key) else {
            logging::debug(
                "stream",
                format!(
                    "active stream capture request has no prepared capture: stream_key={stream_key} request_id={active_request_id}"
                ),
            );
            return None;
        };
        if prepared.request_id != active_request_id {
            logging::debug(
                "stream",
                format!(
                    "prepared stream capture request mismatch: stream_key={stream_key} prepared_request_id={} active_request_id={active_request_id}",
                    prepared.request_id,
                ),
            );
            return None;
        }
        logging::debug(
            "stream",
            format!(
                "taking prepared stream capture: stream_key={stream_key} request_id={active_request_id}"
            ),
        );
        Some(prepared)
    }

    pub fn restore(
        &self,
        stream_key: String,
        prepared: PreparedBroadcastCapture,
    ) -> Result<(), PreparedBroadcastCapture> {
        let mut state = self
            .state
            .lock()
            .expect("stream capture registry lock is not poisoned");
        let active_request_id = state.active_requests.get(&stream_key).copied();
        if active_request_id != Some(prepared.request_id) {
            logging::debug(
                "stream",
                format!(
                    "could not restore stale stream capture: stream_key={stream_key} prepared_request_id={} active_request_id={active_request_id:?}",
                    prepared.request_id,
                ),
            );
            return Err(prepared);
        }
        state.captures.insert(stream_key, prepared);
        Ok(())
    }

    pub fn discard(&self, stream_key: &str) {
        let mut state = self
            .state
            .lock()
            .expect("stream capture registry lock is not poisoned");
        let request_id = state.active_requests.remove(stream_key);
        let had_capture = state.captures.remove(stream_key).is_some();
        logging::debug(
            "stream",
            format!(
                "discarded stream capture registry entry: stream_key={stream_key} request_id={request_id:?} had_capture={had_capture}"
            ),
        );
    }

    #[cfg(test)]
    pub fn is_active(&self, stream_key: &str, request_id: u64) -> bool {
        self.state
            .lock()
            .expect("stream capture registry lock is not poisoned")
            .active_requests
            .get(stream_key)
            .is_some_and(|active| *active == request_id)
    }
}

impl StreamBroadcastRuntimeState {
    pub fn apply(&mut self, event: &VoiceRuntimeEvent) -> StreamBroadcastRuntimeUpdate {
        let mut update = StreamBroadcastRuntimeUpdate::default();
        match event {
            VoiceRuntimeEvent::CurrentUserReady(user_id) => self.current_user_id = *user_id,
            VoiceRuntimeEvent::VoiceState(state) => self.record_voice_state(state, &mut update),
            VoiceRuntimeEvent::BroadcastStreamRequested(request) => {
                if self
                    .requested
                    .as_ref()
                    .is_none_or(|current| current.stream_key != request.stream_key)
                {
                    update.broadcast_ended = self.requested.take();
                    update.close_stream_key =
                        self.active.take().map(|active| active.request.stream_key);
                    update.send_delete = update.close_stream_key.is_some();
                }
                self.reconnect_attempts = 0;
                self.requested = Some(request.clone());
                self.create = None;
                self.server = None;
            }
            VoiceRuntimeEvent::BroadcastStreamCaptureReady { .. } => {}
            VoiceRuntimeEvent::BroadcastStreamCaptureFailed {
                stream_key, error, ..
            } => {
                if self
                    .requested
                    .as_ref()
                    .is_some_and(|request| request.stream_key == *stream_key)
                {
                    update.error = Some(format!("Could not broadcast stream: {error}"));
                    self.clear_matching(stream_key, &mut update, false);
                }
            }
            #[cfg(test)]
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
                        update.retain_capture = true;
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
            VoiceRuntimeEvent::BroadcastStreamConnectionEstablished { .. } => {}
            VoiceRuntimeEvent::BroadcastStreamConnectionStable {
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
                        update.broadcast_ended = self.requested.take();
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
                update.broadcast_ended = self.requested.take();
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

    pub fn requested_destination(
        &self,
        stream_key: &str,
    ) -> Option<(VoiceScope, Id<ChannelMarker>)> {
        self.requested
            .as_ref()
            .filter(|request| request.stream_key == stream_key)
            .map(|request| (request.scope, request.channel_id))
    }

    pub fn record_voice_state(
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
            update.broadcast_ended = self.requested.take();
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
            update.broadcast_ended = self.requested.take();
            self.create = None;
            self.server = None;
            self.reconnect_attempts = 0;
        }
    }

    pub fn clear_matching(
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
            update.broadcast_ended = self.requested.take();
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

    pub fn connect_if_ready(&mut self) -> Option<StreamBroadcastGatewaySession> {
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
            reconnect_delay: broadcast_reconnect_delay(self.reconnect_attempts),
        };
        self.active = Some(session.clone());
        Some(session)
    }
}

pub fn broadcast_reconnect_delay(reconnect_attempts: u8) -> Duration {
    if reconnect_attempts <= 1 {
        return Duration::ZERO;
    }
    let multiplier = 1u32 << u32::from(reconnect_attempts.saturating_sub(2).min(3));
    let base_delay = STREAM_BROADCAST_RECONNECT_BASE_DELAY
        .saturating_mul(multiplier)
        .min(STREAM_BROADCAST_RECONNECT_MAX_DELAY);
    let jitter_limit_millis =
        u64::try_from((base_delay / 4).as_millis()).expect("bounded retry jitter fits u64");
    let jitter = Duration::from_millis(random::<u64>() % (jitter_limit_millis + 1));
    base_delay
        .saturating_add(jitter)
        .min(STREAM_BROADCAST_RECONNECT_MAX_DELAY)
}

pub async fn run_stream_broadcast_session(
    session: StreamBroadcastGatewaySession,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: VoiceStatusPublisher,
    stream_preview_uploader: StreamPreviewUploader,
    broadcast_captures: StreamBroadcastCaptureRegistry,
    mut stop_rx: oneshot::Receiver<()>,
) {
    if !session.reconnect_delay.is_zero() {
        logging::debug(
            "stream",
            format!(
                "waiting {:?} before reconnecting stream broadcast",
                session.reconnect_delay
            ),
        );
        let stopped = tokio::select! {
            _ = sleep(session.reconnect_delay) => false,
            _ = &mut stop_rx => true,
        };
        if stopped {
            let _ = events_tx.send(VoiceRuntimeEvent::BroadcastStreamConnectionEnded {
                connection_id: session.connection_id,
                stream_key: session.request.stream_key.clone(),
                outcome: VoiceConnectionEnd::Stop,
            });
            return;
        }
    }
    let outcome = match connect_stream_broadcast(
        &session,
        &events_tx,
        &status_publisher,
        stream_preview_uploader,
        broadcast_captures,
        stop_rx,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            logging::error("stream", &error.message);
            status_publisher
                .publish_error(format!("Could not broadcast stream: {}", error.message))
                .await;
            error.outcome
        }
    };
    let _ = events_tx.send(VoiceRuntimeEvent::BroadcastStreamConnectionEnded {
        connection_id: session.connection_id,
        stream_key: session.request.stream_key.clone(),
        outcome,
    });
}
