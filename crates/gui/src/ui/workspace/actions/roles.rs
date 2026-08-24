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

    pub fn save_permissions(&mut self) {
        let Some(view) = self.permission_grid.take() else {
            return;
        };
        // Nothing changed, so nothing is sent: it would spend a request and
        // write an audit entry saying so.
        if !view.is_dirty() {
            return;
        }
        let Some(handle) = &self.handle else {
            return;
        };

        match view.scope {
            PermissionScope::Role { guild_id, role_id } => handle.send(AppCommand::ModifyRole {
                guild_id,
                role_id,
                edit: Box::new(concord::discord::RoleEdit {
                    permissions: Some(view.allow),
                    ..concord::discord::RoleEdit::default()
                }),
                label: view.name,
            }),
            PermissionScope::ChannelOverwrite { channel_id, target } => {
                // An overwrite that neither allows nor denies anything is not
                // an overwrite; Discord keeps the row, so it is removed.
                if view.allow == 0 && view.deny == 0 {
                    handle.send(AppCommand::DeleteChannelOverwrite {
                        channel_id,
                        target,
                        label: view.name,
                    });
                } else {
                    handle.send(AppCommand::SetChannelOverwrite {
                        channel_id,
                        target,
                        allow: view.allow,
                        deny: view.deny,
                        label: view.name,
                    });
                }
            }
        }
    }

    /// The open channel's topic, for seeding the field.
    pub fn channel_topic(&self, channel_id: Id<marker::ChannelMarker>) -> String {
        self.last_state
            .as_ref()
            .and_then(|state| state.channel(channel_id))
            .and_then(|channel| channel.topic.clone())
            .unwrap_or_default()
    }

    /// Create a role, with no permissions.
    ///
    /// Like Discord's own "new role": granting anything at creation would be a
    /// guess, and the grid is one click away.
    pub fn create_role(&mut self, name: String) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };
        handle.send(AppCommand::CreateRole { guild_id, name });
    }

    /// Whether this channel takes a voice status, which only a voice one does.
    pub fn is_voice_channel(&self, channel_id: Id<marker::ChannelMarker>) -> bool {
        self.model
            .channels
            .iter()
            .any(|channel| channel.id == Some(channel_id) && channel.kind.joins_voice())
    }

    /// Whether this channel is a stage, which is what makes the stage rows
    /// mean anything.
    pub fn is_stage_channel(&self, channel_id: Id<marker::ChannelMarker>) -> bool {
        self.model
            .channels
            .iter()
            .any(|channel| channel.id == Some(channel_id) && channel.kind == ChannelKind::Stage)
    }

    /// Open the stage panel for a channel, and ask what is running there.
    pub fn open_stage_topic(&mut self, channel_id: Id<marker::ChannelMarker>) {
        let Some(handle) = &self.handle else {
            return;
        };
        self.stage_running = None;
        handle.send(AppCommand::LoadStageInstance { channel_id });
        self.prompt = Some((Prompt::StageTopic(channel_id), Composer::default()));
    }

    /// Start, change or end a stage, whichever fits.
    ///
    /// The rule is in the core, so both clients decide it alike: Discord's
    /// start endpoint fails on a running stage and its patch fails on one that
    /// is not, and the two are indistinguishable from the form alone.
    pub fn submit_stage_topic(&mut self, channel_id: Id<marker::ChannelMarker>, topic: &str) {
        let Some(handle) = &self.handle else {
            return;
        };
        let running = self.stage_running.is_some();
        let command = match concord::discord::stage_action_for(topic, running) {
            concord::discord::StageAction::End => AppCommand::EndStageInstance {
                channel_id,
                label: self.channel_name(channel_id),
            },
            concord::discord::StageAction::ChangeTopic => AppCommand::ModifyStageTopic {
                channel_id,
                topic: topic.trim().to_owned(),
            },
            concord::discord::StageAction::Start => AppCommand::StartStageInstance {
                channel_id,
                topic: topic.trim().to_owned(),
            },
        };
        handle.send(command);
    }

    /// Raise your hand in a stage.
    pub fn request_to_speak(&mut self, channel_id: Id<marker::ChannelMarker>) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };
        handle.send(AppCommand::RequestToSpeak {
            guild_id,
            channel_id,
            requesting: true,
        });
    }

    /// Invite someone in the audience to speak.
    pub fn invite_to_speak(&mut self, user_id: Id<marker::UserMarker>) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };
        // The stage you are in, since inviting someone up only makes sense
        // there - and Discord addresses it by channel.
        let Some((channel_id, _)) = self.voice_channel else {
            return;
        };
        handle.send(AppCommand::SetStageSpeaker {
            guild_id,
            channel_id,
            user_id,
            speaking: true,
            label: self.friend_label(user_id),
        });
    }

    /// Ban several people at once, behind the risk prompt.
    ///
    /// A typed list rather than a member picker: this is what raid cleanup
    /// looks like in practice, and the ids are pasted from somewhere else.
    pub fn submit_bulk_ban(&mut self, text: &str) {
        let Selection::Guild(guild_id) = self.nav.selection else {
            return;
        };
        let user_ids = concord::discord::parse_user_id_list(text);
        // Nothing readable is a typo rather than a ban of nobody, and warning
        // about it would teach the wrong lesson about the warning.
        if user_ids.is_empty() {
            self.model.status_line = t!("status-no-user-ids");
            return;
        }
        let command = AppCommand::BulkBanMembers {
            guild_id,
            user_ids,
            // No message deletion by default: a separate decision from who to
            // ban, and the destructive default would be the wrong one.
            delete_message_seconds: 0,
        };
        if !self.confirm_risk(RiskAction::BulkBan(Box::new(command.clone()))) {
            return;
        }
        if let Some(handle) = &self.handle {
            handle.send(command);
        }
    }

    /// Start editing an event, seeded with it as one line.
    ///
    /// Seeded so a change is a correction rather than a retype, and so what is
    /// shown is exactly what will be sent back.
    pub fn start_event_edit(&mut self, index: usize) {
        let Some(view) = &self.server_management else {
            return;
        };
        let Some(event) = view.events.get(index) else {
            return;
        };
        let (id, line) = (event.id, event.to_line());
        let mut text = Composer::default();
        text.set_text(&line);
        self.prompt = Some((Prompt::EditEvent(id), text));
    }

    pub fn submit_event_edit(&mut self, event_id: u64, text: &str) {
        let (Some(handle), Some(view)) = (&self.handle, &self.server_management) else {
            return;
        };
        let Some(event) = concord::discord::parse_new_event(text) else {
            return;
        };
        if let Some(problem) = event.problem() {
            self.model.status_line = problem.message();
            return;
        }
        handle.send(AppCommand::ModifyScheduledEvent {
            guild_id: view.guild_id,
            event_id,
            event: Box::new(event),
        });
    }

    /// Create a scheduled event from one typed line.
    ///
    /// The same parser both clients use, so the accepted format cannot drift
    /// between them.
    pub fn create_event(&mut self, text: &str) {
        let (Some(handle), Some(view)) = (&self.handle, &self.server_management) else {
            return;
        };
        let Some(event) = concord::discord::parse_new_event(text) else {
            return;
        };
        if let Some(problem) = event.problem() {
            // Said here rather than left to Discord, whose message does not
            // name which of five fields is the problem.
            self.model.status_line = problem.message();
            return;
        }
        handle.send(AppCommand::CreateScheduledEvent {
            guild_id: view.guild_id,
            event: Box::new(event),
        });
    }

    pub fn set_welcome_description(&mut self, text: String) {
        let (Some(handle), Some(view)) = (&self.handle, &self.server_management) else {
            return;
        };
        handle.send(AppCommand::ModifyWelcomeScreen {
            guild_id: view.guild_id,
            // Empty clears it, which is a real thing to want and is distinct
            // from leaving the description alone.
            edit: concord::discord::WelcomeScreenEdit {
                description: Some((!text.is_empty()).then_some(text)),
                ..Default::default()
            },
        });
    }

    /// Point the widget's invite at a channel, by name.
    ///
    /// Names are not unique in Discord, so an ambiguous one is refused rather
    /// than resolved to whichever came first - picking one would aim the
    /// invite at a channel nobody chose.
    pub fn set_widget_channel(&mut self, name: &str) {
        let (Some(handle), Some(view)) = (&self.handle, &self.server_management) else {
            return;
        };
        let guild_id = view.guild_id;
        let mut widget = view.widget.clone().unwrap_or_default();
        if name.trim().is_empty() {
            widget.channel_id = None;
        } else {
            let Some(channel_id) = self.channel_id_by_name(guild_id, name) else {
                self.model.status_line = t!("status-no-such-channel");
                return;
            };
            widget.channel_id = Some(channel_id);
        }
        handle.send(AppCommand::ModifyGuildWidget { guild_id, widget });
    }

    /// The channel with this name, if there is exactly one.
    pub fn channel_id_by_name(
        &self,
        guild_id: Id<marker::GuildMarker>,
        name: &str,
    ) -> Option<Id<marker::ChannelMarker>> {
        let wanted = name.trim().trim_start_matches('#');
        let state = self.last_state.as_ref()?;
        let mut matches = state
            .channels_for_guild(Some(guild_id))
            .into_iter()
            .filter(|channel| channel.name == wanted);
        let first = matches.next()?;
        matches.next().is_none().then_some(first.id)
    }

    /// Take a template of the server as it stands.
    pub fn create_template(&mut self, name: String) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };
        if name.trim().is_empty() {
            // Discord rejects it, and the round trip would read as a failure
            // rather than as an empty name.
            return;
        }
        handle.send(AppCommand::CreateGuildTemplate { guild_id, name });
    }

    pub fn rename_guild(&mut self, name: String) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };
        if !concord::discord::is_valid_guild_name(&name) {
            self.model.status_line = t!("status-name-too-short");
            return;
        }
        handle.send(AppCommand::ModifyGuild {
            guild_id,
            edit: Box::new(concord::discord::GuildEdit {
                name: Some(name.clone()),
                ..concord::discord::GuildEdit::default()
            }),
            label: name,
        });
    }

    pub fn set_guild_icon(&mut self, path: String) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };
        handle.send(AppCommand::SetGuildIcon {
            guild_id,
            image: Box::new(concord::discord::ProfileAvatarUpload::from_path(
                path.into(),
            )),
            label: self
                .model
                .guilds
                .iter()
                .find(|guild| guild.id == Some(guild_id))
                .map(|guild| guild.name.clone())
                .unwrap_or_default(),
        });
    }

    pub fn set_channel_topic(&mut self, channel_id: Id<marker::ChannelMarker>, topic: String) {
        let Some(handle) = &self.handle else {
            return;
        };
        handle.send(AppCommand::ModifyChannel {
            channel_id,
            edit: Box::new(concord::discord::ChannelEdit {
                // An emptied field clears the topic rather than leaving it,
                // which is what the placeholder promises.
                topic: Some((!topic.is_empty()).then_some(topic)),
                ..concord::discord::ChannelEdit::default()
            }),
            label: self.channel_name(channel_id),
        });
    }

    pub fn set_channel_slowmode(&mut self, channel_id: Id<marker::ChannelMarker>, seconds: String) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Ok(seconds) = seconds.trim().parse::<u32>() else {
            self.model.status_line = t!("status-not-a-number");
            return;
        };
        handle.send(AppCommand::ModifyChannel {
            channel_id,
            edit: Box::new(concord::discord::ChannelEdit {
                slowmode_seconds: Some(seconds),
                ..concord::discord::ChannelEdit::default()
            }),
            label: self.channel_name(channel_id),
        });
    }

    /// Start renaming one of the guild's sounds.
    pub fn start_sound_rename(&mut self, index: usize) {
        let Some(view) = &self.server_management else {
            return;
        };
        let Some(sound) = view.sounds.get(index) else {
            return;
        };
        let mut text = Composer::default();
        // Seeded with the current name: a rename is usually a correction.
        text.set_text(&sound.name);
        self.prompt = Some((Prompt::SoundName(index), text));
    }

    /// Apply a renamed sound.
    pub fn rename_sound(&mut self, index: usize, name: String) {
        let (Some(handle), Some(view)) = (&self.handle, &mut self.server_management) else {
            return;
        };
        let guild_id = view.guild_id;
        let Some(sound) = view.sounds.get_mut(index) else {
            return;
        };
        if !concord::discord::is_valid_sound_name(&name) || sound.name == name {
            return;
        }

        // Applied locally too: the list is a snapshot, and leaving the old
        // name showing makes a successful rename look like it failed.
        sound.name = name.clone();
        let sound_id = sound.sound_id;
        handle.send(AppCommand::RenameSoundboardSound {
            guild_id,
            sound_id,
            name,
        });
    }

    /// Delete a role.
    ///
    /// Refuses @everyone and anything at or above your own highest role, with
    /// the reason, rather than round-tripping to a failure Discord explains
    /// less clearly.
    pub fn delete_role(&mut self, index: usize) {
        let (Some(handle), Some(view)) = (&self.handle, &self.server_management) else {
            return;
        };
        let Some(role) = view.roles.get(index).cloned() else {
            return;
        };
        let guild_id = view.guild_id;

        // @everyone is the guild itself.
        if role.id.get() == guild_id.get() {
            self.model.status_line = t!("status-everyone-undeletable");
            return;
        }
        let allowed = self.last_state.as_ref().is_some_and(|state| {
            state.can_manage_roles(guild_id) && state.can_assign_role(guild_id, role.id)
        });
        if !allowed {
            self.model.status_line = t!("status-role-outranks-you");
            return;
        }

        handle.send(AppCommand::DeleteRole {
            guild_id,
            role_id: role.id,
            label: role.name.clone(),
        });
        if let Some(view) = &mut self.server_management {
            view.roles.remove(index);
        }
    }

    /// Turn an AutoMod rule on or off.
    ///
    /// The common case, and non-destructive: a rule switched off can be
    /// switched back on, and its keyword list survives.
    pub fn toggle_automod_rule(&mut self, index: usize) {
        let (Some(handle), Some(view)) = (&self.handle, &mut self.server_management) else {
            return;
        };
        let guild_id = view.guild_id;
        let Some(rule) = view.automod.get_mut(index) else {
            return;
        };
        rule.enabled = !rule.enabled;

        handle.send(AppCommand::SetAutoModRuleEnabled {
            guild_id,
            rule_id: rule.id,
            enabled: rule.enabled,
            label: rule.name.clone(),
        });
    }

    pub fn delete_automod_rule(&mut self, index: usize) {
        let (Some(handle), Some(view)) = (&self.handle, &mut self.server_management) else {
            return;
        };
        if index >= view.automod.len() {
            return;
        }
        let rule = view.automod.remove(index);
        handle.send(AppCommand::DeleteAutoModRule {
            guild_id: view.guild_id,
            rule_id: rule.id,
            label: rule.name,
        });
    }

    /// Delete one of the guild's sounds.
    pub fn delete_sound(&mut self, index: usize) {
        let (Some(handle), Some(view)) = (&self.handle, &mut self.server_management) else {
            return;
        };
        if index >= view.sounds.len() {
            return;
        }
        let sound = view.sounds.remove(index);
        handle.send(AppCommand::DeleteSoundboardSound {
            guild_id: view.guild_id,
            sound_id: sound.sound_id,
            label: sound.name,
        });
    }
}
