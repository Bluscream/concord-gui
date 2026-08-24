use super::super::*;
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use concord::config::{self, AppOptions, CredentialStoreMode, UiStateOptions};
use concord::discord::{
    AccountField, AccountForm, ActivityInfo, ActivityKind, AppCommand, AppEvent,
    ApplicationCommandAutocompleteInvocation, ApplicationCommandInfo, ApplicationCommandInvocation,
    AttachmentDownloadId, AuditLogAction, AuditLogEntryInfo, BuiltinSlashCommandParse,
    BuiltinSlashCommandSubmit, DownloadAttachmentSource, ForumPostArchiveState, ForumPostCreate,
    FriendStatus, GlobalUserProfileUpdate, GuildEmojiInfo, GuildInviteInfo, GuildUserProfileUpdate,
    Id, MAX_MESSAGE_STICKERS, MAX_UPLOAD_ATTACHMENT_COUNT, MediaPlaybackSource, MediaPlaybackTarget,
    MessageAttachmentUpload, MessageHistoryAfterMode, MessageSearchQuery, MuteDuration,
    NewChannelKind, OnboardingRow, PresenceStatus, PrivacySetting, PrivacyState,
    ProfileAvatarUpload, ReactionEmoji, ReplyReference, Secret, SoundboardSound,
    StreamCaptureTargetsRequestId, UserProfileUpdate, VoiceConnectionStatus,
    VoiceParticipantPlaybackSettings, VoiceParticipantVolumePercent, VoiceScope, VoiceVolumePercent,
    application_command_content_is_complete, invite_code_from, marker, next_message_nonce,
    parse_builtin_slash_command,
};
use concord::t;
use concord::token_store;
use concord_ui::model::AttachmentViewerZoom;
use gpui::{Context, FocusHandle, PathPromptOptions, Window, WindowHandle, px, rgb};
use tokio::sync::mpsc;

use crate::model::message::{self, MessageRow};
use crate::model::projection::{self, Navigation, Selection};
use crate::notify;
use crate::session::{SessionHandle, Update};

use crate::keymap::Keymap;
use crate::theme::{self, Presence, active, layout, scaled, space, text};
use crate::ui::chrome::{column, header, icon_button, presence_dot, row, section_label};
use crate::ui::composer::{Composer, composer_view};
use crate::ui::emoji::{self, EmojiPicker};
use crate::ui::forum::{self, ForumPost, ForumView};
use crate::ui::login::{LoginEvent, LoginHandle, LoginScreen, login_view};
use crate::ui::messages::{MessageAction, RenderOptions, message_list};
use crate::ui::overlay;
use crate::ui::profile::ProfileView;
use crate::ui::settings::{OnChange, SettingsWindow};
use crate::ui::slash::{SlashPicker, slash_view};
use crate::ui::stream::StreamPicker;
use crate::ui::switcher::Switcher;
use super::*;
use crate::ui::workspace::{RiskAction, SwitcherPurpose, MfaMethod, PasswordAuthEvent, QrEvent};


impl Workspace {

    pub fn adjust_zoom(&mut self, delta: f32) {
        let next = (crate::theme::zoom() + delta).clamp(0.75, 2.0);
        crate::theme::set_zoom(next);
        self.model.status_line = format!("Interface scale {:.0}%", next * 100.0);
    }

    /// Toggle whether messages are sent as text-to-speech.
    pub fn toggle_tts(&mut self) {
        self.send_as_tts = !self.send_as_tts;
        self.model.status_line = if self.send_as_tts {
            "Next messages send as /tts".to_string()
        } else {
            "Sending normally".to_string()
        };
    }

    /// Set how loudly one participant is played, mute them, or stop showing
    /// their camera and screen.
    ///
    /// Local only, all three: this changes what happens here, and neither
    /// Discord nor the person is told. Hiding someone is a decision about your
    /// own screen; telling them would make it a social act instead.
    pub fn set_participant_playback(
        &mut self,
        user_id: Id<marker::UserMarker>,
        volume: u16,
        muted: bool,
        video_hidden: bool,
    ) {
        let Some(handle) = &self.handle else {
            return;
        };
        handle.send(AppCommand::UpdateVoiceParticipantPlayback {
            user_id,
            settings: VoiceParticipantPlaybackSettings {
                volume: VoiceParticipantVolumePercent::new(volume),
                muted,
                video_hidden,
            },
        });
    }

    /// Step the output volume, persisting it for the next connection.
    ///
    /// The core takes output volume when joining rather than as a standalone
    /// command, so a change applies to the next join; saying so is better than
    /// appearing to do nothing now.
    pub fn adjust_output_volume(&mut self, delta: i16) {
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
    pub fn open_message_thread(&mut self, index: usize) {
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
    pub fn request_thread_preview(
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
    pub fn submit_prompt(&mut self) {
        let Some((prompt, text)) = self.prompt.take() else {
            return;
        };
        let text = text.text().trim().to_string();
        if text.is_empty() {
            return;
        }

        match prompt {
            Prompt::ThreadName => self.rename_thread(text),
            Prompt::EmojiName(index) => self.rename_emoji(index, text),
            Prompt::SoundName(index) => self.rename_sound(index, text),
            Prompt::NewRole => self.create_role(text),
            Prompt::NewTemplate => self.create_template(text),
            Prompt::WelcomeDescription => self.set_welcome_description(text),
            Prompt::WidgetChannel => self.set_widget_channel(&text),
            Prompt::NewEvent => self.create_event(&text),
            Prompt::StageTopic(channel_id) => self.submit_stage_topic(channel_id, &text),
            Prompt::EditEvent(event_id) => self.submit_event_edit(event_id, &text),
            Prompt::BulkBan => self.submit_bulk_ban(&text),
            Prompt::VoiceStatus(channel_id) => self.set_voice_status(channel_id, &text),
            Prompt::NewSticker => self.create_sticker(&text),
            Prompt::StickerRename(index) => self.rename_sticker(index, text),
            Prompt::DiscoveryKeywords => self.set_discovery_text(Some(text), None),
            Prompt::DiscoveryAbout => self.set_discovery_text(None, Some(text)),
            Prompt::GuildName => self.rename_guild(text),
            Prompt::GuildIcon => self.set_guild_icon(text),
            Prompt::ChannelTopic(channel_id) => self.set_channel_topic(channel_id, text),
            Prompt::ChannelSlowmode(channel_id) => self.set_channel_slowmode(channel_id, text),
            Prompt::EmojiImage => self.create_emoji(text),
            Prompt::NewChannel => self.create_channel(text),
            Prompt::ChannelName(channel_id) => self.rename_channel(channel_id, text),
            Prompt::InviteCode => {
                // A link resolves; anything else is treated as a search of
                // Discord's public list. People have one or the other, and
                // asking which before they type is a question with no good
                // answer.
                if concord::discord::invite_code_from(&text).is_some() {
                    self.resolve_invite(&text);
                } else if let Some(handle) = &self.handle {
                    self.discovering = true;
                    handle.send(AppCommand::LoadDiscoverableGuilds { query: text });
                }
            }
            Prompt::ForumPostTitle => {
                // The body is the composer's content, so a post is written the
                // same way a message is and the title is the only extra step.
                let body = self.composer.take();
                self.create_forum_post(text, body);
            }
        }
    }

    /// Apply the typed custom status.
    pub fn submit_custom_status(&mut self) {
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
    pub fn submit_folder_rename(&mut self) {
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

    /// Stickers the open guild offers, as (name, image URL).
    ///
    /// Only the guild's own: sending another guild's sticker needs Nitro, so
    /// listing them would offer things that fail to send.
    pub fn guild_stickers(&self) -> Vec<(String, Option<String>)> {
        let (Some(state), Selection::Guild(guild_id)) = (&self.last_state, self.nav.selection)
        else {
            return Vec::new();
        };

        state
            .stickers_for_guild(guild_id)
            .iter()
            .map(|sticker| (sticker.name.clone(), sticker.image_url()))
            .collect()
    }

    /// Open the sticker picker, fetching previews for what it will show.
    pub fn open_sticker_picker(&mut self) {
        self.sticker_picker = true;

        // Requested here rather than on render: the picker needs the images,
        // and the ordinary preview sweep only covers what is in the log.
        let urls: Vec<_> = self
            .guild_stickers()
            .into_iter()
            .filter_map(|(_, url)| url)
            .filter(|url| self.requested_previews.insert(url.clone()))
            .collect();

        for url in urls {
            if let Some(handle) = &self.handle {
                handle.send(AppCommand::LoadAttachmentPreview { url });
            }
        }
    }

    /// Stage a sticker for the next send.
    pub fn stage_sticker(&mut self, index: usize) {
        let (Some(state), Selection::Guild(guild_id)) = (&self.last_state, self.nav.selection)
        else {
            return;
        };
        let Some(sticker) = state.stickers_for_guild(guild_id).get(index) else {
            return;
        };
        let sticker_id = sticker.id;

        // Discord accepts at most three, and refuses the whole message if more
        // are sent, so the cap is enforced here rather than by the server.
        if self.pending_stickers.len() >= MAX_MESSAGE_STICKERS {
            self.model.status_line = format!("At most {MAX_MESSAGE_STICKERS} stickers per message");
            return;
        }
        if !self.pending_stickers.contains(&sticker_id) {
            self.pending_stickers.push(sticker_id);
        }
        self.sticker_picker = false;
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

    /// Ask before an action Discord's anti-spam checks watch.
    ///
    /// Returns whether the caller may proceed now. A warning that has been
    /// silenced proceeds immediately - the user has already decided.
    pub fn confirm_risk(&mut self, action: RiskAction) -> bool {
        if action.suppressed(&self.options) {
            return true;
        }
        self.risk = Some((action, false));
        false
    }

    /// Change a guild nickname, once the risk has been accepted.
    pub fn set_nickname_confirmed(&mut self, guild_id: Id<marker::GuildMarker>, nickname: String) {
        let (Some(handle), Some(user_id)) = (&self.handle, self.current_user) else {
            return;
        };

        handle.send(AppCommand::UpdateUserProfile {
            update: Box::new(UserProfileUpdate {
                user_id,
                guild_id: Some(guild_id),
                global: GlobalUserProfileUpdate::default(),
                guild: Some(GuildUserProfileUpdate {
                    guild_id,
                    nickname: Some(nickname),
                    // /nick changes only the nickname; the rest of the guild
                    // identity is left as it is.
                    pronouns: None,
                    bio: None,
                    avatar: None,
                }),
            }),
        });
    }

    /// Carry out whatever the open warning was about.
    pub fn accept_risk(&mut self) {
        let Some((action, dont_ask)) = self.risk.take() else {
            return;
        };
        if dont_ask {
            action.suppress(&mut self.options);
            if let Err(error) = config::save_options(&self.options) {
                // Saying so matters: silently failing means the warning
                // returns next time and looks like the box did nothing.
                self.settings_note = Some(format!("Could not save preference: {error}"));
            }
        }

        match action {
            RiskAction::JoinGuild => self.accept_invite_confirmed(),
            RiskAction::LeaveGuild => self.leave_guild_confirmed(),
            RiskAction::ProfileEdit(guild_id, nickname) => {
                self.set_nickname_confirmed(guild_id, nickname)
            }
            RiskAction::FriendAction(command)
            | RiskAction::PruneMembers(command)
            | RiskAction::BulkBan(command) => {
                if let Some(handle) = &self.handle {
                    handle.send(*command);
                }
            }
        }
    }

    /// Join the guild the previewed invite points at.
    pub fn accept_invite(&mut self) {
        // Joining is the action most likely to get a third-party client
        // flagged, so it is warned about before it happens.
        if !self.confirm_risk(RiskAction::JoinGuild) {
            return;
        }
        self.accept_invite_confirmed();
    }

    pub fn accept_invite_confirmed(&mut self) {
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
    pub fn request_command_autocomplete(&mut self) {
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
    /// Dismiss the topmost modal, reporting whether there was one.
    ///
    /// The answer matters because escape has more to do than this: with
    /// nothing modal open it belongs to the reply being composed, or to the
    /// channel being marked read. Returning `false` is what lets the key
    /// carry on to those.
    pub fn close_popup(&mut self) -> bool {
        if self.risk.take().is_some()
            || self.bans.take().is_some()
            || self.editing_roles.take().is_some()
            || self.invite.take().is_some()
            || self.confirming.take().is_some()
            || self.prompt.take().is_some()
            || self.editing_status.take().is_some()
            || self.editing_activity.take().is_some()
            || self.viewing_image.take().is_some()
            || self.deleting_channel.take().is_some()
            || self.context_menu.take().is_some()
            || self.permission_grid.take().is_some()
            || self.renaming_folder.take().is_some()
            || self.picker.take().is_some()
            || self.stream_picker.take().is_some()
            || self.audio_devices.take().is_some()
            || self.reaction_users.take().is_some()
            || self.switcher.take().is_some()
            || self.inbox.take().is_some()
            || self.pane_filter.take().is_some()
        {
            return true;
        }

        // Nothing modal is open, so the selection is what escape clears -
        // and only if there is one, otherwise the key is still unspent.
        let had_selection = self.selected_message.is_some();
        self.clear_message_selection();
        had_selection
    }
}
