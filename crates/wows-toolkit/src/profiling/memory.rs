//! What each feature of the app is still holding once it has run, and whether
//! that memory comes back.
//!
//! The timing half of this module answers how long a load takes; this half
//! answers how much a feature keeps. Every scenario drives the same cache or
//! store the tab itself uses, samples the process after each step, and then
//! drops what it built. A figure that does not come back down is retention
//! rather than a working set the allocator is holding on to.
//!
//! Run through `src/bin/profile_memory.rs`:
//!   cargo run --profile profiling --features profile-bins --bin profile_memory -- <scenario> [count]
//! Adding `--features profile-bins,dhat-heap` reports the exact live Rust heap
//! beside the process counters.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use wowsunpack::data::Version;
use wowsunpack::vfs::VfsPath;

use crate::data::wows_data::BuildDataCache;
use crate::data::wows_data::ReplayDependencies;
use crate::data::wows_data::SharedBuildData;
use crate::ui::player_tracker::PlayerTracker;
use crate::ui::replay_parser::Replay;

const MIB: f64 = 1024.0 * 1024.0;

/// The scenarios [`run`] knows, for the usage line and the argument error.
pub const SCENARIOS: [&str; 8] = ["builds", "reload", "tabs", "parses", "tracker", "unpacker", "maps", "armor"];

/// What the process was holding at one point in a scenario.
#[derive(Clone, Copy, Default)]
struct Sample {
    /// Resident bytes: the figure a task manager shows for the process.
    working_set: u64,
    /// Committed private bytes, which is what unbounded growth grows.
    private: u64,
    /// Live Rust heap bytes and blocks, exact, under the dhat allocator.
    heap: Option<(u64, u64)>,
    /// The high-water mark of that heap, which is what a transient spike costs.
    heap_peak: Option<u64>,
}

#[cfg(windows)]
fn process_counters() -> (u64, u64) {
    use windows_sys::Win32::System::ProcessStatus::GetProcessMemoryInfo;
    use windows_sys::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS;
    use windows_sys::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS_EX;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // The EX layout is the base layout plus PrivateUsage; the call is told the
    // larger size and fills both.
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    if ok == 0 { (0, 0) } else { (counters.WorkingSetSize as u64, counters.PrivateUsage as u64) }
}

#[cfg(not(windows))]
fn process_counters() -> (u64, u64) {
    (0, 0)
}

#[cfg(feature = "dhat-heap")]
fn heap_counters() -> (Option<(u64, u64)>, Option<u64>) {
    let stats = dhat::HeapStats::get();
    (Some((stats.curr_bytes as u64, stats.curr_blocks as u64)), Some(stats.max_bytes as u64))
}

#[cfg(not(feature = "dhat-heap"))]
fn heap_counters() -> (Option<(u64, u64)>, Option<u64>) {
    (None, None)
}

impl Sample {
    fn take() -> Self {
        let (working_set, private) = process_counters();
        let (heap, heap_peak) = heap_counters();
        Sample { working_set, private, heap, heap_peak }
    }
}

/// Prints one step of a scenario against the sample the scenario started from.
struct Report {
    start: Sample,
    last: Sample,
}

impl Report {
    fn new() -> Self {
        let start = Sample::take();
        println!(
            "{:<44} {:>10} {:>10} {:>10} {:>14} {:>10}",
            "step", "WS MiB", "priv MiB", "d priv", "live heap MiB", "peak MiB"
        );
        let report = Report { start, last: start };
        report.line("start");
        report
    }

    /// Samples now and prints the line. Returns the sample so a scenario can
    /// compare two of its own steps.
    fn step(&mut self, label: &str) -> Sample {
        self.last = Sample::take();
        self.line(label);
        self.last
    }

    fn line(&self, label: &str) {
        let sample = self.last;
        let heap = match sample.heap {
            Some((bytes, blocks)) => format!("{:.1} ({blocks}k)", bytes as f64 / MIB, blocks = blocks / 1000),
            None => "-".to_string(),
        };
        let peak = match sample.heap_peak {
            Some(bytes) => format!("{:.1}", bytes as f64 / MIB),
            None => "-".to_string(),
        };
        println!(
            "{:<44} {:>10.1} {:>10.1} {:>+10.1} {:>14} {:>10}",
            label,
            sample.working_set as f64 / MIB,
            sample.private as f64 / MIB,
            (sample.private as f64 - self.start.private as f64) / MIB,
            heap,
            peak,
        );
    }
}

/// The dumped builds on this machine, oldest first, read from the index the app
/// resolves through.
fn dumped_builds(cache_dir: &str) -> Vec<u32> {
    let Some(base) = crate::task::replays::game_data_dump_base_with_override(cache_dir) else {
        return Vec::new();
    };
    let index = wows_data_mgr::builds::BuildsIndex::load(&base.join("builds.toml"));
    let mut builds: Vec<u32> = index.builds.iter().map(|entry| entry.build).collect();
    builds.sort_unstable();
    builds.dedup();
    builds
}

/// The same build loaded over and over, each load replacing the last.
///
/// Tells a per-build retention apart from the allocator keeping freed pages:
/// what one build costs is already known, so anything this run climbs by is
/// what a load leaves behind whatever the cache then drops.
fn reload(wows_dir: &Path, dump_dir: &str, build: u32, times: usize, mut report: Report) {
    for round in 1..=times {
        // A cache of its own per round, so dropping it drops the build with it:
        // what a load leaves behind is what does not come back here.
        let cache = BuildDataCache::new(wows_dir.to_path_buf(), "en".to_string(), dump_dir.to_string());
        let loaded = cache.resolve_build(build).is_some();
        drop(cache);
        report.step(&format!("load {round} of build {build}{}", if loaded { "" } else { " (no data)" }));
    }
}

/// What the armor viewer keeps once it has opened a ship.
///
/// `ShipAssets` is built once per session and held in the tab's state, so what
/// it costs here is what the tab costs for as long as the app runs. Built the
/// way the tab builds it, over the provider the build already holds:
/// `ShipAssets::load` would parse GameParams a second time, which the app does
/// not do.
fn armor(data: &SharedBuildData, mut report: Report) {
    let (vfs, metadata) = {
        let guard = data.read();
        (guard.vfs.clone(), guard.game_metadata.clone())
    };
    let Some(metadata) = metadata else {
        println!("  this build has no game metadata");
        return;
    };
    match wowsunpack::export::ship::ShipAssets::from_vfs_with_metadata(&vfs, metadata) {
        Ok(assets) => {
            report.step("ship assets loaded (assets.bin + camo db)");
            drop(assets);
            report.step("dropped the ship assets");
        }
        Err(err) => println!("  ship assets unavailable: {err}"),
    }
}

/// Every replay under `dir`, one per distinct build, newest build first.
///
/// One per build is what spreads a run across game versions the way a replay
/// directory kept since 2017 does, which is the case the per-build caches are
/// sized by.
fn one_replay_per_build(dir: &Path) -> Vec<(u32, PathBuf)> {
    let mut by_build: std::collections::BTreeMap<u32, PathBuf> = std::collections::BTreeMap::new();
    for path in super::replay_paths(dir) {
        let Some(build) = read_build(&path) else { continue };
        by_build.entry(build).or_insert(path);
    }
    by_build.into_iter().rev().collect()
}

/// The build a replay was recorded on, from its plaintext header alone.
fn read_build(path: &Path) -> Option<u32> {
    let blob = wows_replays::ReplayFile::read_meta_blob(path).ok()?;
    let meta = wows_replays::ReplayMetaRef::from_slice(&blob).ok()?;
    Version::try_from_client_exe(&meta.clientVersionFromExe)?.build.map(|build| build.get())
}

/// Resolving one build after another, which is what a listing spanning game
/// versions asks of the cache.
///
/// The bound the cache states is the claim under test: the figure is expected
/// to plateau once the LRU starts evicting, and a straight line instead means
/// something is holding the evicted builds.
fn builds(cache: &BuildDataCache, builds: &[u32], mut report: Report) {
    for build in builds {
        let resolved = cache.resolve_build(*build).is_some();
        let label = if resolved { format!("resolved {build}") } else { format!("resolved {build} (no data)") };
        report.step(&label);
    }
    println!("\nloaded builds held by the cache: {}", cache.loaded_builds().len());
}

/// One open replay tab per build, all held at once.
///
/// A hydrated replay keeps its build's metadata provider alive, so this is the
/// figure a reader who opens a tab per era pays whatever the cache's own bound
/// says.
fn tabs(deps: &ReplayDependencies, replays: &[(u32, PathBuf)], mut report: Report) {
    let mut held: Vec<Replay> = Vec::new();
    for (build, path) in replays {
        match super::load_timed(path, deps, &mut super::StageTimings::default()) {
            Ok(replay) => {
                held.push(replay);
                report.step(&format!("held {} tabs (build {build})", held.len()));
            }
            Err(reason) => println!("  skipped build {build}: {reason}"),
        }
    }
    println!("\nloaded builds held by the cache: {}", deps.build_cache.loaded_builds().len());
    let count = held.len();
    drop(held);
    report.step(&format!("dropped {count} tabs"));
}

/// Several open tabs on replays from one build, which is the other half of
/// what a tab costs: the parse alone, with the build's data paid for once.
fn parses(deps: &ReplayDependencies, paths: &[PathBuf], mut report: Report) {
    let mut held: Vec<Replay> = Vec::new();
    for path in paths {
        match super::load_timed(path, deps, &mut super::StageTimings::default()) {
            Ok(replay) => {
                held.push(replay);
                report.step(&format!("held {} parses", held.len()));
            }
            Err(reason) => println!("  skipped {}: {reason}", path.display()),
        }
    }
    let count = held.len();
    drop(held);
    report.step(&format!("dropped {count} parses"));
}

/// Every replay fed to the player tracker and then dropped, which is what the
/// "populate from replays" run does.
///
/// Only the tracker is kept, so the growth is the tracker's own.
fn tracker(deps: &ReplayDependencies, paths: &[PathBuf], mut report: Report) {
    let tracker = Arc::new(RwLock::new(PlayerTracker::default()));
    let mut fed = 0usize;
    for path in paths {
        match super::load_timed(path, deps, &mut super::StageTimings::default()) {
            Ok(replay) => {
                tracker.write().update_from_replay(&replay);
                drop(replay);
                fed += 1;
                if fed.is_multiple_of(10) {
                    report.step(&format!("{fed} replays tracked"));
                }
            }
            Err(reason) => println!("  skipped {}: {reason}", path.display()),
        }
    }
    report.step(&format!("{fed} replays tracked"));

    let guard = tracker.read();
    let players = guard.tracked_players.len();
    let serialized = serde_json::to_vec(&*guard).map(|bytes| bytes.len()).unwrap_or_default();
    drop(guard);
    println!("\ntracked players: {players}");
    println!("serialized tracker: {:.1} MiB", serialized as f64 / MIB);

    drop(tracker);
    report.step("dropped the tracker");
}

/// What opening the resource browser costs: the folder tree and the flat file
/// list, then the same for the assets.bin pane.
///
/// Both are built when the pane opens and live in the pane, so what this run
/// holds between steps is what the tab holds for the rest of a session.
fn unpacker(data: &SharedBuildData, mut report: Report) {
    let vfs = data.read().vfs.clone();

    let tree = wows_toolkit_viewmodel::unpacker::listing::build_folder_tree(&vfs, "");
    report.step(&format!("pkg folder tree ({} roots)", tree.len()));
    drop(tree);
    report.step("dropped the folder tree");

    let files = wows_toolkit_viewmodel::unpacker::listing::build_file_list(&vfs);
    report.step(&format!("pkg file list ({} files)", files.len()));
    drop(files);
    report.step("dropped the file list");

    match wows_toolkit_viewmodel::unpacker::assets_bin::open(&vfs) {
        Ok(assets_vfs) => {
            report.step("assets.bin opened");
            let files = wows_toolkit_viewmodel::unpacker::listing::build_file_list(&assets_vfs);
            report.step(&format!("assets.bin file list ({} files)", files.len()));
            drop(files);
            drop(assets_vfs);
            report.step("dropped assets.bin");
        }
        Err(err) => println!("  assets.bin unavailable: {err}"),
    }
}

/// Minimap art decoded through the renderer's asset cache, one map at a time.
///
/// The cache is keyed by game version and map name with no bound, so what this
/// measures per map is also what a session pays per map per version it opens a
/// replay from.
fn maps(data: &SharedBuildData, limit: usize, mut report: Report) {
    let (vfs, version) = {
        let guard = data.read();
        (guard.vfs.clone(), guard.full_version)
    };
    let names = map_names(&vfs, limit);
    if names.is_empty() {
        println!("  no maps under spaces/ in this build");
        return;
    }

    let mut cache = crate::replay::renderer::RendererAssetCache::default();
    let mut decoded = 0usize;
    for name in &names {
        let (image, _info) = cache.get_or_load_map(name, &vfs, version.as_ref());
        let Some(image) = image else {
            println!("  {name}: no minimap art");
            continue;
        };
        decoded += 1;
        report.step(&format!("{decoded} maps cached ({name}, {}x{})", image.width, image.height));
    }
    drop(cache);
    report.step(&format!("dropped the cache ({decoded} maps)"));
}

/// The first `limit` map directories under `spaces/`.
fn map_names(vfs: &VfsPath, limit: usize) -> Vec<String> {
    let Ok(spaces) = vfs.join("spaces") else { return Vec::new() };
    let Ok(entries) = spaces.read_dir() else { return Vec::new() };
    let mut names: Vec<String> =
        entries.filter(|entry| entry.is_dir().unwrap_or(false)).map(|entry| entry.filename()).collect();
    names.sort();
    names.truncate(limit);
    names
}

/// Runs one scenario end to end. `count` bounds how many builds, replays or
/// maps it walks, since every scenario's whole point is what the tenth costs
/// against the first.
pub fn run(scenario: &str, wows_dir: PathBuf, dump_dir: String, replay_dir: PathBuf, count: usize) {
    println!("game install : {}", wows_dir.display());
    println!("dump archive : {dump_dir}");
    println!("replays      : {}", replay_dir.display());
    println!("scenario     : {scenario} (count {count})");
    if cfg!(feature = "dhat-heap") {
        println!("heap         : dhat (live bytes are exact, and dhat adds its own per-block bookkeeping)");
    }
    println!();

    let cache = BuildDataCache::new(wows_dir.clone(), "en".to_string(), dump_dir.clone());

    match scenario {
        "builds" => {
            let mut all = dumped_builds(&dump_dir);
            all.reverse();
            all.truncate(count);
            if all.is_empty() {
                eprintln!("no dumped builds under {dump_dir}");
                return;
            }
            let report = Report::new();
            builds(&cache, &all, report);
        }
        "tabs" => {
            let replays = one_replay_per_build(&replay_dir);
            if replays.is_empty() {
                eprintln!("no readable replays under {}", replay_dir.display());
                return;
            }
            let replays = &replays[..count.min(replays.len())];
            let deps = super::headless_deps(cache);
            let report = Report::new();
            tabs(&deps, replays, report);
        }
        "parses" => {
            let paths = super::replay_paths(&replay_dir);
            if paths.is_empty() {
                eprintln!("no replays under {}", replay_dir.display());
                return;
            }
            let paths = &paths[..count.min(paths.len())];
            let deps = super::headless_deps(cache);
            let report = Report::new();
            parses(&deps, paths, report);
        }
        "tracker" => {
            let paths = super::replay_paths(&replay_dir);
            if paths.is_empty() {
                eprintln!("no replays under {}", replay_dir.display());
                return;
            }
            let paths = &paths[..count.min(paths.len())];
            let deps = super::headless_deps(cache);
            let report = Report::new();
            tracker(&deps, paths, report);
        }
        "reload" => {
            let Some(build) = live_build(&wows_dir) else {
                eprintln!("no build under {}", wows_dir.join("bin").display());
                return;
            };
            drop(cache);
            let report = Report::new();
            reload(&wows_dir, &dump_dir, build, count, report);
        }
        "unpacker" | "maps" | "armor" => {
            // The browser and the renderer both work off one build's data, so
            // the run needs the live install's build loaded first.
            let Some(build) = live_build(&wows_dir) else {
                eprintln!("no build under {}", wows_dir.join("bin").display());
                return;
            };
            let Some(data) = cache.resolve_build(build) else {
                eprintln!("build {build} would not load");
                return;
            };
            let report = Report::new();
            match scenario {
                "unpacker" => unpacker(&data, report),
                "maps" => maps(&data, count, report),
                _ => armor(&data, report),
            }
        }
        other => eprintln!("unknown scenario {other:?}; one of {}", SCENARIOS.join(", ")),
    }
}

/// The newest build directory in a live install.
fn live_build(wows_dir: &Path) -> Option<u32> {
    let entries = std::fs::read_dir(wows_dir.join("bin")).ok()?;
    entries.flatten().filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok()).max()
}
