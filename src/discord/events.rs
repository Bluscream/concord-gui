use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use super::commands::{
    AttachmentDownloadId, DownloadAttachmentSource, ForumPostArchiveState, MediaPlaybackRequestId,
    MessageHistoryAfterMode, MessageSearchPage, MessageSearchQuery, ReactionEmoji,
    StreamCaptureTargetsRequestId,
};
use super::{
    ActivityInfo, AttachmentUpdate, ChannelInfo, ChannelRecipientInfo, CustomEmojiInfo, EmbedInfo,
    GuildBoostTier, GuildNotificationSettingsInfo, GuildOnboardingInfo, GuildVerificationLevel,
    MemberInfo, MentionInfo, MessageInfo, PollInfo, PremiumTier, PresenceStatus, ReactionUserInfo,
    ReadStateInfo, RelationshipInfo, RelationshipUpdateInfo, RoleInfo, SnapshotAreas,
    StreamCaptureTarget, StreamCreateInfo, StreamDeleteInfo, StreamServerInfo, StreamUpdateInfo,
    UserProfileInfo, UserSettingsInfo, VoiceConnectionStatus, VoiceScope, VoiceServerInfo,
    VoiceSoundKind, VoiceStateInfo, is_thread_kind,
};
use super::{ApplicationCommandChoiceInfo, ApplicationCommandInfo};

#[cfg(test)]
use super::PollAnswerInfo;

#[derive(Clone, Debug, PartialEq)]
pub struct GatewayDispatchInfo {
    pub event_type: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelUnreadInfo {
    pub channel_id: Id<ChannelMarker>,
    pub last_message_id: Option<Option<Id<MessageMarker>>>,
    pub last_pin_timestamp: Option<Option<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageUpdateEventFields {
    pub poll: Option<PollInfo>,
    pub content: Option<String>,
    pub sticker_names: Option<Vec<String>>,
    pub stickers: Option<Vec<crate::discord::StickerInfo>>,
    pub mentions: Option<Vec<MentionInfo>>,
    pub mention_everyone: Option<bool>,
    pub mention_roles: Option<Vec<Id<RoleMarker>>>,
    pub flags: Option<u64>,
    pub attachments: AttachmentUpdate,
    pub embeds: Option<Vec<EmbedInfo>>,
    pub edited_timestamp: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MessageUpdateDispatchInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub fields: MessageUpdateEventFields,
    pub extra_fields: BTreeMap<String, Value>,
}

impl Default for MessageUpdateEventFields {
    fn default() -> Self {
        Self {
            poll: None,
            content: None,
            sticker_names: None,
            stickers: None,
            mentions: None,
            mention_everyone: None,
            mention_roles: None,
            flags: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresenceEventFields {
    pub user_id: Id<UserMarker>,
    pub status: PresenceStatus,
    pub activities: Vec<ActivityInfo>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserGuildSettingsInfo {
    pub notification_settings: GuildNotificationSettingsInfo,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadListSyncInfo {
    pub guild_id: Id<GuildMarker>,
    /// `None` means every parent channel in the guild. A present list limits
    /// replacement to those parents, including parents with no active threads.
    pub channel_ids: Option<Vec<Id<ChannelMarker>>>,
    pub threads: Vec<ChannelInfo>,
    pub thread_members: Vec<Value>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadMemberUpdateInfo {
    pub user_id: Id<UserMarker>,
    pub flags: Option<u64>,
    pub muted: Option<bool>,
    pub mute_end_time: Option<String>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadMembersUpdateInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub member_count: Option<u64>,
    pub added_members: Vec<ThreadMemberUpdateInfo>,
    pub removed_user_ids: Vec<Id<UserMarker>>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GuildMemberListItem {
    Member {
        member: MemberInfo,
        presence: Option<PresenceEventFields>,
    },
    Group {
        id: String,
        count: u64,
    },
    Unknown {
        raw: Value,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum GuildMemberListOperation {
    Sync {
        range: (u32, u32),
        items: Vec<GuildMemberListItem>,
    },
    Insert {
        index: u32,
        item: GuildMemberListItem,
    },
    Update {
        index: u32,
        item: GuildMemberListItem,
    },
    Delete {
        index: u32,
    },
    Invalidate {
        range: (u32, u32),
    },
    /// An operation Concord does not understand cannot be treated as a no-op.
    /// Keeping the raw value lets state invalidate the list conservatively and
    /// preserves enough data to add support once Discord introduces it.
    Unknown {
        name: Option<String>,
        raw: Value,
    },
}

impl GuildMemberListOperation {
    pub fn items(&self) -> &[GuildMemberListItem] {
        match self {
            Self::Sync { items, .. } => items,
            Self::Insert { item, .. } | Self::Update { item, .. } => std::slice::from_ref(item),
            Self::Delete { .. } | Self::Invalidate { .. } | Self::Unknown { .. } => &[],
        }
    }
}

impl GuildMemberListItem {
    pub fn member(&self) -> Option<(&MemberInfo, Option<&PresenceEventFields>)> {
        match self {
            Self::Member { member, presence } => Some((member, presence.as_ref())),
            Self::Group { .. } | Self::Unknown { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildMemberListUpdateInfo {
    pub guild_id: Id<GuildMarker>,
    pub list_id: Option<String>,
    pub member_count: Option<u64>,
    pub online_count: Option<u32>,
    pub groups: Vec<Value>,
    pub ops: Vec<GuildMemberListOperation>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadySnapshotInfo {
    /// `None` means the source payload omitted the field, so existing state
    /// must not be reconciled from an incomplete test or future payload.
    pub guild_ids: Option<Vec<Id<GuildMarker>>>,
    /// Guild channel collections are authoritative when present in READY.
    pub guild_channel_ids: BTreeMap<Id<GuildMarker>, Vec<Id<ChannelMarker>>>,
    /// READY and READY_SUPPLEMENTAL together form the private-channel snapshot.
    pub private_channel_ids: Option<Vec<Id<ChannelMarker>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildMembersChunkInfo {
    pub guild_id: Id<GuildMarker>,
    pub members: Vec<MemberInfo>,
    pub presences: Vec<PresenceEventFields>,
    pub chunk_index: Option<u64>,
    pub chunk_count: Option<u64>,
    pub nonce: Option<String>,
    pub not_found: Vec<Id<UserMarker>>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug)]
pub enum AppEvent {
    GatewayDispatchReceived {
        dispatch: GatewayDispatchInfo,
    },
    Ready {
        user: String,
        user_id: Option<Id<UserMarker>>,
    },
    ReadyUserDirectory {
        users: Vec<ChannelRecipientInfo>,
    },
    /// Marks the end of READY parsing. State uses the complete ID sets to
    /// remove guilds and guild channels that belonged only to an older
    /// Gateway session. Private channels wait for READY_SUPPLEMENTAL.
    ReadySnapshotComplete {
        snapshot: ReadySnapshotInfo,
    },
    /// Marks the end of READY_SUPPLEMENTAL private-channel parsing so the
    /// READY and supplemental ID sets can be reconciled as one snapshot.
    ReadySupplementalComplete {
        private_channel_ids: Vec<Id<ChannelMarker>>,
    },
    SignedOut,
    CurrentUserCapabilities {
        premium_tier: PremiumTier,
    },
    CurrentUserVerification {
        email_verified: Option<bool>,
        phone_verified: Option<bool>,
        mfa_enabled: Option<bool>,
    },
    UserIdentityUpdate {
        user_id: Id<UserMarker>,
        username: String,
        global_name: Option<String>,
        avatar_url: Option<String>,
        is_bot: bool,
    },
    ApplicationCommandsLoaded {
        guild_id: Option<Id<GuildMarker>>,
        commands: Vec<ApplicationCommandInfo>,
    },
    ApplicationCommandIndexUpdated {
        guild_id: Id<GuildMarker>,
    },
    InteractionSucceeded {
        interaction_id: u64,
        nonce: Option<String>,
        correlated: bool,
    },
    InteractionFailed {
        interaction_id: u64,
        nonce: Option<String>,
        reason_code: u64,
        correlated: bool,
    },
    ApplicationCommandAutocompleteResponse {
        nonce: Option<String>,
        choices: Vec<ApplicationCommandChoiceInfo>,
    },
    GuildCreate {
        guild_id: Id<GuildMarker>,
        name: String,
        member_count: Option<u64>,
        /// Snowflake of the guild owner. The owner short-circuits permission
        /// checks (sees every channel regardless of overwrites).
        owner_id: Option<Id<UserMarker>>,
        boost_tier: GuildBoostTier,
        boost_count: u32,
        verification_level: Option<GuildVerificationLevel>,
        mfa_level: Option<u64>,
        features: Option<Vec<String>>,
        onboarding: Option<GuildOnboardingInfo>,
        channels: Vec<ChannelInfo>,
        members: Vec<MemberInfo>,
        presences: Vec<(Id<UserMarker>, PresenceStatus)>,
        roles: Option<Vec<RoleInfo>>,
        emojis: Vec<CustomEmojiInfo>,
        /// The guild's own stickers, which are the ones this account can send
        /// there without Nitro.
        stickers: Vec<crate::discord::StickerInfo>,
    },
    GuildUpdate {
        guild_id: Id<GuildMarker>,
        name: String,
        owner_id: Option<Id<UserMarker>>,
        // `Some` only when this GUILD_UPDATE payload actually carried the field,
        // so a rename does not reset a guild's boost state to unboosted.
        boost_tier: Option<GuildBoostTier>,
        boost_count: Option<u32>,
        verification_level: Option<GuildVerificationLevel>,
        mfa_level: Option<u64>,
        features: Option<Vec<String>>,
        onboarding: Option<GuildOnboardingInfo>,
        roles: Option<Vec<RoleInfo>>,
        emojis: Option<Vec<CustomEmojiInfo>>,
    },
    GuildOnboardingUpdate {
        guild_id: Id<GuildMarker>,
        onboarding: GuildOnboardingInfo,
    },
    GuildRolesUpdate {
        guild_id: Id<GuildMarker>,
        roles: Vec<RoleInfo>,
    },
    GuildRoleUpsert {
        guild_id: Id<GuildMarker>,
        role: RoleInfo,
    },
    GuildRoleDelete {
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
    },
    GuildEmojisUpdate {
        guild_id: Id<GuildMarker>,
        emojis: Vec<CustomEmojiInfo>,
    },
    GuildDelete {
        guild_id: Id<GuildMarker>,
    },
    GuildUnavailable {
        guild_id: Id<GuildMarker>,
    },
    SelectedGuildChanged {
        guild_id: Option<Id<GuildMarker>>,
    },
    SelectedMessageChannelChanged {
        channel_id: Option<Id<ChannelMarker>>,
    },
    ChannelUpsert(ChannelInfo),
    LazyPrivateChannelUpsert {
        channel: ChannelInfo,
        recipient_ids: Vec<Id<UserMarker>>,
    },
    ChannelRecipientAdd {
        channel_id: Id<ChannelMarker>,
        recipient: ChannelRecipientInfo,
    },
    ChannelRecipientRemove {
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    },
    ChannelDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
    },
    ThreadListSync {
        sync: ThreadListSyncInfo,
    },
    ThreadMembersUpdateDispatch {
        update: ThreadMembersUpdateInfo,
    },
    ThreadMemberUpdate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        flags: Option<u64>,
        muted: Option<bool>,
        mute_end_time: Option<String>,
    },
    MessageCreate {
        message: MessageInfo,
    },
    MessageSendFailed {
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
    },
    MessageSendRateLimited {
        channel_id: Id<ChannelMarker>,
        retry_after_millis: u64,
    },
    MessageSendCooldownStarted {
        channel_id: Id<ChannelMarker>,
        duration_millis: u64,
    },
    MessageHistoryLoaded {
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        messages: Vec<MessageInfo>,
    },
    MessageHistoryRefreshed {
        channel_id: Id<ChannelMarker>,
        messages: Vec<MessageInfo>,
    },
    MessageHistoryAfterLoaded {
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
        messages: Vec<MessageInfo>,
        has_more: bool,
        mode: MessageHistoryAfterMode,
    },
    MessageHistoryAroundLoaded {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        messages: Vec<MessageInfo>,
    },
    ThreadPreviewLoaded {
        channel_id: Id<ChannelMarker>,
        message: MessageInfo,
    },
    ThreadPreviewLoadFailed {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    ForumPostsLoaded {
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
        next_offset: usize,
        threads: Vec<ChannelInfo>,
        first_messages: Vec<MessageInfo>,
        has_more: bool,
    },
    ForumPostsLoadFailed {
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
        message: String,
    },
    MessageSearchLoaded {
        page: MessageSearchPage,
    },
    MessageSearchLoadFailed {
        query: MessageSearchQuery,
        message: String,
    },
    InboxMentionsLoaded {
        request_id: u64,
        before: Option<Id<MessageMarker>>,
        messages: Vec<MessageInfo>,
        has_more: bool,
    },
    InboxMentionsLoadFailed {
        request_id: u64,
        before: Option<Id<MessageMarker>>,
    },
    InboxRecentMentionDeleted {
        message_id: Id<MessageMarker>,
    },
    InboxRecentMentionDeleteFailed {
        message_id: Id<MessageMarker>,
        message: String,
    },
    InboxChannelMessagesLoaded {
        request_id: u64,
        channel_id: Id<ChannelMarker>,
        messages: Vec<MessageInfo>,
    },
    InboxChannelMessagesLoadFailed {
        request_id: u64,
        channel_id: Id<ChannelMarker>,
    },
    MessageHistoryLoadFailed {
        channel_id: Id<ChannelMarker>,
        target: MessageHistoryLoadTarget,
        message: String,
    },
    MessageUpdateDispatch {
        update: MessageUpdateDispatchInfo,
    },
    MessageDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    MessageDeleteBulk {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_ids: Vec<Id<MessageMarker>>,
    },
    GuildMemberListUpdate {
        update: GuildMemberListUpdateInfo,
    },
    GuildMembersChunk {
        chunk: GuildMembersChunkInfo,
    },
    GuildMemberUpsert {
        guild_id: Id<GuildMarker>,
        member: MemberInfo,
    },
    GuildMemberAdd {
        guild_id: Id<GuildMarker>,
        member: MemberInfo,
    },
    GuildMemberRemove {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    },
    PresenceUpdate {
        guild_id: Option<Id<GuildMarker>>,
        presence: PresenceEventFields,
    },
    /// Rich Presence activities published by local apps over the RPC socket. Not a
    /// gateway dispatch: emitted so the profile popup can list detectable apps. It
    /// does not change presence on its own.
    RichPresenceDetected {
        activities: Vec<ActivityInfo>,
    },
    VoiceStateUpdate {
        state: VoiceStateInfo,
    },
    VoiceSpeakingUpdate {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        speaking: bool,
    },
    VoiceServerUpdate {
        server: VoiceServerInfo,
    },
    StreamCreate {
        stream: StreamCreateInfo,
    },
    StreamUpdate {
        stream: StreamUpdateInfo,
    },
    StreamServerUpdate {
        server: StreamServerInfo,
    },
    StreamDelete {
        stream: StreamDeleteInfo,
    },
    VoiceConnectionStatusChanged {
        scope: VoiceScope,
        channel_id: Option<Id<ChannelMarker>>,
        status: VoiceConnectionStatus,
        message: Option<String>,
    },
    VoiceAudioSourcesLoaded {
        request_id: u64,
        inputs: Vec<(String, String)>,
        outputs: Vec<(String, String)>,
        error: Option<String>,
    },
    VoiceAudioSourcesApplyFailed {
        requested_input_source: Option<String>,
        requested_output_source: Option<String>,
        active_input_source: Option<String>,
        active_output_source: Option<String>,
        message: String,
    },
    VoiceSound {
        kind: VoiceSoundKind,
    },
    /// A DM or group-DM call ended; every voice state in that channel is dropped.
    CallDelete {
        channel_id: Id<ChannelMarker>,
    },
    /// Discord's TYPING_START dispatch: emitted ~10s before the typing
    /// indicator should expire. The dashboard tracks the latest timestamp
    /// per (channel, user) and shows "X is typing…" while it's fresh.
    TypingStart {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        member: Option<MemberInfo>,
    },
    CurrentUserReactionAdd {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    CurrentUserReactionRemove {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    MessageReactionAdd {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
        emoji: ReactionEmoji,
    },
    MessageReactionRemove {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
        emoji: ReactionEmoji,
    },
    MessageReactionRemoveAll {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    MessageReactionRemoveEmoji {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    MessagePinnedUpdate {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    },
    ChannelPinsUpdate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        last_pin_timestamp: Option<String>,
    },
    PinnedMessagesLoaded {
        channel_id: Id<ChannelMarker>,
        messages: Vec<MessageInfo>,
    },
    PinnedMessagesLoadFailed {
        channel_id: Id<ChannelMarker>,
        message: String,
    },
    CurrentUserPollVoteUpdate {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: Vec<u8>,
    },
    ReactionUsersLoaded {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
        users: Vec<ReactionUserInfo>,
        next_after: Option<Id<UserMarker>>,
        /// The cursor this page was requested with: `None` replaces the emoji's
        /// users (first page), `Some` appends (next page).
        after: Option<Id<UserMarker>>,
    },
    ReactionUsersLoadFailed {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    UserSettingsUpdate {
        settings: UserSettingsInfo,
    },
    UserNotificationSettingsUpdate {
        flags: u64,
    },
    UserGuildSettingsInit {
        settings: Vec<UserGuildSettingsInfo>,
    },
    UserGuildSettingsSync {
        settings: Vec<UserGuildSettingsInfo>,
        partial: bool,
        version: Option<i64>,
    },
    UserGuildSettingsUpdate {
        settings: UserGuildSettingsInfo,
    },
    GatewayError {
        message: String,
    },
    /// A REST action was refused until Discord's CAPTCHA is solved. `action`
    /// labels what was attempted (e.g. "send message"). Shown as a transient
    /// toast, never the gateway-error banner, since the connection is fine.
    CaptchaRequired {
        action: String,
    },
    GuildBansLoaded {
        guild_id: Id<GuildMarker>,
        bans: Vec<crate::discord::GuildBanInfo>,
    },
    GuildBansLoadFailed {
        guild_id: Id<GuildMarker>,
        message: String,
    },
    /// A departed guild's cache was dropped at the user's request.
    GuildForgotten {
        guild_id: Id<GuildMarker>,
    },
    /// Somebody played a soundboard sound in a voice channel we are in.
    ///
    /// Discord sends the same event for emoji reactions in voice, which carry
    /// no sound - those arrive with no `sound_id` and are not turned into this.
    SoundboardSoundPlayed {
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        sound_id: u64,
        /// 0 to 1, as the sender configured it.
        volume: f64,
    },
    SoundboardSoundsLoaded {
        /// `None` for the default sounds, which belong to no guild.
        guild_id: Option<Id<GuildMarker>>,
        sounds: Vec<crate::discord::SoundboardSound>,
    },
    SoundboardSoundsLoadFailed {
        guild_id: Option<Id<GuildMarker>>,
        message: String,
    },
    ConnectionsLoaded {
        connections: Vec<crate::discord::Connection>,
    },
    AuthSessionsLoaded {
        sessions: Vec<crate::discord::AuthSession>,
    },
    AuthSessionsLoadFailed {
        message: String,
    },
    /// The live stage in a channel, or `None` when nobody has started one.
    StageInstanceLoaded {
        channel_id: Id<ChannelMarker>,
        instance: Option<crate::discord::StageInstance>,
    },
    StageRequestFailed {
        message: String,
    },
    DiscoverableGuildsLoaded {
        guilds: Vec<crate::discord::DiscoverableGuild>,
    },
    DiscoveryMetadataLoaded {
        guild_id: Id<GuildMarker>,
        metadata: Box<crate::discord::DiscoveryMetadata>,
        categories: Vec<crate::discord::DiscoveryCategory>,
    },
    GuildStickersLoaded {
        guild_id: Id<GuildMarker>,
        stickers: Vec<crate::discord::GuildSticker>,
    },
    OnboardingLoaded {
        guild_id: Id<GuildMarker>,
        onboarding: Box<crate::discord::Onboarding>,
    },
    OnboardingCompleted {
        guild_id: Id<GuildMarker>,
    },
    ScheduledEventsLoaded {
        guild_id: Id<GuildMarker>,
        events: Vec<crate::discord::ScheduledEvent>,
    },
    GuildTemplatesLoaded {
        guild_id: Id<GuildMarker>,
        templates: Vec<crate::discord::GuildTemplate>,
    },
    MembersBulkBanned {
        guild_id: Id<GuildMarker>,
        /// How many were actually banned, which is not always how many were
        /// asked for.
        banned: usize,
        attempted: usize,
    },
    PruneCountLoaded {
        guild_id: Id<GuildMarker>,
        count: u64,
    },
    GuildPruned {
        guild_id: Id<GuildMarker>,
        count: u64,
    },
    WelcomeScreenLoaded {
        guild_id: Id<GuildMarker>,
        screen: crate::discord::WelcomeScreen,
    },
    GuildWidgetLoaded {
        guild_id: Id<GuildMarker>,
        widget: crate::discord::GuildWidget,
    },
    MembershipRequestFailed {
        message: String,
    },
    AccountModified,
    AccountModifyFailed {
        message: String,
    },
    /// Two-factor is on. Carries the backup codes, which arrive once and are
    /// the only thing between a lost phone and a lost account.
    TotpEnabled {
        backup_codes: Vec<crate::discord::BackupCode>,
    },
    TotpDisabled,
    TotpFailed {
        message: String,
    },
    BackupCodesLoaded {
        codes: Vec<crate::discord::BackupCode>,
    },
    BackupCodesFailed {
        message: String,
    },
    AuthorisedAppsLoaded {
        apps: Vec<crate::discord::AuthorisedApp>,
    },
    AuthorisedAppsLoadFailed {
        message: String,
    },
    ConnectionsLoadFailed {
        message: String,
    },
    AutoModRulesLoaded {
        guild_id: Id<GuildMarker>,
        rules: Vec<crate::discord::AutoModRule>,
    },
    AutoModRulesLoadFailed {
        guild_id: Id<GuildMarker>,
        message: String,
    },
    GuildInvitesLoaded {
        guild_id: Id<GuildMarker>,
        invites: Vec<crate::discord::GuildInviteInfo>,
    },
    GuildInvitesLoadFailed {
        guild_id: Id<GuildMarker>,
        message: String,
    },
    GuildEmojisLoaded {
        guild_id: Id<GuildMarker>,
        emojis: Vec<crate::discord::GuildEmojiInfo>,
    },
    GuildEmojisLoadFailed {
        guild_id: Id<GuildMarker>,
        message: String,
    },
    GuildAuditLogLoaded {
        guild_id: Id<GuildMarker>,
        entries: Vec<crate::discord::AuditLogEntryInfo>,
    },
    GuildAuditLogLoadFailed {
        guild_id: Id<GuildMarker>,
        message: String,
    },
    /// A new invite, so the code can be shown and copied without a refetch.
    InviteCreated {
        channel_id: Id<ChannelMarker>,
        code: String,
    },
    InviteResolved {
        preview: crate::discord::rest::InvitePreview,
    },
    InviteResolveFailed {
        code: String,
        message: String,
    },
    InviteAccepted {
        code: String,
        guild_id: Option<Id<GuildMarker>>,
    },
    InviteAcceptFailed {
        code: String,
        message: String,
    },
    MediaPlaybackWindowReady {
        request_id: MediaPlaybackRequestId,
        url: String,
    },
    StreamPlaybackWindowReady {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    },
    StreamPlaybackEnded {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        reconnecting: bool,
    },
    StreamCaptureTargetsLoaded {
        request_id: StreamCaptureTargetsRequestId,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        targets: Vec<StreamCaptureTarget>,
        error: Option<String>,
    },
    StreamBroadcastStarted {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    },
    StreamBroadcastAudioUnavailable {
        message: String,
    },
    StreamBroadcastStartFailed {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    },
    StreamBroadcastEnded {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    },
    AttachmentDownloadStarted {
        id: AttachmentDownloadId,
        filename: String,
        total_bytes: Option<u64>,
        source: DownloadAttachmentSource,
    },
    AttachmentDownloadProgress {
        id: AttachmentDownloadId,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    AttachmentDownloadCompleted {
        id: AttachmentDownloadId,
        path: String,
        source: DownloadAttachmentSource,
    },
    AttachmentDownloadFailed {
        id: AttachmentDownloadId,
        filename: String,
        message: String,
        source: DownloadAttachmentSource,
    },
    UpdateAvailable {
        latest_version: String,
    },
    AttachmentPreviewLoaded {
        url: String,
        bytes: Vec<u8>,
    },
    AttachmentPreviewLoadFailed {
        url: String,
        message: String,
    },
    UserProfileLoaded {
        guild_id: Option<Id<GuildMarker>>,
        profile: UserProfileInfo,
    },
    UserProfileLoadFailed {
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
        message: String,
    },
    UserProfileUpdateFailed {
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
        message: String,
    },
    UserNoteLoaded {
        user_id: Id<UserMarker>,
        note: Option<String>,
    },
    RelationshipsLoaded {
        relationships: Vec<RelationshipInfo>,
    },
    RelationshipUpsert {
        relationship: RelationshipInfo,
    },
    RelationshipUpdate {
        update: RelationshipUpdateInfo,
    },
    RelationshipRemove {
        user_id: Id<UserMarker>,
    },
    /// Full read-state replacement used by internal and test data sources.
    ReadStateInit {
        entries: Vec<ReadStateInfo>,
    },
    /// READY read states with their versioned-array replacement semantics.
    ReadStateSync {
        entries: Vec<ReadStateInfo>,
        partial: bool,
        version: Option<i64>,
    },
    /// Gateway `MESSAGE_ACK` or a locally synthesized ack on activation.
    MessageAck {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        mention_count: Option<u32>,
        flags: Option<u64>,
        last_viewed: Option<u64>,
        /// Gateway acknowledgements carry the aggregate read-state version.
        /// Locally synthesized optimistic acknowledgements leave it unknown.
        version: Option<i64>,
    },
    FeatureReadStateAck {
        read_state_type: u8,
        resource_id: u64,
        entity_id: u64,
        version: i64,
    },
    ChannelPinsAck {
        channel_id: Id<ChannelMarker>,
        timestamp: String,
        version: i64,
    },
    ChannelUnreadUpdate {
        guild_id: Id<GuildMarker>,
        channels: Vec<ChannelUnreadInfo>,
    },
    GatewayResumed,
    GatewayReidentified,
    GatewayClosed,
    /// Optimistic update for the current user's notification level on a thread,
    /// published by the `SetThreadNotificationLevel` command handler on success.
    ThreadNotificationLevelUpdate {
        channel_id: Id<ChannelMarker>,
        flags: u64,
    },
    /// Optimistic update for the current user's thread member mute settings.
    ThreadMuteUpdate {
        channel_id: Id<ChannelMarker>,
        muted: bool,
        mute_end_time: Option<String>,
    },
}

macro_rules! define_app_event_kinds {
    ($($kind:ident: $pattern:pat,)*) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum AppEventKind {
            $($kind,)*
        }

        impl AppEvent {
            pub(crate) fn kind(&self) -> AppEventKind {
                match self {
                    $($pattern => AppEventKind::$kind,)*
                }
            }
        }
    };
}

define_app_event_kinds! {
    GatewayDispatchReceived: AppEvent::GatewayDispatchReceived { .. },
    Ready: AppEvent::Ready { .. },
    ReadyUserDirectory: AppEvent::ReadyUserDirectory { .. },
    ReadySnapshotComplete: AppEvent::ReadySnapshotComplete { .. },
    ReadySupplementalComplete: AppEvent::ReadySupplementalComplete { .. },
    SignedOut: AppEvent::SignedOut,
    CurrentUserCapabilities: AppEvent::CurrentUserCapabilities { .. },
    CurrentUserVerification: AppEvent::CurrentUserVerification { .. },
    UserIdentityUpdate: AppEvent::UserIdentityUpdate { .. },
    ApplicationCommandsLoaded: AppEvent::ApplicationCommandsLoaded { .. },
    ApplicationCommandIndexUpdated: AppEvent::ApplicationCommandIndexUpdated { .. },
    InteractionSucceeded: AppEvent::InteractionSucceeded { .. },
    InteractionFailed: AppEvent::InteractionFailed { .. },
    ApplicationCommandAutocompleteResponse: AppEvent::ApplicationCommandAutocompleteResponse { .. },
    GuildCreate: AppEvent::GuildCreate { .. },
    GuildUpdate: AppEvent::GuildUpdate { .. },
    GuildOnboardingUpdate: AppEvent::GuildOnboardingUpdate { .. },
    GuildRolesUpdate: AppEvent::GuildRolesUpdate { .. },
    GuildRoleUpsert: AppEvent::GuildRoleUpsert { .. },
    GuildRoleDelete: AppEvent::GuildRoleDelete { .. },
    GuildEmojisUpdate: AppEvent::GuildEmojisUpdate { .. },
    GuildDelete: AppEvent::GuildDelete { .. },
    GuildUnavailable: AppEvent::GuildUnavailable { .. },
    SelectedGuildChanged: AppEvent::SelectedGuildChanged { .. },
    SelectedMessageChannelChanged: AppEvent::SelectedMessageChannelChanged { .. },
    ChannelUpsert: AppEvent::ChannelUpsert(_),
    LazyPrivateChannelUpsert: AppEvent::LazyPrivateChannelUpsert { .. },
    ChannelRecipientAdd: AppEvent::ChannelRecipientAdd { .. },
    ChannelRecipientRemove: AppEvent::ChannelRecipientRemove { .. },
    ChannelDelete: AppEvent::ChannelDelete { .. },
    ThreadListSync: AppEvent::ThreadListSync { .. },
    ThreadMembersUpdateDispatch: AppEvent::ThreadMembersUpdateDispatch { .. },
    ThreadMemberUpdate: AppEvent::ThreadMemberUpdate { .. },
    MessageCreate: AppEvent::MessageCreate { .. },
    MessageSendFailed: AppEvent::MessageSendFailed { .. },
    MessageSendRateLimited: AppEvent::MessageSendRateLimited { .. },
    MessageSendCooldownStarted: AppEvent::MessageSendCooldownStarted { .. },
    MessageHistoryLoaded: AppEvent::MessageHistoryLoaded { .. },
    MessageHistoryRefreshed: AppEvent::MessageHistoryRefreshed { .. },
    MessageHistoryAfterLoaded: AppEvent::MessageHistoryAfterLoaded { .. },
    MessageHistoryAroundLoaded: AppEvent::MessageHistoryAroundLoaded { .. },
    ThreadPreviewLoaded: AppEvent::ThreadPreviewLoaded { .. },
    ThreadPreviewLoadFailed: AppEvent::ThreadPreviewLoadFailed { .. },
    ForumPostsLoaded: AppEvent::ForumPostsLoaded { .. },
    ForumPostsLoadFailed: AppEvent::ForumPostsLoadFailed { .. },
    MessageSearchLoaded: AppEvent::MessageSearchLoaded { .. },
    MessageSearchLoadFailed: AppEvent::MessageSearchLoadFailed { .. },
    InboxMentionsLoaded: AppEvent::InboxMentionsLoaded { .. },
    InboxMentionsLoadFailed: AppEvent::InboxMentionsLoadFailed { .. },
    InboxRecentMentionDeleted: AppEvent::InboxRecentMentionDeleted { .. },
    InboxRecentMentionDeleteFailed: AppEvent::InboxRecentMentionDeleteFailed { .. },
    InboxChannelMessagesLoaded: AppEvent::InboxChannelMessagesLoaded { .. },
    InboxChannelMessagesLoadFailed: AppEvent::InboxChannelMessagesLoadFailed { .. },
    MessageHistoryLoadFailed: AppEvent::MessageHistoryLoadFailed { .. },
    MessageUpdateDispatch: AppEvent::MessageUpdateDispatch { .. },
    MessageDelete: AppEvent::MessageDelete { .. },
    MessageDeleteBulk: AppEvent::MessageDeleteBulk { .. },
    GuildMemberListUpdate: AppEvent::GuildMemberListUpdate { .. },
    GuildMembersChunk: AppEvent::GuildMembersChunk { .. },
    GuildMemberUpsert: AppEvent::GuildMemberUpsert { .. },
    GuildMemberAdd: AppEvent::GuildMemberAdd { .. },
    GuildMemberRemove: AppEvent::GuildMemberRemove { .. },
    PresenceUpdate: AppEvent::PresenceUpdate { .. },
    RichPresenceDetected: AppEvent::RichPresenceDetected { .. },
    VoiceStateUpdate: AppEvent::VoiceStateUpdate { .. },
    VoiceSpeakingUpdate: AppEvent::VoiceSpeakingUpdate { .. },
    VoiceServerUpdate: AppEvent::VoiceServerUpdate { .. },
    StreamCreate: AppEvent::StreamCreate { .. },
    StreamUpdate: AppEvent::StreamUpdate { .. },
    StreamServerUpdate: AppEvent::StreamServerUpdate { .. },
    StreamDelete: AppEvent::StreamDelete { .. },
    VoiceConnectionStatusChanged: AppEvent::VoiceConnectionStatusChanged { .. },
    VoiceAudioSourcesLoaded: AppEvent::VoiceAudioSourcesLoaded { .. },
    VoiceAudioSourcesApplyFailed: AppEvent::VoiceAudioSourcesApplyFailed { .. },
    VoiceSound: AppEvent::VoiceSound { .. },
    CallDelete: AppEvent::CallDelete { .. },
    TypingStart: AppEvent::TypingStart { .. },
    CurrentUserReactionAdd: AppEvent::CurrentUserReactionAdd { .. },
    CurrentUserReactionRemove: AppEvent::CurrentUserReactionRemove { .. },
    MessageReactionAdd: AppEvent::MessageReactionAdd { .. },
    MessageReactionRemove: AppEvent::MessageReactionRemove { .. },
    MessageReactionRemoveAll: AppEvent::MessageReactionRemoveAll { .. },
    MessageReactionRemoveEmoji: AppEvent::MessageReactionRemoveEmoji { .. },
    MessagePinnedUpdate: AppEvent::MessagePinnedUpdate { .. },
    ChannelPinsUpdate: AppEvent::ChannelPinsUpdate { .. },
    PinnedMessagesLoaded: AppEvent::PinnedMessagesLoaded { .. },
    PinnedMessagesLoadFailed: AppEvent::PinnedMessagesLoadFailed { .. },
    CurrentUserPollVoteUpdate: AppEvent::CurrentUserPollVoteUpdate { .. },
    ReactionUsersLoaded: AppEvent::ReactionUsersLoaded { .. },
    ReactionUsersLoadFailed: AppEvent::ReactionUsersLoadFailed { .. },
    UserSettingsUpdate: AppEvent::UserSettingsUpdate { .. },
    UserNotificationSettingsUpdate: AppEvent::UserNotificationSettingsUpdate { .. },
    UserGuildSettingsInit: AppEvent::UserGuildSettingsInit { .. },
    UserGuildSettingsSync: AppEvent::UserGuildSettingsSync { .. },
    UserGuildSettingsUpdate: AppEvent::UserGuildSettingsUpdate { .. },
    GatewayError: AppEvent::GatewayError { .. },
    CaptchaRequired: AppEvent::CaptchaRequired { .. },
    GuildBansLoaded: AppEvent::GuildBansLoaded { .. },
    GuildBansLoadFailed: AppEvent::GuildBansLoadFailed { .. },
    GuildForgotten: AppEvent::GuildForgotten { .. },
    SoundboardSoundPlayed: AppEvent::SoundboardSoundPlayed { .. },
    SoundboardSoundsLoaded: AppEvent::SoundboardSoundsLoaded { .. },
    SoundboardSoundsLoadFailed: AppEvent::SoundboardSoundsLoadFailed { .. },
    ConnectionsLoaded: AppEvent::ConnectionsLoaded { .. },
    AuthSessionsLoaded: AppEvent::AuthSessionsLoaded { .. },
    AuthSessionsLoadFailed: AppEvent::AuthSessionsLoadFailed { .. },
    StageInstanceLoaded: AppEvent::StageInstanceLoaded { .. },
    StageRequestFailed: AppEvent::StageRequestFailed { .. },
    DiscoverableGuildsLoaded: AppEvent::DiscoverableGuildsLoaded { .. },
    DiscoveryMetadataLoaded: AppEvent::DiscoveryMetadataLoaded { .. },
    GuildStickersLoaded: AppEvent::GuildStickersLoaded { .. },
    OnboardingLoaded: AppEvent::OnboardingLoaded { .. },
    OnboardingCompleted: AppEvent::OnboardingCompleted { .. },
    ScheduledEventsLoaded: AppEvent::ScheduledEventsLoaded { .. },
    GuildTemplatesLoaded: AppEvent::GuildTemplatesLoaded { .. },
    MembersBulkBanned: AppEvent::MembersBulkBanned { .. },
    PruneCountLoaded: AppEvent::PruneCountLoaded { .. },
    GuildPruned: AppEvent::GuildPruned { .. },
    WelcomeScreenLoaded: AppEvent::WelcomeScreenLoaded { .. },
    GuildWidgetLoaded: AppEvent::GuildWidgetLoaded { .. },
    MembershipRequestFailed: AppEvent::MembershipRequestFailed { .. },
    AccountModified: AppEvent::AccountModified,
    AccountModifyFailed: AppEvent::AccountModifyFailed { .. },
    TotpEnabled: AppEvent::TotpEnabled { .. },
    TotpDisabled: AppEvent::TotpDisabled,
    TotpFailed: AppEvent::TotpFailed { .. },
    BackupCodesLoaded: AppEvent::BackupCodesLoaded { .. },
    BackupCodesFailed: AppEvent::BackupCodesFailed { .. },
    AuthorisedAppsLoaded: AppEvent::AuthorisedAppsLoaded { .. },
    AuthorisedAppsLoadFailed: AppEvent::AuthorisedAppsLoadFailed { .. },
    ConnectionsLoadFailed: AppEvent::ConnectionsLoadFailed { .. },
    AutoModRulesLoaded: AppEvent::AutoModRulesLoaded { .. },
    AutoModRulesLoadFailed: AppEvent::AutoModRulesLoadFailed { .. },
    GuildInvitesLoaded: AppEvent::GuildInvitesLoaded { .. },
    GuildInvitesLoadFailed: AppEvent::GuildInvitesLoadFailed { .. },
    GuildEmojisLoaded: AppEvent::GuildEmojisLoaded { .. },
    GuildEmojisLoadFailed: AppEvent::GuildEmojisLoadFailed { .. },
    GuildAuditLogLoaded: AppEvent::GuildAuditLogLoaded { .. },
    GuildAuditLogLoadFailed: AppEvent::GuildAuditLogLoadFailed { .. },
    InviteCreated: AppEvent::InviteCreated { .. },
    InviteResolved: AppEvent::InviteResolved { .. },
    InviteResolveFailed: AppEvent::InviteResolveFailed { .. },
    InviteAccepted: AppEvent::InviteAccepted { .. },
    InviteAcceptFailed: AppEvent::InviteAcceptFailed { .. },
    ThreadNotificationLevelUpdate: AppEvent::ThreadNotificationLevelUpdate { .. },
    ThreadMuteUpdate: AppEvent::ThreadMuteUpdate { .. },
    MediaPlaybackWindowReady: AppEvent::MediaPlaybackWindowReady { .. },
    StreamPlaybackWindowReady: AppEvent::StreamPlaybackWindowReady { .. },
    StreamPlaybackEnded: AppEvent::StreamPlaybackEnded { .. },
    StreamCaptureTargetsLoaded: AppEvent::StreamCaptureTargetsLoaded { .. },
    StreamBroadcastStarted: AppEvent::StreamBroadcastStarted { .. },
    StreamBroadcastAudioUnavailable: AppEvent::StreamBroadcastAudioUnavailable { .. },
    StreamBroadcastStartFailed: AppEvent::StreamBroadcastStartFailed { .. },
    StreamBroadcastEnded: AppEvent::StreamBroadcastEnded { .. },
    AttachmentDownloadStarted: AppEvent::AttachmentDownloadStarted { .. },
    AttachmentDownloadProgress: AppEvent::AttachmentDownloadProgress { .. },
    AttachmentDownloadCompleted: AppEvent::AttachmentDownloadCompleted { .. },
    AttachmentDownloadFailed: AppEvent::AttachmentDownloadFailed { .. },
    UpdateAvailable: AppEvent::UpdateAvailable { .. },
    AttachmentPreviewLoaded: AppEvent::AttachmentPreviewLoaded { .. },
    AttachmentPreviewLoadFailed: AppEvent::AttachmentPreviewLoadFailed { .. },
    UserProfileLoaded: AppEvent::UserProfileLoaded { .. },
    UserProfileLoadFailed: AppEvent::UserProfileLoadFailed { .. },
    UserProfileUpdateFailed: AppEvent::UserProfileUpdateFailed { .. },
    UserNoteLoaded: AppEvent::UserNoteLoaded { .. },
    RelationshipsLoaded: AppEvent::RelationshipsLoaded { .. },
    RelationshipUpsert: AppEvent::RelationshipUpsert { .. },
    RelationshipUpdate: AppEvent::RelationshipUpdate { .. },
    RelationshipRemove: AppEvent::RelationshipRemove { .. },
    ReadStateInit: AppEvent::ReadStateInit { .. },
    ReadStateSync: AppEvent::ReadStateSync { .. },
    MessageAck: AppEvent::MessageAck { .. },
    FeatureReadStateAck: AppEvent::FeatureReadStateAck { .. },
    ChannelPinsAck: AppEvent::ChannelPinsAck { .. },
    ChannelUnreadUpdate: AppEvent::ChannelUnreadUpdate { .. },
    GatewayResumed: AppEvent::GatewayResumed,
    GatewayReidentified: AppEvent::GatewayReidentified,
    GatewayClosed: AppEvent::GatewayClosed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageHistoryLoadTarget {
    Latest,
    Older { before: Id<MessageMarker> },
    Newer { after: Id<MessageMarker> },
    Around { message_id: Id<MessageMarker> },
}

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
        | AppEventKind::ThreadMembersUpdateDispatch
        | AppEventKind::ChannelUpsert
        | AppEventKind::LazyPrivateChannelUpsert
        | AppEventKind::ChannelRecipientAdd
        | AppEventKind::ChannelRecipientRemove
        | AppEventKind::Ready => AppEventMetadata::mutating(SnapshotAreas::navigation()),

        AppEventKind::ForumPostsLoaded => {
            AppEventMetadata::mutating_effect(SnapshotAreas::navigation_and_message())
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
        | AppEventKind::UserGuildSettingsUpdate
        | AppEventKind::ThreadMemberUpdate => {
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
        | AppEventKind::ThreadPreviewLoadFailed
        | AppEventKind::ForumPostsLoadFailed
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

#[cfg(test)]
fn poll_result_info_from_fields<'a>(
    fields: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<PollInfo> {
    let mut question = None;
    let mut winner_id = None;
    let mut winner_text = None;
    let mut winner_votes = None;
    let mut total_votes = None;
    for (name, value) in fields {
        match name {
            "poll_question_text" => question = Some(value.to_owned()),
            "victor_answer_id" => winner_id = value.parse::<u8>().ok(),
            "victor_answer_text" => winner_text = Some(value.to_owned()),
            "victor_answer_votes" => winner_votes = value.parse::<u64>().ok(),
            "total_votes" => total_votes = value.parse::<u64>().ok(),
            _ => {}
        }
    }

    let question = question.unwrap_or_else(|| "Poll results".to_owned());
    let answers = winner_text
        .map(|text| {
            vec![PollAnswerInfo {
                answer_id: winner_id.unwrap_or(1),
                text,
                vote_count: winner_votes,
                me_voted: false,
            }]
        })
        .unwrap_or_default();

    Some(PollInfo {
        answers,
        results_finalized: Some(true),
        total_votes,
        ..PollInfo::test(question)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::{AttachmentInfo, AttachmentMediaType};

    #[test]
    pub fn attachment_media_classification_controls_inline_preview() {
        let video = attachment_info("clip.mp4", Some("video/mp4"));
        assert!(video.media_type() == Some(AttachmentMediaType::Video));
        assert_eq!(video.inline_preview_url(), None);
        assert_eq!(
            video.inline_preview_info().map(|info| (
                info.url,
                info.proxy_url,
                info.proxy_preview_only,
            )),
            Some((
                "https://media.discordapp.net/clip.mp4",
                Some("https://media.discordapp.net/clip.mp4"),
                true,
            ))
        );

        let image = attachment_info("cat.png", Some("image/png"));
        assert!(image.media_type() == Some(AttachmentMediaType::Image));
        assert_eq!(
            image.inline_preview_url(),
            Some("https://cdn.discordapp.com/cat.png")
        );
        assert_eq!(
            image.inline_preview_info().and_then(|info| info.proxy_url),
            Some("https://media.discordapp.net/cat.png")
        );

        assert!(attachment_info("CAT.PNG", None).media_type() == Some(AttachmentMediaType::Image));
        assert!(attachment_info("CLIP.MP4", None).media_type() == Some(AttachmentMediaType::Video));
        assert!(
            attachment_info("MUSIC.MP3", None).media_type() == Some(AttachmentMediaType::Audio)
        );
    }

    #[test]
    pub fn poll_result_embed_fields_map_to_poll_summary() {
        let poll = poll_result_info_from_fields([
            ("poll_question_text", "오늘 뭐 먹지?"),
            ("victor_answer_id", "1"),
            ("victor_answer_text", "김치찌개"),
            ("victor_answer_votes", "5"),
            ("total_votes", "7"),
        ])
        .expect("poll result fields should map");

        assert_eq!(poll.question, "오늘 뭐 먹지?");
        assert_eq!(poll.total_votes, Some(7));
        assert_eq!(poll.results_finalized, Some(true));
        assert_eq!(poll.answers[0].text, "김치찌개");
        assert_eq!(poll.answers[0].vote_count, Some(5));
    }

    #[test]
    pub fn event_metadata_routes_each_delivery_category() {
        let cases = [
            (
                "mutating, snapshot only",
                AppEvent::MessageDeleteBulk {
                    guild_id: Some(Id::new(1)),
                    channel_id: Id::new(10),
                    message_ids: vec![Id::new(20), Id::new(30)],
                },
                Some(SnapshotAreas::message()),
                false,
            ),
            (
                "mutating, also delivered as an effect",
                AppEvent::CurrentUserCapabilities {
                    premium_tier: PremiumTier::Nitro,
                },
                Some(SnapshotAreas::navigation()),
                true,
            ),
            ("effect only", AppEvent::GatewayClosed, None, true),
            (
                "inert",
                AppEvent::GuildUnavailable {
                    guild_id: Id::new(1),
                },
                None,
                false,
            ),
            (
                "typing updates shared member and message identity",
                AppEvent::TypingStart {
                    guild_id: Some(Id::new(1)),
                    channel_id: Id::new(10),
                    user_id: Id::new(20),
                    member: None,
                },
                Some(SnapshotAreas::navigation_and_message()),
                false,
            ),
            (
                "ready user directory joins guild and message identity",
                AppEvent::ReadyUserDirectory {
                    users: vec![ChannelRecipientInfo::test(Id::new(20), "Ready User")],
                },
                Some(SnapshotAreas::navigation_and_message()),
                false,
            ),
        ];

        for (label, event, expected_areas, expected_effect) in cases {
            assert_eq!(event.snapshot_areas(), expected_areas, "{label}");
            assert_eq!(event.needs_effect_delivery(), expected_effect, "{label}");
        }
    }

    pub fn attachment_info(filename: &str, content_type: Option<&str>) -> AttachmentInfo {
        AttachmentInfo {
            url: format!("https://cdn.discordapp.com/{filename}"),
            proxy_url: format!("https://media.discordapp.net/{filename}"),
            content_type: content_type.map(str::to_owned),
            size: 1024,
            width: Some(640),
            height: Some(480),
            ..AttachmentInfo::test(Id::new(1), filename)
        }
    }
}

/// Gateway events, built the way the gateway would send them.
///
/// The point is fidelity: a fixture that omits a field the real payload
/// carries makes a front end look correct until it meets Discord.
///
/// Here rather than in `concord-fixtures` because this crate's own tests use
/// them, and a crate cannot depend on something that depends on it. That crate
/// re-exports these, so a front end still has one place to import from.
#[cfg(any(test, feature = "fixtures"))]
#[allow(
    clippy::new_without_default,
    reason = "a fixture's `new` takes the fields a test cares about; a Default \
              would invite tests to depend on values nothing chose"
)]
pub mod test_builders {
    use super::*;
    use crate::discord::ids::{Id, marker::*};

    // A glob because these build whole events and touch most of the event
    // vocabulary; naming each would be a list that grows with every fixture.
    use crate::discord::*;

    /// Message fixtures, as an extension trait.
    ///
    /// `MessageInfo` belongs to the core, so these cannot be inherent methods from
    /// out here. A trait keeps the call sites reading the same - and makes the
    /// import explicit, which is no bad thing for something that only ever builds
    /// fake data.
    pub type MessageCreateFixture = MessageInfo;

    impl MessageCreateFixture {
        pub fn test_fixture_default() -> Self {
            Self {
                channel_id: Id::new(2),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                message_kind: MessageKind::regular(),
                content: Some("hello".to_owned()),
                ..Self::default()
            }
        }

        pub fn direct_message(
            channel_id: Id<ChannelMarker>,
            message_id: Id<MessageMarker>,
        ) -> Self {
            Self {
                channel_id,
                message_id,
                ..Self::test_fixture_default()
            }
        }

        pub fn guild_message(
            guild_id: Id<GuildMarker>,
            channel_id: Id<ChannelMarker>,
            message_id: Id<MessageMarker>,
        ) -> Self {
            Self {
                guild_id: Some(guild_id),
                channel_id,
                message_id,
                ..Self::test_fixture_default()
            }
        }

        pub fn with_author_id(mut self, author_id: Id<UserMarker>) -> Self {
            self.author_id = author_id;
            self
        }

        pub fn with_author(mut self, author_id: Id<UserMarker>, author: impl Into<String>) -> Self {
            self.author_id = author_id;
            self.author = author.into();
            self
        }

        pub fn with_message_kind(mut self, message_kind: MessageKind) -> Self {
            self.message_kind = message_kind;
            self
        }

        pub fn with_reference(mut self, reference: MessageReferenceInfo) -> Self {
            self.reference = Some(reference);
            self
        }

        pub fn with_attachments(mut self, attachments: Vec<AttachmentInfo>) -> Self {
            self.attachments = attachments;
            self
        }

        pub fn with_content(mut self, content: impl Into<String>) -> Self {
            self.content = Some(content.into());
            self
        }
    }

    pub fn guild_message_create_fixture() -> MessageInfo {
        MessageInfo::guild_message(Id::new(1), Id::new(2), Id::new(1))
    }

    pub fn message_create_event(event: MessageInfo) -> AppEvent {
        AppEvent::MessageCreate { message: event }
    }

    use crate::discord::{
        ChannelInfo, CustomEmojiInfo, GuildBoostTier, GuildOnboardingInfo, MemberInfo,
        PresenceStatus, RoleInfo,
    };

    // Single construction seam for `AppEvent::GuildCreate` so a new field on the
    // variant only touches this fixture, not the ~20 test files that build the event.
    pub struct GuildCreateFixture {
        pub guild_id: Id<GuildMarker>,
        pub name: String,
        pub member_count: Option<u64>,
        pub owner_id: Option<Id<UserMarker>>,
        pub boost_tier: GuildBoostTier,
        pub boost_count: u32,
        pub verification_level: GuildVerificationLevel,
        pub mfa_level: u64,
        pub features: Vec<String>,
        pub onboarding: Option<GuildOnboardingInfo>,
        pub channels: Vec<ChannelInfo>,
        pub members: Vec<MemberInfo>,
        pub presences: Vec<(Id<UserMarker>, PresenceStatus)>,
        pub roles: Vec<RoleInfo>,
        pub emojis: Vec<CustomEmojiInfo>,
    }

    impl GuildCreateFixture {
        pub fn new(guild_id: Id<GuildMarker>) -> Self {
            Self {
                guild_id,
                name: "guild".to_owned(),
                member_count: None,
                owner_id: None,
                boost_tier: GuildBoostTier::None,
                boost_count: 0,
                verification_level: GuildVerificationLevel::None,
                mfa_level: 0,
                features: Vec::new(),
                onboarding: None,
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
                roles: Vec::new(),
                emojis: Vec::new(),
            }
        }
    }

    pub fn guild_create_event(event: GuildCreateFixture) -> AppEvent {
        AppEvent::GuildCreate {
            stickers: Vec::new(),
            guild_id: event.guild_id,
            name: event.name,
            member_count: event.member_count,
            owner_id: event.owner_id,
            boost_tier: event.boost_tier,
            boost_count: event.boost_count,
            verification_level: Some(event.verification_level),
            mfa_level: Some(event.mfa_level),
            features: Some(event.features),
            onboarding: event.onboarding,
            channels: event.channels,
            members: event.members,
            presences: event.presences,
            roles: Some(event.roles),
            emojis: event.emojis,
        }
    }

    pub struct ForumPostsLoadedFixture {
        pub channel_id: Id<ChannelMarker>,
        pub archive_state: ForumPostArchiveState,
        pub offset: usize,
        pub next_offset: usize,
        pub threads: Vec<ChannelInfo>,
        pub first_messages: Vec<MessageInfo>,
        pub has_more: bool,
    }

    impl ForumPostsLoadedFixture {
        pub fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                archive_state: ForumPostArchiveState::default(),
                offset: 0,
                next_offset: 0,
                threads: Vec::new(),
                first_messages: Vec::new(),
                has_more: false,
            }
        }
    }

    pub fn forum_posts_loaded_event(f: ForumPostsLoadedFixture) -> AppEvent {
        AppEvent::ForumPostsLoaded {
            channel_id: f.channel_id,
            archive_state: f.archive_state,
            offset: f.offset,
            next_offset: f.next_offset,
            threads: f.threads,
            first_messages: f.first_messages,
            has_more: f.has_more,
        }
    }

    pub struct MessageHistoryLoadedFixture {
        pub channel_id: Id<ChannelMarker>,
        pub before: Option<Id<MessageMarker>>,
        pub messages: Vec<MessageInfo>,
    }

    impl MessageHistoryLoadedFixture {
        pub fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                before: None,
                messages: Vec::new(),
            }
        }
    }

    pub fn message_history_loaded_event(f: MessageHistoryLoadedFixture) -> AppEvent {
        AppEvent::MessageHistoryLoaded {
            channel_id: f.channel_id,
            before: f.before,
            messages: f.messages,
        }
    }

    pub fn empty_latest_message_history_loaded_event(channel_id: Id<ChannelMarker>) -> AppEvent {
        message_history_loaded_event(MessageHistoryLoadedFixture {
            channel_id,
            ..MessageHistoryLoadedFixture::new()
        })
    }

    pub struct MessageHistoryLoadFailedFixture {
        pub channel_id: Id<ChannelMarker>,
        pub target: MessageHistoryLoadTarget,
        pub message: String,
    }
    pub fn message_history_load_failed_event(f: MessageHistoryLoadFailedFixture) -> AppEvent {
        AppEvent::MessageHistoryLoadFailed {
            channel_id: f.channel_id,
            target: f.target,
            message: f.message,
        }
    }

    pub struct TypingStartFixture {
        pub guild_id: Option<Id<GuildMarker>>,
        pub channel_id: Id<ChannelMarker>,
        pub user_id: Id<UserMarker>,
        pub member: Option<MemberInfo>,
    }

    impl TypingStartFixture {
        pub fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                user_id: Id::new(1),
                member: None,
            }
        }
    }

    pub fn typing_start_event(f: TypingStartFixture) -> AppEvent {
        AppEvent::TypingStart {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            user_id: f.user_id,
            member: f.member,
        }
    }

    pub struct VoiceSpeakingUpdateFixture {
        pub scope: VoiceScope,
        pub channel_id: Id<ChannelMarker>,
        pub user_id: Id<UserMarker>,
        pub speaking: bool,
    }
    pub fn voice_speaking_update_event(f: VoiceSpeakingUpdateFixture) -> AppEvent {
        AppEvent::VoiceSpeakingUpdate {
            scope: f.scope,
            channel_id: f.channel_id,
            user_id: f.user_id,
            speaking: f.speaking,
        }
    }

    pub struct MessageHistoryAfterLoadedFixture {
        pub channel_id: Id<ChannelMarker>,
        pub after: Id<MessageMarker>,
        pub messages: Vec<MessageInfo>,
        pub has_more: bool,
        pub mode: MessageHistoryAfterMode,
    }

    impl MessageHistoryAfterLoadedFixture {
        pub fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                after: Id::new(1),
                messages: Vec::new(),
                has_more: false,
                mode: MessageHistoryAfterMode::GapFill,
            }
        }
    }

    pub fn message_history_after_loaded_event(f: MessageHistoryAfterLoadedFixture) -> AppEvent {
        AppEvent::MessageHistoryAfterLoaded {
            channel_id: f.channel_id,
            after: f.after,
            messages: f.messages,
            has_more: f.has_more,
            mode: f.mode,
        }
    }

    pub struct VoiceConnectionStatusChangedFixture {
        pub scope: VoiceScope,
        pub channel_id: Option<Id<ChannelMarker>>,
        pub status: VoiceConnectionStatus,
        pub message: Option<String>,
    }

    impl VoiceConnectionStatusChangedFixture {
        pub fn new() -> Self {
            Self {
                scope: VoiceScope::Guild(Id::new(1)),
                channel_id: None,
                status: VoiceConnectionStatus::Connecting,
                message: None,
            }
        }
    }

    pub fn voice_connection_status_changed_event(
        f: VoiceConnectionStatusChangedFixture,
    ) -> AppEvent {
        AppEvent::VoiceConnectionStatusChanged {
            scope: f.scope,
            channel_id: f.channel_id,
            status: f.status,
            message: f.message,
        }
    }

    pub struct MessageReactionAddFixture {
        pub guild_id: Option<Id<GuildMarker>>,
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
        pub user_id: Id<UserMarker>,
        pub emoji: ReactionEmoji,
    }

    impl MessageReactionAddFixture {
        pub fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                message_id: Id::new(1),
                user_id: Id::new(1),
                emoji: ReactionEmoji::Unicode(String::new()),
            }
        }
    }

    pub fn message_reaction_add_event(f: MessageReactionAddFixture) -> AppEvent {
        AppEvent::MessageReactionAdd {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            message_id: f.message_id,
            user_id: f.user_id,
            emoji: f.emoji,
        }
    }

    pub struct ChannelPinsUpdateFixture {
        pub guild_id: Option<Id<GuildMarker>>,
        pub channel_id: Id<ChannelMarker>,
        pub last_pin_timestamp: Option<String>,
    }

    impl ChannelPinsUpdateFixture {
        pub fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                last_pin_timestamp: None,
            }
        }
    }

    pub fn channel_pins_update_event(f: ChannelPinsUpdateFixture) -> AppEvent {
        AppEvent::ChannelPinsUpdate {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            last_pin_timestamp: f.last_pin_timestamp,
        }
    }

    pub struct UserProfileLoadFailedFixture {
        pub user_id: Id<UserMarker>,
        pub guild_id: Option<Id<GuildMarker>>,
        pub message: String,
    }

    impl UserProfileLoadFailedFixture {
        pub fn new() -> Self {
            Self {
                user_id: Id::new(1),
                guild_id: None,
                message: String::new(),
            }
        }
    }

    pub fn user_profile_load_failed_event(f: UserProfileLoadFailedFixture) -> AppEvent {
        AppEvent::UserProfileLoadFailed {
            user_id: f.user_id,
            guild_id: f.guild_id,
            message: f.message,
        }
    }

    pub struct MessageAckFixture {
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
        pub mention_count: u32,
    }

    impl MessageAckFixture {
        pub fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                message_id: Id::new(1),
                mention_count: 0,
            }
        }
    }

    pub fn message_ack_event(f: MessageAckFixture) -> AppEvent {
        AppEvent::MessageAck {
            channel_id: f.channel_id,
            message_id: f.message_id,
            mention_count: Some(f.mention_count),
            flags: None,
            last_viewed: None,
            version: None,
        }
    }

    pub struct ReactionUsersLoadedFixture {
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
        pub emoji: ReactionEmoji,
        pub users: Vec<ReactionUserInfo>,
        pub next_after: Option<Id<UserMarker>>,
        pub after: Option<Id<UserMarker>>,
    }
    pub fn reaction_users_loaded_event(f: ReactionUsersLoadedFixture) -> AppEvent {
        AppEvent::ReactionUsersLoaded {
            channel_id: f.channel_id,
            message_id: f.message_id,
            emoji: f.emoji,
            users: f.users,
            next_after: f.next_after,
            after: f.after,
        }
    }

    pub struct CurrentUserPollVoteUpdateFixture {
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
        pub answer_ids: Vec<u8>,
    }

    impl CurrentUserPollVoteUpdateFixture {
        pub fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                message_id: Id::new(1),
                answer_ids: Vec::new(),
            }
        }
    }

    pub fn current_user_poll_vote_update_event(f: CurrentUserPollVoteUpdateFixture) -> AppEvent {
        AppEvent::CurrentUserPollVoteUpdate {
            channel_id: f.channel_id,
            message_id: f.message_id,
            answer_ids: f.answer_ids,
        }
    }

    pub struct UserIdentityUpdateFixture {
        pub user_id: Id<UserMarker>,
        pub username: String,
        pub global_name: Option<String>,
        pub avatar_url: Option<String>,
        pub is_bot: bool,
    }

    impl UserIdentityUpdateFixture {
        pub fn new() -> Self {
            Self {
                user_id: Id::new(1),
                username: String::new(),
                global_name: None,
                avatar_url: None,
                is_bot: false,
            }
        }
    }

    pub fn user_identity_update_event(f: UserIdentityUpdateFixture) -> AppEvent {
        AppEvent::UserIdentityUpdate {
            user_id: f.user_id,
            username: f.username,
            global_name: f.global_name,
            avatar_url: f.avatar_url,
            is_bot: f.is_bot,
        }
    }

    pub struct MessagePinnedUpdateFixture {
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
        pub pinned: bool,
    }

    impl MessagePinnedUpdateFixture {
        pub fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                message_id: Id::new(1),
                pinned: false,
            }
        }
    }

    pub fn message_pinned_update_event(f: MessagePinnedUpdateFixture) -> AppEvent {
        AppEvent::MessagePinnedUpdate {
            channel_id: f.channel_id,
            message_id: f.message_id,
            pinned: f.pinned,
        }
    }

    pub struct MessageHistoryAroundLoadedFixture {
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
        pub messages: Vec<MessageInfo>,
    }
    pub fn message_history_around_loaded_event(f: MessageHistoryAroundLoadedFixture) -> AppEvent {
        AppEvent::MessageHistoryAroundLoaded {
            channel_id: f.channel_id,
            message_id: f.message_id,
            messages: f.messages,
        }
    }

    pub struct CurrentUserReactionAddFixture {
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
        pub emoji: ReactionEmoji,
    }
    pub fn current_user_reaction_add_event(f: CurrentUserReactionAddFixture) -> AppEvent {
        AppEvent::CurrentUserReactionAdd {
            channel_id: f.channel_id,
            message_id: f.message_id,
            emoji: f.emoji,
        }
    }

    pub struct GuildUpdateFixture {
        pub guild_id: Id<GuildMarker>,
        pub name: String,
        pub owner_id: Option<Id<UserMarker>>,
        pub boost_tier: Option<GuildBoostTier>,
        pub boost_count: Option<u32>,
        pub verification_level: Option<GuildVerificationLevel>,
        pub mfa_level: Option<u64>,
        pub features: Option<Vec<String>>,
        pub onboarding: Option<GuildOnboardingInfo>,
        pub roles: Option<Vec<RoleInfo>>,
        pub emojis: Option<Vec<CustomEmojiInfo>>,
    }

    impl GuildUpdateFixture {
        pub fn new() -> Self {
            Self {
                guild_id: Id::new(1),
                name: String::new(),
                owner_id: None,
                boost_tier: None,
                boost_count: None,
                verification_level: None,
                mfa_level: None,
                features: None,
                onboarding: None,
                roles: None,
                emojis: None,
            }
        }
    }

    pub fn guild_update_event(f: GuildUpdateFixture) -> AppEvent {
        AppEvent::GuildUpdate {
            guild_id: f.guild_id,
            name: f.name,
            owner_id: f.owner_id,
            boost_tier: f.boost_tier,
            boost_count: f.boost_count,
            verification_level: f.verification_level,
            mfa_level: f.mfa_level,
            features: f.features,
            onboarding: f.onboarding,
            roles: f.roles,
            emojis: f.emojis,
        }
    }

    pub struct MessageReactionRemoveFixture {
        pub guild_id: Option<Id<GuildMarker>>,
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
        pub user_id: Id<UserMarker>,
        pub emoji: ReactionEmoji,
    }

    impl MessageReactionRemoveFixture {
        pub fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                message_id: Id::new(1),
                user_id: Id::new(1),
                emoji: ReactionEmoji::Unicode(String::new()),
            }
        }
    }

    pub fn message_reaction_remove_event(f: MessageReactionRemoveFixture) -> AppEvent {
        AppEvent::MessageReactionRemove {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            message_id: f.message_id,
            user_id: f.user_id,
            emoji: f.emoji,
        }
    }

    pub struct AttachmentDownloadStartedFixture {
        pub id: AttachmentDownloadId,
        pub filename: String,
        pub total_bytes: Option<u64>,
        pub source: DownloadAttachmentSource,
    }

    impl AttachmentDownloadStartedFixture {
        pub fn new() -> Self {
            Self {
                id: AttachmentDownloadId::new(0),
                filename: String::new(),
                total_bytes: None,
                source: DownloadAttachmentSource::AttachmentViewer,
            }
        }
    }

    pub fn attachment_download_started_event(f: AttachmentDownloadStartedFixture) -> AppEvent {
        AppEvent::AttachmentDownloadStarted {
            id: f.id,
            filename: f.filename,
            total_bytes: f.total_bytes,
            source: f.source,
        }
    }

    pub struct MessageReactionRemoveAllFixture {
        pub guild_id: Option<Id<GuildMarker>>,
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
    }

    impl MessageReactionRemoveAllFixture {
        pub fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                message_id: Id::new(1),
            }
        }
    }

    pub fn message_reaction_remove_all_event(f: MessageReactionRemoveAllFixture) -> AppEvent {
        AppEvent::MessageReactionRemoveAll {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            message_id: f.message_id,
        }
    }

    pub struct MessageDeleteBulkFixture {
        pub guild_id: Option<Id<GuildMarker>>,
        pub channel_id: Id<ChannelMarker>,
        pub message_ids: Vec<Id<MessageMarker>>,
    }
    pub fn message_delete_bulk_event(f: MessageDeleteBulkFixture) -> AppEvent {
        AppEvent::MessageDeleteBulk {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            message_ids: f.message_ids,
        }
    }

    pub struct CurrentUserReactionRemoveFixture {
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
        pub emoji: ReactionEmoji,
    }
    pub fn current_user_reaction_remove_event(f: CurrentUserReactionRemoveFixture) -> AppEvent {
        AppEvent::CurrentUserReactionRemove {
            channel_id: f.channel_id,
            message_id: f.message_id,
            emoji: f.emoji,
        }
    }

    pub struct AttachmentDownloadProgressFixture {
        pub id: AttachmentDownloadId,
        pub downloaded_bytes: u64,
        pub total_bytes: Option<u64>,
    }
    pub fn attachment_download_progress_event(f: AttachmentDownloadProgressFixture) -> AppEvent {
        AppEvent::AttachmentDownloadProgress {
            id: f.id,
            downloaded_bytes: f.downloaded_bytes,
            total_bytes: f.total_bytes,
        }
    }

    pub struct MessageReactionRemoveEmojiFixture {
        pub guild_id: Option<Id<GuildMarker>>,
        pub channel_id: Id<ChannelMarker>,
        pub message_id: Id<MessageMarker>,
        pub emoji: ReactionEmoji,
    }

    impl MessageReactionRemoveEmojiFixture {
        pub fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                message_id: Id::new(1),
                emoji: ReactionEmoji::Unicode(String::new()),
            }
        }
    }

    pub fn message_reaction_remove_emoji_event(f: MessageReactionRemoveEmojiFixture) -> AppEvent {
        AppEvent::MessageReactionRemoveEmoji {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            message_id: f.message_id,
            emoji: f.emoji,
        }
    }

    pub struct ForumPostsLoadFailedFixture {
        pub channel_id: Id<ChannelMarker>,
        pub archive_state: ForumPostArchiveState,
        pub offset: usize,
        pub message: String,
    }

    impl ForumPostsLoadFailedFixture {
        pub fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                archive_state: ForumPostArchiveState::default(),
                offset: 0,
                message: String::new(),
            }
        }
    }

    pub fn forum_posts_load_failed_event(f: ForumPostsLoadFailedFixture) -> AppEvent {
        AppEvent::ForumPostsLoadFailed {
            channel_id: f.channel_id,
            archive_state: f.archive_state,
            offset: f.offset,
            message: f.message,
        }
    }

    pub struct AttachmentDownloadFailedFixture {
        pub id: AttachmentDownloadId,
        pub filename: String,
        pub message: String,
        pub source: DownloadAttachmentSource,
    }
    pub fn attachment_download_failed_event(f: AttachmentDownloadFailedFixture) -> AppEvent {
        AppEvent::AttachmentDownloadFailed {
            id: f.id,
            filename: f.filename,
            message: f.message,
            source: f.source,
        }
    }
    pub struct AttachmentDownloadCompletedFixture {
        pub id: AttachmentDownloadId,
        pub path: String,
        pub source: DownloadAttachmentSource,
    }
    pub fn attachment_download_completed_event(f: AttachmentDownloadCompletedFixture) -> AppEvent {
        AppEvent::AttachmentDownloadCompleted {
            id: f.id,
            path: f.path,
            source: f.source,
        }
    }
}
