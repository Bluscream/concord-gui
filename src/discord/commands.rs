use std::{
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::discord::ids::{
    Id,
    marker::{
        ChannelMarker, EmojiMarker, ForumTagMarker, GuildMarker, MessageMarker, RoleMarker,
        StickerMarker, UserMarker,
    },
};

use super::application_commands::{
    ApplicationCommandAutocompleteInvocation, ApplicationCommandInvocation,
};
use super::emoji::custom_emoji_image_url;
use super::message::MessageInfo;
use super::{
    ActivityInfo, ChannelEdit, GuildEdit, NewChannelKind, OverwriteTarget, PresenceStatus,
    RoleEdit, VoiceScope,
};

pub const MAX_UPLOAD_ATTACHMENT_COUNT: usize = 10;
pub const MAX_PROFILE_AVATAR_BYTES: u64 = 10 * 1024 * 1024;

/// Memory bound for decoding a local attachment preview thumbnail, kept
/// separate from the upload limit (now up to 500 MiB) so a preview of a huge
/// file is skipped rather than loaded into RAM. The upload still proceeds.
pub const MAX_UPLOAD_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;

/// Generates a unique snowflake-shaped nonce before a message enters the
/// asynchronous send pipeline. The TUI and Discord request share this value,
/// which lets the local pending row match the later `MESSAGE_CREATE` event.
/// Public so out-of-crate front-ends can construct `AppCommand::SendMessage`,
/// which requires a client-generated nonce for echo suppression.
pub fn next_message_nonce() -> Id<MessageMarker> {
    const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
    static LAST_NONCE: AtomicU64 = AtomicU64::new(0);

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(DISCORD_EPOCH_MS);
    let time_candidate = now_ms.saturating_sub(DISCORD_EPOCH_MS) << 22;

    let mut previous = LAST_NONCE.load(Ordering::Relaxed);
    loop {
        let next = time_candidate.max(previous.saturating_add(1)).max(1);
        match LAST_NONCE.compare_exchange_weak(previous, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return Id::new(next),
            Err(actual) => previous = actual,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AttachmentDownloadId(u64);

impl AttachmentDownloadId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageAttachmentUpload {
    source: UploadSource,
    pub filename: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForumPostCreate {
    pub channel_id: Id<ChannelMarker>,
    pub title: String,
    pub content: String,
    pub applied_tags: Vec<Id<ForumTagMarker>>,
    pub attachments: Vec<MessageAttachmentUpload>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GlobalUserProfileUpdate {
    pub display_name: Option<String>,
    pub pronouns: Option<String>,
    pub avatar: Option<ProfileAvatarUpload>,
}

impl GlobalUserProfileUpdate {
    pub fn is_empty(&self) -> bool {
        self.display_name.is_none() && self.pronouns.is_none() && self.avatar.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileAvatarUpload {
    source: UploadSource,
    pub filename: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum UploadSource {
    File(PathBuf),
    Bytes(Vec<u8>),
}

impl UploadSource {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::File(path) => Some(path),
            Self::Bytes(_) => None,
        }
    }

    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::File(_) => None,
            Self::Bytes(bytes) => Some(bytes),
        }
    }
}

impl ProfileAvatarUpload {
    pub fn from_path(path: PathBuf) -> Self {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("avatar")
            .to_owned();
        Self {
            source: UploadSource::File(path),
            filename,
            size_bytes: 0,
        }
    }

    pub fn from_bytes(filename: String, bytes: Vec<u8>) -> Self {
        Self {
            size_bytes: bytes.len() as u64,
            source: UploadSource::Bytes(bytes),
            filename,
        }
    }

    pub fn from_message_attachment(upload: MessageAttachmentUpload) -> Self {
        Self {
            source: upload.source,
            filename: upload.filename,
            size_bytes: upload.size_bytes,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.source.path()
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        self.source.bytes()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildUserProfileUpdate {
    pub guild_id: Id<GuildMarker>,
    pub nickname: Option<String>,
    pub pronouns: Option<String>,
    /// A separate avatar for this guild. Distinct from the global one, which
    /// is the point of a per-guild identity.
    pub avatar: Option<ProfileAvatarUpload>,
    pub bio: Option<String>,
}

impl GuildUserProfileUpdate {
    pub fn is_empty(&self) -> bool {
        self.nickname.is_none()
            && self.pronouns.is_none()
            && self.avatar.is_none()
            && self.bio.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserProfileUpdate {
    pub user_id: Id<UserMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub global: GlobalUserProfileUpdate,
    pub guild: Option<GuildUserProfileUpdate>,
}

impl UserProfileUpdate {
    pub fn is_empty(&self) -> bool {
        self.global.is_empty()
            && self
                .guild
                .as_ref()
                .is_none_or(GuildUserProfileUpdate::is_empty)
    }
}

impl MessageAttachmentUpload {
    pub fn from_path(path: PathBuf, filename: String, size_bytes: u64) -> Self {
        Self {
            source: UploadSource::File(path),
            filename,
            size_bytes,
        }
    }

    pub fn from_existing_path(path: PathBuf) -> io::Result<Self> {
        let metadata = path.metadata()?;
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment")
            .to_owned();
        Ok(Self::from_path(path, filename, metadata.len()))
    }

    pub fn from_bytes(filename: String, bytes: Vec<u8>) -> Self {
        Self {
            size_bytes: bytes.len() as u64,
            source: UploadSource::Bytes(bytes),
            filename,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.source.path()
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        self.source.bytes()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReactionEmoji {
    Unicode(String),
    Custom {
        id: Id<EmojiMarker>,
        name: Option<String>,
        animated: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ForumPostArchiveState {
    #[default]
    Active,
    Archived,
}

impl ForumPostArchiveState {
    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::Active => "false",
            Self::Archived => "true",
        }
    }

    pub fn as_log_label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MuteDuration {
    Minutes(u64),
    Permanent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageSearchHas {
    Link,
    Embed,
    File,
    Video,
    Image,
    Sound,
    Sticker,
}

impl MessageSearchHas {
    pub fn from_input(value: &str) -> Option<Self> {
        match normalized_search_token(value).as_str() {
            "link" | "links" => Some(Self::Link),
            "embed" | "embeds" => Some(Self::Embed),
            "file" | "files" | "attachment" | "attachments" => Some(Self::File),
            "video" | "videos" => Some(Self::Video),
            "image" | "images" | "img" => Some(Self::Image),
            "sound" | "sounds" | "audio" => Some(Self::Sound),
            "sticker" | "stickers" => Some(Self::Sticker),
            _ => None,
        }
    }

    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Embed => "embed",
            Self::File => "file",
            Self::Video => "video",
            Self::Image => "image",
            Self::Sound => "sound",
            Self::Sticker => "sticker",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageSearchAuthorType {
    User,
    Bot,
    Webhook,
}

impl MessageSearchAuthorType {
    pub fn from_input(value: &str) -> Option<Self> {
        match normalized_search_token(value).as_str() {
            "user" | "person" | "people" => Some(Self::User),
            "bot" | "bots" => Some(Self::Bot),
            "webhook" | "webhooks" => Some(Self::Webhook),
            _ => None,
        }
    }

    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Bot => "bot",
            Self::Webhook => "webhook",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MessageSearchQuery {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub author_id: Option<Id<UserMarker>>,
    pub mentions_user_id: Option<Id<UserMarker>>,
    pub content: Option<String>,
    pub has: Vec<MessageSearchHas>,
    pub date: Option<String>,
    pub author_type: Vec<MessageSearchAuthorType>,
    pub pinned: Option<bool>,
    pub offset: usize,
}

impl MessageSearchQuery {
    pub fn is_empty(&self) -> bool {
        self.channel_id.is_none()
            && self.author_id.is_none()
            && self.mentions_user_id.is_none()
            && self.content.as_deref().is_none_or(str::is_empty)
            && self.has.is_empty()
            && self.date.as_deref().is_none_or(str::is_empty)
            && self.author_type.is_empty()
            && self.pinned.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageSearchPage {
    pub query: MessageSearchQuery,
    pub messages: Vec<MessageInfo>,
    pub total_results: Option<usize>,
    pub has_more: bool,
}

impl MuteDuration {
    pub fn minutes(self) -> Option<u64> {
        match self {
            Self::Minutes(minutes) => Some(minutes),
            Self::Permanent => None,
        }
    }

    pub fn selected_time_window_seconds(self) -> i64 {
        match self {
            Self::Minutes(minutes) => i64::try_from(minutes.saturating_mul(60)).unwrap_or(i64::MAX),
            Self::Permanent => -1,
        }
    }
}

impl ReactionEmoji {
    pub fn status_label(&self) -> String {
        match self {
            Self::Unicode(emoji) => emoji.clone(),
            Self::Custom { name, .. } => name
                .as_deref()
                .map(|name| format!(":{name}:"))
                .unwrap_or_else(|| ":custom:".to_owned()),
        }
    }

    pub fn custom_image_url(&self) -> Option<String> {
        let Self::Custom { id, animated, .. } = self else {
            return None;
        };
        Some(custom_emoji_image_url(id.get(), *animated))
    }

    pub(crate) fn route_component(&self) -> String {
        match self {
            Self::Unicode(name) => percent_encode_path_segment(name),
            Self::Custom { id, name, .. } => percent_encode_path_segment(&format!(
                "{}:{id}",
                name.as_deref().unwrap_or_default()
            )),
        }
    }
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageHistoryAfterMode {
    GapFill,
    CatchUp,
}

impl MessageHistoryAfterMode {
    pub(crate) fn exhausts_on_empty(self) -> bool {
        matches!(self, Self::GapFill)
    }

    pub(crate) fn is_catch_up(self) -> bool {
        matches!(self, Self::CatchUp)
    }
}

/// A reply target paired with whether it should ping the referenced author.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyReference {
    pub message_id: Id<MessageMarker>,
    pub mention_author: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    SignOut,
    LoadMessageHistory {
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
    },
    RefreshMessageHistory {
        channel_id: Id<ChannelMarker>,
    },
    LoadMessageHistoryAfter {
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
        mode: MessageHistoryAfterMode,
    },
    LoadMessageHistoryAround {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    LoadThreadPreview {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    LoadForumPosts {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    },
    SearchMessages {
        query: MessageSearchQuery,
    },
    LoadGuildMembersByIds {
        guild_id: Id<GuildMarker>,
        user_ids: Vec<Id<UserMarker>>,
    },
    SearchGuildMembers {
        guild_id: Id<GuildMarker>,
        query: String,
        limit: u16,
    },
    SetSelectedGuild {
        guild_id: Option<Id<GuildMarker>>,
    },
    LeaveGuild {
        guild_id: Id<GuildMarker>,
        label: String,
    },
    /// Forward a message into another channel.
    ForwardMessage {
        source_channel_id: Id<ChannelMarker>,
        source_guild_id: Option<Id<GuildMarker>>,
        message_id: Id<MessageMarker>,
        target_channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
    },
    /// Remove a member from a guild.
    KickMember {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        label: String,
    },
    /// Ban a member, optionally purging their recent messages.
    BanMember {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        delete_message_seconds: u32,
        label: String,
    },
    /// Fetch a guild's ban list, so bans can be reviewed and lifted.
    LoadGuildBans {
        guild_id: Id<GuildMarker>,
    },
    /// Really remove a departed guild's cached conversation.
    ///
    /// Distinct from leaving, which only stops membership: rule 7 keeps the
    /// data until the user says otherwise, and this is them saying so.
    ForgetGuild {
        guild_id: Id<GuildMarker>,
        label: String,
    },
    /// Fetch every linked account.
    LoadConnections,
    /// Fetch every session signed in to this account.
    LoadAuthSessions,
    /// Log other sessions out.
    ///
    /// The password is the user's to type; it is carried for the one request
    /// that needs it and never stored. `Secret` keeps it out of any `{:?}`.
    RevokeAuthSessions {
        id_hashes: Vec<String>,
        password: crate::discord::Secret,
    },
    LoadAuthorisedApps,
    /// How many members a prune would remove. Always asked before pruning:
    /// the count is what makes an irreversible action reviewable beforehand.
    LoadPruneCount {
        guild_id: Id<GuildMarker>,
        days: u16,
        include_roles: Vec<Id<RoleMarker>>,
    },
    PruneGuild {
        guild_id: Id<GuildMarker>,
        days: u16,
        include_roles: Vec<Id<RoleMarker>>,
        label: String,
    },
    LoadScheduledEvents {
        guild_id: Id<GuildMarker>,
    },
    /// A status change rather than a delete: Discord keeps a cancelled event
    /// visible so people who said they were coming can see it is off.
    CancelScheduledEvent {
        guild_id: Id<GuildMarker>,
        event_id: u64,
        label: String,
    },
    DeleteScheduledEvent {
        guild_id: Id<GuildMarker>,
        event_id: u64,
        label: String,
    },
    SetEventInterest {
        guild_id: Id<GuildMarker>,
        event_id: u64,
        interested: bool,
    },
    /// The live stage in this channel, if one is running.
    LoadStageInstance {
        channel_id: Id<ChannelMarker>,
    },
    StartStageInstance {
        channel_id: Id<ChannelMarker>,
        topic: String,
    },
    ModifyStageTopic {
        channel_id: Id<ChannelMarker>,
        topic: String,
    },
    EndStageInstance {
        channel_id: Id<ChannelMarker>,
        label: String,
    },
    /// Raise or lower your hand in a stage.
    RequestToSpeak {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        requesting: bool,
    },
    /// Invite someone in the audience to speak, or move them back down.
    SetStageSpeaker {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        speaking: bool,
        label: String,
    },
    CreateScheduledEvent {
        guild_id: Id<GuildMarker>,
        event: Box<crate::discord::NewEvent>,
    },
    LoadGuildTemplates {
        guild_id: Id<GuildMarker>,
    },
    CreateGuildTemplate {
        guild_id: Id<GuildMarker>,
        name: String,
    },
    /// Bring a template up to date with the server as it stands.
    SyncGuildTemplate {
        guild_id: Id<GuildMarker>,
        code: String,
        label: String,
    },
    DeleteGuildTemplate {
        guild_id: Id<GuildMarker>,
        code: String,
        label: String,
    },
    LoadWelcomeScreen {
        guild_id: Id<GuildMarker>,
    },
    ModifyWelcomeScreen {
        guild_id: Id<GuildMarker>,
        edit: crate::discord::WelcomeScreenEdit,
    },
    LoadGuildWidget {
        guild_id: Id<GuildMarker>,
    },
    ModifyGuildWidget {
        guild_id: Id<GuildMarker>,
        widget: crate::discord::GuildWidget,
    },
    /// Change username, email or password. All of it is what the user typed;
    /// none of it is stored.
    ModifyAccount {
        edit: crate::discord::AccountEdit,
        current_password: crate::discord::Secret,
    },
    /// Turn on two-factor authentication with a locally generated secret.
    EnableTotp {
        secret: String,
        code: String,
        password: crate::discord::Secret,
    },
    /// Turn it off. A code rather than a password, which is Discord's rule.
    DisableTotp {
        code: String,
    },
    /// Fetch the backup codes, or regenerate them - which invalidates the old
    /// ones, so it is a flag rather than something the fetch does on its own.
    LoadBackupCodes {
        password: crate::discord::Secret,
        regenerate: bool,
    },
    RevokeAuthorisedApp {
        id: String,
        label: String,
    },
    /// Change what a connection shows on your profile.
    ModifyConnection {
        kind: String,
        id: String,
        visibility: crate::discord::ConnectionVisibility,
        show_activity: bool,
        label: String,
    },
    /// Change privacy and safety. Only the named fields are sent.
    ModifyPrivacySettings {
        edit: crate::discord::PrivacyEdit,
    },
    DeleteConnection {
        kind: String,
        id: String,
        label: String,
    },
    LoadAutoModRules {
        guild_id: Id<GuildMarker>,
    },
    SetAutoModRuleEnabled {
        guild_id: Id<GuildMarker>,
        rule_id: u64,
        enabled: bool,
        label: String,
    },
    DeleteAutoModRule {
        guild_id: Id<GuildMarker>,
        rule_id: u64,
        label: String,
    },
    ModifyGuild {
        guild_id: Id<GuildMarker>,
        edit: Box<GuildEdit>,
        label: String,
    },
    SetGuildIcon {
        guild_id: Id<GuildMarker>,
        image: Box<ProfileAvatarUpload>,
        label: String,
    },
    CreateRole {
        guild_id: Id<GuildMarker>,
        name: String,
    },
    ModifyRole {
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
        edit: Box<RoleEdit>,
        label: String,
    },
    DeleteRole {
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
        label: String,
    },
    ReorderRoles {
        guild_id: Id<GuildMarker>,
        positions: Vec<(Id<RoleMarker>, u32)>,
    },
    CreateGuildChannel {
        guild_id: Id<GuildMarker>,
        name: String,
        kind: NewChannelKind,
        parent_id: Option<Id<ChannelMarker>>,
    },
    ModifyChannel {
        channel_id: Id<ChannelMarker>,
        edit: Box<ChannelEdit>,
        label: String,
    },
    DeleteChannel {
        channel_id: Id<ChannelMarker>,
        label: String,
    },
    ReorderChannels {
        guild_id: Id<GuildMarker>,
        positions: Vec<(Id<ChannelMarker>, u32)>,
    },
    SetChannelOverwrite {
        channel_id: Id<ChannelMarker>,
        target: OverwriteTarget,
        allow: u64,
        deny: u64,
        label: String,
    },
    DeleteChannelOverwrite {
        channel_id: Id<ChannelMarker>,
        target: OverwriteTarget,
        label: String,
    },
    SetVoiceChannelStatus {
        channel_id: Id<ChannelMarker>,
        status: Option<String>,
    },
    LoadSoundboardSounds {
        guild_id: Option<Id<GuildMarker>>,
    },
    /// Play a sound into the voice channel we are in.
    PlaySoundboardSound {
        channel_id: Id<ChannelMarker>,
        sound_id: u64,
        /// The guild the sound came from, for a guild sound played elsewhere.
        source_guild_id: Option<Id<GuildMarker>>,
        label: String,
    },
    RenameSoundboardSound {
        guild_id: Id<GuildMarker>,
        sound_id: u64,
        name: String,
    },
    DeleteSoundboardSound {
        guild_id: Id<GuildMarker>,
        sound_id: u64,
        label: String,
    },
    LoadGuildInvites {
        guild_id: Id<GuildMarker>,
    },
    CreateChannelInvite {
        channel_id: Id<ChannelMarker>,
        /// Seconds until it stops working. Zero never expires.
        max_age_seconds: u32,
        /// Zero is unlimited, which is what Discord means by it here.
        max_uses: u32,
        temporary: bool,
    },
    RevokeInvite {
        code: String,
    },
    LoadGuildEmojis {
        guild_id: Id<GuildMarker>,
    },
    /// Add a custom emoji from an image on disk.
    CreateEmoji {
        guild_id: Id<GuildMarker>,
        name: String,
        image: Box<ProfileAvatarUpload>,
    },
    RenameEmoji {
        guild_id: Id<GuildMarker>,
        emoji_id: Id<EmojiMarker>,
        name: String,
    },
    DeleteEmoji {
        guild_id: Id<GuildMarker>,
        emoji_id: Id<EmojiMarker>,
        label: String,
    },
    LoadGuildAuditLog {
        guild_id: Id<GuildMarker>,
    },
    UnbanMember {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        label: String,
    },
    /// Ask to be someone's friend, by username.
    SendFriendRequest {
        /// As typed. Parsed by `friend_request_target`, so both clients accept
        /// the same forms and neither has to know about discriminators.
        target: String,
    },
    /// Accept an incoming request, or befriend a known user.
    AddFriend {
        user_id: Id<UserMarker>,
        label: String,
    },
    BlockUser {
        user_id: Id<UserMarker>,
        label: String,
    },
    /// Unfriend, cancel, decline or unblock - Discord models all four the
    /// same way. `label` names which one the user asked for.
    RemoveRelationship {
        user_id: Id<UserMarker>,
        label: String,
    },
    /// Replace a member's roles with this set.
    SetMemberRoles {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        role_ids: Vec<Id<RoleMarker>>,
        label: String,
    },
    /// Time a member out for a number of minutes, or clear it with `None`.
    TimeoutMember {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        minutes: Option<u32>,
        label: String,
    },
    /// Look up an invite without joining, so it can be shown before accepting.
    ResolveInvite {
        code: String,
    },
    /// Accept an invite, joining the guild it points at.
    AcceptInvite {
        code: String,
    },
    SetSelectedMessageChannel {
        channel_id: Option<Id<ChannelMarker>>,
    },
    TriggerTyping {
        channel_id: Id<ChannelMarker>,
    },
    SubscribeDirectMessage {
        channel_id: Id<ChannelMarker>,
    },
    SubscribeGuildChannel {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    },
    /// Resubscribe an active op-37 channel subscription with a wider set of
    /// member-list ranges as the user scrolls through the member sidebar.
    UpdateMemberListSubscription {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        ranges: Vec<(u32, u32)>,
    },
    JoinVoiceChannel {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        self_mute: bool,
        self_deaf: bool,
        input_source: Option<String>,
        output_source: Option<String>,
        allow_microphone_transmit: bool,
        noise_suppression: bool,
        microphone_sensitivity: crate::discord::MicrophoneSensitivityDb,
        microphone_volume: crate::discord::VoiceVolumePercent,
        voice_output_volume: crate::discord::VoiceVolumePercent,
        participant_playback_settings: Vec<(
            Id<UserMarker>,
            crate::discord::VoiceParticipantPlaybackSettings,
        )>,
    },
    UpdateVoiceState {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        self_mute: bool,
        self_deaf: bool,
    },
    UpdateVoiceCapturePermission {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        allow_microphone_transmit: bool,
        noise_suppression: bool,
        microphone_sensitivity: crate::discord::MicrophoneSensitivityDb,
        microphone_volume: crate::discord::VoiceVolumePercent,
        voice_output_volume: crate::discord::VoiceVolumePercent,
    },
    UpdateVoiceAudioSources {
        input_source: Option<String>,
        output_source: Option<String>,
    },
    LoadVoiceAudioSources {
        request_id: u64,
    },
    UpdateVoiceParticipantPlayback {
        user_id: Id<UserMarker>,
        settings: crate::discord::VoiceParticipantPlaybackSettings,
    },
    WatchVoiceStream {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        display_name: String,
    },
    LoadStreamCaptureTargets {
        request_id: StreamCaptureTargetsRequestId,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    },
    StartVoiceStream {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        target: crate::discord::StreamCaptureTarget,
    },
    StopVoiceStream {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    },
    LeaveVoiceChannel {
        scope: VoiceScope,
        self_mute: bool,
        self_deaf: bool,
    },
    LoadAttachmentPreview {
        url: String,
    },
    LoadProfileAvatarPreview {
        key: String,
        upload: ProfileAvatarUpload,
    },
    SendMessage {
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
        content: String,
        reply_to: Option<ReplyReference>,
        attachments: Vec<MessageAttachmentUpload>,
        /// Stickers to send with it. Discord accepts at most three, and a
        /// sticker-only message is allowed to have no content.
        sticker_ids: Vec<Id<StickerMarker>>,
    },
    CreateForumPost {
        post: ForumPostCreate,
    },
    SendTtsMessage {
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
        content: String,
    },
    LoadApplicationCommands {
        guild_id: Option<Id<GuildMarker>>,
    },
    RunApplicationCommand {
        invocation: ApplicationCommandInvocation,
    },
    RequestApplicationCommandAutocomplete {
        invocation: ApplicationCommandAutocompleteInvocation,
    },
    EditMessage {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: String,
    },
    DeleteMessage {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    RemoveMessageEmbeds {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    OpenUrl {
        url: String,
    },
    PlayMedia {
        target: MediaPlaybackTarget,
        request_id: Option<MediaPlaybackRequestId>,
    },
    DownloadAttachment {
        id: AttachmentDownloadId,
        url: String,
        filename: String,
        source: DownloadAttachmentSource,
    },
    AddReaction {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    RemoveReaction {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    LoadReactionUsers {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
        after: Option<Id<UserMarker>>,
    },
    LoadPinnedMessages {
        channel_id: Id<ChannelMarker>,
    },
    SetMessagePinned {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    },
    VotePoll {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: Vec<u8>,
    },
    LoadUserProfile {
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    },
    UpdateUserProfile {
        // Boxed: this is by far the largest variant - two profile halves, each
        // with an avatar upload - and unboxed it sets the size of every
        // AppCommand and of the dispatch enums that carry one.
        update: Box<UserProfileUpdate>,
    },
    UpdateCurrentUserStatus {
        status: PresenceStatus,
    },
    UpdateGuildFolderSettings {
        folder_id: u64,
        name: Option<String>,
        color: Option<u32>,
    },
    UpdateCurrentUserActivity {
        status: PresenceStatus,
        activities: Vec<ActivityInfo>,
        /// RPC `client_id` whose live activity this is, so the RPC server keeps
        /// re-broadcasting it. `None` for a manual activity, which RPC must not override.
        track_client_id: Option<String>,
    },
    AckChannel {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    ScheduleAckChannel {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    SetGuildMuted {
        guild_id: Id<GuildMarker>,
        muted: bool,
        duration: Option<MuteDuration>,
        label: String,
    },
    SetChannelMuted {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        muted: bool,
        duration: Option<MuteDuration>,
        label: String,
    },
    /// Mute a forum post (thread). Uses the thread-member settings endpoint
    /// rather than the guild `channel_overrides`, which rejects thread types.
    SetThreadMuted {
        channel_id: Id<ChannelMarker>,
        muted: bool,
        duration: Option<MuteDuration>,
        label: String,
    },
    /// Follow (join) or unfollow (leave) a forum post thread.
    SetThreadFollowed {
        channel_id: Id<ChannelMarker>,
        followed: bool,
        label: String,
    },
    /// Set the notification level for a thread. Flags: 2 = All messages,
    /// 4 = Only @mentions (Discord default), 8 = Nothing.
    SetThreadNotificationLevel {
        channel_id: Id<ChannelMarker>,
        flags: u64,
        label: String,
    },
    /// Archive ("close") or reopen a thread (regular thread or forum post).
    SetThreadArchived {
        channel_id: Id<ChannelMarker>,
        archived: bool,
        label: String,
    },
    /// Lock or unlock a thread.
    SetThreadLocked {
        channel_id: Id<ChannelMarker>,
        locked: bool,
        label: String,
    },
    /// Pin or unpin a forum post within its parent forum (pinning is forum-only).
    /// `current_flags` is the thread's present channel flags so the handler can
    /// flip just the PINNED bit without clobbering the others.
    SetThreadPinned {
        channel_id: Id<ChannelMarker>,
        pinned: bool,
        current_flags: u64,
        label: String,
    },
    /// Permanently delete a thread (its channel).
    DeleteThread {
        channel_id: Id<ChannelMarker>,
        label: String,
    },
    /// Edit a thread's general settings (title, applied tags for forum posts,
    /// slow-mode cooldown, auto-archive duration) in one PATCH. The result
    /// arrives over the gateway THREAD_UPDATE, so there is no optimistic event.
    EditThread {
        channel_id: Id<ChannelMarker>,
        name: String,
        applied_tags: Vec<Id<ForumTagMarker>>,
        rate_limit_per_user: u64,
        auto_archive_duration: u64,
        label: String,
    },
    AckChannels {
        targets: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
    },
    /// Fetch recent mentions for the inbox Mentions tab in one request.
    LoadInboxMentions {
        request_id: u64,
        before: Option<Id<MessageMarker>>,
    },
    /// Remove one message from Discord's recent-mentions inbox.
    DeleteInboxMention {
        message_id: Id<MessageMarker>,
    },
    /// Fetch a small slice of a channel's latest messages for the inbox Unreads tab.
    LoadInboxChannelHistory {
        channel_id: Id<ChannelMarker>,
        request_id: u64,
    },
}

fn normalized_search_token(value: &str) -> String {
    value.trim().trim_start_matches(':').to_ascii_lowercase()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadAttachmentSource {
    AttachmentViewer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaPlaybackSource {
    Message,
    AttachmentViewer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediaPlaybackRequestId(u64);

impl MediaPlaybackRequestId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamCaptureTargetsRequestId(u64);

impl StreamCaptureTargetsRequestId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaPlaybackTarget {
    pub url: String,
    pub label: String,
    pub source: MediaPlaybackSource,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_nonces_are_unique_snowflake_values() {
        let first = next_message_nonce();
        let second = next_message_nonce();

        assert_ne!(first, second);
        assert!(second.get() > first.get());
    }
}
