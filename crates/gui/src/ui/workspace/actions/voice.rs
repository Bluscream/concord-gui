use super::super::*;

use concord::discord::{AppCommand, FriendStatus, Id, ReactionEmoji, marker};
use concord::t;
use concord_ui::model::AttachmentViewerZoom;
use gpui::{Context, px, rgb};

use crate::model::projection::Selection;

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{column, section_label};
use crate::ui::composer::Composer;
use crate::ui::overlay;

impl Workspace {
    pub fn toggle_reaction(&mut self, message: usize, reaction: usize) {
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
    pub fn moderation_controls(
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
                "mod-roles",
                "Manage roles",
                reason(state.can_manage_roles(guild_id)),
                ModerationAction::ManageRoles,
            ),
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

    /// Friend controls for the profile on screen.
    ///
    /// Separate from moderation because friendship is not a guild thing: these
    /// appear wherever a profile does, including in a DM, which is where most
    /// of them are wanted. What is offered depends on how things already
    /// stand - "add friend" and "accept request" are the same call, and
    /// showing both would be a choice with no difference.
    pub fn friend_controls(
        &self,
        user_id: Id<marker::UserMarker>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        if Some(user_id) == self.current_user {
            return None;
        }
        let status = self.last_state.as_ref()?.friend_status(user_id);
        let label = self.friend_label(user_id);

        let mut entries: Vec<(&'static str, String, AppCommand)> = Vec::new();
        match status {
            FriendStatus::None => entries.push((
                "friend-add",
                t!("action-send-friend-request"),
                AppCommand::AddFriend {
                    user_id,
                    label: label.clone(),
                },
            )),
            FriendStatus::IncomingRequest => entries.push((
                "friend-accept",
                t!("action-accept-friend-request"),
                AppCommand::AddFriend {
                    user_id,
                    label: label.clone(),
                },
            )),
            FriendStatus::OutgoingRequest => entries.push((
                "friend-cancel",
                t!("action-cancel-friend-request"),
                AppCommand::RemoveRelationship {
                    user_id,
                    label: label.clone(),
                },
            )),
            FriendStatus::Friend => entries.push((
                "friend-remove",
                t!("action-remove-friend"),
                AppCommand::RemoveRelationship {
                    user_id,
                    label: label.clone(),
                },
            )),
            FriendStatus::Blocked => {}
        }

        entries.push(if status == FriendStatus::Blocked {
            (
                "friend-unblock",
                t!("action-unblock"),
                AppCommand::RemoveRelationship {
                    user_id,
                    label: label.clone(),
                },
            )
        } else {
            (
                "friend-block",
                t!("action-block"),
                AppCommand::BlockUser { user_id, label },
            )
        });

        let mut panel = column()
            .w_full()
            .p(px(space::MD))
            .gap(px(space::XS))
            .border_t_1()
            .border_color(rgb(active().border))
            .child(section_label(t!("label-friendship")));

        for (id, label, command) in entries {
            panel = panel.child(
                gpui::div()
                    .id(id)
                    .px(px(space::SM))
                    .py(px(space::XS))
                    .rounded(px(layout::RADIUS))
                    .text_size(px(scaled(text::SM)))
                    .text_color(rgb(active().text))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(active().surface_hover)))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.friend_action(command.clone());
                        cx.notify();
                    }))
                    .child(label),
            );
        }

        Some(panel)
    }

    /// The best name known for someone, for use in a status line.
    pub fn friend_label(&self, user_id: Id<marker::UserMarker>) -> String {
        self.last_state
            .as_ref()
            .and_then(|state| state.relationship_display_name(user_id))
            .map(str::to_owned)
            .unwrap_or_else(|| user_id.get().to_string())
    }

    /// The first tab the account may actually see.
    ///
    /// Opening on Invites when the account cannot read them would show an
    /// error where a usable tab was available.
    pub fn first_server_tab(&self) -> ServerTab {
        let Some(state) = self.last_state.as_ref() else {
            return ServerTab::Invites;
        };
        let Selection::Guild(guild_id) = self.nav.selection else {
            return ServerTab::Invites;
        };

        if state.can_manage_invites(guild_id) {
            ServerTab::Invites
        } else if state.can_manage_emoji(guild_id) {
            ServerTab::Emoji
        } else {
            ServerTab::AuditLog
        }
    }

    /// Open a message's images full size, starting at the one clicked.
    pub fn view_image(&mut self, row_index: usize, position: usize) {
        let Some(row) = self.messages.get(row_index) else {
            return;
        };

        // Only the images: paging through a message that mixes an image with
        // a zip file should not land on the zip.
        let urls: Vec<String> = row
            .attachments
            .iter()
            .filter(|attachment| attachment.is_image)
            .map(|attachment| attachment.url.clone())
            .collect();
        if urls.is_empty() {
            return;
        }

        // The clicked position counts every attachment, so it has to be
        // mapped onto the image-only list rather than used directly.
        let index = row
            .attachments
            .iter()
            .take(position + 1)
            .filter(|attachment| attachment.is_image)
            .count()
            .saturating_sub(1);

        self.viewing_image = Some(ImageViewerView {
            urls,
            index,
            zoom: AttachmentViewerZoom::default(),
        });
    }

    /// Step through the images in the open message, wrapping at each end.
    pub fn step_viewed_image(&mut self, forward: bool) {
        if let Some(view) = &mut self.viewing_image {
            view.step(forward);
        }
    }

    pub fn zoom_viewed_image(&mut self, in_: bool) {
        if let Some(view) = &mut self.viewing_image {
            view.zoom = if in_ {
                view.zoom.zoom_in()
            } else {
                view.zoom.zoom_out()
            };
        }
    }

    /// Create a text channel in the open guild.
    ///
    /// Text, because that is what "new channel" means nine times out of ten;
    /// the other kinds belong in a fuller editor rather than behind a guess.
    pub fn create_channel(&mut self, name: String) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };

        handle.send(AppCommand::CreateGuildChannel {
            guild_id,
            name,
            kind: concord::discord::NewChannelKind::Text,
            // At the top level. Putting it in whichever category is nearby
            // would be a guess about what was meant.
            parent_id: None,
        });
    }

    pub fn rename_channel(&mut self, channel_id: Id<marker::ChannelMarker>, name: String) {
        let Some(handle) = &self.handle else {
            return;
        };
        let label = self.channel_name(channel_id);

        handle.send(AppCommand::ModifyChannel {
            channel_id,
            edit: Box::new(concord::discord::ChannelEdit {
                name: Some(name),
                ..concord::discord::ChannelEdit::default()
            }),
            label,
        });
    }

    /// Delete the open channel, once confirmed.
    pub fn delete_channel_confirmed(&mut self, channel_id: Id<marker::ChannelMarker>) {
        let Some(handle) = &self.handle else {
            return;
        };
        let label = self.channel_name(channel_id);
        handle.send(AppCommand::DeleteChannel { channel_id, label });
    }

    /// Whether channels can be managed in the open guild.
    pub fn can_manage_channels(&self) -> bool {
        self.last_state.as_ref().is_some_and(|state| {
            matches!(self.nav.selection, Selection::Guild(guild_id)
                if state.can_manage_channels(guild_id))
        })
    }

    /// Open a context menu on something.
    pub fn open_context_menu(&mut self, subject: ContextSubject, at: gpui::Point<gpui::Pixels>) {
        self.context_menu = Some(ContextMenu { subject, at });
    }

    /// What a context menu offers for its subject.
    ///
    /// Built from the same state the panels use, so a menu never offers
    /// something a panel refuses or vice versa.
    pub fn context_items(&self, subject: ContextSubject) -> Vec<overlay::ContextItem> {
        let manage = self.can_manage_channels();
        match subject {
            ContextSubject::Message(index) => {
                let mine = self
                    .messages
                    .get(index)
                    .is_some_and(|row| Some(row.author_id) == self.current_user);
                vec![
                    overlay::ContextItem {
                        label: t!("action-reply"),
                        disabled_reason: None,
                        destructive: false,
                    },
                    overlay::ContextItem {
                        label: t!("action-copy-text"),
                        disabled_reason: None,
                        destructive: false,
                    },
                    overlay::ContextItem {
                        label: t!("action-edit"),
                        // Discord only lets you edit your own, so saying so
                        // beats a round trip that fails.
                        disabled_reason: (!mine).then(|| t!("status-not-your-message")),
                        destructive: false,
                    },
                    overlay::ContextItem {
                        label: t!("action-delete"),
                        disabled_reason: None,
                        destructive: true,
                    },
                ]
            }
            ContextSubject::Channel(channel_id) => vec![
                overlay::ContextItem {
                    label: t!("action-open-in-new-tab"),
                    disabled_reason: None,
                    destructive: false,
                },
                overlay::ContextItem {
                    label: t!("action-rename-channel"),
                    disabled_reason: (!manage).then(|| t!("status-no-permission")),
                    destructive: false,
                },
                overlay::ContextItem {
                    label: t!("action-delete-channel"),
                    disabled_reason: (!manage).then(|| t!("status-no-permission")),
                    destructive: true,
                },
                // Only on a stage, where they mean something: on an ordinary
                // voice channel there is no audience to ask to leave.
                overlay::ContextItem {
                    label: t!("action-ask-to-speak"),
                    disabled_reason: (!self.is_stage_channel(channel_id))
                        .then(|| t!("status-not-a-stage")),
                    destructive: false,
                },
                overlay::ContextItem {
                    label: t!("action-move-up"),
                    disabled_reason: (!manage).then(|| t!("status-no-permission")),
                    destructive: false,
                },
                overlay::ContextItem {
                    label: t!("action-move-down"),
                    disabled_reason: (!manage).then(|| t!("status-no-permission")),
                    destructive: false,
                },
                overlay::ContextItem {
                    label: t!("action-voice-status"),
                    disabled_reason: if !self.is_voice_channel(channel_id) {
                        Some(t!("status-not-a-voice-channel"))
                    } else {
                        (!manage).then(|| t!("status-no-permission"))
                    },
                    destructive: false,
                },
                overlay::ContextItem {
                    label: t!("action-stage-topic"),
                    disabled_reason: if !self.is_stage_channel(channel_id) {
                        Some(t!("status-not-a-stage"))
                    } else {
                        (!manage).then(|| t!("status-no-permission"))
                    },
                    destructive: false,
                },
            ],
            ContextSubject::Guild(guild_id) => vec![overlay::ContextItem {
                // Phrased by what activating it does. Unknown says so rather
                // than guessing: the list arrives with READY, and a guess
                // would assert a privacy setting nobody confirmed.
                label: match self.privacy_state().guild_direct_messages_allowed(guild_id) {
                    Some(true) => t!("action-guild-dms-block"),
                    Some(false) => t!("action-guild-dms-allow"),
                    None => t!("action-guild-dms-unknown"),
                },
                disabled_reason: None,
                destructive: false,
            }],
            ContextSubject::Member(_) => vec![
                overlay::ContextItem {
                    label: t!("action-view-profile"),
                    disabled_reason: None,
                    destructive: false,
                },
                overlay::ContextItem {
                    label: t!("action-block"),
                    disabled_reason: None,
                    destructive: true,
                },
                // Last, after the destructive one, because it only does
                // anything in a stage - Discord refuses it elsewhere - and a
                // row that usually fails should not sit above one that works.
                overlay::ContextItem {
                    label: t!("action-invite-to-speak"),
                    disabled_reason: None,
                    destructive: false,
                },
                overlay::ContextItem {
                    label: t!("action-hide-video"),
                    disabled_reason: None,
                    destructive: false,
                },
            ],
        }
    }

    /// Carry out the picked entry.
    pub fn pick_context_item(&mut self, index: usize) {
        let Some(menu) = self.context_menu.take() else {
            return;
        };

        match (menu.subject, index) {
            (ContextSubject::Message(row), 0) => {
                if let Some(message) = self.messages.get(row) {
                    let (id, author) = (message.id, message.author.clone());
                    self.start_reply(id, author);
                }
            }
            (ContextSubject::Message(row), 1) => {
                self.pending_copy = self
                    .messages
                    .get(row)
                    .map(|message| message.content.clone());
            }
            (ContextSubject::Message(row), 2) => {
                if let Some(message) = self.messages.get(row) {
                    self.start_edit(message.id);
                }
            }
            (ContextSubject::Message(row), 3) => {
                self.confirming = Some(Confirm {
                    message: row,
                    action: ConfirmAction::Delete,
                });
            }
            (ContextSubject::Channel(channel_id), 0) => self.open_channel_in_new_tab(channel_id),
            (ContextSubject::Channel(channel_id), 1) => {
                let mut text = Composer::default();
                text.set_text(&self.channel_name(channel_id));
                self.prompt = Some((Prompt::ChannelName(channel_id), text));
            }
            (ContextSubject::Channel(channel_id), 2) => {
                self.deleting_channel = Some(channel_id);
            }
            (ContextSubject::Channel(channel_id), 3) => self.request_to_speak(channel_id),
            (ContextSubject::Channel(channel_id), 4) => self.move_channel(channel_id, true),
            (ContextSubject::Channel(channel_id), 5) => self.move_channel(channel_id, false),
            (ContextSubject::Channel(channel_id), 6) => {
                self.prompt = Some((Prompt::VoiceStatus(channel_id), Composer::default()));
            }
            (ContextSubject::Channel(channel_id), 7) => self.open_stage_topic(channel_id),
            (ContextSubject::Guild(guild_id), 0) => {
                self.toggle_guild_direct_messages(guild_id);
            }
            (ContextSubject::Member(user_id), 0) => self.open_profile(user_id),
            // Only does anything in a stage; elsewhere Discord refuses it,
            // which is why the row is offered from the member menu rather than
            // hidden behind a mode.
            (ContextSubject::Member(user_id), 2) => self.invite_to_speak(user_id),
            (ContextSubject::Member(user_id), 3) => self.toggle_video_hidden(user_id),
            (ContextSubject::Member(user_id), 1) => {
                let label = self.friend_label(user_id);
                self.friend_action(AppCommand::BlockUser { user_id, label });
            }
            _ => {}
        }
    }

    /// The signed-in username, for seeding the form and labelling the
    /// enrolment URI.
    pub fn current_user_name(&self) -> Option<&str> {
        self.last_state
            .as_ref()
            .and_then(|state| state.current_user())
    }

    pub fn open_account(&mut self) {
        self.account = Some(AccountView {
            // Seeded with the current username, so an untouched field compares
            // equal and is left out of the edit.
            form: concord::discord::AccountForm::new(
                self.current_user_name().unwrap_or_default(),
                "",
            ),
            focused: 0,
            totp_secret: None,
            totp_code: String::new(),
            backup_codes: Vec::new(),
        });
    }

    pub fn focus_account_field(&mut self, index: usize) {
        if let Some(view) = &mut self.account
            && index < concord::discord::AccountField::ALL.len()
        {
            view.focused = index;
        }
    }

    /// Take one keystroke into the focused field.
    pub fn type_account_key(&mut self, key: &str) {
        let Some(view) = &mut self.account else {
            return;
        };
        let Some(field) = concord::discord::AccountField::at(view.focused) else {
            return;
        };
        match key {
            "backspace" => view.form.pop(field),
            other => {
                // Only real characters. A bare modifier or an arrow key
                // arrives as a name like "shift", and appending it would put
                // "shift" into the field.
                let mut characters = other.chars();
                if let (Some(character), None) = (characters.next(), characters.next()) {
                    view.form.push(field, character);
                }
            }
        }
    }

    /// Start or cancel two-factor enrolment.
    pub fn toggle_totp_enrolment(&mut self) {
        let Some(view) = &mut self.account else {
            return;
        };
        if view.totp_secret.is_some() {
            view.totp_secret = None;
            view.totp_code.clear();
        } else {
            view.totp_secret = Some(concord::discord::TotpSecret::generate());
        }
    }

    /// Type into the two-factor code field.
    ///
    /// Separate from the form fields: the code is not part of the credential
    /// change and is submitted by its own button.
    pub fn type_totp_code(&mut self, key: &str) {
        let Some(view) = &mut self.account else {
            return;
        };
        match key {
            "backspace" => {
                view.totp_code.pop();
            }
            other => {
                let mut characters = other.chars();
                if let (Some(character), None) = (characters.next(), characters.next()) {
                    view.totp_code.push(character);
                }
            }
        }
    }

    /// Finish enrolment with the code from the authenticator app.
    pub fn submit_totp_enrolment(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.account else {
            return;
        };
        let Some(secret) = view.totp_secret.clone() else {
            return;
        };
        if view.totp_code.is_empty() {
            return;
        }
        // Discord needs the account password here too, and it is the one the
        // form already asks for rather than a second prompt.
        let password = view
            .form
            .value(concord::discord::AccountField::CurrentPassword);
        if password.is_empty() {
            return;
        }
        let password = concord::discord::Secret::new(password);
        let code = std::mem::take(&mut view.totp_code);
        view.totp_secret = None;
        handle.send(AppCommand::EnableTotp {
            secret: secret.as_str().to_owned(),
            code,
            password,
        });
    }
}
