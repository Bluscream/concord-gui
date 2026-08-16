mod account_form;
mod action_policy;
mod application_commands;
pub mod auth_http;
mod avatar;
mod builtin_commands;
mod capabilities;
mod captcha;
mod channel;
mod client;
mod commands;
mod display_name;
mod emoji;
mod events;
mod fingerprint;
#[cfg(feature = "fixtures")]
pub mod fixtures;
mod gateway;
mod guild;
pub mod ids;
mod json;
mod member;
mod message;
mod message_policy;
mod notification;
pub mod password_auth;
mod permission;
pub mod permissions_catalogue;
mod presence;
mod profile;
pub mod qr_auth;
mod read;
mod request_lifecycle;
mod rest;
mod rpc;
mod secret;
pub(in crate::discord) mod state;
mod totp;
pub(crate) mod upload;
mod user_settings;
mod verification;
mod voice;

pub(crate) use action_policy::ActionDecision;
pub use action_policy::{ActionBlockReason, DiscordAction};
pub use application_commands::{
    APPLICATION_COMMAND_CHANNEL_KIND, APPLICATION_COMMAND_MENTIONABLE_KIND,
    APPLICATION_COMMAND_ROLE_KIND, APPLICATION_COMMAND_STRING_KIND, APPLICATION_COMMAND_USER_KIND,
    ApplicationCommandAutocompleteInvocation, ApplicationCommandChoiceInfo,
    ApplicationCommandIdentity, ApplicationCommandInfo, ApplicationCommandInteraction,
    ApplicationCommandInteractionOption, ApplicationCommandInvocation,
    ApplicationCommandOptionInfo, application_command_content_is_complete,
    application_command_option_scope, parsed_application_command_option_names,
};
// Public so that out-of-crate front-ends can drive the login flows and pass
// the resulting session to `app::Session::start`.
pub use account_form::{AccountField, AccountForm, AccountFormProblem};
pub use auth_http::DiscordAuthSession;
pub use avatar::still_avatar_url;
pub use builtin_commands::{
    BuiltinSlashCommandInfo, BuiltinSlashCommandParse, BuiltinSlashCommandSubmit,
    builtin_slash_commands, parse_builtin_slash_command,
};
pub(crate) use capabilities::MessageSendLimits;
pub use capabilities::{
    BASE_ATTACHMENT_LIMIT_BYTES, GuildBoostTier, PremiumTier, effective_attachment_limit_bytes,
};
#[cfg(test)]
pub(crate) use capabilities::{BASE_MESSAGE_CHARACTER_LIMIT, NITRO_MESSAGE_CHARACTER_LIMIT};
pub(crate) use channel::is_thread_kind;
pub use channel::{
    ChannelInfo, ChannelRecipientInfo, ForumTagInfo, PermissionOverwriteInfo,
    PermissionOverwriteKind, ThreadMetadataInfo,
};
pub use client::DiscordClient;
pub(crate) use client::validate_token_header;
pub use commands::next_message_nonce;
pub use commands::{
    AppCommand, AttachmentDownloadId, DownloadAttachmentSource, ForumPostArchiveState,
    ForumPostCreate, GlobalUserProfileUpdate, GuildUserProfileUpdate, MediaPlaybackRequestId,
    MediaPlaybackSource, MediaPlaybackTarget, MessageHistoryAfterMode, MessageSearchAuthorType,
    MessageSearchHas, MessageSearchPage, MessageSearchQuery, MuteDuration, ProfileAvatarUpload,
    ReplyReference, StreamCaptureTargetsRequestId, UserProfileUpdate,
};
pub use commands::{
    MAX_PROFILE_AVATAR_BYTES, MAX_UPLOAD_ATTACHMENT_COUNT, MAX_UPLOAD_PREVIEW_BYTES,
    MessageAttachmentUpload, ReactionEmoji,
};
pub use emoji::custom_emoji_image_url;
#[cfg(test)]
pub(crate) use events::test_builders;
pub use events::{
    AppEvent, GatewayDispatchInfo, GuildMemberListItem, GuildMemberListOperation,
    GuildMemberListUpdateInfo, GuildMembersChunkInfo, MessageHistoryLoadTarget,
    MessageUpdateDispatchInfo, MessageUpdateEventFields, PresenceEventFields, ReadySnapshotInfo,
    SequencedAppEvent, ThreadListSyncInfo, ThreadMemberUpdateInfo, ThreadMembersUpdateInfo,
    UserGuildSettingsInfo,
};
pub(crate) use fingerprint::load_client_fingerprint_and_http;
pub use guild::{
    CustomEmojiInfo, GuildFolder, GuildOnboardingInfo, GuildOnboardingMode, GuildVerificationLevel,
};
pub use ids::{Id, marker};
pub use member::{MemberInfo, MemberOnboardingStatus, RoleInfo};
pub use message::{
    AttachmentInfo, AttachmentMediaType, AttachmentUpdate, EmbedFieldInfo, EmbedInfo,
    InlinePreviewInfo, MESSAGE_FLAG_SUPPRESS_EMBEDS, MentionInfo, MessageInfo,
    MessageInteractionInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo, PollAnswerInfo,
    PollInfo, ReactionInfo, ReactionUserInfo, ReplyInfo, StickerFormat, StickerInfo,
};
pub(crate) use message_policy::{
    validate_attachment_sizes, validate_message_content, validate_message_content_length,
    validate_message_payload,
};
pub use notification::{
    ChannelNotificationOverrideInfo, GuildNotificationSettingsInfo, NotificationLevel,
};
pub(crate) use permission::PermissionDecision;
pub use permission::{DiscordPermission, PermissionDataGap};
pub use presence::{
    ActivityAssets, ActivityButton, ActivityEmoji, ActivityInfo, ActivityKind, ActivityParty,
    ActivityTimestamps, PresenceStatus,
};
pub use profile::{
    FriendStatus, MutualGuildInfo, RelationshipInfo, RelationshipUpdateInfo, UserProfileInfo,
};
pub use read::ReadStateInfo;
pub(crate) use request_lifecycle::GuildMemberSearchSurface;
pub use rest::{
    AFK_TIMEOUTS, AccountEdit, AuditLogAction, AuditLogEntryInfo, AuthSession, AuthorisedApp,
    AutoModAction, AutoModRule, AutoModTrigger, BackupCode, ChannelEdit, Connection,
    ConnectionVisibility, DefaultNotifications, DmScanLevel, ExplicitContentFilter, ForumPostPage,
    FriendDiscovery, FriendSources, GuildBanInfo, GuildEdit, GuildEmojiInfo, GuildInviteInfo,
    InvitePreview, MAX_BAN_DELETE_MESSAGE_SECONDS, MAX_CHANNEL_NAME_CHARS, MAX_CHANNEL_TOPIC_CHARS,
    MAX_EMOJI_BYTES, MAX_GUILD_NAME_CHARS, MAX_INVITE_MAX_AGE_SECONDS, MAX_INVITE_MAX_USES,
    MAX_MESSAGE_STICKERS, MAX_ROLE_NAME_CHARS, MAX_SLOWMODE_SECONDS, MAX_SOUND_NAME_CHARS,
    MAX_USERNAME_CHARS, MAX_VOICE_USER_LIMIT, MIN_GUILD_NAME_CHARS, MIN_PASSWORD_CHARS,
    MIN_SOUND_NAME_CHARS, MIN_USERNAME_CHARS, NewChannelKind, OverwriteTarget, PrivacyEdit,
    PrivacySetting, PrivacyState, ReactionUsersPage, RoleEdit, SoundboardSound,
    clamp_invite_max_age, clamp_invite_max_uses, emoji_name_from_filename, friend_request_target,
    invite_code_from, is_valid_emoji_name, is_valid_guild_name, is_valid_sound_name,
    nearest_afk_timeout, password_problem, username_problem, verification_code, verification_label,
};
pub use secret::Secret;
pub use state::{
    ChannelRecipientState, ChannelState, ChannelUnreadState, ChannelVisibilityStats,
    CurrentVoiceConnectionState, DiscordSnapshot, DiscordState, GuildMemberListEntry,
    GuildMemberState, GuildState, MessageCapabilities, MessageState, RoleState, SnapshotAreas,
    SnapshotRevision, TypingUserState, VoiceParticipantState,
};
pub use totp::TotpSecret;
pub(crate) use upload::read_profile_avatar_image;
pub use user_settings::{UserCustomStatusInfo, UserFriendSourceFlagsInfo, UserSettingsInfo};
pub(crate) use verification::GuildParticipationDecision;
pub use verification::{
    GuildParticipationBlock, GuildParticipationDataGap, GuildParticipationRestriction,
};
pub use voice::{
    MicrophoneSensitivityDb, VoiceAudioSettings, VoiceParticipantPlaybackSettings,
    VoiceParticipantVolumePercent, VoiceVolumePercent,
};
pub use voice::{
    StreamCaptureTarget, StreamCaptureTargetKind, StreamCreateInfo, StreamDeleteInfo,
    StreamServerInfo, StreamUpdateInfo, VoiceConnectionStatus, VoiceScope, VoiceServerInfo,
    VoiceSoundKind, VoiceStateInfo,
};
pub(crate) use voice::{
    VoiceAudioSourceOptions, VoiceAudioSources, list_stream_capture_targets,
    list_voice_audio_sources,
};
