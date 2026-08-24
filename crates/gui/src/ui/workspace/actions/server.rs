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

    pub fn disable_totp(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.account else {
            return;
        };
        // Only when no enrolment is in progress, or this would disable using a
        // code meant for the enrolment being set up.
        if view.totp_secret.is_some() || view.totp_code.is_empty() {
            return;
        }
        let code = std::mem::take(&mut view.totp_code);
        handle.send(AppCommand::DisableTotp { code });
    }

    /// Fetch the backup codes, or regenerate them - which invalidates the old
    /// ones, so it is a flag rather than something the fetch does on its own.
    pub fn load_backup_codes(&mut self, regenerate: bool) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &self.account else {
            return;
        };
        let password = view
            .form
            .value(concord::discord::AccountField::CurrentPassword);
        if password.is_empty() {
            return;
        }
        handle.send(AppCommand::LoadBackupCodes {
            password: concord::discord::Secret::new(password),
            regenerate,
        });
    }

    /// Send the credential change, consuming the form so no copy of three
    /// passwords is left behind.
    pub fn submit_account_form(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.account else {
            return;
        };
        if view.form.problem().is_some() {
            return;
        }
        let form = std::mem::take(&mut view.form);
        if let Some(command) = form.submit() {
            handle.send(command);
        }
    }

    pub fn open_access(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };
        self.access = Some(AccessView {
            sessions: Vec::new(),
            apps: Vec::new(),
            loading: true,
            error: None,
            logout_targets: std::collections::BTreeSet::new(),
            password: String::new(),
        });
        // Both, because the panel shows both and fetching one on demand would
        // leave half of it empty until it was touched.
        handle.send(AppCommand::LoadAuthSessions);
        handle.send(AppCommand::LoadAuthorisedApps);
    }

    /// Select a session for logout, or revoke an app.
    ///
    /// Sessions select rather than act: Discord needs the account password, so
    /// it takes a prompt either way, and one prompt for several beats one each.
    pub fn activate_access_row(&mut self, index: usize) {
        let Some(view) = &mut self.access else {
            return;
        };
        if let Some(session) = view.sessions.get(index) {
            let id_hash = session.id_hash.clone();
            if !view.logout_targets.remove(&id_hash) {
                view.logout_targets.insert(id_hash);
            }
            return;
        }

        // Apps are listed after sessions, so the row index is offset by them.
        let Some(app_index) = index.checked_sub(view.sessions.len()) else {
            return;
        };
        if app_index >= view.apps.len() {
            return;
        }
        let app = view.apps.remove(app_index);
        if let Some(handle) = &self.handle {
            handle.send(AppCommand::RevokeAuthorisedApp {
                id: app.id,
                label: app.name,
            });
        }
    }

    /// Take one keystroke into the password field.
    pub fn type_access_password(&mut self, key: &str) {
        let Some(view) = &mut self.access else {
            return;
        };
        match key {
            "backspace" => {
                view.password.pop();
            }
            other => {
                // Only real characters. A bare modifier or an arrow key
                // arrives here as a name like "shift", and appending it would
                // put "shift" into the password.
                let mut characters = other.chars();
                if let (Some(character), None) = (characters.next(), characters.next()) {
                    view.password.push(character);
                }
            }
        }
    }

    /// Send the logout, dropping the password in the same step.
    pub fn log_out_selected_sessions(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.access else {
            return;
        };
        let id_hashes: Vec<String> = view.logout_targets.iter().cloned().collect();
        let password = std::mem::take(&mut view.password);
        view.logout_targets.clear();
        // Empty is not sent: Discord would reject it, and the round trip would
        // read as a wrong password rather than as an empty one.
        if id_hashes.is_empty() || password.is_empty() {
            return;
        }
        handle.send(AppCommand::RevokeAuthSessions {
            id_hashes,
            password: concord::discord::Secret::new(password),
        });
    }

    /// Open the privacy and safety panel.
    pub fn open_privacy(&mut self) {
        self.privacy_open = true;
    }

    /// Privacy and safety as the account last reported it.
    ///
    /// Nothing arrived yet leaves every field None, which the panel shows as
    /// "unknown" rather than as a set of permissive defaults.
    pub fn privacy_state(&self) -> concord::discord::PrivacyState {
        self.last_state
            .as_ref()
            .map_or_else(Default::default, |state| state.privacy_state())
    }

    /// Whether this server's members may send you direct messages.
    pub fn toggle_guild_direct_messages(&mut self, guild_id: Id<marker::GuildMarker>) {
        let Some(handle) = &self.handle else {
            return;
        };
        let edit = self.privacy_state().toggled_guild_direct_messages(guild_id);
        handle.send(AppCommand::ModifyPrivacySettings { edit });
    }

    pub fn toggle_privacy_setting(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(setting) = concord::discord::PrivacySetting::at(index) else {
            return;
        };
        let edit = setting.toggled(&self.privacy_state());
        handle.send(AppCommand::ModifyPrivacySettings { edit });
    }

    pub fn open_connections(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };
        self.connections = Some(ConnectionsView {
            connections: Vec::new(),
            loading: true,
            error: None,
        });
        handle.send(AppCommand::LoadConnections);
    }

    /// Show or hide a connection on your profile.
    ///
    /// Visibility and activity go in the same request, so each sends the other
    /// unchanged rather than a default - which would quietly turn the other
    /// setting off.
    pub fn toggle_connection_visibility(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.connections else {
            return;
        };
        let Some(connection) = view.connections.get_mut(index) else {
            return;
        };
        connection.visibility = connection.visibility.toggled();
        handle.send(AppCommand::ModifyConnection {
            kind: connection.kind.clone(),
            id: connection.id.clone(),
            visibility: connection.visibility,
            show_activity: connection.show_activity,
            label: connection.name.clone(),
        });
    }

    /// Whether what you do on that service appears in your presence.
    pub fn toggle_connection_activity(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.connections else {
            return;
        };
        let Some(connection) = view.connections.get_mut(index) else {
            return;
        };
        connection.show_activity = !connection.show_activity;
        handle.send(AppCommand::ModifyConnection {
            kind: connection.kind.clone(),
            id: connection.id.clone(),
            visibility: connection.visibility,
            show_activity: connection.show_activity,
            label: connection.name.clone(),
        });
    }

    pub fn unlink_connection(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.connections else {
            return;
        };
        if index >= view.connections.len() {
            return;
        }
        let connection = view.connections.remove(index);
        handle.send(AppCommand::DeleteConnection {
            kind: connection.kind,
            id: connection.id,
            label: connection.name,
        });
    }

    pub fn open_soundboard(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };
        let guild_id = match self.nav.selection {
            Selection::Guild(guild_id) => Some(guild_id),
            Selection::DirectMessages => None,
        };

        self.soundboard = Some(SoundboardView {
            guild_sounds: Vec::new(),
            default_sounds: Vec::new(),
            loading: true,
            error: None,
        });

        // Both lists, because a guild that has added no sounds should still
        // get a usable picker rather than an empty one.
        if let Some(guild_id) = guild_id {
            handle.send(AppCommand::LoadSoundboardSounds {
                guild_id: Some(guild_id),
            });
        }
        handle.send(AppCommand::LoadSoundboardSounds { guild_id: None });
    }

    /// Play the picked sound into the voice channel we are in.
    pub fn play_sound(&mut self, index: usize) {
        let (Some(handle), Some(view)) = (&self.handle, &self.soundboard) else {
            return;
        };
        let Some((channel_id, _)) = self.voice_channel else {
            return;
        };
        let Some(sound) = view.sounds().nth(index) else {
            return;
        };
        // Refused rather than sent: Discord rejects an unavailable sound, and
        // the picker already says why it is greyed.
        if !sound.available {
            return;
        }

        handle.send(AppCommand::PlaySoundboardSound {
            channel_id,
            sound_id: sound.sound_id,
            source_guild_id: sound.guild_id,
            label: sound.name.clone(),
        });
    }

    /// Open the server-management panel on the given tab.
    /// Move a role up or down.
    ///
    /// Position decides which role wins a permission conflict, so this is a
    /// permission change rather than a cosmetic one.
    pub fn move_role(&mut self, index: usize, up: bool) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.server_management else {
            return;
        };
        let guild_id = view.guild_id;
        let ordered: Vec<_> = view.roles.iter().map(|role| role.id).collect();
        let positions = concord::discord::moved_positions(&ordered, index, up);
        if positions.is_empty() {
            return;
        }
        // Moved locally too, so the list does not sit still until the refetch
        // arrives - which reads as a button that did nothing.
        let swap_with = if up { index - 1 } else { index + 1 };
        view.roles.swap(index, swap_with);
        handle.send(AppCommand::ReorderRoles {
            guild_id,
            positions,
        });
    }

    /// Move a channel up or down among its siblings.
    ///
    /// Siblings only: a channel moved past its category boundary would leave
    /// the category, which is a different action from reordering within one.
    pub fn move_channel(&mut self, channel_id: Id<marker::ChannelMarker>, up: bool) {
        let (Some(handle), Selection::Guild(guild_id), Some(state)) =
            (&self.handle, self.nav.selection, &self.last_state)
        else {
            return;
        };
        let Some(channel) = state.channel(channel_id) else {
            return;
        };
        let parent = channel.parent_id;
        let ordered: Vec<_> = state
            .channels_for_guild(Some(guild_id))
            .into_iter()
            .filter(|sibling| sibling.parent_id == parent && !sibling.is_category())
            .map(|sibling| sibling.id)
            .collect();
        let Some(index) = ordered.iter().position(|id| *id == channel_id) else {
            return;
        };
        let positions = concord::discord::moved_positions(&ordered, index, up);
        if positions.is_empty() {
            return;
        }
        handle.send(AppCommand::ReorderChannels {
            guild_id,
            positions,
        });
    }

    /// Set the short line beside a voice channel's name.
    pub fn set_voice_status(&mut self, channel_id: Id<marker::ChannelMarker>, text: &str) {
        let Some(handle) = &self.handle else {
            return;
        };
        let status = text.trim().to_owned();
        handle.send(AppCommand::SetVoiceChannelStatus {
            channel_id,
            // Empty clears it, which is a real thing to want and distinct from
            // leaving the status alone.
            status: (!status.is_empty()).then_some(status),
        });
    }

    /// Cancel an event still to come, or delete one already finished.
    ///
    /// Cancelling first: Discord keeps a cancelled event visible so people who
    /// said they were coming can see it is off, which deleting does not.
    pub fn remove_event(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.server_management else {
            return;
        };
        let guild_id = view.guild_id;
        let Some(event) = view.events.get(index) else {
            return;
        };
        if event.status.is_cancellable() {
            handle.send(AppCommand::CancelScheduledEvent {
                guild_id,
                event_id: event.id,
                label: event.name.clone(),
            });
            return;
        }
        let event = view.events.remove(index);
        handle.send(AppCommand::DeleteScheduledEvent {
            guild_id,
            event_id: event.id,
            label: event.name,
        });
    }

    pub fn mark_event_interest(&mut self, index: usize) {
        let (Some(handle), Some(view)) = (&self.handle, &self.server_management) else {
            return;
        };
        let Some(event) = view.events.get(index) else {
            return;
        };
        handle.send(AppCommand::SetEventInterest {
            guild_id: view.guild_id,
            event_id: event.id,
            interested: true,
        });
    }

    pub fn sync_template(&mut self, index: usize) {
        let (Some(handle), Some(view)) = (&self.handle, &self.server_management) else {
            return;
        };
        let Some(template) = view.templates.get(index) else {
            return;
        };
        handle.send(AppCommand::SyncGuildTemplate {
            guild_id: view.guild_id,
            code: template.code.clone(),
            label: template.name.clone(),
        });
    }

    pub fn delete_template(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.server_management else {
            return;
        };
        if index >= view.templates.len() {
            return;
        }
        let guild_id = view.guild_id;
        let template = view.templates.remove(index);
        handle.send(AppCommand::DeleteGuildTemplate {
            guild_id,
            code: template.code,
            label: template.name,
        });
    }

    /// Stop or resume showing someone's camera and screen share.
    ///
    /// Local only, like the mute beside it: neither Discord nor the person is
    /// told. Hiding someone is a decision about your own screen, and telling
    /// them would make it a social act instead.
    pub fn toggle_video_hidden(&mut self, user_id: Id<marker::UserMarker>) {
        let hidden = if self.video_hidden.contains(&user_id) {
            self.video_hidden.remove(&user_id);
            false
        } else {
            self.video_hidden.insert(user_id);
            true
        };
        let volume = self.options.voice.voice_output_volume.value() as u16;
        let muted = self.locally_muted.contains(&user_id);
        self.set_participant_playback(user_id, volume, muted, hidden);
    }

    /// Join a discovered server by its vanity invite.
    ///
    /// Through the ordinary invite path rather than a discovery endpoint of
    /// its own: that path is already written and tested, and a server with no
    /// vanity code cannot be joined from here - which its row says.
    pub fn join_discovered_guild(&mut self, index: usize) {
        let Some(code) = self
            .discovered
            .get(index)
            .and_then(|guild| guild.vanity_url_code.clone())
        else {
            return;
        };
        self.discovered.clear();
        self.resolve_invite(&code);
    }

    /// Upload a sticker from one typed line.
    pub fn create_sticker(&mut self, text: &str) {
        let (Some(handle), Some(view)) = (&self.handle, &self.server_management) else {
            return;
        };
        let mut parts = text.split('|').map(str::trim);
        let name = parts.next().unwrap_or_default().to_owned();
        let tags = parts.next().unwrap_or_default().to_owned();
        // Anything after the second separator is part of the path, since a
        // filename may itself contain one.
        let path = parts.collect::<Vec<_>>().join(" | ");
        if name.is_empty() || path.is_empty() {
            return;
        }
        if let Some(problem) = concord::discord::sticker_name_problem(&name) {
            self.model.status_line = problem.to_owned();
            return;
        }
        handle.send(AppCommand::CreateSticker {
            guild_id: view.guild_id,
            name,
            tags,
            path,
        });
    }
}
