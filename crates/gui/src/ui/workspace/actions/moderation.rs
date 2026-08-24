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


    pub fn start_sticker_rename(&mut self, index: usize) {
        let Some(view) = &self.server_management else {
            return;
        };
        let Some(sticker) = view.stickers.get(index) else {
            return;
        };
        let mut text = Composer::default();
        text.set_text(&sticker.name);
        self.prompt = Some((Prompt::StickerRename(index), text));
    }

    pub fn rename_sticker(&mut self, index: usize, name: String) {
        let (Some(handle), Some(view)) = (&self.handle, &self.server_management) else {
            return;
        };
        let Some(sticker) = view.stickers.get(index) else {
            return;
        };
        if let Some(problem) = concord::discord::sticker_name_problem(&name) {
            self.model.status_line = problem.to_owned();
            return;
        }
        handle.send(AppCommand::RenameSticker {
            guild_id: view.guild_id,
            sticker_id: sticker.id,
            name,
        });
    }

    pub fn delete_sticker(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.server_management else {
            return;
        };
        if index >= view.stickers.len() {
            return;
        }
        let guild_id = view.guild_id;
        let sticker = view.stickers.remove(index);
        handle.send(AppCommand::DeleteSticker {
            guild_id,
            sticker_id: sticker.id,
            label: sticker.name,
        });
    }

    /// The discovery tab's rows as label and value.
    pub fn discovery_rows(&self) -> Vec<(String, String)> {
        let Some(view) = &self.server_management else {
            return Vec::new();
        };
        let Some(metadata) = view.discovery.as_ref() else {
            return Vec::new();
        };
        vec![
            (
                t!("label-primary-category"),
                match metadata.primary_category_id {
                    // Named rather than numbered, and "none" says the server
                    // is not listed at all - which the other fields do not.
                    Some(id) => view
                        .discovery_categories
                        .iter()
                        .find(|category| category.id == id)
                        .map_or_else(|| format!("category {id}"), |c| c.name.clone()),
                    None => t!("status-not-listed"),
                },
            ),
            (
                t!("label-keywords"),
                if metadata.keywords.is_empty() {
                    t!("state-none")
                } else {
                    metadata.keywords.join(", ")
                },
            ),
            (
                t!("label-emoji-discoverability"),
                if metadata.emoji_discoverability_enabled {
                    t!("state-on")
                } else {
                    t!("state-off")
                },
            ),
            (
                t!("label-about"),
                metadata
                    .about
                    .clone()
                    .unwrap_or_else(|| t!("state-not-set")),
            ),
        ]
    }

    /// Set the keywords or the description, whichever was typed.
    pub fn set_discovery_text(&mut self, keywords: Option<String>, about: Option<String>) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.server_management else {
            return;
        };
        let guild_id = view.guild_id;
        let Some(metadata) = view.discovery.as_mut() else {
            return;
        };
        if let Some(keywords) = keywords {
            metadata.keywords = keywords
                .split(',')
                .map(|keyword| keyword.trim().to_owned())
                .filter(|keyword| !keyword.is_empty())
                .collect();
        }
        if let Some(about) = about {
            // Empty clears it, which is a real thing to want.
            metadata.about = (!about.trim().is_empty()).then(|| about.trim().to_owned());
        }
        // Refused here rather than by Discord, whose rejection does not name
        // the field that was wrong.
        if let Some(problem) = metadata.problem() {
            self.model.status_line = problem.message();
            return;
        }
        let metadata = metadata.clone();
        handle.send(AppCommand::ModifyDiscoveryMetadata {
            guild_id,
            metadata: Box::new(metadata),
        });
    }

    /// Act on a discovery row.
    pub fn activate_discovery_row(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.server_management else {
            return;
        };
        let guild_id = view.guild_id;
        let categories = view.discovery_categories.clone();
        let Some(metadata) = view.discovery.as_mut() else {
            return;
        };
        match index {
            0 => {
                // Cycles the categories Discord allows as primary, plus
                // "none" - which unlists the server and must be reachable.
                let primary: Vec<u32> = categories
                    .iter()
                    .filter(|category| category.is_primary)
                    .map(|category| category.id)
                    .collect();
                if primary.is_empty() {
                    return;
                }
                metadata.primary_category_id = match metadata.primary_category_id {
                    None => primary.first().copied(),
                    Some(current) => primary
                        .iter()
                        .position(|id| *id == current)
                        .and_then(|at| primary.get(at + 1).copied()),
                };
            }
            2 => {
                metadata.emoji_discoverability_enabled = !metadata.emoji_discoverability_enabled;
            }
            // Keywords and the description need text, through their prompts.
            1 => {
                self.prompt = Some((Prompt::DiscoveryKeywords, Composer::default()));
                return;
            }
            3 => {
                self.prompt = Some((Prompt::DiscoveryAbout, Composer::default()));
                return;
            }
            _ => return,
        }
        let metadata = metadata.clone();
        handle.send(AppCommand::ModifyDiscoveryMetadata {
            guild_id,
            metadata: Box::new(metadata),
        });
    }

    /// The onboarding form as rows, rebuilt from what has been picked.
    pub fn onboarding_rows(&self) -> Vec<concord::discord::OnboardingRow> {
        let Some(view) = &self.server_management else {
            return Vec::new();
        };
        view.onboarding
            .as_ref()
            .map(|onboarding| onboarding.rows(&view.onboarding_picked))
            .unwrap_or_default()
    }

    /// Pick or unpick an answer.
    ///
    /// The rules live in the core: a single-select question replaces its
    /// answer rather than adding to it, and only its own answers are cleared.
    pub fn pick_onboarding_answer(&mut self, index: usize) {
        let rows = self.onboarding_rows();
        let Some(option_id) = rows.get(index).and_then(|row| row.option_id()) else {
            return;
        };
        let Some(view) = &mut self.server_management else {
            return;
        };
        let Some(onboarding) = view.onboarding.as_ref() else {
            return;
        };
        view.onboarding_picked = onboarding.toggled(&view.onboarding_picked, option_id);
    }

    /// Send the answers.
    pub fn submit_onboarding(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &self.server_management else {
            return;
        };
        let Some(onboarding) = view.onboarding.clone() else {
            return;
        };
        let unanswered = onboarding.unanswered(&view.onboarding_picked);
        if !unanswered.is_empty() {
            // Named rather than counted: Discord's own rejection does not say
            // which question is missing.
            self.model.status_line = unanswered.join(", ");
            return;
        }
        handle.send(AppCommand::SubmitOnboarding {
            guild_id: view.guild_id,
            onboarding: Box::new(onboarding),
            picked: view.onboarding_picked.clone(),
        });
    }

    /// The guild's members, filtered by the search box.
    ///
    /// Read from the snapshot on every draw rather than cached: the gateway
    /// keeps changing it, and a cached copy would list members who have left.
    pub fn server_member_rows(&self) -> Vec<concord::discord::MemberRow> {
        let (Some(view), Some(state)) = (&self.server_management, &self.last_state) else {
            return Vec::new();
        };
        state.member_rows(view.guild_id, &view.member_query)
    }

    pub fn ban_listed_member(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &self.server_management else {
            return;
        };
        let rows = self.server_member_rows();
        let Some(member) = rows.get(index) else {
            return;
        };
        handle.send(AppCommand::BanMember {
            guild_id: view.guild_id,
            user_id: member.user_id,
            label: member.name.clone(),
            delete_message_seconds: 0,
        });
    }

    pub fn open_listed_member(&mut self, index: usize) {
        let rows = self.server_member_rows();
        if let Some(member) = rows.get(index) {
            self.open_profile(member.user_id);
        }
    }

    /// The membership tab's rows as label and value.
    pub fn membership_rows(&self) -> Vec<(String, String)> {
        let Some(view) = &self.server_management else {
            return Vec::new();
        };
        vec![
            (
                t!("label-welcome-screen"),
                // "unknown" rather than "off": one that has not arrived is not
                // one Discord confirmed is off.
                view.welcome.as_ref().map_or_else(
                    || t!("state-unknown"),
                    |screen| {
                        if screen.enabled {
                            t!("state-on")
                        } else {
                            t!("state-off")
                        }
                    },
                ),
            ),
            (
                t!("label-welcome-description"),
                view.welcome
                    .as_ref()
                    .and_then(|screen| screen.description.clone())
                    .unwrap_or_else(|| t!("state-not-set")),
            ),
            (
                t!("label-widget"),
                view.widget.as_ref().map_or_else(
                    || t!("state-unknown"),
                    |widget| {
                        if widget.enabled {
                            t!("state-on")
                        } else {
                            t!("state-off")
                        }
                    },
                ),
            ),
            (
                t!("label-widget-channel"),
                view.widget
                    .as_ref()
                    .and_then(|widget| widget.channel_id)
                    .map_or_else(|| t!("state-no-invite"), |id| self.channel_name(id)),
            ),
            (t!("label-prune-after"), format!("{} days", view.prune_days)),
            (
                t!("label-prune"),
                match view.prune_count {
                    // Zero is a real answer and the commonest one: Discord
                    // exempts every member who has any role at all.
                    Some(count) => {
                        t!("status-prune-count", "count" => i64::try_from(count).unwrap_or(i64::MAX))
                    }
                    None => t!("status-loading"),
                },
            ),
        ]
    }

    /// Act on a membership row.
    pub fn activate_membership_row(&mut self, index: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(view) = &mut self.server_management else {
            return;
        };
        let guild_id = view.guild_id;
        match index {
            0 => {
                let Some(screen) = view.welcome.as_mut() else {
                    return;
                };
                screen.enabled = !screen.enabled;
                handle.send(AppCommand::ModifyWelcomeScreen {
                    guild_id,
                    edit: concord::discord::WelcomeScreenEdit {
                        enabled: Some(screen.enabled),
                        ..Default::default()
                    },
                });
            }
            2 => {
                let Some(widget) = view.widget.as_mut() else {
                    return;
                };
                widget.enabled = !widget.enabled;
                handle.send(AppCommand::ModifyGuildWidget {
                    guild_id,
                    widget: widget.clone(),
                });
            }
            4 => {
                // Cycles through what Discord accepts, wrapping, so every
                // window is reachable from every other.
                let current = concord::discord::PRUNE_DAYS
                    .iter()
                    .position(|days| *days == view.prune_days)
                    .unwrap_or(0);
                let next = (current + 1) % concord::discord::PRUNE_DAYS.len();
                view.prune_days = concord::discord::PRUNE_DAYS[next];
                // The old count described the old window, so it is cleared
                // rather than left to describe the wrong one.
                view.prune_count = None;
                handle.send(AppCommand::LoadPruneCount {
                    guild_id,
                    days: view.prune_days,
                    include_roles: Vec::new(),
                });
            }
            5 => self.prune_guild(),
            // The two that need text rather than a toggle.
            1 => {
                let mut text = Composer::default();
                if let Some(description) = view
                    .welcome
                    .as_ref()
                    .and_then(|screen| screen.description.clone())
                {
                    // Seeded: this is usually a correction rather than
                    // something written from nothing.
                    text.set_text(&description);
                }
                self.prompt = Some((Prompt::WelcomeDescription, text));
            }
            3 => self.prompt = Some((Prompt::WidgetChannel, Composer::default())),
            _ => {}
        }
    }

    /// The guild's name, or a stand-in for the log line.
    pub fn guild_name_or_default(&self, guild_id: Id<marker::GuildMarker>) -> String {
        self.model
            .guilds
            .iter()
            .find(|guild| guild.id == Some(guild_id))
            .map_or_else(|| "this server".to_owned(), |guild| guild.name.clone())
    }

    /// Remove inactive members, behind the risk prompt.
    pub fn prune_guild(&mut self) {
        let Some(view) = &self.server_management else {
            return;
        };
        // Nothing to confirm when the count is zero or has not arrived: a
        // warning about removing nobody teaches the wrong lesson about it.
        if view.prune_count.unwrap_or(0) == 0 {
            return;
        }
        let (guild_id, days) = (view.guild_id, view.prune_days);
        let command = AppCommand::PruneGuild {
            guild_id,
            days,
            include_roles: Vec::new(),
            label: self.guild_name_or_default(guild_id),
        };
        if !self.confirm_risk(RiskAction::PruneMembers(Box::new(command.clone()))) {
            return;
        }
        if let Some(handle) = &self.handle {
            handle.send(command);
        }
    }

    pub fn open_server_management(&mut self, tab: ServerTab) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };

        self.server_management = Some(ServerManagementView {
            guild_id,
            tab,
            invites: Vec::new(),
            emojis: Vec::new(),
            audit_log: Vec::new(),
            roles: Vec::new(),
            sounds: Vec::new(),
            automod: Vec::new(),
            settings: Vec::new(),
            welcome: None,
            widget: None,
            prune_days: DEFAULT_PRUNE_DAYS,
            prune_count: None,
            member_query: String::new(),
            onboarding: None,
            onboarding_picked: Vec::new(),
            discovery: None,
            discovery_categories: Vec::new(),
            stickers: Vec::new(),
            events: Vec::new(),
            templates: Vec::new(),
            loading: true,
            error: None,
        });
        let fetches = tab.load(guild_id);
        if fetches.is_empty() {
            // Settings and roles read the snapshot, so nothing is asked for.
            self.fill_server_snapshot_tab(tab);
            if let Some(view) = &mut self.server_management {
                view.loading = false;
            }
            return;
        }
        for command in fetches {
            handle.send(command);
        }
    }

    /// Fill a tab that reads the snapshot rather than fetching.
    pub fn fill_server_snapshot_tab(&mut self, tab: ServerTab) {
        let Some(guild_id) = self.server_management.as_ref().map(|view| view.guild_id) else {
            return;
        };
        let Some(state) = self.last_state.as_ref() else {
            return;
        };

        match tab {
            ServerTab::Roles => {
                let mut roles: Vec<_> = state
                    .roles_for_guild(guild_id)
                    .into_iter()
                    .cloned()
                    .collect();
                // Highest first, the order that decides which role wins a
                // permission conflict.
                roles.sort_by_key(|role| std::cmp::Reverse(role.position));
                if let Some(view) = &mut self.server_management {
                    view.roles = roles;
                }
            }
            ServerTab::Settings => {
                let Some(guild) = state.guild(guild_id) else {
                    return;
                };
                // Only what the snapshot carries: default notifications and
                // the explicit-content filter are not parsed off the wire, so
                // showing them would mean showing a guess.
                let settings = vec![
                    (t!("label-name"), guild.name.clone()),
                    (
                        t!("label-verification"),
                        concord::discord::verification_label(
                            guild.verification_level.unwrap_or_default(),
                        ),
                    ),
                    (
                        t!("label-boosts"),
                        format!("{} ({:?})", guild.boost_count, guild.boost_tier),
                    ),
                ];
                if let Some(view) = &mut self.server_management {
                    view.settings = settings;
                }
            }
            _ => {}
        }
    }

    /// Switch tabs, fetching that tab's list if it has not been fetched.
    pub fn select_server_tab(&mut self, tab: ServerTab) {
        let (Some(handle), Some(view)) = (&self.handle, &mut self.server_management) else {
            return;
        };
        if view.tab == tab {
            return;
        }
        view.tab = tab;
        view.error = None;

        // Only fetch what has not arrived yet. Refetching on every tab switch
        // would make the panel flicker and spend requests for no new
        // information; a refresh is what the reload control is for.
        let already_loaded = match tab {
            ServerTab::Settings | ServerTab::Roles => true,
            ServerTab::Invites => !view.invites.is_empty(),
            ServerTab::Emoji => !view.emojis.is_empty(),
            ServerTab::Sounds => !view.sounds.is_empty(),
            ServerTab::AutoMod => !view.automod.is_empty(),
            ServerTab::AuditLog => !view.audit_log.is_empty(),
            ServerTab::Membership => view.welcome.is_some() && view.widget.is_some(),
            ServerTab::Events => !view.events.is_empty(),
            ServerTab::Templates => !view.templates.is_empty(),
            ServerTab::Members => true,
            ServerTab::Onboarding => view.onboarding.is_some(),
            ServerTab::Discovery => view.discovery.is_some(),
            ServerTab::Stickers => !view.stickers.is_empty(),
        };
        view.loading = !already_loaded;
        let guild_id = view.guild_id;
        if !already_loaded {
            for command in tab.load(guild_id) {
                handle.send(command);
            }
        }
        self.fill_server_snapshot_tab(tab);
    }

    /// Refetch the open tab.
    pub fn reload_server_tab(&mut self) {
        let (Some(handle), Some(view)) = (&self.handle, &mut self.server_management) else {
            return;
        };
        let (tab, guild_id) = (view.tab, view.guild_id);
        let fetches = tab.load(guild_id);
        match fetches.is_empty() {
            false => {
                view.loading = true;
                view.error = None;
                for command in fetches {
                    handle.send(command);
                }
            }
            // A refresh of a snapshot tab re-reads rather than spending a
            // request that would fetch nothing.
            true => self.fill_server_snapshot_tab(tab),
        }
    }

    /// Revoke an invite, taking the row out straight away.
    ///
    /// The list is a snapshot; leaving a revoked code on screen invites a
    /// second revoke for one that no longer exists.
    pub fn revoke_invite(&mut self, index: usize) {
        let (Some(handle), Some(view)) = (&self.handle, &mut self.server_management) else {
            return;
        };
        if index >= view.invites.len() {
            return;
        }
        let invite = view.invites.remove(index);
        handle.send(AppCommand::RevokeInvite { code: invite.code });
    }

    /// Start renaming a custom emoji.
    pub fn start_emoji_rename(&mut self, index: usize) {
        let Some(view) = &self.server_management else {
            return;
        };
        let Some(emoji) = view.emojis.get(index) else {
            return;
        };

        // Seeded with the current name: a rename is usually a correction, and
        // retyping the whole thing to fix one letter is busywork.
        let mut text = Composer::default();
        text.set_text(&emoji.name);
        self.prompt = Some((Prompt::EmojiName(index), text));
    }

    /// Add an emoji from an image on disk.
    ///
    /// The name comes from the filename, which is what people mean nine times
    /// out of ten and can be corrected with the rename control afterwards.
    pub fn create_emoji(&mut self, path: String) {
        let (Some(handle), Some(view)) = (&self.handle, &self.server_management) else {
            return;
        };
        let Some(name) = concord::discord::emoji_name_from_filename(&path) else {
            self.model.status_line = t!("status-emoji-name-unusable");
            return;
        };

        handle.send(AppCommand::CreateEmoji {
            guild_id: view.guild_id,
            name,
            image: Box::new(concord::discord::ProfileAvatarUpload::from_path(
                path.into(),
            )),
        });
        // Not added to the list here: unlike a rename, this can fail on size
        // or format, and a row that vanishes on the next reload is worse than
        // one that appears when it is really there.
        self.model.status_line = t!("status-emoji-uploading");
    }

    /// Apply a renamed emoji.
    pub fn rename_emoji(&mut self, index: usize, name: String) {
        let (Some(handle), Some(view)) = (&self.handle, &mut self.server_management) else {
            return;
        };
        let Some(emoji) = view.emojis.get_mut(index) else {
            return;
        };
        if emoji.name == name {
            return;
        }

        // Applied locally too: the list is a snapshot, and leaving the old
        // name showing makes a successful rename look like it failed.
        emoji.name = name.clone();
        handle.send(AppCommand::RenameEmoji {
            guild_id: view.guild_id,
            emoji_id: emoji.id,
            name,
        });
    }

    /// Edit what a role may do.
    pub fn open_role_permissions(&mut self, index: usize) {
        let Some(view) = &self.server_management else {
            return;
        };
        let Some(role) = view.roles.get(index).cloned() else {
            return;
        };
        let guild_id = view.guild_id;

        // Discord refuses a change to a role at or above your own highest,
        // which is what stops anyone granting themselves more than they have.
        let allowed = self.last_state.as_ref().is_some_and(|state| {
            state.can_manage_roles(guild_id) && state.can_assign_role(guild_id, role.id)
        });
        if !allowed {
            self.model.status_line = t!("status-role-outranks-you");
            return;
        }

        self.permission_grid = Some(PermissionGridView {
            scope: PermissionScope::Role {
                guild_id,
                role_id: role.id,
            },
            name: role.name,
            allow: role.permissions,
            deny: 0,
            original_allow: role.permissions,
            original_deny: 0,
        });
    }

    /// Edit a channel's overwrite for @everyone.
    ///
    /// Seeded from the existing overwrite when there is one, so the grid opens
    /// on what is actually in force rather than on blank.
    pub fn open_channel_overwrite(&mut self, channel_id: Id<marker::ChannelMarker>) {
        let Some(state) = self.last_state.as_ref() else {
            return;
        };
        let Some(channel) = state.channel(channel_id) else {
            return;
        };
        let Some(guild_id) = channel.guild_id else {
            return;
        };
        if !state.can_manage_roles(guild_id) {
            self.model.status_line = t!("status-no-permission");
            return;
        }

        // @everyone's role id is the guild id.
        let role_id = concord::discord::Id::new(guild_id.get());
        let existing = channel
            .permission_overwrites
            .iter()
            .find(|overwrite| overwrite.id == role_id.get());
        let (allow, deny) = existing.map_or((0, 0), |overwrite| (overwrite.allow, overwrite.deny));

        self.permission_grid = Some(PermissionGridView {
            scope: PermissionScope::ChannelOverwrite {
                channel_id,
                target: concord::discord::OverwriteTarget::Role(role_id),
            },
            name: format!("#{} - @everyone", channel.name),
            allow,
            deny,
            original_allow: allow,
            original_deny: deny,
        });
    }

    /// Step one permission through the states its scope has.
    ///
    /// Inherit, allow, deny for an overwrite; on and off for a role, which has
    /// no inherit to offer.
    pub fn cycle_permission(&mut self, index: usize) {
        let Some(view) = &mut self.permission_grid else {
            return;
        };
        let Some(permission) = concord::discord::permissions_catalogue::ALL
            .get(index)
            .copied()
        else {
            return;
        };
        use concord::discord::permissions_catalogue::with;

        let allowed = permission.is_set(view.allow);
        let denied = permission.is_set(view.deny);

        if !view.allows_inherit() {
            view.allow = with(view.allow, permission, !allowed);
            return;
        }
        // inherit -> allow -> deny -> inherit
        let (allow, deny) = match (allowed, denied) {
            (false, false) => (true, false),
            (true, _) => (false, true),
            (false, true) => (false, false),
        };
        view.allow = with(view.allow, permission, allow);
        view.deny = with(view.deny, permission, deny);
    }
}
