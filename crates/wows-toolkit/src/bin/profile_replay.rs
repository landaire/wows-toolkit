//! Per-stage wall-clock breakdown of the replay load path.
//!
//! Usage: profile_replay [replay_dir] [limit]
//!   cargo run --profile profiling --features profile-bins --bin profile_replay
//!
//! Game data locations come from the environment so the binary is not tied to
//! one machine:
//!   WOWS_DIR         live install (default E:\WoWs\World_of_Warships)
//!   WOWS_BUILDS_DIR  dumped-build archive (default G:\wows_builds)
//! Replays whose build is in neither are skipped and listed at the end.

#[cfg(all(feature = "profile-bins", feature = "mimalloc"))]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "profile-bins")]
fn main() {
    use std::path::PathBuf;

    // Data resolution reports which dump answered for a build through tracing,
    // which is worth seeing when a run is slower or emptier than expected.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let wows_dir = std::env::var("WOWS_DIR").unwrap_or_else(|_| r"E:\WoWs\World_of_Warships".to_string());
    let dump_dir = std::env::var("WOWS_BUILDS_DIR").unwrap_or_else(|_| r"G:\wows_builds".to_string());

    let replay_dir =
        std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(&wows_dir).join("replays"));
    let limit = std::env::args().nth(2).and_then(|s| s.parse().ok());

    wows_toolkit::profiling::run(PathBuf::from(wows_dir), dump_dir, replay_dir, limit);
}

#[cfg(not(feature = "profile-bins"))]
fn main() {
    eprintln!("rebuild with --features profile-bins");
}
