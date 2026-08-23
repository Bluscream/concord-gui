mod command_dispatch;
mod command_loop;
mod gateway_commands;
mod history_commands;
mod inbox_commands;
mod media_adapters;
mod media_commands;
mod message_commands;
mod notification_commands;
mod read_state_commands;
mod session_commands;
mod shutdown;

mod user_commands;
mod voice_commands;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::{
    DiscordClient, Result, config,
    discord::{
        AppCommand, AppEvent, DiscordAuthSession, SequencedAppEvent, SnapshotRevision,
        load_client_fingerprint_and_http,
    },
    logging, version_check,
};

use self::command_loop::start_command_loop;

/// Shutting a session down cleanly. Public because a front end owns the loop
/// that decides when that is, and the sequence - stop the gateway, then the
/// command loop - is a property of the session rather than of any one screen.
pub use shutdown::shutdown_gateway;

/// A live, front-end-agnostic Discord session.
///
/// This is the seam every front-end drives. It owns the gateway and command
/// tasks and hands out the four channels a UI needs:
///
/// * `effects`   - ordered `AppEvent`s from the gateway (131 variants)
/// * `snapshots` - a watch channel signalling state-store revisions
/// * `commands`  - the sink for `AppCommand` (70 variants)
/// * `client`    - for direct queries against the state store
///
/// Obtaining a token is deliberately *not* part of this type: login is
/// inherently interactive and therefore front-end-specific. Callers resolve a
/// token however suits them and pass it to [`Session::start`].
pub struct Session {
    pub effects: mpsc::Receiver<SequencedAppEvent>,
    pub snapshots: watch::Receiver<SnapshotRevision>,
    pub commands: mpsc::Sender<AppCommand>,
    pub client: DiscordClient,
    /// Non-fatal warnings accumulated during startup, for the UI to surface.
    pub warnings: Vec<String>,
    pub gateway_task: JoinHandle<()>,
    pub command_task: JoinHandle<()>,
}

impl Session {
    /// Build a fresh auth session (client fingerprint + HTTP stack).
    ///
    /// Front-ends need this before login, since the login flows
    /// (password, QR) are driven through it.
    pub async fn new_auth_session() -> DiscordAuthSession {
        let (fingerprint, http) = load_client_fingerprint_and_http().await;
        DiscordAuthSession::with_http(fingerprint, http)
    }

    /// Start the gateway and command loop for an already-resolved token.
    pub async fn start(
        token: String,
        auth_session: DiscordAuthSession,
        warnings: Vec<String>,
        extension: Option<std::sync::Arc<dyn crate::discord::ClientExtension>>,
    ) -> Result<Self> {
        let mut client = DiscordClient::new_with_auth_session(token, auth_session)?;
        if let Some(extension) = extension {
            // Attached before the gateway starts, so the first events already
            // have somewhere to go rather than being the ones that get lost.
            client.attach_extension(extension);
        }
        // Injected events are published like any other, so whatever an
        // extension replays is drawn through the same path as live data.
        if let Some(mut injected) = client.take_injected_events() {
            let publisher = client.clone();
            tokio::spawn(async move {
                while let Some(event) = injected.recv().await {
                    publisher.publish_event(event).await;
                }
            });
        }
        let effects = client.take_effects();
        let snapshots = client.subscribe_snapshots();
        let (commands_tx, commands_rx) = mpsc::channel(64);

        let serve_rich_presence = config::load_options()
            .map(|options| options.presence.share_rich_presence)
            .unwrap_or(true);

        let gateway_task = client.start_gateway(serve_rich_presence);
        let command_task = start_command_loop(client.clone(), commands_rx);

        let version_client = client.clone();
        tokio::spawn(async move {
            match version_check::check_latest_version().await {
                Ok(Some(latest_version)) => {
                    version_client
                        .publish_event(AppEvent::UpdateAvailable { latest_version })
                        .await;
                }
                Ok(None) => {}
                Err(error) => {
                    logging::debug("version", format!("latest version check failed: {error}"))
                }
            }
        });

        // Startup warnings are surfaced through the same event stream the UI
        // already consumes, so no front-end needs a second reporting path.
        for warning in &warnings {
            logging::error("app", warning);
            client
                .publish_event(AppEvent::GatewayError {
                    message: warning.clone(),
                })
                .await;
        }

        Ok(Self {
            effects,
            snapshots,
            commands: commands_tx,
            client,
            warnings,
            gateway_task,
            command_task,
        })
    }

    /// Stop the command loop and shut the gateway down cleanly.
    pub async fn shutdown(self) {
        self.command_task.abort();
        shutdown_gateway(&self.client, self.gateway_task).await;
    }
}
