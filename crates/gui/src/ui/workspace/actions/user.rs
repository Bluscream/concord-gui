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

    pub fn delete_emoji(&mut self, index: usize) {
        let (Some(handle), Some(view)) = (&self.handle, &mut self.server_management) else {
            return;
        };
        if index >= view.emojis.len() {
            return;
        }
        let emoji = view.emojis.remove(index);
        handle.send(AppCommand::DeleteEmoji {
            guild_id: view.guild_id,
            emoji_id: emoji.id,
            label: emoji.name,
        });
    }

    /// Make an invite to the open channel.
    pub fn create_invite_here(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::CreateChannelInvite {
            channel_id,
            // Discord's own defaults: a day, unlimited uses, not temporary.
            // Anything narrower would be a guess about what was wanted.
            max_age_seconds: 86_400,
            max_uses: 0,
            temporary: false,
        });
    }

    /// Open the guild's ban list.
    pub fn open_ban_list(&mut self) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };

        self.bans = Some(BanListView {
            guild_id,
            bans: Vec::new(),
            loading: true,
            error: None,
        });
        handle.send(AppCommand::LoadGuildBans { guild_id });
    }

    /// Lift a ban, removing the row straight away.
    ///
    /// The list is a snapshot; leaving a lifted ban on screen invites a second
    /// unban for someone already unbanned.
    pub fn unban(&mut self, index: usize) {
        let (Some(handle), Some(view)) = (&self.handle, self.bans.as_mut()) else {
            return;
        };
        if index >= view.bans.len() {
            return;
        }

        let guild_id = view.guild_id;
        let ban = view.bans.remove(index);
        handle.send(AppCommand::UnbanMember {
            guild_id,
            user_id: ban.user_id,
            label: ban.username,
        });
    }

    /// Open the role picker for a member.
    pub fn open_role_picker(&mut self, user_id: Id<marker::UserMarker>) {
        let (Some(state), Selection::Guild(guild_id)) = (&self.last_state, self.nav.selection)
        else {
            return;
        };
        let assigned = state
            .member_for_guild(guild_id, user_id)
            .map(|member| member.role_ids.clone())
            .unwrap_or_default();

        self.editing_roles = Some((user_id, assigned));
    }

    /// Every role in the guild, ordered as Discord orders them.
    pub fn guild_roles(&self) -> Vec<overlay::RoleChoice> {
        let (Some(state), Selection::Guild(guild_id), Some((_, assigned))) = (
            &self.last_state,
            self.nav.selection,
            self.editing_roles.as_ref(),
        ) else {
            return Vec::new();
        };

        let mut roles = state.roles_for_guild(guild_id);
        // Highest first, matching Discord everywhere else.
        roles.sort_by(|a, b| b.position.cmp(&a.position).then(a.name.cmp(&b.name)));

        roles
            .into_iter()
            // @everyone carries the guild's own id, belongs to everyone
            // implicitly, and cannot be granted or taken away.
            .filter(|role| role.id.get() != guild_id.get())
            .map(|role| overlay::RoleChoice {
                name: role.name.clone(),
                color: role.color,
                assigned: assigned.contains(&role.id),
                disabled_reason: (!state.can_assign_role(guild_id, role.id))
                    .then_some("above your highest role"),
            })
            .collect()
    }

    /// Add or remove a role from the edited set, without sending yet.
    pub fn toggle_role(&mut self, index: usize) {
        let (Some(state), Selection::Guild(guild_id)) = (&self.last_state, self.nav.selection)
        else {
            return;
        };

        let mut roles = state.roles_for_guild(guild_id);
        roles.sort_by(|a, b| b.position.cmp(&a.position).then(a.name.cmp(&b.name)));
        let Some(role_id) = roles
            .into_iter()
            .filter(|role| role.id.get() != guild_id.get())
            .nth(index)
            .map(|role| role.id)
        else {
            return;
        };
        if !state.can_assign_role(guild_id, role_id) {
            return;
        }

        if let Some((_, assigned)) = &mut self.editing_roles {
            match assigned.iter().position(|id| *id == role_id) {
                Some(position) => {
                    assigned.remove(position);
                }
                None => assigned.push(role_id),
            }
        }
    }

    /// Send the edited role set.
    ///
    /// The whole set goes at once rather than one role at a time, which is
    /// what the official client does and avoids a race between two edits.
    pub fn save_roles(&mut self) {
        let (Some(handle), Selection::Guild(guild_id), Some((user_id, role_ids))) =
            (&self.handle, self.nav.selection, self.editing_roles.take())
        else {
            return;
        };

        let label = self
            .model
            .members
            .iter()
            .find(|member| member.user_id == Some(user_id))
            .map(|member| member.name.clone())
            .unwrap_or_else(|| user_id.get().to_string());

        handle.send(AppCommand::SetMemberRoles {
            guild_id,
            user_id,
            role_ids,
            label,
        });
    }

    /// Carry out a moderation action against a member.
    pub fn moderate(&mut self, user_id: Id<marker::UserMarker>, action: ModerationAction) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };

        let label = self
            .model
            .members
            .iter()
            .find(|member| member.user_id == Some(user_id))
            .map(|member| member.name.clone())
            .unwrap_or_else(|| user_id.get().to_string());

        // Roles open a picker instead of acting immediately: the change is a
        // set, not a single decision.
        if action == ModerationAction::ManageRoles {
            self.open_role_picker(user_id);
            return;
        }

        handle.send(match action {
            // Handled above; the picker sends its own command on save.
            ModerationAction::ManageRoles => return,
            ModerationAction::Kick => AppCommand::KickMember {
                guild_id,
                user_id,
                label,
            },
            ModerationAction::Ban => AppCommand::BanMember {
                guild_id,
                user_id,
                // Nothing is purged by default: deleting someone's history is
                // a separate decision from removing them.
                delete_message_seconds: 0,
                label,
            },
            ModerationAction::Timeout => AppCommand::TimeoutMember {
                guild_id,
                user_id,
                minutes: Some(10),
                label,
            },
            ModerationAction::ClearTimeout => AppCommand::TimeoutMember {
                guild_id,
                user_id,
                minutes: None,
                label,
            },
        });
    }





    /// Open a forum channel, which lists posts rather than messages.
    pub fn open_forum(&mut self, channel_id: Id<marker::ChannelMarker>, name: String) {
        self.forum = Some(ForumView::loading(channel_id, name));
        self.messages.clear();
        self.request_forum_posts(0);
    }

    /// Request a page of posts for the open forum.
    pub fn request_forum_posts(&mut self, offset: usize) {
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
    pub fn toggle_forum_archived(&mut self) {
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
    pub fn open_forum_post(&mut self, index: usize) {
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
    pub fn in_thread(&self) -> bool {
        let Some(channel_id) = self.nav.channel else {
            return false;
        };
        if let Some(state) = &self.last_state
            && let Some(c) = state.channel(channel_id)
        {
            return c.is_thread();
        }
        self.model
            .channels
            .iter()
            .find(|c| c.id == Some(channel_id))
            .is_some_and(|c| c.kind == ChannelKind::Thread)
    }

    /// A DM or group DM with no call already running can be called.
    pub fn can_call(&self) -> bool {
        matches!(self.nav.selection, Selection::DirectMessages)
            && self.nav.channel.is_some()
            && self.voice_channel.is_none()
    }

    /// The member pane only applies to guild channels.
    pub fn shows_members(&self) -> bool {
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
    pub fn open_mention(&mut self, index: usize) {
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

        // The surrounding conversation, so the mention has context rather than
        // arriving as one isolated line.
        if let Some(handle) = &self.handle {
            self.inbox_history_request = self.inbox_history_request.wrapping_add(1);
            handle.send(AppCommand::LoadInboxChannelHistory {
                channel_id,
                request_id: self.inbox_history_request,
            });
        }
    }

    /// Dismiss a mention without visiting it.
    pub fn dismiss_mention(&mut self, index: usize) {
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
    pub fn vote_poll(&mut self, index: usize, answer_id: u8) {
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

    /// Open a link from a message in the system browser.
    ///
    /// Routed through the core's OpenUrl rather than launched here, so the
    /// same URL policy applies in both clients - the core normalises and
    /// rejects schemes that should not be handed to a browser.
    pub fn open_link(&mut self, index: usize, link: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(url) = self
            .messages
            .get(index)
            .and_then(|row| row.links.get(link))
            .cloned()
        else {
            return;
        };

        handle.send(AppCommand::OpenUrl { url });
    }

    /// Strip embeds from a message.
    ///
    /// Useful when a link unfurls into something large or unwanted; the
    /// message text stays, only the preview goes.
    pub fn remove_embeds(&mut self, index: usize) {
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
}
