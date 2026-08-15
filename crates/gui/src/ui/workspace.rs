//! The main three-pane workspace: guild rail, channel sidebar, content area,
//! plus the member list, search and emoji panels that share the right column.
//!
//! Rendering reads only from `WorkspaceModel`, which `model::projection`
//! rebuilds from `DiscordState` on every snapshot revision. Nothing here
//! touches core types directly except to issue commands.

use concord::config::{self, AppOptions, CredentialStoreMode, UiStateOptions};
use concord::discord::{
    ActivityInfo, ActivityKind, AppCommand, AppEvent, ApplicationCommandAutocompleteInvocation,
    ApplicationCommandInfo, ApplicationCommandInvocation, AttachmentDownloadId,
    BuiltinSlashCommandParse, BuiltinSlashCommandSubmit, DownloadAttachmentSource,
    ForumPostArchiveState, ForumPostCreate, GlobalUserProfileUpdate, GuildUserProfileUpdate, Id,
    InvitePreview, MAX_UPLOAD_ATTACHMENT_COUNT, MediaPlaybackSource, MediaPlaybackTarget,
    MessageAttachmentUpload, MessageHistoryAfterMode, MessageSearchQuery, MuteDuration,
    PresenceStatus, ProfileAvatarUpload, ReactionEmoji, ReplyReference,
    StreamCaptureTargetsRequestId, UserProfileUpdate, VoiceConnectionStatus,
    VoiceParticipantPlaybackSettings, VoiceParticipantVolumePercent, VoiceScope,
    VoiceVolumePercent, application_command_content_is_complete, invite_code_from, marker,
    next_message_nonce, parse_builtin_slash_command,
    password_auth::{MfaMethod, PasswordAuthEvent},
    qr_auth::QrEvent,
};
use concord::token_store;
use gpui::{
    ClipboardItem, Context, FocusHandle, KeyDownEvent, PathPromptOptions, Window, WindowHandle,
    prelude::*, px, rgb,
};
use tokio::sync::mpsc;

use crate::model::message::{self, MessageRow};
use crate::model::projection::{self, Navigation, Selection};
use crate::notify;
use crate::session::{SessionHandle, Update};

use concord::tui::keybindings::external::Resolution;

use crate::keymap::{self, Keymap};
use crate::theme::{self, Presence, active, layout, scaled, space, text};
use crate::ui::chrome::{
    VoiceRow, avatar, avatar_with_url, column, header, panel_sunken, presence_dot, row,
    section_label, sidebar_row, voice_participant_row,
};
use crate::ui::composer::{ClipboardIntent, Composer, composer_view};
use crate::ui::emoji::{self, EmojiPicker};
use crate::ui::forum::{self, ForumPost, ForumView};
use crate::ui::login::{Login, LoginEvent, LoginHandle, LoginScreen, PasswordField, login_view};
use crate::ui::messages::{MessageAction, RenderOptions, message_list};
use crate::ui::overlay;
use crate::ui::profile::{ProfileView, profile_view};
use crate::ui::settings::{OnChange, SettingsWindow};
use crate::ui::slash::{SlashPicker, slash_view};
use crate::ui::stream::{self, StreamPicker, share_button};
use crate::ui::switcher::{self, Switcher};

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
}

impl ChannelKind {
    pub fn glyph(self) -> &'static str {
        match self {
            ChannelKind::Text => "#",
            ChannelKind::Voice => "♪",
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
}

impl Prompt {
    fn title(self) -> &'static str {
        match self {
            Prompt::ThreadName => "Rename thread",
            Prompt::ForumPostTitle => "New post",
            Prompt::InviteCode => "Join a server",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
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
    fn prompt(self) -> &'static str {
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

/// A moderation action against a member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModerationAction {
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

pub struct Workspace {
    pub screen: Screen,
    pub model: WorkspaceModel,
    /// Command sink into the core. `None` until a session starts.
    pub handle: Option<SessionHandle>,
    /// Navigation is GUI-owned: the core has no concept of "what is on screen".
    pub nav: Navigation,
    /// Projected rows for the open channel.
    pub messages: Vec<MessageRow>,
    pub composer: Composer,
    /// Display names of users currently typing in the open channel.
    pub typing: Vec<String>,
    /// Message being replied to, shown above the composer until cleared.
    pub replying_to: Option<(Id<marker::MessageMarker>, String)>,
    /// Whether a reply mentions its author. Discord defaults this on, and it
    /// is the setting people most often want to change per message.
    pub reply_ping: bool,
    /// Message being edited. While set, the composer edits instead of sends.
    pub editing: Option<Id<marker::MessageMarker>>,
    /// Channel the user is connected to by voice, if any.
    pub voice_channel: Option<(Id<marker::ChannelMarker>, String)>,
    /// Whether the open channel is muted, as last reported by the core.
    pub channel_muted: bool,
    /// Whether the open guild is muted.
    pub guild_muted: bool,
    /// Recent error-log lines, shown when the debug panel is open.
    pub debug_log: Option<Vec<String>>,
    /// Thread flags as last seen, preserved when pinning so other flags are
    /// not cleared.
    pub thread_flags: u64,
    /// When this client last told the server it was typing.
    pub last_typing: Option<std::time::Instant>,
    /// Presence others see.
    pub status: PresenceStatus,
    /// Whether sends go out as text-to-speech.
    pub send_as_tts: bool,
    /// A destructive action awaiting confirmation.
    pub confirming: Option<Confirm>,
    /// Message the keyboard is on, when navigating the log without a mouse.
    pub selected_message: Option<usize>,
    /// Which pane keyboard focus is on, for cycling and filtering.
    pub focus_pane: Pane,
    /// Filter text for the focused pane, when filtering is active.
    pub pane_filter: Option<Composer>,
    /// Scroll position of the message list, so navigation can move it.
    pub message_scroll: gpui::ScrollHandle,
    /// Pane visibility and widths, shared with the TUI through ui_state.toml.
    pub ui_state: UiStateOptions,
    /// Slash commands published by bots in the open guild.
    pub app_commands: Vec<ApplicationCommandInfo>,
    /// The authenticated user, once READY reports it.
    pub current_user: Option<Id<marker::UserMarker>>,
    /// Slash-command autocomplete, present while typing a bare command.
    pub slash: Option<SlashPicker>,
    /// Recent mentions across every guild. `None` when the panel is closed.
    pub inbox: Option<Vec<InboxMention>>,
    /// Pinned messages for the open channel, shown in a panel when requested.
    pub pins: Option<Vec<(Id<marker::MessageMarker>, String, String)>>,
    /// Text queued for the clipboard, written on the next render pass where
    /// an `App` context is available.
    pub pending_copy: Option<String>,
    /// Reaction the user asked about: message, emoji, and who reacted.
    pub reaction_users: Option<(Id<marker::MessageMarker>, String, Vec<String>)>,
    /// Quick switcher, open while jumping to a channel.
    pub switcher: Option<Switcher>,
    /// Forum being browsed, when the open channel is a forum.
    pub forum: Option<ForumView>,
    /// Capture-source picker, open while choosing what to share.
    pub stream_picker: Option<StreamPicker>,
    /// True while this client is broadcasting.
    pub broadcasting: bool,
    /// Scope of the joined connection, retained so leaving works after the
    /// user navigates away from the channel they joined.
    voice_scope_joined: Option<VoiceScope>,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// Audio device picker: input and output devices as (id, label), with the
    /// current selections. `None` while the picker is closed.
    pub audio_devices: Option<AudioDevices>,
    /// Whether this client is allowed to transmit. Distinct from self-mute:
    /// mute is a per-session toggle others can see, while this governs whether
    /// the capture device is opened at all.
    pub allow_microphone_transmit: bool,
    /// Stream being watched, if any.
    pub watching: Option<(Id<marker::UserMarker>, String)>,
    /// Sequence number for device-list requests, so a slow earlier reply
    /// cannot overwrite the list from a later one.
    audio_sources_request: u64,
    /// Sequence number for inbox-context requests.
    inbox_history_request: u64,
    /// Content of sends still in flight, keyed by nonce, so a rejected send
    /// can be put back in the composer rather than lost.
    pending_sends: std::collections::HashMap<Id<marker::MessageMarker>, String>,
    /// Downloads in progress: id to (filename, fraction complete).
    pub downloads: Vec<(AttachmentDownloadId, String, Option<f32>)>,
    /// Autocomplete choices offered by a bot for the argument being typed.
    pub command_choices: Vec<String>,
    /// Decoded image attachments, keyed by URL.
    ///
    /// Fetched through the core rather than by GPUI's URL loader so the
    /// request goes out with the session's headers and lands in the core's
    /// cache, the same path the TUI uses.
    pub attachment_previews: std::collections::HashMap<String, std::sync::Arc<gpui::Image>>,
    /// URLs already requested, so a reprojection does not re-ask on every
    /// snapshot revision.
    requested_previews: std::collections::HashSet<String>,
    /// What the open switcher will do with its selection.
    switcher_purpose: SwitcherPurpose,
    /// Invite being previewed, once resolved or while resolving.
    pub invite: Option<InviteState>,
    /// Custom status as last set, shown in the status bar.
    pub custom_status: String,
    /// Custom status being typed, when the editor is open.
    pub editing_status: Option<Composer>,
    /// The user's keymap, shared with the TUI.
    pub keymap: Keymap,
    /// Complaints from the config parsers, shown once at startup.
    pub config_warnings: Vec<String>,
    /// Participants muted locally, which no one else can see.
    pub locally_muted: std::collections::HashSet<Id<marker::UserMarker>>,
    /// A one-line prompt awaiting input: what it is for, and the text so far.
    pub prompt: Option<(Prompt, Composer)>,
    /// Folder being renamed, with the new name as typed.
    pub renaming_folder: Option<(u64, Composer)>,
    /// Key of the avatar preview being awaited, if any.
    pub pending_avatar: Option<String>,
    /// Threads a preview has already been requested for.
    thread_previews: std::collections::HashSet<Id<marker::ChannelMarker>>,
    /// Search state. `None` when the search panel is closed.
    pub search: Option<Search>,
    /// Emoji picker, anchored to the message being reacted to.
    pub picker: Option<EmojiPicker<Id<marker::MessageMarker>>>,
    /// Profile panel target, and the projected profile once it arrives.
    pub profile: Option<(Id<marker::UserMarker>, Option<ProfileView>)>,
    /// Whether the window has focus. Notifications for the channel being read
    /// are suppressed only while it does.
    pub window_focused: bool,
    /// Loaded config, mutated by the settings panel and persisted on change.
    pub options: AppOptions,
    /// Whether the settings panel is open.
    /// Result of the last persistence attempt, surfaced in the panel.
    pub settings_note: Option<String>,
    /// Composer for editing custom Discord base URL.
    /// Files staged for the next send.
    pub attachments: Vec<MessageAttachmentUpload>,
    /// Reason the last staging attempt failed, shown above the composer.
    pub attachment_error: Option<String>,
    /// Last snapshot of core state, stored so local navigation (guild/channel switching)
    /// can immediately re-project the model without waiting for an async roundtrip.
    pub last_state: Option<std::sync::Arc<concord::discord::DiscordState>>,
    focus: FocusHandle,
}

impl Workspace {
    pub fn new(model: WorkspaceModel, screen: Screen, cx: &mut Context<Self>) -> Self {
        // Warnings are kept, not discarded. The parsers are deliberately
        // tolerant - one bad line is skipped rather than failing the file -
        // which means a typo silently does nothing unless it is reported.
        let (options, mut config_warnings) =
            config::load_options_with_warnings().unwrap_or_default();
        // Keymap warnings join the others: a bad binding is skipped by the
        // parser, so without this it would silently not exist.
        let keymap = Keymap::load();
        config_warnings.extend(keymap.warnings.iter().cloned());

        // theme.toml, applied over the built-in palettes. Once per process:
        // active() is read thousands of times per frame.
        config_warnings.extend(theme::load_overrides());

        let ui_state = match config::load_ui_state_options_with_warnings() {
            Ok((ui_state, warnings)) => {
                config_warnings.extend(warnings);
                ui_state
            }
            Err(_) => Default::default(),
        };

        Self {
            screen,
            model,
            handle: None,
            nav: Navigation::default(),
            messages: Vec::new(),
            composer: Composer::default(),
            typing: Vec::new(),
            replying_to: None,
            reply_ping: true,
            editing: None,
            voice_channel: None,
            voice_scope_joined: None,
            channel_muted: false,
            guild_muted: false,
            thread_flags: 0,
            last_typing: None,
            status: PresenceStatus::Online,
            send_as_tts: false,
            confirming: None,
            selected_message: None,
            focus_pane: Pane::Channels,
            pane_filter: None,
            debug_log: None,
            message_scroll: gpui::ScrollHandle::new(),
            ui_state,
            app_commands: Vec::new(),
            current_user: None,
            slash: None,
            inbox: None,
            pins: None,
            pending_copy: None,
            reaction_users: None,
            switcher: None,
            forum: None,
            stream_picker: None,
            broadcasting: false,
            audio_devices: None,
            audio_sources_request: 0,
            inbox_history_request: 0,
            attachment_previews: std::collections::HashMap::new(),
            requested_previews: std::collections::HashSet::new(),
            switcher_purpose: SwitcherPurpose::Navigate,
            invite: None,
            custom_status: String::new(),
            editing_status: None,
            keymap,
            config_warnings,
            locally_muted: std::collections::HashSet::new(),
            prompt: None,
            pending_sends: std::collections::HashMap::new(),
            downloads: Vec::new(),
            command_choices: Vec::new(),
            renaming_folder: None,
            pending_avatar: None,
            thread_previews: std::collections::HashSet::new(),
            allow_microphone_transmit: true,
            watching: None,
            self_mute: false,
            self_deaf: false,
            search: None,
            picker: None,
            profile: None,
            window_focused: true,
            options,
            settings_note: None,
            attachments: Vec::new(),
            attachment_error: None,
            last_state: None,
            focus: cx.focus_handle(),
        }
    }

    pub fn open_settings_window(&mut self, cx: &mut Context<Self>) {
        let options = self.options.clone();
        let bounds = gpui::Bounds::centered(None, gpui::size(px(600.), px(650.)), cx);

        // The window edits its own copy, so it needs a way back: without this
        // the live client keeps stale settings until restart, and a later
        // workspace save would overwrite the window's changes.
        let entity = cx.entity();
        let on_change: OnChange = std::rc::Rc::new(move |options, cx| {
            entity.update(cx, |workspace, cx| {
                workspace.options = options.clone();
                cx.notify();
            });
        });

        let _ = cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|cx| SettingsWindow::new(options, cx).on_change(on_change)),
        );
    }

    /// Sign out: drop the session and return to the login screen.
    ///
    /// The stored credential is deleted too. A sign-out that left the token on
    /// disk would silently log back in on the next launch, which is the
    /// opposite of what the action means.
    pub fn sign_out(&mut self, cx: &mut Context<Self>) {
        // Told to the server first: a purely local sign-out leaves the session
        // alive on Discord's side.
        if let Some(handle) = &self.handle {
            handle.send(AppCommand::SignOut);
        }

        let _ = token_store::delete_token(self.options.credentials.store);

        // Drop everything session-scoped, so nothing from the old account is
        // visible behind the login screen.
        self.handle = None;
        self.last_state = None;
        self.messages.clear();
        self.model = WorkspaceModel::empty();
        self.nav = Navigation::default();
        self.current_user = None;
        self.profile = None;
        self.inbox = None;
        self.pins = None;
        self.search = None;
        self.switcher = None;
        self.forum = None;
        self.voice_channel = None;
        self.voice_scope_joined = None;
        self.composer.clear();
        self.attachments.clear();

        self.screen = Screen::Login(Box::default());
        cx.notify();
    }

    /// Open the authenticated user's own profile.
    pub fn open_own_profile(&mut self) {
        if let Some(user_id) = self.current_user {
            self.open_profile(user_id);
        }
    }

    /// Hand the draft to an external editor and take back what it returns.
    ///
    /// The editor blocks until it exits, so it runs on the shared runtime
    /// rather than GPUI's thread - editing in place would freeze the window
    /// for as long as the editor was open.
    fn compose_externally(&mut self, cx: &mut Context<Self>) {
        let draft = self.composer.text().to_string();
        let entity = cx.entity();

        let spawned = crate::runtime::spawn(async move {
            let result = tokio::task::spawn_blocking(move || crate::editor::edit(&draft)).await;
            (entity, result)
        });

        let Some(task) = spawned else {
            self.model.status_line = "Could not start the editor".to_string();
            return;
        };

        cx.spawn(async move |_workspace, cx| {
            let Ok((entity, result)) = task.await else {
                return;
            };

            let _ = cx.update(|cx| {
                entity.update(cx, |workspace, cx| {
                    match result {
                        Ok(Ok(edited)) => workspace.composer.set_text(&edited),
                        Ok(Err(error)) => workspace.model.status_line = error.message(),
                        // The blocking task panicked or was cancelled; the
                        // draft is untouched either way.
                        Err(_) => {}
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    /// Refresh slash autocomplete after the composer changes.
    fn refresh_slash(&mut self) {
        self.slash = SlashPicker::for_input(self.composer.text(), &self.app_commands);

        // Past the command name the picker closes, and completion becomes the
        // bot's job rather than ours.
        if self.slash.is_none() {
            self.request_command_autocomplete();
        }
    }

    /// Accept the highlighted completion.
    fn accept_slash(&mut self) {
        if let Some(replacement) = self.slash.as_ref().and_then(|picker| picker.completion()) {
            self.composer.set_text(&replacement);
        }
        self.slash = None;
    }

    /// Dispatch a builtin slash command, if the content is one.
    ///
    /// Parsing lives in the core so the GUI and TUI accept the same syntax.
    /// Returns true when the content was handled as a command.
    fn dispatch_slash(&mut self, content: &str, channel_id: Id<marker::ChannelMarker>) -> bool {
        let Some(handle) = &self.handle else {
            return false;
        };

        match parse_builtin_slash_command(content) {
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Message {
                content,
                tts,
            }) => {
                if tts {
                    handle.send(AppCommand::SendTtsMessage {
                        channel_id,
                        nonce: next_message_nonce(),
                        content,
                    });
                } else {
                    handle.send(AppCommand::SendMessage {
                        channel_id,
                        nonce: next_message_nonce(),
                        content,
                        reply_to: None,
                        attachments: Vec::new(),
                    });
                }
                true
            }
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Nickname { nickname }) => {
                // Nicknames are per guild; in a DM there is nothing to rename.
                let Selection::Guild(guild_id) = self.nav.selection else {
                    self.model.status_line = "Nicknames only apply inside a server".to_string();
                    return true;
                };
                let Some(user_id) = self.current_user else {
                    return true;
                };

                handle.send(AppCommand::UpdateUserProfile {
                    update: UserProfileUpdate {
                        user_id,
                        guild_id: Some(guild_id),
                        global: GlobalUserProfileUpdate::default(),
                        guild: Some(GuildUserProfileUpdate {
                            guild_id,
                            nickname: Some(nickname),
                            pronouns: None,
                        }),
                    },
                });
                true
            }
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Unsupported { message }) => {
                // Reported rather than silently swallowed: a command that
                // looks accepted but does nothing is worse than a refusal.
                self.model.status_line = message;
                true
            }
            // Not a builtin: it may still be a bot's command.
            BuiltinSlashCommandParse::Incomplete | BuiltinSlashCommandParse::NotBuiltin => {
                self.dispatch_application_command(content, channel_id)
            }
        }
    }

    /// Run a bot-provided slash command, if the content names one.
    fn dispatch_application_command(
        &mut self,
        content: &str,
        channel_id: Id<marker::ChannelMarker>,
    ) -> bool {
        let name = content
            .strip_prefix('/')
            .and_then(|rest| rest.split_whitespace().next())
            .map(str::to_string);
        let Some(name) = name else {
            return false;
        };

        let Some(command) = self
            .app_commands
            .iter()
            .find(|candidate| candidate.name == name)
            .cloned()
        else {
            return false;
        };

        // Incomplete arguments are left in the composer rather than sent: the
        // server would reject them, and clearing the input would lose what the
        // user typed.
        if !application_command_content_is_complete(content, &command) {
            self.composer.set_text(content);
            self.model.status_line = format!("/{name} needs more arguments", name = command.name);
            return true;
        }

        let Some(handle) = &self.handle else {
            return true;
        };

        handle.send(AppCommand::RunApplicationCommand {
            invocation: ApplicationCommandInvocation {
                guild_id: match self.nav.selection {
                    Selection::Guild(id) => Some(id),
                    Selection::DirectMessages => None,
                },
                channel_id,
                command_identity: Some(command.identity()),
                command_name: command.name.clone(),
                content: content.to_string(),
            },
        });
        true
    }

    /// Show or hide the debug log.
    ///
    /// Reads the in-memory error ring rather than the file: the file needs
    /// debug logging enabled, while errors are always retained, so the panel
    /// has something useful to show in a default build.
    pub fn toggle_debug_log(&mut self) {
        self.debug_log = match self.debug_log {
            Some(_) => None,
            None => Some(
                concord::logging::error_entries()
                    .iter()
                    .map(|entry| entry.line())
                    .collect(),
            ),
        };
    }

    /// Move the message viewport by a fraction of its height.
    ///
    /// A fraction rather than a fixed pixel count, so half-page means the same
    /// thing on a tall window as on a short one.
    pub fn scroll_by_pages(&mut self, pages: f32) {
        let offset = self.message_scroll.offset();
        let height = self.message_scroll.bounds().size.height;
        self.message_scroll
            .set_offset(gpui::point(offset.x, offset.y + height * pages));
    }

    /// Lock or unlock the open thread, which stops further replies.
    pub fn set_thread_locked(&mut self, locked: bool) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::SetThreadLocked {
            channel_id,
            locked,
            label: String::new(),
        });
    }

    /// Mute the open thread.
    ///
    /// Separate from channel mute: a thread mutes independently of the channel
    /// it lives in, so routing it through SetChannelMuted would silence the
    /// wrong thing.
    pub fn set_thread_muted(&mut self, muted: bool) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::SetThreadMuted {
            channel_id,
            muted,
            duration: Some(MuteDuration::Permanent),
            label: String::new(),
        });
    }

    /// Pin or unpin the open thread within its parent.
    pub fn set_thread_pinned(&mut self, pinned: bool) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        // Existing flags are preserved: the command rewrites the field, so
        // passing zero would clear whatever else Discord had set.
        let current_flags = self.thread_flags;
        handle.send(AppCommand::SetThreadPinned {
            channel_id,
            pinned,
            current_flags,
            label: String::new(),
        });
    }

    /// Rename the open thread.
    pub fn rename_thread(&mut self, name: String) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::EditThread {
            channel_id,
            name,
            applied_tags: Vec::new(),
            // Zero means "unchanged" for both: the command carries the whole
            // thread config, and inventing values would overwrite the real ones.
            rate_limit_per_user: 0,
            auto_archive_duration: 0,
            label: String::new(),
        });
    }

    /// Delete the open thread.
    pub fn delete_thread(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::DeleteThread {
            channel_id,
            label: String::new(),
        });
        // The thread is gone, so stay in its parent rather than on a dead view.
        self.nav.channel = None;
        self.messages.clear();
    }

    /// Create a forum post.
    ///
    /// Forum posts are creatable even though plain threads are not - the core
    /// has CreateForumPost but no thread-creation command.
    pub fn create_forum_post(&mut self, title: String, content: String) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(forum) = self.forum.as_ref().map(|forum| forum.channel_id) else {
            return;
        };

        handle.send(AppCommand::CreateForumPost {
            post: ForumPostCreate {
                channel_id: forum,
                title,
                content,
                applied_tags: Vec::new(),
                attachments: Vec::new(),
            },
        });
    }

    /// Tell the server this client is typing.
    ///
    /// Discord expects roughly one of these every ten seconds while composing,
    /// so it is rate-limited here: sending on every keystroke would be a burst
    /// of requests that reads as abusive traffic.
    fn notify_typing(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };

        let now = std::time::Instant::now();
        let due = self
            .last_typing
            .is_none_or(|last| now.duration_since(last).as_secs() >= 8);
        if !due {
            return;
        }

        self.last_typing = Some(now);
        handle.send(AppCommand::TriggerTyping { channel_id });
    }

    /// Set the presence others see.
    pub fn set_status(&mut self, status: PresenceStatus) {
        let Some(handle) = &self.handle else {
            return;
        };
        self.status = status;
        handle.send(AppCommand::UpdateCurrentUserStatus { status });
    }

    /// Set a custom status line.
    pub fn set_custom_activity(&mut self, text: String) {
        let Some(handle) = &self.handle else {
            return;
        };

        let activities = if text.trim().is_empty() {
            Vec::new()
        } else {
            vec![ActivityInfo {
                kind: ActivityKind::Custom,
                name: "Custom Status".to_string(),
                // Discord carries a custom status in `state`, not `details`;
                // putting it in the wrong field shows nothing to anyone.
                state: Some(text),
                details: None,
                url: None,
                application_id: None,
                emoji: None,
                timestamps: None,
                assets: None,
                party: None,
                buttons: Vec::new(),
            }]
        };

        handle.send(AppCommand::UpdateCurrentUserActivity {
            status: self.status,
            activities,
            // Manual, so the RPC server must not overwrite it with a game.
            track_client_id: None,
        });
    }

    /// Leave the open guild.
    pub fn leave_guild(&mut self) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };
        handle.send(AppCommand::LeaveGuild {
            guild_id,
            label: String::new(),
        });
        self.open_guild(None);
    }

    /// Widen or narrow the focused pane, persisting the width.
    ///
    /// Bounded so a pane cannot be dragged to nothing: a zero-width sidebar
    /// looks like it vanished, and there is no handle left to bring it back.
    pub fn resize_pane(&mut self, delta: i16) {
        let width = match self.focus_pane {
            Pane::Guilds => &mut self.ui_state.server_width,
            Pane::Channels => &mut self.ui_state.channel_list_width,
            Pane::Members => &mut self.ui_state.member_list_width,
            // The log takes what the panes leave, so it has no width of its own.
            Pane::Messages => return,
        };

        *width = (*width as i16 + delta).clamp(120, 480) as u16;

        if let Err(error) = config::save_ui_state_options(&self.ui_state) {
            tracing::debug!("could not save pane width: {error}");
        }
    }

    /// Step the interface scale.
    ///
    /// Applied to the type scale rather than to the window, so layout reflows
    /// at the new size instead of being magnified with it.
    fn adjust_zoom(&mut self, delta: f32) {
        let next = (crate::theme::zoom() + delta).clamp(0.75, 2.0);
        crate::theme::set_zoom(next);
        self.model.status_line = format!("Interface scale {:.0}%", next * 100.0);
    }

    /// Toggle whether messages are sent as text-to-speech.
    fn toggle_tts(&mut self) {
        self.send_as_tts = !self.send_as_tts;
        self.model.status_line = if self.send_as_tts {
            "Next messages send as /tts".to_string()
        } else {
            "Sending normally".to_string()
        };
    }

    /// Set how loudly one participant is played, or mute them locally.
    ///
    /// Local only: this changes playback here, not what anyone else hears.
    fn set_participant_playback(
        &mut self,
        user_id: Id<marker::UserMarker>,
        volume: u16,
        muted: bool,
    ) {
        let Some(handle) = &self.handle else {
            return;
        };
        handle.send(AppCommand::UpdateVoiceParticipantPlayback {
            user_id,
            settings: VoiceParticipantPlaybackSettings {
                volume: VoiceParticipantVolumePercent::new(volume),
                muted,
            },
        });
    }

    /// Step the output volume, persisting it for the next connection.
    ///
    /// The core takes output volume when joining rather than as a standalone
    /// command, so a change applies to the next join; saying so is better than
    /// appearing to do nothing now.
    fn adjust_output_volume(&mut self, delta: i16) {
        let current = self.options.voice.voice_output_volume;
        let next = (current.value() as i16 + delta).clamp(0, 200) as u8;
        self.options.voice.voice_output_volume = VoiceVolumePercent::new(next);

        if let Err(error) = config::save_options(&self.options) {
            self.settings_note = Some(format!("Could not save volume: {error}"));
        }
        self.model.status_line = format!("Output volume {next}% (applies on next connect)");
    }

    /// Set a thread's notification level.
    ///
    /// Threads are the only scope the core can set a level for; guilds and
    /// channels expose mute alone, so those keep the mute control.
    ///
    /// The flags are the thread-specific ones the command documents (2 all,
    /// 4 mentions, 8 nothing), not the `NotificationLevel` codes - the two
    /// vocabularies differ and mixing them would set the wrong level silently.
    pub fn set_thread_notification_level(&mut self, flags: u64) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::SetThreadNotificationLevel {
            channel_id,
            flags,
            label: String::new(),
        });
    }

    /// Carry out the pending confirmation.
    pub fn confirm(&mut self) {
        let Some(pending) = self.confirming.take() else {
            return;
        };
        let Some(row) = self.messages.get(pending.message) else {
            return;
        };
        let message_id = row.id;

        match pending.action {
            ConfirmAction::Delete => self.delete_message(message_id),
            ConfirmAction::Pin => self.set_pinned(pending.message, true),
            ConfirmAction::Unpin => self.set_pinned(pending.message, false),
        }
    }

    /// Open the thread started from a message, if it has one.
    fn open_message_thread(&mut self, index: usize) {
        let Some(thread) = self.messages.get(index).and_then(|row| row.thread) else {
            self.model.status_line = "This message has no thread".to_string();
            return;
        };
        self.forum = None;
        self.open_channel(thread);
    }

    /// Ask for a thread's most recent message, to show under its starter.
    ///
    /// A preview is what makes a thread worth noticing: "3 messages" says
    /// nothing about whether the conversation moved.
    fn request_thread_preview(
        &self,
        channel_id: Id<marker::ChannelMarker>,
        message_id: Id<marker::MessageMarker>,
    ) {
        let Some(handle) = &self.handle else {
            return;
        };
        handle.send(AppCommand::LoadThreadPreview {
            channel_id,
            message_id,
        });
    }

    /// Apply whatever the open prompt was collecting.
    fn submit_prompt(&mut self) {
        let Some((prompt, text)) = self.prompt.take() else {
            return;
        };
        let text = text.text().trim().to_string();
        if text.is_empty() {
            return;
        }

        match prompt {
            Prompt::ThreadName => self.rename_thread(text),
            Prompt::InviteCode => self.resolve_invite(&text),
            Prompt::ForumPostTitle => {
                // The body is the composer's content, so a post is written the
                // same way a message is and the title is the only extra step.
                let body = self.composer.take();
                self.create_forum_post(text, body);
            }
        }
    }

    /// Apply the typed custom status.
    fn submit_custom_status(&mut self) {
        let Some(text) = self.editing_status.take() else {
            return;
        };
        let text = text.text().trim().to_string();
        // An empty string is meaningful here - it clears the status - so it is
        // sent rather than treated as a cancel.
        self.custom_status = text.clone();
        self.set_custom_activity(text);
    }

    /// Apply the typed folder name.
    fn submit_folder_rename(&mut self) {
        let Some((folder_id, name)) = self.renaming_folder.take() else {
            return;
        };
        let name = name.text().trim().to_string();
        if name.is_empty() {
            return;
        }
        // Colour is left alone: the command carries both fields, and passing
        // None means unchanged rather than cleared.
        self.update_guild_folder(folder_id, Some(name), None);
    }

    /// Look up an invite the user pasted.
    pub fn resolve_invite(&mut self, input: &str) {
        let Some(handle) = &self.handle else {
            return;
        };
        // Parsed by the core, so both clients accept the same forms.
        let Some(code) = invite_code_from(input) else {
            self.model.status_line = "That does not look like an invite".to_string();
            return;
        };

        self.invite = Some(InviteState {
            code: code.clone(),
            preview: None,
            error: None,
        });
        handle.send(AppCommand::ResolveInvite { code });
    }

    /// Join the guild the previewed invite points at.
    pub fn accept_invite(&mut self) {
        let (Some(handle), Some(invite)) = (&self.handle, self.invite.as_ref()) else {
            return;
        };
        handle.send(AppCommand::AcceptInvite {
            code: invite.code.clone(),
        });
        // Closed immediately: the guild arrives over the gateway, and leaving
        // the dialog up would invite a second click that joins twice.
        self.invite = None;
    }

    /// Rename or recolour a guild folder.
    pub fn update_guild_folder(
        &mut self,
        folder_id: u64,
        name: Option<String>,
        color: Option<u32>,
    ) {
        let Some(handle) = &self.handle else {
            return;
        };
        handle.send(AppCommand::UpdateGuildFolderSettings {
            folder_id,
            name,
            color,
        });
    }

    /// Ask a bot to complete the argument being typed.
    ///
    /// Only for application commands: builtins are parsed locally and have no
    /// remote side to ask.
    fn request_command_autocomplete(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };

        let content = self.composer.text().to_string();
        let Some(rest) = content.strip_prefix('/') else {
            return;
        };
        let Some((name, _)) = rest.split_once(char::is_whitespace) else {
            // No argument started yet, so there is nothing to complete.
            return;
        };

        let Some(command) = self
            .app_commands
            .iter()
            .find(|command| command.name.eq_ignore_ascii_case(name))
        else {
            return;
        };

        let guild_id = match self.nav.selection {
            Selection::Guild(guild_id) => Some(guild_id),
            Selection::DirectMessages => None,
        };

        handle.send(AppCommand::RequestApplicationCommandAutocomplete {
            invocation: ApplicationCommandAutocompleteInvocation {
                guild_id,
                channel_id,
                command_identity: command.identity(),
                command_version: command.version.clone(),
                command_name: command.name.clone(),
                content,
                // The core resolves which option the cursor sits in; an empty
                // name means "the one being typed".
                focused_option_name: String::new(),
                nonce: next_message_nonce().to_string(),
            },
        });
    }

    /// Move keyboard focus to a pane, showing it first if it is hidden.
    ///
    /// Focusing a pane the user cannot see would send their next keystrokes
    /// somewhere invisible.
    pub fn focus_pane(&mut self, pane: Pane) {
        if !self.pane_visible(pane) {
            self.toggle_pane(pane);
        }
        self.focus_pane = pane;
        // A filter belongs to the pane it was opened on.
        self.pane_filter = None;
    }

    /// Jump to the oldest loaded message.
    pub fn scroll_to_top(&mut self) {
        self.message_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
    }

    /// Jump to the newest message.
    pub fn scroll_to_bottom(&mut self) {
        self.message_scroll.scroll_to_bottom();
    }

    /// Put the caret back in the composer.
    pub fn focus_composer(&mut self) {
        self.focus_pane = Pane::Messages;
        self.pane_filter = None;
        // Any modal would otherwise keep taking the keys that follow.
        self.close_popup();
    }

    /// Dismiss whatever panel is open, innermost first.
    ///
    /// Ordered so one press closes one thing: escape with several panels open
    /// should peel them back, not clear the screen.
    pub fn close_popup(&mut self) {
        if self.invite.take().is_some()
            || self.confirming.take().is_some()
            || self.prompt.take().is_some()
            || self.editing_status.take().is_some()
            || self.renaming_folder.take().is_some()
            || self.picker.take().is_some()
            || self.stream_picker.take().is_some()
            || self.audio_devices.take().is_some()
            || self.reaction_users.take().is_some()
            || self.switcher.take().is_some()
            || self.inbox.take().is_some()
            || self.pane_filter.take().is_some()
        {
            return;
        }

        // Nothing modal is open, so the selection is what escape clears.
        self.clear_message_selection();
    }

    /// Open the action list for whichever pane has focus.
    ///
    /// The TUI shows a popup per pane. Here each pane's actions already sit on
    /// the thing they act on, so this opens the one panel that has no other
    /// route: the message list's action is its selection.
    pub fn open_focused_pane_action(&mut self) {
        match self.focus_pane {
            Pane::Guilds | Pane::Channels => self.toggle_pane_filter(),
            Pane::Messages => self.act_on_selection(MessageAction::React),
            Pane::Members => self.toggle_pane_filter(),
        }
    }

    /// Show who reacted, using the selected message's first reaction.
    ///
    /// The TUI's binding acts on the selection rather than naming a reaction,
    /// so this matches it; the mouse path can still pick any of them.
    pub fn show_first_reaction_users(&mut self) {
        let Some(index) = self.selected_message else {
            return;
        };
        if self
            .messages
            .get(index)
            .is_some_and(|row| !row.reactions.is_empty())
        {
            self.show_reaction_users(index, 0);
        }
    }

    /// Move the message selection, entering the log if not already in it.
    ///
    /// Selection starts at the newest message rather than the oldest: that is
    /// where attention is, and the TUI does the same.
    pub fn move_message_selection(&mut self, delta: isize) {
        if self.messages.is_empty() {
            return;
        }

        let last = self.messages.len() - 1;
        let next = match self.selected_message {
            None => last,
            Some(current) => (current as isize + delta).clamp(0, last as isize) as usize,
        };

        self.selected_message = Some(next);
        // Keep the selection on screen; a selection scrolled out of view is
        // indistinguishable from none.
        self.message_scroll.scroll_to_item(next);
    }

    fn clear_message_selection(&mut self) {
        self.selected_message = None;
    }

    /// Apply an action to the selected message, if any.
    pub fn act_on_selection(&mut self, action: MessageAction) {
        let Some(index) = self.selected_message else {
            return;
        };
        self.handle_message_action(index, action);
    }

    /// Move keyboard focus between panes.
    pub fn cycle_focus(&mut self, forward: bool) {
        // Cycling only visits panes that are actually shown; focusing a hidden
        // pane would silently swallow keys.
        let order = [Pane::Guilds, Pane::Channels, Pane::Messages, Pane::Members];
        let visible: Vec<Pane> = order
            .into_iter()
            .filter(|pane| self.pane_visible(*pane))
            .collect();

        if visible.is_empty() {
            return;
        }

        let current = visible
            .iter()
            .position(|pane| *pane == self.focus_pane)
            .unwrap_or(0) as isize;
        let step = if forward { 1 } else { -1 };
        let next = (current + step).rem_euclid(visible.len() as isize) as usize;

        self.focus_pane = visible[next];
        // A filter belongs to the pane it was opened on.
        self.pane_filter = None;
    }

    fn pane_visible(&self, pane: Pane) -> bool {
        match pane {
            Pane::Guilds => self.ui_state.guild_pane_visible,
            Pane::Channels => self.ui_state.channel_pane_visible,
            // Always focusable: the log is the content area, so there is no
            // state in which it can be hidden.
            Pane::Messages => true,
            Pane::Members => self.ui_state.member_pane_visible && self.shows_members(),
        }
    }

    /// Start or stop filtering the focused pane.
    pub fn toggle_pane_filter(&mut self) {
        self.pane_filter = match self.pane_filter {
            Some(_) => None,
            None => Some(Composer::default()),
        };
    }

    /// Whether a name survives the active filter.
    fn passes_filter(&self, name: &str) -> bool {
        match &self.pane_filter {
            None => true,
            Some(filter) => {
                let needle = filter.text().trim().to_lowercase();
                needle.is_empty() || name.to_lowercase().contains(&needle)
            }
        }
    }

    /// Quit the application.
    ///
    /// Explicit rather than window-close only, since the TUI has a quit key
    /// and muscle memory carries over.
    pub fn quit(&mut self, cx: &mut Context<Self>) {
        cx.quit();
    }

    /// Collapse or expand a channel category.
    fn toggle_category(&mut self, channel_id: Id<marker::ChannelMarker>) {
        let collapsed = &mut self.ui_state.collapsed_channel_categories;
        if let Some(position) = collapsed.iter().position(|id| *id == channel_id) {
            collapsed.remove(position);
        } else {
            collapsed.push(channel_id);
        }

        if let Err(error) = config::save_ui_state_options(&self.ui_state) {
            tracing::debug!("could not save collapsed categories: {error}");
        }
    }

    fn category_collapsed(&self, channel_id: Id<marker::ChannelMarker>) -> bool {
        self.ui_state
            .collapsed_channel_categories
            .contains(&channel_id)
    }

    /// Show or hide a pane, persisting the choice.
    ///
    /// Written through the same ui_state the TUI uses, so a layout chosen in
    /// one client is the layout in the other.
    pub fn toggle_pane(&mut self, pane: Pane) {
        let state = &mut self.ui_state;
        let field = match pane {
            Pane::Guilds => &mut state.guild_pane_visible,
            Pane::Channels => &mut state.channel_pane_visible,
            Pane::Members => &mut state.member_pane_visible,
            // The message log has no visibility toggle; it is the content.
            Pane::Messages => return,
        };
        *field = !*field;

        // Persisted immediately: a layout that reverted on restart would be
        // worse than one that could not be changed.
        if let Err(error) = config::save_ui_state_options(&self.ui_state) {
            tracing::debug!("could not save pane layout: {error}");
        }
    }

    /// Fetch the slash commands available in the open guild.
    fn load_app_commands(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };
        handle.send(AppCommand::LoadApplicationCommands {
            guild_id: match self.nav.selection {
                Selection::Guild(id) => Some(id),
                Selection::DirectMessages => None,
            },
        });
    }

    /// Send the composer's contents to the open channel.
    ///
    /// The nonce lets the core match the gateway echo back to this send, so
    /// the message does not briefly appear twice.
    fn send_message(&mut self) {
        let Some(channel_id) = self.nav.channel else {
            return;
        };
        if self.handle.is_none() {
            return;
        }

        let content = self.composer.take();
        self.slash = None;

        // A message may be attachments only, but never entirely empty.
        if content.trim().is_empty() && self.attachments.is_empty() {
            return;
        }

        // A builtin command consumes the input instead of sending it. Done
        // before the handle is borrowed, since dispatch needs &mut self.
        if self.dispatch_slash(&content, channel_id) {
            self.attachments.clear();
            return;
        }

        let Some(handle) = &self.handle else {
            return;
        };

        if let Some(message_id) = self.editing.take() {
            handle.send(AppCommand::EditMessage {
                channel_id,
                message_id,
                content,
            });
            return;
        }

        let reply_to = self
            .replying_to
            .take()
            .map(|(message_id, _)| ReplyReference {
                message_id,
                mention_author: self.reply_ping,
            });

        let nonce = next_message_nonce();
        // Kept so a failure can return the text to the composer. Discord
        // rejects sends for reasons the client cannot predict (slowmode,
        // permissions, filters), and without this the message is simply gone.
        self.pending_sends.insert(nonce, content.clone());

        if self.send_as_tts && reply_to.is_none() && self.attachments.is_empty() {
            // TTS has no reply or attachment form, so it applies only to a
            // plain message rather than silently dropping either.
            handle.send(AppCommand::SendTtsMessage {
                channel_id,
                nonce,
                content,
            });
        } else {
            handle.send(AppCommand::SendMessage {
                channel_id,
                nonce,
                content,
                reply_to,
                attachments: std::mem::take(&mut self.attachments),
            });
        }
        self.attachment_error = None;
    }

    /// Attach the command sink once the session thread is running.
    pub fn attach(&mut self, handle: SessionHandle) {
        self.handle = Some(handle);
    }

    /// Reproject the view model and message list from the cached core state.
    pub fn reproject(&mut self) {
        let Some(state) = &self.last_state else {
            return;
        };
        self.model = projection::project(state, &self.nav, true);
        let guild_id = match self.nav.selection {
            Selection::Guild(id) => Some(id),
            Selection::DirectMessages => None,
        };

        if let Some((user_id, _)) = self.profile {
            let view = projection::project_profile(state, user_id, guild_id);
            self.profile = Some((user_id, view));
        }

        (self.messages, self.typing) = match self.nav.channel {
            Some(channel_id) => (
                message::project_messages(state, channel_id, state.current_user_id()),
                projection::typing_names(state, channel_id, guild_id),
            ),
            None => (Vec::new(), Vec::new()),
        };

        // Previews for threads visible in the log. Requested once each: the
        // reprojection runs on every snapshot, and re-asking each time would
        // be a request per thread per state change.
        let pending: Vec<_> = self
            .messages
            .iter()
            .filter_map(|row| row.thread.map(|thread| (thread, row.id)))
            .filter(|(thread, _)| self.thread_previews.insert(*thread))
            .collect();
        for (thread, message_id) in pending {
            self.request_thread_preview(thread, message_id);
        }

        // Image attachments in view. Gated on the display options, so turning
        // previews off stops the fetch rather than only hiding the result.
        if self.options.display.show_images && !self.options.display.disable_image_preview {
            let urls: Vec<_> = self
                .messages
                .iter()
                .flat_map(|row| row.attachments.iter())
                .filter(|attachment| attachment.is_image && !attachment.url.is_empty())
                .map(|attachment| attachment.url.clone())
                .filter(|url| self.requested_previews.insert(url.clone()))
                .collect();

            for url in urls {
                if let Some(handle) = &self.handle {
                    handle.send(AppCommand::LoadAttachmentPreview { url });
                }
            }
        }

        if let Some((voice_channel_id, _)) = &self.voice_channel
            && let Some(channel) = self
                .model
                .channels
                .iter_mut()
                .find(|c| c.id == Some(*voice_channel_id))
        {
            let user_name = self
                .last_state
                .as_ref()
                .and_then(|s| s.current_user())
                .unwrap_or("You")
                .to_string();

            if let Some(member) = channel
                .voice
                .iter_mut()
                .find(|m| m.name == user_name || m.name == "You")
            {
                member.muted = self.self_mute;
                member.deafened = self.self_deaf;
            } else {
                channel.voice.push(VoiceMember {
                    user_id: self.current_user.unwrap_or(Id::new(1)),
                    name: user_name,
                    muted: self.self_mute,
                    deafened: self.self_deaf,
                    streaming: false,
                    speaking: !self.self_mute,
                });
            }
        }
    }

    /// Drain the bridge's update stream on the foreground executor, reprojecting
    /// the view model whenever the core's state store advances.
    pub fn pump(
        window: WindowHandle<Workspace>,
        mut updates: mpsc::UnboundedReceiver<Update>,
        cx: &mut gpui::App,
    ) {
        cx.spawn(async move |cx| {
            while let Some(update) = updates.recv().await {
                let applied = window.update(cx, |workspace, _window, cx| {
                    match update {
                        Update::State(state) => {
                            workspace.last_state = Some(state);
                            workspace.reproject();
                        }
                        Update::Event(event, state) => {
                            // The core owns the mute/mention rules; the GUI
                            // only adds "not the channel you are reading".
                            if workspace.options.notifications.desktop_notifications
                                && let Some(notification) = notify::notification_for(
                                    &state,
                                    &event,
                                    workspace.nav.channel,
                                    workspace.window_focused,
                                )
                            {
                                notify::deliver(&notification);
                            }
                            workspace.absorb(*event);
                        }
                        Update::Closed(reason) => {
                            workspace.model.connected = false;
                            workspace.model.status_line =
                                reason.unwrap_or_else(|| "session closed".to_string());
                        }
                    }
                    cx.notify();
                });

                if applied.is_err() {
                    // Window is gone; stop pumping.
                    break;
                }
            }
        })
        .detach();
    }

    /// Switch the open channel.
    ///
    /// Three commands are needed: the core tracks its own notion of the
    /// selected channel (for read-state and typing), history must be requested
    /// because the cache is lazily populated, and a gateway subscription is
    /// required before Discord will push updates for it.
    pub fn open_channel(&mut self, channel_id: Id<marker::ChannelMarker>) {
        self.nav.channel = Some(channel_id);
        self.messages.clear();

        if let Some(handle) = &self.handle {
            handle.send(AppCommand::SetSelectedMessageChannel {
                channel_id: Some(channel_id),
            });
            handle.send(AppCommand::LoadMessageHistory {
                channel_id,
                before: None,
            });

            match self.nav.selection {
                Selection::Guild(guild_id) => {
                    handle.send(AppCommand::SubscribeGuildChannel {
                        guild_id,
                        channel_id,
                    });
                    // Discord streams the member list in windowed ranges; the
                    // first two cover what fits on screen without over-fetching.
                    handle.send(AppCommand::UpdateMemberListSubscription {
                        guild_id,
                        channel_id,
                        ranges: vec![(0, 99), (100, 199)],
                    });
                }
                Selection::DirectMessages => {
                    handle.send(AppCommand::SubscribeDirectMessage { channel_id });
                }
            }
        }

        self.reproject();
    }

    /// Advance the login state machine based on a user action in the current sub-screen.
    ///
    /// Called from the key handler with a `LoginAction` that describes what
    /// the user just did (submit, back, pick a method, etc.).
    pub(crate) fn handle_login_action(
        &mut self,
        action: LoginAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            // -- Picker: user chose a method --------------------------------
            LoginAction::PickPassword => {
                if let Screen::Login(l) = &mut self.screen {
                    l.screen = LoginScreen::Password;
                    l.error = None;
                }
            }
            LoginAction::PickToken => {
                if let Screen::Login(l) = &mut self.screen {
                    l.screen = LoginScreen::Token;
                    l.error = None;
                }
            }
            LoginAction::PickQr => {
                if let Screen::Login(l) = &mut self.screen {
                    // Abort any stale handle.
                    l.handle = None;
                    l.qr.reset();
                    l.screen = LoginScreen::QrScan;
                    l.error = None;

                    let rx = crate::session::spawn_qr_login();
                    l.handle = Some(LoginHandle {
                        rx: Self::wrap_qr(rx),
                    });

                    if let Some(wh) = window.window_handle().downcast::<Workspace>() {
                        Workspace::pump_login(wh, cx);
                    }
                }
            }
            LoginAction::PickDemo => {
                if cfg!(feature = "fixtures") {
                    self.start_token_session("test".to_string(), false, window, cx);
                } else if let Screen::Login(login) = &mut self.screen {
                    // Reachable through the keyboard shortcut even when the
                    // button is hidden, so it explains itself rather than
                    // failing as a bad credential.
                    login.error = Some(
                        "This build has no demo data. Rebuild with --features fixtures."
                            .to_string(),
                    );
                }
            }

            // -- Back: return to picker -------------------------------------
            LoginAction::Back => {
                if let Screen::Login(l) = &mut self.screen {
                    // Abort any running auth task.
                    l.handle = None;
                    l.screen = LoginScreen::Picker;
                    l.error = None;
                }
            }

            // -- Token screen: submit ---------------------------------------
            LoginAction::SubmitToken => {
                let Screen::Login(login) = &mut self.screen else {
                    return;
                };
                if !login.token_submittable() {
                    return;
                }
                let token = login.token.take();
                let remember = login.remember;
                self.start_token_session(token, remember, window, cx);
            }

            // -- Password screen: submit ------------------------------------
            LoginAction::SubmitPassword => {
                let Screen::Login(login) = &mut self.screen else {
                    return;
                };
                if !login.password.is_submittable() {
                    return;
                }
                let login_id = login.password.login.text().to_string();
                let pw = login.password.password.text().to_string();
                login.password.in_progress = true;
                login.password.status = "Authenticating with Discord…".to_string();
                login.error = None;

                let rx = crate::session::spawn_password_login(login_id, pw);
                login.handle = Some(LoginHandle {
                    rx: Self::wrap_password(rx),
                });

                if let Some(wh) = window.window_handle().downcast::<Workspace>() {
                    Workspace::pump_login(wh, cx);
                }
            }

            // -- MFA select: user picked a method ---------------------------
            LoginAction::PickMfaMethod(method) => {
                let Screen::Login(login) = &mut self.screen else {
                    return;
                };
                let Some(challenge) = login.password.mfa.clone() else {
                    return;
                };
                match method {
                    MfaMethod::Totp => {
                        login.password.mfa_method = Some(MfaMethod::Totp);
                        login.password.status =
                            "Enter the 6-digit code from your authenticator app.".to_string();
                        login.screen = LoginScreen::MfaCode;
                    }
                    MfaMethod::Sms => {
                        // Ask Discord to send the SMS first.
                        login.password.in_progress = true;
                        login.password.status = "Requesting SMS code…".to_string();
                        login.error = None;

                        let rx = crate::session::spawn_sms_send(challenge.ticket.clone());
                        login.handle = Some(LoginHandle {
                            rx: Self::wrap_password(rx),
                        });

                        if let Some(wh) = window.window_handle().downcast::<Workspace>() {
                            Workspace::pump_login(wh, cx);
                        }
                    }
                }
            }

            // -- MFA code: user submitted the code --------------------------
            LoginAction::SubmitMfaCode => {
                let Screen::Login(login) = &mut self.screen else {
                    return;
                };
                if !login.password.is_mfa_submittable() {
                    return;
                }
                let Some(challenge) = login.password.mfa.clone() else {
                    return;
                };
                let Some(method) = login.password.mfa_method else {
                    return;
                };
                let code = login.password.mfa_code.text().to_string();
                login.password.in_progress = true;
                login.password.status = "Verifying…".to_string();
                login.error = None;

                let rx = crate::session::spawn_mfa_verify(
                    method,
                    code,
                    challenge.ticket.clone(),
                    challenge.login_instance_id.clone(),
                );
                login.handle = Some(LoginHandle {
                    rx: Self::wrap_password(rx),
                });

                if let Some(wh) = window.window_handle().downcast::<Workspace>() {
                    Workspace::pump_login(wh, cx);
                }
            }

            // -- Toggle remember --------------------------------------------
            LoginAction::ToggleRemember => {
                if let Screen::Login(l) = &mut self.screen {
                    l.remember = !l.remember;
                }
            }
        }
    }

    /// Convert a `PasswordAuthEvent` receiver into the unified `LoginEvent` channel.
    fn wrap_password(rx: mpsc::Receiver<PasswordAuthEvent>) -> mpsc::Receiver<LoginEvent> {
        let (tx, out) = mpsc::channel(8);
        // Shared runtime: this runs on GPUI's thread, which has no reactor.
        let _ = crate::runtime::spawn(async move {
            let mut rx = rx;
            while let Some(ev) = rx.recv().await {
                if tx.send(LoginEvent::Password(ev)).await.is_err() {
                    break;
                }
            }
        });
        out
    }

    /// Convert a `QrEvent` receiver into the unified `LoginEvent` channel.
    fn wrap_qr(rx: mpsc::Receiver<QrEvent>) -> mpsc::Receiver<LoginEvent> {
        let (tx, out) = mpsc::channel(8);
        // Shared runtime: this runs on GPUI's thread, which has no reactor.
        let _ = crate::runtime::spawn(async move {
            let mut rx = rx;
            while let Some(ev) = rx.recv().await {
                if tx.send(LoginEvent::Qr(ev)).await.is_err() {
                    break;
                }
            }
        });
        out
    }

    /// Drain the active login auth handle's event stream on GPUI's executor.
    ///
    /// Starts the token session as soon as a `Token` event arrives, or
    /// advances the MFA / QR state machine for intermediate events.
    fn pump_login(window: WindowHandle<Workspace>, cx: &mut gpui::App) {
        cx.spawn(async move |cx| {
            loop {
                // Peek: is there still a handle and does it have an event?
                let event = window.update(cx, |workspace, _window, _cx| {
                    let Screen::Login(login) = &mut workspace.screen else {
                        return None;
                    };
                    // Try to receive without blocking. We'll re-schedule if empty.
                    login.handle.as_mut().and_then(|h| h.rx.try_recv().ok())
                });

                match event {
                    Err(_) => break, // window gone
                    Ok(None) => {
                        // Nothing yet – yield to other GPUI work and try again
                        // via a small async sleep so we don't busy-spin.
                        // Use recv() properly by driving from a spawn.
                        // We reschedule ourselves in 16ms.
                        tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                        continue;
                    }
                    Ok(Some(event)) => {
                        let done = window.update(cx, |workspace, win, cx| {
                            workspace.apply_login_event(event, win, cx)
                        });
                        match done {
                            Err(_) => break,
                            Ok(true) => break, // session started or fatal error
                            Ok(false) => {}    // keep pumping
                        }
                    }
                }
            }
        })
        .detach();
    }

    /// Apply a single `LoginEvent` to the login state.
    ///
    /// Returns `true` when pumping should stop (session started or unrecoverable).
    fn apply_login_event(
        &mut self,
        event: LoginEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Screen::Login(login) = &mut self.screen else {
            return true;
        };

        match event {
            // ---- Password / MFA events ------------------------------------
            LoginEvent::Password(pev) => match pev {
                PasswordAuthEvent::Status(s) => {
                    login.password.status = s;
                    cx.notify();
                    false
                }
                PasswordAuthEvent::Token(token) => {
                    login.password.reset_sensitive();
                    login.handle = None;
                    let remember = login.remember;
                    cx.notify();
                    self.start_token_session(token, remember, window, cx);
                    true
                }
                PasswordAuthEvent::Failed(reason) => {
                    login.password.in_progress = false;
                    login.password.status.clear();
                    login.error = Some(format!("Login failed: {reason}"));
                    login.handle = None;
                    cx.notify();
                    false
                }
                PasswordAuthEvent::MfaRequired(challenge) => {
                    login.password.in_progress = false;
                    login.password.password.clear();
                    login.password.mfa = Some(challenge);
                    login.password.mfa_method = None;
                    login.password.mfa_code.clear();
                    login.password.status =
                        "Choose a two-factor authentication method.".to_string();
                    login.screen = LoginScreen::MfaSelect;
                    login.handle = None;
                    cx.notify();
                    false
                }
                PasswordAuthEvent::SmsSent { phone } => {
                    login.password.in_progress = false;
                    login.password.mfa_method = Some(MfaMethod::Sms);
                    login.password.mfa_code.clear();
                    login.password.status = match phone {
                        Some(p) => format!("SMS sent to {p}. Enter the code below."),
                        None => "SMS sent. Enter the code below.".to_string(),
                    };
                    login.screen = LoginScreen::MfaCode;
                    login.handle = None;
                    cx.notify();
                    false
                }
                PasswordAuthEvent::RequiredActions(actions) => {
                    login.password.reset_sensitive();
                    login.handle = None;
                    let list = actions
                        .into_iter()
                        .map(|a| match a.as_str() {
                            "update_password" => "update your account password".to_owned(),
                            other => other.to_owned(),
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    login.error = Some(format!(
                        "Discord requires you to {list} in the official client before Concord can log in."
                    ));
                    cx.notify();
                    false
                }
            },

            // ---- QR events -----------------------------------------------
            LoginEvent::Qr(qev) => match qev {
                QrEvent::Status(s) => {
                    login.qr.status = s;
                    cx.notify();
                    false
                }
                QrEvent::QrBitmap(bm) => {
                    login.qr.bitmap = Some(bm);
                    cx.notify();
                    false
                }
                QrEvent::UserPending {
                    username,
                    discriminator,
                } => {
                    let display = if discriminator == "0" {
                        username
                    } else {
                        format!("{username}#{discriminator}")
                    };
                    login.qr.pending_user = Some(display);
                    cx.notify();
                    false
                }
                QrEvent::Token(token) => {
                    login.handle = None;
                    let remember = login.remember;
                    cx.notify();
                    self.start_token_session(token, remember, window, cx);
                    true
                }
                QrEvent::Cancelled => {
                    login.handle = None;
                    login.qr.reset();
                    login.screen = LoginScreen::Picker;
                    login.error =
                        Some("QR login was cancelled in the Discord mobile app.".to_string());
                    cx.notify();
                    false
                }
                QrEvent::Failed(reason) => {
                    login.handle = None;
                    login.qr.reset();
                    login.screen = LoginScreen::Picker;
                    login.error = Some(format!("QR login failed: {reason}"));
                    cx.notify();
                    false
                }
            },
        }
    }

    /// Spawn the core session from a resolved token and transition to `Screen::Ready`.
    fn start_token_session(
        &mut self,
        token: String,
        remember: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if remember {
            let _ = token_store::save_token(&token, CredentialStoreMode::default());
        }
        match crate::session::spawn(token) {
            Ok((updates, handle)) => {
                self.attach(handle);
                self.screen = Screen::Ready;
                self.model.status_line = "connecting…".to_string();
                if let Some(wh) = window.window_handle().downcast::<Workspace>() {
                    Workspace::pump(wh, updates, cx);
                }
            }
            Err(error) => {
                if let Screen::Login(login) = &mut self.screen {
                    login.error = Some(format!("could not start session: {error}"));
                }
            }
        }
    }

    /// Open the channel containing a search hit and load history around it.
    ///
    /// The surrounding history matters: jumping to a message with nothing
    /// above or below it gives no context for why it matched.
    pub fn jump_to(
        &mut self,
        channel_id: Id<marker::ChannelMarker>,
        message_id: Id<marker::MessageMarker>,
    ) {
        if self.nav.channel != Some(channel_id) {
            self.open_channel(channel_id);
        }

        if let Some(handle) = &self.handle {
            handle.send(AppCommand::LoadMessageHistoryAround {
                channel_id,
                message_id,
            });
        }

        self.search = None;
    }

    /// Open or close the search panel.
    pub fn toggle_search(&mut self) {
        self.search = match self.search {
            Some(_) => None,
            None => Some(Search::default()),
        };
    }

    /// Run the current search query, scoped to the open guild.
    pub fn run_search(&mut self) {
        let Some(search) = &mut self.search else {
            return;
        };
        let content = search.input.text().trim().to_string();
        if content.is_empty() {
            return;
        }

        search.running = true;
        search.error = None;
        search.results.clear();

        let Some(handle) = &self.handle else {
            return;
        };

        handle.send(AppCommand::SearchMessages {
            query: MessageSearchQuery {
                guild_id: match self.nav.selection {
                    Selection::Guild(id) => Some(id),
                    Selection::DirectMessages => None,
                },
                // A DM search has no guild, so it is scoped to the open
                // channel instead; otherwise Discord rejects the query.
                channel_id: match self.nav.selection {
                    Selection::DirectMessages => self.nav.channel,
                    Selection::Guild(_) => None,
                },
                content: Some(content),
                ..Default::default()
            },
        });
    }

    /// The voice scope for a channel: guild channels are guild-scoped, DM and
    /// group-DM calls are private-scoped to the channel itself.
    fn voice_scope(&self, channel_id: Id<marker::ChannelMarker>) -> VoiceScope {
        match self.nav.selection {
            Selection::Guild(guild_id) => VoiceScope::Guild(guild_id),
            Selection::DirectMessages => VoiceScope::Private(channel_id),
        }
    }

    /// Open the audio device picker, asking the core for the device list.
    ///
    /// The list is fetched rather than cached: devices appear and disappear
    /// while the app runs, and a stale list offers a device that is gone.
    pub fn open_audio_devices(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };

        self.audio_sources_request = self.audio_sources_request.wrapping_add(1);
        self.audio_devices = Some(AudioDevices::default());
        handle.send(AppCommand::LoadVoiceAudioSources {
            request_id: self.audio_sources_request,
        });
    }

    /// Select an input or output device.
    pub fn set_audio_device(&mut self, input: Option<String>, output: Option<String>) {
        let Some(handle) = &self.handle else {
            return;
        };

        if let Some(devices) = &mut self.audio_devices {
            if input.is_some() {
                devices.selected_input = input.clone();
            }
            if output.is_some() {
                devices.selected_output = output.clone();
            }
        }

        handle.send(AppCommand::UpdateVoiceAudioSources {
            input_source: input,
            output_source: output,
        });
    }

    /// Allow or block microphone transmission for the joined connection.
    pub fn set_microphone_allowed(&mut self, allowed: bool) {
        self.allow_microphone_transmit = allowed;

        let (Some(handle), Some(scope), Some((channel_id, _))) = (
            &self.handle,
            self.voice_scope_joined,
            self.voice_channel.clone(),
        ) else {
            return;
        };

        handle.send(AppCommand::UpdateVoiceCapturePermission {
            scope,
            channel_id,
            allow_microphone_transmit: allowed,
            noise_suppression: self.options.voice.noise_suppression,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        });
    }

    /// Watch another participant's stream.
    pub fn watch_stream(&mut self, user_id: Id<marker::UserMarker>, display_name: String) {
        let (Some(handle), Some(scope), Some((channel_id, _))) = (
            &self.handle,
            self.voice_scope_joined,
            self.voice_channel.clone(),
        ) else {
            return;
        };

        handle.send(AppCommand::WatchVoiceStream {
            scope,
            channel_id,
            user_id,
            display_name: display_name.clone(),
        });
        self.watching = Some((user_id, display_name));
    }

    /// Join a voice channel or start a DM call, leaving any current one first.
    pub fn join_voice(&mut self, channel_id: Id<marker::ChannelMarker>, name: String) {
        let scope = self.voice_scope(channel_id);

        // Leave first, while nothing else borrows the handle.
        if self.voice_channel.is_some() {
            self.leave_voice();
        }

        if let Some(handle) = &self.handle {
            handle.send(AppCommand::JoinVoiceChannel {
                scope,
                channel_id,
                self_mute: self.self_mute,
                self_deaf: self.self_deaf,
                // Carried from the picker so a device chosen before joining is
                // honoured, rather than silently falling back to the default.
                input_source: self
                    .audio_devices
                    .as_ref()
                    .and_then(|devices| devices.selected_input.clone()),
                output_source: self
                    .audio_devices
                    .as_ref()
                    .and_then(|devices| devices.selected_output.clone()),
                allow_microphone_transmit: self.allow_microphone_transmit,
                // Audio tuning lives in settings, which does not exist yet; the
                // core's defaults are the right starting point.
                noise_suppression: self.options.voice.noise_suppression,
                microphone_sensitivity: Default::default(),
                microphone_volume: Default::default(),
                voice_output_volume: Default::default(),
                participant_playback_settings: Vec::new(),
            });
        }
        self.voice_channel = Some((channel_id, name));
        self.voice_scope_joined = Some(scope);
        self.reproject();
    }

    pub fn leave_voice(&mut self) {
        // The scope must match the channel actually joined, not the current
        // selection - the user may have navigated elsewhere while connected.
        let Some((_, _)) = self.voice_channel else {
            return;
        };

        if let (Some(handle), Some(scope)) = (&self.handle, self.voice_scope_joined) {
            handle.send(AppCommand::LeaveVoiceChannel {
                scope,
                self_mute: self.self_mute,
                self_deaf: self.self_deaf,
            });
        }
        self.voice_channel = None;
        self.voice_scope_joined = None;
        self.reproject();
    }

    /// Toggle mute or deafen on the live connection.
    ///
    /// Deafening implies muting, matching Discord: a deafened user who could
    /// still transmit would be talking into a conversation they cannot hear.
    pub fn toggle_voice_flag(&mut self, deafen: bool) {
        if deafen {
            self.self_deaf = !self.self_deaf;
            if self.self_deaf {
                self.self_mute = true;
            }
        } else {
            self.self_mute = !self.self_mute;
            if !self.self_mute {
                self.self_deaf = false;
            }
        }

        if let (Some(handle), Some(scope), Some((channel_id, _))) = (
            &self.handle,
            self.voice_scope_joined,
            self.voice_channel.as_ref(),
        ) {
            handle.send(AppCommand::UpdateVoiceState {
                scope,
                channel_id: *channel_id,
                self_mute: self.self_mute,
                self_deaf: self.self_deaf,
            });
        }
        self.reproject();
    }

    /// Route a toolbar action for the row at `index`.
    fn handle_message_action(&mut self, index: usize, action: MessageAction) {
        let Some(row) = self.messages.get(index) else {
            return;
        };
        let (message_id, author) = (row.id, row.author.clone());

        match action {
            MessageAction::Reply => self.start_reply(message_id, author),
            MessageAction::Edit => self.start_edit(message_id),
            MessageAction::Delete => {
                self.confirming = Some(Confirm {
                    message: index,
                    action: ConfirmAction::Delete,
                });
            }
            MessageAction::LoadOlder => self.load_older_messages(),
            MessageAction::LoadNewer => self.load_newer_messages(MessageHistoryAfterMode::GapFill),
            MessageAction::Forward => self.start_forward(index),
            MessageAction::JumpToReplied => {
                if let Some(target) = self
                    .messages
                    .get(index)
                    .and_then(|row| row.reply_to.as_ref())
                    .and_then(|(_, _, target)| *target)
                    && let Some(channel_id) = self.nav.channel
                {
                    self.jump_to(channel_id, target);
                }
            }
            MessageAction::CopyText => {
                self.pending_copy = self.messages.get(index).map(|row| row.content.clone())
            }
            MessageAction::CopyLink => {
                self.pending_copy = self
                    .messages
                    .get(index)
                    .map(|row| self.message_link(row.id));
            }
            MessageAction::ShowReactionUsers(reaction) => {
                self.show_reaction_users(index, reaction);
            }
            MessageAction::VotePoll(answer_id) => self.vote_poll(index, answer_id),
            MessageAction::DownloadAttachment(attachment) => {
                self.download_attachment(index, attachment);
            }
            MessageAction::PlayAttachment(attachment) => {
                self.play_attachment(index, attachment);
            }
            MessageAction::RemoveEmbeds => self.remove_embeds(index),
            MessageAction::OpenLink(link) => self.open_link(index, link),
            MessageAction::OpenThread => self.open_message_thread(index),
            MessageAction::TogglePin => {
                // Confirmed rather than applied directly: pinning is visible
                // to the whole channel, and the prompts for it already
                // existed with nothing constructing them.
                let pinned = self
                    .messages
                    .get(index)
                    .map(|row| row.pinned)
                    .unwrap_or(false);
                self.confirming = Some(Confirm {
                    message: index,
                    action: if pinned {
                        ConfirmAction::Unpin
                    } else {
                        ConfirmAction::Pin
                    },
                });
            }
            MessageAction::React => self.open_emoji_picker(message_id),
            MessageAction::OpenProfile => {
                if let Some(row) = self.messages.get(index) {
                    let author_id = row.author_id;
                    self.open_profile(author_id);
                }
            }
            MessageAction::ToggleReaction(reaction) => {
                self.toggle_reaction(index, reaction);
            }
            MessageAction::RevealSpoiler => {
                if let Some(row) = self.messages.get_mut(index) {
                    row.spoiler_revealed = true;
                }
            }
        }
    }

    /// Open a file picker and stage the chosen files for the next send.
    fn attach_files(&mut self, cx: &mut Context<Self>) {
        if self.nav.channel.is_none() {
            return;
        }

        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });

        cx.spawn(async move |workspace, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                // Cancelled, or the platform refused - neither is an error.
                return;
            };

            let _ = workspace.update(cx, |workspace, cx| {
                workspace.stage_attachments(paths);
                cx.notify();
            });
        })
        .detach();
    }

    /// Pick a new avatar and ask for a preview of it.
    ///
    /// The preview comes first because the upload is not reversible in any
    /// useful sense: Discord keeps whatever is sent, so seeing the crop before
    /// committing is the whole point of the step.
    fn change_avatar(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose avatar".into()),
        });

        cx.spawn(async move |workspace, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };

            let _ = workspace.update(cx, |workspace, cx| {
                let Some(handle) = &workspace.handle else {
                    return;
                };
                let upload = ProfileAvatarUpload::from_path(path);
                // The key identifies which preview a reply belongs to; the
                // filename is enough, since only one is ever in flight.
                let key = upload.filename.clone();
                workspace.pending_avatar = Some(key.clone());
                handle.send(AppCommand::LoadProfileAvatarPreview { key, upload });
                cx.notify();
            });
        })
        .detach();
    }

    /// Validate and stage picked files.
    fn stage_attachments(&mut self, paths: Vec<std::path::PathBuf>) {
        self.attachment_error = None;

        for path in paths {
            if self.attachments.len() >= MAX_UPLOAD_ATTACHMENT_COUNT {
                self.attachment_error = Some(format!(
                    "Discord allows at most {MAX_UPLOAD_ATTACHMENT_COUNT} attachments per message"
                ));
                break;
            }

            match MessageAttachmentUpload::from_existing_path(path.clone()) {
                Ok(upload) => self.attachments.push(upload),
                // A file that vanished or cannot be read is reported by name:
                // silently dropping it would look like the picker failed.
                Err(error) => {
                    self.attachment_error = Some(format!(
                        "{}: {error}",
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string())
                    ));
                }
            }
        }
    }

    fn remove_attachment(&mut self, index: usize) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
        }
        self.attachment_error = None;
    }

    /// Open the profile panel for a user, requesting it if not cached.
    pub fn open_profile(&mut self, user_id: Id<marker::UserMarker>) {
        let guild_id = match self.nav.selection {
            Selection::Guild(id) => Some(id),
            Selection::DirectMessages => None,
        };

        self.profile = Some((user_id, None));

        if let Some(handle) = &self.handle {
            handle.send(AppCommand::LoadUserProfile { user_id, guild_id });
        }
    }

    /// Open the emoji picker for a message.
    fn open_emoji_picker(&mut self, message_id: Id<marker::MessageMarker>) {
        self.picker = Some(EmojiPicker {
            target: message_id,
            cursor: 0,
        });
    }

    /// Send the picked reaction and close the picker.
    fn pick_emoji(&mut self, glyph: &str) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        self.react(picker.target, glyph);
    }

    /// Move the picker cursor, wrapping at both ends.
    fn move_picker(&mut self, delta: isize) {
        let total = emoji::flat().len() as isize;
        if let Some(picker) = &mut self.picker {
            let next = (picker.cursor as isize + delta).rem_euclid(total);
            picker.cursor = next as usize;
        }
    }

    /// Add or remove a reaction, depending on whether the user already
    /// reacted with that emoji.
    fn toggle_reaction(&mut self, message: usize, reaction: usize) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(row) = self.messages.get(message) else {
            return;
        };
        let Some((glyph, _, mine)) = row.reactions.get(reaction) else {
            return;
        };

        // Custom emoji round-trip as :name:, which the API will not accept as
        // a unicode reaction, so only unicode reactions toggle for now.
        if glyph.starts_with(':') {
            return;
        }

        let emoji = ReactionEmoji::Unicode(glyph.clone());
        let message_id = row.id;

        if *mine {
            handle.send(AppCommand::RemoveReaction {
                channel_id,
                message_id,
                emoji,
            });
        } else {
            handle.send(AppCommand::AddReaction {
                channel_id,
                message_id,
                emoji,
            });
        }
    }

    /// Begin replying to a message.
    pub fn start_reply(&mut self, message_id: Id<marker::MessageMarker>, author: String) {
        self.editing = None;
        self.replying_to = Some((message_id, author));
    }

    /// Begin editing one of the user's own messages, preloading its body.
    pub fn start_edit(&mut self, message_id: Id<marker::MessageMarker>) {
        let Some(row) = self.messages.iter().find(|row| row.id == message_id) else {
            return;
        };
        if !row.own {
            return;
        }
        self.replying_to = None;
        self.editing = Some(message_id);
        self.composer.set_text(&row.content);
    }

    pub fn cancel_compose_context(&mut self) {
        self.replying_to = None;
        self.editing = None;
        self.composer.clear();
    }

    pub fn delete_message(&mut self, message_id: Id<marker::MessageMarker>) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::DeleteMessage {
            channel_id,
            message_id,
        });
    }

    /// Toggle a reaction. Only adding is wired: removing needs the emoji
    /// identity the user reacted with, which the row already carries, so this
    /// is a small follow-up rather than a gap in the command surface.
    pub fn react(&mut self, message_id: Id<marker::MessageMarker>, emoji: &str) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::AddReaction {
            channel_id,
            message_id,
            emoji: ReactionEmoji::Unicode(emoji.to_string()),
        });
    }

    /// Profile panel for the selected user.
    /// Moderation controls for the profile on screen.
    ///
    /// Offered with their reason when refused rather than hidden: a panel that
    /// changes shape per member is harder to learn than one whose entries
    /// explain themselves. Discord rejects these anyway when the permission or
    /// the role hierarchy is wrong, so the check here only saves a round trip.
    fn moderation_controls(
        &self,
        user_id: Id<marker::UserMarker>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let Selection::Guild(guild_id) = self.nav.selection else {
            return None;
        };
        let state = self.last_state.as_ref()?;

        let outranks = state.outranks_member(guild_id, user_id);
        let reason = |permitted: bool| -> Option<&'static str> {
            if !permitted {
                Some("you do not have permission")
            } else if !outranks {
                Some("their highest role is above yours")
            } else {
                None
            }
        };

        let entries = [
            (
                "mod-timeout",
                "Time out 10m",
                reason(state.can_timeout_members(guild_id)),
                ModerationAction::Timeout,
            ),
            (
                "mod-untimeout",
                "Clear timeout",
                reason(state.can_timeout_members(guild_id)),
                ModerationAction::ClearTimeout,
            ),
            (
                "mod-kick",
                "Kick",
                reason(state.can_kick_members(guild_id)),
                ModerationAction::Kick,
            ),
            (
                "mod-ban",
                "Ban",
                reason(state.can_ban_members(guild_id)),
                ModerationAction::Ban,
            ),
        ];

        let mut panel = column()
            .w_full()
            .p(px(space::MD))
            .gap(px(space::XS))
            .border_t_1()
            .border_color(rgb(active().border))
            .child(section_label("Moderation"));

        for (id, label, refused, action) in entries {
            panel = panel.child(
                gpui::div()
                    .id(id)
                    .px(px(space::SM))
                    .py(px(space::XS))
                    .rounded(px(layout::RADIUS))
                    .text_size(px(scaled(text::SM)))
                    .text_color(rgb(if refused.is_some() {
                        active().text_subtle
                    } else {
                        active().danger
                    }))
                    .when(refused.is_none(), |entry| {
                        entry
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(active().surface_hover)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.moderate(user_id, action);
                                cx.notify();
                            }))
                    })
                    .child(match refused {
                        Some(reason) => format!("{label} - {reason}"),
                        None => label.to_string(),
                    }),
            );
        }

        Some(panel)
    }

    /// Carry out a moderation action against a member.
    fn moderate(&mut self, user_id: Id<marker::UserMarker>, action: ModerationAction) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };

        let label = self
            .model
            .members
            .iter()
            .find(|member| member.user_id == Some(user_id))
            .map(|member| member.name.clone())
            .unwrap_or_else(|| user_id.get().to_string());

        handle.send(match action {
            ModerationAction::Kick => AppCommand::KickMember {
                guild_id,
                user_id,
                label,
            },
            ModerationAction::Ban => AppCommand::BanMember {
                guild_id,
                user_id,
                // Nothing is purged by default: deleting someone's history is
                // a separate decision from removing them.
                delete_message_seconds: 0,
                label,
            },
            ModerationAction::Timeout => AppCommand::TimeoutMember {
                guild_id,
                user_id,
                minutes: Some(10),
                label,
            },
            ModerationAction::ClearTimeout => AppCommand::TimeoutMember {
                guild_id,
                user_id,
                minutes: None,
                label,
            },
        });
    }

    fn profile_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((user_id, view)) = &self.profile else {
            return gpui::div();
        };
        let user_id = *user_id;
        let moderation = self.moderation_controls(user_id, cx);

        match view {
            Some(view) => gpui::div()
                .child(profile_view(view, self.options.display.circular_avatars))
                .children(moderation),
            // The fetch is in flight. A skeleton with the id keeps the panel
            // from flashing empty.
            None => gpui::div().child(profile_view(
                &ProfileView {
                    display_name: user_id.get().to_string(),
                    handle: None,
                    avatar: None,
                    pronouns: None,
                    bio: None,
                    roles: Vec::new(),
                    mutual_guilds: Vec::new(),
                    loaded: false,
                },
                self.options.display.circular_avatars,
            )),
        }
    }

    /// Search panel. Replaces the member list rather than adding a fourth
    /// column, which would squeeze the message area past readability.
    fn search_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(search) = &self.search else {
            return gpui::div();
        };

        let mut pane = column()
            .w(px(layout::MEMBERS + 80.))
            .h_full()
            .bg(rgb(active().surface_sunken))
            .border_l_1()
            .border_color(rgb(active().border));

        pane = pane.child(
            row()
                .w_full()
                .h(px(layout::HEADER))
                .px(px(space::MD))
                .border_b_1()
                .border_color(rgb(active().border))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text))
                .child("Search"),
        );

        // Query field.
        pane = pane.child(
            gpui::div().w_full().p(px(space::SM)).child(
                row()
                    .w_full()
                    .min_h(px(32.))
                    .px(px(space::SM))
                    .rounded(px(layout::RADIUS))
                    .bg(rgb(active().surface))
                    .border_1()
                    .border_color(rgb(active().accent))
                    .text_size(px(scaled(text::SM)))
                    .child(if search.input.text().is_empty() {
                        gpui::div()
                            .text_color(rgb(active().text_subtle))
                            .child("Type and press Enter")
                    } else {
                        gpui::div()
                            .text_color(rgb(active().text))
                            .child(search.input.text().to_string())
                    }),
            ),
        );

        let status = if search.running {
            Some("Searching…".to_string())
        } else if let Some(error) = &search.error {
            Some(error.clone())
        } else {
            search
                .total
                .map(|total| format!("{total} result{}", if total == 1 { "" } else { "s" }))
        };

        if let Some(status) = status {
            pane = pane.child(
                gpui::div()
                    .px(px(space::MD))
                    .pb(px(space::XS))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().text_subtle))
                    .child(status),
            );
        }

        let mut results = column()
            .id("search-results")
            .flex_1()
            .w_full()
            .overflow_y_scroll();

        for (index, result) in search.results.iter().enumerate() {
            // Long bodies are trimmed: the panel is a jump list, not a reader.
            let preview: String = result.content.chars().take(160).collect();

            let (channel_id, message_id) = (result.channel_id, result.message_id);

            results = results.child(
                column()
                    .id(("search-result", index))
                    .w_full()
                    .px(px(space::MD))
                    .py(px(space::SM))
                    .gap(px(2.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(active().surface_hover)))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.jump_to(channel_id, message_id);
                        cx.notify();
                    }))
                    .child(
                        gpui::div()
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().accent))
                            .child(result.author.clone()),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(scaled(text::SM)))
                            .text_color(rgb(active().text_muted))
                            .child(preview),
                    ),
            );
        }

        pane.child(results)
    }

    /// Discord-style Voice Connected Card rendered right above the user profile bar at the bottom of the sidebar.
    fn voice_connected_card(&self, name: &str, cx: &mut Context<Self>) -> gpui::Div {
        let mute = self.self_mute;
        let deaf = self.self_deaf;

        column()
            .w_full()
            .p(px(space::SM))
            .gap(px(space::XS))
            .bg(rgb(active().surface))
            .border_t_1()
            .border_color(rgb(active().border))
            // Top row: signal wave icon + Voice Connected status + channel name + disconnect button
            .child(
                row()
                    .w_full()
                    .items_center()
                    .gap(px(space::SM))
                    .child(
                        gpui::div()
                            .text_size(px(14.))
                            .text_color(rgb(active().success))
                            .child("📶"),
                    )
                    .child(
                        column()
                            .flex_1()
                            .child(
                                gpui::div()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().success))
                                    .child("Voice Connected"),
                            )
                            .child(
                                gpui::div()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_subtle))
                                    .child(name.to_string()),
                            ),
                    )
                    .child(
                        gpui::div()
                            .id("voice-card-leave")
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(layout::RADIUS))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(active().surface_hover)))
                            .text_size(px(14.))
                            .text_color(rgb(active().danger))
                            .child("📞")
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.leave_voice();
                                cx.notify();
                            })),
                    ),
            )
            // Second row: 4 rounded quick control action buttons
            .child(
                row()
                    .w_full()
                    .gap(px(space::XS))
                    .justify_around()
                    .child(
                        gpui::div()
                            .id("card-mute")
                            .flex_1()
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(if mute {
                                active().danger
                            } else {
                                active().surface_sunken
                            }))
                            .text_size(px(12.))
                            .text_color(rgb(if mute {
                                active().on_accent
                            } else {
                                active().text
                            }))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(active().surface_hover)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_voice_flag(false);
                                cx.notify();
                            }))
                            .child(if mute { "🎤̸" } else { "🎤" }),
                    )
                    .child(
                        gpui::div()
                            .id("card-deafen")
                            .flex_1()
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(if deaf {
                                active().danger
                            } else {
                                active().surface_sunken
                            }))
                            .text_size(px(12.))
                            .text_color(rgb(if deaf {
                                active().on_accent
                            } else {
                                active().text
                            }))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(active().surface_hover)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_voice_flag(true);
                                cx.notify();
                            }))
                            .child(if deaf { "🎧̸" } else { "🎧" }),
                    )
                    .child(
                        gpui::div()
                            .id("card-screen")
                            .flex_1()
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(active().surface_sunken))
                            .text_size(px(12.))
                            .text_color(rgb(active().text))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(active().surface_hover)))
                            // Labelled rather than a bare icon: without the
                            // media feature there is no capture path at all,
                            // and an icon that silently does nothing is worse
                            // than one that says why.
                            .child(share_button(self.broadcasting, self.can_broadcast()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                // Toggles: while broadcasting this stops it,
                                // which is what a lit button should do.
                                this.toggle_stream();
                                cx.notify();
                            })),
                    )
                    .child(
                        gpui::div()
                            .id("card-devices")
                            .flex_1()
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(active().surface_sunken))
                            .text_size(px(12.))
                            .text_color(rgb(active().text))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(active().surface_hover)))
                            .child("🎚")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_audio_devices();
                                cx.notify();
                            })),
                    )
                    .child(
                        gpui::div()
                            .id("card-mic-permission")
                            .flex_1()
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(if self.allow_microphone_transmit {
                                active().surface_sunken
                            } else {
                                active().danger
                            }))
                            .text_size(px(12.))
                            .text_color(rgb(active().text))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(active().surface_hover)))
                            // Distinct from mute: this decides whether the
                            // capture device is opened at all.
                            .child("🎙")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let allowed = !this.allow_microphone_transmit;
                                this.set_microphone_allowed(allowed);
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Discord-style User Profile Bar rendered at the very bottom of the channel sidebar.
    fn user_profile_bar(&self, cx: &mut Context<Self>) -> gpui::Div {
        let user_name = self
            .last_state
            .as_ref()
            .and_then(|s| s.current_user())
            .unwrap_or("blu")
            .to_string();

        let mute = self.self_mute;
        let deaf = self.self_deaf;

        row()
            .w_full()
            .h(px(52.))
            .px(px(space::SM))
            .items_center()
            .bg(rgb(active().surface))
            .border_t_1()
            .border_color(rgb(active().border))
            // User Avatar & Name block (clicking opens profile)
            .child(
                row()
                    .id("user-bar-profile")
                    .flex_1()
                    .items_center()
                    .gap(px(space::SM))
                    .px(px(4.))
                    .py(px(4.))
                    .rounded(px(layout::RADIUS))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(active().surface_hover)))
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        if let Some(state) = &this.last_state {
                            let user_id = state.current_user_id();
                            this.profile = user_id.map(|id| (id, None));
                            cx.notify();
                        }
                    }))
                    .child(
                        gpui::div()
                            .id("bar-avatar")
                            .relative()
                            .cursor_pointer()
                            .child(avatar(32., &user_name))
                            .child(presence_dot(Presence::Online))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                // Without this the row's own handler also runs
                                // and opens the profile behind the file picker.
                                cx.stop_propagation();
                                this.change_avatar(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        column()
                            .overflow_hidden()
                            .child(
                                gpui::div()
                                    .text_size(px(scaled(text::SM)))
                                    .text_color(rgb(active().text))
                                    .child(user_name),
                            )
                            .child(
                                gpui::div()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_subtle))
                                    .child("Online"),
                            ),
                    ),
            )
            // Controls (Mic, Headphones/Deafen, Settings Gear)
            .child(
                row()
                    .gap(px(2.))
                    .child(
                        gpui::div()
                            .id("bar-mute")
                            .w(px(28.))
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(if mute {
                                active().danger
                            } else {
                                active().surface
                            }))
                            .text_size(px(14.))
                            .text_color(rgb(if mute {
                                active().on_accent
                            } else {
                                active().text_muted
                            }))
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(rgb(active().surface_hover))
                                    .text_color(rgb(active().text))
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_voice_flag(false);
                                cx.notify();
                            }))
                            .child(if mute { "🎤̸" } else { "🎤" }),
                    )
                    .child(
                        gpui::div()
                            .id("bar-deafen")
                            .w(px(28.))
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(if deaf {
                                active().danger
                            } else {
                                active().surface
                            }))
                            .text_size(px(14.))
                            .text_color(rgb(if deaf {
                                active().on_accent
                            } else {
                                active().text_muted
                            }))
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(rgb(active().surface_hover))
                                    .text_color(rgb(active().text))
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_voice_flag(true);
                                cx.notify();
                            }))
                            .child(if deaf { "🎧̸" } else { "🎧" }),
                    )
                    .child(
                        gpui::div()
                            .id("bar-settings")
                            .w(px(28.))
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(active().surface))
                            .text_size(px(14.))
                            .text_color(rgb(active().text_muted))
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(rgb(active().surface_hover))
                                    .text_color(rgb(active().text))
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_settings_window(cx);
                            }))
                            .child("⚙"),
                    ),
            )
    }

    /// Open a forum channel, which lists posts rather than messages.
    fn open_forum(&mut self, channel_id: Id<marker::ChannelMarker>, name: String) {
        self.forum = Some(ForumView::loading(channel_id, name));
        self.messages.clear();
        self.request_forum_posts(0);
    }

    /// Request a page of posts for the open forum.
    fn request_forum_posts(&mut self, offset: usize) {
        let (Some(handle), Some(forum), Selection::Guild(guild_id)) =
            (&self.handle, &self.forum, self.nav.selection)
        else {
            return;
        };

        handle.send(AppCommand::LoadForumPosts {
            guild_id,
            channel_id: forum.channel_id,
            archive_state: if forum.showing_archived {
                ForumPostArchiveState::Archived
            } else {
                ForumPostArchiveState::Active
            },
            offset,
        });
    }

    /// Switch between active and archived posts, refetching from the start.
    fn toggle_forum_archived(&mut self) {
        let Some(forum) = &mut self.forum else {
            return;
        };
        forum.showing_archived = !forum.showing_archived;
        forum.posts.clear();
        forum.complete = false;
        forum.loading = true;
        forum.error = None;
        self.request_forum_posts(0);
    }

    /// Open a post. A forum post is a thread, so this is a channel switch.
    fn open_forum_post(&mut self, index: usize) {
        let Some(channel_id) = self
            .forum
            .as_ref()
            .and_then(|forum| forum.posts.get(index))
            .map(|post| post.channel_id)
        else {
            return;
        };
        self.forum = None;
        self.open_channel(channel_id);
    }

    /// Whether this build can capture a screen at all.
    ///
    /// Capture lives behind the core's `stream-broadcast` feature; without it
    /// there is no capture path, so the control says so rather than issuing a
    /// command that cannot succeed.
    pub fn can_broadcast(&self) -> bool {
        cfg!(feature = "media")
    }

    /// Ask the core to enumerate screens and windows.
    pub fn open_stream_picker(&mut self) {
        let (Some(handle), Some((channel_id, _)), Some(scope)) = (
            &self.handle,
            self.voice_channel.as_ref(),
            self.voice_scope_joined,
        ) else {
            return;
        };

        self.stream_picker = Some(StreamPicker::loading());
        handle.send(AppCommand::LoadStreamCaptureTargets {
            // One outstanding request at a time, so a fixed id is enough to
            // recognise the reply as ours.
            request_id: StreamCaptureTargetsRequestId::new(1),
            scope,
            channel_id: *channel_id,
        });
    }

    /// Begin broadcasting the chosen source.
    pub fn start_stream(&mut self, index: usize) {
        let Some(picker) = &self.stream_picker else {
            return;
        };
        let Some(target) = picker.targets.get(index).cloned() else {
            return;
        };
        let (Some(handle), Some((channel_id, _)), Some(scope)) = (
            &self.handle,
            self.voice_channel.as_ref(),
            self.voice_scope_joined,
        ) else {
            return;
        };

        handle.send(AppCommand::StartVoiceStream {
            scope,
            channel_id: *channel_id,
            target,
        });
        self.stream_picker = None;
    }

    pub fn stop_stream(&mut self) {
        let (Some(handle), Some((channel_id, _)), Some(scope)) = (
            &self.handle,
            self.voice_channel.as_ref(),
            self.voice_scope_joined,
        ) else {
            return;
        };
        handle.send(AppCommand::StopVoiceStream {
            scope,
            channel_id: *channel_id,
        });
    }

    /// Toggle sharing from the voice bar.
    pub fn toggle_stream(&mut self) {
        if !self.can_broadcast() {
            return;
        }
        if self.broadcasting {
            self.stop_stream();
        } else {
            self.open_stream_picker();
        }
    }

    /// Whether the open channel is a thread, which decides if thread controls
    /// apply at all.
    fn in_thread(&self) -> bool {
        self.model
            .channels
            .get(self.model.selected_channel)
            .is_some_and(|channel| channel.kind == ChannelKind::Thread)
    }

    /// A DM or group DM with no call already running can be called.
    fn can_call(&self) -> bool {
        matches!(self.nav.selection, Selection::DirectMessages)
            && self.nav.channel.is_some()
            && self.voice_channel.is_none()
    }

    /// The member pane only applies to guild channels.
    fn shows_members(&self) -> bool {
        matches!(self.nav.selection, Selection::Guild(_)) && self.nav.channel.is_some()
    }

    /// Open the mention inbox.
    ///
    /// Mentions arrive from every guild at once, which is the point: it is the
    /// surface for "what needs me", not for browsing a channel.
    pub fn open_inbox(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };
        self.inbox = Some(Vec::new());
        handle.send(AppCommand::LoadInboxMentions {
            request_id: 1,
            before: None,
        });
    }

    /// Jump to a mention and dismiss it.
    fn open_mention(&mut self, index: usize) {
        let Some(mention) = self
            .inbox
            .as_ref()
            .and_then(|mentions| mentions.get(index))
            .map(|mention| (mention.channel_id, mention.message_id, mention.guild_id))
        else {
            return;
        };

        let (channel_id, message_id, guild_id) = mention;
        self.inbox = None;

        // A mention can be in any guild, so the guild has to change with it or
        // the sidebar would show the wrong channel list.
        let target = guild_id.map_or(Selection::DirectMessages, Selection::Guild);
        if self.nav.selection != target {
            self.open_guild(guild_id);
        }
        self.forum = None;
        self.jump_to(channel_id, message_id);

        // The surrounding conversation, so the mention has context rather than
        // arriving as one isolated line.
        if let Some(handle) = &self.handle {
            self.inbox_history_request = self.inbox_history_request.wrapping_add(1);
            handle.send(AppCommand::LoadInboxChannelHistory {
                channel_id,
                request_id: self.inbox_history_request,
            });
        }
    }

    /// Dismiss a mention without visiting it.
    fn dismiss_mention(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(mentions) = &mut self.inbox else {
            return;
        };
        if index >= mentions.len() {
            return;
        }

        let mention = mentions.remove(index);
        handle.send(AppCommand::DeleteInboxMention {
            message_id: mention.message_id,
        });
    }

    /// Vote for a poll answer.
    ///
    /// Multi-select polls accumulate the choice; single-answer polls replace
    /// it, matching how Discord treats a second vote.
    fn vote_poll(&mut self, index: usize, answer_id: u8) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(row) = self.messages.get(index) else {
            return;
        };
        let Some(poll) = &row.poll else {
            return;
        };
        if poll.finalized {
            return;
        }

        let mut answer_ids: Vec<u8> = if poll.multiselect {
            poll.answers
                .iter()
                .filter(|answer| answer.mine)
                .map(|answer| answer.answer_id)
                .collect()
        } else {
            Vec::new()
        };

        // Clicking an answer already voted for withdraws it.
        if let Some(position) = answer_ids.iter().position(|id| *id == answer_id) {
            answer_ids.remove(position);
        } else {
            answer_ids.push(answer_id);
        }

        handle.send(AppCommand::VotePoll {
            channel_id,
            message_id: row.id,
            answer_ids,
        });
    }

    /// Open a link from a message in the system browser.
    ///
    /// Routed through the core's OpenUrl rather than launched here, so the
    /// same URL policy applies in both clients - the core normalises and
    /// rejects schemes that should not be handed to a browser.
    fn open_link(&mut self, index: usize, link: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(url) = self
            .messages
            .get(index)
            .and_then(|row| row.links.get(link))
            .cloned()
        else {
            return;
        };

        handle.send(AppCommand::OpenUrl { url });
    }

    /// Strip embeds from a message.
    ///
    /// Useful when a link unfurls into something large or unwanted; the
    /// message text stays, only the preview goes.
    fn remove_embeds(&mut self, index: usize) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(row) = self.messages.get(index) else {
            return;
        };

        handle.send(AppCommand::RemoveMessageEmbeds {
            channel_id,
            message_id: row.id,
        });
    }

    /// Archive or unarchive the open thread.
    ///
    /// The core exposes no thread *creation*; threads are created by Discord
    /// or by a forum post, and this manages ones that already exist.
    pub fn set_thread_archived(&mut self, archived: bool) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::SetThreadArchived {
            channel_id,
            archived,
            label: String::new(),
        });
    }

    /// Follow or unfollow the open thread, which controls whether its
    /// activity reaches the sidebar at all.
    pub fn set_thread_followed(&mut self, followed: bool) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::SetThreadFollowed {
            channel_id,
            followed,
            label: String::new(),
        });
    }

    /// Play an attachment in the configured external player.
    ///
    /// Upstream shells out to mpv rather than decoding in-process, so this
    /// opens a separate window. That is stated in the UI rather than dressed
    /// up as inline playback.
    fn play_attachment(&mut self, index: usize, attachment: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(file) = self
            .messages
            .get(index)
            .and_then(|row| row.attachments.get(attachment))
        else {
            return;
        };

        if file.url.is_empty() {
            self.model.status_line = format!("{} has no source to play", file.filename);
            return;
        }

        if !self.options.display.media_playback {
            // Enabled explicitly rather than assumed: playback launches an
            // external process, which is not something to do unasked.
            self.model.status_line =
                "Enable media playback in settings to open this externally".to_string();
            return;
        }

        handle.send(AppCommand::PlayMedia {
            target: MediaPlaybackTarget {
                url: file.url.clone(),
                label: file.filename.clone(),
                source: MediaPlaybackSource::Message,
            },
            request_id: None,
        });
        self.model.status_line = format!("Opening {} externally…", file.filename);
    }

    /// Download an attachment to the user's download directory.
    fn download_attachment(&mut self, index: usize, attachment: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(row) = self.messages.get(index) else {
            return;
        };
        let Some(file) = row.attachments.get(attachment) else {
            return;
        };

        // A demo attachment carries no URL, since nothing was uploaded; there
        // is nothing to fetch, so this reports rather than failing opaquely.
        if file.url.is_empty() {
            self.model.status_line = format!("{} has no source to download", file.filename);
            return;
        }

        handle.send(AppCommand::DownloadAttachment {
            id: AttachmentDownloadId::new(row.id.get()),
            url: file.url.clone(),
            filename: file.filename.clone(),
            source: DownloadAttachmentSource::AttachmentViewer,
        });
    }

    /// Pin or unpin a message.
    fn set_pinned(&mut self, index: usize, pinned: bool) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(row) = self.messages.get(index) else {
            return;
        };

        handle.send(AppCommand::SetMessagePinned {
            channel_id,
            message_id: row.id,
            pinned,
        });
    }

    /// Open the pinned-messages panel for the current channel.
    pub fn open_pins(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        self.pins = Some(Vec::new());
        handle.send(AppCommand::LoadPinnedMessages { channel_id });
    }

    /// Mute or unmute the open channel.
    ///
    /// Permanent rather than timed: a timed mute needs a duration picker, and
    /// silently choosing one for the user would be worse than not offering it.
    pub fn toggle_channel_muted(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let guild_id = match self.nav.selection {
            Selection::Guild(id) => Some(id),
            Selection::DirectMessages => None,
        };

        let muted = !self.channel_muted;
        self.channel_muted = muted;

        handle.send(AppCommand::SetChannelMuted {
            guild_id,
            channel_id,
            muted,
            duration: Some(MuteDuration::Permanent),
            label: String::new(),
        });
    }

    /// Mute or unmute the open guild.
    pub fn toggle_guild_muted(&mut self) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };

        let muted = !self.guild_muted;
        self.guild_muted = muted;

        handle.send(AppCommand::SetGuildMuted {
            guild_id,
            muted,
            duration: Some(MuteDuration::Permanent),
            label: String::new(),
        });
    }

    /// A discord.com link to a message in the open channel.
    ///
    /// DMs use the `@me` sentinel in place of a guild id, matching Discord's
    /// own link format - a DM link built with a guild id resolves to nothing.
    fn message_link(&self, message_id: Id<marker::MessageMarker>) -> String {
        let guild = match self.nav.selection {
            Selection::Guild(id) => id.get().to_string(),
            Selection::DirectMessages => "@me".to_string(),
        };
        let channel = self
            .nav
            .channel
            .map(|id| id.get().to_string())
            .unwrap_or_default();

        format!(
            "https://discord.com/channels/{guild}/{channel}/{}",
            message_id.get()
        )
    }

    /// Ask who reacted with a given emoji.
    fn show_reaction_users(&mut self, message: usize, reaction: usize) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(row) = self.messages.get(message) else {
            return;
        };
        let Some((glyph, _, _)) = row.reactions.get(reaction) else {
            return;
        };

        // Custom emoji round-trip as :name:, which is not a reaction identity
        // the API accepts, so only unicode reactions can be queried.
        if glyph.starts_with(':') {
            return;
        }

        self.reaction_users = Some((row.id, glyph.clone(), Vec::new()));
        handle.send(AppCommand::LoadReactionUsers {
            channel_id,
            message_id: row.id,
            emoji: ReactionEmoji::Unicode(glyph.clone()),
            after: None,
        });
    }

    /// Open the quick switcher, seeded with the full candidate list.
    pub fn open_switcher(&mut self) {
        self.open_switcher_for(SwitcherPurpose::Navigate);
    }

    /// Open the switcher to pick a channel for something other than navigating.
    ///
    /// Forwarding needs exactly the picker the switcher already is - every
    /// channel across every guild, fuzzy-ranked - so it reuses it rather than
    /// growing a second one that would rank differently.
    fn open_switcher_for(&mut self, purpose: SwitcherPurpose) {
        let mut switcher = Switcher::default();
        if let Some(state) = &self.last_state {
            switcher.rank(projection::switcher_candidates(state));
        }
        self.switcher_purpose = purpose;
        self.switcher = Some(switcher);
    }

    /// Begin forwarding a message: pick the destination.
    fn start_forward(&mut self, index: usize) {
        let Some(row) = self.messages.get(index) else {
            return;
        };
        let source = (row.id, self.nav.channel);
        let Some(channel_id) = source.1 else {
            return;
        };

        self.open_switcher_for(SwitcherPurpose::Forward {
            message_id: source.0,
            source_channel_id: channel_id,
        });
    }

    /// Re-rank after the query changes.
    fn rerank_switcher(&mut self) {
        let Some(state) = self.last_state.clone() else {
            return;
        };
        if let Some(switcher) = &mut self.switcher {
            switcher.rank(projection::switcher_candidates(&state));
        }
    }

    /// Jump to the highlighted candidate.
    fn activate_switcher(&mut self) {
        let Some(target) = self
            .switcher
            .as_ref()
            .and_then(|switcher| switcher.selection())
            .map(|candidate| (candidate.channel_id, candidate.guild_id))
        else {
            return;
        };

        self.switcher = None;

        // Forwarding consumes the selection instead of navigating to it: the
        // point is to send the message elsewhere, not to go there.
        if let SwitcherPurpose::Forward {
            message_id,
            source_channel_id,
        } = std::mem::take(&mut self.switcher_purpose)
        {
            if let Some(handle) = &self.handle {
                let source_guild_id = match self.nav.selection {
                    Selection::Guild(guild_id) => Some(guild_id),
                    Selection::DirectMessages => None,
                };
                handle.send(AppCommand::ForwardMessage {
                    source_channel_id,
                    source_guild_id,
                    message_id,
                    target_channel_id: target.0,
                    nonce: next_message_nonce(),
                });
                self.model.status_line = "Forwarded".to_string();
            }
            return;
        }

        // Switching guild first keeps the sidebar and the open channel
        // consistent; opening the channel alone would leave the wrong guild
        // selected and its channel list showing.
        let (channel_id, guild_id) = target;
        if self.nav.selection != guild_id.map_or(Selection::DirectMessages, Selection::Guild) {
            self.open_guild(guild_id);
        }
        self.forum = None;
        self.open_channel(channel_id);
    }

    /// Mark the open channel read up to its newest message.
    ///
    /// Without this, unread badges accumulate with no way to clear them - the
    /// counts are correct but permanently rising, which is worse than not
    /// showing them.
    pub fn mark_read(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(newest) = self.messages.last().map(|row| row.id) else {
            return;
        };

        handle.send(AppCommand::AckChannel {
            channel_id,
            message_id: newest,
        });
    }

    /// Mark every unread channel in view read.
    ///
    /// Batched into one command rather than one per channel: the core accepts
    /// a list, and a burst of individual acks is exactly the traffic pattern
    /// that gets a third-party client flagged.
    pub fn mark_all_read(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };

        let targets: Vec<_> = self
            .model
            .channels
            .iter()
            .filter(|channel| channel.unread)
            .filter_map(|channel| {
                // Acking needs a message to ack up to; a channel whose last
                // message is unknown is skipped rather than guessed at.
                channel.id.zip(channel.last_message)
            })
            .collect();

        if targets.is_empty() {
            return;
        }

        handle.send(AppCommand::AckChannels { targets });
    }

    /// Request the page of messages before the oldest one loaded.
    ///
    /// The message cache is lazily populated, so scrollback exists only if it
    /// is asked for. Without this the log stops at whatever the initial fetch
    /// returned, which is far short of the TUI's unlimited scrollback.
    pub fn load_older_messages(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(oldest) = self.messages.first().map(|row| row.id) else {
            return;
        };

        handle.send(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(oldest),
        });
    }

    /// Request the page of messages after the newest one loaded.
    ///
    /// Needed whenever the loaded range is not anchored to the live end of the
    /// channel: jumping to a search result or an inbox mention lands mid-history,
    /// and without forward paging the view is stuck there.
    pub fn load_newer_messages(&mut self, mode: MessageHistoryAfterMode) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(newest) = self.messages.last().map(|row| row.id) else {
            return;
        };

        handle.send(AppCommand::LoadMessageHistoryAfter {
            channel_id,
            after: newest,
            mode,
        });
    }

    /// Whether the channel has messages newer than the loaded range.
    ///
    /// Compared against the channel's own `last_message_id` rather than a
    /// scroll position: after jumping to a search result the view is at the
    /// bottom of what is loaded, which is not the bottom of the channel.
    fn has_newer_messages(&self) -> bool {
        let (Some(channel_id), Some(newest)) =
            (self.nav.channel, self.messages.last().map(|row| row.id))
        else {
            return false;
        };

        self.model
            .channels
            .iter()
            .find(|channel| channel.id == Some(channel_id))
            .and_then(|channel| channel.last_message)
            .is_some_and(|last| last > newest)
    }

    /// Re-fetch the open channel from scratch.
    ///
    /// The gateway can drop messages across a reconnect, leaving a hole that no
    /// amount of scrolling fills, because both paging directions extend from
    /// what is already cached.
    pub fn refresh_history(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::RefreshMessageHistory { channel_id });
    }

    /// Mark the open channel read, after a delay.
    ///
    /// Used when the newest message arrives while the channel is on screen.
    /// An immediate ack would race the user's eyes - and, sent on every
    /// incoming message, would be a request per message.
    pub fn schedule_mark_read(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(newest) = self.messages.last().map(|row| row.id) else {
            return;
        };

        handle.send(AppCommand::ScheduleAckChannel {
            channel_id,
            message_id: newest,
        });
    }

    /// Ask the server for members matching a query.
    ///
    /// The member list only holds the windowed ranges this client subscribed
    /// to, so a mention for someone further down it has never seen would
    /// otherwise not resolve.
    pub fn search_members(&mut self, query: String) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Selection::Guild(guild_id) = self.nav.selection else {
            return;
        };
        if query.trim().is_empty() {
            return;
        }

        handle.send(AppCommand::SearchGuildMembers {
            guild_id,
            query,
            limit: 25,
        });
    }

    /// Fetch members referenced on screen but absent from the cache.
    ///
    /// Without this an unhydrated author renders as a raw id, and a mention of
    /// someone outside the subscribed window stays unresolved.
    ///
    /// The demand comes from the core rather than from a scan of the visible
    /// rows: it already tracks voice, typing and thread participants too, and
    /// a second heuristic here would drift from the one the TUI uses.
    pub fn hydrate_missing_members(&mut self) {
        let (Some(handle), Some(state)) = (&self.handle, self.last_state.as_ref()) else {
            return;
        };
        let selected = match self.nav.selection {
            Selection::Guild(guild_id) => Some(guild_id),
            Selection::DirectMessages => None,
        };

        for (guild_id, user_ids) in
            state.missing_member_hydration_requests(selected, std::time::Instant::now())
        {
            handle.send(AppCommand::LoadGuildMembersByIds { guild_id, user_ids });
        }
    }

    /// Switch the open guild, clearing the channel selection.
    pub fn open_guild(&mut self, guild_id: Option<Id<marker::GuildMarker>>) {
        self.nav.selection = match guild_id {
            Some(id) => Selection::Guild(id),
            None => Selection::DirectMessages,
        };
        self.nav.channel = None;
        self.messages.clear();
        // Commands are per guild, so the previous guild's set must not linger.
        self.app_commands.clear();
        self.load_app_commands();

        if let (Some(handle), Some(guild_id)) = (&self.handle, guild_id) {
            handle.send(AppCommand::SetSelectedGuild {
                guild_id: Some(guild_id),
            });
        }
        self.reproject();
    }

    /// Fold a discrete event into the model.
    ///
    /// Most state arrives through reprojection; this handles only what is not
    /// represented in the state store, such as transient errors.
    fn absorb(&mut self, event: AppEvent) {
        match &event {
            // A reconnect can have dropped messages while the socket was down.
            // Neither paging direction fills that hole, because both extend
            // from what is already cached.
            AppEvent::VoiceAudioSourcesLoaded {
                request_id,
                inputs,
                outputs,
                error,
            } if *request_id == self.audio_sources_request => {
                let devices = self.audio_devices.get_or_insert_with(Default::default);
                devices.inputs = inputs.clone();
                devices.outputs = outputs.clone();
                devices.error = error.clone();
            }
            // A send that Discord rejected. The text goes back to the composer
            // rather than being dropped: retyping a long message because the
            // client silently ate it is the worst possible outcome here.
            AppEvent::MessageSendFailed { channel_id, nonce } => {
                let restored = self.pending_sends.remove(nonce);
                if Some(*channel_id) == self.nav.channel
                    && let Some(content) = restored
                    && self.composer.is_empty()
                {
                    self.composer.set_text(&content);
                }
                self.model.status_line = "Message was not sent".to_string();
            }
            AppEvent::MessageSendRateLimited {
                retry_after_millis, ..
            } => {
                self.model.status_line = format!(
                    "Rate limited; retrying in {:.0}s",
                    *retry_after_millis as f64 / 1000.0
                );
            }
            AppEvent::MessageSendCooldownStarted {
                duration_millis, ..
            } => {
                // Slowmode. Reported as a duration rather than a bare refusal
                // so the user knows whether to wait or give up.
                self.model.status_line = format!(
                    "Slowmode: {:.0}s before the next message",
                    *duration_millis as f64 / 1000.0
                );
            }
            // One arm, not two: a message can be both in the open channel and
            // one of ours, and separate guarded arms would let the first match
            // shadow the second.
            AppEvent::MessageCreate { message } => {
                // Confirmed by the server, so the retry copy is no longer
                // needed. The nonce comes back on its own field rather than as
                // the message id, which the server assigns independently.
                if let Some(nonce) = message.nonce {
                    self.pending_sends.remove(&nonce);
                }

                // A message landing in the channel on screen means the user is
                // most likely looking at it, so schedule the ack rather than
                // letting the badge sit there. The core owns the delay.
                if Some(message.channel_id) == self.nav.channel {
                    self.schedule_mark_read();
                }
            }

            // Login cannot continue in this client: solving a captcha needs a
            // browser. Said plainly rather than leaving the attempt hanging.
            AppEvent::InviteResolved { preview } => {
                if let Some(invite) = &mut self.invite
                    && invite.code == preview.code
                {
                    invite.preview = Some(preview.clone());
                }
            }
            AppEvent::InviteResolveFailed { code, message } => {
                if let Some(invite) = &mut self.invite
                    && invite.code == *code
                {
                    invite.error = Some(message.clone());
                }
            }
            AppEvent::InviteAccepted { .. } => {
                // The guild itself arrives as a GuildCreate and reprojects.
                self.model.status_line = "Joined".to_string();
            }
            AppEvent::InviteAcceptFailed { message, .. } => {
                self.model.status_line = format!("Could not join: {message}");
            }
            AppEvent::CaptchaRequired { action } => {
                self.model.status_line =
                    format!("Discord demanded a captcha for {action}; use a browser to continue");
            }
            AppEvent::SignedOut => {
                self.model.connected = false;
                self.model.status_line = "Signed out".to_string();
            }
            AppEvent::GatewayClosed => {
                self.model.connected = false;
                self.model.status_line = "Disconnected; reconnecting".to_string();
            }
            AppEvent::GatewayResumed => {
                self.model.connected = true;
                self.model.status_line = "Reconnected".to_string();
            }
            AppEvent::UpdateAvailable { latest_version } => {
                self.model.status_line = format!("concord {latest_version} is available");
            }

            AppEvent::InteractionFailed { reason_code, .. } => {
                self.model.status_line = format!("The command failed (code {reason_code})");
            }
            AppEvent::InteractionSucceeded { .. } => {
                // The bot's reply arrives as an ordinary message, so there is
                // nothing to show beyond clearing any earlier failure.
                self.model.status_line.clear();
            }
            AppEvent::ApplicationCommandAutocompleteResponse { choices, .. } => {
                self.command_choices = choices.iter().map(|choice| choice.name.clone()).collect();
            }
            AppEvent::ApplicationCommandIndexUpdated { guild_id } => {
                // A bot's command list changed; re-fetch so the picker is not
                // offering commands that no longer exist.
                if self.nav.selection == Selection::Guild(*guild_id)
                    && let Some(handle) = &self.handle
                {
                    handle.send(AppCommand::LoadApplicationCommands {
                        guild_id: Some(*guild_id),
                    });
                }
            }

            AppEvent::AttachmentDownloadStarted {
                id,
                filename,
                total_bytes,
                ..
            } => {
                self.downloads
                    .push((*id, filename.clone(), total_bytes.map(|_| 0.0)));
                self.model.status_line = format!("Downloading {filename}");
            }
            AppEvent::AttachmentDownloadProgress {
                id,
                downloaded_bytes,
                total_bytes,
            } => {
                if let Some(entry) = self.downloads.iter_mut().find(|entry| entry.0 == *id) {
                    // Only meaningful with a known total; a download of unknown
                    // length shows activity without a false percentage.
                    entry.2 = total_bytes
                        .filter(|total| *total > 0)
                        .map(|total| (*downloaded_bytes as f32 / total as f32).clamp(0.0, 1.0));
                }
            }
            AppEvent::AttachmentDownloadCompleted { id, path, .. } => {
                self.downloads.retain(|entry| entry.0 != *id);
                self.model.status_line = format!("Saved to {path}");
            }
            AppEvent::AttachmentDownloadFailed {
                id,
                filename,
                message,
                ..
            } => {
                self.downloads.retain(|entry| entry.0 != *id);
                self.model.status_line = format!("{filename}: {message}");
            }

            AppEvent::AttachmentPreviewLoaded { url, bytes } => {
                match image_format_for(url) {
                    Some(format) => {
                        self.attachment_previews.insert(
                            url.clone(),
                            std::sync::Arc::new(gpui::Image::from_bytes(format, bytes.clone())),
                        );
                    }
                    // An extension GPUI cannot decode. Dropped rather than
                    // guessed at, since handing it the wrong format renders
                    // nothing and logs nothing.
                    None => {
                        self.model.status_line =
                            "Attachment is in a format this client cannot display".to_string();
                    }
                }
            }
            AppEvent::AttachmentPreviewLoadFailed { url, message } => {
                // Dropped from the requested set so a later reprojection can
                // retry; a transient CDN failure should not be permanent.
                self.requested_previews.remove(url);
                self.model.status_line = format!("Preview failed: {message}");
            }

            AppEvent::UserProfileLoadFailed { message, .. } => {
                // The panel is closed rather than left on a spinner that will
                // never resolve.
                self.profile = None;
                self.model.status_line = format!("Could not load profile: {message}");
            }
            AppEvent::UserProfileUpdateFailed { message, .. } => {
                self.model.status_line = format!("Profile not updated: {message}");
            }

            AppEvent::VoiceAudioSourcesApplyFailed {
                active_input_source,
                active_output_source,
                message,
                ..
            } => {
                // The picker is corrected to what is actually in use, so it
                // does not keep showing a device that was refused.
                if let Some(devices) = &mut self.audio_devices {
                    devices.selected_input = active_input_source.clone();
                    devices.selected_output = active_output_source.clone();
                    devices.error = Some(message.clone());
                }
                self.model.status_line = message.clone();
            }
            AppEvent::VoiceConnectionStatusChanged {
                status, message, ..
            } => {
                self.model.status_line = match status {
                    VoiceConnectionStatus::Connecting => "Voice: connecting".to_string(),
                    VoiceConnectionStatus::Connected => "Voice: connected".to_string(),
                    VoiceConnectionStatus::Disconnected => "Voice: disconnected".to_string(),
                    VoiceConnectionStatus::Failed => message
                        .clone()
                        .unwrap_or_else(|| "Voice: connection failed".to_string()),
                };

                // A failed or dropped connection clears the local voice state,
                // or the sidebar keeps showing a call that is not happening.
                if matches!(
                    status,
                    VoiceConnectionStatus::Disconnected | VoiceConnectionStatus::Failed
                ) {
                    self.voice_channel = None;
                    self.voice_scope_joined = None;
                    self.broadcasting = false;
                    self.watching = None;
                }
            }
            AppEvent::VoiceSound { .. } => {
                // The core plays the sound; there is nothing to display.
            }
            AppEvent::MediaPlaybackWindowReady { .. }
            | AppEvent::StreamPlaybackWindowReady { .. } => {
                // Playback opens in an external player, which is its own
                // visible confirmation.
            }
            AppEvent::StreamPlaybackEnded { reconnecting, .. } => {
                if !reconnecting {
                    self.watching = None;
                }
            }

            AppEvent::Ready { .. } => {
                self.refresh_history();
                self.hydrate_missing_members();
            }
            _ => {}
        }

        match event {
            AppEvent::GatewayError { message } => {
                self.model.status_line = message;
            }
            AppEvent::ForumPostsLoaded {
                channel_id,
                threads,
                first_messages,
                has_more,
                next_offset,
                ..
            } => {
                if let Some(forum) = &mut self.forum
                    && forum.channel_id == channel_id
                {
                    forum.loading = false;
                    forum.complete = !has_more;
                    forum.next_offset = next_offset;

                    for (index, thread) in threads.iter().enumerate() {
                        // Discord returns opening messages positionally
                        // alongside the threads, so they are paired by index.
                        let opening = first_messages.get(index);

                        forum.posts.push(ForumPost {
                            channel_id: thread.channel_id,
                            title: thread.name.clone(),
                            preview: opening
                                .and_then(|message| message.content.clone())
                                .map(|content| content.chars().take(160).collect())
                                .unwrap_or_default(),
                            author: opening
                                .map(|message| message.author.clone())
                                .unwrap_or_default(),
                            message_count: thread.message_count.unwrap_or(0),
                            archived: forum.showing_archived,
                        });
                    }
                }
            }
            AppEvent::ForumPostsLoadFailed { channel_id, .. } => {
                if let Some(forum) = &mut self.forum
                    && forum.channel_id == channel_id
                {
                    forum.loading = false;
                    forum.error = Some("Could not load posts".to_string());
                }
            }
            AppEvent::InboxMentionsLoaded { messages, .. } => {
                self.inbox = Some(
                    messages
                        .into_iter()
                        .map(|message| InboxMention {
                            channel_id: message.channel_id,
                            message_id: message.message_id,
                            guild_id: message.guild_id,
                            author: message.author,
                            content: message.content.unwrap_or_default(),
                        })
                        .collect(),
                );
            }
            AppEvent::InboxMentionsLoadFailed { .. } => self.inbox = None,
            AppEvent::ApplicationCommandsLoaded { commands, .. } => {
                self.app_commands = commands;
            }
            AppEvent::PinnedMessagesLoaded { messages, .. } => {
                self.pins = Some(
                    messages
                        .into_iter()
                        .map(|message| {
                            (
                                message.message_id,
                                message.author,
                                message.content.unwrap_or_default(),
                            )
                        })
                        .collect(),
                );
            }
            AppEvent::PinnedMessagesLoadFailed { .. } => self.pins = None,
            AppEvent::ReactionUsersLoaded {
                message_id,
                users,
                after,
                ..
            } => {
                if let Some((target, _, existing)) = &mut self.reaction_users
                    && *target == message_id
                {
                    let names = users.into_iter().map(|user| user.display_name);
                    // `after: None` is the first page and replaces; a cursor
                    // means this is a continuation and appends.
                    if after.is_none() {
                        *existing = names.collect();
                    } else {
                        existing.extend(names);
                    }
                }
            }
            AppEvent::ReactionUsersLoadFailed { .. } => self.reaction_users = None,
            AppEvent::StreamCaptureTargetsLoaded { targets, error, .. } => {
                if let Some(picker) = &mut self.stream_picker {
                    picker.loading = false;
                    picker.targets = targets;
                    picker.error = error;
                }
            }
            AppEvent::StreamBroadcastStarted { .. } => {
                self.broadcasting = true;
                self.stream_picker = None;
            }
            AppEvent::StreamBroadcastEnded { .. } => self.broadcasting = false,
            AppEvent::StreamBroadcastStartFailed { .. } => {
                self.broadcasting = false;
                self.stream_picker = None;
                // The event carries no reason, so the message stays generic
                // rather than inventing a cause.
                self.model.status_line = "Screen share failed to start".to_string();
            }
            AppEvent::StreamBroadcastAudioUnavailable { .. } => {
                // Video still works, so this is a note rather than a failure.
                self.model.status_line =
                    "Sharing without audio - system audio capture unavailable".to_string();
            }
            AppEvent::MessageSearchLoaded { page } => {
                if let Some(search) = &mut self.search {
                    search.running = false;
                    search.total = page.total_results;
                    search.results = page
                        .messages
                        .into_iter()
                        .map(|message| SearchResult {
                            author: message.author,
                            content: message.content.unwrap_or_default(),
                            channel_id: message.channel_id,
                            message_id: message.message_id,
                        })
                        .collect();
                }
            }
            AppEvent::MessageSearchLoadFailed { .. } => {
                if let Some(search) = &mut self.search {
                    search.running = false;
                    search.error = Some("search failed".to_string());
                }
            }
            AppEvent::Ready { user, user_id } => {
                self.current_user = user_id;
                self.model.connected = true;
                self.model.status_line = format!("connected as {user}");
            }
            _ => {}
        }
    }

    fn guild_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rail = column()
            .w(px(layout::GUILD_RAIL))
            .h_full()
            .bg(rgb(active().bg))
            .items_center()
            .pt(px(space::MD))
            .gap(px(space::SM));

        let mut open_folder: Option<u64> = None;

        for (index, guild) in self.model.guilds.iter().enumerate() {
            let selected = index == self.model.selected_guild;
            let guild_id = guild.id;

            // A folder header precedes the first guild in each run. Runs are
            // adjacent by construction, so a change of folder id is the
            // boundary.
            if let Some(folder) = &guild.folder
                && open_folder != Some(folder.id)
            {
                open_folder = Some(folder.id);
                let folder_id = folder.id;
                let label = folder.name.clone().unwrap_or_else(|| "Folder".to_string());
                let color = folder.color.unwrap_or(active().text_subtle);

                rail = rail.child(
                    gpui::div()
                        .id(("folder", folder_id as usize))
                        .w(px(44.))
                        .py(px(space::XS))
                        .flex()
                        .justify_center()
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(color))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        // Truncated because the rail is one avatar wide; the
                        // full name is not the point, telling folders apart is.
                        .child(label.chars().take(6).collect::<String>())
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.renaming_folder = Some((folder_id, Composer::default()));
                            cx.notify();
                        })),
                );
            } else if guild.folder.is_none() {
                open_folder = None;
            }
            rail = rail.child(
                gpui::div()
                    .id(("guild", index))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.open_guild(guild_id);
                        cx.notify();
                    }))
                    .relative()
                    .child(avatar(44., &guild.name))
                    .when(selected, |d| {
                        d.border_2()
                            .border_color(rgb(active().accent))
                            .rounded_full()
                    })
                    .when(guild.unread && !selected, |d| {
                        d.border_1()
                            .border_color(rgb(active().text_muted))
                            .rounded_full()
                    })
                    // Mention count, which the projection computed but the
                    // rail never showed. Without it a server with unread
                    // mentions looks the same as one with idle chatter.
                    .when(guild.mentions > 0, |d| {
                        d.child(
                            gpui::div()
                                .absolute()
                                .bottom(px(-2.))
                                .right(px(-2.))
                                .px(px(5.))
                                .rounded_full()
                                .bg(rgb(active().danger))
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().on_accent))
                                // Capped, because a four-digit badge is wider
                                // than the avatar it sits on.
                                .child(if guild.mentions > 99 {
                                    "99+".to_string()
                                } else {
                                    guild.mentions.to_string()
                                }),
                        )
                    }),
            );
        }

        // Joining a server. Until this existed a client could leave a guild
        // but never join one, so the official client was still needed for it.
        rail.child(
            gpui::div()
                .id("guild-join")
                .w(px(44.))
                .h(px(44.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgb(active().surface))
                .cursor_pointer()
                .text_size(px(scaled(text::LG)))
                .text_color(rgb(active().success))
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .child("+")
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.prompt = Some((Prompt::InviteCode, Composer::default()));
                    cx.notify();
                })),
        )
    }

    fn channel_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let guild_name = self
            .model
            .guilds
            .get(self.model.selected_guild)
            .map(|g| g.name.clone())
            .unwrap_or_default();

        let header_row = row()
            .w_full()
            .h(px(layout::HEADER))
            .px(px(space::MD))
            .border_b_1()
            .border_color(rgb(active().border))
            .text_size(px(scaled(text::BASE)))
            .text_color(rgb(active().text))
            .gap(px(space::SM))
            .child(gpui::div().flex_1().child(guild_name))
            // Guild-level controls, shown only for a real guild: neither mute
            // nor leave means anything for the DM pseudo-guild.
            .when(
                matches!(self.nav.selection, Selection::Guild(_)),
                |header| {
                    let muted = self.guild_muted;
                    header
                        .child(
                            gpui::div()
                                .id("guild-mute")
                                .px(px(space::SM))
                                .rounded(px(layout::RADIUS))
                                .cursor_pointer()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(if muted {
                                    active().danger
                                } else {
                                    active().text_subtle
                                }))
                                .hover(|style| style.bg(rgb(active().surface_hover)))
                                .child(if muted { "unmute" } else { "mute" })
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.toggle_guild_muted();
                                    cx.notify();
                                })),
                        )
                        .child(
                            gpui::div()
                                .id("guild-leave")
                                .px(px(space::SM))
                                .rounded(px(layout::RADIUS))
                                .cursor_pointer()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .hover(|style| style.text_color(rgb(active().danger)))
                                .child("leave")
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.leave_guild();
                                    cx.notify();
                                })),
                        )
                },
            );

        let mut list = column()
            .id("channel-list")
            .flex_1()
            .w_full()
            .pt(px(space::XS))
            .gap(px(1.))
            .overflow_y_scroll();

        // Set while walking a collapsed category, so its children can be
        // skipped without a lookup per row.
        let mut hidden_parent: Option<Id<marker::ChannelMarker>> = None;

        if let Some(filter) = &self.pane_filter
            && self.focus_pane == Pane::Channels
        {
            list = list.child(
                gpui::div()
                    .w_full()
                    .px(px(space::MD))
                    .py(px(space::XS))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().accent))
                    .child(if filter.text().is_empty() {
                        "filter…".to_string()
                    } else {
                        format!("filter: {}", filter.text())
                    }),
            );
        }

        for (index, channel) in self.model.channels.iter().enumerate() {
            if channel.kind == ChannelKind::Category {
                let collapsed = channel.id.is_some_and(|id| self.category_collapsed(id));
                let category_id = channel.id;

                list = list.child(
                    row()
                        .id(("category", index))
                        .w_full()
                        .cursor_pointer()
                        .child(section_label(format!(
                            "{} {}",
                            if collapsed { "\u{25b8}" } else { "\u{25be}" },
                            channel.name
                        )))
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            if let Some(id) = category_id {
                                this.toggle_category(id);
                            }
                            cx.notify();
                        })),
                );

                // Remember which category we are under, so its children can be
                // hidden without another lookup per row.
                hidden_parent = collapsed.then_some(channel.id).flatten();
                continue;
            }

            // Children of a collapsed category are skipped entirely rather
            // than rendered zero-height, so keyboard order matches what is
            // visible.
            if hidden_parent.is_some() && channel.parent == hidden_parent {
                continue;
            }

            if self.focus_pane == Pane::Channels && !self.passes_filter(&channel.name) {
                continue;
            }

            let selected = index == self.model.selected_channel;
            let is_thread = channel.kind == ChannelKind::Thread;

            let mut entry = sidebar_row(selected)
                // Threads indent under their parent, and archived ones dim so
                // an auto-archived thread stays visible without competing.
                .when(is_thread, |d| d.pl(px(space::LG)))
                .when(channel.archived, |d| d.opacity(0.55))
                .child(
                    gpui::div()
                        .w(px(14.))
                        .text_color(rgb(active().text_subtle))
                        .child(channel.kind.glyph()),
                )
                .child(
                    gpui::div()
                        .flex_1()
                        .when(channel.unread && !selected, |d| {
                            d.text_color(rgb(active().text))
                        })
                        .child(channel.name.clone()),
                );

            if channel.mentions > 0 {
                entry = entry.child(
                    gpui::div()
                        .px(px(6.))
                        .rounded_full()
                        .bg(rgb(active().danger))
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().on_accent))
                        .child(channel.mentions.to_string()),
                );
            }

            // Text channels switch the view; voice channels join a call.
            let entry = match channel.id {
                Some(channel_id) if channel.kind == ChannelKind::Voice => {
                    let name = channel.name.clone();
                    entry
                        .id(("channel", index))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.join_voice(channel_id, name.clone());
                            cx.notify();
                        }))
                        .into_any_element()
                }
                Some(channel_id) if channel.kind == ChannelKind::Forum => {
                    let name = channel.name.clone();
                    entry
                        .id(("channel", index))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.open_forum(channel_id, name.clone());
                            cx.notify();
                        }))
                        .into_any_element()
                }
                Some(channel_id) => entry
                    .id(("channel", index))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.forum = None;
                        this.open_channel(channel_id);
                        cx.notify();
                    }))
                    .into_any_element(),
                None => entry.into_any_element(),
            };

            list = list.child(entry);

            // Occupants render nested under their voice channel.
            for participant in &channel.voice {
                let participant_id = participant.user_id;
                let participant_name = participant.name.clone();
                list = list.child(voice_participant_row(
                    VoiceRow {
                        name: &participant.name,
                        muted: participant.muted,
                        deafened: participant.deafened,
                        streaming: participant.streaming,
                        speaking: participant.speaking,
                        id_seed: participant_id.get(),
                    },
                    {
                        let entity = cx.entity();
                        move |cx: &mut gpui::App| {
                            let name = participant_name.clone();
                            entity.update(cx, |workspace, cx| {
                                workspace.watch_stream(participant_id, name);
                                cx.notify();
                            });
                        }
                    },
                    {
                        // Locally muting one participant, which is separate
                        // from their own mute state and visible only here.
                        let entity = cx.entity();
                        move |cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                let muted = workspace.locally_muted.insert(participant_id);
                                if !muted {
                                    workspace.locally_muted.remove(&participant_id);
                                }
                                let volume =
                                    workspace.options.voice.voice_output_volume.value() as u16;
                                workspace.set_participant_playback(participant_id, volume, muted);
                                cx.notify();
                            });
                        }
                    },
                ));
            }
        }

        let mut sidebar = panel_sunken(layout::SIDEBAR).child(header_row).child(list);

        if let Some((_, name)) = &self.voice_channel {
            sidebar = sidebar.child(self.voice_connected_card(name, cx));
        }

        sidebar = sidebar.child(self.user_profile_bar(cx));

        sidebar
    }

    fn member_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut pane = column()
            .w(px(layout::MEMBERS))
            .h_full()
            .bg(rgb(active().surface_sunken))
            .border_l_1()
            .border_color(rgb(active().border))
            .pt(px(space::SM))
            .overflow_hidden();

        if self.model.members.is_empty() {
            return pane.child(
                gpui::div()
                    .px(px(space::MD))
                    .pt(px(space::MD))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().text_subtle))
                    .child("No member data"),
            );
        }

        for (pane_index, member) in self.model.members.iter().enumerate() {
            if member.is_group {
                pane = pane.child(section_label(member.name.clone()));
                continue;
            }

            let mut entry = sidebar_row(false)
                .when(self.options.display.show_avatars, |d| {
                    d.child(avatar_with_url(
                        layout::AVATAR_SM,
                        &member.name,
                        member.avatar.as_deref(),
                        self.options.display.circular_avatars,
                    ))
                })
                .child(presence_dot(member.presence))
                .child(
                    gpui::div()
                        .flex_1()
                        .text_color(rgb(member.color.unwrap_or(active().text_muted)))
                        .child(member.name.clone()),
                );

            if member.is_bot {
                entry = entry.child(
                    gpui::div()
                        .px(px(4.))
                        .rounded(px(3.))
                        .bg(rgb(active().accent))
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().on_accent))
                        .child("BOT"),
                );
            }

            if self.focus_pane == Pane::Members
                && !member.is_group
                && !self.passes_filter(&member.name)
            {
                continue;
            }

            let entry = match member.user_id {
                Some(user_id) => entry
                    .id(("member", pane_index))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.open_profile(user_id);
                        cx.notify();
                    }))
                    .into_any_element(),
                None => entry.into_any_element(),
            };

            pane = pane.child(entry);
        }

        pane
    }

    /// The single open modal, if any.
    ///
    /// Ordered by urgency: a confirmation blocks whatever opened it, so it
    /// wins over everything else. Only one is returned, because a second modal
    /// drawn over the first would leave the one underneath clickable.
    fn overlays(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let entity = cx.entity();

        if let Some(pending) = &self.confirming {
            let prompt = pending.action.prompt();
            return Some(overlay::scrim().child(overlay::confirm_view(
                prompt,
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.confirm();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.confirming = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(invite) = &self.invite {
            let row = overlay::InviteRow {
                guild_name: invite
                    .preview
                    .as_ref()
                    .map(|preview| preview.guild_name.clone())
                    .unwrap_or_else(|| "Looking up invite...".to_string()),
                channel_name: invite
                    .preview
                    .as_ref()
                    .and_then(|preview| preview.channel_name.clone()),
                inviter: invite
                    .preview
                    .as_ref()
                    .and_then(|preview| preview.inviter.clone()),
                member_count: invite.preview.as_ref().and_then(|p| p.member_count),
                online_count: invite.preview.as_ref().and_then(|p| p.online_count),
                already_joined: invite
                    .preview
                    .as_ref()
                    .is_some_and(|preview| preview.already_joined),
                // No preview yet and no error means it is still in flight, so
                // the join button stays hidden rather than acting on nothing.
                status: invite
                    .error
                    .clone()
                    .or_else(|| invite.preview.is_none().then(|| "Resolving...".to_string())),
            };

            return Some(overlay::scrim().child(overlay::invite_view(
                &row,
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.accept_invite();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.invite = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some((prompt, text)) = &self.prompt {
            let (title, placeholder) = (prompt.title(), prompt.placeholder());
            return Some(overlay::scrim().child(overlay::text_prompt_view(
                title,
                placeholder,
                text.text(),
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.submit_prompt();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.prompt = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(text) = &self.editing_status {
            return Some(overlay::scrim().child(overlay::text_prompt_view(
                "Custom status",
                "What are you up to?",
                text.text(),
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.submit_custom_status();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.editing_status = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some((_, name)) = &self.renaming_folder {
            return Some(overlay::scrim().child(overlay::text_prompt_view(
                "Rename folder",
                "Type a name",
                name.text(),
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.submit_folder_rename();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.renaming_folder = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(switcher) = &self.switcher {
            return Some(
                overlay::scrim()
                    .items_start()
                    .pt(px(96.))
                    .child(switcher::switcher_view(switcher)),
            );
        }

        if let Some(picker) = &self.picker {
            let cursor = picker.cursor;
            return Some(overlay::scrim().child(emoji::picker_view(cursor, {
                let entity = entity.clone();
                move |glyph: &'static str, cx: &mut gpui::App| {
                    entity.update(cx, |workspace, cx| {
                        workspace.pick_emoji(glyph);
                        cx.notify();
                    });
                }
            })));
        }

        if let Some(picker) = &self.stream_picker {
            return Some(overlay::scrim().child(stream::picker_view(
                picker,
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.start_stream(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.stream_picker = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(devices) = &self.audio_devices {
            return Some(overlay::scrim().child(overlay::audio_devices_view(
                &devices.inputs,
                &devices.outputs,
                devices.selected_input.as_deref(),
                devices.selected_output.as_deref(),
                devices.error.as_deref(),
                {
                    let entity = entity.clone();
                    move |is_input, id, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            if is_input {
                                workspace.set_audio_device(Some(id.clone()), None);
                            } else {
                                workspace.set_audio_device(None, Some(id.clone()));
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.audio_devices = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some((_, glyph, users)) = &self.reaction_users {
            return Some(
                overlay::scrim().child(overlay::reaction_users_view(glyph, users, {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.reaction_users = None;
                            cx.notify();
                        });
                    }
                })),
            );
        }

        if let Some(mentions) = &self.inbox {
            let rows: Vec<_> = mentions
                .iter()
                .map(|mention| overlay::InboxRow {
                    author: mention.author.clone(),
                    content: mention.content.clone(),
                })
                .collect();

            return Some(overlay::scrim().child(overlay::inbox_view(
                &rows,
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.open_mention(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.dismiss_mention(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.inbox = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        None
    }

    fn content(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        if let Some(view) = &self.forum {
            return forum::forum_view(
                view,
                {
                    let entity = cx.entity();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.open_forum_post(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = cx.entity();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.toggle_forum_archived();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = cx.entity();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            let offset =
                                workspace.forum.as_ref().map(|f| f.next_offset).unwrap_or(0);
                            if let Some(forum) = &mut workspace.forum {
                                forum.loading = true;
                            }
                            workspace.request_forum_posts(offset);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = cx.entity();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.prompt = Some((Prompt::ForumPostTitle, Composer::default()));
                            cx.notify();
                        });
                    }
                },
            )
            .into_any_element();
        }

        self.chat_content(window, cx).into_any_element()
    }

    /// The ordinary message view.
    fn chat_content(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let channel_name = self
            .model
            .channels
            .get(self.model.selected_channel)
            .map(|c| c.name.clone())
            .unwrap_or_default();

        column()
            .flex_1()
            .h_full()
            .bg(rgb(active().surface))
            .child(
                header()
                    .child(
                        gpui::div()
                            .text_color(rgb(active().text_subtle))
                            .child(ChannelKind::Text.glyph()),
                    )
                    .child(
                        gpui::div()
                            .flex_1()
                            .text_size(px(scaled(text::BASE)))
                            .text_color(rgb(active().text))
                            .child(channel_name.clone()),
                    )
                    // Thread controls, shown only in a thread: archiving a
                    // regular channel is not a thing Discord permits.
                    .when(self.in_thread(), |header| {
                        header
                            .child(
                                gpui::div()
                                    .id("thread-follow")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_muted))
                                    .hover(|style| style.bg(rgb(active().surface_hover)))
                                    .child("follow")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_followed(true);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                gpui::div()
                                    .id("thread-notify-all")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_muted))
                                    .hover(|style| style.bg(rgb(active().surface_hover)))
                                    .child("all")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_notification_level(2);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                gpui::div()
                                    .id("thread-notify-mentions")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_muted))
                                    .hover(|style| style.bg(rgb(active().surface_hover)))
                                    .child("mentions")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_notification_level(4);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                gpui::div()
                                    .id("thread-notify-none")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_muted))
                                    .hover(|style| style.bg(rgb(active().surface_hover)))
                                    .child("none")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_notification_level(8);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                gpui::div()
                                    .id("thread-mute")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_muted))
                                    .hover(|style| style.bg(rgb(active().surface_hover)))
                                    .child("mute thread")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_muted(true);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                gpui::div()
                                    .id("thread-pin")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_muted))
                                    .hover(|style| style.bg(rgb(active().surface_hover)))
                                    .child("pin thread")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_pinned(true);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                gpui::div()
                                    .id("thread-rename")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_muted))
                                    .hover(|style| style.bg(rgb(active().surface_hover)))
                                    .child("rename")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.prompt =
                                            Some((Prompt::ThreadName, Composer::default()));
                                        cx.notify();
                                    })),
                            )
                            .child(
                                gpui::div()
                                    .id("thread-lock")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_muted))
                                    .hover(|style| style.bg(rgb(active().surface_hover)))
                                    .child("lock")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_locked(true);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                gpui::div()
                                    .id("thread-delete")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().danger))
                                    .hover(|style| style.bg(rgb(active().surface_hover)))
                                    .child("delete")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.delete_thread();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                gpui::div()
                                    .id("thread-archive")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_muted))
                                    .hover(|style| style.bg(rgb(active().surface_hover)))
                                    .child("archive")
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_archived(true);
                                        cx.notify();
                                    })),
                            )
                    })
                    .child(
                        gpui::div()
                            .id("channel-pins")
                            .px(px(space::SM))
                            .py(px(space::XS))
                            .rounded(px(layout::RADIUS))
                            .cursor_pointer()
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().text_muted))
                            .hover(|style| style.bg(rgb(active().surface_hover)))
                            .child("pins")
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.open_pins();
                                cx.notify();
                            })),
                    )
                    .child(
                        gpui::div()
                            .id("channel-mute")
                            .px(px(space::SM))
                            .py(px(space::XS))
                            .rounded(px(layout::RADIUS))
                            .cursor_pointer()
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(if self.channel_muted {
                                active().danger
                            } else {
                                active().text_muted
                            }))
                            .hover(|style| style.bg(rgb(active().surface_hover)))
                            .child(if self.channel_muted { "unmute" } else { "mute" })
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.toggle_channel_muted();
                                cx.notify();
                            })),
                    )
                    .when(self.can_call(), |header| {
                        let call_channel = self.nav.channel;
                        let call_name = channel_name.clone();
                        header.child(
                            gpui::div()
                                .id("dm-call")
                                .px(px(space::SM))
                                .py(px(space::XS))
                                .rounded(px(layout::RADIUS))
                                .cursor_pointer()
                                .bg(rgb(active().surface_hover))
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().success))
                                .child("call")
                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                    if let Some(channel_id) = call_channel {
                                        this.join_voice(channel_id, call_name.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(message_list(
                &self.messages,
                self.selected_message,
                &self.message_scroll,
                RenderOptions {
                    show_avatars: self.options.display.show_avatars,
                    circular_avatars: self.options.display.circular_avatars,
                    hour24: self.options.display.hour_format_24,
                    show_emoji: self.options.display.show_custom_emoji,
                    show_images: self.options.display.show_images
                        && !self.options.display.disable_image_preview,
                    previews: &self.attachment_previews,
                },
                self.has_newer_messages(),
                {
                    // Click handlers run with only an `App`, so the workspace is
                    // reached through its entity handle rather than captured.
                    let entity = cx.entity();
                    move |index, action, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.handle_message_action(index, action);
                            cx.notify();
                        });
                    }
                },
            ))
            .child(self.composer_row(window, cx))
    }

    fn composer_row(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.nav.channel.is_some() && self.handle.is_some();
        let placeholder = if !enabled {
            "Select a channel to start typing".to_string()
        } else {
            let name = self
                .model
                .channels
                .get(self.model.selected_channel)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            format!("Message #{name}  ·  ctrl-o to attach")
        };

        // The pickers sit above the composer, in a column with it, so they
        // push the input down rather than covering the text being typed.
        column()
            .w_full()
            // Staged files. Without this, attaching produced no visible change
            // at all and there was no way to remove one before sending.
            .children((!self.attachments.is_empty()).then(|| {
                let mut tray = row()
                    .w_full()
                    .gap(px(space::SM))
                    .px(px(space::MD))
                    .py(px(space::XS));
                for (index, upload) in self.attachments.iter().enumerate() {
                    tray = tray.child(
                        row()
                            .id(("staged", index))
                            .gap(px(space::XS))
                            .px(px(space::SM))
                            .py(px(space::XS))
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(active().surface_hover))
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().text))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(active().surface_active)))
                            .child(upload.filename.clone())
                            .child(gpui::div().text_color(rgb(active().danger)).child("x"))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.remove_attachment(index);
                                cx.notify();
                            })),
                    );
                }
                tray
            }))
            .children(self.attachment_error.as_ref().map(|error| {
                gpui::div()
                    .px(px(space::MD))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().danger))
                    .child(error.clone())
            }))
            // Config complaints, listed once so a typo in config.toml is not
            // indistinguishable from a setting that simply does nothing.
            .children(
                self.config_warnings
                    .iter()
                    .enumerate()
                    .map(|(slot, warning)| {
                        gpui::div()
                            .id(("config-warning", slot))
                            .px(px(space::MD))
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().danger))
                            .cursor_pointer()
                            // Clicking dismisses the whole list: they are startup
                            // notices, not a log to work through.
                            .child(format!("config: {warning}"))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.config_warnings.clear();
                                cx.notify();
                            }))
                    }),
            )
            // Whether the last settings write actually landed. Silently
            // failing to persist a preference is worse than saying so.
            .children(self.settings_note.as_ref().map(|note| {
                gpui::div()
                    .px(px(space::MD))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().text_subtle))
                    .child(note.clone())
            }))
            .children(self.slash.as_ref().map(slash_view))
            .children((!self.command_choices.is_empty()).then(|| {
                // Choices supplied by a bot for the argument in progress.
                // Listed rather than auto-inserted: the values are the bot's,
                // and picking one for the user would guess at intent.
                let mut list = column()
                    .w_full()
                    .rounded(px(layout::RADIUS))
                    .bg(rgb(active().surface))
                    .border_1()
                    .border_color(rgb(active().border));

                for (slot, choice) in self.command_choices.iter().enumerate().take(10) {
                    let value = choice.clone();
                    list = list.child(
                        gpui::div()
                            .id(("choice", slot))
                            .w_full()
                            .px(px(space::MD))
                            .py(px(space::XS))
                            .cursor_pointer()
                            .text_size(px(scaled(text::SM)))
                            .text_color(rgb(active().text_muted))
                            .hover(|style| style.bg(rgb(active().surface_hover)))
                            .child(choice.clone())
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.accept_command_choice(&value);
                                cx.notify();
                            })),
                    );
                }

                list
            }))
            .child(composer_view(
                &self.composer,
                self.focus.is_focused(window),
                enabled,
                &placeholder,
            ))
    }

    /// Put a bot-supplied choice into the composer, replacing the argument
    /// being typed.
    fn accept_command_choice(&mut self, value: &str) {
        let content = self.composer.text().to_string();
        // Only the last whitespace-separated token is replaced: earlier
        // arguments were already accepted and must survive.
        let head = content
            .rsplit_once(char::is_whitespace)
            .map(|(head, _)| head)
            .unwrap_or(&content);
        self.composer.set_text(&format!("{head} {value}"));
        self.command_choices.clear();
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        row()
            .w_full()
            .h(px(24.))
            .px(px(space::MD))
            .gap(px(space::SM))
            .bg(rgb(active().surface_sunken))
            .border_t_1()
            .border_color(rgb(active().border))
            .text_size(px(scaled(text::XS)))
            .text_color(rgb(active().text_subtle))
            .children(
                [
                    (0usize, "online", PresenceStatus::Online),
                    (1, "idle", PresenceStatus::Idle),
                    (2, "dnd", PresenceStatus::DoNotDisturb),
                    (3, "invisible", PresenceStatus::Offline),
                ]
                .map(|(slot, label, status)| {
                    gpui::div()
                        .id(("status", slot))
                        .px(px(space::SM))
                        .rounded(px(layout::RADIUS))
                        .cursor_pointer()
                        .text_color(rgb(if self.status == status {
                            active().text
                        } else {
                            active().text_subtle
                        }))
                        .child(label)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.set_status(status);
                            cx.notify();
                        }))
                }),
            )
            .child(
                gpui::div()
                    .id("custom-status")
                    .px(px(space::SM))
                    .rounded(px(layout::RADIUS))
                    .cursor_pointer()
                    .text_color(rgb(active().text_subtle))
                    .hover(|style| style.text_color(rgb(active().text)))
                    .child(if self.custom_status.is_empty() {
                        "set status".to_string()
                    } else {
                        self.custom_status.clone()
                    })
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.editing_status = Some(Composer::default());
                        cx.notify();
                    })),
            )
            .child(presence_dot(if self.model.connected {
                Presence::Online
            } else {
                Presence::Offline
            }))
            .child(gpui::div().flex_1().child(self.model.status_line.clone()))
            // Downloads live here rather than in a modal: they run alongside
            // whatever the user is doing, and a dialog would interrupt it.
            .children(
                self.downloads
                    .iter()
                    .enumerate()
                    .map(|(slot, (_, filename, progress))| {
                        gpui::div()
                            .id(("download", slot))
                            .px(px(space::SM))
                            .text_color(rgb(active().accent))
                            .child(match progress {
                                Some(fraction) => format!("{filename} {:.0}%", fraction * 100.0),
                                // No total means no honest percentage to show.
                                None => format!("{filename}..."),
                            })
                    }),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Sampled per render rather than observed: GPUI re-renders on
        // activation changes, so this stays current without a second channel.
        self.window_focused = window.is_window_active();

        // Copies are queued by action handlers, which have no clipboard
        // access, and flushed here where the context is available.
        if let Some(text) = self.pending_copy.take() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }

        column()
            .track_focus(&self.focus)
            .key_context("Workspace")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match &mut this.screen {
                    Screen::Login(login) => {
                        let key = event.keystroke.key.as_str();
                        let ctrl =
                            event.keystroke.modifiers.control || event.keystroke.modifiers.platform;

                        // ctrl-r toggles credential persistence on any sub-screen.
                        if key == "r" && ctrl {
                            let action = LoginAction::ToggleRemember;
                            this.handle_login_action(action, window, cx);
                            return;
                        }

                        match login.screen {
                            // ---- Picker: number-key or letter shortcuts ----
                            LoginScreen::Picker => {
                                let action = match key {
                                    "1" => Some(LoginAction::PickPassword),
                                    "2" => Some(LoginAction::PickToken),
                                    "3" => Some(LoginAction::PickQr),
                                    "4" | "d" => Some(LoginAction::PickDemo),
                                    _ => None,
                                };
                                if let Some(a) = action {
                                    this.handle_login_action(a, window, cx);
                                }
                            }

                            // ---- Password: two-field entry ----------------
                            LoginScreen::Password => {
                                match key {
                                    "escape" => {
                                        this.handle_login_action(LoginAction::Back, window, cx);
                                    }
                                    "tab" => {
                                        // Cycle focus between login and password fields.
                                        if let Screen::Login(l) = &mut this.screen {
                                            l.password.focused_field =
                                                l.password.focused_field.next();
                                        }
                                    }
                                    "enter" => {
                                        this.handle_login_action(
                                            LoginAction::SubmitPassword,
                                            window,
                                            cx,
                                        );
                                    }
                                    _ => {
                                        let pasted = (key == "v" && ctrl)
                                            .then(|| {
                                                cx.read_from_clipboard()
                                                    .and_then(|item| item.text())
                                            })
                                            .flatten();
                                        if let Screen::Login(l) = &mut this.screen {
                                            let field = l.password.focused_field;
                                            match field {
                                                PasswordField::Login => l
                                                    .password
                                                    .login
                                                    .handle_key_with_clipboard(event, pasted),
                                                PasswordField::Password => l
                                                    .password
                                                    .password
                                                    .handle_key_with_clipboard(event, pasted),
                                            };
                                        }
                                    }
                                }
                            }

                            // ---- MFA method select: number keys -----------
                            LoginScreen::MfaSelect => {
                                // Pick a method by number key or Escape to go back.
                                let methods: Vec<MfaMethod> = login
                                    .password
                                    .mfa
                                    .as_ref()
                                    .map(|c| c.methods.clone())
                                    .unwrap_or_default();
                                match key {
                                    "escape" => {
                                        this.handle_login_action(LoginAction::Back, window, cx);
                                    }
                                    "1" if !methods.is_empty() => {
                                        this.handle_login_action(
                                            LoginAction::PickMfaMethod(methods[0]),
                                            window,
                                            cx,
                                        );
                                    }
                                    "2" if methods.len() >= 2 => {
                                        this.handle_login_action(
                                            LoginAction::PickMfaMethod(methods[1]),
                                            window,
                                            cx,
                                        );
                                    }
                                    _ => {}
                                }
                            }

                            // ---- MFA code entry ---------------------------
                            LoginScreen::MfaCode => match key {
                                "escape" => {
                                    if let Screen::Login(l) = &mut this.screen {
                                        l.screen = LoginScreen::MfaSelect;
                                    }
                                }
                                "enter" => {
                                    this.handle_login_action(
                                        LoginAction::SubmitMfaCode,
                                        window,
                                        cx,
                                    );
                                }
                                _ => {
                                    let pasted = (key == "v" && ctrl)
                                        .then(|| {
                                            cx.read_from_clipboard().and_then(|item| item.text())
                                        })
                                        .flatten();
                                    if let Screen::Login(l) = &mut this.screen {
                                        l.password
                                            .mfa_code
                                            .handle_key_with_clipboard(event, pasted);
                                    }
                                }
                            },

                            // ---- Token entry ------------------------------
                            LoginScreen::Token => match key {
                                "escape" => {
                                    this.handle_login_action(LoginAction::Back, window, cx);
                                }
                                _ => {
                                    let pasted = (key == "v" && ctrl)
                                        .then(|| {
                                            cx.read_from_clipboard().and_then(|item| item.text())
                                        })
                                        .flatten();
                                    let submit = if let Screen::Login(l) = &mut this.screen {
                                        l.token.handle_key_with_clipboard(event, pasted)
                                    } else {
                                        false
                                    };
                                    if submit {
                                        this.handle_login_action(
                                            LoginAction::SubmitToken,
                                            window,
                                            cx,
                                        );
                                    }
                                }
                            },

                            // ---- QR scan: only Escape to cancel ----------
                            LoginScreen::QrScan => {
                                if key == "escape" {
                                    this.handle_login_action(LoginAction::Back, window, cx);
                                }
                            }
                        }
                        return; // consumed
                    }
                    Screen::Ready => {
                        let key = event.keystroke.key.as_str();

                        // Modal text entry takes the keyboard outright; the
                        // composer only takes unmodified characters, which
                        // `resolve` handles via `composer_live`.
                        let modal_text = this.pane_filter.is_some()
                            || this.prompt.is_some()
                            || this.editing_status.is_some()
                            || this.renaming_folder.is_some()
                            // The search panel owns typing whenever it is open;
                            // it has no separate focus flag.
                            || this.search.is_some();

                        if !modal_text || this.keymap.is_pending() {
                            let composer_live = this.focus_pane == Pane::Messages && !modal_text;
                            match this.keymap.resolve(event, composer_live) {
                                Resolution::Action(action) => {
                                    if keymap::apply(this, action, cx) {
                                        cx.notify();
                                        return;
                                    }
                                }
                                // Mid-sequence: swallow the key so a leader
                                // chord does not also type its own letters.
                                Resolution::Pending => {
                                    // Escape abandons the sequence even when it
                                    // is itself a valid next chord; otherwise a
                                    // half-typed leader has no way out.
                                    if key == "escape" {
                                        this.keymap.cancel();
                                    }
                                    cx.notify();
                                    return;
                                }
                                Resolution::Unbound => {}
                            }
                        }

                        if this.confirming.is_some() {
                            match key {
                                "enter" => this.confirm(),
                                "escape" => this.confirming = None,
                                _ => {}
                            }
                        } else if let Some(switcher) = &mut this.switcher {
                            match key {
                                "escape" => this.switcher = None,
                                "up" => switcher.move_selection(-1),
                                "down" => switcher.move_selection(1),
                                "enter" => this.activate_switcher(),
                                _ => {
                                    let pasted = (key == "v"
                                        && (event.keystroke.modifiers.control
                                            || event.keystroke.modifiers.platform))
                                        .then(|| {
                                            cx.read_from_clipboard().and_then(|item| item.text())
                                        })
                                        .flatten();
                                    switcher.query.handle_key_with_clipboard(event, pasted);
                                    this.rerank_switcher();
                                }
                            }
                        } else if key == "k"
                            && (event.keystroke.modifiers.control
                                || event.keystroke.modifiers.platform)
                        {
                            this.open_switcher();
                        } else if this.prompt.is_some() {
                            match key {
                                "escape" => this.prompt = None,
                                "enter" => this.submit_prompt(),
                                _ => {
                                    let pasted = (key == "v"
                                        && (event.keystroke.modifiers.control
                                            || event.keystroke.modifiers.platform))
                                        .then(|| {
                                            cx.read_from_clipboard().and_then(|item| item.text())
                                        })
                                        .flatten();
                                    if let Some((_, text)) = &mut this.prompt {
                                        text.handle_key_with_clipboard(event, pasted);
                                    }
                                }
                            }
                        } else if this.editing_status.is_some() {
                            match key {
                                "escape" => this.editing_status = None,
                                "enter" => this.submit_custom_status(),
                                _ => {
                                    let pasted = (key == "v"
                                        && (event.keystroke.modifiers.control
                                            || event.keystroke.modifiers.platform))
                                        .then(|| {
                                            cx.read_from_clipboard().and_then(|item| item.text())
                                        })
                                        .flatten();
                                    if let Some(text) = &mut this.editing_status {
                                        text.handle_key_with_clipboard(event, pasted);
                                    }
                                }
                            }
                        } else if this.renaming_folder.is_some() {
                            match key {
                                "escape" => this.renaming_folder = None,
                                "enter" => this.submit_folder_rename(),
                                _ => {
                                    let pasted = (key == "v"
                                        && (event.keystroke.modifiers.control
                                            || event.keystroke.modifiers.platform))
                                        .then(|| {
                                            cx.read_from_clipboard().and_then(|item| item.text())
                                        })
                                        .flatten();
                                    if let Some((_, name)) = &mut this.renaming_folder {
                                        name.handle_key_with_clipboard(event, pasted);
                                    }
                                }
                            }
                        } else if this.stream_picker.is_some() && key == "escape" {
                            this.stream_picker = None;
                        } else if this.picker.is_some() {
                            // The picker owns the keyboard while open.
                            match key {
                                "escape" => this.picker = None,
                                "left" => this.move_picker(-1),
                                "right" => this.move_picker(1),
                                // The grid reflows with width, so up/down move
                                // by a nominal row rather than a measured one.
                                "up" => this.move_picker(-8),
                                "down" => this.move_picker(8),
                                "enter" => {
                                    let glyph = emoji::flat()
                                        .get(this.picker.as_ref().map_or(0, |p| p.cursor))
                                        .copied();
                                    if let Some(glyph) = glyph {
                                        this.pick_emoji(glyph);
                                    }
                                }
                                _ => {}
                            }
                        } else if key == "comma"
                            && (event.keystroke.modifiers.control
                                || event.keystroke.modifiers.platform)
                        {
                            this.open_settings_window(cx);
                        } else if key == "a"
                            && event.keystroke.modifiers.control
                            && event.keystroke.modifiers.shift
                        {
                            this.mark_all_read();
                        } else if key == "o" && event.keystroke.modifiers.control {
                            this.attach_files(cx);
                        } else if key == "q"
                            && event.keystroke.modifiers.control
                            && event.keystroke.modifiers.shift
                        {
                            this.sign_out(cx);
                        } else if key == "p"
                            && event.keystroke.modifiers.control
                            && event.keystroke.modifiers.shift
                        {
                            this.open_own_profile();
                        } else if key == "e"
                            && event.keystroke.modifiers.control
                            && event.keystroke.modifiers.shift
                        {
                            this.compose_externally(cx);
                        } else if event.keystroke.modifiers.control
                            && event.keystroke.modifiers.shift
                            && matches!(key, "-" | "=" | "+")
                        {
                            // ctrl-shift +/-: output volume. Shifted so it does
                            // not collide with the zoom bindings on the same
                            // keys, which are far more frequently used.
                            this.adjust_output_volume(if key == "-" { -5 } else { 5 });
                        } else if event.keystroke.modifiers.control && matches!(key, "d" | "u") {
                            // ctrl-d / ctrl-u: half page, as in vim and less.
                            this.scroll_by_pages(if key == "d" { 0.5 } else { -0.5 });
                        } else if event.keystroke.modifiers.control && matches!(key, "home" | "end")
                        {
                            if key == "home" {
                                this.message_scroll
                                    .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
                            } else {
                                this.message_scroll.scroll_to_bottom();
                            }
                        } else if event.keystroke.modifiers.control
                            && matches!(key, "1" | "2" | "3")
                        {
                            let pane = match key {
                                "1" => Pane::Guilds,
                                "2" => Pane::Channels,
                                _ => Pane::Members,
                            };
                            this.toggle_pane(pane);
                        } else if this.pane_filter.is_some() && !event.keystroke.modifiers.control {
                            // While filtering, the pane owns typing so the
                            // query does not leak into the composer.
                            if key == "escape" {
                                this.pane_filter = None;
                            } else if let Some(filter) = &mut this.pane_filter
                                && filter.handle_key(event)
                            {
                                this.pane_filter = None;
                            } else if this.focus_pane == Pane::Members {
                                // Filtering members searches the server too:
                                // the member list holds only the ranges this
                                // client subscribed to, so filtering alone
                                // cannot find someone further down it.
                                let query = this
                                    .pane_filter
                                    .as_ref()
                                    .map(|filter| filter.text().to_string())
                                    .unwrap_or_default();
                                this.search_members(query);
                            }
                        } else if this.focus_pane == Pane::Messages
                            && matches!(
                                key,
                                "up" | "down" | "escape" | "r" | "e" | "y" | "p" | "delete"
                            )
                            && this.composer.is_empty()
                        {
                            // Only when the composer is empty: otherwise these
                            // are ordinary characters being typed.
                            match key {
                                "up" => this.move_message_selection(-1),
                                "down" => this.move_message_selection(1),
                                "escape" => this.clear_message_selection(),
                                "r" => this.act_on_selection(MessageAction::Reply),
                                "e" => this.act_on_selection(MessageAction::Edit),
                                "y" => this.act_on_selection(MessageAction::CopyText),
                                "p" => this.act_on_selection(MessageAction::TogglePin),
                                _ => this.act_on_selection(MessageAction::Delete),
                            }
                        } else if event.keystroke.modifiers.control
                            && event.keystroke.modifiers.shift
                            && matches!(key, "left" | "right")
                        {
                            this.resize_pane(if key == "right" { 20 } else { -20 });
                        } else if event.keystroke.modifiers.control
                            && matches!(key, "=" | "+" | "-" | "0")
                        {
                            match key {
                                "-" => this.adjust_zoom(-0.1),
                                "0" => crate::theme::set_zoom(1.0),
                                _ => this.adjust_zoom(0.1),
                            }
                        } else if key == "t"
                            && event.keystroke.modifiers.control
                            && event.keystroke.modifiers.shift
                        {
                            this.toggle_tts();
                        } else if key == "slash" && event.keystroke.modifiers.control {
                            this.toggle_pane_filter();
                        } else if key == "tab" {
                            this.cycle_focus(!event.keystroke.modifiers.shift);
                        } else if key == "q"
                            && event.keystroke.modifiers.control
                            && !event.keystroke.modifiers.shift
                        {
                            this.quit(cx);
                        } else if key == "l"
                            && event.keystroke.modifiers.control
                            && event.keystroke.modifiers.shift
                        {
                            this.toggle_debug_log();
                        } else if key == "i" && event.keystroke.modifiers.control {
                            this.open_inbox();
                        } else if key == "f" && event.keystroke.modifiers.control {
                            this.toggle_search();
                        } else if this.profile.is_some() && key == "escape" {
                            this.profile = None;
                        } else if this.search.is_some() && key == "escape" {
                            this.search = None;
                        } else if let Some(search) = &mut this.search {
                            // While the search panel is open it owns the
                            // keyboard, so typing does not leak into the
                            // composer behind it.
                            if search.input.handle_key(event) {
                                this.run_search();
                            }
                        } else if key == "escape" {
                            // Escape dismisses an active reply or edit first;
                            // only once nothing is open does it mark the
                            // channel read, matching the TUI's ordering.
                            if this.replying_to.is_some() || this.editing.is_some() {
                                this.cancel_compose_context();
                            } else {
                                this.mark_read();
                            }
                        } else if this.slash.is_some()
                            && matches!(key, "up" | "down" | "tab" | "escape")
                        {
                            match key {
                                "escape" => this.slash = None,
                                "up" => {
                                    if let Some(picker) = &mut this.slash {
                                        picker.move_selection(-1);
                                    }
                                }
                                "down" => {
                                    if let Some(picker) = &mut this.slash {
                                        picker.move_selection(1);
                                    }
                                }
                                // Tab completes; Enter still sends, so a
                                // fully-typed command is not intercepted.
                                _ => this.accept_slash(),
                            }
                        } else {
                            // Read the clipboard only for the paste chord, so
                            // ordinary typing does not hit the platform on
                            // every keystroke.
                            let pasted = (event.keystroke.key == "v"
                                && (event.keystroke.modifiers.control
                                    || event.keystroke.modifiers.platform))
                                .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                                .flatten();

                            let send = this.composer.handle_key_with_clipboard(event, pasted);

                            // The composer reports copy/cut rather than
                            // reaching the clipboard itself, so perform it here.
                            // Taken once: a second take would clear the intent
                            // and silently turn every cut into a copy.
                            let intent = this.composer.take_clipboard_intent();
                            if intent != ClipboardIntent::None
                                && let Some(selected) = this.composer.selected_text()
                            {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    selected.to_string(),
                                ));
                                if intent == ClipboardIntent::Cut {
                                    this.composer.cut_selection();
                                }
                            }

                            if send {
                                this.send_message();
                            } else {
                                this.refresh_slash();
                                if !this.composer.is_empty() {
                                    this.notify_typing();
                                }
                            }
                        }
                    }
                }
                cx.notify();
            }))
            .size_full()
            .bg(rgb(active().bg))
            .text_size(px(scaled(text::BASE)))
            .when(matches!(self.screen, Screen::Login(_)), |d| {
                let Screen::Login(login) = &self.screen else {
                    return d;
                };
                d.child(login_view(login, cx))
            })
            .when(matches!(self.screen, Screen::Ready), |d| {
                d.child(
                    row()
                        .flex_1()
                        .w_full()
                        .overflow_hidden()
                        .when(self.ui_state.guild_pane_visible, |d| {
                            d.child(self.guild_rail(cx))
                        })
                        .when(self.ui_state.channel_pane_visible, |d| {
                            d.child(self.channel_sidebar(cx))
                        })
                        .child(self.content(window, cx))
                        // Right column precedence: profile, then search, then
                        // the member list. Only one occupies it at a time so
                        // the message area keeps a readable width.
                        .when(self.profile.is_some(), |d| d.child(self.profile_pane(cx)))
                        .when(self.profile.is_none() && self.search.is_some(), |d| {
                            d.child(self.search_pane(cx))
                        })
                        .when(
                            self.profile.is_none() && self.search.is_none() && self.shows_members(),
                            |d| d.child(self.member_pane(cx)),
                        ),
                )
                .child(self.status_bar(cx))
                .children(self.overlays(cx))
            })
    }
}

/// Pick a decoder from a URL's file extension.
///
/// Discord's CDN serves the content type in a header the image bytes do not
/// carry, so the extension is what is left. An unknown one returns `None`
/// rather than defaulting to PNG: a wrong format decodes to nothing at all.
pub fn image_format_for(url: &str) -> Option<gpui::ImageFormat> {
    // Query strings are always present on CDN links, and would otherwise be
    // part of the "extension".
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();

    match extension.as_str() {
        "png" => Some(gpui::ImageFormat::Png),
        "jpg" | "jpeg" => Some(gpui::ImageFormat::Jpeg),
        "webp" => Some(gpui::ImageFormat::Webp),
        "gif" => Some(gpui::ImageFormat::Gif),
        "svg" => Some(gpui::ImageFormat::Svg),
        "bmp" => Some(gpui::ImageFormat::Bmp),
        "tiff" | "tif" => Some(gpui::ImageFormat::Tiff),
        _ => None,
    }
}
