//! Where cached Discord data lives between runs.
//!
//! The client holds everything in memory today, so a restart starts blank and
//! stays blank until the gateway has finished sending a READY payload. That is
//! seconds of nothing on a large account, and nothing at all with no network.
//!
//! Two backends: a file beside the client's other state, or a MariaDB or MySQL
//! server that several clients on a network can share.
//!
//! A crate of its own, between the core and the front ends. The core keeps its
//! state in memory as it always did and knows nothing about databases; this
//! attaches to it through `ClientExtension`, watching events and feeding back
//! what it has. A build that wants no cache simply does not depend on this.

pub mod concurrent;
pub mod dsn;
pub mod extension;
pub mod persist;
pub mod replay;
pub mod schema;
pub mod store;

pub use concurrent::{Absence, Revision, should_write};
pub use dsn::{DsnProblem, StorageBackend};
pub use extension::{CacheExtension, open_from_config};
pub use persist::{CACHED_MEMBERS_PER_GUILD, Write};
pub use schema::{Dialect, SCHEMA_VERSION};
pub use store::{
    CachedAttachment, CachedChannel, CachedGuild, CachedMember, CachedMessage, CachedSticker,
    CachedUser, NEWER_STORE_MARKER, Store,
};
