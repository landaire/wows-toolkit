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

/// Worker threads for the shared runtime. Tokio's own default of two starves
/// the sqlx pool when the replay index and the settings load overlap.
const WORKER_THREADS: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum TokioError {
    #[error("the tokio runtime is not initialized")]
    NotInitialized,
    #[error("the tokio runtime could not be built: {0}")]
    Build(std::io::Error),
    #[error("the tokio task did not complete: {0}")]
    Join(#[from] tokio::task::JoinError),
}

/// Owns the runtime for the life of the process. Held as a GPUI global so the
/// runtime outlives every task submitted to it.
struct TokioRuntime(Runtime);

impl Global for TokioRuntime {}

/// Installs the runtime. Call once, during app startup, before any `spawn`.
pub fn init(cx: &mut App) -> Result<(), TokioError> {
    let runtime = Builder::new_multi_thread()
        .worker_threads(WORKER_THREADS)
        .enable_all()
        .build()
        .map_err(TokioError::Build)?;
    cx.set_global(TokioRuntime(runtime));
    Ok(())
}

fn handle(cx: &AsyncApp) -> Option<Handle> {
    cx.update(|cx| cx.try_global::<TokioRuntime>().map(|rt| rt.0.handle().clone()))
}

/// Runs `future` on the tokio runtime, yielding a GPUI `Task` for its result.
pub fn spawn<R>(cx: &AsyncApp, future: impl Future<Output = R> + Send + 'static) -> Task<Result<R, TokioError>>
where
    R: Send + 'static,
{
    let Some(handle) = handle(cx) else {
        return cx.background_spawn(async move { Err(TokioError::NotInitialized) });
    };
    let join = handle.spawn(future);
    cx.background_spawn(async move { join.await.map_err(TokioError::Join) })
}
