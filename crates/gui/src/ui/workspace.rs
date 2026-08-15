//! The main three-pane workspace: guild rail, channel sidebar, content area,
//! plus the member list, search and emoji panels that share the right column.
//!
//! Rendering reads only from `WorkspaceModel`, which `model::projection`
//! rebuilds from `DiscordState` on every snapshot revision. Nothing here
//! touches core types directly except to issue commands.

use concord::config::{self, AppOptions, CredentialStoreMode};
use concord::discord::{
    AppCommand, AppEvent, AttachmentDownloadId, BuiltinSlashCommandParse,
    BuiltinSlashCommandSubmit, DownloadAttachmentSource, ForumPostArchiveState,
    GlobalUserProfileUpdate, GuildUserProfileUpdate, Id, MAX_UPLOAD_ATTACHMENT_COUNT,
    MediaPlaybackSource, MediaPlaybackTarget, MessageAttachmentUpload, MessageSearchQuery,
    MuteDuration, ReactionEmoji, ReplyReference, StreamCaptureTargetsRequestId, UserProfileUpdate,
    VoiceScope, marker, next_message_nonce, parse_builtin_slash_command,
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

use crate::theme::{self, Presence, active, layout, space, text};
use crate::ui::chrome::{
    avatar, avatar_with_url, column, header, hint, panel_sunken, presence_dot, row, section_label,
    sidebar_row, voice_participant_row,
};
use crate::ui::composer::{ClipboardIntent, Composer, composer_view};
use crate::ui::emoji::{self, EmojiPicker};
use crate::ui::forum::{self, ForumPost, ForumView};
use crate::ui::login::{Login, LoginEvent, LoginHandle, LoginScreen, PasswordField, login_view};
use crate::ui::messages::{MessageAction, message_list};
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
}

pub struct ChannelEntry {
    pub id: Option<Id<marker::ChannelMarker>>,
    /// Newest message, needed to mark the channel read.
    pub last_message: Option<Id<marker::MessageMarker>>,
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
    pub name: String,
    pub muted: bool,
    pub deafened: bool,
    pub streaming: bool,
    pub speaking: bool,
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
    Login(Login),
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
        let options = config::load_options().unwrap_or_default();

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

        self.screen = Screen::Login(Login::default());
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
        self.slash = SlashPicker::for_input(self.composer.text());
    }

    /// Accept the highlighted completion.
    fn accept_slash(&mut self) {
        if let Some(replacement) = self.slash.as_ref().and_then(|picker| picker.completion()) {
            self.composer.set_text(replacement);
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
            // Still being typed, or not a command at all - send as written.
            BuiltinSlashCommandParse::Incomplete | BuiltinSlashCommandParse::NotBuiltin => false,
        }
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

        handle.send(AppCommand::SendMessage {
            channel_id,
            nonce: next_message_nonce(),
            content,
            reply_to,
            attachments: std::mem::take(&mut self.attachments),
        });
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

        if let Some((voice_channel_id, _)) = &self.voice_channel {
            if let Some(channel) = self
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
                        name: user_name,
                        muted: self.self_mute,
                        deafened: self.self_deaf,
                        streaming: false,
                        speaking: !self.self_mute,
                    });
                }
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
                input_source: None,
                output_source: None,
                allow_microphone_transmit: true,
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
            MessageAction::Delete => self.delete_message(message_id),
            MessageAction::LoadOlder => self.load_older_messages(),
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
            MessageAction::TogglePin => {
                let pinned = self
                    .messages
                    .get(index)
                    .map(|row| row.pinned)
                    .unwrap_or(false);
                self.set_pinned(index, !pinned);
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
    fn profile_pane(&self) -> impl IntoElement {
        let Some((user_id, view)) = &self.profile else {
            return gpui::div();
        };

        match view {
            Some(view) => {
                gpui::div().child(profile_view(view, self.options.display.circular_avatars))
            }
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
                .text_size(px(text::SM))
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
                    .text_size(px(text::SM))
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
                    .text_size(px(text::XS))
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
                            .text_size(px(text::XS))
                            .text_color(rgb(active().accent))
                            .child(result.author.clone()),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(text::SM))
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
                                    .text_size(px(text::XS))
                                    .text_color(rgb(active().success))
                                    .child("Voice Connected"),
                            )
                            .child(
                                gpui::div()
                                    .text_size(px(text::XS))
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
                            .child("🖥"),
                    )
                    .child(
                        gpui::div()
                            .id("card-activity")
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
                            .child("🎮"),
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
                            .relative()
                            .child(avatar(32., &user_name))
                            .child(presence_dot(Presence::Online)),
                    )
                    .child(
                        column()
                            .overflow_hidden()
                            .child(
                                gpui::div()
                                    .text_size(px(text::SM))
                                    .text_color(rgb(active().text))
                                    .child(user_name),
                            )
                            .child(
                                gpui::div()
                                    .text_size(px(text::XS))
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
        let mut switcher = Switcher::default();
        if let Some(state) = &self.last_state {
            switcher.rank(projection::switcher_candidates(state));
        }
        self.switcher = Some(switcher);
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

    /// Switch the open guild, clearing the channel selection.
    pub fn open_guild(&mut self, guild_id: Option<Id<marker::GuildMarker>>) {
        self.nav.selection = match guild_id {
            Some(id) => Selection::Guild(id),
            None => Selection::DirectMessages,
        };
        self.nav.channel = None;
        self.messages.clear();

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

        for (index, guild) in self.model.guilds.iter().enumerate() {
            let selected = index == self.model.selected_guild;
            let guild_id = guild.id;
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
                    }),
            );
        }

        rail
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
            .text_size(px(text::BASE))
            .text_color(rgb(active().text))
            .child(guild_name);

        let mut list = column()
            .id("channel-list")
            .flex_1()
            .w_full()
            .pt(px(space::XS))
            .gap(px(1.))
            .overflow_y_scroll();

        for (index, channel) in self.model.channels.iter().enumerate() {
            if channel.kind == ChannelKind::Category {
                list = list.child(section_label(channel.name.clone()));
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
                        .text_size(px(text::XS))
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
                list = list.child(voice_participant_row(
                    &participant.name,
                    participant.muted,
                    participant.deafened,
                    participant.streaming,
                    participant.speaking,
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
                    .text_size(px(text::XS))
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
                        .text_size(px(text::XS))
                        .text_color(rgb(active().on_accent))
                        .child("BOT"),
                );
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
                            .text_size(px(text::BASE))
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
                                    .text_size(px(text::XS))
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
                                    .id("thread-archive")
                                    .px(px(space::SM))
                                    .py(px(space::XS))
                                    .rounded(px(layout::RADIUS))
                                    .cursor_pointer()
                                    .text_size(px(text::XS))
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
                            .text_size(px(text::XS))
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
                            .text_size(px(text::XS))
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
                                .text_size(px(text::XS))
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
                self.options.display.show_avatars,
                self.options.display.circular_avatars,
                self.options.display.hour_format_24,
                self.options.display.show_custom_emoji,
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

        composer_view(
            &self.composer,
            self.focus.is_focused(window),
            enabled,
            &placeholder,
        )
    }

    fn status_bar(&self) -> impl IntoElement {
        row()
            .w_full()
            .h(px(24.))
            .px(px(space::MD))
            .gap(px(space::SM))
            .bg(rgb(active().surface_sunken))
            .border_t_1()
            .border_color(rgb(active().border))
            .text_size(px(text::XS))
            .text_color(rgb(active().text_subtle))
            .child(presence_dot(if self.model.connected {
                Presence::Online
            } else {
                Presence::Offline
            }))
            .child(self.model.status_line.clone())
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
                                    drop(login); // release borrow before calling
                                    this.handle_login_action(a, window, cx);
                                }
                            }

                            // ---- Password: two-field entry ----------------
                            LoginScreen::Password => {
                                match key {
                                    "escape" => {
                                        drop(login);
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
                                        drop(login);
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
                                        drop(login);
                                        this.handle_login_action(LoginAction::Back, window, cx);
                                    }
                                    "1" if !methods.is_empty() => {
                                        drop(login);
                                        this.handle_login_action(
                                            LoginAction::PickMfaMethod(methods[0]),
                                            window,
                                            cx,
                                        );
                                    }
                                    "2" if methods.len() >= 2 => {
                                        drop(login);
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
                                    drop(login);
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
                                    drop(login);
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
                                    drop(login);
                                    this.handle_login_action(LoginAction::Back, window, cx);
                                }
                            }
                        }
                        return; // consumed
                    }
                    Screen::Ready => {
                        let key = event.keystroke.key.as_str();

                        if let Some(switcher) = &mut this.switcher {
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
                            }
                        }
                    }
                }
                cx.notify();
            }))
            .size_full()
            .bg(rgb(active().bg))
            .text_size(px(text::BASE))
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
                        .child(self.guild_rail(cx))
                        .child(self.channel_sidebar(cx))
                        .child(self.content(window, cx))
                        // Right column precedence: profile, then search, then
                        // the member list. Only one occupies it at a time so
                        // the message area keeps a readable width.
                        .when(self.profile.is_some(), |d| d.child(self.profile_pane()))
                        .when(self.profile.is_none() && self.search.is_some(), |d| {
                            d.child(self.search_pane(cx))
                        })
                        .when(
                            self.profile.is_none() && self.search.is_none() && self.shows_members(),
                            |d| d.child(self.member_pane(cx)),
                        ),
                )
                .child(self.status_bar())
            })
    }
}
