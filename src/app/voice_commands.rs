use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, UserMarker},
};
use crate::{
    DiscordClient,
    discord::{
        AppEvent, MicrophoneSensitivityDb, VoiceConnectionStatus, VoiceParticipantPlaybackSettings,
        VoiceScope, VoiceVolumePercent,
    },
    logging,
};

/// Join carries the whole audio configuration alongside the target channel, so
/// it travels as one value rather than a nine-argument call.
pub(super) struct JoinRequest {
    pub scope: VoiceScope,
    pub channel_id: Id<ChannelMarker>,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub allow_microphone_transmit: bool,
    pub microphone_sensitivity: MicrophoneSensitivityDb,
    pub microphone_volume: VoiceVolumePercent,
    pub voice_output_volume: VoiceVolumePercent,
    pub participant_playback_settings: Vec<(Id<UserMarker>, VoiceParticipantPlaybackSettings)>,
}

pub(super) async fn join_channel(client: DiscordClient, request: JoinRequest) {
    let JoinRequest {
        scope,
        channel_id,
        self_mute,
        self_deaf,
        allow_microphone_transmit,
        microphone_sensitivity,
        microphone_volume,
        voice_output_volume,
        participant_playback_settings,
    } = request;

    client.replace_voice_participant_playback_settings(participant_playback_settings);
    if let Err(message) = client.request_voice_join(scope, channel_id, self_mute, self_deaf) {
        logging::error("app", &message);
        client
            .publish_event(AppEvent::VoiceConnectionStatusChanged {
                scope,
                channel_id: Some(channel_id),
                status: VoiceConnectionStatus::Failed,
                message: Some(message),
            })
            .await;
        return;
    }

    if let Err(message) = client.update_voice_capture_permission(
        scope,
        channel_id,
        allow_microphone_transmit,
        microphone_sensitivity,
        microphone_volume,
        voice_output_volume,
    ) {
        logging::error("app", &message);
        let _ = client.update_voice_capture_permission(
            scope,
            channel_id,
            false,
            microphone_sensitivity,
            microphone_volume,
            voice_output_volume,
        );
        client
            .publish_event(AppEvent::GatewayError {
                message: format!("Joined voice listen-only: {message}"),
            })
            .await;
    }
    client
        .publish_event(AppEvent::VoiceConnectionStatusChanged {
            scope,
            channel_id: Some(channel_id),
            status: VoiceConnectionStatus::Connecting,
            message: Some("Voice join requested".to_owned()),
        })
        .await;
}

pub(super) async fn update_state(
    client: DiscordClient,
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    self_mute: bool,
    self_deaf: bool,
) {
    if let Err(message) = client.update_voice_state(scope, Some(channel_id), self_mute, self_deaf) {
        logging::error("app", &message);
        client
            .publish_event(AppEvent::GatewayError { message })
            .await;
    }
}

pub(super) async fn update_capture_permission(
    client: DiscordClient,
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    allow_microphone_transmit: bool,
    microphone_sensitivity: MicrophoneSensitivityDb,
    microphone_volume: VoiceVolumePercent,
    voice_output_volume: VoiceVolumePercent,
) {
    if let Err(message) = client.update_voice_capture_permission(
        scope,
        channel_id,
        allow_microphone_transmit,
        microphone_sensitivity,
        microphone_volume,
        voice_output_volume,
    ) {
        logging::error("app", &message);
        client
            .publish_event(AppEvent::GatewayError { message })
            .await;
    }
}

pub(super) fn update_participant_playback(
    client: &DiscordClient,
    user_id: Id<UserMarker>,
    settings: VoiceParticipantPlaybackSettings,
) {
    client.update_voice_participant_playback_settings(user_id, settings);
}

pub(super) async fn leave_channel(
    client: DiscordClient,
    scope: VoiceScope,
    self_mute: bool,
    self_deaf: bool,
) {
    if let Err(message) = client.update_voice_state(scope, None, self_mute, self_deaf) {
        logging::error("app", &message);
        client
            .publish_event(AppEvent::VoiceConnectionStatusChanged {
                scope,
                channel_id: None,
                status: VoiceConnectionStatus::Failed,
                message: Some(message),
            })
            .await;
    } else {
        client
            .publish_event(AppEvent::VoiceConnectionStatusChanged {
                scope,
                channel_id: None,
                status: VoiceConnectionStatus::Disconnected,
                message: Some("Voice leave requested".to_owned()),
            })
            .await;
    }
}
