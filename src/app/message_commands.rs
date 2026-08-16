use std::collections::BTreeMap;

use crate::{
    AppError, DiscordClient,
    discord::{
        AppEvent, ApplicationCommandAutocompleteInvocation, ApplicationCommandInvocation,
        AttachmentUpdate, ForumPostCreate, MessageAttachmentUpload, MessageInfo,
        MessageUpdateDispatchInfo, MessageUpdateEventFields, ReactionEmoji, ReplyReference,
        friend_request_target,
        ids::{
            Id,
            marker::{
                ChannelMarker, EmojiMarker, ForumTagMarker, GuildMarker, MessageMarker, RoleMarker,
                StickerMarker, UserMarker,
            },
        },
    },
};

use super::command_loop::{log_app_error, publish_app_error};

pub(super) async fn send_message(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    nonce: Id<MessageMarker>,
    content: String,
    reply_to: Option<ReplyReference>,
    attachments: Vec<MessageAttachmentUpload>,
    sticker_ids: Vec<Id<StickerMarker>>,
) {
    match client
        .send_message(
            channel_id,
            nonce,
            &content,
            reply_to,
            &attachments,
            &sticker_ids,
        )
        .await
    {
        Ok(mut message) => {
            message.nonce = Some(nonce);
            client.publish_event(message_create_event(message)).await;
        }
        Err(error) => {
            client
                .publish_event(AppEvent::MessageSendFailed { channel_id, nonce })
                .await;
            publish_message_send_error(&client, channel_id, "send message failed", &error).await
        }
    }
}

pub(super) async fn trigger_typing(client: DiscordClient, channel_id: Id<ChannelMarker>) {
    if let Err(error) = client.trigger_typing(channel_id) {
        publish_app_error(&client, "show typing indicator failed", &error).await;
    }
}

pub(super) async fn send_tts_message(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    nonce: Id<MessageMarker>,
    content: String,
) {
    match client.send_tts_message(channel_id, nonce, &content).await {
        Ok(mut message) => {
            message.nonce = Some(nonce);
            client.publish_event(message_create_event(message)).await;
        }
        Err(error) => {
            client
                .publish_event(AppEvent::MessageSendFailed { channel_id, nonce })
                .await;
            publish_message_send_error(&client, channel_id, "send tts message failed", &error).await
        }
    }
}

pub(super) async fn create_forum_post(client: DiscordClient, post: ForumPostCreate) {
    match client.create_forum_post(&post).await {
        Ok(created) => {
            let slow_mode = client.message_slow_mode(post.channel_id);
            client
                .publish_event(AppEvent::ChannelUpsert(created.thread))
                .await;
            if let Some(message) = created.first_message {
                client.publish_event(message_create_event(message)).await;
            }
            if let Some(slow_mode) = slow_mode {
                client
                    .publish_event(AppEvent::MessageSendCooldownStarted {
                        channel_id: post.channel_id,
                        duration_millis: u64::try_from(slow_mode.as_millis()).unwrap_or(u64::MAX),
                    })
                    .await;
            }
        }
        Err(error) => {
            publish_message_send_error(&client, post.channel_id, "create forum post failed", &error)
                .await
        }
    }
}

/// The archive/lock/pin/delete results arrive over the gateway
/// (THREAD_UPDATE / THREAD_DELETE), which updates the cached thread, so
/// these only need to report failures.
pub(super) async fn set_thread_archived(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    archived: bool,
    _label: String,
) {
    if let Err(error) = client.set_thread_archived(channel_id, archived).await {
        let context = if archived {
            "archive thread failed"
        } else {
            "reopen thread failed"
        };
        publish_app_error(&client, context, &error).await;
    }
}

pub(super) async fn set_thread_locked(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    locked: bool,
    _label: String,
) {
    if let Err(error) = client.set_thread_locked(channel_id, locked).await {
        let context = if locked {
            "lock thread failed"
        } else {
            "unlock thread failed"
        };
        publish_app_error(&client, context, &error).await;
    }
}

pub(super) async fn set_thread_pinned(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    pinned: bool,
    current_flags: u64,
    _label: String,
) {
    if let Err(error) = client
        .set_thread_pinned(channel_id, pinned, current_flags)
        .await
    {
        let context = if pinned {
            "pin post failed"
        } else {
            "unpin post failed"
        };
        publish_app_error(&client, context, &error).await;
    }
}

pub(super) async fn delete_thread(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    _label: String,
) {
    if let Err(error) = client.delete_thread(channel_id).await {
        publish_app_error(&client, "delete thread failed", &error).await;
    }
}

pub(super) async fn edit_thread(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    name: String,
    applied_tags: Vec<Id<ForumTagMarker>>,
    rate_limit_per_user: u64,
    auto_archive_duration: u64,
    _label: String,
) {
    if let Err(error) = client
        .edit_thread_settings(
            channel_id,
            &name,
            &applied_tags,
            rate_limit_per_user,
            auto_archive_duration,
        )
        .await
    {
        publish_app_error(&client, "edit thread failed", &error).await;
    }
}

pub(super) async fn load_application_commands(
    client: DiscordClient,
    guild_id: Option<Id<GuildMarker>>,
) {
    match client.load_application_commands(guild_id).await {
        Ok(Some(commands)) => {
            client
                .publish_event(AppEvent::ApplicationCommandsLoaded { guild_id, commands })
                .await;
        }
        Ok(None) => {}
        Err(error) => log_app_error("load application commands failed", &error),
    }
}

pub(super) async fn run_application_command(
    client: DiscordClient,
    invocation: ApplicationCommandInvocation,
) {
    if let Err(error) = client.run_application_command(&invocation).await {
        publish_app_error(&client, "run application command failed", &error).await;
    }
}

pub(super) async fn request_application_command_autocomplete(
    client: DiscordClient,
    invocation: ApplicationCommandAutocompleteInvocation,
) {
    if let Err(error) = client
        .request_application_command_autocomplete(&invocation)
        .await
    {
        log_app_error("application command autocomplete failed", &error);
    }
}

pub(super) async fn edit_message(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    content: String,
) {
    match client.edit_message(channel_id, message_id, &content).await {
        Ok(message) => {
            client.publish_event(message_update_event(message)).await;
        }
        Err(error) => publish_app_error(&client, "edit message failed", &error).await,
    }
}

pub(super) async fn delete_message(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) {
    match client.delete_message(channel_id, message_id).await {
        Ok(()) => {
            client
                .publish_event(AppEvent::MessageDelete {
                    guild_id: None,
                    channel_id,
                    message_id,
                })
                .await;
        }
        Err(error) => publish_app_error(&client, "delete message failed", &error).await,
    }
}

pub(super) async fn remove_message_embeds(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) {
    match client.remove_message_embeds(channel_id, message_id).await {
        Ok(message) => {
            client.publish_event(message_update_event(message)).await;
        }
        Err(error) => publish_app_error(&client, "remove message embeds failed", &error).await,
    }
}

pub(super) async fn leave_guild(client: DiscordClient, guild_id: Id<GuildMarker>, label: String) {
    match client.leave_guild(guild_id).await {
        Ok(()) => {
            client
                .publish_event(AppEvent::GuildDelete { guild_id })
                .await;
        }
        Err(error) => {
            log_app_error("leave guild failed", &error);
            client
                .publish_event(AppEvent::GatewayError {
                    message: format!("leave server {label} failed: {error}"),
                })
                .await;
        }
    }
}

/// Forward a message into another channel.
pub(super) async fn forward_message(
    client: DiscordClient,
    source_channel_id: Id<ChannelMarker>,
    source_guild_id: Option<Id<GuildMarker>>,
    message_id: Id<MessageMarker>,
    target_channel_id: Id<ChannelMarker>,
    nonce: Id<MessageMarker>,
) {
    match client
        .forward_message(
            target_channel_id,
            source_channel_id,
            source_guild_id,
            message_id,
            nonce,
        )
        .await
    {
        // The forwarded message arrives over the gateway like any other send,
        // so there is nothing to publish on success.
        Ok(_) => {}
        Err(error) => {
            log_app_error("forward message failed", &error);
            client
                .publish_event(AppEvent::MessageSendFailed {
                    channel_id: target_channel_id,
                    nonce,
                })
                .await;
        }
    }
}

/// Report a moderation action that the server refused.
///
/// Discord rejects these for reasons the client cannot always predict - role
/// hierarchy changing underneath, a permission revoked mid-session - so the
/// failure is surfaced rather than swallowed.
async fn report_moderation_failure(
    client: &DiscordClient,
    action: &str,
    label: &str,
    error: &AppError,
) {
    log_app_error(&format!("{action} failed"), error);
    client
        .publish_event(AppEvent::GatewayError {
            message: format!("{action} {label} failed: {error}"),
        })
        .await;
}

pub(super) async fn kick_member(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    label: String,
) {
    if let Err(error) = client.kick_member(guild_id, user_id).await {
        report_moderation_failure(&client, "kick", &label, &error).await;
    }
}

pub(super) async fn ban_member(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    delete_message_seconds: u32,
    label: String,
) {
    if let Err(error) = client
        .ban_member(guild_id, user_id, delete_message_seconds)
        .await
    {
        report_moderation_failure(&client, "ban", &label, &error).await;
    }
}

pub(super) async fn load_guild_bans(client: DiscordClient, guild_id: Id<GuildMarker>) {
    match client.guild_bans(guild_id).await {
        Ok(bans) => {
            client
                .publish_event(AppEvent::GuildBansLoaded { guild_id, bans })
                .await;
        }
        Err(error) => {
            log_app_error("load guild bans failed", &error);
            client
                .publish_event(AppEvent::GuildBansLoadFailed {
                    guild_id,
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn unban_member(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    label: String,
) {
    if let Err(error) = client.unban_member(guild_id, user_id).await {
        report_moderation_failure(&client, "unban", &label, &error).await;
    }
}

/// Ask to be someone's friend.
///
/// A name that cannot be parsed is refused here rather than sent: a rejected
/// request costs a round trip and counts towards exactly the anti-spam
/// heuristics this client tries not to trip.
/// Load something a server-management panel asked for.
///
/// One shape for all of them: fetch, publish the result, and publish the
/// failure rather than swallowing it - a panel that stays "loading" forever
/// tells the user nothing about why.
macro_rules! load_guild_panel {
    ($name:ident, $fetch:ident, $loaded:ident, $failed:ident, $field:ident, $what:literal) => {
        pub(super) async fn $name(client: DiscordClient, guild_id: Id<GuildMarker>) {
            match client.$fetch(guild_id).await {
                Ok($field) => {
                    client
                        .publish_event(AppEvent::$loaded { guild_id, $field })
                        .await;
                }
                Err(error) => {
                    log_app_error(concat!("load ", $what, " failed"), &error);
                    client
                        .publish_event(AppEvent::$failed {
                            guild_id,
                            message: error.to_string(),
                        })
                        .await;
                }
            }
        }
    };
}

load_guild_panel!(
    load_automod_rules,
    automod_rules,
    AutoModRulesLoaded,
    AutoModRulesLoadFailed,
    rules,
    "automod rules"
);
load_guild_panel!(
    load_guild_invites,
    guild_invites,
    GuildInvitesLoaded,
    GuildInvitesLoadFailed,
    invites,
    "guild invites"
);
load_guild_panel!(
    load_guild_emojis,
    guild_emojis,
    GuildEmojisLoaded,
    GuildEmojisLoadFailed,
    emojis,
    "guild emojis"
);
load_guild_panel!(
    load_guild_audit_log,
    guild_audit_log,
    GuildAuditLogLoaded,
    GuildAuditLogLoadFailed,
    entries,
    "guild audit log"
);

/// Fetch a sound list. `None` asks for the default sounds.
pub(super) async fn modify_account(
    client: DiscordClient,
    edit: crate::discord::AccountEdit,
    current_password: crate::discord::Secret,
) {
    match client.modify_account(&edit, &current_password).await {
        Ok(()) => client.publish_event(AppEvent::AccountModified).await,
        Err(error) => {
            log_app_error("account change failed", &error);
            client
                .publish_event(AppEvent::AccountModifyFailed {
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn enable_totp(
    client: DiscordClient,
    secret: String,
    code: String,
    password: crate::discord::Secret,
) {
    match client.enable_totp(&secret, &code, &password).await {
        Ok(backup_codes) => {
            client
                .publish_event(AppEvent::TotpEnabled { backup_codes })
                .await;
        }
        Err(error) => {
            log_app_error("enable two-factor failed", &error);
            client
                .publish_event(AppEvent::TotpFailed {
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn disable_totp(client: DiscordClient, code: String) {
    match client.disable_totp(&code).await {
        Ok(()) => client.publish_event(AppEvent::TotpDisabled).await,
        Err(error) => {
            log_app_error("disable two-factor failed", &error);
            client
                .publish_event(AppEvent::TotpFailed {
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn load_backup_codes(
    client: DiscordClient,
    password: crate::discord::Secret,
    regenerate: bool,
) {
    match client.backup_codes(&password, regenerate).await {
        Ok(codes) => {
            client
                .publish_event(AppEvent::BackupCodesLoaded { codes })
                .await;
        }
        Err(error) => {
            log_app_error("backup codes failed", &error);
            client
                .publish_event(AppEvent::BackupCodesFailed {
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn load_auth_sessions(client: DiscordClient) {
    match client.auth_sessions().await {
        Ok(sessions) => {
            client
                .publish_event(AppEvent::AuthSessionsLoaded { sessions })
                .await;
        }
        Err(error) => {
            log_app_error("load sessions failed", &error);
            client
                .publish_event(AppEvent::AuthSessionsLoadFailed {
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn revoke_auth_sessions(
    client: DiscordClient,
    id_hashes: Vec<String>,
    password: crate::discord::Secret,
) {
    if let Err(error) = client.revoke_auth_sessions(&id_hashes, &password).await {
        report_moderation_failure(&client, "logging out", "those sessions", &error).await;
        return;
    }
    // Refetched rather than removed locally: a partial failure would otherwise
    // leave rows on screen for sessions that are gone, or drop rows for ones
    // that are not.
    load_auth_sessions(client).await;
}

pub(super) async fn load_authorised_apps(client: DiscordClient) {
    match client.authorised_apps().await {
        Ok(apps) => {
            client
                .publish_event(AppEvent::AuthorisedAppsLoaded { apps })
                .await;
        }
        Err(error) => {
            log_app_error("load authorised apps failed", &error);
            client
                .publish_event(AppEvent::AuthorisedAppsLoadFailed {
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn revoke_authorised_app(client: DiscordClient, id: String, label: String) {
    if let Err(error) = client.revoke_authorised_app(&id).await {
        report_moderation_failure(&client, "revoking", &label, &error).await;
    }
}

pub(super) async fn load_connections(client: DiscordClient) {
    match client.connections().await {
        Ok(connections) => {
            client
                .publish_event(AppEvent::ConnectionsLoaded { connections })
                .await;
        }
        Err(error) => {
            log_app_error("load connections failed", &error);
            client
                .publish_event(AppEvent::ConnectionsLoadFailed {
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn modify_connection(
    client: DiscordClient,
    kind: String,
    id: String,
    visibility: crate::discord::ConnectionVisibility,
    show_activity: bool,
    label: String,
) {
    if let Err(error) = client
        .modify_connection(&kind, &id, visibility, show_activity)
        .await
    {
        report_moderation_failure(&client, "changing", &label, &error).await;
    }
}

pub(super) async fn modify_privacy_settings(
    client: DiscordClient,
    edit: crate::discord::PrivacyEdit,
) {
    if let Err(error) = client.modify_privacy_settings(&edit).await {
        report_moderation_failure(&client, "changing", "privacy settings", &error).await;
    }
}

pub(super) async fn delete_connection(
    client: DiscordClient,
    kind: String,
    id: String,
    label: String,
) {
    if let Err(error) = client.delete_connection(&kind, &id).await {
        report_moderation_failure(&client, "unlinking", &label, &error).await;
    }
}

pub(super) async fn set_automod_rule_enabled(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    rule_id: u64,
    enabled: bool,
    label: String,
) {
    if let Err(error) = client
        .set_automod_rule_enabled(guild_id, rule_id, enabled)
        .await
    {
        report_moderation_failure(&client, "toggling rule", &label, &error).await;
    }
}

pub(super) async fn delete_automod_rule(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    rule_id: u64,
    label: String,
) {
    if let Err(error) = client.delete_automod_rule(guild_id, rule_id).await {
        report_moderation_failure(&client, "deleting rule", &label, &error).await;
    }
}

pub(super) async fn modify_guild(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    edit: Box<crate::discord::GuildEdit>,
    label: String,
) {
    if let Err(error) = client.modify_guild(guild_id, &edit).await {
        report_moderation_failure(&client, "editing", &label, &error).await;
    }
}

pub(super) async fn set_guild_icon(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    image: Box<crate::discord::ProfileAvatarUpload>,
    label: String,
) {
    if let Err(error) = client.set_guild_icon(guild_id, &image).await {
        report_moderation_failure(&client, "setting the icon for", &label, &error).await;
    }
}

pub(super) async fn create_role(client: DiscordClient, guild_id: Id<GuildMarker>, name: String) {
    if let Err(error) = client.create_role(guild_id, &name).await {
        report_moderation_failure(&client, "creating role", &name, &error).await;
    }
}

pub(super) async fn modify_role(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    role_id: Id<RoleMarker>,
    edit: Box<crate::discord::RoleEdit>,
    label: String,
) {
    if let Err(error) = client.modify_role(guild_id, role_id, &edit).await {
        report_moderation_failure(&client, "editing role", &label, &error).await;
    }
}

pub(super) async fn delete_role(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    role_id: Id<RoleMarker>,
    label: String,
) {
    if let Err(error) = client.delete_role(guild_id, role_id).await {
        report_moderation_failure(&client, "deleting role", &label, &error).await;
    }
}

pub(super) async fn reorder_roles(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    positions: Vec<(Id<RoleMarker>, u32)>,
) {
    if let Err(error) = client.reorder_roles(guild_id, &positions).await {
        report_moderation_failure(&client, "reordering", "roles", &error).await;
    }
}

pub(super) async fn create_guild_channel(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    name: String,
    kind: crate::discord::NewChannelKind,
    parent_id: Option<Id<ChannelMarker>>,
) {
    if let Err(error) = client
        .create_guild_channel(guild_id, &name, kind, parent_id)
        .await
    {
        report_moderation_failure(&client, "creating channel", &name, &error).await;
    }
}

pub(super) async fn modify_channel(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    edit: Box<crate::discord::ChannelEdit>,
    label: String,
) {
    if let Err(error) = client.modify_channel(channel_id, &edit).await {
        report_moderation_failure(&client, "editing channel", &label, &error).await;
    }
}

pub(super) async fn delete_channel(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    label: String,
) {
    if let Err(error) = client.delete_channel(channel_id).await {
        report_moderation_failure(&client, "deleting channel", &label, &error).await;
    }
}

pub(super) async fn reorder_channels(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    positions: Vec<(Id<ChannelMarker>, u32)>,
) {
    if let Err(error) = client.reorder_channels(guild_id, &positions).await {
        report_moderation_failure(&client, "reordering", "channels", &error).await;
    }
}

pub(super) async fn set_channel_overwrite(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    target: crate::discord::OverwriteTarget,
    allow: u64,
    deny: u64,
    label: String,
) {
    if let Err(error) = client
        .set_channel_overwrite(channel_id, target, allow, deny)
        .await
    {
        report_moderation_failure(&client, "setting permissions for", &label, &error).await;
    }
}

pub(super) async fn delete_channel_overwrite(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    target: crate::discord::OverwriteTarget,
    label: String,
) {
    if let Err(error) = client.delete_channel_overwrite(channel_id, target).await {
        report_moderation_failure(&client, "clearing permissions for", &label, &error).await;
    }
}

pub(super) async fn set_voice_channel_status(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    status: Option<String>,
) {
    if let Err(error) = client
        .set_voice_channel_status(channel_id, status.as_deref())
        .await
    {
        report_moderation_failure(&client, "setting", "the channel status", &error).await;
    }
}

pub(super) async fn load_soundboard_sounds(
    client: DiscordClient,
    guild_id: Option<Id<GuildMarker>>,
) {
    let result = match guild_id {
        Some(guild_id) => client.guild_soundboard_sounds(guild_id).await,
        None => client.default_soundboard_sounds().await,
    };

    match result {
        Ok(sounds) => {
            client
                .publish_event(AppEvent::SoundboardSoundsLoaded { guild_id, sounds })
                .await;
        }
        Err(error) => {
            log_app_error("load soundboard sounds failed", &error);
            client
                .publish_event(AppEvent::SoundboardSoundsLoadFailed {
                    guild_id,
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn play_soundboard_sound(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    sound_id: u64,
    source_guild_id: Option<Id<GuildMarker>>,
    label: String,
) {
    if let Err(error) = client
        .send_soundboard_sound(channel_id, sound_id, source_guild_id)
        .await
    {
        report_moderation_failure(&client, "playing", &label, &error).await;
    }
}

pub(super) async fn rename_soundboard_sound(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    sound_id: u64,
    name: String,
) {
    if let Err(error) = client
        .rename_soundboard_sound(guild_id, sound_id, &name)
        .await
    {
        report_moderation_failure(&client, "renaming sound to", &name, &error).await;
    }
}

pub(super) async fn delete_soundboard_sound(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    sound_id: u64,
    label: String,
) {
    if let Err(error) = client.delete_soundboard_sound(guild_id, sound_id).await {
        report_moderation_failure(&client, "deleting sound", &label, &error).await;
    }
}

pub(super) async fn create_channel_invite(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    max_age_seconds: u32,
    max_uses: u32,
    temporary: bool,
) {
    match client
        .create_channel_invite(channel_id, max_age_seconds, max_uses, temporary)
        .await
    {
        Ok(code) => {
            client
                .publish_event(AppEvent::InviteCreated { channel_id, code })
                .await;
        }
        Err(error) => {
            report_moderation_failure(&client, "creating an invite for", "this channel", &error)
                .await;
        }
    }
}

pub(super) async fn revoke_invite(client: DiscordClient, code: String) {
    if let Err(error) = client.revoke_invite(&code).await {
        report_moderation_failure(&client, "revoking invite", &code, &error).await;
    }
}

pub(super) async fn create_emoji(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    name: String,
    image: Box<crate::discord::ProfileAvatarUpload>,
) {
    if let Err(error) = client.create_emoji(guild_id, &name, &image).await {
        report_moderation_failure(&client, "adding emoji", &name, &error).await;
    }
}

pub(super) async fn rename_emoji(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    emoji_id: Id<EmojiMarker>,
    name: String,
) {
    if let Err(error) = client.rename_emoji(guild_id, emoji_id, &name).await {
        report_moderation_failure(&client, "renaming emoji to", &name, &error).await;
    }
}

pub(super) async fn delete_emoji(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    emoji_id: Id<EmojiMarker>,
    label: String,
) {
    if let Err(error) = client.delete_emoji(guild_id, emoji_id).await {
        report_moderation_failure(&client, "deleting emoji", &label, &error).await;
    }
}

pub(super) async fn send_friend_request(client: DiscordClient, target: String) {
    let Some((username, discriminator)) = friend_request_target(&target) else {
        client
            .publish_event(AppEvent::GatewayError {
                message: format!("{target} is not a username"),
            })
            .await;
        return;
    };

    if let Err(error) = client.send_friend_request(&username, discriminator).await {
        report_moderation_failure(&client, "friend request to", &target, &error).await;
    }
}

pub(super) async fn add_friend(client: DiscordClient, user_id: Id<UserMarker>, label: String) {
    if let Err(error) = client.add_friend(user_id).await {
        report_moderation_failure(&client, "friending", &label, &error).await;
    }
}

pub(super) async fn block_user(client: DiscordClient, user_id: Id<UserMarker>, label: String) {
    if let Err(error) = client.block_user(user_id).await {
        report_moderation_failure(&client, "blocking", &label, &error).await;
    }
}

pub(super) async fn remove_relationship(
    client: DiscordClient,
    user_id: Id<UserMarker>,
    label: String,
) {
    if let Err(error) = client.remove_relationship(user_id).await {
        report_moderation_failure(&client, "removing", &label, &error).await;
    }
}

pub(super) async fn set_member_roles(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    role_ids: Vec<Id<RoleMarker>>,
    label: String,
) {
    if let Err(error) = client.set_member_roles(guild_id, user_id, &role_ids).await {
        report_moderation_failure(&client, "role change for", &label, &error).await;
    }
}

pub(super) async fn timeout_member(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    minutes: Option<u32>,
    label: String,
) {
    if let Err(error) = client.timeout_member(guild_id, user_id, minutes).await {
        report_moderation_failure(&client, "timeout for", &label, &error).await;
    }
}

/// Look up an invite so the user can see where it leads before joining.
pub(super) async fn resolve_invite(client: DiscordClient, code: String) {
    match client.resolve_invite(&code).await {
        Ok(mut preview) => {
            // Answered here rather than in the front end: whether the account
            // is already a member is state the client owns, and a UI asking
            // "join?" about a guild you are in is simply wrong.
            preview.already_joined = preview
                .guild_id
                .is_some_and(|guild_id| client.is_member_of(guild_id));

            client
                .publish_event(AppEvent::InviteResolved { preview })
                .await;
        }
        Err(error) => {
            log_app_error("resolve invite failed", &error);
            client
                .publish_event(AppEvent::InviteResolveFailed {
                    code,
                    message: error.to_string(),
                })
                .await;
        }
    }
}

/// Accept an invite, joining the guild.
pub(super) async fn accept_invite(client: DiscordClient, code: String) {
    match client.accept_invite(&code).await {
        Ok(guild_id) => {
            // The guild itself arrives over the gateway as a GuildCreate; this
            // only reports that the join was accepted.
            client
                .publish_event(AppEvent::InviteAccepted { code, guild_id })
                .await;
        }
        Err(error) => {
            log_app_error("accept invite failed", &error);
            client
                .publish_event(AppEvent::InviteAcceptFailed {
                    code,
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn add_reaction(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    emoji: ReactionEmoji,
) {
    match client.add_reaction(channel_id, message_id, &emoji).await {
        Ok(()) => {
            client
                .publish_event(AppEvent::CurrentUserReactionAdd {
                    channel_id,
                    message_id,
                    emoji: emoji.clone(),
                })
                .await;
        }
        Err(error) => publish_app_error(&client, "add reaction failed", &error).await,
    }
}

pub(super) async fn remove_reaction(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    emoji: ReactionEmoji,
) {
    match client
        .remove_current_user_reaction(channel_id, message_id, &emoji)
        .await
    {
        Ok(()) => {
            client
                .publish_event(AppEvent::CurrentUserReactionRemove {
                    channel_id,
                    message_id,
                    emoji: emoji.clone(),
                })
                .await;
        }
        Err(error) => publish_app_error(&client, "remove reaction failed", &error).await,
    }
}

pub(super) async fn load_reaction_users(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    emoji: ReactionEmoji,
    after: Option<Id<UserMarker>>,
) {
    match client
        .load_reaction_users_page(channel_id, message_id, &emoji, after)
        .await
    {
        Ok(page) => {
            client
                .publish_event(AppEvent::ReactionUsersLoaded {
                    channel_id,
                    message_id,
                    emoji,
                    users: page.users,
                    next_after: page.next_after,
                    after,
                })
                .await;
        }
        Err(error) => {
            publish_app_error(&client, "load reaction users failed", &error).await;
            // Clears the popup's in-flight flag so the emoji can be retried.
            client
                .publish_event(AppEvent::ReactionUsersLoadFailed {
                    channel_id,
                    message_id,
                    emoji,
                })
                .await;
        }
    }
}

pub(super) async fn load_pinned_messages(client: DiscordClient, channel_id: Id<ChannelMarker>) {
    match client.load_pinned_messages(channel_id).await {
        Ok(messages) => {
            client
                .publish_event(AppEvent::PinnedMessagesLoaded {
                    channel_id,
                    messages,
                })
                .await;
        }
        Err(error) => {
            log_app_error("load pinned messages failed", &error);
            client
                .publish_event(AppEvent::PinnedMessagesLoadFailed {
                    channel_id,
                    message: format!("load pinned messages failed: {error}"),
                })
                .await;
        }
    }
}

pub(super) async fn set_message_pinned(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    pinned: bool,
) {
    match client
        .set_message_pinned(channel_id, message_id, pinned)
        .await
    {
        Ok(()) => {
            client
                .publish_event(AppEvent::MessagePinnedUpdate {
                    channel_id,
                    message_id,
                    pinned,
                })
                .await;
        }
        Err(error) => publish_app_error(&client, "set pin failed", &error).await,
    }
}

pub(super) async fn vote_poll(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    answer_ids: Vec<u8>,
) {
    match client.vote_poll(channel_id, message_id, &answer_ids).await {
        Ok(()) => {
            client
                .publish_event(AppEvent::CurrentUserPollVoteUpdate {
                    channel_id,
                    message_id,
                    answer_ids,
                })
                .await;
        }
        Err(error) => publish_app_error(&client, "poll vote failed", &error).await,
    }
}

fn message_create_event(message: MessageInfo) -> AppEvent {
    AppEvent::MessageCreate { message }
}

async fn publish_message_send_error(
    client: &DiscordClient,
    channel_id: Id<ChannelMarker>,
    context: &str,
    error: &AppError,
) {
    let retry_after_millis = match error {
        AppError::DiscordRateLimited {
            retry_after_millis, ..
        }
        | AppError::MessageSlowModeActive { retry_after_millis } => Some(*retry_after_millis),
        _ => None,
    };
    if let Some(retry_after_millis) = retry_after_millis {
        log_app_error(context, error);
        client
            .publish_event(AppEvent::MessageSendRateLimited {
                channel_id,
                retry_after_millis,
            })
            .await;
        return;
    }
    publish_app_error(client, context, error).await;
}

fn message_update_event(message: MessageInfo) -> AppEvent {
    AppEvent::MessageUpdateDispatch {
        update: MessageUpdateDispatchInfo {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.message_id,
            fields: MessageUpdateEventFields {
                poll: message.poll,
                content: message.content,
                sticker_names: Some(message.sticker_names),
                stickers: Some(message.stickers),
                mentions: Some(message.mentions),
                mention_everyone: Some(message.mention_everyone),
                mention_roles: Some(message.mention_roles),
                flags: Some(message.flags),
                attachments: AttachmentUpdate::Replace(message.attachments),
                embeds: Some(message.embeds),
                edited_timestamp: message.edited_timestamp,
            },
            extra_fields: BTreeMap::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn typing_in_an_uncached_channel_reports_the_block_instead_of_sending() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();

        trigger_typing(client.clone(), Id::new(1)).await;

        let effect = effects.try_recv().expect("block reason is published");
        let AppEvent::GatewayError { message } = effect.event else {
            panic!("expected a gateway error, got {:?}", effect.event);
        };
        assert!(
            message.contains("show typing indicator failed"),
            "unexpected message: {message}"
        );
    }
}
