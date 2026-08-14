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

use gpui::{
    App, Application, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    rgb, size,
};

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

struct Shell {
    status: CoreStatus,
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let row = |label: &'static str, value: String| {
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(div().w(px(140.)).text_color(rgb(0x8b93a7)).child(label))
                .child(div().text_color(rgb(0xe4e6ea)).child(value))
        };

        div()
            .flex()
            .flex_col()
            .gap_4()
            .size_full()
            .bg(rgb(0x1a1c20))
            .p_8()
            .text_sm()
            .child(
                div()
                    .text_xl()
                    .text_color(rgb(0xffffff))
                    .child("concord-gui"),
            )
            .child(
                div()
                    .text_color(rgb(0x8b93a7))
                    .child("GPUI front-end - bootstrap shell"),
            )
            .child(div().h(px(12.)))
            .child(row("core linked", "concord (library)".to_string()))
            .child(row("gui version", self.status.core_version.to_string()))
            .child(row("config", self.status.config_path.clone()))
            .child(row("state", self.status.state_path.clone()))
            .child(row(
                "credential",
                if self.status.has_token {
                    "present".to_string()
                } else {
                    "absent - login required".to_string()
                },
            ))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.), px(600.)), cx);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| {
                cx.new(|_cx| Shell {
                    status: CoreStatus::probe(),
                })
            },
        )
        .expect("failed to open window");

        cx.activate(true);
    });
}
