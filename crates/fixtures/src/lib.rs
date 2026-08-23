//! A fake Discord, for developing and testing a front end against.
//!
//! A whole populated state - servers, channels, members, messages, profiles -
//! and event builders for the things a gateway would send. Enough that a front
//! end can be run, clicked through and screenshotted with no network, no
//! account and no rate limit.
//!
//! It lives in a crate of its own so both front ends get the same fake world.
//! When it was inside the core, only the core's own tests could see it, and
//! each front end grew its own half-overlapping pile of hand-built state that
//! agreed with neither the other nor the real payloads.
//!
//! Not behind a `cfg(test)`: a front end depends on this normally and decides
//! with its own feature whether to include it, which is what lets the actual
//! binary run against the fake rather than only the test harness.

pub mod backend;

pub use backend::{Emission, FakeBackend};

/// Gateway event builders.
///
/// Re-exported from the core for the same reason as [`world`]: the core's own
/// tests use them, and a crate cannot depend on something that depends on it.
pub use concord::discord::test_builders as events;

/// The fake world itself.
///
/// Re-exported from the core rather than living here: building a state from
/// nothing means reaching inside the caches, and those are private on purpose:
/// state is meant to change by applying events, which keeps every write on the
/// path the real client uses. Opening all of that permanently, so this
/// crate could hold the builder, would cost more than it buys. What matters is
/// that a front end has one place to import from, and it does.
pub use concord::discord::fixtures as world;

pub use concord::discord::fixtures::{demo_state, demo_user_id};
