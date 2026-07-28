#![cfg(feature = "battle-report")]
//! Effective fire chance measured against a corpus of real replays.
//!
//! The unit tests prove the eligibility and attribution model is self
//! consistent. This proves it is right about actual matches, and measures the
//! one assumption the whole model rests on: that the server assigns a fire to
//! the hull section nearest the impact. Nothing in the client scripts states
//! that rule, so [`predicted_sections_agree_with_the_server`] is the only
//! evidence there is.
//!
//! Ignored: needs a replay corpus and a game install. Run with
//! `cargo test -p wows-replay-insights --release --features battle-report
//! --test fire_chance_corpus -- --ignored --nocapture`. The feature carries
//! `resolve_battle_results`, which the external attribution check reads the
//! authoritative per-victim fire counts through.
//!
//! Two directories are read:
//! - the corpus, `WOWS_REPLAY_CORPUS` or `~/Downloads`, scanned for
//!   `*.wowsreplay`;
//! - the game install, `WOWS_DIR` or `E:\WoWs\World_of_Warships`, for
//!   `content/assets.bin`, which is the only place fire-node geometry lives.
//!
//! Per-replay GameParams come from the dumped build archives
//! (`wows_data_mgr::game_dir_for_build`), so a replay whose build has no local
//! dump is skipped. **Every skip is counted and printed**: a corpus test that
//! silently measures half the corpus reports a rate for a population it does
//! not describe.
//!
//! The geometry is a known cross-build approximation. The dumps carry no
//! `assets.bin`, so fire-node positions come from the installed build whatever
//! the replay's build is. A hull whose `.model` path no longer exists in the
//! install simply fails to resolve and its hits are refused, which costs
//! samples rather than corrupting them; a hull whose model was reworked in
//! place while keeping its path would shift the nodes, which is the one way
//! this could mislead. The printed per-build breakdown is there so that risk
//! stays visible.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use wows_battle_world::BattleWorld;
use wows_battle_world::report::BattleReport;
use wows_replay_insights::battle_report::resolve_battle_results;
use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wows_replay_insights::fire_chance::analysis::ExclusionReason;
use wows_replay_insights::fire_chance::analysis::analyze;
use wows_replay_insights::fire_chance::resolve::ResolutionDiagnostics;
use wows_replay_insights::fire_chance::resolve::resolve_fire_chance_input;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::models::assets_bin;
use wowsunpack::models::assets_bin::PrototypeDatabase;
use wowsunpack::models::fire_nodes;
use wowsunpack::models::fire_nodes::BurnNodeIndex;
use wowsunpack::models::fire_nodes::FireSectionGeometry;
use wowsunpack::vfs::VfsPath;
use wowsunpack::vfs::impls::physical::PhysicalFS;

fn corpus_dir() -> PathBuf {
    std::env::var_os("WOWS_REPLAY_CORPUS").map(PathBuf::from).unwrap_or_else(|| {
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
        PathBuf::from(home).join("Downloads")
    })
}

fn game_dir() -> PathBuf {
    std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"))
}

/// Why a replay produced no measurement. Kept as a discrete key rather than a
/// message so the printed tally names populations, not incidents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SkipReason {
    /// The file did not parse as a replay at all.
    Unreadable,
    /// The replay's build has no dumped game data on this machine.
    NoGameData,
    /// Building the report panicked, e.g. a pre-0.9 replay carrying no roster
    /// RPC so no player is ever tagged as the recording one.
    ReportUnbuildable,
    /// The attacker side did not resolve; the printed detail names which.
    AttackerUnresolved,
    /// `analyze` refused: no victim hull resolved to geometry, or the attacker
    /// ship carries no tier.
    AnalysisRefused,
}

/// One replay's measurement, reduced to plain data so the whole corpus can be
/// computed once and shared across the three tests. Holding `BattleReport`s
/// instead would make the set unshareable and force three full corpus passes.
struct Measurement {
    name: String,
    build: u32,
    eligible_hits: u32,
    fires: u32,
    unattributed_fires: u32,
    exclusions: BTreeMap<ExclusionReason, u32>,
    /// Attributed fires per victim ship index.
    fires_by_ship: BTreeMap<String, u64>,
    /// Server-recorded fires per victim ship index, from the post-battle
    /// results. `None` when the replay carries no results blob or the build's
    /// constants table is missing, so the external check simply has nothing to
    /// compare against.
    server_fires_by_ship: Option<BTreeMap<String, u64>>,
    /// `(predicted, actual)` per attributed fire.
    section_predictions: Vec<(u8, u8)>,
    diagnostics: ResolutionDiagnostics,
    /// Presence windows still open at the end of the parse, against the total.
    /// A window left open is `PresenceLog`'s one remaining route to a false
    /// "observed", so the ratio bounds how much the observation gate can be
    /// trusting more than it should.
    open_windows: u32,
    total_windows: u32,
}

struct Corpus {
    measurements: Vec<Measurement>,
    skips: BTreeMap<SkipReason, u32>,
    skip_details: Vec<String>,
    replays_seen: u32,
    /// Hull model paths that resolved to geometry, against those that did not.
    hulls_placed: u32,
    hulls_unplaced: u32,
}

static CORPUS: OnceLock<Corpus> = OnceLock::new();

fn corpus() -> &'static Corpus {
    CORPUS.get_or_init(build_corpus)
}

/// The installed build's `assets.bin`, parsed once. Leaked because
/// `PrototypeDatabase` borrows the bytes and this lives for the whole test
/// process anyway.
struct Geometry {
    db: PrototypeDatabase<'static>,
    self_id_index: HashMap<u64, usize>,
    /// Keyed by (hull model path, section count), because the count is part of
    /// the resolver's contract: a geometry is only valid for the hull whose
    /// `burnNodes` length it was resolved against.
    memo: RefCell<HashMap<(String, usize), Option<FireSectionGeometry>>>,
}

impl Geometry {
    fn load() -> Geometry {
        let vfs = wowsunpack::game_data::build_game_vfs(&game_dir()).expect("game install vfs");
        let mut file = vfs.join("content/assets.bin").expect("assets.bin path").open_file().expect("open assets.bin");
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes).expect("read assets.bin");
        let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let db = assets_bin::parse_assets_bin(bytes).expect("parse assets.bin");
        let self_id_index = db.build_self_id_index();
        Geometry { db, self_id_index, memo: RefCell::new(HashMap::new()) }
    }

    fn get(&self, hull_model_path: &str, expected_nodes: usize) -> Option<FireSectionGeometry> {
        let key = (hull_model_path.to_owned(), expected_nodes);
        if let Some(hit) = self.memo.borrow().get(&key) {
            return hit.clone();
        }
        let resolved =
            fire_nodes::resolve_fire_sections(&self.db, &self.self_id_index, hull_model_path, expected_nodes).ok();
        self.memo.borrow_mut().insert(key, resolved.clone());
        resolved
    }
}

/// Everything one build's replays share. Loaded once per build so a corpus
/// spanning several builds does not hold several GameParams sets at a time.
struct BuildData {
    provider: GameMetadataProvider,
    constants_json: Option<serde_json::Value>,
    game_constants: GameConstants,
}

fn load_build(build: u32) -> Option<BuildData> {
    let game_dir = wows_data_mgr::game_dir_for_build(build)?;
    let vfs_root = game_dir.join("vfs");
    if !vfs_root.exists() {
        return None;
    }
    let vfs = VfsPath::new(PhysicalFS::new(&vfs_root));

    let rkyv_path = game_dir.join("game_params.rkyv");
    let provider = match wowsunpack::game_params::cache::load(&rkyv_path) {
        Some(params) => GameMetadataProvider::from_params_with_vfs(params, &vfs).ok()?,
        None => GameMetadataProvider::from_vfs(&vfs).ok()?,
    };
    let game_constants = GameConstants::from_vfs(&vfs);
    // The constants table only feeds the post-battle results resolution, so its
    // absence costs the external check on one replay rather than the replay.
    let constants_json = std::fs::read(game_dir.join("constants.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());

    Some(BuildData { provider, constants_json, game_constants })
}

fn replay_paths(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("wowsreplay")))
        .collect();
    paths.sort();
    paths
}

fn build_report(replay: &ReplayFile, provider: &GameMetadataProvider, constants: &GameConstants) -> BattleReport {
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
    let mut world = BattleWorld::new(&replay.meta, provider, Some(constants));
    // Without this the hit history is empty and every rate is zero-sample.
    world.set_record_hit_history(true);

    let mut parser = wows_replays::packet2::Parser::with_version(provider.entity_specs(), version);
    let mut remaining = replay.packet_data.as_slice();
    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => world.process(&packet),
            Err(_) => break,
        }
    }
    world.finish();
    world.into_report()
}

/// Server-recorded fires the recording player started, per victim ship index.
///
/// The post-battle results are authoritative and are keyed by the victim's
/// account id, so this joins them to ship indices through the report's own
/// player list. Two players in the same ship fold into one entry, matching how
/// the per-ship breakdown groups.
fn server_fires_by_ship(report: &BattleReport, constants: &serde_json::Value) -> Option<BTreeMap<String, u64>> {
    let raw: serde_json::Value = serde_json::from_str(report.battle_results()?).ok()?;
    let resolved = resolve_battle_results(raw, constants);

    let self_db_id = report.self_player().initial_state().db_id();
    let interactions = resolved.pointer(&format!("/playersPublicInfo/{self_db_id}/interactions"))?.as_object()?.clone();

    let mut ship_by_db_id: HashMap<String, String> = HashMap::new();
    for player in report.players() {
        ship_by_db_id.insert(player.initial_state().db_id().to_string(), player.vehicle().index().to_owned());
    }

    let mut fires: BTreeMap<String, u64> = BTreeMap::new();
    for (victim_db_id, victim) in &interactions {
        let Some(ship) = ship_by_db_id.get(victim_db_id) else { continue };
        let count = victim.get("fires").and_then(|v| v.as_u64()).unwrap_or(0);
        *fires.entry(ship.clone()).or_insert(0) += count;
    }
    Some(fires)
}

fn measure(
    name: String,
    build: u32,
    replay: &ReplayFile,
    data: &BuildData,
    geometry: &Geometry,
    corpus: &mut Corpus,
) -> Option<Measurement> {
    let report =
        match std::panic::catch_unwind(AssertUnwindSafe(|| build_report(replay, &data.provider, &data.game_constants)))
        {
            Ok(report) => report,
            Err(_) => {
                *corpus.skips.entry(SkipReason::ReportUnbuildable).or_insert(0) += 1;
                corpus.skip_details.push(format!("{name}: building the report panicked"));
                return None;
            }
        };

    let resolution = match resolve_fire_chance_input(&report, &data.provider) {
        Ok(resolution) => resolution,
        Err(error) => {
            *corpus.skips.entry(SkipReason::AttackerUnresolved).or_insert(0) += 1;
            corpus.skip_details.push(format!("{name}: {error}"));
            return None;
        }
    };

    let mut placed: HashMap<String, FireSectionGeometry> = HashMap::new();
    for victim in resolution.victims().values() {
        if placed.contains_key(&victim.hull_model_path) {
            continue;
        }
        match geometry.get(&victim.hull_model_path, victim.node_probability.len()) {
            Some(geom) => {
                corpus.hulls_placed += 1;
                placed.insert(victim.hull_model_path.clone(), geom);
            }
            None => corpus.hulls_unplaced += 1,
        }
    }

    let lookup = |path: &str| placed.get(path).cloned();
    let input = resolution.input(&report, &data.provider, &lookup);
    let Some(out) = analyze(&input) else {
        *corpus.skips.entry(SkipReason::AnalysisRefused).or_insert(0) += 1;
        corpus.skip_details.push(format!("{name}: analyze refused (no placed hull, or the attacker has no tier)"));
        return None;
    };

    let (mut open_windows, mut total_windows) = (0u32, 0u32);
    for windows in report.presence().0.values() {
        for window in windows {
            total_windows += 1;
            if window.left.is_none() {
                open_windows += 1;
            }
        }
    }

    let fires_by_ship =
        out.per_ship.iter().map(|ship| (ship.victim_ship_index.clone(), u64::from(ship.fires))).collect();
    let server_fires_by_ship =
        data.constants_json.as_ref().and_then(|constants| server_fires_by_ship(&report, constants));

    Some(Measurement {
        name,
        build,
        eligible_hits: out.eligible_hits,
        fires: out.fires,
        unattributed_fires: out.unattributed_fires,
        exclusions: out.exclusions.clone(),
        fires_by_ship,
        server_fires_by_ship,
        section_predictions: section_pairs(&out),
        diagnostics: resolution.diagnostics(),
        open_windows,
        total_windows,
    })
}

fn section_pairs(out: &EffectiveFireChance) -> Vec<(u8, u8)> {
    out.section_predictions.iter().map(|pair| (pair.predicted.get(), pair.actual.get())).collect()
}

fn build_corpus() -> Corpus {
    let mut corpus = Corpus {
        measurements: Vec::new(),
        skips: BTreeMap::new(),
        skip_details: Vec::new(),
        replays_seen: 0,
        hulls_placed: 0,
        hulls_unplaced: 0,
    };

    let paths = replay_paths(&corpus_dir());
    if paths.is_empty() {
        return corpus;
    }
    let geometry = Geometry::load();

    // Grouped by build so one GameParams set is resident at a time.
    let mut by_build: BTreeMap<u32, Vec<PathBuf>> = BTreeMap::new();
    for path in paths {
        corpus.replays_seen += 1;
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let Ok(replay) = ReplayFile::from_file(&path) else {
            *corpus.skips.entry(SkipReason::Unreadable).or_insert(0) += 1;
            corpus.skip_details.push(format!("{name}: not a readable replay"));
            continue;
        };
        let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
        let Some(build) = version.build else {
            *corpus.skips.entry(SkipReason::NoGameData).or_insert(0) += 1;
            corpus.skip_details.push(format!("{name}: replay names no build"));
            continue;
        };
        by_build.entry(build.get()).or_default().push(path);
    }

    for (build, paths) in by_build {
        let Some(data) = load_build(build) else {
            *corpus.skips.entry(SkipReason::NoGameData).or_insert(0) += paths.len() as u32;
            corpus.skip_details.push(format!("build {build}: no dumped game data ({} replays)", paths.len()));
            continue;
        };
        for path in paths {
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let Ok(replay) = ReplayFile::from_file(&path) else {
                *corpus.skips.entry(SkipReason::Unreadable).or_insert(0) += 1;
                corpus.skip_details.push(format!("{name}: not a readable replay"));
                continue;
            };
            if let Some(measurement) = measure(name, build, &replay, &data, &geometry, &mut corpus) {
                corpus.measurements.push(measurement);
            }
        }
    }

    corpus
}

/// Printed once by whichever test runs first, so a failure in any of the three
/// carries the population it was measured over.
fn print_corpus_summary(corpus: &Corpus) {
    println!("corpus: {} replays seen, {} measured", corpus.replays_seen, corpus.measurements.len());
    for (reason, count) in &corpus.skips {
        println!("  skipped {count} for {reason:?}");
    }
    for detail in &corpus.skip_details {
        println!("    {detail}");
    }
    println!("hull geometry: {} placed, {} unplaced", corpus.hulls_placed, corpus.hulls_unplaced);

    let mut diagnostics = ResolutionDiagnostics::default();
    let (mut open, mut total) = (0u32, 0u32);
    let mut exclusions: BTreeMap<ExclusionReason, u32> = BTreeMap::new();
    for measurement in &corpus.measurements {
        let d = measurement.diagnostics;
        diagnostics.resolved += d.resolved;
        diagnostics.no_build += d.no_build;
        diagnostics.no_hull_component += d.no_hull_component;
        diagnostics.no_burn_nodes += d.no_burn_nodes;
        diagnostics.no_hull_model += d.no_hull_model;
        diagnostics.unknown_fate += d.unknown_fate;
        diagnostics.unknown_fire_prevention += d.unknown_fire_prevention;
        open += measurement.open_windows;
        total += measurement.total_windows;
        for (reason, count) in &measurement.exclusions {
            *exclusions.entry(*reason).or_insert(0) += count;
        }
    }
    println!("victims: {diagnostics:?}");
    if diagnostics.resolved > 0 {
        println!(
            "  unknown fate {:.3} of resolved victims, unknown fire prevention {:.3}",
            f64::from(diagnostics.unknown_fate) / f64::from(diagnostics.resolved),
            f64::from(diagnostics.unknown_fire_prevention) / f64::from(diagnostics.resolved),
        );
    }
    if total > 0 {
        println!(
            "presence windows: {open} of {total} still open at end of parse ({:.3})",
            f64::from(open) / f64::from(total)
        );
    }
    println!("exclusions over the whole corpus:");
    for (reason, count) in &exclusions {
        println!("  {reason:?}: {count}");
    }

    let mut per_build: BTreeMap<u32, (u32, u32, u32)> = BTreeMap::new();
    for measurement in &corpus.measurements {
        let entry = per_build.entry(measurement.build).or_insert((0, 0, 0));
        entry.0 += 1;
        entry.1 += measurement.eligible_hits;
        entry.2 += measurement.fires;
    }
    println!("per build (replays, eligible hits, attributed fires):");
    for (build, (replays, hits, fires)) in &per_build {
        println!("  {build}: {replays}, {hits}, {fires}");
    }
}

/// Attributed fires can never exceed what the server recorded for the same
/// attacker/victim pair. This is the one external check on attribution: the
/// post-battle results are authoritative.
#[test]
#[ignore = "requires replays and a game install"]
fn attributed_fires_never_exceed_the_battle_results() {
    let corpus = corpus();
    print_corpus_summary(corpus);
    assert!(!corpus.measurements.is_empty(), "corpus produced no measurements at all");

    let mut compared = 0u32;
    let mut without_results = 0u32;
    for measurement in &corpus.measurements {
        let Some(server) = measurement.server_fires_by_ship.as_ref() else {
            without_results += 1;
            continue;
        };
        for (ship, ours) in &measurement.fires_by_ship {
            let recorded = server.get(ship).copied().unwrap_or(0);
            compared += 1;
            assert!(
                *ours <= recorded,
                "{}: attributed {ours} fires to {ship} but the server recorded {recorded}",
                measurement.name
            );
        }
    }
    println!("checked {compared} attacker/victim-ship pairs; {without_results} replays carried no usable results");
    assert!(compared > 0, "no replay carried post-battle results to check against");
}

/// A SetFire ribbon that matches no eligible hit. Nonzero by design: our own
/// secondaries with no main-battery candidate, fires on victims outside AOI, and
/// fires whose causing hit was excluded all land here legitimately. What is
/// being judged is the rate, not its existence.
#[test]
#[ignore = "requires replays and a game install"]
fn unattributed_ribbons_are_rare() {
    let corpus = corpus();
    let (mut unattributed, mut total) = (0u32, 0u32);
    for measurement in &corpus.measurements {
        println!(
            "{}: {} attributed, {} unattributed",
            measurement.name, measurement.fires, measurement.unattributed_fires
        );
        unattributed += measurement.unattributed_fires;
        total += measurement.fires + measurement.unattributed_fires;
    }
    assert!(total > 0, "corpus produced no SetFire ribbons at all");
    let rate = f64::from(unattributed) / f64::from(total);
    println!("unattributed rate {rate:.3} over {total} ribbons");
    assert!(rate < 0.15, "unattributed rate {rate:.3} is systemic, not a tail");
}

/// The load-bearing measurement. If nearest-node is what the server does, the
/// section we predict is the one that lights. Chance for a four-section ship is
/// 0.25, so anything near that means the model is wrong rather than merely
/// imprecise.
#[test]
#[ignore = "requires replays and a game install"]
fn predicted_sections_agree_with_the_server() {
    let corpus = corpus();
    let (mut agreed, mut compared) = (0u32, 0u32);
    // confusion[predicted][actual], so a constant offset shows as a shifted
    // diagonal and a non-positional rule as a flat block.
    let mut confusion = [[0u32; BurnNodeIndex::MAX_NODES as usize]; BurnNodeIndex::MAX_NODES as usize];

    for measurement in &corpus.measurements {
        if measurement.section_predictions.is_empty() {
            continue;
        }
        let replay_agreed =
            measurement.section_predictions.iter().filter(|(predicted, actual)| predicted == actual).count();
        println!(
            "{}: section agreement {:.3} over {} fires",
            measurement.name,
            replay_agreed as f64 / measurement.section_predictions.len() as f64,
            measurement.section_predictions.len()
        );
        for (predicted, actual) in &measurement.section_predictions {
            confusion[usize::from(*predicted)][usize::from(*actual)] += 1;
            compared += 1;
            if predicted == actual {
                agreed += 1;
            }
        }
    }

    assert!(compared > 50, "only {compared} attributed fires; corpus is too small to conclude");
    let rate = f64::from(agreed) / f64::from(compared);
    println!("overall section agreement {rate:.3} over {compared} fires");
    for (predicted, row) in confusion.iter().enumerate() {
        println!("  predicted {predicted}: {row:?}");
    }
    assert!(rate > 0.6, "section agreement {rate:.3} is near chance; the positional model is wrong");
}
