use super::super::*;

use concord::discord::{
    AppCommand, Id, MAX_UPLOAD_ATTACHMENT_COUNT, MessageAttachmentUpload, MessageHistoryAfterMode,
    ProfileAvatarUpload, marker,
};
use gpui::{Context, PathPromptOptions};

use crate::model::projection::{self, Selection};

use crate::ui::emoji::{self, EmojiPicker};
use crate::ui::messages::MessageAction;

impl Workspace {
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
    pub fn handle_message_action(&mut self, index: usize, action: MessageAction) {
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
            MessageAction::ViewImage(position) => self.view_image(index, position),
            MessageAction::ContextMenu(at) => {
                self.open_context_menu(ContextSubject::Message(index), at);
            }
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
    pub fn attach_files(&mut self, cx: &mut Context<Self>) {
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
    pub fn change_avatar(&mut self, cx: &mut Context<Self>) {
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
    pub fn stage_attachments(&mut self, paths: Vec<std::path::PathBuf>) {
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

    pub fn remove_attachment(&mut self, index: usize) {
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

        let initial_view = self
            .last_state
            .as_ref()
            .and_then(|state| projection::project_profile(state, user_id, guild_id));

        self.profile = Some((user_id, initial_view));

        if let Some(handle) = &self.handle {
            handle.send(AppCommand::LoadUserProfile { user_id, guild_id });
        }
    }

    /// Open the emoji picker for a message.
    pub fn open_emoji_picker(&mut self, message_id: Id<marker::MessageMarker>) {
        self.picker = Some(EmojiPicker {
            target: message_id,
            cursor: 0,
        });
    }

    /// Send the picked reaction and close the picker.
    pub fn pick_emoji(&mut self, glyph: &str) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        self.react(picker.target, glyph);
    }

    /// Move the picker cursor, wrapping at both ends.
    pub fn move_picker(&mut self, delta: isize) {
        let total = emoji::flat().len() as isize;
        if let Some(picker) = &mut self.picker {
            let next = (picker.cursor as isize + delta).rem_euclid(total);
            picker.cursor = next as usize;
        }
    }
}
