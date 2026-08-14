//! Bridges the core's async session onto GPUI's executor.
//!
//! The core is tokio-driven: it publishes `SequencedAppEvent`s on an mpsc
//! channel and signals state-store revisions on a watch channel. GPUI has its
//! own executor and requires entity mutation on the foreground thread.
//!
//! The bridge therefore runs a tokio runtime on a dedicated thread, forwards
//! everything it observes down a single channel, and lets the foreground task
//! drain that channel and update the view.
//!
//! Reprojection is driven by the snapshot watch rather than by individual
//! events: `DiscordState` is already an immutable snapshot behind `Arc`s, so
//! rebuilding the view model is cheap and avoids maintaining a parallel
//! reducer that could drift from the core's own state machine.

use std::sync::Arc;

use anyhow::Result;
use concord::app::Session;
use concord::discord::{AppCommand, AppEvent, DiscordState};
use tokio::sync::mpsc;

/// What the bridge forwards to the UI thread.
pub enum Update {
    /// The state store changed; carries a freshly-read state to project.
    State(Arc<DiscordState>),
    /// A discrete event worth surfacing (errors, toasts, connection status).
    ///
    /// Carries the state as observed when the event fired, so notification
    /// eligibility - which the core computes from mutes and mention rules -
    /// can be evaluated against a consistent snapshot.
    Event(Box<AppEvent>, Arc<DiscordState>),
    /// The session ended, with an optional reason.
    Closed(Option<String>),
}

/// Handle held by the UI for issuing commands to the core.
#[derive(Clone)]
pub struct SessionHandle {
    commands: mpsc::Sender<AppCommand>,
}

impl SessionHandle {
    /// Issue a command.
    ///
    /// Uses `try_send` rather than spawning an async send: this is called from
    /// GPUI's main thread, which has no tokio runtime, so `tokio::spawn` here
    /// panics with "there is no reactor running". The channel has a 64-slot
    /// buffer, which is ample for UI-driven commands.
    ///
    /// A full or closed channel drops the command silently; the UI learns
    /// about a dead session through `Update::Closed` rather than per-command
    /// error handling.
    pub fn send(&self, command: AppCommand) {
        if let Err(error) = self.commands.try_send(command) {
            tracing::warn!("dropped command: {error}");
        }
    }
}

/// Spawns the core on a dedicated tokio thread.
///
/// Returns the update stream and a handle for issuing commands. The token is
/// consumed here and never retained by the GUI.
pub fn spawn(token: String) -> Result<(mpsc::UnboundedReceiver<Update>, SessionHandle)> {
    let (updates_tx, updates_rx) = mpsc::unbounded_channel();
    let (commands_tx, mut commands_rx) = mpsc::channel::<AppCommand>(64);

    // Fixture mode: the token "test" loads synthetic state instead of
    // connecting. This exercises every rendering path offline, with no
    // account and no network.
    #[cfg(feature = "fixtures")]
    if concord::discord::fixtures::is_fixture_token(&token) {
        let _ = updates_tx.send(Update::State(Arc::new(
            concord::discord::fixtures::demo_state(),
        )));
        let _ = updates_tx.send(Update::Event(
            Box::new(AppEvent::Ready {
                user: "test-account".to_string(),
                user_id: None,
            }),
            Arc::new(concord::discord::fixtures::demo_state()),
        ));

        // Commands are drained and dropped: there is no server to accept them.
        std::thread::spawn(move || while commands_rx.blocking_recv().is_some() {});

        return Ok((
            updates_rx,
            SessionHandle {
                commands: commands_tx,
            },
        ));
    }

    std::thread::Builder::new()
        .name("concord-core".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = updates_tx.send(Update::Closed(Some(format!(
                        "failed to start runtime: {error}"
                    ))));
                    return;
                }
            };

            runtime.block_on(async move {
                let auth = Session::new_auth_session().await;

                let session = match Session::start(token, auth, Vec::new()).await {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = updates_tx
                            .send(Update::Closed(Some(format!("session failed: {error}"))));
                        return;
                    }
                };

                let Session {
                    mut effects,
                    mut snapshots,
                    commands,
                    client,
                    ..
                } = session;

                // Forward UI commands into the core's command loop.
                let command_forwarder = tokio::spawn(async move {
                    while let Some(command) = commands_rx.recv().await {
                        if commands.send(command).await.is_err() {
                            break;
                        }
                    }
                });

                // Emit an initial projection so the window is populated as soon
                // as READY lands rather than waiting for the first change.
                let _ = updates_tx.send(Update::State(Arc::new(client.current_discord_snapshot().to_state())));

                loop {
                    tokio::select! {
                        changed = snapshots.changed() => {
                            if changed.is_err() {
                                break;
                            }
                            if updates_tx.send(Update::State(Arc::new(client.current_discord_snapshot().to_state()))).is_err() {
                                break;
                            }
                        }
                        event = effects.recv() => {
                            match event {
                                Some(sequenced) => {
                                    let state =
                                        Arc::new(client.current_discord_snapshot().to_state());
                                    if updates_tx
                                        .send(Update::Event(Box::new(sequenced.event), state))
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                None => break,
                            }
                        }
                    }
                }

                command_forwarder.abort();
                let _ = updates_tx.send(Update::Closed(None));
            });
        })?;

    Ok((
        updates_rx,
        SessionHandle {
            commands: commands_tx,
        },
    ))
}
