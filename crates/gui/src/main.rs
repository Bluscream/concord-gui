// concord-gui — a GPUI front-end for the concord Discord client core.
//
// This crate deliberately contains no protocol, gateway, voice or media code.
// All of that lives in the upstream `concord` crate and is consumed unmodified
// as a library. The architectural premise of this rewrite:
//
//   * `concord::discord` (85k lines) has zero ratatui/crossterm references
//   * `concord::app`     drives it via 70 AppCommand variants
//   * `concord::AppEvent` reports back via 131 variants
//
// so a front-end only has to render state and issue commands.
//
// Status: bootstrap shell. Proves linkage against the core and opens a window.
// Wiring the command/event loop is the next step; see docs/REWRITE.md.

mod model;
mod session;
mod theme;
mod ui;

use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};

use crate::ui::login::Login;
use crate::ui::workspace::{Screen, Workspace, WorkspaceModel};

use concord::config::CredentialStoreMode;
use concord::{paths, token_store};

/// What the shell knows about the core before a session is established.
///
/// Deliberately minimal: this is the seam probe. It reads only what the core
/// exposes publicly, which is how we verify the GUI can be built against the
/// library without touching `src/`.
struct CoreStatus {
    config_path: String,
    state_path: String,
    /// Whether a credential is already present. The token itself is never read
    /// into the GUI - only its presence is reported.
    has_token: bool,
    core_version: &'static str,
}

impl CoreStatus {
    fn probe() -> Self {
        let config_path = paths::config_file()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unavailable>".into());

        let state_path = paths::state_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unavailable>".into());

        // Presence check only. A failure to reach the credential store is
        // reported as "absent" rather than surfaced as an error - the login
        // flow is responsible for that path.
        let has_token = token_store::env_token().is_some()
            || matches!(
                token_store::load_token(CredentialStoreMode::default()),
                Ok(Some(_))
            );

        Self {
            config_path,
            state_path,
            has_token,
            core_version: env!("CARGO_PKG_VERSION"),
        }
    }
}

/// Resolve a token without any interactive flow.
///
/// The GUI login screen is not built yet, so for now a session can only start
/// from an existing credential (env var or the configured store). When that is
/// absent the workspace opens in a disconnected state that says so, rather
/// than failing to launch.
fn existing_token() -> Option<String> {
    if let Some(token) = token_store::env_token() {
        return Some(token);
    }
    token_store::load_token(CredentialStoreMode::default())
        .ok()
        .flatten()
}

fn main() {
    let status = CoreStatus::probe();

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);

        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_window, cx| {
                    let mut model = WorkspaceModel::empty();
                    model.status_line = "connecting…".to_string();
                    // With a stored credential the workspace opens directly;
                    // otherwise the login screen is the entry point.
                    let screen = if status.has_token {
                        Screen::Ready
                    } else {
                        Screen::Login(Login::default())
                    };
                    cx.new(|cx| Workspace::new(model, screen, cx))
                },
            )
            .expect("failed to open window");

        // Start the core only when a credential already exists.
        if let Some(token) = existing_token() {
            match session::spawn(token) {
                Ok((updates, handle)) => {
                    window
                        .update(cx, |workspace, _window, cx| {
                            workspace.attach(handle);
                            cx.notify();
                        })
                        .ok();

                    Workspace::pump(window, updates, cx);
                }
                Err(error) => {
                    window
                        .update(cx, |workspace, _window, cx| {
                            workspace.model.status_line =
                                format!("failed to start session: {error}");
                            cx.notify();
                        })
                        .ok();
                }
            }
        }

        cx.activate(true);
    });
}
