//! Work that can hold a thread for seconds: replay walks and bakes, game data loads, directory
//! passes, downloads, bulk writes.
//!
//! zgui's [`blocking`] runs on a pool of at most four threads that the whole process shares, and
//! the quick probes the views wait on (a folder check, a listing read, a thumbnail) run there too.
//! Four long jobs on that pool stall every probe behind them, so long jobs run on a pool of their
//! own that starts threads as jobs arrive and lets idle ones exit.

use std::pin::Pin;
use std::sync::LazyLock;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use zgui::task::Background;
use zgui::task::blocking;

/// A value being computed on a long-job thread, awaited on the UI thread.
///
/// Dropping it before it resolves abandons the result; the work runs on. Work that should stop
/// early takes a cancel flag of its own.
#[must_use = "a long job's result is only delivered when it is awaited"]
pub struct LongJob<T> {
    running: Running<T>,
}

impl<T> Future for LongJob<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let joined = match &mut self.get_mut().running {
            Running::Own(handle) => std::task::ready!(Pin::new(handle).poll(cx)),
            Running::Shared(background) => return Pin::new(background).poll(cx),
        };

        // A panic in the job is raised where its result is awaited, as zgui's `blocking` does.
        match joined {
            Ok(value) => Poll::Ready(value),
            Err(err) if err.is_panic() => std::panic::resume_unwind(err.into_panic()),
            Err(err) => panic!("a long job did not produce a value: {err}"),
        }
    }
}

enum Running<T> {
    Own(JoinHandle<T>),
    /// The long-job pool did not start, so the work shares zgui's pool.
    Shared(Background<T>),
}

/// Runs `work` on a long-job thread, and resolves on the UI thread with what it produced.
///
/// For work that may hold its thread for more than a moment. A stat, a canonicalize or one small
/// read belongs in zgui's [`blocking`].
pub fn long_job<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> LongJob<T> {
    let running = match POOL.as_ref() {
        Some(pool) => Running::Own(pool.spawn_blocking(work)),
        None => Running::Shared(blocking(work)),
    };

    LongJob { running }
}

/// The long-job pool, started on first use. `None` when it could not be built.
///
/// A runtime that is never dropped, used only for its blocking pool: dropping a runtime waits for
/// the blocking work still running on it, and a job may be minutes from done.
static POOL: LazyLock<Option<Runtime>> = LazyLock::new(|| {
    // A ceiling against a burst, not a target: a job past it waits for a long-job thread, never
    // for zgui's pool.
    const MAX_THREADS: usize = 16;
    // Long enough that one job after another reuses a thread.
    const KEEP_ALIVE: Duration = Duration::from_secs(30);

    tokio::runtime::Builder::new_current_thread()
        .max_blocking_threads(MAX_THREADS)
        .thread_keep_alive(KEEP_ALIVE)
        .thread_name("wt-long-job")
        .build()
        .inspect_err(|err| tracing::error!(%err, "the long-job pool did not start; long jobs share zgui's pool"))
        .ok()
});
