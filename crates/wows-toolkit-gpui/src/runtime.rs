//! Tokio bridge.
//!
//! GPUI drives its own non-tokio executor, so a bare sqlx future awaited on a
//! GPUI task panics for want of a reactor. A process-wide runtime lives in a
//! GPUI global; work is submitted to it and the result crosses back through
//! GPUI's own background pool, where it can be awaited like any other `Task`.

use gpui_kit::AppContext as _;
use gpui_kit::{App, AsyncApp, Global, Task};
use std::future::Future;
use tokio::runtime::{Builder, Handle, Runtime};
use tokio::task::JoinHandle;

/// Worker threads for the shared runtime. What is submitted here is sqlx and
/// reqwest work: the settings and index reads, the expected-values refresh,
/// and the live match-stats lookup. Replay parsing, the directory scan and
/// ship loading are CPU-bound and run on GPUI's own pool via
/// `background_spawn`, not here, so this stays sized for a handful of
/// concurrent awaits rather than for throughput.
const WORKER_THREADS: usize = 2;

#[derive(Debug, thiserror::Error)]
pub enum TokioError {
    #[error("the tokio runtime could not be built")]
    Build(#[source] std::io::Error),
    #[error("the tokio task did not complete")]
    Join(#[from] tokio::task::JoinError),
}

/// Owns the runtime for the life of the process. Held as a GPUI global so the
/// runtime outlives every task submitted to it.
struct TokioRuntime(Runtime);

impl Global for TokioRuntime {}

/// Installs the runtime. Call once, during app startup, before any `spawn`.
pub fn init(cx: &mut App) -> Result<(), TokioError> {
    let runtime =
        Builder::new_multi_thread().worker_threads(WORKER_THREADS).enable_all().build().map_err(TokioError::Build)?;
    cx.set_global(TokioRuntime(runtime));
    Ok(())
}

/// Aborts the tokio task when the GPUI-side `Task` is dropped.
///
/// A bare `JoinHandle` detaches on drop, so without this a cancelled await
/// leaves its future running against a runtime that is itself about to be
/// dropped with the window -- for the settings read, an `SqlitePool` still
/// working while the app quits.
struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Runs `future` on the tokio runtime, yielding a GPUI `Task` for its result.
///
/// Dropping the returned `Task` aborts `future`. Panics if `init` has not run,
/// which is a startup-order bug rather than a runtime condition.
pub fn spawn<R>(cx: &AsyncApp, future: impl Future<Output = R> + Send + 'static) -> Task<Result<R, TokioError>>
where
    R: Send + 'static,
{
    let handle: Handle = cx.update(|cx| cx.global::<TokioRuntime>().0.handle().clone());
    let mut guard = AbortOnDrop(handle.spawn(future));
    cx.background_spawn(async move { (&mut guard.0).await.map_err(TokioError::Join) })
}
