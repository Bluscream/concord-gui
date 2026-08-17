//! Where cached Discord data lives between runs.
//!
//! The client holds everything in memory today, so a restart starts blank and
//! stays blank until the gateway has finished sending a READY payload. That is
//! seconds of nothing on a large account, and nothing at all with no network.
//!
//! Two backends: a file beside the client's other state, or a MariaDB or MySQL
//! server that several clients on a network can share.

pub mod concurrent;
pub mod dsn;
pub mod schema;
#[cfg(feature = "storage")]
pub mod store;

pub use concurrent::{Absence, Revision, should_write};
pub use dsn::{DsnProblem, StorageBackend};
pub use schema::{Dialect, SCHEMA_VERSION};
#[cfg(feature = "storage")]
pub use store::{CachedGuild, CachedMessage, CachedUser, Store};
