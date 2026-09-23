//! The view-state types the workspace renders from.
//!
//! Separated from the workspace itself because none of them touch it: they
//! are the shapes its fields hold, and reading them meant scrolling past
//! nine hundred lines to reach the first line of behaviour.

use concord::config::AppOptions;
use concord::discord::{
    ActivityInfo, ActivityKind, AppCommand, Id, marker, password_auth::MfaMethod,
};
use concord::risk::RiskKind;
use concord::t;

use crate::model::projection::Selection;
use crate::theme::Presence;
use crate::ui::composer::Composer;
use crate::ui::login::Login;
use concord::discord::InvitePreview;
use concord_ui::model::AttachmentViewerZoom;

/// Everything the workspace renders, projected from the core's state store.
pub struct WorkspaceModel {
    pub guilds: Vec<GuildEntry>,
    pub channels: Vec<ChannelEntry>,
    pub members: Vec<MemberEntry>,
    pub selected_guild: usize,
    pub selected_channel: usize,
    pub connected: bool,
    pub status_line: String,
}

pub struct GuildEntry {
    /// `None` for the Direct Messages pseudo-guild.
    pub id: Option<Id<marker::GuildMarker>>,
    pub name: String,
    pub icon: Option<String>,
    pub unread: bool,
    pub mentions: u32,
    /// Folder this guild sits in, if any: its id, name and colour.
    ///
    /// Carried per guild rather than as a separate tree because the rail is a
    /// flat list; a folder is a run of adjacent guilds sharing this value.
    pub folder: Option<GuildFolderEntry>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct GuildFolderEntry {
    pub id: u64,
    pub name: Option<String>,
    pub color: Option<u32>,
}

pub struct ChannelEntry {
    pub id: Option<Id<marker::ChannelMarker>>,
    /// Newest message, needed to mark the channel read.
    pub last_message: Option<Id<marker::MessageMarker>>,
    /// Category this channel sits under, for collapse.
    pub parent: Option<Id<marker::ChannelMarker>>,
    pub name: String,
    pub kind: ChannelKind,
    /// Archived threads are shown dimmed rather than hidden, so a thread the
    /// user is reading does not vanish when it auto-archives.
    pub archived: bool,
    pub unread: bool,
    pub mentions: u32,
    /// Occupants, for voice channels only.
    pub voice: Vec<VoiceMember>,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum ChannelKind {
    Text,
    Voice,
    Forum,
    Category,
    /// A thread, rendered nested under its parent channel.
    Thread,
    /// A voice channel with an audience. Joined like a voice channel, but you
    /// are listening until a moderator invites you to speak - so it is its own
    /// kind rather than a flag on Voice, and the menu can say so.
    Stage,
}

impl ChannelKind {
    /// Whether clicking this joins a call.
    ///
    /// A stage is joined exactly like a voice channel. Comparing against
    /// `Voice` alone is how a stage becomes a channel you can see and not
    /// enter, which is the bug this exists to prevent.
    pub fn joins_voice(self) -> bool {
        matches!(self, Self::Voice | Self::Stage)
    }
}

impl ChannelKind {
    pub fn glyph(self) -> &'static str {
        match self {
            ChannelKind::Text => "#",
            ChannelKind::Voice => "♪",
            ChannelKind::Stage => "◎",
            ChannelKind::Forum => "▤",
            ChannelKind::Category => "",
            ChannelKind::Thread => "\u{2937}",
        }
    }
}

/// A participant in a voice channel.
pub struct VoiceMember {
    /// Needed to address per-participant playback settings.
    pub user_id: Id<marker::UserMarker>,
    pub name: String,
    pub muted: bool,
    pub deafened: bool,
    pub streaming: bool,
    /// Whether their camera is on. Separate from `streaming`, which is a
    /// shared screen - someone can be doing both at once.
    pub on_camera: bool,
    pub speaking: bool,
}

/// Audio devices offered by the picker.
#[derive(Default)]
pub struct AudioDevices {
    /// Input devices as (id, label).
    pub inputs: Vec<(String, String)>,
    pub outputs: Vec<(String, String)>,
    pub selected_input: Option<String>,
    pub selected_output: Option<String>,
    /// Why enumeration failed, if it did.
    pub error: Option<String>,
}

pub struct MemberEntry {
    pub name: String,
    /// What they are doing, shown under the name. `None` for group headers
    /// and for anyone idle.
    pub activity: Option<String>,
    /// `None` for group headers, which are not clickable.
    pub user_id: Option<Id<marker::UserMarker>>,
    pub avatar: Option<String>,
    pub presence: Presence,
    /// Group headers ("ONLINE - 42") render as section labels, not rows.
    pub is_group: bool,
    pub is_bot: bool,
    pub color: Option<u32>,
}

impl WorkspaceModel {
    /// An empty model, shown before a session delivers state.
    ///
    /// Deliberately empty rather than populated with sample content: fake
    /// guilds on screen during connect would be indistinguishable from real
    /// ones that failed to load. For sample content, run with the `fixtures`
    /// feature and the token "test".
    pub fn empty() -> Self {
        Self {
            guilds: Vec::new(),
            channels: Vec::new(),
            members: Vec::new(),
            selected_guild: usize::MAX,
            selected_channel: usize::MAX,
            connected: false,
            status_line: String::new(),
        }
    }
}

/// A destructive action held for confirmation.
///
/// Deleting is irreversible and pinning is visible to everyone in the channel,
/// so both are worth a second press rather than a single misplaced click.
/// What a one-line text prompt is collecting.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    ThreadName,
    /// Title for a new forum post; the body comes from the composer.
    ForumPostTitle,
    /// An invite link or code to join.
    InviteCode,
    /// A new name for a custom emoji, by its index in the open emoji tab.
    EmojiName(usize),
    /// A new name for a sound, by its index in the open sounds tab.
    SoundName(usize),
    /// A name for a new role in the open guild.
    NewRole,
    /// A name for a new server template.
    NewTemplate,
    /// The line shown to people arriving at the server.
    WelcomeDescription,
    /// The channel the widget's invite points at, by name.
    WidgetChannel,
    /// A new scheduled event, typed as one line of fields.
    NewEvent,
    /// A stage's topic. Empty ends the stage.
    StageTopic(Id<marker::ChannelMarker>),
    /// An existing event, by its id. Same line format as creating one.
    EditEvent(u64),
    /// User ids to ban in one request.
    BulkBan,
    /// The short line beside a voice channel's name.
    VoiceStatus(Id<marker::ChannelMarker>),
    /// `name | tags | path` for a sticker to upload.
    NewSticker,
    /// A new name for the sticker at this index.
    StickerRename(usize),
    /// Discovery search keywords, comma separated.
    DiscoveryKeywords,
    /// The server's long description in discovery.
    DiscoveryAbout,
    /// A new name for the open guild.
    GuildName,
    /// A path to an image to use as the guild icon.
    GuildIcon,
    /// A channel topic, by channel.
    ChannelTopic(Id<marker::ChannelMarker>),
    /// Slowmode in seconds, by channel.
    ChannelSlowmode(Id<marker::ChannelMarker>),
    /// A path to an image to add as a custom emoji.
    EmojiImage,
    /// A name for a new channel in the open guild.
    NewChannel,
    /// A new name for the open channel.
    ChannelName(Id<marker::ChannelMarker>),
}

impl Prompt {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Prompt::EmojiName(_) => "Rename emoji",
            Prompt::SoundName(_) => "Rename sound",
            Prompt::NewRole => "New role",
            Prompt::NewTemplate => "New template",
            Prompt::WelcomeDescription => "Welcome description",
            Prompt::WidgetChannel => "Widget invite channel",
            Prompt::NewEvent => "New event",
            Prompt::StageTopic(_) => "Stage topic",
            Prompt::EditEvent(_) => "Edit event",
            Prompt::BulkBan => "Ban by user id",
            Prompt::VoiceStatus(_) => "Voice status",
            Prompt::NewSticker => "Add a sticker",
            Prompt::StickerRename(_) => "Rename sticker",
            Prompt::DiscoveryKeywords => "Search keywords",
            Prompt::DiscoveryAbout => "Discovery description",
            Prompt::GuildName => "Rename server",
            Prompt::GuildIcon => "Server icon",
            Prompt::ChannelTopic(_) => "Channel topic",
            Prompt::ChannelSlowmode(_) => "Slowmode",
            Prompt::EmojiImage => "Add emoji",
            Prompt::NewChannel => "New channel",
            Prompt::ChannelName(_) => "Rename channel",
            Prompt::ThreadName => "Rename thread",
            Prompt::ForumPostTitle => "New post",
            Prompt::InviteCode => "Join a server",
        }
    }

    pub(crate) fn placeholder(self) -> &'static str {
        match self {
            Prompt::EmojiName(_) => "Emoji name",
            Prompt::SoundName(_) => "Sound name",
            Prompt::NewRole => "Role name",
            Prompt::NewTemplate => "Template name",
            Prompt::WelcomeDescription => "Shown to people arriving - empty clears it",
            Prompt::WidgetChannel => "Channel name - empty means no invite",
            Prompt::NewEvent => "name | start | end | where - times as 2026-09-01T19:00:00Z",
            Prompt::StageTopic(_) => "What the session is about - empty ends the stage",
            Prompt::EditEvent(_) => "name | start | end | where - times as 2026-09-01T19:00:00Z",
            Prompt::BulkBan => "User ids, separated by anything - spaces, commas, newlines",
            Prompt::VoiceStatus(_) => "What is happening in there - empty clears it",
            Prompt::NewSticker => "name | tags | path - PNG, APNG, GIF or Lottie under 500 KiB",
            Prompt::StickerRename(_) => "Sticker name",
            Prompt::DiscoveryKeywords => "Comma separated, at most 10",
            Prompt::DiscoveryAbout => "Shown on the server's discovery page",
            Prompt::GuildName => "Server name",
            Prompt::GuildIcon => "Path to a PNG, JPEG, GIF or WebP",
            Prompt::ChannelTopic(_) => "Topic - empty clears it",
            Prompt::ChannelSlowmode(_) => "Seconds between messages, 0 for none",
            Prompt::EmojiImage => "Path to a PNG, JPEG, GIF or WebP",
            Prompt::NewChannel => "Channel name",
            Prompt::ChannelName(_) => "Channel name",
            Prompt::ThreadName => "Thread name",
            Prompt::ForumPostTitle => "Post title",
            Prompt::InviteCode => "discord.gg/... or an invite code",
        }
    }
}

pub struct Confirm {
    pub message: usize,
    pub action: ConfirmAction,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    Delete,
    Pin,
    Unpin,
}

impl ConfirmAction {
    pub(crate) fn prompt(self) -> &'static str {
        match self {
            ConfirmAction::Delete => "Delete this message?",
            ConfirmAction::Pin => "Pin this message for everyone?",
            ConfirmAction::Unpin => "Unpin this message?",
        }
    }
}

/// Why the switcher is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwitcherPurpose {
    #[default]
    Navigate,
    Forward {
        message_id: Id<marker::MessageMarker>,
        source_channel_id: Id<marker::ChannelMarker>,
    },
}

/// One open channel tab.
/// The verb Discord puts in front of an activity name.
///
/// Translated here rather than in the core because the core's verb is part of
/// the wire-facing display line, and a translated one there would change what
/// gets stored and compared.
pub(crate) fn activity_kind_label(kind: ActivityKind) -> String {
    match kind {
        ActivityKind::Playing => t!("activity-playing"),
        ActivityKind::Listening => t!("activity-listening"),
        ActivityKind::Watching => t!("activity-watching"),
        ActivityKind::Competing => t!("activity-competing"),
        ActivityKind::Streaming => t!("activity-streaming"),
        ActivityKind::Hang | ActivityKind::Custom | ActivityKind::Unknown(_) => {
            t!("activity-custom")
        }
    }
}

/// An activity being composed.
///
/// One editor per field rather than one shared one, so switching fields keeps
/// what was typed in the others - a form that forgets is worse than no form.
pub struct ActivityDraft {
    pub kind: ActivityKind,
    pub fields: [Composer; ActivityDraft::FIELDS],
    pub focused: usize,
}

impl ActivityDraft {
    pub const FIELDS: usize = 3;

    /// The kinds worth offering.
    ///
    /// Streaming is left out because it renders as nothing without a verified
    /// Twitch or YouTube URL, and Custom has its own simpler prompt.
    pub const KINDS: [ActivityKind; 4] = [
        ActivityKind::Playing,
        ActivityKind::Listening,
        ActivityKind::Watching,
        ActivityKind::Competing,
    ];

    /// What the draft broadcasts.
    ///
    /// An empty name yields nothing rather than a nameless activity, which
    /// Discord happily shows to everyone as a blank line.
    pub(crate) fn to_activities(&self) -> Vec<ActivityInfo> {
        let name = self.fields[0].text().trim().to_owned();
        if name.is_empty() {
            return Vec::new();
        }
        let text = |index: usize| {
            let value = self.fields[index].text().trim().to_owned();
            (!value.is_empty()).then_some(value)
        };
        vec![ActivityInfo {
            kind: self.kind,
            details: text(1),
            state: text(2),
            ..ActivityInfo::playing(name)
        }]
    }

    pub(crate) fn from_current(current: Option<&ActivityInfo>) -> Self {
        let mut draft = Self {
            kind: current
                .map(|activity| activity.kind)
                .filter(|kind| Self::KINDS.contains(kind))
                .unwrap_or(ActivityKind::Playing),
            fields: std::array::from_fn(|_| Composer::default()),
            focused: 0,
        };
        if let Some(activity) = current {
            draft.fields[0].set_text(&activity.name);
            draft.fields[1].set_text(activity.details.as_deref().unwrap_or_default());
            draft.fields[2].set_text(activity.state.as_deref().unwrap_or_default());
        }
        draft
    }
}

pub struct ChannelTab {
    pub channel_id: Id<marker::ChannelMarker>,
    /// The guild it was opened from, so switching tabs restores the sidebar
    /// rather than leaving the wrong channel list showing.
    pub selection: Selection,
    pub name: String,
    /// What was typed and not sent. Restored on return.
    pub draft: String,
    /// Where the log was scrolled to, so returning does not jump to the
    /// bottom and lose the reader's place.
    pub scroll: gpui::Point<gpui::Pixels>,
}

/// An action Discord's anti-spam checks watch, which is warned about before
/// it is carried out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RiskAction {
    JoinGuild,
    LeaveGuild,
    /// Changing your profile while connected through a third-party client.
    /// Carries the change, since it has to survive the confirmation.
    ProfileEdit(Id<marker::GuildMarker>, String),
    /// Adding, removing or blocking someone. Carries the command, which is
    /// already built by the time the warning goes up.
    FriendAction(Box<AppCommand>),
    /// Removing inactive members. Carries the command, which needs the window
    /// and the count that were on screen when it was offered.
    PruneMembers(Box<AppCommand>),
    /// Banning several people at once. Carries the list, which is the whole
    /// danger: it must be the one that was on screen.
    BulkBan(Box<AppCommand>),
}

impl RiskAction {
    /// The shared kind, which owns the wording and the opt-out. Only the
    /// payload each action has to carry through the confirmation lives here.
    pub(crate) fn kind(&self) -> RiskKind {
        match self {
            Self::JoinGuild => RiskKind::JoinGuild,
            Self::LeaveGuild => RiskKind::LeaveGuild,
            Self::ProfileEdit(..) => RiskKind::ProfileEdit,
            Self::FriendAction(..) => RiskKind::FriendAction,
            Self::PruneMembers(..) => RiskKind::PruneMembers,
            Self::BulkBan(..) => RiskKind::BulkBan,
        }
    }

    pub(crate) fn body(&self) -> String {
        self.kind().explanation()
    }

    pub(crate) fn suppressed(&self, options: &AppOptions) -> bool {
        self.kind().suppressed(&options.warnings)
    }

    pub(crate) fn suppress(&self, options: &mut AppOptions) {
        self.kind().suppress(&mut options.warnings);
    }
}

/// An invite's limits, in the words Discord's own UI uses.
pub(crate) fn invite_summary(invite: &concord::discord::GuildInviteInfo) -> String {
    let uses = match invite.max_uses {
        Some(max) => format!("{}/{max}", invite.uses),
        // Discord writes "no limit" as 0; showing "3/0" would read as spent.
        None => format!("{} ({})", invite.uses, t!("status-invite-unlimited")),
    };
    let expiry = match invite.max_age_seconds {
        Some(seconds) => format!("{}m", seconds / 60),
        None => t!("status-invite-never-expires"),
    };

    let mut parts = vec![uses, expiry];
    if let Some(channel) = &invite.channel_name {
        parts.push(format!("#{channel}"));
    }
    if let Some(inviter) = &invite.inviter {
        parts.push(inviter.clone());
    }
    parts.join(" - ")
}

/// What is unusual about an emoji, if anything.
pub(crate) fn emoji_summary(emoji: &concord::discord::GuildEmojiInfo) -> Option<String> {
    let mut parts = Vec::new();
    if emoji.animated {
        parts.push(t!("label-emoji-animated"));
    }
    // Worth saying: a role-restricted emoji is invisible to most members, who
    // would otherwise wonder why they cannot use one they can see listed.
    if emoji.role_restricted {
        parts.push(t!("label-emoji-restricted"));
    }
    (!parts.is_empty()).then(|| parts.join(" - "))
}

/// One audit entry as a sentence.
pub(crate) fn audit_line(entry: &concord::discord::AuditLogEntryInfo) -> String {
    let actor = entry.actor.clone().unwrap_or_else(|| t!("label-unknown"));
    match &entry.target {
        Some(target) => format!("{actor} {} {target}", entry.action.label()),
        None => format!("{actor} {}", entry.action.label()),
    }
}

/// What a context menu was opened on.
///
/// Carrying the subject rather than a list of closures: the same menu is built
/// from the same action enums the keyboard paths use, so the two cannot offer
/// different things.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextSubject {
    Message(usize),
    Channel(Id<marker::ChannelMarker>),
    Member(Id<marker::UserMarker>),
    Guild(Id<marker::GuildMarker>),
}

/// An open context menu.
pub struct ContextMenu {
    pub subject: ContextSubject,
    pub at: gpui::Point<gpui::Pixels>,
}

/// What a permission grid is editing.
///
/// A role grants or does not - two states. A channel overwrite allows, denies,
/// or says nothing and lets the roles decide - three. Modelling the role case
/// as a special overwrite would be neater and wrong: a role's bitfield has no
/// inherit, and offering one would show a state that cannot be saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionScope {
    Role {
        guild_id: Id<marker::GuildMarker>,
        role_id: Id<marker::RoleMarker>,
    },
    ChannelOverwrite {
        channel_id: Id<marker::ChannelMarker>,
        target: concord::discord::OverwriteTarget,
    },
}

/// Permissions being edited.
pub struct PermissionGridView {
    pub scope: PermissionScope,
    pub name: String,
    /// Allowed bits. For a role this is the whole answer.
    pub allow: u64,
    /// Denied bits. Always zero for a role, which has no deny.
    pub deny: u64,
    pub original_allow: u64,
    pub original_deny: u64,
}

impl PermissionGridView {
    pub fn allows_inherit(&self) -> bool {
        matches!(self.scope, PermissionScope::ChannelOverwrite { .. })
    }

    pub fn is_dirty(&self) -> bool {
        self.allow != self.original_allow || self.deny != self.original_deny
    }
}

/// The soundboard picker.
///
/// Holds both lists at once: the guild's sounds and the defaults every account
/// has. A guild that has added nothing still gets a usable picker, which is
/// the reason for fetching both rather than one.
pub struct SoundboardView {
    pub guild_sounds: Vec<concord::discord::SoundboardSound>,
    pub default_sounds: Vec<concord::discord::SoundboardSound>,
    pub loading: bool,
    pub error: Option<String>,
}

impl SoundboardView {
    /// Every sound the picker shows, the guild's first.
    ///
    /// The guild's own come first because they are the ones somebody opened
    /// the picker to reach; the defaults are the fallback.
    pub fn sounds(&self) -> impl Iterator<Item = &concord::discord::SoundboardSound> {
        self.guild_sounds.iter().chain(self.default_sounds.iter())
    }
}

/// The linked-accounts panel.
pub struct ConnectionsView {
    pub connections: Vec<concord::discord::Connection>,
    pub loading: bool,
    pub error: Option<String>,
}

/// Credentials and two-factor.
///
/// The form itself is `concord::discord::AccountForm`, whose own `Debug`
/// redacts the three password fields - so a `{:?}` of this view cannot print
/// them.
#[derive(Debug)]
pub struct AccountView {
    pub form: concord::discord::AccountForm,
    pub focused: usize,
    /// The enrolment secret while two-factor is being set up.
    pub totp_secret: Option<concord::discord::TotpSecret>,
    pub totp_code: String,
    pub backup_codes: Vec<concord::discord::BackupCode>,
}

/// Sessions and authorised applications.
pub struct AccessView {
    pub sessions: Vec<concord::discord::AuthSession>,
    pub apps: Vec<concord::discord::AuthorisedApp>,
    pub loading: bool,
    pub error: Option<String>,
    /// Which sessions are selected for logout.
    pub logout_targets: std::collections::BTreeSet<String>,
    /// The password Discord requires for a logout, held only while it is being
    /// typed and dropped the moment the request is sent. Never persisted.
    pub password: String,
}

impl AccessView {
    /// Bullets, so the password is never drawn.
    pub fn masked_password(&self) -> String {
        "•".repeat(self.password.chars().count())
    }
}

/// An image opened full size.
///
/// Carries every image in the message rather than only the one clicked, so
/// paging between them does not have to go back to the row it came from -
/// which may have scrolled away by then.
pub struct ImageViewerView {
    pub urls: Vec<String>,
    pub index: usize,
    pub zoom: AttachmentViewerZoom,
}

impl ImageViewerView {
    pub fn url(&self) -> Option<&str> {
        self.urls.get(self.index).map(String::as_str)
    }

    /// Step to the next or previous image, wrapping at each end.
    ///
    /// Wrapping rather than stopping: an arrow that does nothing at the edge
    /// reads as a broken control rather than as the end of the list.
    pub(crate) fn step(&mut self, forward: bool) {
        let count = self.urls.len();
        if count == 0 {
            return;
        }
        self.index = if forward {
            (self.index + 1) % count
        } else {
            (self.index + count - 1) % count
        };
    }
}

/// Discord's own default prune window.
pub(crate) const DEFAULT_PRUNE_DAYS: u16 = 30;

/// Which part of the server-management panel is showing.
///
/// One panel with three tabs rather than three separate ones: they are all
/// "administering this server", and three entry points to find would be worse
/// than one with tabs in it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerTab {
    Settings,
    Invites,
    Roles,
    Emoji,
    Sounds,
    AutoMod,
    AuditLog,
    /// The welcome screen, the widget, and pruning - how members arrive and
    /// leave. One tab because each is two or three rows.
    Membership,
    Events,
    Templates,
    Members,
    Onboarding,
    Discovery,
    Stickers,
}

impl ServerTab {
    pub const ALL: [Self; 14] = [
        Self::Settings,
        Self::Invites,
        Self::Roles,
        Self::Emoji,
        Self::Sounds,
        Self::AutoMod,
        Self::AuditLog,
        Self::Membership,
        Self::Events,
        Self::Templates,
        Self::Members,
        Self::Onboarding,
        Self::Discovery,
        Self::Stickers,
    ];

    pub fn label(self) -> String {
        match self {
            Self::Settings => t!("label-settings"),
            Self::Invites => t!("label-invites"),
            Self::Roles => t!("label-roles"),
            Self::Emoji => t!("label-emoji"),
            Self::Sounds => t!("label-soundboard"),
            Self::AutoMod => t!("label-automod"),
            Self::AuditLog => t!("label-audit-log"),
            Self::Membership => t!("label-membership"),
            Self::Events => t!("label-events"),
            Self::Templates => t!("label-templates"),
            Self::Members => t!("label-members"),
            Self::Onboarding => t!("label-onboarding"),
            Self::Discovery => t!("label-discovery"),
            Self::Stickers => t!("label-stickers"),
        }
    }

    /// What to fetch when this tab opens.
    ///
    /// A list rather than one command: membership needs three. Empty for
    /// settings and roles, which arrive with the guild and are read from the
    /// snapshot rather than asked for.
    pub(crate) fn load(self, guild_id: Id<marker::GuildMarker>) -> Vec<AppCommand> {
        match self {
            // All three are read from the snapshot rather than fetched.
            Self::Settings | Self::Roles | Self::Members => Vec::new(),
            Self::Onboarding => vec![AppCommand::LoadOnboarding { guild_id }],
            Self::Discovery => vec![AppCommand::LoadDiscoveryMetadata { guild_id }],
            Self::Stickers => vec![AppCommand::LoadGuildStickers { guild_id }],
            Self::Invites => vec![AppCommand::LoadGuildInvites { guild_id }],
            Self::Emoji => vec![AppCommand::LoadGuildEmojis { guild_id }],
            Self::Sounds => vec![AppCommand::LoadSoundboardSounds {
                guild_id: Some(guild_id),
            }],
            Self::AutoMod => vec![AppCommand::LoadAutoModRules { guild_id }],
            Self::AuditLog => vec![AppCommand::LoadGuildAuditLog { guild_id }],
            Self::Events => vec![AppCommand::LoadScheduledEvents { guild_id }],
            Self::Templates => vec![AppCommand::LoadGuildTemplates { guild_id }],
            Self::Membership => vec![
                AppCommand::LoadWelcomeScreen { guild_id },
                AppCommand::LoadGuildWidget { guild_id },
                AppCommand::LoadPruneCount {
                    guild_id,
                    days: DEFAULT_PRUNE_DAYS,
                    include_roles: Vec::new(),
                },
            ],
        }
    }
}

/// A guild being administered.
pub struct ServerManagementView {
    pub guild_id: Id<marker::GuildMarker>,
    pub tab: ServerTab,
    pub invites: Vec<concord::discord::GuildInviteInfo>,
    pub emojis: Vec<concord::discord::GuildEmojiInfo>,
    pub audit_log: Vec<concord::discord::AuditLogEntryInfo>,
    /// Read from the snapshot when the tab opens, highest first - the order
    /// that decides which role wins a conflict.
    pub roles: Vec<concord::discord::RoleState>,
    /// The guild's own sounds. The defaults belong to the picker, where they
    /// can be played but not managed.
    pub sounds: Vec<concord::discord::SoundboardSound>,
    pub automod: Vec<concord::discord::AutoModRule>,
    pub welcome: Option<concord::discord::WelcomeScreen>,
    pub widget: Option<concord::discord::GuildWidget>,
    pub prune_days: u16,
    /// `None` until the count has arrived - not the same as zero, and a panel
    /// showing them alike would offer to prune nobody.
    pub prune_count: Option<u64>,
    /// What the members tab is filtered by.
    pub member_query: String,
    pub onboarding: Option<concord::discord::Onboarding>,
    pub onboarding_picked: Vec<u64>,
    pub discovery: Option<concord::discord::DiscoveryMetadata>,
    pub discovery_categories: Vec<concord::discord::DiscoveryCategory>,
    pub stickers: Vec<concord::discord::GuildSticker>,
    pub events: Vec<concord::discord::ScheduledEvent>,
    pub templates: Vec<concord::discord::GuildTemplate>,
    /// The guild's settings as label and value, read from the snapshot.
    pub settings: Vec<(String, String)>,
    /// Set while the open tab's fetch is outstanding. Distinct from an empty
    /// list: a slow fetch must not read as "there are none".
    pub loading: bool,
    pub error: Option<String>,
}

/// A guild's ban list, as shown.
pub struct BanListView {
    pub guild_id: Id<marker::GuildMarker>,
    pub bans: Vec<concord::discord::GuildBanInfo>,
    /// Set while the fetch is outstanding. Distinct from an empty list: a slow
    /// fetch must not read as "nobody is banned".
    pub loading: bool,
    pub error: Option<String>,
}

/// A moderation action against a member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModerationAction {
    ManageRoles,
    Kick,
    Ban,
    Timeout,
    ClearTimeout,
}

/// An invite being looked at, before joining.
pub struct InviteState {
    pub code: String,
    pub preview: Option<InvitePreview>,
    /// Why it cannot be joined, when that is known.
    pub error: Option<String>,
}

/// A pane that can be shown or hidden.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Guilds,
    Channels,
    Messages,
    Members,
}

/// One entry in the mention inbox.
pub struct InboxMention {
    pub channel_id: Id<marker::ChannelMarker>,
    pub message_id: Id<marker::MessageMarker>,
    pub guild_id: Option<Id<marker::GuildMarker>>,
    pub author: String,
    pub content: String,
}

/// Message-search state.
#[derive(Default)]
pub struct Search {
    pub input: Composer,
    pub results: Vec<SearchResult>,
    pub total: Option<usize>,
    pub running: bool,
    pub error: Option<String>,
}

pub struct SearchResult {
    pub author: String,
    pub content: String,
    pub channel_id: Id<marker::ChannelMarker>,
    pub message_id: Id<marker::MessageMarker>,
}

/// Which top-level surface is showing.
pub enum Screen {
    // Boxed because `Login` is ~630 bytes of form state and `Ready` carries
    // none: unboxed, every Screen - including the one the client spends all
    // its time in - pays for the login form's size.
    Login(Box<Login>),
    Ready,
}

/// Actions the login key handler or mouse click handlers can request from the workspace.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LoginAction {
    // Picker selections
    PickPassword,
    PickToken,
    PickQr,
    PickDemo,
    // Navigation
    Back,
    ToggleRemember,
    // Submissions
    SubmitPassword,
    SubmitToken,
    SubmitMfaCode,
    // MFA method choice
    PickMfaMethod(MfaMethod),
}

impl LoginAction {
    /// A stable element id.
    ///
    /// Taken from the action rather than the label, which changes with the
    /// interface language - and an id that changes underneath GPUI loses the
    /// element's state.
    pub(crate) fn element_id(self) -> &'static str {
        match self {
            Self::PickPassword => "login-pick-password",
            Self::PickToken => "login-pick-token",
            Self::PickQr => "login-pick-qr",
            Self::PickDemo => "login-pick-demo",
            Self::Back => "login-back",
            Self::ToggleRemember => "login-toggle-remember",
            Self::SubmitPassword => "login-submit-password",
            Self::SubmitToken => "login-submit-token",
            Self::SubmitMfaCode => "login-submit-mfa",
            Self::PickMfaMethod(MfaMethod::Totp) => "login-mfa-totp",
            Self::PickMfaMethod(MfaMethod::Sms) => "login-mfa-sms",
        }
    }
}
