//! The main three-pane workspace: guild rail, channel sidebar, content area,
//! plus the member list, search and emoji panels that share the right column.
//!
//! Rendering reads only from `WorkspaceModel`, which `model::projection`
//! rebuilds from `DiscordState` on every snapshot revision. Nothing here
//! touches core types directly except to issue commands.

use concord::config::CredentialStoreMode;
use concord::discord::{
    AppCommand, AppEvent, Id, MAX_UPLOAD_ATTACHMENT_COUNT, MessageAttachmentUpload,
    MessageSearchQuery, ReactionEmoji, ReplyReference, VoiceScope, marker, next_message_nonce,
};
use concord::token_store;
use gpui::{
    Context, FocusHandle, KeyDownEvent, PathPromptOptions, Window, WindowHandle, prelude::*, px,
    rgb,
};
use tokio::sync::mpsc;

use crate::model::message::{self, MessageRow};
use crate::model::projection::{self, Navigation, Selection};
use crate::notify;
use crate::session::{SessionHandle, Update};

use crate::theme::{DARK, Presence, layout, space, text};
use crate::ui::chrome::{
    avatar, avatar_with_url, column, header, hint, panel_sunken, presence_dot, row, section_label,
    sidebar_row, voice_participant_row,
};
use crate::ui::composer::{Composer, composer_view};
use crate::ui::emoji::{self, EmojiPicker};
use crate::ui::login::{Login, login_view};
use crate::ui::messages::{MessageAction, message_list};
use crate::ui::profile::{ProfileView, profile_view};

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
    fn glyph(self) -> &'static str {
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
    /// Message being edited. While set, the composer edits instead of sends.
    pub editing: Option<Id<marker::MessageMarker>>,
    /// Channel the user is connected to by voice, if any.
    pub voice_channel: Option<(Id<marker::ChannelMarker>, String)>,
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
    /// Files staged for the next send.
    pub attachments: Vec<MessageAttachmentUpload>,
    /// Reason the last staging attempt failed, shown above the composer.
    pub attachment_error: Option<String>,
    focus: FocusHandle,
}

impl Workspace {
    pub fn new(model: WorkspaceModel, screen: Screen, cx: &mut Context<Self>) -> Self {
        Self {
            screen,
            model,
            handle: None,
            nav: Navigation::default(),
            messages: Vec::new(),
            composer: Composer::default(),
            typing: Vec::new(),
            replying_to: None,
            editing: None,
            voice_channel: None,
            self_mute: false,
            self_deaf: false,
            search: None,
            picker: None,
            profile: None,
            window_focused: true,
            attachments: Vec::new(),
            attachment_error: None,
            focus: cx.focus_handle(),
        }
    }

    /// Send the composer's contents to the open channel.
    ///
    /// The nonce lets the core match the gateway echo back to this send, so
    /// the message does not briefly appear twice.
    fn send_message(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };

        let content = self.composer.take();

        // A message may be attachments only, but never entirely empty.
        if content.trim().is_empty() && self.attachments.is_empty() {
            return;
        }

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
                mention_author: true,
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
                            workspace.model = projection::project(&state, &workspace.nav, true);
                            let guild_id = match workspace.nav.selection {
                                Selection::Guild(id) => Some(id),
                                Selection::DirectMessages => None,
                            };

                            if let Some((user_id, _)) = workspace.profile {
                                let view = projection::project_profile(&state, user_id, guild_id);
                                workspace.profile = Some((user_id, view));
                            }

                            (workspace.messages, workspace.typing) = match workspace.nav.channel {
                                Some(channel_id) => (
                                    message::project_messages(
                                        &state,
                                        channel_id,
                                        state.current_user_id(),
                                    ),
                                    projection::typing_names(&state, channel_id, guild_id),
                                ),
                                None => (Vec::new(), Vec::new()),
                            };
                        }
                        Update::Event(event, state) => {
                            // The core owns the mute/mention rules; the GUI
                            // only adds "not the channel you are reading".
                            if let Some(notification) = notify::notification_for(
                                &state,
                                &event,
                                workspace.nav.channel,
                                workspace.window_focused,
                            ) {
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

        let Some(handle) = &self.handle else {
            return;
        };

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

    /// Start a session from a token entered on the login screen.
    fn submit_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Login(login) = &mut self.screen else {
            return;
        };
        if !login.is_submittable() {
            return;
        }

        let token = login.input.take();
        let remember = login.remember;
        login.connecting = true;
        login.error = None;

        // Persisting is best-effort: a failure to write the credential store
        // must not block a session that would otherwise work.
        if remember {
            let _ = token_store::save_token(&token, CredentialStoreMode::default());
        }

        match crate::session::spawn(token) {
            Ok((updates, handle)) => {
                self.attach(handle);
                self.screen = Screen::Ready;
                self.model.status_line = "connecting…".to_string();

                if let Some(window_handle) = window.window_handle().downcast::<Workspace>() {
                    Workspace::pump(window_handle, updates, cx);
                }
            }
            Err(error) => {
                if let Screen::Login(login) = &mut self.screen {
                    login.connecting = false;
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

    /// Join a voice channel, leaving any current one first.
    ///
    /// Only guild voice is wired: DM calls use VoiceScope::Private and a
    /// different entry point in the sidebar, which does not exist yet.
    pub fn join_voice(&mut self, channel_id: Id<marker::ChannelMarker>, name: String) {
        let Selection::Guild(guild_id) = self.nav.selection else {
            return;
        };

        // Leave first, while nothing else borrows the handle.
        if self.voice_channel.is_some() {
            self.leave_voice();
        }

        let Some(handle) = &self.handle else {
            return;
        };

        handle.send(AppCommand::JoinVoiceChannel {
            scope: VoiceScope::Guild(guild_id),
            channel_id,
            self_mute: self.self_mute,
            self_deaf: self.self_deaf,
            input_source: None,
            output_source: None,
            allow_microphone_transmit: true,
            // Audio tuning lives in settings, which does not exist yet; the
            // core's defaults are the right starting point.
            noise_suppression: true,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
            participant_playback_settings: Vec::new(),
        });
        self.voice_channel = Some((channel_id, name));
    }

    pub fn leave_voice(&mut self) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };
        handle.send(AppCommand::LeaveVoiceChannel {
            scope: VoiceScope::Guild(guild_id),
            self_mute: self.self_mute,
            self_deaf: self.self_deaf,
        });
        self.voice_channel = None;
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

        let (Some(handle), Selection::Guild(guild_id), Some((channel_id, _))) = (
            &self.handle,
            self.nav.selection,
            self.voice_channel.as_ref(),
        ) else {
            return;
        };

        handle.send(AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(guild_id),
            channel_id: *channel_id,
            self_mute: self.self_mute,
            self_deaf: self.self_deaf,
        });
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
            Some(view) => gpui::div().child(profile_view(view)),
            // The fetch is in flight. A skeleton with the id keeps the panel
            // from flashing empty.
            None => gpui::div().child(profile_view(&ProfileView {
                display_name: user_id.get().to_string(),
                handle: None,
                avatar: None,
                pronouns: None,
                bio: None,
                roles: Vec::new(),
                mutual_guilds: Vec::new(),
                loaded: false,
            })),
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
            .bg(rgb(DARK.surface_sunken))
            .border_l_1()
            .border_color(rgb(DARK.border));

        pane = pane.child(
            row()
                .w_full()
                .h(px(layout::HEADER))
                .px(px(space::MD))
                .border_b_1()
                .border_color(rgb(DARK.border))
                .text_size(px(text::SM))
                .text_color(rgb(DARK.text))
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
                    .bg(rgb(DARK.surface))
                    .border_1()
                    .border_color(rgb(DARK.accent))
                    .text_size(px(text::SM))
                    .child(if search.input.text().is_empty() {
                        gpui::div()
                            .text_color(rgb(DARK.text_subtle))
                            .child("Type and press Enter")
                    } else {
                        gpui::div()
                            .text_color(rgb(DARK.text))
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
                    .text_color(rgb(DARK.text_subtle))
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
                    .hover(|style| style.bg(rgb(DARK.surface_hover)))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.jump_to(channel_id, message_id);
                        cx.notify();
                    }))
                    .child(
                        gpui::div()
                            .text_size(px(text::XS))
                            .text_color(rgb(DARK.accent))
                            .child(result.author.clone()),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(text::SM))
                            .text_color(rgb(DARK.text_muted))
                            .child(preview),
                    ),
            );
        }

        pane.child(results)
    }

    /// Connection bar, shown only while connected to voice.
    fn voice_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((_, name)) = &self.voice_channel else {
            return gpui::div();
        };

        gpui::div().w_full().child(
            row()
                .w_full()
                .h(px(40.))
                .px(px(space::MD))
                .gap(px(space::SM))
                .bg(rgb(DARK.surface))
                .border_t_1()
                .border_color(rgb(DARK.border))
                .child(presence_dot(Presence::Online))
                .child(
                    column()
                        .flex_1()
                        .child(
                            gpui::div()
                                .text_size(px(text::SM))
                                .text_color(rgb(DARK.success))
                                .child("Voice connected"),
                        )
                        .child(
                            gpui::div()
                                .text_size(px(text::XS))
                                .text_color(rgb(DARK.text_subtle))
                                .child(name.clone()),
                        ),
                )
                .child(
                    self.voice_button("mute", self.self_mute)
                        .id("voice-mute")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.toggle_voice_flag(false);
                            cx.notify();
                        })),
                )
                .child(
                    self.voice_button("deafen", self.self_deaf)
                        .id("voice-deafen")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.toggle_voice_flag(true);
                            cx.notify();
                        })),
                )
                .child(
                    self.voice_button("leave", false)
                        .id("voice-leave")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.leave_voice();
                            cx.notify();
                        })),
                ),
        )
    }

    /// One control in the voice bar. Active toggles are filled, so state is
    /// readable at a glance rather than needing a colour comparison.
    fn voice_button(&self, label: &'static str, active: bool) -> gpui::Div {
        gpui::div()
            .px(px(space::SM))
            .py(px(space::XS))
            .rounded(px(layout::RADIUS))
            .text_size(px(text::XS))
            .bg(rgb(if active {
                DARK.danger
            } else {
                DARK.surface_hover
            }))
            .text_color(rgb(if active {
                DARK.on_accent
            } else {
                DARK.text_muted
            }))
            .child(label)
    }

    /// The member pane only applies to guild channels.
    fn shows_members(&self) -> bool {
        matches!(self.nav.selection, Selection::Guild(_)) && self.nav.channel.is_some()
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
            AppEvent::Ready { user, .. } => {
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
            .bg(rgb(DARK.bg))
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
                        d.border_2().border_color(rgb(DARK.accent)).rounded_full()
                    })
                    .when(guild.unread && !selected, |d| {
                        d.border_1()
                            .border_color(rgb(DARK.text_muted))
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

        let sidebar = panel_sunken(layout::SIDEBAR).child(
            row()
                .w_full()
                .h(px(layout::HEADER))
                .px(px(space::MD))
                .border_b_1()
                .border_color(rgb(DARK.border))
                .text_size(px(text::BASE))
                .text_color(rgb(DARK.text))
                .child(guild_name),
        );

        let mut list = column().w_full().pt(px(space::XS)).gap(px(1.));

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
                        .text_color(rgb(DARK.text_subtle))
                        .child(channel.kind.glyph()),
                )
                .child(
                    gpui::div()
                        .flex_1()
                        .when(channel.unread && !selected, |d| {
                            d.text_color(rgb(DARK.text))
                        })
                        .child(channel.name.clone()),
                );

            if channel.mentions > 0 {
                entry = entry.child(
                    gpui::div()
                        .px(px(6.))
                        .rounded_full()
                        .bg(rgb(DARK.danger))
                        .text_size(px(text::XS))
                        .text_color(rgb(DARK.on_accent))
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
                Some(channel_id) => entry
                    .id(("channel", index))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, _window, cx| {
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

        sidebar.child(list)
    }

    fn member_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut pane = column()
            .w(px(layout::MEMBERS))
            .h_full()
            .bg(rgb(DARK.surface_sunken))
            .border_l_1()
            .border_color(rgb(DARK.border))
            .pt(px(space::SM))
            .overflow_hidden();

        if self.model.members.is_empty() {
            return pane.child(
                gpui::div()
                    .px(px(space::MD))
                    .pt(px(space::MD))
                    .text_size(px(text::XS))
                    .text_color(rgb(DARK.text_subtle))
                    .child("No member data"),
            );
        }

        for (pane_index, member) in self.model.members.iter().enumerate() {
            if member.is_group {
                pane = pane.child(section_label(member.name.clone()));
                continue;
            }

            let mut entry = sidebar_row(false)
                .child(avatar_with_url(
                    layout::AVATAR_SM,
                    &member.name,
                    member.avatar.as_deref(),
                ))
                .child(presence_dot(member.presence))
                .child(
                    gpui::div()
                        .flex_1()
                        .text_color(rgb(member.color.unwrap_or(DARK.text_muted)))
                        .child(member.name.clone()),
                );

            if member.is_bot {
                entry = entry.child(
                    gpui::div()
                        .px(px(4.))
                        .rounded(px(3.))
                        .bg(rgb(DARK.accent))
                        .text_size(px(text::XS))
                        .text_color(rgb(DARK.on_accent))
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

    fn content(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let channel_name = self
            .model
            .channels
            .get(self.model.selected_channel)
            .map(|c| c.name.clone())
            .unwrap_or_default();

        column()
            .flex_1()
            .h_full()
            .bg(rgb(DARK.surface))
            .child(
                header()
                    .child(
                        gpui::div()
                            .text_color(rgb(DARK.text_subtle))
                            .child(ChannelKind::Text.glyph()),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(text::BASE))
                            .text_color(rgb(DARK.text))
                            .child(channel_name),
                    ),
            )
            .child(message_list(&self.messages, {
                // Click handlers run with only an `App`, so the workspace is
                // reached through its entity handle rather than captured.
                let entity = cx.entity();
                move |index, action, cx: &mut gpui::App| {
                    entity.update(cx, |workspace, cx| {
                        workspace.handle_message_action(index, action);
                        cx.notify();
                    });
                }
            }))
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
            .bg(rgb(DARK.surface_sunken))
            .border_t_1()
            .border_color(rgb(DARK.border))
            .text_size(px(text::XS))
            .text_color(rgb(DARK.text_subtle))
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

        column()
            .track_focus(&self.focus)
            .key_context("Workspace")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match &mut this.screen {
                    Screen::Login(login) => {
                        // ctrl-r toggles persistence; everything else edits the
                        // token buffer.
                        if event.keystroke.key == "r" && event.keystroke.modifiers.control {
                            login.remember = !login.remember;
                        } else if login.input.handle_key(event) {
                            this.submit_login(window, cx);
                        }
                    }
                    Screen::Ready => {
                        let key = event.keystroke.key.as_str();

                        if this.picker.is_some() {
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
                        } else if key == "o" && event.keystroke.modifiers.control {
                            this.attach_files(cx);
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
                            this.cancel_compose_context();
                        } else if this.composer.handle_key(event) {
                            this.send_message();
                        }
                    }
                }
                cx.notify();
            }))
            .size_full()
            .bg(rgb(DARK.bg))
            .text_size(px(text::BASE))
            .when(matches!(self.screen, Screen::Login(_)), |d| {
                let Screen::Login(login) = &self.screen else {
                    return d;
                };
                d.child(login_view(login))
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
                .child(self.voice_bar(cx))
                .child(self.status_bar())
            })
    }
}
