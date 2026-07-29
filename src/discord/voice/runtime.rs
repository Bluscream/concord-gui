use super::broadcast::{
    StreamBroadcastGatewaySession, StreamBroadcastRuntimeState, run_stream_broadcast_session,
};
use super::stream::{StreamGatewaySession, StreamRuntimeState, run_stream_gateway_session};
use super::*;
use tokio::sync::oneshot;

pub(super) const MAX_VOICE_RECONNECT_ATTEMPTS: u8 = 3;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum VoiceRuntimeAction {
    Connect(VoiceGatewaySession),
    Close,
}

struct VoiceRuntimeApplyResult {
    action: Option<VoiceRuntimeAction>,
    participant_playback_changed: bool,
}

#[derive(Default)]
pub(super) struct VoiceRuntimeState {
    current_user_id: Option<Id<UserMarker>>,
    requested: Option<CurrentVoiceConnectionState>,
    current_voice: Option<ObservedSelfVoiceState>,
    server: Option<VoiceServerInfo>,
    active: Option<VoiceGatewaySession>,
    blocked: Option<VoiceGatewaySession>,
    reconnect_target: Option<VoiceGatewaySession>,
    reconnect_attempts: u8,
    push_to_talk: bool,
    push_to_talk_pressed: bool,
    participant_playback_settings: HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>,
    next_connection_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedSelfVoiceState {
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    session_id: String,
}

impl VoiceRuntimeState {
    #[cfg(test)]
    pub(super) fn apply(&mut self, event: VoiceRuntimeEvent) -> Option<VoiceRuntimeAction> {
        self.apply_with_changes(event).action
    }

    fn apply_with_changes(&mut self, event: VoiceRuntimeEvent) -> VoiceRuntimeApplyResult {
        let mut participant_playback_changed = false;
        match event {
            VoiceRuntimeEvent::Requested(requested) => {
                let target_changed = match (self.requested, requested) {
                    (Some(current), Some(next)) => {
                        current.scope != next.scope || current.channel_id != next.channel_id
                    }
                    (None, None) => false,
                    _ => true,
                };
                if target_changed {
                    self.push_to_talk_pressed = false;
                }
                if requested.is_none() || self.current_voice.is_none() {
                    self.blocked = None;
                }
                if let Some(next) = requested
                    && self.requested.is_some_and(|current| {
                        current.scope != next.scope || current.channel_id != next.channel_id
                    })
                {
                    self.server = None;
                }
                self.requested = requested;
                if self.requested.is_none() {
                    self.current_voice = None;
                    self.server = None;
                    return VoiceRuntimeApplyResult {
                        action: self.close_active(),
                        participant_playback_changed,
                    };
                }
            }
            VoiceRuntimeEvent::ManualRetry(requested) => {
                let target_changed = self.requested.is_none_or(|current| {
                    current.scope != requested.scope || current.channel_id != requested.channel_id
                });
                if target_changed {
                    self.server = None;
                    self.push_to_talk_pressed = false;
                }
                self.requested = Some(requested);
                self.blocked = None;
                self.reconnect_target = None;
                self.reconnect_attempts = 0;
            }
            #[cfg(feature = "voice-playback")]
            VoiceRuntimeEvent::PushToTalkEnabledChanged(enabled) => {
                if self.push_to_talk != enabled {
                    self.push_to_talk = enabled;
                    self.push_to_talk_pressed = false;
                }
            }
            #[cfg(feature = "voice-playback")]
            VoiceRuntimeEvent::PushToTalkPressed(pressed) => {
                self.push_to_talk_pressed = pressed;
            }
            VoiceRuntimeEvent::ReplaceParticipantPlaybackSettings(settings) => {
                let settings = settings
                    .into_iter()
                    .filter(|(_, settings)| {
                        *settings != VoiceParticipantPlaybackSettings::default()
                    })
                    .collect();
                participant_playback_changed = self.participant_playback_settings != settings;
                self.participant_playback_settings = settings;
            }
            VoiceRuntimeEvent::UpdateParticipantPlaybackSettings { user_id, settings } => {
                if settings == VoiceParticipantPlaybackSettings::default() {
                    participant_playback_changed = self
                        .participant_playback_settings
                        .remove(&user_id)
                        .is_some();
                } else {
                    participant_playback_changed =
                        self.participant_playback_settings.insert(user_id, settings)
                            != Some(settings);
                }
            }
            VoiceRuntimeEvent::CurrentUserReady(user_id) => {
                self.current_user_id = user_id;
            }
            VoiceRuntimeEvent::VoiceState(state) => {
                if let Some(action) = self.record_voice_state(state) {
                    return VoiceRuntimeApplyResult {
                        action: Some(action),
                        participant_playback_changed,
                    };
                }
            }
            VoiceRuntimeEvent::VoiceServer(server) => {
                if server.endpoint.is_none() {
                    self.server = None;
                    return VoiceRuntimeApplyResult {
                        action: self.close_active(),
                        participant_playback_changed,
                    };
                }
                self.server = Some(server);
            }
            VoiceRuntimeEvent::WatchStreamRequested(_)
            | VoiceRuntimeEvent::WatchStreamCancelled { .. }
            | VoiceRuntimeEvent::StreamCreate(_)
            | VoiceRuntimeEvent::StreamServer(_)
            | VoiceRuntimeEvent::StreamDelete(_)
            | VoiceRuntimeEvent::StreamConnectionEstablished { .. }
            | VoiceRuntimeEvent::StreamConnectionEnded { .. }
            | VoiceRuntimeEvent::BroadcastStreamRequested(_)
            | VoiceRuntimeEvent::BroadcastStreamCancelled { .. }
            | VoiceRuntimeEvent::BroadcastStreamStopRequested { .. }
            | VoiceRuntimeEvent::BroadcastStreamConnectionEstablished { .. }
            | VoiceRuntimeEvent::BroadcastStreamConnectionEnded { .. } => {}
            VoiceRuntimeEvent::ConnectionEstablished { connection_id } => {
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.connection_id == connection_id)
                {
                    self.reconnect_attempts = 0;
                }
                return VoiceRuntimeApplyResult {
                    action: None,
                    participant_playback_changed,
                };
            }
            VoiceRuntimeEvent::ConnectionEnded {
                connection_id,
                scope,
                channel_id,
                session_id,
                endpoint,
                outcome,
            } => {
                if let Some(active) = self
                    .active
                    .as_ref()
                    .filter(|active| {
                        active.matches_connection_end(
                            connection_id,
                            scope,
                            channel_id,
                            &session_id,
                            &endpoint,
                        )
                    })
                    .cloned()
                {
                    self.active = None;
                    if outcome == VoiceConnectionEnd::Stop {
                        self.blocked = Some(active);
                        return VoiceRuntimeApplyResult {
                            action: None,
                            participant_playback_changed,
                        };
                    }
                    if self.reconnect_attempts >= MAX_VOICE_RECONNECT_ATTEMPTS {
                        self.blocked = Some(active);
                        logging::debug(
                            "voice",
                            format!(
                                "voice reconnect limit reached after {} attempts",
                                MAX_VOICE_RECONNECT_ATTEMPTS
                            ),
                        );
                        return VoiceRuntimeApplyResult {
                            action: None,
                            participant_playback_changed,
                        };
                    }
                    self.reconnect_attempts += 1;
                    return VoiceRuntimeApplyResult {
                        action: self.connect_if_ready(),
                        participant_playback_changed,
                    };
                }
                return VoiceRuntimeApplyResult {
                    action: None,
                    participant_playback_changed,
                };
            }
            VoiceRuntimeEvent::Shutdown => {
                self.push_to_talk_pressed = false;
                return VoiceRuntimeApplyResult {
                    action: self.close_active(),
                    participant_playback_changed,
                };
            }
        }

        VoiceRuntimeApplyResult {
            action: self.connect_if_ready(),
            participant_playback_changed,
        }
    }

    fn record_voice_state(&mut self, state: VoiceStateInfo) -> Option<VoiceRuntimeAction> {
        if self.current_user_id != Some(state.user_id) {
            return None;
        }
        let requested = self.requested?;
        // A leave clears the channel; for a DM that also clears the scope, so we
        // treat any channel-less state for the current user as a disconnect.
        let Some(channel_id) = state.channel_id else {
            self.current_voice = None;
            self.server = None;
            self.push_to_talk_pressed = false;
            return self.close_active();
        };
        if state.scope() != Some(requested.scope) {
            return None;
        }
        let session_id = state
            .session_id
            .filter(|session_id| !session_id.is_empty())?;
        self.current_voice = Some(ObservedSelfVoiceState {
            scope: requested.scope,
            channel_id,
            session_id,
        });
        None
    }

    fn connect_if_ready(&mut self) -> Option<VoiceRuntimeAction> {
        let requested = self.requested?;
        let voice = self.current_voice.as_ref()?;
        if requested.scope != voice.scope || requested.channel_id != voice.channel_id {
            return self.close_active();
        }
        let server = self.server.as_ref()?;
        if server.scope() != Some(requested.scope) {
            return None;
        }
        let endpoint = server.endpoint.as_ref()?.trim_end_matches('/').to_owned();
        if endpoint.is_empty() || server.token.is_empty() {
            return None;
        }
        let mut session = VoiceGatewaySession {
            connection_id: 0,
            scope: requested.scope,
            channel_id: requested.channel_id,
            user_id: self.current_user_id?,
            session_id: voice.session_id.clone(),
            endpoint,
            token: server.token.clone(),
        };
        if self.reconnect_target.as_ref() != Some(&session) {
            self.reconnect_target = Some(session.clone());
            self.reconnect_attempts = 0;
        }
        if self.active.as_ref() == Some(&session) {
            return None;
        }
        if self.blocked.as_ref() == Some(&session) {
            return None;
        }
        self.blocked = None;
        self.next_connection_id = self.next_connection_id.wrapping_add(1).max(1);
        session.connection_id = self.next_connection_id;
        self.active = Some(session.clone());
        Some(VoiceRuntimeAction::Connect(session))
    }

    fn close_active(&mut self) -> Option<VoiceRuntimeAction> {
        self.active.take().map(|_| VoiceRuntimeAction::Close)
    }

    pub(super) fn capture_gate(&self) -> Option<VoiceCaptureGate> {
        let active = self.active.as_ref()?;
        let requested = self.requested?;
        if active.scope != requested.scope || active.channel_id != requested.channel_id {
            return None;
        }
        let capture_enabled = requested.allow_microphone_transmit && !requested.self_mute;
        Some(VoiceCaptureGate {
            capture_enabled,
            transmit_enabled: capture_enabled && (!self.push_to_talk || self.push_to_talk_pressed),
            use_voice_activity: !self.push_to_talk,
            noise_suppression: requested.noise_suppression,
            microphone_sensitivity: requested.microphone_sensitivity,
            microphone_volume: requested.microphone_volume,
        })
    }

    pub(super) fn playback_gate(&self) -> Option<VoicePlaybackGate> {
        let active = self.active.as_ref()?;
        let requested = self.requested?;
        if active.scope != requested.scope || active.channel_id != requested.channel_id {
            return None;
        }
        Some(VoicePlaybackGate {
            enabled: !requested.self_deaf,
            volume: requested.voice_output_volume,
        })
    }
}

pub(crate) fn forward_app_event(
    sender: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    event: &AppEvent,
) {
    let runtime_event = match event {
        AppEvent::Ready { user_id, .. } => VoiceRuntimeEvent::CurrentUserReady(*user_id),
        AppEvent::VoiceStateUpdate { state } => VoiceRuntimeEvent::VoiceState(state.clone()),
        AppEvent::VoiceServerUpdate { server } => VoiceRuntimeEvent::VoiceServer(server.clone()),
        AppEvent::StreamCreate { stream } => VoiceRuntimeEvent::StreamCreate(stream.clone()),
        AppEvent::StreamServerUpdate { server } => VoiceRuntimeEvent::StreamServer(server.clone()),
        AppEvent::StreamDelete { stream } => VoiceRuntimeEvent::StreamDelete(stream.clone()),
        _ => return,
    };
    let _ = sender.send(runtime_event);
}

pub(crate) async fn run_voice_runtime(
    mut events: mpsc::UnboundedReceiver<VoiceRuntimeEvent>,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    gateway_commands_tx: mpsc::UnboundedSender<GatewayCommand>,
    status_publisher: VoiceStatusPublisher,
) {
    let mut state = VoiceRuntimeState::default();
    let mut stream_state = StreamRuntimeState::default();
    let mut broadcast_state = StreamBroadcastRuntimeState::default();
    let mut connection_task: Option<JoinHandle<()>> = None;
    let mut connection_session: Option<VoiceGatewaySession> = None;
    let mut capture_gate_tx: Option<mpsc::UnboundedSender<VoiceCaptureGate>> = None;
    let mut playback_gate_tx: Option<mpsc::UnboundedSender<VoicePlaybackGate>> = None;
    let mut participant_playback_tx: Option<
        watch::Sender<HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>>,
    > = None;
    let mut stream_task: Option<JoinHandle<()>> = None;
    let mut stream_session: Option<StreamGatewaySession> = None;
    let mut broadcast_task: Option<JoinHandle<()>> = None;
    let mut broadcast_session: Option<StreamBroadcastGatewaySession> = None;
    let mut broadcast_stop_tx: Option<oneshot::Sender<()>> = None;

    while let Some(event) = events.recv().await {
        let shutdown = matches!(event, VoiceRuntimeEvent::Shutdown);
        let broadcast_started = match (&event, broadcast_session.as_ref()) {
            (
                VoiceRuntimeEvent::BroadcastStreamConnectionEstablished {
                    connection_id,
                    stream_key,
                },
                Some(session),
            ) => {
                session.connection_id == *connection_id && session.request.stream_key == *stream_key
            }
            _ => false,
        };
        let stream_update = stream_state.apply(&event);
        let broadcast_update = broadcast_state.apply(&event);
        let VoiceRuntimeApplyResult {
            action,
            participant_playback_changed,
        } = state.apply_with_changes(event);
        if let Some(error) = stream_update.error {
            status_publisher.publish_error(error).await;
        }
        if let Some(stream_key) = stream_update.close_stream_key {
            if let Some(stopped_session) = stop_stream_connection_task(
                &mut stream_task,
                &mut stream_session,
                "stopping active stream connection task",
            )
            .await
            {
                status_publisher
                    .publish_stream_playback_ended(
                        stopped_session.request.scope,
                        stopped_session.request.channel_id,
                        stopped_session.request.owner_id,
                        false,
                    )
                    .await;
            }
            if stream_update.send_delete {
                let _ = gateway_commands_tx.send(GatewayCommand::DeleteStream { stream_key });
            }
        }
        if let Some(session) = stream_update.connect {
            if let Some(stopped_session) = stop_stream_connection_task(
                &mut stream_task,
                &mut stream_session,
                "stopping previous stream connection task before reconnect",
            )
            .await
            {
                status_publisher
                    .publish_stream_playback_ended(
                        stopped_session.request.scope,
                        stopped_session.request.channel_id,
                        stopped_session.request.owner_id,
                        true,
                    )
                    .await;
            }
            stream_session = Some(session.clone());
            stream_task = Some(tokio::spawn(run_stream_gateway_session(
                session,
                events_tx.clone(),
                status_publisher.clone(),
            )));
        }
        if let Some(error) = broadcast_update.error {
            status_publisher.publish_error(error).await;
        }
        if let Some(stream_key) = broadcast_update.close_stream_key {
            if let Some(stopped_session) = stop_stream_broadcast_task(
                &mut broadcast_task,
                &mut broadcast_session,
                &mut broadcast_stop_tx,
                "stopping active stream broadcast task",
            )
            .await
            {
                status_publisher
                    .publish_stream_broadcast_ended(
                        stopped_session.request.scope,
                        stopped_session.request.channel_id,
                    )
                    .await;
            }
            if broadcast_update.send_delete {
                let _ = gateway_commands_tx.send(GatewayCommand::DeleteStream { stream_key });
            }
        }
        if let Some(session) = broadcast_update.connect {
            if let Some(stopped_session) = stop_stream_broadcast_task(
                &mut broadcast_task,
                &mut broadcast_session,
                &mut broadcast_stop_tx,
                "stopping previous stream broadcast task before reconnect",
            )
            .await
            {
                status_publisher
                    .publish_stream_broadcast_ended(
                        stopped_session.request.scope,
                        stopped_session.request.channel_id,
                    )
                    .await;
            }
            let (stop_tx, stop_rx) = oneshot::channel();
            broadcast_stop_tx = Some(stop_tx);
            broadcast_session = Some(session.clone());
            broadcast_task = Some(tokio::spawn(run_stream_broadcast_session(
                session,
                events_tx.clone(),
                status_publisher.clone(),
                stop_rx,
            )));
        }
        if broadcast_started && let Some(session) = broadcast_session.as_ref() {
            status_publisher
                .publish_stream_broadcast_started(session.request.scope, session.request.channel_id)
                .await;
        }
        let connected_this_event = matches!(&action, Some(VoiceRuntimeAction::Connect(_)));
        if let Some(action) = action {
            match action {
                VoiceRuntimeAction::Connect(session) => {
                    if let Some(stopped_session) = stop_voice_connection_task(
                        &mut connection_task,
                        &mut connection_session,
                        &mut capture_gate_tx,
                        &mut playback_gate_tx,
                        "stopping previous voice connection task before reconnect",
                    )
                    .await
                    {
                        status_publisher
                            .publish_speaking(&stopped_session, stopped_session.user_id, false)
                            .await;
                    }
                    let (next_capture_gate_tx, capture_gate_rx) = mpsc::unbounded_channel();
                    let (next_playback_gate_tx, playback_gate_rx) = mpsc::unbounded_channel();
                    let (next_participant_playback_tx, participant_playback_rx) =
                        watch::channel(state.participant_playback_settings.clone());
                    capture_gate_tx = Some(next_capture_gate_tx);
                    playback_gate_tx = Some(next_playback_gate_tx);
                    participant_playback_tx = Some(next_participant_playback_tx);
                    let initial_capture_gate = state.capture_gate().unwrap_or(VoiceCaptureGate {
                        capture_enabled: false,
                        transmit_enabled: false,
                        use_voice_activity: true,
                        noise_suppression: false,
                        microphone_sensitivity: MicrophoneSensitivityDb::default(),
                        microphone_volume: VoiceVolumePercent::default(),
                    });
                    let initial_playback_gate =
                        state.playback_gate().unwrap_or(VoicePlaybackGate {
                            enabled: true,
                            volume: VoiceVolumePercent::default(),
                        });
                    connection_session = Some(session.clone());
                    connection_task = Some(tokio::spawn(run_voice_gateway_session(
                        session,
                        events_tx.clone(),
                        status_publisher.clone(),
                        VoiceGatewayControls {
                            initial_capture_gate,
                            capture_gate_rx,
                            initial_playback_gate,
                            playback_gate_rx,
                            participant_playback_rx,
                        },
                    )));
                }
                VoiceRuntimeAction::Close => {
                    if let Some(stopped_session) = stop_voice_connection_task(
                        &mut connection_task,
                        &mut connection_session,
                        &mut capture_gate_tx,
                        &mut playback_gate_tx,
                        "stopping active voice connection task",
                    )
                    .await
                    {
                        status_publisher
                            .publish_speaking(&stopped_session, stopped_session.user_id, false)
                            .await;
                    }
                    participant_playback_tx = None;
                }
            }
        }
        if state.active.is_none() {
            capture_gate_tx = None;
            playback_gate_tx = None;
            participant_playback_tx = None;
        }
        if let (Some(capture_gate_tx), Some(capture_gate)) =
            (capture_gate_tx.as_ref(), state.capture_gate())
        {
            let _ = capture_gate_tx.send(capture_gate);
        }
        if let (Some(playback_gate_tx), Some(playback_gate)) =
            (playback_gate_tx.as_ref(), state.playback_gate())
        {
            let _ = playback_gate_tx.send(playback_gate);
        }
        if participant_playback_changed
            && !connected_this_event
            && let Some(participant_playback_tx) = participant_playback_tx.as_mut()
        {
            participant_playback_tx.send_replace(state.participant_playback_settings.clone());
        }
        if shutdown {
            break;
        }
    }

    if let Some(stopped_session) = stop_voice_connection_task(
        &mut connection_task,
        &mut connection_session,
        &mut capture_gate_tx,
        &mut playback_gate_tx,
        "stopping voice connection task during voice runtime shutdown",
    )
    .await
    {
        status_publisher
            .publish_speaking(&stopped_session, stopped_session.user_id, false)
            .await;
    }
    if let Some(stopped_session) = stop_stream_connection_task(
        &mut stream_task,
        &mut stream_session,
        "stopping stream connection task during voice runtime shutdown",
    )
    .await
    {
        status_publisher
            .publish_stream_playback_ended(
                stopped_session.request.scope,
                stopped_session.request.channel_id,
                stopped_session.request.owner_id,
                false,
            )
            .await;
    }
    if let Some(stopped_session) = stop_stream_broadcast_task(
        &mut broadcast_task,
        &mut broadcast_session,
        &mut broadcast_stop_tx,
        "stopping stream broadcast task during voice runtime shutdown",
    )
    .await
    {
        status_publisher
            .publish_stream_broadcast_ended(
                stopped_session.request.scope,
                stopped_session.request.channel_id,
            )
            .await;
    }
}

async fn stop_stream_connection_task(
    stream_task: &mut Option<JoinHandle<()>>,
    stream_session: &mut Option<StreamGatewaySession>,
    label: &str,
) -> Option<StreamGatewaySession> {
    let stopped_session = stream_session.take();
    let Some(mut task) = stream_task.take() else {
        return stopped_session;
    };
    logging::debug("stream", label);
    task.abort();
    let _ = timeout(Duration::from_millis(100), &mut task).await;
    stopped_session
}

async fn stop_stream_broadcast_task(
    stream_task: &mut Option<JoinHandle<()>>,
    stream_session: &mut Option<StreamBroadcastGatewaySession>,
    stop_tx: &mut Option<oneshot::Sender<()>>,
    label: &str,
) -> Option<StreamBroadcastGatewaySession> {
    let stopped_session = stream_session.take();
    if let Some(stop_tx) = stop_tx.take() {
        let _ = stop_tx.send(());
    }
    let Some(mut task) = stream_task.take() else {
        return stopped_session;
    };
    logging::debug("stream", label);
    match timeout(VOICE_CONNECTION_SHUTDOWN_TIMEOUT, &mut task).await {
        Ok(Ok(())) => {
            logging::debug("stream", "stream broadcast task stopped cleanly");
        }
        Ok(Err(error)) => {
            logging::debug("stream", format!("stream broadcast task ended: {error}"));
        }
        Err(_) => {
            logging::debug("stream", "stream broadcast graceful stop timed out");
            task.abort();
            let _ = timeout(Duration::from_millis(100), &mut task).await;
        }
    }
    stopped_session
}

pub(super) async fn stop_voice_connection_task(
    connection_task: &mut Option<JoinHandle<()>>,
    connection_session: &mut Option<VoiceGatewaySession>,
    capture_gate_tx: &mut Option<mpsc::UnboundedSender<VoiceCaptureGate>>,
    playback_gate_tx: &mut Option<mpsc::UnboundedSender<VoicePlaybackGate>>,
    label: &str,
) -> Option<VoiceGatewaySession> {
    capture_gate_tx.take();
    playback_gate_tx.take();
    let stopped_session = connection_session.take();
    let Some(mut task) = connection_task.take() else {
        return stopped_session;
    };
    logging::debug("voice", label);
    match timeout(VOICE_CONNECTION_SHUTDOWN_TIMEOUT, &mut task).await {
        Ok(Ok(())) => None,
        Ok(Err(error)) => {
            logging::debug("voice", format!("voice connection task ended: {error}"));
            stopped_session
        }
        Err(_) => {
            logging::debug("voice", "voice connection graceful stop timed out");
            task.abort();
            let _ = timeout(Duration::from_millis(100), &mut task).await;
            stopped_session
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn broadcast_stop_waits_for_graceful_cleanup() {
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (cleaned_tx, cleaned_rx) = tokio::sync::oneshot::channel();
        let mut task = Some(tokio::spawn(async move {
            let _ = started_tx.send(());
            let _ = stop_rx.await;
            let _ = cleaned_tx.send(());
        }));
        let mut stop_tx = Some(stop_tx);
        let mut session = None;
        started_rx.await.expect("test broadcast task should start");

        stop_stream_broadcast_task(
            &mut task,
            &mut session,
            &mut stop_tx,
            "stopping test broadcast task",
        )
        .await;

        cleaned_rx
            .await
            .expect("broadcast task should clean up before stop returns");
        assert!(task.is_none());
        assert!(stop_tx.is_none());
    }
}
