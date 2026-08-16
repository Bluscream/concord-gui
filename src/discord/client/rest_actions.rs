use std::time::Duration;

use chrono::{DateTime, Utc};

use super::DiscordClient;
use crate::discord::{
    ActionBlockReason, ActionDecision, ApplicationCommandInvocation, DiscordAction,
    DiscordPermission, DiscordState, ForumPostCreate, GuildFolder, MESSAGE_FLAG_SUPPRESS_EMBEDS,
    MessageAttachmentUpload, MessageInfo, MessageSendLimits, MessageState, PermissionDecision,
    ReactionEmoji, ReactionUsersPage, ReplyReference, UserProfileInfo, UserProfileUpdate,
    commands::ForumPostArchiveState,
    ids::{
        Id,
        marker::{ChannelMarker, ForumTagMarker, GuildMarker, MessageMarker, UserMarker},
    },
    rest::{CreatedForumPost, ForumPostPage, MessageCreateRequest, MessageEditRequest},
};
use crate::{AppError, Result};

impl DiscordClient {
    pub async fn validate_token_authentication(&self) -> Result<()> {
        self.rest.validate_token_authentication().await
    }

    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
        content: &str,
        reply_to: Option<ReplyReference>,
        attachments: &[MessageAttachmentUpload],
        sticker_ids: &[Id<crate::discord::ids::marker::StickerMarker>],
    ) -> Result<MessageInfo> {
        self.ensure_can_send_message(channel_id, reply_to.as_ref(), attachments)?;
        let limits = self.message_send_limits(channel_id);
        let slow_mode = self.message_slow_mode(channel_id);
        self.rest
            .send_message(
                channel_id,
                MessageCreateRequest {
                    nonce,
                    content,
                    reply_to,
                    attachments,
                    sticker_ids,
                },
                limits,
                slow_mode,
            )
            .await
    }

    pub async fn send_tts_message(
        &self,
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
        content: &str,
    ) -> Result<MessageInfo> {
        self.ensure_can_send_tts_message(channel_id)?;
        let limits = self.message_send_limits(channel_id);
        let slow_mode = self.message_slow_mode(channel_id);
        self.rest
            .send_tts_message(
                channel_id,
                nonce,
                content,
                limits.max_content_chars,
                slow_mode,
            )
            .await
    }

    pub fn trigger_typing(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::ShowTypingIndicator)?;
        self.rest.spawn_typing(channel_id);
        Ok(())
    }

    pub async fn create_forum_post(&self, post: &ForumPostCreate) -> Result<CreatedForumPost> {
        self.ensure_can_create_forum_post(post)?;
        let limits = self.message_send_limits(post.channel_id);
        let slow_mode = self.message_slow_mode(post.channel_id);
        self.rest.create_forum_post(post, limits, slow_mode).await
    }

    /// Effective message and attachment limits for `channel_id`, resolved from
    /// account entitlements and guild capabilities in one state snapshot.
    fn message_send_limits(&self, channel_id: Id<ChannelMarker>) -> MessageSendLimits {
        self.read_state().message_send_limits(channel_id)
    }

    pub(crate) fn message_slow_mode(&self, channel_id: Id<ChannelMarker>) -> Option<Duration> {
        let state = self.read_state();
        let channel = state.channel(channel_id)?;
        let seconds = channel.rate_limit_per_user.filter(|seconds| *seconds > 0)?;
        (!state.bypasses_slow_mode(channel)).then(|| Duration::from_secs(seconds))
    }

    pub(super) fn ensure_can_send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        reply_to: Option<&ReplyReference>,
        attachments: &[MessageAttachmentUpload],
    ) -> Result<()> {
        let state = self.read_state();
        let channel = state.channel(channel_id).ok_or_else(|| {
            action_blocked(
                DiscordAction::SendMessage,
                ActionBlockReason::ChannelDataUnavailable,
            )
        })?;
        if let Some(reason) = state
            .message_send_decision(channel, reply_to.is_some(), !attachments.is_empty())
            .block_reason()
        {
            return Err(action_blocked(DiscordAction::SendMessage, reason));
        }
        Ok(())
    }

    pub(super) fn ensure_can_create_forum_post(&self, post: &ForumPostCreate) -> Result<()> {
        let state = self.read_state();
        let channel = state.channel(post.channel_id).ok_or_else(|| {
            action_blocked(
                DiscordAction::CreateForumPost,
                ActionBlockReason::ChannelDataUnavailable,
            )
        })?;
        if !channel.is_forum() {
            return Err(AppError::DiscordRequest(
                "cannot create forum post outside a forum channel".to_owned(),
            ));
        }
        if let Some(reason) = state
            .forum_post_decision(channel, !post.attachments.is_empty())
            .block_reason()
        {
            return Err(action_blocked(DiscordAction::CreateForumPost, reason));
        }
        if channel.requires_forum_tag() && post.applied_tags.is_empty() {
            return Err(AppError::DiscordRequest(
                "forum post requires a tag".to_owned(),
            ));
        }
        if !post.applied_tags.is_empty()
            && post
                .applied_tags
                .iter()
                .any(|tag_id| !channel.available_tags.iter().any(|tag| tag.id == *tag_id))
        {
            return Err(AppError::DiscordRequest(
                "forum post includes an unknown tag".to_owned(),
            ));
        }
        if post.applied_tags.iter().any(|tag_id| {
            channel
                .available_tags
                .iter()
                .any(|tag| tag.id == *tag_id && tag.moderated)
        }) {
            ensure_permission(
                &state,
                channel,
                DiscordAction::ApplyModeratedForumTag,
                DiscordPermission::ManageThreads,
            )?;
        }
        Ok(())
    }

    pub(super) fn ensure_can_send_tts_message(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        let state = self.read_state();
        let channel = state.channel(channel_id).ok_or_else(|| {
            action_blocked(
                DiscordAction::SendTtsMessage,
                ActionBlockReason::ChannelDataUnavailable,
            )
        })?;
        ensure_channel_action_policy(&state, channel, DiscordAction::SendTtsMessage)
    }

    pub async fn edit_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: &str,
    ) -> Result<MessageInfo> {
        self.ensure_can_edit_message(channel_id, message_id)?;
        let max_content_chars = self.message_send_limits(channel_id).max_content_chars;
        self.rest
            .edit_message(
                channel_id,
                message_id,
                MessageEditRequest::Content(content),
                max_content_chars,
            )
            .await
    }

    pub async fn remove_message_embeds(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<MessageInfo> {
        let flags = {
            let state = self.read_state();
            let channel = state.channel(channel_id).ok_or_else(|| {
                action_blocked(
                    DiscordAction::RemoveMessageEmbeds,
                    ActionBlockReason::ChannelDataUnavailable,
                )
            })?;
            ensure_channel_action_policy(&state, channel, DiscordAction::RemoveMessageEmbeds)?;
            let message = state
                .messages_for_channel(channel_id)
                .into_iter()
                .find(|message| message.id == message_id)
                .ok_or_else(|| {
                    AppError::DiscordRequest(format!(
                        "message {} was not found in channel {}",
                        message_id.get(),
                        channel_id.get()
                    ))
                })?;
            let is_author = Some(message.author_id) == state.current_user_id();
            if !is_author {
                ensure_permission(
                    &state,
                    channel,
                    DiscordAction::RemoveMessageEmbeds,
                    DiscordPermission::ManageMessages,
                )?;
            }
            message.flags | MESSAGE_FLAG_SUPPRESS_EMBEDS
        };
        let max_content_chars = self.message_send_limits(channel_id).max_content_chars;
        self.rest
            .edit_message(
                channel_id,
                message_id,
                MessageEditRequest::Flags(flags),
                max_content_chars,
            )
            .await
    }

    pub async fn delete_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        self.ensure_can_delete_message(channel_id, message_id)?;
        self.rest.delete_message(channel_id, message_id).await
    }

    pub async fn leave_guild(&self, guild_id: Id<GuildMarker>) -> Result<()> {
        self.rest.leave_guild(guild_id).await
    }

    pub async fn forward_message(
        &self,
        target_channel_id: Id<ChannelMarker>,
        source_channel_id: Id<ChannelMarker>,
        source_guild_id: Option<Id<GuildMarker>>,
        message_id: Id<MessageMarker>,
        nonce: Id<MessageMarker>,
    ) -> Result<crate::discord::MessageInfo> {
        self.rest
            .forward_message(
                target_channel_id,
                source_channel_id,
                source_guild_id,
                message_id,
                nonce,
            )
            .await
    }

    pub async fn kick_member(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Result<()> {
        self.rest.kick_member(guild_id, user_id).await
    }

    pub async fn ban_member(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        delete_message_seconds: u32,
    ) -> Result<()> {
        self.rest
            .ban_member(guild_id, user_id, delete_message_seconds)
            .await
    }

    pub async fn guild_bans(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Result<Vec<crate::discord::GuildBanInfo>> {
        self.rest.guild_bans(guild_id).await
    }

    pub async fn unban_member(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Result<()> {
        self.rest.unban_member(guild_id, user_id).await
    }

    pub async fn connections(&self) -> Result<Vec<crate::discord::Connection>> {
        self.rest.connections().await
    }

    pub async fn modify_connection(
        &self,
        kind: &str,
        id: &str,
        visibility: crate::discord::ConnectionVisibility,
        show_activity: bool,
    ) -> Result<()> {
        self.rest
            .modify_connection(kind, id, visibility, show_activity)
            .await
    }

    pub async fn auth_sessions(&self) -> Result<Vec<crate::discord::AuthSession>> {
        // The current session is not identified here: the gateway's session id
        // is a different value from the hash this endpoint keys on, and
        // guessing a match would mark the wrong row as "this session".
        self.rest.auth_sessions(None).await
    }

    pub async fn revoke_auth_sessions(
        &self,
        id_hashes: &[String],
        password: &crate::discord::Secret,
    ) -> Result<()> {
        self.rest
            .revoke_auth_sessions(id_hashes, password.expose())
            .await
    }

    pub async fn authorised_apps(&self) -> Result<Vec<crate::discord::AuthorisedApp>> {
        self.rest.authorised_apps().await
    }

    pub async fn revoke_authorised_app(&self, id: &str) -> Result<()> {
        self.rest.revoke_authorised_app(id).await
    }

    pub async fn modify_privacy_settings(&self, edit: &crate::discord::PrivacyEdit) -> Result<()> {
        self.rest.modify_privacy_settings(edit).await
    }

    pub async fn delete_connection(&self, kind: &str, id: &str) -> Result<()> {
        self.rest.delete_connection(kind, id).await
    }

    pub async fn automod_rules(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Result<Vec<crate::discord::AutoModRule>> {
        self.rest.automod_rules(guild_id).await
    }

    pub async fn set_automod_rule_enabled(
        &self,
        guild_id: Id<GuildMarker>,
        rule_id: u64,
        enabled: bool,
    ) -> Result<()> {
        self.rest
            .set_automod_rule_enabled(guild_id, rule_id, enabled)
            .await
    }

    pub async fn delete_automod_rule(&self, guild_id: Id<GuildMarker>, rule_id: u64) -> Result<()> {
        self.rest.delete_automod_rule(guild_id, rule_id).await
    }

    pub async fn modify_guild(
        &self,
        guild_id: Id<GuildMarker>,
        edit: &crate::discord::GuildEdit,
    ) -> Result<()> {
        self.rest.modify_guild(guild_id, edit).await
    }

    pub async fn set_guild_icon(
        &self,
        guild_id: Id<GuildMarker>,
        image: &crate::discord::ProfileAvatarUpload,
    ) -> Result<()> {
        self.rest.set_guild_icon(guild_id, image).await
    }

    pub async fn create_role(&self, guild_id: Id<GuildMarker>, name: &str) -> Result<()> {
        self.rest.create_role(guild_id, name).await
    }

    pub async fn modify_role(
        &self,
        guild_id: Id<GuildMarker>,
        role_id: Id<crate::discord::ids::marker::RoleMarker>,
        edit: &crate::discord::RoleEdit,
    ) -> Result<()> {
        self.rest.modify_role(guild_id, role_id, edit).await
    }

    pub async fn delete_role(
        &self,
        guild_id: Id<GuildMarker>,
        role_id: Id<crate::discord::ids::marker::RoleMarker>,
    ) -> Result<()> {
        self.rest.delete_role(guild_id, role_id).await
    }

    pub async fn reorder_roles(
        &self,
        guild_id: Id<GuildMarker>,
        positions: &[(Id<crate::discord::ids::marker::RoleMarker>, u32)],
    ) -> Result<()> {
        self.rest.reorder_roles(guild_id, positions).await
    }

    pub async fn create_guild_channel(
        &self,
        guild_id: Id<GuildMarker>,
        name: &str,
        kind: crate::discord::NewChannelKind,
        parent_id: Option<Id<crate::discord::ids::marker::ChannelMarker>>,
    ) -> Result<()> {
        self.rest
            .create_guild_channel(guild_id, name, kind, parent_id)
            .await
    }

    pub async fn modify_channel(
        &self,
        channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
        edit: &crate::discord::ChannelEdit,
    ) -> Result<()> {
        self.rest.modify_channel(channel_id, edit).await
    }

    pub async fn delete_channel(
        &self,
        channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
    ) -> Result<()> {
        self.rest.delete_channel(channel_id).await
    }

    pub async fn reorder_channels(
        &self,
        guild_id: Id<GuildMarker>,
        positions: &[(Id<crate::discord::ids::marker::ChannelMarker>, u32)],
    ) -> Result<()> {
        self.rest.reorder_channels(guild_id, positions).await
    }

    pub async fn set_channel_overwrite(
        &self,
        channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
        target: crate::discord::OverwriteTarget,
        allow: u64,
        deny: u64,
    ) -> Result<()> {
        self.rest
            .set_channel_overwrite(channel_id, target, allow, deny)
            .await
    }

    pub async fn delete_channel_overwrite(
        &self,
        channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
        target: crate::discord::OverwriteTarget,
    ) -> Result<()> {
        self.rest.delete_channel_overwrite(channel_id, target).await
    }

    pub async fn set_voice_channel_status(
        &self,
        channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
        status: Option<&str>,
    ) -> Result<()> {
        self.rest.set_voice_channel_status(channel_id, status).await
    }

    pub async fn default_soundboard_sounds(&self) -> Result<Vec<crate::discord::SoundboardSound>> {
        self.rest.default_soundboard_sounds().await
    }

    pub async fn guild_soundboard_sounds(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Result<Vec<crate::discord::SoundboardSound>> {
        self.rest.guild_soundboard_sounds(guild_id).await
    }

    pub async fn send_soundboard_sound(
        &self,
        channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
        sound_id: u64,
        source_guild_id: Option<Id<GuildMarker>>,
    ) -> Result<()> {
        self.rest
            .send_soundboard_sound(channel_id, sound_id, source_guild_id)
            .await
    }

    pub async fn rename_soundboard_sound(
        &self,
        guild_id: Id<GuildMarker>,
        sound_id: u64,
        name: &str,
    ) -> Result<()> {
        self.rest
            .rename_soundboard_sound(guild_id, sound_id, name)
            .await
    }

    pub async fn delete_soundboard_sound(
        &self,
        guild_id: Id<GuildMarker>,
        sound_id: u64,
    ) -> Result<()> {
        self.rest.delete_soundboard_sound(guild_id, sound_id).await
    }

    pub async fn guild_invites(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Result<Vec<crate::discord::GuildInviteInfo>> {
        self.rest.guild_invites(guild_id).await
    }

    pub async fn create_channel_invite(
        &self,
        channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
        max_age_seconds: u32,
        max_uses: u32,
        temporary: bool,
    ) -> Result<String> {
        self.rest
            .create_channel_invite(channel_id, max_age_seconds, max_uses, temporary)
            .await
    }

    pub async fn revoke_invite(&self, code: &str) -> Result<()> {
        self.rest.revoke_invite(code).await
    }

    pub async fn guild_emojis(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Result<Vec<crate::discord::GuildEmojiInfo>> {
        self.rest.guild_emojis(guild_id).await
    }

    pub async fn create_emoji(
        &self,
        guild_id: Id<GuildMarker>,
        name: &str,
        image: &crate::discord::ProfileAvatarUpload,
    ) -> Result<()> {
        self.rest.create_emoji(guild_id, name, image).await
    }

    pub async fn rename_emoji(
        &self,
        guild_id: Id<GuildMarker>,
        emoji_id: Id<crate::discord::ids::marker::EmojiMarker>,
        name: &str,
    ) -> Result<()> {
        self.rest.rename_emoji(guild_id, emoji_id, name).await
    }

    pub async fn delete_emoji(
        &self,
        guild_id: Id<GuildMarker>,
        emoji_id: Id<crate::discord::ids::marker::EmojiMarker>,
    ) -> Result<()> {
        self.rest.delete_emoji(guild_id, emoji_id).await
    }

    pub async fn guild_audit_log(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Result<Vec<crate::discord::AuditLogEntryInfo>> {
        self.rest.guild_audit_log(guild_id).await
    }

    pub async fn send_friend_request(
        &self,
        username: &str,
        discriminator: Option<u16>,
    ) -> Result<()> {
        self.rest.send_friend_request(username, discriminator).await
    }

    pub async fn add_friend(&self, user_id: Id<UserMarker>) -> Result<()> {
        self.rest.add_friend(user_id).await
    }

    pub async fn block_user(&self, user_id: Id<UserMarker>) -> Result<()> {
        self.rest.block_user(user_id).await
    }

    pub async fn remove_relationship(&self, user_id: Id<UserMarker>) -> Result<()> {
        self.rest.remove_relationship(user_id).await
    }

    pub async fn set_member_roles(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        role_ids: &[Id<crate::discord::ids::marker::RoleMarker>],
    ) -> Result<()> {
        self.rest
            .set_member_roles(guild_id, user_id, role_ids)
            .await
    }

    pub async fn timeout_member(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        minutes: Option<u32>,
    ) -> Result<()> {
        self.rest.timeout_member(guild_id, user_id, minutes).await
    }

    pub async fn resolve_invite(&self, code: &str) -> Result<crate::discord::rest::InvitePreview> {
        self.rest.resolve_invite(code).await
    }

    pub async fn accept_invite(&self, code: &str) -> Result<Option<Id<GuildMarker>>> {
        self.rest.accept_invite(code).await
    }

    /// Whether the account is already in a guild.
    ///
    /// Exposed for the invite flow: offering to join a guild you are already
    /// in is worse than offering to open it.
    pub fn is_member_of(&self, guild_id: Id<GuildMarker>) -> bool {
        self.read_state().guild(guild_id).is_some()
    }

    pub async fn ack_channel(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        let (flags, last_viewed) = self.read_state().channel_ack_metadata(channel_id);
        self.rest
            .ack_channel(channel_id, message_id, flags, last_viewed)
            .await
    }

    pub async fn set_guild_muted(
        &self,
        guild_id: Id<GuildMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.rest
            .set_guild_muted(guild_id, muted, mute_end_time, selected_time_window)
            .await
    }

    pub async fn update_guild_folder_settings(
        &self,
        folder_id: u64,
        name: Option<String>,
        color: Option<u32>,
    ) -> Result<Vec<GuildFolder>> {
        let mut folders = self.read_state().guild_folders().to_vec();
        let Some(folder) = folders
            .iter_mut()
            .find(|folder| folder.id == Some(folder_id))
        else {
            return Err(AppError::DiscordRequest(format!(
                "guild folder {folder_id} was not found"
            )));
        };
        folder.name = name;
        folder.color = color;
        self.rest.update_guild_folders(&folders).await?;
        Ok(folders)
    }

    pub async fn set_channel_muted(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.rest
            .set_channel_muted(
                guild_id,
                channel_id,
                muted,
                mute_end_time,
                selected_time_window,
            )
            .await
    }

    pub async fn set_thread_notification_level(
        &self,
        thread_id: Id<ChannelMarker>,
        flags: u64,
    ) -> Result<()> {
        self.rest
            .set_thread_notification_level(thread_id, flags)
            .await
    }

    pub async fn set_thread_muted(
        &self,
        thread_id: Id<ChannelMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.rest
            .set_thread_muted(thread_id, muted, mute_end_time, selected_time_window)
            .await
    }

    pub async fn follow_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_can_change_thread_membership(thread_id, true)?;
        self.rest.follow_thread(thread_id).await
    }

    pub async fn unfollow_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_can_change_thread_membership(thread_id, false)?;
        self.rest.unfollow_thread(thread_id).await
    }

    pub async fn set_thread_archived(
        &self,
        thread_id: Id<ChannelMarker>,
        archived: bool,
    ) -> Result<()> {
        if archived {
            self.ensure_can_manage_thread(thread_id, DiscordAction::ArchiveThread)?;
        } else {
            self.ensure_can_reopen_thread(thread_id)?;
        }
        self.rest.set_thread_archived(thread_id, archived).await
    }

    pub async fn set_thread_locked(
        &self,
        thread_id: Id<ChannelMarker>,
        locked: bool,
    ) -> Result<()> {
        self.ensure_can_manage_thread(thread_id, DiscordAction::ChangeThreadLock)?;
        self.rest.set_thread_locked(thread_id, locked).await
    }

    pub async fn set_thread_pinned(
        &self,
        thread_id: Id<ChannelMarker>,
        pinned: bool,
        current_flags: u64,
    ) -> Result<()> {
        self.ensure_can_manage_thread(thread_id, DiscordAction::PinForumPost)?;
        self.rest
            .set_thread_pinned(thread_id, pinned, current_flags)
            .await
    }

    pub async fn delete_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_can_manage_thread(thread_id, DiscordAction::DeleteThread)?;
        self.rest.delete_thread(thread_id).await
    }

    pub async fn edit_thread_settings(
        &self,
        thread_id: Id<ChannelMarker>,
        name: &str,
        applied_tags: &[Id<ForumTagMarker>],
        rate_limit_per_user: u64,
        auto_archive_duration: u64,
    ) -> Result<()> {
        let can_manage_threads =
            self.ensure_can_edit_thread_settings(thread_id, applied_tags, rate_limit_per_user)?;
        self.rest
            .edit_thread_settings(
                thread_id,
                name,
                applied_tags,
                can_manage_threads.then_some(rate_limit_per_user),
                auto_archive_duration,
            )
            .await
    }

    pub(super) fn ensure_can_edit_thread_settings(
        &self,
        thread_id: Id<ChannelMarker>,
        applied_tags: &[Id<ForumTagMarker>],
        rate_limit_per_user: u64,
    ) -> Result<bool> {
        let state = self.read_state();
        let channel = state.channel(thread_id).ok_or_else(|| {
            action_blocked(
                DiscordAction::EditThread,
                ActionBlockReason::ChannelDataUnavailable,
            )
        })?;
        if !channel.is_thread() {
            return Err(AppError::DiscordRequest(
                "thread editing requires a thread channel".to_owned(),
            ));
        }
        ensure_channel_action_policy(&state, channel, DiscordAction::EditThread)?;

        let manage_threads_decision =
            state.channel_permission_decision(channel, DiscordPermission::ManageThreads);
        let can_manage_threads = matches!(manage_threads_decision, PermissionDecision::Allowed);
        let rate_limit_changed = channel.rate_limit_per_user.unwrap_or(0) != rate_limit_per_user;
        if !can_manage_threads
            && (rate_limit_changed || changed_moderated_thread_tags(&state, channel, applied_tags))
        {
            let reason = match manage_threads_decision {
                PermissionDecision::Allowed => unreachable!("manage permission was not allowed"),
                PermissionDecision::Denied(permission) => {
                    ActionBlockReason::PermissionDenied(permission)
                }
                PermissionDecision::Unavailable(gap) => {
                    ActionBlockReason::PermissionDataUnavailable(gap)
                }
            };
            return Err(action_blocked(DiscordAction::EditThread, reason));
        }
        Ok(can_manage_threads)
    }

    pub async fn ack_channels(
        &self,
        targets: &[(Id<ChannelMarker>, Id<MessageMarker>)],
    ) -> Result<()> {
        self.rest.ack_channels(targets).await
    }

    pub async fn load_message_history(
        &self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest
            .load_message_history(channel_id, before, limit)
            .await
    }

    pub async fn load_message_history_around(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest
            .load_message_history_around(channel_id, message_id, limit)
            .await
    }

    pub async fn load_recent_mentions(
        &self,
        before: Option<Id<MessageMarker>>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.rest.load_recent_mentions(before, limit).await
    }

    pub async fn delete_recent_mention(&self, message_id: Id<MessageMarker>) -> Result<()> {
        self.rest.delete_recent_mention(message_id).await
    }

    pub async fn load_message_history_after(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest
            .load_message_history_after(channel_id, message_id, limit)
            .await
    }

    pub async fn search_messages(
        &self,
        query: crate::discord::MessageSearchQuery,
    ) -> Result<crate::discord::MessageSearchPage> {
        if let Some(channel_id) = query.channel_id {
            self.ensure_can_read_message_history(channel_id)?;
        }
        self.rest.search_messages(query).await
    }

    pub async fn load_forum_posts(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    ) -> Result<ForumPostPage> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest
            .load_forum_posts(guild_id, channel_id, archive_state, offset)
            .await
    }

    pub async fn add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.ensure_can_add_reaction(channel_id, message_id, emoji)?;
        self.rest.add_reaction(channel_id, message_id, emoji).await
    }

    pub async fn remove_current_user_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.ensure_can_remove_current_user_reaction(channel_id)?;
        self.rest
            .remove_current_user_reaction(channel_id, message_id, emoji)
            .await
    }

    pub async fn load_reaction_users_page(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
        after: Option<Id<UserMarker>>,
    ) -> Result<ReactionUsersPage> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest
            .load_reaction_users_page(channel_id, message_id, emoji, after)
            .await
    }

    pub async fn load_pinned_messages(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<Vec<MessageInfo>> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest.load_pinned_messages(channel_id).await
    }

    pub async fn set_message_pinned(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) -> Result<()> {
        self.ensure_can_pin_message(channel_id)?;
        self.rest
            .set_message_pinned(channel_id, message_id, pinned)
            .await
    }

    pub async fn vote_poll(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: &[u8],
    ) -> Result<()> {
        self.ensure_can_vote_poll(channel_id)?;
        self.rest
            .vote_poll(channel_id, message_id, answer_ids)
            .await
    }

    pub async fn load_user_profile(
        &self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
        is_self: bool,
    ) -> Result<UserProfileInfo> {
        self.rest
            .load_user_profile(user_id, guild_id, is_self)
            .await
    }

    pub async fn load_user_note(&self, user_id: Id<UserMarker>) -> Result<Option<String>> {
        self.rest.load_user_note(user_id).await
    }

    pub async fn update_user_profile(&self, update: &UserProfileUpdate) -> Result<()> {
        self.rest.update_user_profile(update).await
    }

    pub(super) fn ensure_can_run_application_command(
        &self,
        invocation: &ApplicationCommandInvocation,
    ) -> Result<()> {
        self.ensure_channel_action(invocation.channel_id, DiscordAction::RunApplicationCommand)
    }

    pub(super) fn ensure_can_request_application_command_autocomplete(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::RunApplicationCommand)
    }

    pub(super) fn ensure_can_remove_current_user_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::RemoveReaction)
    }

    pub(super) fn ensure_can_pin_message(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::PinMessage)
    }

    pub(super) fn ensure_can_vote_poll(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::VotePoll)
    }

    pub(super) fn ensure_can_manage_thread(
        &self,
        thread_id: Id<ChannelMarker>,
        action: DiscordAction,
    ) -> Result<()> {
        let state = self.read_state();
        let channel = state
            .channel(thread_id)
            .ok_or_else(|| action_blocked(action, ActionBlockReason::ChannelDataUnavailable))?;
        if !channel.is_thread() {
            return Err(AppError::DiscordRequest(
                "thread management requires a thread channel".to_owned(),
            ));
        }
        ensure_channel_action_policy(&state, channel, action)
    }

    pub(super) fn ensure_can_change_thread_membership(
        &self,
        thread_id: Id<ChannelMarker>,
        joining: bool,
    ) -> Result<()> {
        let state = self.read_state();
        let channel = state.channel(thread_id).ok_or_else(|| {
            AppError::DiscordRequest("cannot verify thread membership permissions".to_owned())
        })?;
        if !channel.is_thread() {
            return Err(AppError::DiscordRequest(
                "thread membership can only change for a thread".to_owned(),
            ));
        }
        ensure_channel_action_policy(&state, channel, DiscordAction::ChangeThreadMembership)?;
        if channel.thread_archived().unwrap_or(false) {
            return Err(AppError::DiscordRequest(
                "thread membership cannot change while the thread is archived".to_owned(),
            ));
        }
        if joining {
            ensure_permission(
                &state,
                channel,
                DiscordAction::ChangeThreadMembership,
                DiscordPermission::ViewChannel,
            )?;
        }
        Ok(())
    }

    pub(super) fn ensure_can_reopen_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        let state = self.read_state();
        let channel = state.channel(thread_id).ok_or_else(|| {
            AppError::DiscordRequest("cannot verify thread reopen permissions".to_owned())
        })?;
        if !channel.is_thread() {
            return Err(AppError::DiscordRequest(
                "thread reopen requires a thread channel".to_owned(),
            ));
        }
        ensure_channel_action_policy(&state, channel, DiscordAction::ReopenThread)
    }

    pub(super) fn ensure_can_read_message_history(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::ReadMessageHistory)
    }

    pub(super) fn ensure_can_edit_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        let state = self.read_state();
        let channel = state.channel(channel_id).ok_or_else(|| {
            AppError::DiscordRequest("cannot verify permission to edit this message".to_owned())
        })?;
        ensure_channel_action_policy(&state, channel, DiscordAction::EditMessage)?;
        let message = cached_message(&state, channel_id, message_id)?;
        if Some(message.author_id) == state.current_user_id() {
            Ok(())
        } else {
            Err(AppError::DiscordRequest(
                "only the message author can edit this message".to_owned(),
            ))
        }
    }

    pub(super) fn ensure_can_delete_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        let state = self.read_state();
        let channel = state.channel(channel_id).ok_or_else(|| {
            AppError::DiscordRequest("cannot verify permission to delete this message".to_owned())
        })?;
        ensure_channel_action_policy(&state, channel, DiscordAction::DeleteMessage)?;
        let message = cached_message(&state, channel_id, message_id)?;
        if Some(message.author_id) == state.current_user_id() {
            return Ok(());
        }
        ensure_permission(
            &state,
            channel,
            DiscordAction::DeleteMessage,
            DiscordPermission::ManageMessages,
        )
    }

    pub(super) fn ensure_can_add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        let state = self.read_state();
        let channel = state.channel(channel_id).ok_or_else(|| {
            AppError::DiscordRequest("cannot verify reaction channel permissions".to_owned())
        })?;
        ensure_channel_action_policy(&state, channel, DiscordAction::AddReaction)?;
        ensure_permission(
            &state,
            channel,
            DiscordAction::AddReaction,
            DiscordPermission::ReadMessageHistory,
        )?;
        if channel.is_thread() && channel.thread_archived().unwrap_or(false) {
            return Err(AppError::DiscordRequest(
                "cannot add reactions while the thread is archived".to_owned(),
            ));
        }
        let reaction_exists = state
            .messages_for_channel(channel_id)
            .into_iter()
            .find(|message| message.id == message_id)
            .is_some_and(|message| {
                message
                    .reactions
                    .iter()
                    .any(|reaction| reaction.emoji == *emoji)
            });
        if !reaction_exists {
            ensure_permission(
                &state,
                channel,
                DiscordAction::AddReaction,
                DiscordPermission::AddReactions,
            )?;
        }
        if state.reaction_emoji_requires_external_permission(channel, emoji) {
            ensure_permission(
                &state,
                channel,
                DiscordAction::AddReaction,
                DiscordPermission::UseExternalEmojis,
            )?;
        }
        Ok(())
    }

    fn ensure_channel_action(
        &self,
        channel_id: Id<ChannelMarker>,
        action: DiscordAction,
    ) -> Result<()> {
        let state = self.read_state();
        let channel = state
            .channel(channel_id)
            .ok_or_else(|| action_blocked(action, ActionBlockReason::ChannelDataUnavailable))?;
        ensure_channel_action_policy(&state, channel, action)
    }
}

fn changed_moderated_thread_tags(
    state: &crate::discord::state::DiscordState,
    thread: &crate::discord::ChannelState,
    applied_tags: &[Id<ForumTagMarker>],
) -> bool {
    let Some(parent) = thread
        .parent_id
        .and_then(|parent_id| state.channel(parent_id))
    else {
        return false;
    };
    parent.available_tags.iter().any(|tag| {
        tag.moderated && (thread.applied_tags.contains(&tag.id) != applied_tags.contains(&tag.id))
    })
}

pub(super) fn ensure_channel_action_policy(
    state: &crate::discord::state::DiscordState,
    channel: &crate::discord::ChannelState,
    action: DiscordAction,
) -> Result<()> {
    match state.channel_action_decision(channel, action) {
        ActionDecision::Allowed => Ok(()),
        ActionDecision::Blocked(reason) => Err(action_blocked(action, reason)),
    }
}

pub(super) fn ensure_permission(
    state: &crate::discord::state::DiscordState,
    channel: &crate::discord::ChannelState,
    action: DiscordAction,
    permission: DiscordPermission,
) -> Result<()> {
    let reason = match state.channel_permission_decision(channel, permission) {
        PermissionDecision::Allowed => return Ok(()),
        PermissionDecision::Denied(permission) => ActionBlockReason::PermissionDenied(permission),
        PermissionDecision::Unavailable(gap) => ActionBlockReason::PermissionDataUnavailable(gap),
    };
    Err(action_blocked(action, reason))
}

fn action_blocked(action: DiscordAction, reason: ActionBlockReason) -> AppError {
    AppError::DiscordActionBlocked { action, reason }
}

fn cached_message(
    state: &DiscordState,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) -> Result<&MessageState> {
    state
        .messages_for_channel(channel_id)
        .into_iter()
        .find(|message| message.id == message_id)
        .ok_or_else(|| {
            AppError::DiscordRequest(format!(
                "message {} was not found in channel {}",
                message_id.get(),
                channel_id.get()
            ))
        })
}
