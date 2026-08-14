mod command_dispatch;
mod command_loop;
mod credentials;
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
    logging, tui, version_check,
};

use self::{
    command_loop::start_command_loop, credentials::resolve_token, shutdown::shutdown_gateway,
};

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
    gateway_task: JoinHandle<()>,
    command_task: JoinHandle<()>,
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
    ) -> Result<Self> {
        let client = DiscordClient::new_with_auth_session(token, auth_session)?;
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

#[derive(Default)]
pub struct App;

impl App {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> Result<()> {
        let theme_warnings = match config::load_theme_options_with_warnings() {
            Ok((theme_options, mut warnings)) => {
                warnings.extend(tui::initialize_theme(&theme_options));
                warnings
            }
            Err(error) => {
                logging::error("config", format!("failed to load theme config: {error}"));
                let mut warnings = vec![format!("theme.toml could not be loaded: {error}")];
                warnings.extend(tui::initialize_theme(&config::ThemeOptions::default()));
                warnings
            }
        };

        loop {
            let auth_session = Session::new_auth_session().await;
            let resolved_token = resolve_token(auth_session.clone()).await?;

            let session =
                Session::start(resolved_token.token, auth_session, resolved_token.warnings).await?;

            // `effects` is a non-Clone receiver, so the session is destructured
            // by value here and the task handles are retained for teardown.
            let Session {
                effects,
                snapshots,
                commands,
                client,
                gateway_task,
                command_task,
                warnings: _,
            } = session;

            let result = tui::run(
                effects,
                snapshots,
                commands,
                client.clone(),
                theme_warnings.clone(),
            )
            .await;

            command_task.abort();
            shutdown_gateway(&client, gateway_task).await;
            match result? {
                tui::DashboardExit::Quit => return Ok(()),
                // Sign-out of an env-token session quits: re-resolving would
                // read the same CONCORD_TOKEN and log straight back in.
                tui::DashboardExit::SignOut => {
                    if crate::token_store::env_token().is_some() {
                        return Ok(());
                    }
                }
            }
        }
    }
}
