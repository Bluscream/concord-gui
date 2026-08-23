pub mod app;
pub mod config;
pub mod discord;
pub mod error;
pub mod i18n;
pub mod logging;
pub mod risk;
/// Paths and small helpers. Public because a front end, or the cache crate
/// beside it, needs to put its own files where this client keeps its state.
pub mod support;

/// Notification and voice sounds, shared by both front ends.
pub use support::notification_audio as sound;

pub use app::Session;
pub use discord::{AppEvent, DiscordClient};
pub use error::{AppError, Result};
pub(crate) use support::url_policy;
pub use support::{paths, token_store, version_check};
