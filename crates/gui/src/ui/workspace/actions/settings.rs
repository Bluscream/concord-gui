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
    pub fn compose_externally(&mut self, cx: &mut Context<Self>) {
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
    pub fn refresh_slash(&mut self) {
        self.slash = SlashPicker::for_input(self.composer.text(), &self.app_commands);

        // Past the command name the picker closes, and completion becomes the
        // bot's job rather than ours.
        if self.slash.is_none() {
            self.request_command_autocomplete();
        }
    }

    /// Accept the highlighted completion.
    pub fn accept_slash(&mut self) {
        if let Some(replacement) = self.slash.as_ref().and_then(|picker| picker.completion()) {
            self.composer.set_text(&replacement);
        }
        self.slash = None;
    }

    /// Dispatch a builtin slash command, if the content is one.
    ///
    /// Parsing lives in the core so the GUI and TUI accept the same syntax.
    /// Returns true when the content was handled as a command.
    pub fn dispatch_slash(&mut self, content: &str, channel_id: Id<marker::ChannelMarker>) -> bool {
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
                        // A slash command's text is its own payload; staged
                        // stickers belong to the composer's own send.
                        sticker_ids: Vec::new(),
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
                // Checked before asking: warning about an action that cannot
                // happen would be worse than doing nothing quietly.
                if self.current_user.is_none() {
                    return true;
                }

                // Editing a profile from a third-party client is one of the
                // actions Discord's anti-spam checks watch, so it asks first.
                if self.confirm_risk(RiskAction::ProfileEdit(guild_id, nickname.clone())) {
                    self.set_nickname_confirmed(guild_id, nickname);
                }
                true
            }
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::FriendRequest {
                target,
            }) => {
                if concord::discord::friend_request_target(&target).is_none() {
                    self.model.status_line = format!("{target} is not a username");
                } else {
                    self.friend_action(AppCommand::SendFriendRequest { target });
                }
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
    pub fn dispatch_application_command(
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
    pub fn notify_typing(&mut self) {
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

    /// Add, remove or block someone.
    ///
    /// Every friend action goes through here so the warning is asked once, in
    /// one place: managing the friends list is on the short list of things
    /// that get third-party clients flagged.
    pub fn friend_action(&mut self, command: AppCommand) {
        if !self.confirm_risk(RiskAction::FriendAction(Box::new(command.clone()))) {
            return;
        }
        if let Some(handle) = &self.handle {
            handle.send(command);
        }
    }

    /// Set the presence others see.
    pub fn set_status(&mut self, status: PresenceStatus) {
        let Some(handle) = &self.handle else {
            return;
        };
        self.status = status;
        handle.send(AppCommand::UpdateCurrentUserStatus { status });
    }

    /// Open the activity editor, seeded with whatever is being broadcast.
    pub fn open_activity_editor(&mut self) {
        self.editing_activity = Some(ActivityDraft::from_current(self.current_activity.as_ref()));
    }

    /// Send the composed activity.
    ///
    /// An empty name clears rather than broadcasting a nameless activity,
    /// which Discord shows as an empty line to everyone who looks.
    pub fn submit_activity(&mut self) {
        let Some(draft) = self.editing_activity.take() else {
            return;
        };
        let Some(handle) = &self.handle else {
            return;
        };

        let activities = draft.to_activities();
        self.current_activity = activities.first().cloned();

        handle.send(AppCommand::UpdateCurrentUserActivity {
            status: self.status,
            activities,
            // Typed by hand, so an RPC-detected game must not replace it.
            track_client_id: None,
        });
    }

    /// Stop broadcasting an activity.
    pub fn clear_activity(&mut self) {
        self.editing_activity = None;
        self.current_activity = None;
        if let Some(handle) = &self.handle {
            handle.send(AppCommand::UpdateCurrentUserActivity {
                status: self.status,
                activities: Vec::new(),
                track_client_id: None,
            });
        }
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

    /// Drop a departed guild's cached conversation.
    ///
    /// No warning: this removes only local data, and rule 6's warnings are
    /// about actions Discord watches. Nothing is sent.
    pub fn forget_guild(&mut self) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };
        let label = self
            .model
            .guilds
            .iter()
            .find(|guild| guild.id == Some(guild_id))
            .map(|guild| guild.name.clone())
            .unwrap_or_else(|| guild_id.get().to_string());

        handle.send(AppCommand::ForgetGuild { guild_id, label });
    }

    /// Leave the open guild.
    pub fn leave_guild(&mut self) {
        if !self.confirm_risk(RiskAction::LeaveGuild) {
            return;
        }
        self.leave_guild_confirmed();
    }

    pub fn leave_guild_confirmed(&mut self) {
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
}
