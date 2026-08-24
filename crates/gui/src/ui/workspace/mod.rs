//! The main three-pane workspace: guild rail, channel sidebar, content area,
//! plus the member list, search and emoji panels that share the right column.
//!
//! Rendering reads only from `WorkspaceModel`, which `model::projection`
//! rebuilds from `DiscordState` on every snapshot revision. Nothing here
//! touches core types directly except to issue commands.

use concord::config::{self, AppOptions, UiStateOptions};
use concord::discord::{
    ActivityInfo, ApplicationCommandInfo, AttachmentDownloadId, Id, MessageAttachmentUpload,
    PresenceStatus, VoiceScope, marker,
    password_auth::{MfaMethod, PasswordAuthEvent},
    qr_auth::QrEvent,
};
use concord_ui::model::AttachmentViewerZoom;
use gpui::{Context, FocusHandle, prelude::*};

use crate::model::message::MessageRow;
use crate::model::projection::{Navigation, Selection};
use crate::session::SessionHandle;

use crate::keymap::Keymap;
use crate::theme::{self, Presence};
use crate::ui::composer::Composer;
use crate::ui::emoji::EmojiPicker;
use crate::ui::forum::ForumView;
use crate::ui::profile::ProfileView;
use crate::ui::slash::SlashPicker;
use crate::ui::stream::StreamPicker;
use crate::ui::switcher::Switcher;

pub mod actions;
mod channel_sidebar;
mod events;
mod guild_rail;
mod keys;
mod member_pane;
mod overlays;
mod profile_pane;
mod rendering;
#[cfg(test)]
mod tests;

mod types;

pub use types::*;

/// The workspace itself: every piece of live interface state, and the
/// behaviour that acts on it.
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
    /// Whether the log should keep itself pinned to the newest message.
    ///
    /// Set when a channel opens and cleared as soon as the reader scrolls
    /// back, so arriving pictures - which change the list's height after it
    /// has been laid out - do not leave the view stranded mid-backlog, and
    /// do not yank someone away from what they were reading either.
    pub follow_bottom: bool,
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
    /// Index of the active tab within `tabs`.
    pub active_tab: usize,
    /// Channels held open as tabs, in strip order.
    ///
    /// A tab keeps its own draft and scroll position, which is the point:
    /// switching away and back should land where you left, mid-sentence.
    pub tabs: Vec<ChannelTab>,
    /// A risk warning awaiting an answer, with the "don't ask again" box as
    /// currently ticked.
    pub risk: Option<(RiskAction, bool)>,
    /// The guild's bans, once asked for. `None` while the panel is closed.
    pub bans: Option<BanListView>,
    /// The server-management panel, once opened.
    pub server_management: Option<ServerManagementView>,
    /// The soundboard picker, once opened.
    pub soundboard: Option<SoundboardView>,
    pub connections: Option<ConnectionsView>,
    /// Privacy and safety. No fetch of its own - the values arrive with READY,
    /// so the panel is either open or not.
    pub privacy_open: bool,
    /// Servers Discord will show anyone, for finding one without a link.
    pub discovered: Vec<concord::discord::DiscoverableGuild>,
    pub discovering: bool,
    /// Whose camera and screen share to stop showing. Local only, like the
    /// mute beside it - neither Discord nor the person is told.
    pub video_hidden: std::collections::BTreeSet<Id<marker::UserMarker>>,
    /// The stage running in the channel whose topic is being edited, if any.
    /// Decides which of Discord's three stage endpoints the form uses.
    pub stage_running: Option<concord::discord::StageInstance>,
    pub access: Option<AccessView>,
    pub account: Option<AccountView>,
    /// A role's permissions, once opened.
    pub permission_grid: Option<PermissionGridView>,
    /// An image being viewed full size.
    pub viewing_image: Option<ImageViewerView>,
    /// A channel awaiting delete confirmation.
    pub deleting_channel: Option<Id<marker::ChannelMarker>>,
    /// An open context menu: where it is, and what it acts on.
    pub context_menu: Option<ContextMenu>,
    /// Roles being edited for a member: who, and the set as edited so far.
    pub editing_roles: Option<(Id<marker::UserMarker>, Vec<Id<marker::RoleMarker>>)>,
    /// Stickers staged for the next send. Discord accepts at most three.
    pub pending_stickers: Vec<Id<marker::StickerMarker>>,
    /// Sticker picker, open while choosing one.
    pub sticker_picker: bool,
    /// Invite being previewed, once resolved or while resolving.
    pub invite: Option<InviteState>,
    /// Custom status as last set, shown in the status bar.
    pub custom_status: String,
    /// Custom status being typed, when the editor is open.
    pub editing_status: Option<Composer>,
    /// The activity being composed, if the editor is open.
    pub editing_activity: Option<ActivityDraft>,
    /// What is currently being broadcast, so the editor reopens on it.
    pub current_activity: Option<ActivityInfo>,
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
    /// Self profile popout card in the bottom-left corner with status selector.
    pub self_profile_popout: bool,
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

        // Language, before anything renders: every label reads it, and
        // switching mid-frame would leave a half-translated window.
        concord::i18n::set_language(
            options
                .display
                .language
                .unwrap_or_else(concord::i18n::language_from_system),
        );

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
            follow_bottom: true,
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
            tabs: Vec::new(),
            active_tab: 0,
            risk: None,
            bans: None,
            server_management: None,
            soundboard: None,
            connections: None,
            privacy_open: false,
            discovered: Vec::new(),
            discovering: false,
            video_hidden: std::collections::BTreeSet::new(),
            stage_running: None,
            access: None,
            account: None,
            permission_grid: None,
            viewing_image: None,
            deleting_channel: None,
            context_menu: None,
            editing_roles: None,
            pending_stickers: Vec::new(),
            sticker_picker: false,
            invite: None,
            custom_status: String::new(),
            editing_status: None,
            editing_activity: None,
            current_activity: None,
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
            self_profile_popout: false,
            window_focused: true,
            options,
            settings_note: None,
            attachments: Vec::new(),
            attachment_error: None,
            last_state: None,
            focus: cx.focus_handle(),
        }
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
