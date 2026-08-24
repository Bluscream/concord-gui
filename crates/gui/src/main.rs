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
// See docs/REWRITE.md for the phased plan and what remains.

mod demo;
mod editor;
mod http;
mod keymap;
mod model;
mod notify;
mod runtime;
mod session;
mod theme;
mod ui;

use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};

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
    // Held for the life of the process: GPUI's main thread needs an ambient
    // tokio context because several dependencies spawn onto one from here.
    let _runtime_guard = runtime::enter();

    let allow_multi = std::env::args().any(|arg| arg == "--multi-instance")
        || std::env::var("MULTI_INSTANCE").is_ok();

    if !allow_multi {
        let socket_path = std::env::var("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("concord-gui.sock");

        if socket_path.exists() {
            if std::os::unix::net::UnixStream::connect(&socket_path).is_ok() {
                eprintln!("concord-gui is already running at {socket_path:?}. Exiting second instance.");
                std::process::exit(0);
            }
            let _ = std::fs::remove_file(&socket_path);
        }
        match std::os::unix::net::UnixListener::bind(&socket_path) {
            Ok(listener) => {
                eprintln!("Bound single-instance socket at {socket_path:?}");
                std::thread::spawn(move || {
                    for stream in listener.incoming() {
                        if let Ok(mut stream) = stream {
                            use std::io::Read;
                            let mut buf = [0u8; 16];
                            let _ = stream.read(&mut buf);
                        }
                    }
                });
            }
            Err(err) => {
                eprintln!("Could not bind single-instance socket at {socket_path:?}: {err}");
            }
        }
    }

    // GPUI needs an HTTP client before it will load images from a URI.
    let _app_guard = runtime::enter();
    Application::new()
        .with_http_client(http::ReqwestClient::shared())
        .run(move |cx: &mut App| {
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
                        let screen = if existing_token().is_some() {
                            Screen::Ready
                        } else {
                            Screen::Login(Box::default())
                        };
                        cx.new(|cx| Workspace::new(model, screen, cx))
                    },
                )
                .expect("failed to open window");

            let initial_token = std::env::args().nth(1).or_else(existing_token);
            if let Some(token) = initial_token {
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
