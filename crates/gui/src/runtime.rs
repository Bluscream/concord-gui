//! A single shared tokio runtime for background work.
//!
//! GPUI's main thread has no tokio runtime, so calling `tokio::spawn` from a
//! view or an action handler panics with "there is no reactor running". That
//! failure is invisible to the type checker and only fires at the moment of
//! use, and it has already reached this branch twice - once in the command
//! sink and once across all four auth spawners.
//!
//! This module is the guard: [`spawn`] is always safe to call from any thread,
//! including GPUI's, because it targets a runtime this module owns rather than
//! an ambient one. Code that needs background work should call it instead of
//! `tokio::spawn`.
//!
//! It also removes a real cost. Each auth call previously built its own
//! multi-threaded runtime on its own OS thread, so a single password login
//! with an MFA step spun up two complete runtimes and tore them down again.

use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinHandle;

static RUNTIME: OnceLock<Option<Runtime>> = OnceLock::new();

/// The shared runtime, or `None` if it could not be created.
///
/// Creation fails only under real resource pressure (thread or descriptor
/// exhaustion). Callers surface that as an error rather than panicking, so a
/// constrained machine degrades instead of crashing.
fn shared() -> Option<&'static Runtime> {
    RUNTIME
        .get_or_init(|| {
            // Two worker threads: this carries gateway I/O, auth flows and
            // media fetches, none of which are CPU-bound. The default (one per
            // core) would be wasteful for a chat client.
            Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("concord-bg")
                .build()
                .ok()
        })
        .as_ref()
}

/// Spawn a future on the shared runtime.
///
/// Safe from any thread, including GPUI's. Returns `None` only if the runtime
/// could not be created, which the caller should report rather than ignore.
pub fn spawn<F>(future: F) -> Option<JoinHandle<F::Output>>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    shared().map(|runtime| {
        let _guard = runtime.handle().enter();
        runtime.spawn(future)
    })
}

/// Run a future to completion on the shared runtime.
///
/// Only the tests use it: the UI never blocks its thread on a future, which is
/// the whole reason `spawn` exists. Kept because the tests need a way to drive
/// one to completion without a second runtime.
#[cfg(test)]
pub fn block_on<F: std::future::Future>(future: F) -> Option<F::Output> {
    shared().map(|runtime| runtime.block_on(future))
}

/// Install the shared runtime as the calling thread's ambient context.
///
/// Needed because several dependencies - zbus via ashpd and oo7, which GPUI
/// itself pulls in, and notify-rust - create D-Bus connections from whichever
/// thread calls them and then expect `tokio::spawn` to work. Without a context
/// on GPUI's main thread they panic with "there is no reactor running" from
/// inside a worker, which is neither our call site nor fixable there.
///
/// The returned guard must be held for as long as that thread runs; dropping
/// it removes the context.
pub fn enter() -> Option<tokio::runtime::EnterGuard<'static>> {
    shared().map(|runtime| runtime.enter())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_works_off_any_thread_without_an_ambient_runtime() {
        // This test thread has no tokio runtime, which is exactly the
        // situation GPUI's main thread is in. `tokio::spawn` would panic here;
        // this must not.
        let handle = spawn(async { 21 * 2 }).expect("runtime should be available");
        let result = block_on(async move { handle.await.unwrap() }).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn the_runtime_is_shared_not_rebuilt() {
        let first = shared().map(|runtime| runtime as *const Runtime);
        let second = shared().map(|runtime| runtime as *const Runtime);
        assert_eq!(first, second, "each call must reuse one runtime");
    }
}
