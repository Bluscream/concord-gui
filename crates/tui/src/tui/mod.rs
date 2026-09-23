mod clipboard;
mod commands;
// Public so the GUI scores the quick switcher identically to the TUI;
// a second matcher would rank the same query differently in each client.
/// Kept as a name here so the terminal code that used it need not change,
/// but the implementation lives in `concord-ui` now: the GPUI front end wants
/// the same matching, and two copies of a scoring rule drift.
pub use concord_ui::fuzzy;
#[cfg(feature = "voice-playback")]
mod global_push_to_talk;
mod input;
// Public so the GUI resolves the same keymap.toml the TUI does, including
// custom bindings and leader sequences. Only `keybindings::external` is
// reachable from outside; the rest of the module stays private.
pub use concord_ui::keybindings;
mod login;
mod media;
mod message;
mod runtime;
mod selection;
mod state;
mod terminal;
mod text;
pub use concord_ui::text_cursor;
/// Editing semantics rather than terminal semantics: what a key press does to
/// a string is the same question in both front ends, so it lives in
/// `concord-ui` and is named here for the terminal code that already used it.
pub use concord_ui::text_input;
// Public so the GUI resolves the same theme.toml the TUI does; only
// `theme::external` is reachable from outside.
/// Resolved in `concord-ui` so a colour edited once in `theme.toml` looks the
/// same in both front ends.
pub use concord_ui::theme;
mod ui;

use tokio::sync::{mpsc, watch};

use concord::{
    AppError, Result,
    config::{AppOptions, KeymapOptions, ThemeOptions},
    discord::{AppCommand, DiscordAuthSession, DiscordClient, SequencedAppEvent, SnapshotRevision},
};

pub use runtime::DashboardExit;

pub fn validate_keymap_options(keymap_options: &KeymapOptions) -> Result<()> {
    keybindings::KeyBindings::try_from_options(keymap_options)
        .map(|_| ())
        .map_err(AppError::InvalidKeymapConfig)
}

/// Resolves `theme_options` against the built-in defaults and returns any
/// per-field warnings, without applying the result. Theme values never fail
/// startup outright (an unparseable color just falls back), so this is a
/// report, not a pass/fail check like [`validate_keymap_options`].
pub fn theme_options_warnings(theme_options: &ThemeOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    theme::Theme::from_options(theme_options, &mut warnings);
    warnings
}

pub fn initialize_theme(theme_options: &ThemeOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    let resolved = theme::Theme::from_options(theme_options, &mut warnings);
    theme::init(resolved);
    warnings
}

pub async fn prompt_login_with_auth_session(
    notice: Option<String>,
    auth_session: DiscordAuthSession,
) -> Result<String> {
    login::prompt_login(notice, auth_session).await
}

pub async fn run(
    effects: mpsc::Receiver<SequencedAppEvent>,
    snapshots: watch::Receiver<SnapshotRevision>,
    commands: mpsc::Sender<AppCommand>,
    client: DiscordClient,
    mut config_warnings: Vec<String>,
) -> Result<DashboardExit> {
    let options = match concord::config::load_options_with_warnings() {
        Ok((options, warnings)) => {
            config_warnings.extend(warnings);
            options
        }
        Err(error) => {
            concord::logging::error("config", format!("failed to load config: {error}"));
            AppOptions::default()
        }
    };
    run_with_options(
        effects,
        snapshots,
        commands,
        client,
        options,
        config_warnings,
    )
    .await
}

pub(crate) async fn run_with_options(
    mut effects: mpsc::Receiver<SequencedAppEvent>,
    mut snapshots: watch::Receiver<SnapshotRevision>,
    commands: mpsc::Sender<AppCommand>,
    client: DiscordClient,
    options: AppOptions,
    config_warnings: Vec<String>,
) -> Result<DashboardExit> {
    let mut terminal = ratatui::init();
    let _restore_guard = terminal::TerminalRestoreGuard::new();

    runtime::run_dashboard(
        &mut terminal,
        &mut effects,
        &mut snapshots,
        commands,
        client,
        options,
        config_warnings,
    )
    .await
}
