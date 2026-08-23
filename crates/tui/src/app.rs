//! Running the terminal client.
//!
//! The loop that owns a terminal: resolve a token, start a session, draw until
//! the user quits or signs out. Lives here rather than in the core because it
//! is one front end's idea of what running means - the GUI has its own, and
//! neither should be something the core has an opinion about.

use concord::app::shutdown_gateway;
use concord::config;
use concord::logging;
use concord::{Result, Session};

use crate::credentials::resolve_token;
use crate::tui;

#[derive(Default)]
pub struct App;

impl App {
    pub fn new() -> Self {
        Self
    }

    /// Run, optionally with something watching the events.
    ///
    /// `None` is the client as it was before a cache existed: state lives in
    /// memory and nothing outlives the process.
    pub async fn run_with(
        self,
        extension: Option<std::sync::Arc<dyn concord::discord::ClientExtension>>,
    ) -> Result<()> {
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

            // Checked before a session is opened, so a demo token never
            // reaches the network - the whole point is running with no account.
            #[cfg(feature = "fixtures")]
            let session = if crate::demo::is_demo_token(&resolved_token.token) {
                crate::demo::start(resolved_token.warnings)?
            } else {
                Session::start(
                    resolved_token.token,
                    auth_session,
                    resolved_token.warnings,
                    extension.clone(),
                )
                .await?
            };
            #[cfg(not(feature = "fixtures"))]
            let session = Session::start(
                resolved_token.token,
                auth_session,
                resolved_token.warnings,
                extension.clone(),
            )
            .await?;

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
                    if concord::token_store::env_token().is_some() {
                        return Ok(());
                    }
                }
            }
        }
    }
}
