//! Stage-by-stage wall-clock timing of the replay load path, from a file on
//! disk to a populated `UiReport`.
//!
//! Lives in the library rather than in `src/bin` because [`ReplayDependencies`]
//! names types from private modules (`twitch`, `ui`) that a separate binary
//! crate cannot reach. `src/bin/profile_replay.rs` is a thin wrapper over
//! [`run`].
//!
//! The per-packet timers cost two `Instant::now()` calls per packet. On a
//! stream of a few hundred thousand packets that is single-digit milliseconds,
//! which is small against a parse measured in hundreds, but it is real: treat
//! the packet/process split as a ratio and let the criterion benches in
//! `wows-battle-world`, which carry no instrumentation, supply absolute
//! numbers.

use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use parking_lot::Mutex;
use parking_lot::RwLock;
use wows_battle_world::world::BattleWorld;
use wows_replays::ReplayFile;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use crate::data::wows_data::ReplayDependencies;
use crate::data::wows_data::WoWsDataMap;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SortOrder;

/// Time attributed to one named block inside a stage.
pub struct SubStage {
    pub name: &'static str,
    pub elapsed: Duration,
}

/// A replay the run could not time, and why.
struct Skipped {
    path: PathBuf,
    reason: String,
}

thread_local! {
    /// Sub-stage totals recorded by the `timed_stage!` macro, in first-seen
    /// order so a report reads top to bottom through the function it came from.
    static SUBSTAGES: RefCell<Vec<SubStage>> = const { RefCell::new(Vec::new()) };
}

/// Accumulate time against a named sub-stage on the calling thread. Called by
/// `timed_stage!`; not meant to be called directly.
pub fn record(name: &'static str, elapsed: Duration) {
    SUBSTAGES.with_borrow_mut(|stages| match stages.iter_mut().find(|stage| stage.name == name) {
        Some(stage) => stage.elapsed += elapsed,
        None => stages.push(SubStage { name, elapsed }),
    });
}

/// Drain the sub-stage totals recorded on this thread.
pub fn take_substages() -> Vec<SubStage> {
    SUBSTAGES.with_borrow_mut(std::mem::take)
}

/// Wall time attributed to one replay load.
#[derive(Default, Clone)]
pub struct StageTimings {
    /// Reading the file off disk.
    pub read: Duration,
    /// Blowfish decrypt, zlib inflate, and metadata JSON.
    pub container: Duration,
    /// Resolving version-matched game data. Pays the full build load on the
    /// first replay of a build and hits the cache afterwards, so it is reported
    /// separately from the per-replay stages.
    pub resolve: Duration,
    /// `Parser::parse_packet` across the whole stream.
    pub packet_parse: Duration,
    /// `BattleWorld::process` across the whole stream.
    pub world_process: Duration,
    /// `finish` plus `into_report`.
    pub report: Duration,
    /// `UiReport::new`.
    pub ui_report: Duration,
    pub packets: u64,
    pub packet_bytes: u64,
}

impl StageTimings {
    /// Everything a user waits through after the game data is already loaded.
    pub fn per_replay_total(&self) -> Duration {
        self.read + self.container + self.packet_parse + self.world_process + self.report + self.ui_report
    }

    fn add(&mut self, other: &StageTimings) {
        self.read += other.read;
        self.container += other.container;
        self.resolve += other.resolve;
        self.packet_parse += other.packet_parse;
        self.world_process += other.world_process;
        self.report += other.report;
        self.ui_report += other.ui_report;
        self.packets += other.packets;
        self.packet_bytes += other.packet_bytes;
    }
}

/// Build dependencies sufficient for `build_ui_report`. Twitch and
/// personal-rating state are empty: neither is populated during a normal load
/// either, and the sender is a dead end because the UI report only uses it to
/// queue follow-up work.
fn headless_deps(wows_data_map: WoWsDataMap) -> ReplayDependencies {
    let (tx, rx) = mpsc::channel();
    // Leaking the receiver keeps sends from failing; nothing consumes them.
    std::mem::forget(rx);
    ReplayDependencies {
        wows_data_map,
        shipbuilds_client: crate::data::shipbuilds::ShipBuildsClient::new()
            .expect("failed to build ShipBuilds HTTP client"),
        twitch_state: Arc::new(RwLock::new(Default::default())),
        replay_sort: Arc::new(Mutex::new(SortOrder::default())),
        background_task_sender: tx,
        is_debug_mode: false,
        personal_rating_data: Arc::new(RwLock::new(Default::default())),
    }
}

/// Time one replay end to end. `Err` carries a reason the replay was skipped.
fn time_one(path: &Path, deps: &ReplayDependencies) -> Result<StageTimings, String> {
    let mut t = StageTimings::default();

    let start = Instant::now();
    let bytes = std::fs::read(path).map_err(|e| format!("read failed: {e}"))?;
    t.read = start.elapsed();

    let start = Instant::now();
    let replay_file = ReplayFile::from_bytes(&bytes).map_err(|e| format!("container parse failed: {e:?}"))?;
    t.container = start.elapsed();
    t.packet_bytes = replay_file.packet_data.len() as u64;

    let raw_version = replay_file.meta.clientVersionFromExe.clone();
    let version = Version::try_from_client_exe(&raw_version).ok_or_else(|| format!("bad version {raw_version:?}"))?;

    let start = Instant::now();
    let wows_data = deps.wows_data_map.resolve(&version).ok_or_else(|| {
        format!("no game data for build {}", version.build_number().map_or("unknown".to_string(), |b| b.to_string()))
    })?;
    t.resolve = start.elapsed();

    let (metadata_provider, game_constants, patch_version) = {
        let data = wows_data.read();
        let provider = data.game_metadata.clone().ok_or_else(|| "build has no game metadata".to_string())?;
        (provider, Arc::clone(&data.game_constants), data.patch_version)
    };

    let expected_build = patch_version.to_string();
    let replay_build = raw_version.split(',').nth(3).unwrap_or_default().trim();
    if replay_build != expected_build {
        return Err(format!("build mismatch: replay {replay_build}, data {expected_build}"));
    }

    // Mirrors the single-replay fast path of `Replay::parse`, split so each
    // stage can be attributed. The alt-perspective merge path is not covered:
    // a double-click never takes it.
    let mut world = BattleWorld::new(&replay_file.meta, metadata_provider.as_ref(), Some(game_constants.as_ref()));
    world.set_record_hit_history(true);
    world.set_record_salvo_history(true);

    let mut parser = wows_replays::packet2::Parser::with_version(metadata_provider.entity_specs(), version);
    let mut remaining = replay_file.packet_data.as_slice();
    while !remaining.is_empty() {
        let start = Instant::now();
        let packet = parser.parse_packet(&mut remaining);
        t.packet_parse += start.elapsed();

        match packet {
            Ok(packet) => {
                t.packets += 1;
                let start = Instant::now();
                wows_replays::analyzer::Analyzer::process(&mut world, &packet);
                t.world_process += start.elapsed();
            }
            Err(_) => break,
        }
    }

    let start = Instant::now();
    wows_replays::analyzer::Analyzer::finish(&mut world);
    let report = world.into_report();
    t.report = start.elapsed();

    let mut replay = Replay::new(replay_file, metadata_provider);
    replay.game_constants = Some(game_constants);
    replay.source_path = Some(path.to_path_buf());
    replay.battle_report = Some(report);

    let start = Instant::now();
    replay.build_ui_report(deps);
    t.ui_report = start.elapsed();

    if replay.ui_report.is_none() {
        return Err("ui report was not built".to_string());
    }

    Ok(t)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn print_row(label: &str, d: Duration, total: Duration) {
    let pct = if total.is_zero() { 0.0 } else { ms(d) / ms(total) * 100.0 };
    println!("  {label:<16} {:>10.1} ms  {pct:>5.1}%", ms(d));
}

/// Collect `.wowsreplay` files, sorted so runs are comparable.
fn replay_paths(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wowsreplay")))
        .collect();
    paths.sort();
    paths
}

/// Time every replay under `dir` and print a per-replay and aggregate
/// breakdown.
///
/// `wows_dir` is a live game install and `dump_dir` a dumped-build archive;
/// resolution tries the install first and falls back to the archive, which is
/// what [`WoWsDataMap::resolve`] does for the real app. Replays whose build is
/// in neither are skipped and listed.
pub fn run(wows_dir: PathBuf, dump_dir: String, replay_dir: PathBuf, limit: Option<usize>) {
    let paths = replay_paths(&replay_dir);
    if paths.is_empty() {
        eprintln!("no .wowsreplay files under {}", replay_dir.display());
        return;
    }
    let paths = match limit {
        Some(n) => &paths[..n.min(paths.len())],
        None => &paths[..],
    };

    println!("game install : {}", wows_dir.display());
    println!("dump archive : {dump_dir}");
    println!("replays      : {} under {}", paths.len(), replay_dir.display());
    println!();

    let deps = headless_deps(WoWsDataMap::new(wows_dir, "en".to_string(), dump_dir));

    let mut total = StageTimings::default();
    let mut succeeded = 0usize;
    let mut skipped: Vec<Skipped> = Vec::new();

    for path in paths {
        match time_one(path, &deps) {
            Ok(t) => {
                println!(
                    "{:<62} {:>8.1} ms  ({} packets, {:.1} MiB)",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    ms(t.per_replay_total()),
                    t.packets,
                    t.packet_bytes as f64 / (1024.0 * 1024.0),
                );
                total.add(&t);
                succeeded += 1;
            }
            Err(reason) => skipped.push(Skipped { path: path.clone(), reason }),
        }
    }

    if succeeded == 0 {
        eprintln!("\nno replays parsed");
        for entry in &skipped {
            eprintln!("  skipped {}: {}", entry.path.display(), entry.reason);
        }
        return;
    }

    let per_replay = total.per_replay_total();
    println!("\n=== aggregate over {succeeded} replays ===");
    print_row("read", total.read, per_replay);
    print_row("container", total.container, per_replay);
    print_row("packet_parse", total.packet_parse, per_replay);
    print_row("world_process", total.world_process, per_replay);
    print_row("report", total.report, per_replay);
    print_row("ui_report", total.ui_report, per_replay);
    println!("  {:<16} {:>10.1} ms", "TOTAL", ms(per_replay));
    println!("  {:<16} {:>10.1} ms", "mean/replay", ms(per_replay) / succeeded as f64);
    println!("  {:<16} {:>10.1} ms  (build loads, amortized across replays)", "resolve", ms(total.resolve));
    println!("  {:<16} {:>10}", "packets", total.packets);

    let substages = take_substages();
    if !substages.is_empty() {
        println!("\n=== ui_report breakdown ===");
        for stage in &substages {
            print_row(stage.name, stage.elapsed, total.ui_report);
        }
        let accounted: Duration = substages.iter().map(|stage| stage.elapsed).sum();
        print_row("unaccounted", total.ui_report.saturating_sub(accounted), total.ui_report);
    }

    if !skipped.is_empty() {
        println!("\n=== skipped {} ===", skipped.len());
        for entry in &skipped {
            println!("  {}: {}", entry.path.file_name().unwrap_or_default().to_string_lossy(), entry.reason);
        }
    }
}
