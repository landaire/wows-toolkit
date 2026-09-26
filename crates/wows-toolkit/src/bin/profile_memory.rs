//! What the app's features hold after they have run.
//!
//! Usage: profile_memory <scenario> [count]
//!   scenarios: builds, tabs, tracker, unpacker, maps
//!   cargo run --profile profiling --features profile-bins --bin profile_memory -- builds 8
//!
//! With `--features profile-bins,dhat-heap` the live Rust heap is reported too.
//!
//! Game data locations come from the environment so the binary is not tied to
//! one machine:
//!   WOWS_DIR         live install (default E:\WoWs\World_of_Warships)
//!   WOWS_BUILDS_DIR  dumped-build archive (default G:\wows_builds)
//!   WOWS_REPLAY_DIR  replays to read (default <WOWS_DIR>\replays)

#[cfg(all(feature = "dhat-heap", not(target_arch = "wasm32")))]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(feature = "profile-bins")]
fn main() {
    use std::path::PathBuf;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    // Held for the whole run: dropping it writes a dhat-heap.json, which with
    // millions of live blocks does not converge (see docs/REPLAY_PARSE_PROFILE).
    // The per-step stats this bin prints do not need the dump.
    #[cfg(feature = "dhat-heap")]
    let profiler = dhat::Profiler::builder().trim_backtraces(Some(16)).build();

    let wows_dir = std::env::var("WOWS_DIR").unwrap_or_else(|_| r"E:\WoWs\World_of_Warships".to_string());
    let dump_dir = std::env::var("WOWS_BUILDS_DIR").unwrap_or_else(|_| r"G:\wows_builds".to_string());
    let replay_dir = std::env::var("WOWS_REPLAY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(&wows_dir).join("replays"));

    let scenario = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: profile_memory <{}> [count]", wows_toolkit::profiling::memory::SCENARIOS.join("|"));
        std::process::exit(2);
    });
    let count = std::env::args().nth(2).and_then(|arg| arg.parse().ok()).unwrap_or(8);

    wows_toolkit::profiling::memory::run(&scenario, PathBuf::from(wows_dir), dump_dir, replay_dir, count);

    #[cfg(feature = "dhat-heap")]
    std::mem::forget(profiler);
}

#[cfg(not(feature = "profile-bins"))]
fn main() {
    eprintln!("rebuild with --features profile-bins");
}
