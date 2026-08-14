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

use crate::ui::workspace::{Workspace, WorkspaceModel};

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

fn main() {
    let status = CoreStatus::probe();

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);

        let mut model = WorkspaceModel::placeholder();
        model.status_line = if status.has_token {
            format!(
                "credential present - session not started | config: {}",
                status.config_path
            )
        } else {
            "no credential - login required (not yet implemented)".to_string()
        };

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|_cx| Workspace::new(model)),
        )
        .expect("failed to open window");

        cx.activate(true);
    });
}
