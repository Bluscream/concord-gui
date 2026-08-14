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
use concord::discord::{
    AppCommand, AppEvent, DiscordAuthSession, DiscordState,
    password_auth::{self, PasswordAuthEvent},
    qr_auth::{self, QrEvent},
};
use tokio::sync::mpsc;

use crate::runtime;

// Re-export the auth event types so workspace.rs only needs to import from here.
pub use concord::discord::password_auth::{MfaChallenge, MfaMethod};

/// Spawn a password-auth task and return a receiver for its events.
///
/// Runs on its own tokio runtime (same pattern as `spawn`). The caller drives
/// the auth state machine via the events and issues follow-up calls
/// (`spawn_mfa_verify`, `spawn_sms_send`) as needed.
pub fn spawn_password_login(login: String, password: String) -> mpsc::Receiver<PasswordAuthEvent> {
    let (tx, rx) = mpsc::channel(8);
    // Runs on the shared runtime rather than a private one: this is
    // called from GPUI's thread, where `tokio::spawn` has no reactor.
    if runtime::spawn(async move {
        let auth_session = DiscordAuthSession::fallback();
        let join = password_auth::spawn_login_with_auth_session(login, password, auth_session, tx);
        let _ = join.await;
    })
    .is_none()
    {
        // No runtime: the receiver closes immediately and the caller
        // reports a failed login rather than hanging.
        return rx;
    }

    rx
}

/// Spawn an MFA verification task (TOTP or SMS code submit).
pub fn spawn_mfa_verify(
    method: MfaMethod,
    code: String,
    ticket: String,
    login_instance_id: String,
) -> mpsc::Receiver<PasswordAuthEvent> {
    let (tx, rx) = mpsc::channel(8);
    // Runs on the shared runtime rather than a private one: this is
    // called from GPUI's thread, where `tokio::spawn` has no reactor.
    if runtime::spawn(async move {
        let auth_session = DiscordAuthSession::fallback();
        let join = password_auth::spawn_mfa_verify_with_auth_session(
            method,
            code,
            ticket,
            login_instance_id,
            auth_session,
            tx,
        );
        let _ = join.await;
    })
    .is_none()
    {
        // No runtime: the receiver closes immediately and the caller
        // reports a failed login rather than hanging.
        return rx;
    }

    rx
}

/// Spawn an SMS send task.
pub fn spawn_sms_send(ticket: String) -> mpsc::Receiver<PasswordAuthEvent> {
    let (tx, rx) = mpsc::channel(8);
    // Runs on the shared runtime rather than a private one: this is
    // called from GPUI's thread, where `tokio::spawn` has no reactor.
    if runtime::spawn(async move {
        let auth_session = DiscordAuthSession::fallback();
        let join = password_auth::spawn_sms_send_with_auth_session(ticket, auth_session, tx);
        let _ = join.await;
    })
    .is_none()
    {
        // No runtime: the receiver closes immediately and the caller
        // reports a failed login rather than hanging.
        return rx;
    }

    rx
}

/// Spawn a QR-auth task and return a receiver for its events.
pub fn spawn_qr_login() -> mpsc::Receiver<QrEvent> {
    let (tx, rx) = mpsc::channel(8);
    // Runs on the shared runtime rather than a private one: this is
    // called from GPUI's thread, where `tokio::spawn` has no reactor.
    if runtime::spawn(async move {
        let auth_session = DiscordAuthSession::fallback();
        let join = qr_auth::spawn_with_auth_session(auth_session, tx);
        let _ = join.await;
    })
    .is_none()
    {
        // No runtime: the receiver closes immediately and the caller
        // reports a failed login rather than hanging.
        return rx;
    }

    rx
}

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
    pub(crate) commands: mpsc::Sender<AppCommand>,
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
    if let Some(res) = crate::demo::try_spawn_demo(&token) {
        return res;
    }

    let (updates_tx, updates_rx) = mpsc::unbounded_channel();
    let (commands_tx, mut commands_rx) = mpsc::channel::<AppCommand>(64);

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
