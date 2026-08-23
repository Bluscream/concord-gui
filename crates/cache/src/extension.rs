//! The cache as the core sees it.
//!
//! Everything here used to live in `DiscordClient`, which meant the core
//! carried a database driver whether or not anybody wanted one. It attaches
//! through `ClientExtension` instead: the core offers events and asks nothing
//! about what happens to them, and this crate answers with what it has.

use std::sync::Arc;

use concord::discord::AppEvent;
use concord::discord::ids::{Id, marker::ChannelMarker};
use concord::discord::{ClientExtension, EventInjector};
use tokio::sync::Mutex;

use crate::store::Store;

/// How many cached messages a channel is drawn with before the fetch lands.
///
/// Matches the page size the client asks Discord for, so the cached view and
/// the fetched one are the same length and the list does not visibly resize.
const CACHED_MESSAGE_REPLAY_LIMIT: u32 = 50;

/// The offline cache, attached to a running client.
pub struct CacheExtension {
    store: Arc<Store>,
    /// Set once, when the core attaches. Behind a lock because `attach` takes
    /// `&self`: the core hands this out from a shared reference, and an
    /// extension that demanded `&mut` would have to be attached before it
    /// could be shared.
    injector: Mutex<Option<EventInjector>>,
}

impl CacheExtension {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            injector: Mutex::new(None),
        }
    }

    /// Bound the cache, once per start.
    ///
    /// At startup rather than after each write: pruning is a whole-table scan,
    /// and running it per message would spend more time evicting than caching.
    /// Overshooting between runs costs disk, which is the cheap direction.
    pub async fn prune(&self) {
        let _ = self
            .store
            .prune_messages(CACHED_MESSAGE_REPLAY_LIMIT * 4)
            .await;
        // After the messages, not before: pruning them is what orphans the
        // attachments, so the reverse order would leave a run's worth behind
        // every time.
        let _ = self.store.prune_orphan_attachments().await;
        let _ = self.store.prune_orphans().await;
    }
}

impl ClientExtension for CacheExtension {
    fn attach(&self, injector: EventInjector) {
        // `try_lock` rather than blocking: attach is called from the core's
        // constructor path, nothing else holds this yet, and blocking a
        // runtime thread to wait for a lock with no other holder would be a
        // deadlock waiting for a reason to happen.
        if let Ok(mut slot) = self.injector.try_lock() {
            *slot = Some(injector.clone());
        }

        // Hydration starts here rather than being something the front end
        // remembers to call. A cache that attached and drew nothing would look
        // exactly like an empty cache, which is the kind of bug that survives
        // for months because nothing about it looks wrong.
        let store = Arc::clone(&self.store);
        tokio::spawn(async move {
            for event in crate::replay::guild_events(&store).await {
                injector.inject(event);
            }
        });
    }

    fn observe(&self, event: &AppEvent) {
        let writes = crate::persist::writes_for(event);
        if writes.is_empty() {
            return;
        }
        let store = Arc::clone(&self.store);
        // Spawned because `observe` is called from the event funnel: a write
        // that waited on a disk would hold up every event behind it.
        tokio::spawn(async move {
            for write in writes {
                let _ = store.apply(&write).await;
            }
        });
    }

    fn channel_opened(&self, channel_id: Id<ChannelMarker>) {
        let Ok(injector) = self.injector.try_lock().map(|slot| slot.clone()) else {
            return;
        };
        let Some(injector) = injector else {
            return;
        };
        let store = Arc::clone(&self.store);
        tokio::spawn(async move {
            if let Some(event) =
                crate::replay::channel_history(&store, channel_id, CACHED_MESSAGE_REPLAY_LIMIT)
                    .await
            {
                injector.inject(event);
            }
        });
    }
}

/// Open the cache the config asks for, ready to attach.
///
/// Here rather than in a front end so both of them get the same behaviour from
/// one place: which backend, where the default file lives, and what to do when
/// it will not open are all decisions about caching rather than about drawing.
///
/// A cache that will not open is `None` rather than an error. This client
/// worked without one before the cache existed, and refusing to start because
/// somebody's MariaDB is down would make an optional backend worse than no
/// backend at all.
pub async fn open_from_config() -> Option<Arc<dyn ClientExtension>> {
    let options = concord::config::load_options().ok()?.storage;
    if !options.enabled {
        return None;
    }

    let backend = if options.dsn.trim().is_empty() {
        crate::StorageBackend::Sqlite {
            path: concord::support::paths::state_dir()?.join("cache.db"),
        }
    } else {
        match crate::StorageBackend::parse(&options.dsn) {
            Ok(backend) => backend,
            // Named rather than swallowed: a typo in a connection string is
            // something somebody can fix, unlike a store that is merely
            // unreachable.
            Err(problem) => {
                concord::logging::error(
                    "storage",
                    format!("could not read the storage setting: {problem}"),
                );
                return None;
            }
        }
    };

    let store = match Store::open(&backend).await {
        Ok(store) => Arc::new(store),
        Err(error) => {
            let message = format!("no cache this run, {backend} could not be opened: {error}");
            // Only one of these is worth a person's attention: a store written
            // by a newer client means another client on a shared store has
            // moved ahead, which somebody can act on. Every other reason is
            // transient or local.
            if error.to_string().contains(crate::NEWER_STORE_MARKER) {
                concord::logging::error("storage", message);
            } else {
                concord::logging::debug("storage", message);
            }
            return None;
        }
    };

    concord::logging::debug("storage", format!("caching to {backend}"));
    let extension = Arc::new(CacheExtension::new(store));
    extension.prune().await;
    Some(extension)
}
