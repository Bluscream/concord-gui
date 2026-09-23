use super::*;

#[cfg(feature = "voice-playback")]
pub(super) struct VoiceMicrophoneCapture {
    pub(super) _stream: cpal::Stream,
    pub(super) _processor: VoiceMicrophoneInputProcessor,
    pub(super) stats: Arc<VoiceMicrophoneCaptureStats>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VoiceRuntimeEvent {
    Requested(Option<CurrentVoiceConnectionState>),
    ManualRetry(CurrentVoiceConnectionState),
    AudioSourcesChanged(VoiceAudioSources),
    AudioSourcesApplyFailed {
        connection_id: u64,
        generation: u64,
        requested_sources: VoiceAudioSources,
        active_sources: VoiceAudioSources,
        message: String,
    },
    #[cfg(feature = "voice-playback")]
    PushToTalkEnabledChanged(bool),
    #[cfg(feature = "voice-playback")]
    PushToTalkPressed(bool),
    ReplaceParticipantPlaybackSettings(Vec<(Id<UserMarker>, VoiceParticipantPlaybackSettings)>),
    UpdateParticipantPlaybackSettings {
        user_id: Id<UserMarker>,
        settings: VoiceParticipantPlaybackSettings,
    },
    CurrentUserReady(Option<Id<UserMarker>>),
    VoiceState(VoiceStateInfo),
    VoiceServer(VoiceServerInfo),
    WatchStreamRequested(StreamWatchRequest),
    WatchStreamCancelled {
        stream_key: String,
    },
    StreamCreate(StreamCreateInfo),
    StreamServer(StreamServerInfo),
    StreamDelete(StreamDeleteInfo),
    StreamConnectionEstablished {
        connection_id: u64,
        stream_key: String,
    },
    StreamConnectionEnded {
        connection_id: u64,
        stream_key: String,
        outcome: VoiceConnectionEnd,
    },
    BroadcastStreamRequested(StreamBroadcastRequest),
    BroadcastStreamCaptureReady {
        request_id: u64,
        stream_key: String,
    },
    BroadcastStreamCaptureFailed {
        request_id: u64,
        stream_key: String,
        error: String,
    },
    #[cfg(test)]
    BroadcastStreamCancelled {
        stream_key: String,
    },
    BroadcastStreamStopRequested {
        stream_key: String,
    },
    BroadcastStreamConnectionEstablished {
        connection_id: u64,
        stream_key: String,
    },
    BroadcastStreamConnectionStable {
        connection_id: u64,
        stream_key: String,
    },
    BroadcastStreamConnectionEnded {
        connection_id: u64,
        stream_key: String,
        outcome: VoiceConnectionEnd,
    },
    ConnectionEstablished {
        connection_id: u64,
    },
    ConnectionEnded {
        connection_id: u64,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        session_id: String,
        endpoint: String,
        outcome: VoiceConnectionEnd,
    },
    Shutdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct VoiceAudioSourceSelection {
    pub(super) generation: u64,
    pub(super) sources: VoiceAudioSources,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct VoiceAudioSourcesApplyOutcome {
    pub(super) active_sources: VoiceAudioSources,
    pub(super) error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StreamWatchRequest {
    pub(crate) stream_key: String,
    pub(crate) scope: VoiceScope,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) owner_id: Id<UserMarker>,
    pub(crate) display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StreamBroadcastRequest {
    pub(crate) stream_key: String,
    pub(crate) scope: VoiceScope,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) target: StreamCaptureTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VoiceConnectionEnd {
    Reconnect,
    Stop,
}

#[derive(Clone)]
pub(crate) struct VoiceStatusPublisher {
    pub(super) events: AppEventPublisher,
}

#[derive(Clone)]
pub(super) struct VoiceGatewaySession {
    pub(super) connection_id: u64,
    pub(super) scope: VoiceScope,
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) user_id: Id<UserMarker>,
    pub(super) session_id: String,
    pub(super) endpoint: String,
    pub(super) token: String,
}

impl VoiceStatusPublisher {
    pub(crate) fn new(events: AppEventPublisher) -> Self {
        Self { events }
    }

    pub(super) async fn publish(
        &self,
        session: &VoiceGatewaySession,
        status: VoiceConnectionStatus,
        message: impl Into<String>,
    ) {
        self.events
            .publish(AppEvent::VoiceConnectionStatusChanged {
                scope: session.scope,
                channel_id: Some(session.channel_id),
                status,
                message: Some(message.into()),
            })
            .await;
    }

    pub(super) async fn publish_speaking(
        &self,
        session: &VoiceGatewaySession,
        user_id: Id<UserMarker>,
        speaking: bool,
    ) {
        self.events
            .publish(AppEvent::VoiceSpeakingUpdate {
                scope: session.scope,
                channel_id: session.channel_id,
                user_id,
                speaking,
            })
            .await;
    }

    pub(super) async fn publish_error(&self, message: String) {
        self.events
            .publish(AppEvent::GatewayError { message })
            .await;
    }

    pub(super) async fn publish_audio_sources_apply_failed(
        &self,
        requested_sources: VoiceAudioSources,
        active_sources: VoiceAudioSources,
        message: String,
    ) {
        self.events
            .publish(AppEvent::VoiceAudioSourcesApplyFailed {
                requested_input_source: requested_sources.input,
                requested_output_source: requested_sources.output,
                active_input_source: active_sources.input,
                active_output_source: active_sources.output,
                message,
            })
            .await;
    }

    pub(super) async fn publish_stream_playback_ready(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    ) {
        self.events
            .publish(AppEvent::StreamPlaybackWindowReady {
                scope,
                channel_id,
                user_id,
            })
            .await;
    }

    pub(super) async fn publish_stream_playback_ended(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        reconnecting: bool,
    ) {
        self.events
            .publish(AppEvent::StreamPlaybackEnded {
                scope,
                channel_id,
                user_id,
                reconnecting,
            })
            .await;
    }

    pub(super) async fn publish_stream_broadcast_started(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) {
        self.events
            .publish(AppEvent::StreamBroadcastStarted { scope, channel_id })
            .await;
    }

    pub(super) async fn publish_stream_broadcast_audio_unavailable(&self, message: String) {
        self.events
            .publish(AppEvent::StreamBroadcastAudioUnavailable { message })
            .await;
    }

    pub(super) async fn publish_stream_broadcast_ended(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) {
        self.events
            .publish(AppEvent::StreamBroadcastEnded { scope, channel_id })
            .await;
    }
}

impl VoiceGatewaySession {
    pub(super) fn connection_established_event(&self) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::ConnectionEstablished {
            connection_id: self.connection_id,
        }
    }

    pub(super) fn matches_connection_end(
        &self,
        connection_id: u64,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        session_id: &str,
        endpoint: &str,
    ) -> bool {
        self.connection_id == connection_id
            && self.scope == scope
            && self.channel_id == channel_id
            && self.session_id == session_id
            && self.endpoint == endpoint
    }

    pub(super) fn connection_ended_event(&self, outcome: VoiceConnectionEnd) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::ConnectionEnded {
            connection_id: self.connection_id,
            scope: self.scope,
            channel_id: self.channel_id,
            session_id: self.session_id.clone(),
            endpoint: self.endpoint.clone(),
            outcome,
        }
    }
}

impl PartialEq for VoiceGatewaySession {
    fn eq(&self, other: &Self) -> bool {
        self.scope == other.scope
            && self.channel_id == other.channel_id
            && self.user_id == other.user_id
            && self.session_id == other.session_id
            && self.endpoint == other.endpoint
            && self.token == other.token
    }
}

impl Eq for VoiceGatewaySession {}

impl fmt::Debug for VoiceGatewaySession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceGatewaySession")
            .field("connection_id", &self.connection_id)
            .field("scope", &self.scope)
            .field("channel_id", &self.channel_id)
            .field("user_id", &self.user_id)
            .field("session_id", &"<redacted>")
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct VoiceTransportSession {
    pub(super) ssrc: u32,
    pub(super) ip: String,
    pub(super) port: u16,
    pub(super) modes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DiscoveredVoiceAddress {
    pub(super) address: String,
    pub(super) port: u16,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct VoiceSessionDescription {
    pub(super) audio_codec: String,
    pub(super) mode: String,
    pub(super) secret_key: Vec<u8>,
    pub(super) dave_protocol_version: Option<u64>,
    pub(super) video_codec: Option<String>,
    pub(super) media_session_id: String,
    pub(super) keyframe_interval: Option<u64>,
}

impl fmt::Debug for VoiceSessionDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceSessionDescription")
            .field("audio_codec", &self.audio_codec)
            .field("mode", &self.mode)
            .field("secret_key", &"<redacted>")
            .field("secret_key_len", &self.secret_key.len())
            .field("dave_protocol_version", &self.dave_protocol_version)
            .field("video_codec", &self.video_codec)
            .field("media_session_id", &self.media_session_id)
            .field("keyframe_interval", &self.keyframe_interval)
            .finish()
    }
}

impl VoiceSessionDescription {
    pub(super) fn uses_same_transport(&self, other: &Self) -> bool {
        self.mode == other.mode && self.secret_key == other.secret_key
    }
}

pub(super) struct VoiceSpeakingTracker {
    pub(super) local_user_id: Id<UserMarker>,
    pub(super) remote_deadlines: HashMap<Id<UserMarker>, Instant>,
    pub(super) local_speaking: bool,
}

impl VoiceSpeakingTracker {
    pub(super) fn new(local_user_id: Id<UserMarker>) -> Self {
        Self {
            local_user_id,
            remote_deadlines: HashMap::new(),
            local_speaking: false,
        }
    }

    pub(super) fn record_remote(
        &mut self,
        user_id: Id<UserMarker>,
        speaking: bool,
        now: Instant,
    ) -> Option<bool> {
        if user_id == self.local_user_id {
            return None;
        }
        if speaking {
            let was_active = self.remote_deadlines.contains_key(&user_id);
            self.remote_deadlines
                .insert(user_id, now + VOICE_REMOTE_SPEAKING_TTL);
            return (!was_active).then_some(true);
        }
        if self.remote_deadlines.remove(&user_id).is_some() {
            Some(false)
        } else {
            None
        }
    }

    pub(super) fn record_local(&mut self, speaking: bool) -> Option<bool> {
        if self.local_speaking == speaking {
            return None;
        }
        self.local_speaking = speaking;
        Some(speaking)
    }

    pub(super) fn expire_remote(&mut self, now: Instant) -> Vec<Id<UserMarker>> {
        let expired = self
            .remote_deadlines
            .iter()
            .filter_map(|(user_id, deadline)| (*deadline <= now).then_some(*user_id))
            .collect::<Vec<_>>();
        for user_id in &expired {
            self.remote_deadlines.remove(user_id);
        }
        expired
    }

    pub(super) fn clear_all(&mut self) -> Vec<Id<UserMarker>> {
        let mut cleared = self.remote_deadlines.keys().copied().collect::<Vec<_>>();
        self.remote_deadlines.clear();
        if self.local_speaking {
            self.local_speaking = false;
            if !cleared.contains(&self.local_user_id) {
                cleared.push(self.local_user_id);
            }
        }
        cleared
    }
}
