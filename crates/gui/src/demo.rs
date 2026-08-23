//! Running the GPUI client against a fake Discord.
//!
//! The fake itself lives in `concord-fixtures`, shared with the terminal
//! client. What is left here is the wiring: turning what the fake reports into
//! the `Update` stream this front end already knows how to draw.

#[cfg(feature = "fixtures")]
use std::sync::Arc;

#[cfg(feature = "fixtures")]
use concord::discord::AppCommand;
use tokio::sync::mpsc;

// Named in `try_spawn_demo`'s signature whether or not the fake is compiled
// in, so the caller does not need a second code path for a build without it.
use crate::session::{SessionHandle, Update};

/// Whether this token asks for the fake rather than a real account.
pub fn is_demo_token(token: &str) -> bool {
    token.trim().eq_ignore_ascii_case("demo")
}

#[cfg(feature = "fixtures")]
fn forward(
    updates: &mpsc::UnboundedSender<Update>,
    emissions: Vec<concord_fixtures::Emission>,
) -> bool {
    use concord_fixtures::Emission;

    for emission in emissions {
        let sent = match emission {
            Emission::Event(event, state) => updates.send(Update::Event(event, Arc::new(state))),
            Emission::State(state) => updates.send(Update::State(Arc::new(state))),
        };
        if sent.is_err() {
            return false;
        }
    }
    true
}

pub fn try_spawn_demo(
    token: &str,
) -> Option<anyhow::Result<(mpsc::UnboundedReceiver<Update>, SessionHandle)>> {
    #[cfg(feature = "fixtures")]
    if is_demo_token(token) {
        let (updates_tx, updates_rx) = mpsc::unbounded_channel();
        let (commands_tx, mut commands_rx) = mpsc::channel::<AppCommand>(64);

        let mut backend = concord_fixtures::FakeBackend::new();
        let _ = updates_tx.send(Update::State(Arc::new(backend.state().clone())));
        let _ = updates_tx.send(Update::Event(
            Box::new(concord::discord::AppEvent::Ready {
                user: "test-account".to_string(),
                user_id: Some(backend.user_id()),
            }),
            Arc::new(backend.state().clone()),
        ));

        // Runs on the shared runtime rather than a bare thread so it can wait
        // on a timer as well as on commands: the canned reply needs to arrive
        // after a pause, not instantly.
        crate::runtime::spawn(async move {
            loop {
                let next_delay = backend
                    .next_deadline()
                    .map(|at| at.saturating_duration_since(std::time::Instant::now()));

                tokio::select! {
                    command = commands_rx.recv() => {
                        let Some(command) = command else { break };
                        if !forward(&updates_tx, backend.handle(command)) {
                            break;
                        }
                    }
                    // Only armed when something is scheduled.
                    _ = tokio::time::sleep(next_delay.unwrap_or(std::time::Duration::MAX)),
                        if next_delay.is_some() =>
                    {
                        if !forward(&updates_tx, backend.fire_due()) {
                            break;
                        }
                    }
                }
            }
        })?;

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
