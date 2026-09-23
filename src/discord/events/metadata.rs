use crate::discord::{ChannelInfo, SnapshotAreas, is_thread_kind};

use super::app_event::{AppEvent, AppEventKind};

#[derive(Clone, Debug)]
pub struct SequencedAppEvent {
    pub revision: u64,
    pub event: AppEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppEventMetadata {
    /// `Some` means the event mutates `DiscordState` and names the areas whose
    /// revision must advance. Applying without advancing a revision would leave
    /// the TUI permanently unaware of the write, so the two facts are one field
    /// rather than a bool that can drift out of step with the areas.
    pub snapshot_areas: Option<SnapshotAreas>,
    pub needs_effect_delivery: bool,
}

impl AppEventMetadata {
    pub const fn mutating(snapshot_areas: SnapshotAreas) -> Self {
        Self {
            snapshot_areas: Some(snapshot_areas),
            needs_effect_delivery: false,
        }
    }

    pub const fn mutating_effect(snapshot_areas: SnapshotAreas) -> Self {
        Self {
            snapshot_areas: Some(snapshot_areas),
            needs_effect_delivery: true,
        }
    }

    pub const fn effect_only() -> Self {
        Self {
            snapshot_areas: None,
            needs_effect_delivery: true,
        }
    }

    pub const fn inert() -> Self {
        Self {
            snapshot_areas: None,
            needs_effect_delivery: false,
        }
    }
}

impl AppEventKind {
    pub const fn metadata(self) -> AppEventMetadata {
        match self {
        // Forgetting drops cached state, so the snapshot must absorb it
        // exactly like a delete does.
        AppEventKind::GuildForgotten
        | AppEventKind::GuildCreate
        | AppEventKind::GuildUpdate
        | AppEventKind::GuildOnboardingUpdate
        | AppEventKind::ThreadListSync
        | AppEventKind::ArchivedThreadsLoaded
        | AppEventKind::ThreadMembersUpdateDispatch
        | AppEventKind::ChannelUpsert
        | AppEventKind::LazyPrivateChannelUpsert
        | AppEventKind::ChannelRecipientAdd
        | AppEventKind::ChannelRecipientRemove
        | AppEventKind::Ready => AppEventMetadata::mutating(SnapshotAreas::navigation()),


        AppEventKind::ThreadUpsert => {
            AppEventMetadata::mutating_effect(SnapshotAreas::navigation_and_message())
        }

        AppEventKind::ForumPostDataLoaded => {
            AppEventMetadata::mutating_effect(SnapshotAreas::navigation_and_message())
        }

        AppEventKind::ArchivedThreadsLoadFailed => {
            AppEventMetadata::mutating_effect(SnapshotAreas::navigation())
        }

        AppEventKind::MessageCreate => {
            AppEventMetadata::mutating_effect(SnapshotAreas::navigation_and_message())
        }

        AppEventKind::MessageHistoryLoaded
        | AppEventKind::MessageHistoryRefreshed
        | AppEventKind::MessageHistoryAfterLoaded
        | AppEventKind::MessageHistoryAroundLoaded
        | AppEventKind::MessageSearchLoaded
        | AppEventKind::ThreadPreviewLoaded
        | AppEventKind::PinnedMessagesLoaded => {
            AppEventMetadata::mutating_effect(SnapshotAreas::message())
        }

        AppEventKind::MessageUpdateDispatch
        | AppEventKind::CurrentUserReactionAdd
        | AppEventKind::CurrentUserReactionRemove
        | AppEventKind::MessageReactionAdd
        | AppEventKind::MessageReactionRemove
        | AppEventKind::MessageReactionRemoveAll
        | AppEventKind::MessageReactionRemoveEmoji
        | AppEventKind::MessagePinnedUpdate
        | AppEventKind::CurrentUserPollVoteUpdate
        | AppEventKind::MessageDelete
        // A resolved embed is attached to the message it describes, so the
        // message cache changes and the log has to be reprojected.
        | AppEventKind::EmbedResolved
        | AppEventKind::MessageDeleteBulk => {
            AppEventMetadata::mutating(SnapshotAreas::message())
        }

        AppEventKind::ChannelPinsUpdate => {
            AppEventMetadata::mutating(SnapshotAreas::message_and_detail())
        }

        AppEventKind::SelectedMessageChannelChanged => {
            AppEventMetadata::mutating(SnapshotAreas::navigation_and_message())
        }

        AppEventKind::UserProfileLoaded => {
            AppEventMetadata::mutating_effect(SnapshotAreas::navigation_and_message())
        }

        AppEventKind::GuildDelete
        | AppEventKind::ChannelDelete
        | AppEventKind::ReadySnapshotComplete
        | AppEventKind::ReadySupplementalComplete
        | AppEventKind::GuildMemberListUpdate
        | AppEventKind::GuildMembersChunk
        | AppEventKind::GuildMemberAdd
        | AppEventKind::GuildMemberUpsert
        | AppEventKind::RelationshipsLoaded
        | AppEventKind::RelationshipUpsert
        | AppEventKind::RelationshipUpdate
        | AppEventKind::UserIdentityUpdate
        | AppEventKind::RelationshipRemove
        | AppEventKind::ThreadMemberUpdate
        | AppEventKind::ThreadMemberListUpdate
        | AppEventKind::VoiceStateUpdate
        | AppEventKind::TypingStart
        | AppEventKind::ReadyUserDirectory => {
            AppEventMetadata::mutating(SnapshotAreas::navigation_and_message())
        }

        AppEventKind::GuildUnavailable => AppEventMetadata::inert(),

        AppEventKind::GatewayReidentified => {
            AppEventMetadata::mutating_effect(SnapshotAreas::navigation())
        }

        AppEventKind::SelectedGuildChanged
        | AppEventKind::GuildRolesUpdate
        | AppEventKind::GuildRoleUpsert
        | AppEventKind::GuildRoleDelete
        | AppEventKind::GuildEmojisUpdate
        | AppEventKind::GuildMemberRemove
        | AppEventKind::PresenceUpdate
        | AppEventKind::VoiceSpeakingUpdate
        | AppEventKind::CallDelete
        | AppEventKind::UserSettingsUpdate
        | AppEventKind::UserNotificationSettingsUpdate
        | AppEventKind::UserNoteLoaded
        | AppEventKind::CurrentUserVerification
        | AppEventKind::UserGuildSettingsInit
        | AppEventKind::UserGuildSettingsSync
        | AppEventKind::UserGuildSettingsUpdate => {
            AppEventMetadata::mutating(SnapshotAreas::navigation())
        }

        AppEventKind::ReadStateInit
        | AppEventKind::ReadStateSync
        | AppEventKind::MessageAck
        | AppEventKind::FeatureReadStateAck
        | AppEventKind::ChannelPinsAck
        | AppEventKind::ChannelUnreadUpdate => {
            AppEventMetadata::mutating(SnapshotAreas::navigation_and_detail())
        }

        // Invites carry no state of their own: resolving one changes
        // nothing, and accepting one is followed by the GuildCreate that
        // actually adds the guild.
        AppEventKind::GuildBansLoaded
        | AppEventKind::GuildBansLoadFailed
        // Sounds belong to the picker that asked for them, and a sound
        // somebody played is an effect rather than state.
        | AppEventKind::SoundboardSoundPlayed
        | AppEventKind::SoundboardSoundsLoaded
        | AppEventKind::SoundboardSoundsLoadFailed
        | AppEventKind::StageInstanceLoaded
        | AppEventKind::StageRequestFailed
        | AppEventKind::DiscoverableGuildsLoaded
        | AppEventKind::DiscoveryMetadataLoaded
        | AppEventKind::GuildStickersLoaded
        | AppEventKind::OnboardingLoaded
        | AppEventKind::OnboardingCompleted
        | AppEventKind::ScheduledEventsLoaded
        | AppEventKind::GuildTemplatesLoaded
        | AppEventKind::MembersBulkBanned
        | AppEventKind::PruneCountLoaded
        | AppEventKind::GuildPruned
        | AppEventKind::WelcomeScreenLoaded
        | AppEventKind::GuildWidgetLoaded
        | AppEventKind::MembershipRequestFailed
        | AppEventKind::AccountModified
        | AppEventKind::AccountModifyFailed
        | AppEventKind::TotpEnabled
        | AppEventKind::TotpDisabled
        | AppEventKind::TotpFailed
        | AppEventKind::BackupCodesLoaded
        | AppEventKind::BackupCodesFailed
        | AppEventKind::AuthSessionsLoaded
        | AppEventKind::AuthSessionsLoadFailed
        | AppEventKind::AuthorisedAppsLoaded
        | AppEventKind::AuthorisedAppsLoadFailed
        | AppEventKind::ConnectionsLoaded
        | AppEventKind::ConnectionsLoadFailed
        | AppEventKind::AutoModRulesLoaded
        | AppEventKind::AutoModRulesLoadFailed
        | AppEventKind::GuildInvitesLoaded
        | AppEventKind::GuildInvitesLoadFailed
        | AppEventKind::GuildEmojisLoaded
        | AppEventKind::GuildEmojisLoadFailed
        | AppEventKind::GuildAuditLogLoaded
        | AppEventKind::GuildAuditLogLoadFailed
        | AppEventKind::InviteCreated
        | AppEventKind::InviteResolved
        | AppEventKind::InviteResolveFailed
        | AppEventKind::InviteAccepted
        | AppEventKind::InviteAcceptFailed
        | AppEventKind::GatewayError
        | AppEventKind::CaptchaRequired
        | AppEventKind::MessageSendFailed
        | AppEventKind::MessageSendRateLimited
        | AppEventKind::MessageSendCooldownStarted
        | AppEventKind::GatewayDispatchReceived
        | AppEventKind::SignedOut
        | AppEventKind::MediaPlaybackWindowReady
        | AppEventKind::StreamPlaybackWindowReady
        | AppEventKind::StreamPlaybackEnded
        | AppEventKind::StreamCaptureTargetsLoaded
        | AppEventKind::VoiceAudioSourcesLoaded
        | AppEventKind::VoiceAudioSourcesApplyFailed
        | AppEventKind::StreamBroadcastStarted
        | AppEventKind::StreamBroadcastAudioUnavailable
        | AppEventKind::StreamBroadcastStartFailed
        | AppEventKind::StreamBroadcastEnded
        | AppEventKind::ApplicationCommandsLoaded
        | AppEventKind::ApplicationCommandIndexUpdated
        | AppEventKind::InteractionSucceeded
        | AppEventKind::InteractionFailed
        | AppEventKind::ApplicationCommandAutocompleteResponse
        | AppEventKind::AttachmentDownloadStarted
        | AppEventKind::AttachmentDownloadProgress
        | AppEventKind::AttachmentDownloadCompleted
        | AppEventKind::AttachmentDownloadFailed
        | AppEventKind::UpdateAvailable
        | AppEventKind::ReactionUsersLoaded
        | AppEventKind::ReactionUsersLoadFailed
        | AppEventKind::AttachmentPreviewLoaded
        | AppEventKind::AttachmentPreviewLoadFailed
        | AppEventKind::EmbedResolveFailed
        | AppEventKind::ThreadPreviewLoadFailed
        | AppEventKind::ForumPostDataLoadFailed
        | AppEventKind::MessageSearchLoadFailed
        | AppEventKind::MessageHistoryLoadFailed
        | AppEventKind::InboxMentionsLoaded
        | AppEventKind::InboxMentionsLoadFailed
        | AppEventKind::InboxRecentMentionDeleted
        | AppEventKind::InboxRecentMentionDeleteFailed
        | AppEventKind::InboxChannelMessagesLoaded
        | AppEventKind::InboxChannelMessagesLoadFailed
        | AppEventKind::PinnedMessagesLoadFailed
        | AppEventKind::UserProfileLoadFailed
        | AppEventKind::UserProfileUpdateFailed
        | AppEventKind::VoiceConnectionStatusChanged
        | AppEventKind::VoiceSound
        | AppEventKind::RichPresenceDetected
        | AppEventKind::GatewayResumed
        | AppEventKind::GatewayClosed => AppEventMetadata::effect_only(),

        AppEventKind::StreamCreate
        | AppEventKind::StreamUpdate
        | AppEventKind::StreamDelete => AppEventMetadata::mutating(SnapshotAreas::navigation()),

        AppEventKind::VoiceServerUpdate | AppEventKind::StreamServerUpdate => {
            AppEventMetadata::inert()
        }

        // The current user's Nitro tier is stored in the session (part of
        // the navigation snapshot area) so the upload-limit check can read
        // it, and it still needs effect delivery so the TUI can update
        // Nitro-gated UI such as the emoji picker.
        AppEventKind::CurrentUserCapabilities => {
            AppEventMetadata::mutating_effect(SnapshotAreas::navigation())
        }

        AppEventKind::ThreadNotificationLevelUpdate | AppEventKind::ThreadMuteUpdate => {
            AppEventMetadata::mutating(SnapshotAreas::navigation())
        }
    }
    }
}

impl AppEvent {
    pub fn metadata(&self) -> AppEventMetadata {
        match self {
            AppEvent::ChannelUpsert(channel) if channel_upsert_needs_effect_delivery(channel) => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation())
            }
            _ => self.kind().metadata(),
        }
    }

    pub fn needs_effect_delivery(&self) -> bool {
        self.metadata().needs_effect_delivery
    }

    pub fn snapshot_areas(&self) -> Option<SnapshotAreas> {
        self.metadata().snapshot_areas
    }
}

fn channel_upsert_needs_effect_delivery(channel: &ChannelInfo) -> bool {
    channel.parent_id.is_some() && is_thread_kind(&channel.kind)
}
