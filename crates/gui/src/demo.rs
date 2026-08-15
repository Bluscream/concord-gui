//! Isolated demo / fixture mode.
//!
//! Encapsulates offline synthetic state, token checks and demo session setup,
//! so demo-specific code does not clutter the session or workspace logic and
//! can be removed cleanly.
//!
//! The important property is that demo mode *answers* commands. A front-end
//! offline has no server, and simply dropping commands leaves every surface
//! that waits for a reply - forums, search, profiles, the capture picker -
//! spinning forever, which looks like a bug rather than a limitation.

use crate::session::{SessionHandle, Update};
#[cfg(feature = "fixtures")]
use concord::discord::AppCommand;
#[cfg(feature = "fixtures")]
use std::sync::Arc;
use tokio::sync::mpsc;

/// Check if the given token string is a demo/fixture token.
pub fn is_demo_token(token: &str) -> bool {
    #[cfg(feature = "fixtures")]
    {
        concord::discord::fixtures::is_fixture_token(token)
    }
    #[cfg(not(feature = "fixtures"))]
    {
        let _ = token;
        false
    }
}

/// Attempt to spawn an offline demo session if the token matches.
///
/// Returns `Some(Ok(..))` if a demo session was handled, or `None` if the
/// token is for a real connection.
pub fn try_spawn_demo(
    token: &str,
) -> Option<anyhow::Result<(mpsc::UnboundedReceiver<Update>, SessionHandle)>> {
    #[cfg(feature = "fixtures")]
    if is_demo_token(token) {
        let (updates_tx, updates_rx) = mpsc::unbounded_channel();
        let (commands_tx, mut commands_rx) = mpsc::channel::<AppCommand>(64);

        let mut state = concord::discord::fixtures::demo_state();
        let _ = updates_tx.send(Update::State(Arc::new(state.clone())));
        let _ = updates_tx.send(Update::Event(
            Box::new(concord::discord::AppEvent::Ready {
                user: "test-account".to_string(),
                user_id: Some(concord::discord::fixtures::demo_user_id()),
            }),
            Arc::new(state.clone()),
        ));

        // Runs on the shared runtime rather than a bare thread so it can wait
        // on a timer as well as on commands: the canned reply needs to arrive
        // after a pause, not instantly.
        crate::runtime::spawn(async move {
            let mut history_pages = std::collections::HashMap::new();
            let mut pending: Vec<Scheduled> = Vec::new();

            loop {
                let next_delay = pending
                    .iter()
                    .map(|item| item.at)
                    .min()
                    .map(|at| at.saturating_duration_since(std::time::Instant::now()));

                tokio::select! {
                    command = commands_rx.recv() => {
                        let Some(command) = command else { break };
                        if !handle_command(
                            &mut state,
                            command,
                            &updates_tx,
                            &mut history_pages,
                            &mut pending,
                        ) {
                            break;
                        }
                    }
                    // Only armed when something is scheduled.
                    _ = tokio::time::sleep(next_delay.unwrap_or(std::time::Duration::MAX)),
                        if next_delay.is_some() =>
                    {
                        if !fire_due(&mut state, &updates_tx, &mut pending) {
                            break;
                        }
                    }
                }
            }
        })?;

        return Some(Ok((
            updates_rx,
            SessionHandle {
                commands: commands_tx,
            },
        )));
    }

    let _ = token;
    None
}

/// A deferred action, so the demo can show activity over time rather than
/// resolving everything the instant a command arrives.
#[cfg(feature = "fixtures")]
struct Scheduled {
    at: std::time::Instant,
    action: Action,
}

#[cfg(feature = "fixtures")]
enum Action {
    /// Show a fixture user as typing.
    StartTyping {
        channel: concord::discord::Id<concord::discord::marker::ChannelMarker>,
        user: concord::discord::Id<concord::discord::marker::UserMarker>,
    },
    /// Stop typing and post the reply.
    Reply {
        channel: concord::discord::Id<concord::discord::marker::ChannelMarker>,
        user: concord::discord::Id<concord::discord::marker::UserMarker>,
        author: &'static str,
        body: &'static str,
    },
}

/// Run every action whose deadline has passed.
#[cfg(feature = "fixtures")]
fn fire_due(
    state: &mut concord::discord::DiscordState,
    updates: &mpsc::UnboundedSender<Update>,
    pending: &mut Vec<Scheduled>,
) -> bool {
    use concord::discord::fixtures;

    let now = std::time::Instant::now();
    let (due, rest): (Vec<_>, Vec<_>) = std::mem::take(pending)
        .into_iter()
        .partition(|item| item.at <= now);
    *pending = rest;

    for item in due {
        match item.action {
            Action::StartTyping { channel, user } => {
                fixtures::set_typing(state, channel, user);
            }
            Action::Reply {
                channel,
                user,
                author,
                body,
            } => {
                fixtures::clear_typing(state, channel, user);
                let guild = guild_of(state, channel);
                fixtures::append_message(state, channel, guild, user, author, body);
            }
        }
    }

    updates.send(Update::State(Arc::new(state.clone()))).is_ok()
}

/// Apply one command to the synthetic state and publish the result.
///
/// Returns false when the update channel has closed and the loop should stop.
#[cfg(feature = "fixtures")]
fn handle_command(
    state: &mut concord::discord::DiscordState,
    command: AppCommand,
    updates: &mpsc::UnboundedSender<Update>,
    history_pages: &mut std::collections::HashMap<
        concord::discord::Id<concord::discord::marker::ChannelMarker>,
        usize,
    >,
    pending: &mut Vec<Scheduled>,
) -> bool {
    use concord::discord::{AppEvent, ReactionEmoji, fixtures};

    // Events that need a state snapshot alongside them.
    macro_rules! publish_event {
        ($event:expr) => {
            if updates
                .send(Update::Event(Box::new($event), Arc::new(state.clone())))
                .is_err()
            {
                return false;
            }
        };
    }

    macro_rules! publish_state {
        () => {
            if updates
                .send(Update::State(Arc::new(state.clone())))
                .is_err()
            {
                return false;
            }
        };
    }

    match command {
        AppCommand::SendMessage {
            channel_id,
            content,
            attachments,
            ..
        } => {
            let guild_id = guild_of(state, channel_id);
            fixtures::append_message(
                state,
                channel_id,
                guild_id,
                fixtures::demo_user_id(),
                "blu",
                &content,
            );

            // Attachments are recorded on the message just appended, so a
            // staged file is visible after sending rather than vanishing.
            if !attachments.is_empty() {
                let files: Vec<(String, u64)> = attachments
                    .iter()
                    .map(|upload| (upload.filename.clone(), upload.size_bytes))
                    .collect();
                fixtures::attach_to_last_message(state, channel_id, &files);
            }

            publish_state!();

            // A canned reply, so the typing indicator and an incoming message
            // are both demonstrable offline. Delays are long enough to be
            // visible and short enough not to feel broken.
            let (user, author, body) = fixtures::demo_responder(channel_id);
            let now = std::time::Instant::now();
            pending.push(Scheduled {
                at: now + std::time::Duration::from_millis(600),
                action: Action::StartTyping {
                    channel: channel_id,
                    user,
                },
            });
            pending.push(Scheduled {
                at: now + std::time::Duration::from_millis(2200),
                action: Action::Reply {
                    channel: channel_id,
                    user,
                    author,
                    body,
                },
            });
        }

        AppCommand::JoinVoiceChannel {
            scope,
            channel_id,
            self_mute,
            self_deaf,
            ..
        } => {
            fixtures::join_voice(state, scope, channel_id, self_mute, self_deaf);
            publish_state!();
        }

        AppCommand::UpdateVoiceState {
            scope,
            channel_id,
            self_mute,
            self_deaf,
        } => {
            fixtures::join_voice(state, scope, channel_id, self_mute, self_deaf);
            publish_state!();
        }

        AppCommand::LeaveVoiceChannel { scope, .. } => {
            fixtures::leave_voice(state, scope);
            publish_state!();
        }

        AppCommand::LoadMessageHistory {
            channel_id, before, ..
        } => {
            // `before` is only set when paging backwards; the initial load
            // needs nothing, since the fixture is already populated.
            if before.is_some() {
                let page = *history_pages.entry(channel_id).or_insert(0);
                if fixtures::prepend_history(state, channel_id, page) {
                    history_pages.insert(channel_id, page + 1);
                    publish_state!();
                }
            }
        }

        AppCommand::RefreshMessageHistory { channel_id } => {
            // Paging state resets with it: after a refresh the loaded range
            // starts over, so the next backward page must start over too.
            history_pages.remove(&channel_id);
            publish_state!();
        }

        AppCommand::LoadMessageHistoryAfter { .. } => {
            // The fixture holds no messages past the newest, so there is
            // nothing to page forward into. Answered rather than dropped so
            // the caller does not sit waiting.
            publish_state!();
        }

        AppCommand::ScheduleAckChannel {
            channel_id,
            message_id,
        } => {
            fixtures::mark_read(state, channel_id, message_id);
            publish_state!();
        }

        AppCommand::SearchGuildMembers {
            guild_id, query, ..
        } => {
            // Every fixture member is already cached, so the search only has
            // to confirm the command reached something.
            let _ = fixtures::search_members(state, guild_id, &query);
            publish_state!();
        }

        AppCommand::AckChannel {
            channel_id,
            message_id,
        } => {
            fixtures::mark_read(state, channel_id, message_id);
            publish_state!();
        }

        AppCommand::AckChannels { targets } => {
            for (channel_id, message_id) in targets {
                fixtures::mark_read(state, channel_id, message_id);
            }
            publish_state!();
        }

        AppCommand::ForwardMessage {
            source_channel_id,
            message_id,
            target_channel_id,
            ..
        } => {
            // Copied into the target channel, which is what a forward looks
            // like from the reader's side.
            let forwarded = state
                .messages_for_channel(source_channel_id)
                .into_iter()
                .find(|message| message.id == message_id)
                .and_then(|message| message.content.clone());

            if let Some(content) = forwarded {
                fixtures::append_message(
                    state,
                    target_channel_id,
                    None,
                    fixtures::demo_user_id(),
                    "blu",
                    &format!("[forwarded] {content}"),
                );
                publish_state!();
            }
        }

        AppCommand::ResolveInvite { code } => {
            // Answered with a plausible server so the flow can be exercised
            // offline; the code is echoed back so the caller can match it.
            publish_event!(AppEvent::InviteResolved {
                preview: concord::discord::InvitePreview {
                    code,
                    guild_id: None,
                    guild_name: "Rust Community".to_string(),
                    channel_name: Some("welcome".to_string()),
                    inviter: Some("ferris".to_string()),
                    member_count: Some(48_213),
                    online_count: Some(3_907),
                    already_joined: false,
                },
            });
        }

        AppCommand::AcceptInvite { code } => {
            // No guild is added: the fixture's guild list is fixed, and
            // inventing one would leave a server that cannot be opened.
            publish_event!(AppEvent::InviteAccepted {
                code,
                guild_id: None,
            });
        }

        AppCommand::LoadThreadPreview {
            channel_id,
            message_id,
        } => {
            // The thread's newest message, which is what a preview shows.
            match state
                .messages_for_channel(channel_id)
                .last()
                .map(|message| fixtures::message_info(message))
            {
                Some(message) => publish_event!(AppEvent::ThreadPreviewLoaded {
                    channel_id,
                    message,
                }),
                None => publish_event!(AppEvent::ThreadPreviewLoadFailed {
                    channel_id,
                    message_id,
                }),
            }
        }

        AppCommand::LoadInboxChannelHistory { .. } => {
            // The fixture's channels are already fully populated, so the
            // context around a mention is on screen without fetching.
            publish_state!();
        }

        AppCommand::UpdateGuildFolderSettings { .. } => {
            // No folders in the fixture; answered so the caller is not left
            // waiting on a command that will never be acknowledged.
            publish_state!();
        }

        AppCommand::LoadAttachmentPreview { url } => {
            // Seeded from the URL so each attachment gets a distinguishable
            // image rather than every preview looking identical.
            let seed = url.bytes().map(u64::from).sum::<u64>();
            let bytes = fixtures::demo_preview_png(seed);
            if bytes.is_empty() {
                publish_event!(AppEvent::AttachmentPreviewLoadFailed {
                    url,
                    message: "could not encode the demo image".to_string(),
                });
            } else {
                publish_event!(AppEvent::AttachmentPreviewLoaded { url, bytes });
            }
        }

        AppCommand::LoadProfileAvatarPreview { .. }
        | AppCommand::RequestApplicationCommandAutocomplete { .. } => {
            // Both need a real upload or a real bot; there is nothing
            // meaningful the fixture can answer with.
        }

        AppCommand::LoadGuildMembersByIds { .. } => {
            // Every fixture member is already fully hydrated.
        }

        AppCommand::EditMessage {
            channel_id,
            message_id,
            content,
        } => {
            fixtures::edit_message(state, channel_id, message_id, &content);
            publish_state!();
        }

        AppCommand::DeleteMessage {
            channel_id,
            message_id,
        } => {
            fixtures::delete_message(state, channel_id, message_id);
            publish_state!();
        }

        AppCommand::AddReaction {
            channel_id,
            message_id,
            emoji,
        }
        | AppCommand::RemoveReaction {
            channel_id,
            message_id,
            emoji,
        } => {
            // Both map to a toggle: the fixture tracks only whether this user
            // reacted, so add and remove are the same operation here.
            if let ReactionEmoji::Unicode(glyph) = emoji {
                fixtures::toggle_reaction(state, channel_id, message_id, &glyph);
                publish_state!();
            }
        }

        AppCommand::SetMessagePinned {
            channel_id,
            message_id,
            pinned,
        } => {
            fixtures::set_pinned(state, channel_id, message_id, pinned);
            publish_state!();
        }

        AppCommand::EditThread {
            channel_id, name, ..
        } => {
            fixtures::rename_thread(state, channel_id, &name);
            publish_state!();
        }

        AppCommand::DeleteThread { channel_id, .. } => {
            fixtures::delete_thread(state, channel_id);
            publish_state!();
        }

        AppCommand::SetThreadLocked {
            channel_id, locked, ..
        } => {
            fixtures::set_thread_locked(state, channel_id, locked);
            publish_state!();
        }

        AppCommand::SetThreadMuted {
            channel_id, muted, ..
        } => {
            fixtures::set_thread_muted(state, channel_id, muted);
            publish_state!();
        }

        AppCommand::SetThreadPinned {
            channel_id, pinned, ..
        } => {
            fixtures::set_thread_pinned(state, channel_id, pinned);
            publish_state!();
        }

        AppCommand::CreateForumPost { post } => {
            fixtures::create_forum_post(state, post.channel_id, &post.title, &post.content);
            publish_state!();
        }

        AppCommand::LoadPinnedMessages { channel_id } => {
            let messages = state
                .messages_for_channel(channel_id)
                .into_iter()
                .filter(|message| message.pinned)
                .map(fixtures::message_info)
                .collect();
            publish_event!(AppEvent::PinnedMessagesLoaded {
                channel_id,
                messages,
            });
        }

        AppCommand::LoadInboxMentions { request_id, before } => {
            // Mentions of the demo user, gathered from the fixture's own
            // messages so the inbox agrees with what the channels contain.
            let mut messages = Vec::new();
            for channel in fixtures::demo_channel_ids() {
                for message in state.messages_for_channel(channel) {
                    let body = message.content.clone().unwrap_or_default();
                    if body.contains("<@1001>") || body.contains("@blu") {
                        messages.push(fixtures::message_info(message));
                    }
                }
            }

            publish_event!(AppEvent::InboxMentionsLoaded {
                request_id,
                before,
                messages,
                has_more: false,
            });
        }

        AppCommand::VotePoll {
            channel_id,
            message_id,
            answer_ids,
        } => {
            fixtures::vote_poll(state, channel_id, message_id, &answer_ids);
            publish_state!();
        }

        AppCommand::LoadUserProfile { user_id, guild_id } => {
            fixtures::add_profile(state, user_id, guild_id);
            publish_state!();
        }

        AppCommand::SearchMessages { query } => {
            let needle = query.content.clone().unwrap_or_default().to_lowercase();

            // Searched against the fixture's own messages, so results are
            // consistent with what is on screen rather than invented.
            let mut messages = Vec::new();
            for channel in fixtures::demo_channel_ids() {
                for message in state.messages_for_channel(channel) {
                    let body = message.content.clone().unwrap_or_default();
                    if !needle.is_empty() && body.to_lowercase().contains(&needle) {
                        messages.push(fixtures::message_info(message));
                    }
                }
            }

            let total = messages.len();
            publish_event!(AppEvent::MessageSearchLoaded {
                page: concord::discord::MessageSearchPage {
                    query,
                    messages,
                    total_results: Some(total),
                    has_more: false,
                },
            });
        }

        AppCommand::LoadForumPosts {
            channel_id,
            archive_state,
            offset,
            ..
        } => {
            let (threads, first_messages) = fixtures::forum_posts(channel_id, archive_state);
            publish_event!(AppEvent::ForumPostsLoaded {
                channel_id,
                archive_state,
                offset,
                next_offset: offset + threads.len(),
                threads,
                first_messages,
                has_more: false,
            });
        }

        AppCommand::LoadStreamCaptureTargets {
            request_id,
            scope,
            channel_id,
        } => {
            publish_event!(AppEvent::StreamCaptureTargetsLoaded {
                request_id,
                scope,
                channel_id,
                targets: fixtures::capture_targets(),
                error: None,
            });
        }

        AppCommand::StartVoiceStream {
            scope, channel_id, ..
        } => {
            publish_event!(AppEvent::StreamBroadcastStarted { scope, channel_id });
        }

        AppCommand::StopVoiceStream { scope, channel_id } => {
            publish_event!(AppEvent::StreamBroadcastEnded { scope, channel_id });
        }

        // Navigation, subscriptions, typing and history are all satisfied by
        // the fixture already being fully loaded, so they need no reply.
        _ => {}
    }

    true
}

/// The guild a channel belongs to, for messages appended offline.
#[cfg(feature = "fixtures")]
fn guild_of(
    state: &concord::discord::DiscordState,
    channel_id: concord::discord::Id<concord::discord::marker::ChannelMarker>,
) -> Option<concord::discord::Id<concord::discord::marker::GuildMarker>> {
    state
        .channel(channel_id)
        .and_then(|channel| channel.guild_id)
}
