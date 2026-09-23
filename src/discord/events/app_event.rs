use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::discord::commands::{
    AttachmentDownloadId, DownloadAttachmentSource, MediaPlaybackRequestId,
    MessageHistoryAfterMode, MessageSearchPage, MessageSearchQuery, ReactionEmoji,
    StreamCaptureTargetsRequestId,
};

use crate::discord::{
    ActivityInfo, ApplicationCommandChoiceInfo, ApplicationCommandInfo, ChannelInfo,
    ChannelRecipientInfo, CustomEmojiInfo, GuildBoostTier, GuildOnboardingInfo,
    GuildVerificationLevel, MemberInfo, MessageInfo, PremiumTier, ReactionUserInfo, ReadStateInfo,
    RelationshipInfo, RoleInfo, StreamCaptureTarget, StreamCreateInfo, StreamDeleteInfo,
    StreamServerInfo, StreamUpdateInfo, UserProfileInfo, UserSettingsInfo, VoiceConnectionStatus,
    VoiceScope, VoiceServerInfo, VoiceSoundKind, VoiceStateInfo,
};

use super::types::{
    ChannelUnreadInfo, GatewayDispatchInfo, GuildMemberListUpdateInfo, GuildMembersChunkInfo,
    MessageHistoryLoadTarget, MessageUpdateDispatchInfo, PresenceEventFields, ReadySnapshotInfo,
    UserGuildSettingsInfo,
};
// Upstream moved every thread type into its own module in v2.5.10; the copies
// that used to live in events::types were the pre-move ones.
use crate::discord::profile::FriendStatus;
use crate::discord::thread::{
    ArchivedThreadsPage, ForumPostDataInfo, ThreadGatewayInfo, ThreadListSyncInfo,
    ThreadMemberInfo, ThreadMemberListUpdateInfo, ThreadMembersUpdateInfo,
};

// Suppress the unused import warning: `SnapshotAreas` is only referenced inside
// the macro-generated `impl AppEventKind` body in metadata.rs, not here.
#[allow(unused_imports)]
use crate::discord::SnapshotAreas as _;

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
        /// Whether the Gateway payload contained the guild's `threads` array.
        /// An empty array clears the snapshot, while an omitted field in
        /// `CLIENT_STATE_V2` partial mode must preserve cached thread state.
        thread_snapshot_complete: bool,
        current_user_thread_members: Vec<ThreadMemberInfo>,
        members: Vec<MemberInfo>,
        presences: Vec<PresenceEventFields>,
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
        role_id: crate::discord::ids::Id<crate::discord::ids::marker::RoleMarker>,
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
    ThreadUpsert {
        thread: ThreadGatewayInfo,
        created: bool,
    },
    ThreadListSync {
        sync: ThreadListSyncInfo,
    },
    ThreadMembersUpdateDispatch {
        update: ThreadMembersUpdateInfo,
    },
    ThreadMemberListUpdate {
        update: ThreadMemberListUpdateInfo,
    },
    ThreadMemberUpdate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        member: ThreadMemberInfo,
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
    ForumPostDataLoaded {
        channel_id: Id<ChannelMarker>,
        requested_thread_ids: Vec<Id<ChannelMarker>>,
        posts: Vec<ForumPostDataInfo>,
    },
    ForumPostDataLoadFailed {
        channel_id: Id<ChannelMarker>,
        thread_ids: Vec<Id<ChannelMarker>>,
        message: String,
    },
    ArchivedThreadsLoaded {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        before: Option<String>,
        page: ArchivedThreadsPage,
    },
    ArchivedThreadsLoadFailed {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        before: Option<String>,
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
    /// An embed proxy answered, and the link turned out to be describable.
    EmbedResolved {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        embed: Box<crate::discord::EmbedInfo>,
    },
    /// The proxy could not say what the link is. Reported rather than
    /// swallowed: a preview that never appears is indistinguishable from one
    /// still loading, and a misconfigured proxy would look like a slow one.
    EmbedResolveFailed {
        url: String,
        message: String,
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
        update: crate::discord::RelationshipUpdateInfo,
    },
    RelationshipRemove {
        user_id: Id<UserMarker>,
        status: Option<FriendStatus>,
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
        selected_time_window: Option<i64>,
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
    ThreadUpsert: AppEvent::ThreadUpsert { .. },
    ThreadListSync: AppEvent::ThreadListSync { .. },
    ThreadMembersUpdateDispatch: AppEvent::ThreadMembersUpdateDispatch { .. },
    ThreadMemberListUpdate: AppEvent::ThreadMemberListUpdate { .. },
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
    ForumPostDataLoaded: AppEvent::ForumPostDataLoaded { .. },
    ForumPostDataLoadFailed: AppEvent::ForumPostDataLoadFailed { .. },
    ArchivedThreadsLoaded: AppEvent::ArchivedThreadsLoaded { .. },
    ArchivedThreadsLoadFailed: AppEvent::ArchivedThreadsLoadFailed { .. },
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
    EmbedResolved: AppEvent::EmbedResolved { .. },
    EmbedResolveFailed: AppEvent::EmbedResolveFailed { .. },
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
