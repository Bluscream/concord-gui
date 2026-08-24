use super::super::*;

use concord::config::{self};
use concord::discord::{AppCommand, Id, ReplyReference, marker, next_message_nonce};
use gpui::{Context, WindowHandle};
use tokio::sync::mpsc;

use crate::model::message::{self};
use crate::model::projection::{self, Selection};
use crate::notify;
use crate::session::{SessionHandle, Update};

use crate::ui::composer::Composer;
use crate::ui::messages::MessageAction;

impl Workspace {
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

    pub fn clear_message_selection(&mut self) {
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

    pub fn pane_visible(&self, pane: Pane) -> bool {
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
    pub fn passes_filter(&self, name: &str) -> bool {
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
    pub fn toggle_category(&mut self, channel_id: Id<marker::ChannelMarker>) {
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

    pub fn category_collapsed(&self, channel_id: Id<marker::ChannelMarker>) -> bool {
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
    pub fn load_app_commands(&mut self) {
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
    pub fn send_message(&mut self) {
        let Some(channel_id) = self.nav.channel else {
            return;
        };
        if self.handle.is_none() {
            return;
        }

        // Checked here too, not only where the composer is drawn: the key
        // path reaches this directly, and a disabled-looking composer that
        // still sends on enter would be worse than no check at all.
        if let Some(reason) = self
            .last_state
            .as_ref()
            .and_then(|state| state.send_block_reason(channel_id))
        {
            self.model.status_line = format!("Cannot send here: {reason}");
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
                sticker_ids: std::mem::take(&mut self.pending_stickers),
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

        let was_empty = self.messages.is_empty();

        (self.messages, self.typing) = match self.nav.channel {
            Some(channel_id) => (
                message::project_messages(state, channel_id, state.current_user_id()),
                projection::typing_names(state, channel_id, guild_id),
            ),
            None => (Vec::new(), Vec::new()),
        };

        // A channel's first page arrives after it is opened, so asking to
        // scroll at open time scrolls an empty list and lands at the top of
        // the backlog once the messages appear. Done here instead, the moment
        // there is something to scroll.
        if was_empty && !self.messages.is_empty() {
            self.follow_bottom = true;
        }
        if self.follow_bottom && !self.messages.is_empty() {
            self.message_scroll.scroll_to_bottom();
        }

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
        self.resolve_missing_embeds();

        if self.options.display.show_images && !self.options.display.disable_image_preview {
            let attachments = self
                .messages
                .iter()
                .flat_map(|row| row.attachments.iter())
                .filter(|attachment| attachment.is_image && !attachment.url.is_empty())
                .map(|attachment| attachment.url.clone());

            // Embed pictures go through the same cache. Left out before, so
            // an embed that was mostly its image drew as an empty card.
            let embedded = self
                .messages
                .iter()
                .flat_map(|row| row.embeds.iter())
                .flat_map(|embed| {
                    embed
                        .image_url
                        .iter()
                        .chain(embed.thumbnail_url.iter())
                        .cloned()
                })
                .filter(|url| !url.is_empty());

            let urls: Vec<_> = attachments
                .chain(embedded)
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
                    // Camera capture is not wired to this path yet, so
                    // claiming one would show a feed that does not exist.
                    on_camera: false,
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
                                if workspace.options.notifications.notification_sounds {
                                    notify::play_sound(
                                        workspace.options.notifications.notification_sound.clone(),
                                    );
                                }
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
    /// Open a channel in the active tab, or in a new one.
    ///
    /// Replacing by default matches how a channel list is usually used - most
    /// clicks are navigation, not a request for another pane.
    pub fn open_channel_in_new_tab(&mut self, channel_id: Id<marker::ChannelMarker>) {
        self.stash_active_tab();
        let name = self.channel_name(channel_id);
        self.tabs.push(ChannelTab {
            channel_id,
            selection: self.nav.selection,
            name,
            draft: String::new(),
            scroll: gpui::point(gpui::px(0.), gpui::px(0.)),
        });
        self.active_tab = self.tabs.len() - 1;
        self.open_channel(channel_id);
    }

    /// Step to the next or previous tab, wrapping at each end.
    pub fn cycle_tab(&mut self, forward: bool) {
        let count = self.tabs.len();
        if count < 2 {
            return;
        }
        let next = if forward {
            (self.active_tab + 1) % count
        } else {
            (self.active_tab + count - 1) % count
        };
        self.activate_tab(next);
    }

    /// Switch to a tab, restoring its draft and scroll position.
    pub fn activate_tab(&mut self, index: usize) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let (channel_id, selection, draft, scroll) =
            (tab.channel_id, tab.selection, tab.draft.clone(), tab.scroll);

        self.stash_active_tab();
        self.active_tab = index;

        // The guild goes first, or the sidebar shows a different guild's
        // channels than the one the tab belongs to.
        if self.nav.selection != selection {
            self.nav.selection = selection;
        }
        self.open_channel(channel_id);
        self.composer.set_text(&draft);
        self.message_scroll.set_offset(scroll);
        self.persist_tabs();
    }

    /// Close a tab, moving to a neighbour if it was the active one.
    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);

        if self.tabs.is_empty() {
            self.active_tab = 0;
            self.persist_tabs();
            return;
        }

        // Prefer the tab to the left, which is where attention was before the
        // closed one existed.
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        if index <= self.active_tab && self.active_tab > 0 {
            self.active_tab -= 1;
        }
        let target = self.active_tab;
        self.activate_tab(target);
    }

    /// Rebuild the strip from the last session.
    ///
    /// Done on Ready rather than at construction: channel names come from the
    /// snapshot, and restoring earlier would give a strip of bare ids.
    pub fn restore_tabs(&mut self) {
        if !self.tabs.is_empty() || self.ui_state.open_tabs.is_empty() {
            return;
        }

        let saved: Vec<_> = self.ui_state.open_tabs.clone();
        let active = self.ui_state.active_tab;

        self.tabs = saved
            .into_iter()
            // A channel that has since been deleted, or that this account can
            // no longer see, would be a tab that opens nothing.
            .filter(|channel_id| {
                self.model
                    .channels
                    .iter()
                    .any(|channel| channel.id == Some(*channel_id))
            })
            .map(|channel_id| ChannelTab {
                channel_id,
                selection: self.nav.selection,
                name: self.channel_name(channel_id),
                draft: String::new(),
                scroll: gpui::point(gpui::px(0.), gpui::px(0.)),
            })
            .collect();

        self.active_tab = active.min(self.tabs.len().saturating_sub(1));
    }

    /// Save the active tab's draft and scroll before leaving it.
    pub fn stash_active_tab(&mut self) {
        let draft = self.composer.text().to_string();
        let scroll = self.message_scroll.offset();
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.draft = draft;
            tab.scroll = scroll;
        }
    }

    /// An avatar URL honouring the animation setting.
    pub fn still_avatar(&self, url: Option<&str>) -> Option<String> {
        let url = url?;
        Some(if self.options.display.animate_images {
            url.to_owned()
        } else {
            concord::discord::still_avatar_url(url)
        })
    }

    pub fn channel_name(&self, channel_id: Id<marker::ChannelMarker>) -> String {
        self.model
            .channels
            .iter()
            .find(|channel| channel.id == Some(channel_id))
            .map(|channel| channel.name.clone())
            .unwrap_or_else(|| channel_id.get().to_string())
    }

    /// Write the open set to the shared UI state, so it survives a restart
    /// and the TUI can adopt it later.
    pub fn persist_tabs(&mut self) {
        self.ui_state.open_tabs = self.tabs.iter().map(|tab| tab.channel_id).collect();
        self.ui_state.active_tab = self.active_tab;
        let _ = config::save_ui_state_options(&self.ui_state);
    }
}

impl Workspace {
    /// Ask the embed proxy about links Discord did not preview.
    ///
    /// Only links in messages that came back with no embed at all: Discord's
    /// own unfurl is authoritative, and asking about a link it already
    /// described would put a second opinion beside the real one with nothing
    /// to tell them apart.
    ///
    /// Each address is asked about once per session. A page that has no
    /// preview will not grow one while the client is open, and asking again
    /// on every reprojection would be a request per link per keystroke.
    fn resolve_missing_embeds(&mut self) {
        let proxy = self.options.embeds.proxy.trim().to_owned();
        if proxy.is_empty() {
            return;
        }
        let Some(channel_id) = self.nav.channel else {
            return;
        };

        let pending: Vec<_> = self
            .messages
            .iter()
            .filter(|row| row.embeds.is_empty())
            .flat_map(|row| row.links.iter().map(move |url| (row.id, url.clone())))
            .filter(|(_, url)| self.requested_embeds.insert(url.clone()))
            .collect();

        for (message_id, url) in pending {
            if let Some(handle) = &self.handle {
                handle.send(AppCommand::ResolveEmbed {
                    channel_id,
                    message_id,
                    url,
                    proxy: proxy.clone(),
                });
            }
        }
    }
}
