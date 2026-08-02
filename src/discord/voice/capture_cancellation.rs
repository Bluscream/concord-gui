use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Stop signal for an in-flight stream capture preparation.
///
/// This lives outside `capture` because it is runtime plumbing rather than
/// capture machinery: `run_broadcast_capture_preparation` uses it to abandon a
/// preparation that is still waiting on the previous broadcast's cleanup, and
/// shutdown uses it to join that task promptly. Builds without the
/// `stream-broadcast` feature swap `capture` for a stub, so keeping one shared
/// implementation here is what stops the two from drifting apart.
#[derive(Clone, Default)]
pub(super) struct StreamCaptureCancellation {
    cancelled: Arc<AtomicBool>,
}

impl StreamCaptureCancellation {
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    #[cfg_attr(not(feature = "stream-broadcast"), allow(dead_code))]
    pub(super) fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }
}
