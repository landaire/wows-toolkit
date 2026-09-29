//! Live-heap totals sampled out of the running app.
//!
//! What this answers is how much of the process's memory is the Rust heap, and
//! across how many blocks, so this port's footprint can be set beside the egui
//! app's (`wows_toolkit`'s `main.rs` samples the same way, into the same file).
//!
//! [`start`] samples on a timer, which says how the heap moves. [`mark`]
//! samples on demand at a named point, which says what moved it: a timed
//! sample can only be read back against a log line that happens to sit near
//! it, and the startup steps that cost the most log nothing.
//!
//! `dhat::Profiler::drop` does not converge here: it allocates without bound
//! while symbolizing a live heap this size with the other threads still
//! running. The profiler is therefore leaked and no `dhat-heap.json` is
//! written; `HeapStats::get` needs no symbolization and is where the totals
//! come from.
//!
//! Build with `--features dhat-heap`, run, then read `dhat-markers.log`. Both
//! functions compile to nothing without it, so call sites need no `cfg`.
//! `DHAT_TRIM` sets the retained backtrace depth and `DHAT_RUN_SECS` how long
//! to let the app settle before the first timed sample.

#[cfg(feature = "dhat-heap")]
mod enabled {
    use std::io::Write as _;

    /// Where samples are appended. A windowed build has no console on Windows.
    const MARKERS: &str = "dhat-markers.log";

    /// Backtrace depth retained when `DHAT_TRIM` does not say otherwise.
    const DEFAULT_TRIM: usize = 16;

    /// Seconds to let the app settle before the first timed sample, when
    /// `DHAT_RUN_SECS` does not say otherwise.
    const DEFAULT_SETTLE_SECS: u64 = 25;

    /// Seconds between timed samples once sampling has started.
    const SAMPLE_INTERVAL_SECS: u64 = 3;

    fn write_line(line: &str) {
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(MARKERS) {
            let _ = writeln!(file, "{line}");
        }
    }

    fn env_var<T: std::str::FromStr>(name: &str) -> Option<T> {
        std::env::var(name).ok()?.parse().ok()
    }

    /// Milliseconds since the profiler started, so a mark can be placed
    /// against the timed samples without matching wall clocks.
    fn elapsed_ms() -> u128 {
        static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        STARTED.get_or_init(std::time::Instant::now).elapsed().as_millis()
    }

    pub fn mark(label: &str) {
        let stats = dhat::HeapStats::get();
        write_line(&format!(
            "mark at_ms={} label={label} curr_bytes={} curr_blocks={} max_bytes={} max_blocks={}",
            elapsed_ms(),
            stats.curr_bytes,
            stats.curr_blocks,
            stats.max_bytes,
            stats.max_blocks
        ));
    }

    pub fn start() {
        let trim = env_var("DHAT_TRIM").unwrap_or(DEFAULT_TRIM);
        let profiler = dhat::Profiler::builder().trim_backtraces(Some(trim)).build();
        // Leaked rather than dropped: see the module note.
        std::mem::forget(profiler);
        elapsed_ms();
        write_line("profiler_started");

        let settle = env_var("DHAT_RUN_SECS").unwrap_or(DEFAULT_SETTLE_SECS);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(settle));
            for round in 0.. {
                let stats = dhat::HeapStats::get();
                write_line(&format!(
                    "heapstats round={round} at_ms={} curr_bytes={} curr_blocks={} max_bytes={} max_blocks={} total_bytes={} total_blocks={}",
                    elapsed_ms(),
                    stats.curr_bytes,
                    stats.curr_blocks,
                    stats.max_bytes,
                    stats.max_blocks,
                    stats.total_bytes,
                    stats.total_blocks
                ));
                std::thread::sleep(std::time::Duration::from_secs(SAMPLE_INTERVAL_SECS));
            }
        });
    }
}

/// Starts the profiler and the thread that samples it. Call before anything
/// else in `main`, so the totals cover the whole process.
pub fn start() {
    #[cfg(feature = "dhat-heap")]
    enabled::start();
}

/// Samples the live heap at a named point. `label` is a short step name with
/// no spaces, so the log stays one key-value line per sample.
pub fn mark(_label: &str) {
    #[cfg(feature = "dhat-heap")]
    enabled::mark(_label);
}
