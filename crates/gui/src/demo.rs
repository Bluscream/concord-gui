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

        std::thread::Builder::new()
            .name("concord-demo".into())
            .spawn(move || {
                while let Some(command) = commands_rx.blocking_recv() {
                    if !handle_command(&mut state, command, &updates_tx) {
                        break;
                    }
                }
            })
            .ok()?;

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

/// Apply one command to the synthetic state and publish the result.
///
/// Returns false when the update channel has closed and the loop should stop.
#[cfg(feature = "fixtures")]
fn handle_command(
    state: &mut concord::discord::DiscordState,
    command: AppCommand,
    updates: &mpsc::UnboundedSender<Update>,
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
            publish_state!();
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
