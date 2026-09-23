//! Gateway events, built the way the gateway would send them.
//!
//! The point is fidelity: a fixture that omits a field the real payload
//! carries makes a front end look correct until it meets Discord.
//!
//! Here rather than in `concord-fixtures` because this crate's own tests use
//! them, and a crate cannot depend on something that depends on it. That crate
//! re-exports these, so a front end still has one place to import from.

// Both of these were written as outer attributes on the `use` below, where a
// doc comment documents the import and an allow covers nothing - which is why
// twenty-two new_without_default warnings came through anyway.
#![allow(
    clippy::new_without_default,
    reason = "a fixture's `new` takes the fields a test cares about; a Default \
              would invite tests to depend on values nothing chose"
)]

use crate::discord::ids::{Id, marker::*};

// A glob because these build whole events and touch most of the event
// vocabulary; naming each would be a list that grows with every fixture.
use crate::discord::*;

use super::app_event::AppEvent;
use super::types::MessageHistoryLoadTarget;
use crate::discord::commands::{
    AttachmentDownloadId, DownloadAttachmentSource, MessageHistoryAfterMode, ReactionEmoji,
};

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

    pub fn direct_message(channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker>) -> Self {
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
    pub thread_snapshot_complete: bool,
    pub current_user_thread_members: Vec<ThreadMemberInfo>,
    pub presences: Vec<PresenceEventFields>,
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
            thread_snapshot_complete: true,
            current_user_thread_members: Vec::new(),
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
        thread_snapshot_complete: event.thread_snapshot_complete,
        current_user_thread_members: event.current_user_thread_members,
        presences: event.presences,
        roles: Some(event.roles),
        emojis: event.emojis,
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

pub fn voice_connection_status_changed_event(f: VoiceConnectionStatusChangedFixture) -> AppEvent {
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
