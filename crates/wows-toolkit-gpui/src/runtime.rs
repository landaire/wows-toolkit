//! Tokio bridge.
//!
//! GPUI drives its own non-tokio executor, so a bare sqlx future awaited on a
//! GPUI task panics for want of a reactor. A process-wide runtime lives in a
//! GPUI global; work is submitted to it and the result crosses back through
//! GPUI's own background pool, where it can be awaited like any other `Task`.

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::AsyncApp;
use gpui_kit::Global;
use gpui_kit::Task;
use std::future::Future;
use tokio::runtime::Builder;
use tokio::runtime::Handle;
use tokio::runtime::Runtime;
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
struct TokioRuntime(std::sync::Arc<Runtime>);

impl Global for TokioRuntime {}

/// Installs the runtime. Call once, during app startup, before any `spawn`.
pub fn init(cx: &mut App) -> Result<(), TokioError> {
    let runtime =
        Builder::new_multi_thread().worker_threads(WORKER_THREADS).enable_all().build().map_err(TokioError::Build)?;
    cx.set_global(TokioRuntime(std::sync::Arc::new(runtime)));
    Ok(())
}

/// The shared runtime itself, for a caller that owns long-lived work on it
/// rather than a single future.
///
/// `None` before [`init`] has run. The collab session takes this: its peer
/// task runs for the life of the session and spawns its own work, so it needs
/// the runtime rather than one submission to it.
pub fn runtime(cx: &App) -> Option<std::sync::Arc<Runtime>> {
    cx.try_global::<TokioRuntime>().map(|held| std::sync::Arc::clone(&held.0))
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

/// Drives a future that is not `Send` to completion on one thread, yielding a
/// GPUI `Task` for its result.
///
/// [`spawn`] needs a `Send` future because tokio may move it between workers.
/// Some of `wows_data_mgr`'s download futures hold state across an await that
/// cannot cross threads, so they are built and driven on a single GPUI
/// background thread with the tokio reactor entered, which is how the egui app
/// drives the same calls. `make` builds the future there rather than taking
/// one built by the caller, so nothing non-`Send` has to reach the thread.
pub fn block_on<R, F>(cx: &AsyncApp, make: impl FnOnce() -> F + Send + 'static) -> Task<R>
where
    R: Send + 'static,
    F: Future<Output = R>,
{
    let handle: Handle = cx.update(|cx| cx.global::<TokioRuntime>().0.handle().clone());
    cx.background_spawn(async move { handle.block_on(make()) })
}
