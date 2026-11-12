//! The process-wide tokio runtime the desktop front ends submit sqlx and
//! reqwest work to.

use tokio::runtime::Builder;
use tokio::runtime::Runtime;

#[derive(Debug, thiserror::Error)]
#[error("the tokio runtime could not be built")]
pub struct BuildError(#[source] std::io::Error);

/// Builds the shared runtime. A front end builds one at startup and keeps it
/// for the life of the process.
pub fn build() -> Result<Runtime, BuildError> {
    // What is submitted here is sqlx and reqwest work: the settings and index
    // reads, the expected-values refresh, and the live match-stats lookup.
    // Replay parsing, the directory scan and ship loading are CPU-bound and run
    // on the front end's own pool, so this stays sized for a handful of
    // concurrent awaits rather than for throughput.
    const WORKER_THREADS: usize = 2;

    Builder::new_multi_thread().worker_threads(WORKER_THREADS).enable_all().build().map_err(BuildError)
}
