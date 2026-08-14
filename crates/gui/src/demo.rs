//! Isolated Demo / Fixture Mode helper module.
//!
//! Encapsulates all offline synthetic state loading, token checks, and demo session
//! setup. All demo-specific code is isolated here so it does not clutter core session
//! or workspace UI logic, and can be cleanly disabled or removed at any time.

#[cfg(feature = "fixtures")]
use std::sync::Arc;
#[cfg(feature = "fixtures")]
use concord::discord::AppCommand;
use tokio::sync::mpsc;
use crate::session::{SessionHandle, Update};

/// Check if the given token string is a demo/fixture token.
pub fn is_demo_token(token: &str) -> bool {
    #[cfg(feature = "fixtures")]
    {
        concord::discord::fixtures::is_fixture_token(token)
    }
    #[cfg(not(feature = "fixtures"))]
    {
        let _ = token;
        false
    }
}

/// Attempt to spawn an offline demo session if the token matches a demo token.
///
/// Returns `Some(Ok((updates_rx, handle)))` if a demo session was handled, or `None` if
/// the token is a standard live connection token.
pub fn try_spawn_demo(
    token: &str,
) -> Option<anyhow::Result<(mpsc::UnboundedReceiver<Update>, SessionHandle)>> {
    #[cfg(feature = "fixtures")]
    if is_demo_token(token) {
        let (updates_tx, updates_rx) = mpsc::unbounded_channel();
        let (commands_tx, mut commands_rx) = mpsc::channel::<AppCommand>(64);

        let demo_state = Arc::new(concord::discord::fixtures::demo_state());
        let _ = updates_tx.send(Update::State(demo_state.clone()));
        let _ = updates_tx.send(Update::Event(
            Box::new(concord::discord::AppEvent::Ready {
                user: "test-account".to_string(),
                user_id: None,
            }),
            demo_state,
        ));

        // Commands are drained and dropped in offline demo mode.
        std::thread::spawn(move || while commands_rx.blocking_recv().is_some() {});

        return Some(Ok((
            updates_rx,
            SessionHandle {
                commands: commands_tx,
            },
        )));
    }

    let _ = token;
    None
}
