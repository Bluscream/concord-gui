//! Running the terminal client against a fake Discord.
//!
//! The fake itself lives in `concord-fixtures`, shared with the GPUI client.
//! What is here is the wiring, and it is a different shape from that one: this
//! front end draws from a `DiscordClient`, so rather than feed it a stream of
//! updates, the fake's events are published through a real client that never
//! opens a gateway. Everything downstream - the state store, the snapshot
//! revisions, the effect ordering - then behaves exactly as it does against
//! Discord, which is the point of testing against a fake at all.

use concord::discord::{AppCommand, DiscordClient};
use concord::{Result, Session};
use tokio::sync::mpsc;

/// Whether this token asks for the fake rather than a real account.
pub fn is_demo_token(token: &str) -> bool {
    token.trim().eq_ignore_ascii_case("demo")
}

/// A session backed by the fake, shaped like one backed by Discord.
///
/// The gateway task is the loop that answers commands, so the ordinary
/// shutdown path aborts it exactly as it would a real one.
pub fn start(theme_warnings: Vec<String>) -> Result<Session> {
    // Said plainly and at a level that shows without CONCORD_DEBUG: a run
    // against the fake and a run against a real account are otherwise
    // indistinguishable in the log, and telling them apart afterwards is the
    // first question anybody asks of it.
    concord::logging::info(
        "demo",
        "running against the built-in fake - no account, no network",
    );

    let client = DiscordClient::new("demo".to_owned())?;
    let mut backend = concord_fixtures::FakeBackend::new();

    // Seeded rather than replayed: the fake world is built by filling caches,
    // not by a history that happened. Everything after this arrives as events.
    client.seed_state(backend.state().clone());

    let effects = client.take_effects();
    let snapshots = client.subscribe_snapshots();
    let (commands_tx, mut commands_rx) = mpsc::channel::<AppCommand>(64);

    let publisher = client.clone();
    let gateway_task = tokio::spawn(async move {
        publisher
            .publish_event(concord::discord::AppEvent::Ready {
                user: "test-account".to_owned(),
                user_id: Some(backend.user_id()),
            })
            .await;

        loop {
            let next_delay = backend
                .next_deadline()
                .map(|at| at.saturating_duration_since(std::time::Instant::now()));

            let emissions = tokio::select! {
                command = commands_rx.recv() => {
                    let Some(command) = command else { break };
                    backend.handle(command)
                }
                // Only armed when something is scheduled, so an idle demo
                // costs nothing rather than waking on a timer forever.
                _ = tokio::time::sleep(next_delay.unwrap_or(std::time::Duration::MAX)),
                    if next_delay.is_some() =>
                {
                    backend.fire_due()
                }
            };

            for emission in emissions {
                // A state-only emission has nothing to publish: the fake's own
                // copy moved, and the client's copy moves with the next event.
                if let concord_fixtures::Emission::Event(event, _) = emission {
                    publisher.publish_event(*event).await;
                }
            }
        }
    });

    let command_task = tokio::spawn(async {});

    Ok(Session {
        effects,
        snapshots,
        commands: commands_tx,
        client,
        gateway_task,
        command_task,
        warnings: theme_warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_fake_session_comes_up_and_moves_the_state() {
        // The whole point: a client with no network that already has a world
        // and advances when the fake speaks. Watched through the snapshot
        // rather than the effect stream because `Ready` mutates state without
        // being delivered as an effect - asserting on effects here would be
        // asserting on the wrong channel and would hang rather than fail.
        //
        // The client builds an HTTP stack it never uses, and that needs a
        // crypto provider. `main` installs one before anything runs; a test
        // starts further in, so it installs its own.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let session = start(Vec::new()).expect("a fake session should start");
        let mut snapshots = session.snapshots.clone();

        tokio::time::timeout(std::time::Duration::from_secs(5), snapshots.changed())
            .await
            .expect("the fake should publish something")
            .expect("the snapshot channel should stay open");

        session.gateway_task.abort();
        session.command_task.abort();
    }

    #[test]
    fn only_the_demo_token_opens_the_fake() {
        // A real token that happened to start with "demo" must still go to
        // Discord, and the check is case-insensitive because nobody types a
        // sentinel carefully.
        assert!(is_demo_token("demo"));
        assert!(is_demo_token("  DEMO  "));
        assert!(!is_demo_token("demo-token"));
        assert!(!is_demo_token(""));
    }
}
