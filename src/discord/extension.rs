//! A seam for things that watch the client without being part of it.
//!
//! The core keeps its state in memory and answers from the gateway. Anything
//! that wants those events to outlive the process - an offline cache, and
//! later a merged view across several accounts - lives outside this crate and
//! attaches here.
//!
//! Deliberately narrow. The core knows an extension may want to see events and
//! may want to put some back; it knows nothing about databases, files or
//! accounts. That is what keeps a build with no cache from paying for one.

use std::sync::Arc;

use tokio::sync::mpsc;

use super::events::AppEvent;
use super::ids::{Id, marker::ChannelMarker};

/// Something that observes the client and may feed events back into it.
///
/// Every method is synchronous and must not block: they are called from the
/// event funnel, so work that touches a disk or a network belongs on a task
/// the extension spawns itself.
pub trait ClientExtension: Send + Sync + 'static {
    /// Called once, with the handle for putting events back in.
    fn attach(&self, injector: EventInjector);

    /// Every event, after the core has applied it.
    fn observe(&self, event: &AppEvent);

    /// A channel the user just opened.
    ///
    /// Separate from `observe` because opening a channel is a command rather
    /// than an event: nothing is published until the fetch answers, which is
    /// exactly the gap an extension might fill.
    fn channel_opened(&self, channel_id: Id<ChannelMarker>);
}

/// How an extension puts an event back into the client.
///
/// A channel rather than a direct call: an extension holding the client would
/// be a reference cycle, and the events it injects should go through the same
/// funnel as the gateway's rather than around it.
#[derive(Clone, Debug)]
pub struct EventInjector {
    events_tx: mpsc::UnboundedSender<AppEvent>,
}

impl EventInjector {
    pub(crate) fn new(events_tx: mpsc::UnboundedSender<AppEvent>) -> Self {
        Self { events_tx }
    }

    /// Publish an event as though it had arrived from the gateway.
    ///
    /// Dropped silently if the client is gone, which is the shutdown case: an
    /// extension still draining its own work when the client goes away should
    /// not be an error anybody sees.
    pub fn inject(&self, event: AppEvent) {
        let _ = self.events_tx.send(event);
    }
}

/// Where the client keeps its extension, if it has one.
///
/// A named type rather than a bare `Option` so it can carry its own `Debug`.
/// Requiring `Debug` of every extension would put whatever one holds into any
/// log that prints the client, and one of them holds a database connection
/// string with a password in it.
#[derive(Clone, Default)]
pub(crate) struct AttachedExtension(pub(crate) Option<Arc<dyn ClientExtension>>);

impl std::fmt::Debug for AttachedExtension {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(if self.0.is_some() { "attached" } else { "none" })
    }
}
