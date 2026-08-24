use super::image_format_for;
use concord::discord::{ActivityKind, AppCommand, AppEvent, VoiceConnectionStatus};

use crate::model::projection::Selection;

use crate::ui::forum::ForumPost;
use crate::ui::workspace::*;

impl Workspace {
    /// Fold a discrete event into the model.
    ///
    /// Most state arrives through reprojection; this handles only what is not
    /// represented in the state store, such as transient errors.
    pub(crate) fn absorb_event(&mut self, event: AppEvent) {
        match &event {
            // Our own presence, so the activity button and the editor show
            // what is actually being broadcast - including when another
            // client or the RPC socket set it rather than this one.
            AppEvent::PresenceUpdate { presence, .. }
                if Some(presence.user_id) == self.current_user =>
            {
                self.status = presence.status;
                self.current_activity = presence
                    .activities
                    .iter()
                    .find(|activity| activity.kind != ActivityKind::Custom)
                    .cloned();
                self.custom_status = presence
                    .activities
                    .iter()
                    .find(|activity| activity.kind == ActivityKind::Custom)
                    .and_then(|activity| activity.state.clone())
                    .unwrap_or_default();
            }
            // A reconnect can have dropped messages while the socket was down.
            // Neither paging direction fills that hole, because both extend
            // from what is already cached.
            AppEvent::VoiceAudioSourcesLoaded {
                request_id,
                inputs,
                outputs,
                error,
            } if *request_id == self.audio_sources_request => {
                let devices = self.audio_devices.get_or_insert_with(Default::default);
                devices.inputs = inputs.clone();
                devices.outputs = outputs.clone();
                devices.error = error.clone();
            }
            // A send that Discord rejected. The text goes back to the composer
            // rather than being dropped: retyping a long message because the
            // client silently ate it is the worst possible outcome here.
            AppEvent::MessageSendFailed { channel_id, nonce } => {
                let restored = self.pending_sends.remove(nonce);
                if Some(*channel_id) == self.nav.channel
                    && let Some(content) = restored
                    && self.composer.is_empty()
                {
                    self.composer.set_text(&content);
                }
                self.model.status_line = "Message was not sent".to_string();
            }
            AppEvent::MessageSendRateLimited {
                retry_after_millis, ..
            } => {
                self.model.status_line = format!(
                    "Rate limited; retrying in {:.0}s",
                    *retry_after_millis as f64 / 1000.0
                );
            }
            AppEvent::MessageSendCooldownStarted {
                duration_millis, ..
            } => {
                // Slowmode. Reported as a duration rather than a bare refusal
                // so the user knows whether to wait or give up.
                self.model.status_line = format!(
                    "Slowmode: {:.0}s before the next message",
                    *duration_millis as f64 / 1000.0
                );
            }
            // One arm, not two: a message can be both in the open channel and
            // one of ours, and separate guarded arms would let the first match
            // shadow the second.
            AppEvent::MessageCreate { message } => {
                // Confirmed by the server, so the retry copy is no longer
                // needed. The nonce comes back on its own field rather than as
                // the message id, which the server assigns independently.
                if let Some(nonce) = message.nonce {
                    self.pending_sends.remove(&nonce);
                }

                // A message landing in the channel on screen means the user is
                // most likely looking at it, so schedule the ack rather than
                // letting the badge sit there. The core owns the delay.
                if Some(message.channel_id) == self.nav.channel {
                    self.schedule_mark_read();
                }
            }

            // Login cannot continue in this client: solving a captcha needs a
            // browser. Said plainly rather than leaving the attempt hanging.
            AppEvent::SoundboardSoundsLoaded { guild_id, sounds } => {
                // The server panel may be waiting on the same event. Only the
                // guild's own list belongs there - the defaults cannot be
                // renamed or deleted by anyone.
                if let (Some(view), Some(_)) = (&mut self.server_management, guild_id) {
                    view.loading = false;
                    view.error = None;
                    view.sounds = sounds.clone();
                }
                if let Some(view) = &mut self.soundboard {
                    view.loading = false;
                    view.error = None;
                    // The two lists arrive as separate replies, told apart by
                    // whether they name a guild.
                    match guild_id {
                        Some(_) => view.guild_sounds = sounds.clone(),
                        None => view.default_sounds = sounds.clone(),
                    }
                }
            }
            AppEvent::SoundboardSoundsLoadFailed { message, .. } => {
                if let Some(view) = &mut self.soundboard {
                    view.loading = false;
                    view.error = Some(message.clone());
                }
            }
            AppEvent::StageInstanceLoaded { instance, .. } => {
                self.stage_running = instance.clone();
                // Seeded, so changing a topic is a correction rather than a
                // retype - and so emptying it is a deliberate act.
                if let (Some(instance), Some((Prompt::StageTopic(_), composer))) =
                    (instance, self.prompt.as_mut())
                {
                    composer.set_text(&instance.topic);
                }
            }
            AppEvent::DiscoverableGuildsLoaded { guilds } => {
                self.discovering = false;
                self.discovered = guilds.clone();
            }
            AppEvent::DiscoveryMetadataLoaded {
                guild_id,
                metadata,
                categories,
            } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.discovery = Some((**metadata).clone());
                    view.discovery_categories = categories.clone();
                }
            }
            AppEvent::GuildStickersLoaded { guild_id, stickers } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.stickers = stickers.clone();
                }
            }
            AppEvent::OnboardingLoaded {
                guild_id,
                onboarding,
            } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.onboarding = Some((**onboarding).clone());
                }
            }
            AppEvent::ScheduledEventsLoaded { guild_id, events } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.events = events.clone();
                }
            }
            AppEvent::GuildTemplatesLoaded {
                guild_id,
                templates,
            } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.templates = templates.clone();
                }
            }
            AppEvent::WelcomeScreenLoaded { guild_id, screen } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.welcome = Some(screen.clone());
                }
            }
            AppEvent::GuildWidgetLoaded { guild_id, widget } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.widget = Some(widget.clone());
                }
            }
            AppEvent::PruneCountLoaded { guild_id, count } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.prune_count = Some(*count);
                }
            }
            AppEvent::GuildPruned { guild_id, .. } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    // Zeroed rather than left alone: the old count described
                    // members who have just been removed, so leaving it would
                    // offer to prune them again.
                    view.prune_count = Some(0);
                }
            }
            AppEvent::MembershipRequestFailed { message } => {
                if let Some(view) = &mut self.server_management {
                    view.loading = false;
                    view.error = Some(message.clone());
                }
            }
            AppEvent::TotpEnabled { backup_codes } => {
                if let Some(view) = &mut self.account {
                    // Kept on screen rather than flashed: these are the only
                    // thing between a lost phone and a lost account.
                    view.backup_codes = backup_codes.clone();
                    view.totp_secret = None;
                    view.totp_code.clear();
                }
            }
            AppEvent::BackupCodesLoaded { codes } => {
                if let Some(view) = &mut self.account {
                    view.backup_codes = codes.clone();
                }
            }
            AppEvent::AuthSessionsLoaded { sessions } => {
                if let Some(view) = &mut self.access {
                    view.loading = false;
                    view.error = None;
                    // Selections for sessions that are gone are dropped: a
                    // logout aimed at one would fail the whole request.
                    view.logout_targets
                        .retain(|hash| sessions.iter().any(|s| &s.id_hash == hash));
                    view.sessions = sessions.clone();
                }
            }
            AppEvent::AuthorisedAppsLoaded { apps } => {
                if let Some(view) = &mut self.access {
                    view.loading = false;
                    view.error = None;
                    view.apps = apps.clone();
                }
            }
            AppEvent::AuthSessionsLoadFailed { message }
            | AppEvent::AuthorisedAppsLoadFailed { message } => {
                if let Some(view) = &mut self.access {
                    view.loading = false;
                    view.error = Some(message.clone());
                }
            }
            AppEvent::ConnectionsLoaded { connections } => {
                if let Some(view) = &mut self.connections {
                    view.loading = false;
                    view.error = None;
                    view.connections = connections.clone();
                }
            }
            AppEvent::ConnectionsLoadFailed { message } => {
                if let Some(view) = &mut self.connections {
                    view.loading = false;
                    view.error = Some(message.clone());
                }
            }
            AppEvent::AutoModRulesLoaded { guild_id, rules } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.automod = rules.clone();
                }
            }
            AppEvent::AutoModRulesLoadFailed { guild_id, message } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = Some(message.clone());
                }
            }
            AppEvent::GuildInvitesLoaded { guild_id, invites } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.invites = invites.clone();
                }
            }
            AppEvent::GuildEmojisLoaded { guild_id, emojis } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.emojis = emojis.clone();
                }
            }
            AppEvent::GuildAuditLogLoaded { guild_id, entries } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.audit_log = entries.clone();
                }
            }
            AppEvent::GuildInvitesLoadFailed { guild_id, message }
            | AppEvent::GuildEmojisLoadFailed { guild_id, message }
            | AppEvent::GuildAuditLogLoadFailed { guild_id, message } => {
                if let Some(view) = &mut self.server_management
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = Some(message.clone());
                }
            }
            // Shown rather than only logged: an invite nobody can read is an
            // invite nobody can send, which is the whole point of making one.
            AppEvent::InviteCreated { code, .. } => {
                self.model.status_line = concord::i18n::translate_text(
                    "status-invite-created",
                    &[("code", code.as_str())],
                );
            }
            AppEvent::GuildBansLoaded { guild_id, bans } => {
                if let Some(view) = &mut self.bans
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = None;
                    view.bans = bans.clone();
                }
            }
            AppEvent::GuildBansLoadFailed { guild_id, message } => {
                if let Some(view) = &mut self.bans
                    && view.guild_id == *guild_id
                {
                    view.loading = false;
                    view.error = Some(message.clone());
                }
            }
            AppEvent::InviteResolved { preview } => {
                if let Some(invite) = &mut self.invite
                    && invite.code == preview.code
                {
                    invite.preview = Some(preview.clone());
                }
            }
            AppEvent::InviteResolveFailed { code, message } => {
                if let Some(invite) = &mut self.invite
                    && invite.code == *code
                {
                    invite.error = Some(message.clone());
                }
            }
            AppEvent::InviteAccepted { .. } => {
                // The guild itself arrives as a GuildCreate and reprojects.
                self.model.status_line = "Joined".to_string();
            }
            AppEvent::InviteAcceptFailed { message, .. } => {
                self.model.status_line = format!("Could not join: {message}");
            }
            AppEvent::CaptchaRequired { action } => {
                self.model.status_line =
                    format!("Discord demanded a captcha for {action}; use a browser to continue");
            }
            AppEvent::SignedOut => {
                self.model.connected = false;
                self.model.status_line = "Signed out".to_string();
            }
            AppEvent::GatewayClosed => {
                self.model.connected = false;
                self.model.status_line = "Disconnected; reconnecting".to_string();
            }
            AppEvent::GatewayResumed => {
                self.model.connected = true;
                self.model.status_line = "Reconnected".to_string();
            }
            AppEvent::UpdateAvailable { latest_version } => {
                self.model.status_line = format!("concord {latest_version} is available");
            }

            AppEvent::InteractionFailed { reason_code, .. } => {
                self.model.status_line = format!("The command failed (code {reason_code})");
            }
            AppEvent::InteractionSucceeded { .. } => {
                // The bot's reply arrives as an ordinary message, so there is
                // nothing to show beyond clearing any earlier failure.
                self.model.status_line.clear();
            }
            AppEvent::ApplicationCommandAutocompleteResponse { choices, .. } => {
                self.command_choices = choices.iter().map(|choice| choice.name.clone()).collect();
            }
            AppEvent::ApplicationCommandIndexUpdated { guild_id } => {
                // A bot's command list changed; re-fetch so the picker is not
                // offering commands that no longer exist.
                if self.nav.selection == Selection::Guild(*guild_id)
                    && let Some(handle) = &self.handle
                {
                    handle.send(AppCommand::LoadApplicationCommands {
                        guild_id: Some(*guild_id),
                    });
                }
            }

            AppEvent::AttachmentDownloadStarted {
                id,
                filename,
                total_bytes,
                ..
            } => {
                self.downloads
                    .push((*id, filename.clone(), total_bytes.map(|_| 0.0)));
                self.model.status_line = format!("Downloading {filename}");
            }
            AppEvent::AttachmentDownloadProgress {
                id,
                downloaded_bytes,
                total_bytes,
            } => {
                if let Some(entry) = self.downloads.iter_mut().find(|entry| entry.0 == *id) {
                    // Only meaningful with a known total; a download of unknown
                    // length shows activity without a false percentage.
                    entry.2 = total_bytes
                        .filter(|total| *total > 0)
                        .map(|total| (*downloaded_bytes as f32 / total as f32).clamp(0.0, 1.0));
                }
            }
            AppEvent::AttachmentDownloadCompleted { id, path, .. } => {
                self.downloads.retain(|entry| entry.0 != *id);
                self.model.status_line = format!("Saved to {path}");
            }
            AppEvent::AttachmentDownloadFailed {
                id,
                filename,
                message,
                ..
            } => {
                self.downloads.retain(|entry| entry.0 != *id);
                self.model.status_line = format!("{filename}: {message}");
            }

            AppEvent::AttachmentPreviewLoaded { url, bytes } => {
                match image_format_for(url) {
                    Some(format) => {
                        self.attachment_previews.insert(
                            url.clone(),
                            std::sync::Arc::new(gpui::Image::from_bytes(format, bytes.clone())),
                        );
                        // A picture that has just decoded makes the log
                        // taller than it was when it was last scrolled, so
                        // the newest message slides back out of view unless
                        // the follow is re-applied here.
                        if self.follow_bottom {
                            self.message_scroll.scroll_to_bottom();
                        }
                    }
                    // An extension GPUI cannot decode. Dropped rather than
                    // guessed at, since handing it the wrong format renders
                    // nothing and logs nothing.
                    None => {
                        self.model.status_line =
                            "Attachment is in a format this client cannot display".to_string();
                    }
                }
            }
            AppEvent::AttachmentPreviewLoadFailed { url, message } => {
                // Dropped from the requested set so a later reprojection can
                // retry; a transient CDN failure should not be permanent.
                self.requested_previews.remove(url);
                self.model.status_line = format!("Preview failed: {message}");
            }

            AppEvent::UserProfileLoadFailed { message, .. } => {
                // The panel is closed rather than left on a spinner that will
                // never resolve.
                self.profile = None;
                self.model.status_line = format!("Could not load profile: {message}");
            }
            AppEvent::UserProfileUpdateFailed { message, .. } => {
                self.model.status_line = format!("Profile not updated: {message}");
            }

            AppEvent::VoiceAudioSourcesApplyFailed {
                active_input_source,
                active_output_source,
                message,
                ..
            } => {
                // The picker is corrected to what is actually in use, so it
                // does not keep showing a device that was refused.
                if let Some(devices) = &mut self.audio_devices {
                    devices.selected_input = active_input_source.clone();
                    devices.selected_output = active_output_source.clone();
                    devices.error = Some(message.clone());
                }
                self.model.status_line = message.clone();
            }
            AppEvent::VoiceConnectionStatusChanged {
                status, message, ..
            } => {
                self.model.status_line = match status {
                    VoiceConnectionStatus::Connecting => "Voice: connecting".to_string(),
                    VoiceConnectionStatus::Connected => "Voice: connected".to_string(),
                    VoiceConnectionStatus::Disconnected => "Voice: disconnected".to_string(),
                    VoiceConnectionStatus::Failed => message
                        .clone()
                        .unwrap_or_else(|| "Voice: connection failed".to_string()),
                };

                // A failed or dropped connection clears the local voice state,
                // or the sidebar keeps showing a call that is not happening.
                if matches!(
                    status,
                    VoiceConnectionStatus::Disconnected | VoiceConnectionStatus::Failed
                ) {
                    self.voice_channel = None;
                    self.voice_scope_joined = None;
                    self.broadcasting = false;
                    self.watching = None;
                }
            }
            AppEvent::VoiceSound { .. } => {
                // The core plays the sound; there is nothing to display.
            }
            AppEvent::MediaPlaybackWindowReady { .. }
            | AppEvent::StreamPlaybackWindowReady { .. } => {
                // Playback opens in an external player, which is its own
                // visible confirmation.
            }
            AppEvent::StreamPlaybackEnded { reconnecting, .. } => {
                if !reconnecting {
                    self.watching = None;
                }
            }

            AppEvent::Ready { .. } => {
                self.refresh_history();
                self.hydrate_missing_members();
                self.restore_tabs();
                self.land_somewhere();
            }
            _ => {}
        }

        match event {
            AppEvent::GatewayError { message } => {
                self.model.status_line = message;
            }
            AppEvent::ForumPostsLoaded {
                channel_id,
                threads,
                first_messages,
                has_more,
                next_offset,
                ..
            } => {
                if let Some(forum) = &mut self.forum
                    && forum.channel_id == channel_id
                {
                    forum.loading = false;
                    forum.complete = !has_more;
                    forum.next_offset = next_offset;

                    for (index, thread) in threads.iter().enumerate() {
                        // Discord returns opening messages positionally
                        // alongside the threads, so they are paired by index.
                        let opening = first_messages.get(index);

                        forum.posts.push(ForumPost {
                            channel_id: thread.channel_id,
                            title: thread.name.clone(),
                            preview: opening
                                .and_then(|message| message.content.clone())
                                .map(|content| content.chars().take(160).collect())
                                .unwrap_or_default(),
                            author: opening
                                .map(|message| message.author.clone())
                                .unwrap_or_default(),
                            message_count: thread.message_count.unwrap_or(0),
                            archived: forum.showing_archived,
                        });
                    }
                }
            }
            AppEvent::ForumPostsLoadFailed { channel_id, .. } => {
                if let Some(forum) = &mut self.forum
                    && forum.channel_id == channel_id
                {
                    forum.loading = false;
                    forum.error = Some("Could not load posts".to_string());
                }
            }
            AppEvent::InboxMentionsLoaded { messages, .. } => {
                self.inbox = Some(
                    messages
                        .into_iter()
                        .map(|message| InboxMention {
                            channel_id: message.channel_id,
                            message_id: message.message_id,
                            guild_id: message.guild_id,
                            author: message.author,
                            content: message.content.unwrap_or_default(),
                        })
                        .collect(),
                );
            }
            AppEvent::InboxMentionsLoadFailed { .. } => self.inbox = None,
            AppEvent::ApplicationCommandsLoaded { commands, .. } => {
                self.app_commands = commands;
            }
            AppEvent::PinnedMessagesLoaded { messages, .. } => {
                self.pins = Some(
                    messages
                        .into_iter()
                        .map(|message| {
                            (
                                message.message_id,
                                message.author,
                                message.content.unwrap_or_default(),
                            )
                        })
                        .collect(),
                );
            }
            AppEvent::PinnedMessagesLoadFailed { .. } => self.pins = None,
            AppEvent::ReactionUsersLoaded {
                message_id,
                users,
                after,
                ..
            } => {
                if let Some((target, _, existing)) = &mut self.reaction_users
                    && *target == message_id
                {
                    let names = users.into_iter().map(|user| user.display_name);
                    // `after: None` is the first page and replaces; a cursor
                    // means this is a continuation and appends.
                    if after.is_none() {
                        *existing = names.collect();
                    } else {
                        existing.extend(names);
                    }
                }
            }
            AppEvent::ReactionUsersLoadFailed { .. } => self.reaction_users = None,
            AppEvent::StreamCaptureTargetsLoaded { targets, error, .. } => {
                if let Some(picker) = &mut self.stream_picker {
                    picker.loading = false;
                    picker.targets = targets;
                    picker.error = error;
                }
            }
            AppEvent::StreamBroadcastStarted { .. } => {
                self.broadcasting = true;
                self.stream_picker = None;
            }
            AppEvent::StreamBroadcastEnded { .. } => self.broadcasting = false,
            AppEvent::StreamBroadcastStartFailed { .. } => {
                self.broadcasting = false;
                self.stream_picker = None;
                // The event carries no reason, so the message stays generic
                // rather than inventing a cause.
                self.model.status_line = "Screen share failed to start".to_string();
            }
            AppEvent::StreamBroadcastAudioUnavailable { .. } => {
                // Video still works, so this is a note rather than a failure.
                self.model.status_line =
                    "Sharing without audio - system audio capture unavailable".to_string();
            }
            AppEvent::MessageSearchLoaded { page } => {
                if let Some(search) = &mut self.search {
                    search.running = false;
                    search.total = page.total_results;
                    search.results = page
                        .messages
                        .into_iter()
                        .map(|message| SearchResult {
                            author: message.author,
                            content: message.content.unwrap_or_default(),
                            channel_id: message.channel_id,
                            message_id: message.message_id,
                        })
                        .collect();
                }
            }
            AppEvent::MessageSearchLoadFailed { .. } => {
                if let Some(search) = &mut self.search {
                    search.running = false;
                    search.error = Some("search failed".to_string());
                }
            }
            AppEvent::Ready { user, user_id } => {
                self.current_user = user_id;
                self.model.connected = true;
                self.model.status_line = format!("connected as {user}");
            }
            _ => {}
        }
    }
}
