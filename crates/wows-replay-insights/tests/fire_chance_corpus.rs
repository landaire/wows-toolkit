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
//! Two of the tests reconcile the pipeline against the game's own accounting
//! rather than against itself. [`every_hit_the_game_credited_is_one_we_saw`]
//! checks that no hit the server raised a ribbon for is missing from the hit
//! history, since a hit we never see is a fire trial silently dropped, and
//! cross-checks the server's two hit accountings against each other.
//! [`our_impacts_land_on_the_hull_we_keyed_them_to`] then asks how many of
//! those impacts the body-frame projection actually places on the victim's
//! hull. It is under 100% and the doc comment says why, with the two
//! measurements that establish the cause; the gate there is a regression bound
//! on a limit of the packet stream, not an endorsement of the rate.
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
//! The dumped archives are also thin in one way that shows up in the printed
//! diagnostics: most crews in them carry no `Skills` table at all (67 of 632
//! for build 11965230, against 604 of 651 in a live install), so a victim's
//! learned skill ids cannot be named and `FirePrevention` resolves `Unknown`
//! for far more victims here than it would against a full install. That is a
//! property of the harness's data source, not of the eligibility model.
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
use wows_replay_insights::fire_chance::analysis::FireChanceInput;
use wows_replay_insights::fire_chance::analysis::analyze;
use wows_replay_insights::fire_chance::geometry::section_for_hit;
use wows_replay_insights::fire_chance::geometry::world_offset_to_body;
use wows_replay_insights::fire_chance::resolve::ResolutionDiagnostics;
use wows_replay_insights::fire_chance::resolve::resolve_fire_chance_input;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::analyzer::battle_controller::state::VictimPose;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::WorldDistance;
use wowsunpack::game_types::CollisionType;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::game_types::DamageStatWeapon;
use wowsunpack::game_types::Ribbon;
use wowsunpack::game_types::ShellHitType;
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
    /// Fire-capable main-battery shells fired, over the salvos the replay lets
    /// us see, against how many of them landed on a ship.
    he_shells_fired: u32,
    shells_on_target: u32,
    /// Of those, the ones that landed on a ship that was already dead.
    not_applicable: u32,
    eligible_hits: u32,
    fires: u32,
    /// The model's expected fire count over the same trials, saturating at one
    /// fire per section per tick exactly as `fires` does. `None` when the
    /// attacker's modifiers could not be folded for this build, which is a
    /// replay the observed/expected comparison simply has no expected side for.
    expected_fires: Option<f32>,
    unattributed_fires: u32,
    /// Every `SetFire` ribbon increment the replay carried, counted straight
    /// off the ribbon log rather than from anything `analyze` returned. This is
    /// the independent total the two buckets have to add back up to.
    set_fire_ribbons: u32,
    exclusions: BTreeMap<ExclusionReason, u32>,
    /// Attributed fires per victim ship index.
    fires_by_ship: BTreeMap<String, u64>,
    /// `(shells on target, eligible, refused, not applicable)` per victim ship
    /// row, for the same partition check the aggregate gets.
    per_ship_partition: Vec<(u32, u32, u32, u32)>,
    /// One row per victim ship. The per-ship row is the only level
    /// `EffectiveFireChance` states a rate at, so this is what the corpus
    /// summary reduces when it wants a corpus-wide figure, and what the printed
    /// distribution of sample sizes is taken over.
    per_ship_trials: Vec<ShipTrials>,
    /// Server-recorded fires per victim ship index, from the post-battle
    /// results. `None` when the replay carries no results blob or the build's
    /// constants table is missing, so the external check simply has nothing to
    /// compare against.
    server_fires_by_ship: Option<BTreeMap<String, u64>>,
    /// `(predicted, actual, independent)` per attributed fire. `independent` is
    /// false when the matched transition lit several sections at once, in which
    /// case `actual` was chosen as the risen bit nearest the prediction, and
    /// when the victim's hull has one fire section, in which case the pair
    /// agrees by construction. Neither can be scored as evidence for the
    /// positional model.
    section_predictions: Vec<(u8, u8, bool)>,
    diagnostics: ResolutionDiagnostics,
    /// Presence windows still open at the end of the parse, against the total.
    /// A window left open is `PresenceLog`'s one remaining route to a false
    /// "observed", so the ratio bounds how much the observation gate can be
    /// trusting more than it should.
    open_windows: u32,
    total_windows: u32,
    /// What the game said we hit, against what our hit pipeline accounted for.
    reconciliation: HitReconciliation,
}

/// One victim ship's trials in one replay, observed against predicted.
struct ShipTrials {
    ship: String,
    eligible_hits: u32,
    fires: u32,
    expected_fires: Option<f32>,
}

struct Corpus {
    measurements: Vec<Measurement>,
    skips: BTreeMap<SkipReason, u32>,
    skip_details: Vec<String>,
    replays_seen: u32,
    /// Measured replays whose packet stream stopped parsing before the end.
    truncated_parses: u32,
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

/// A report, plus whether the packet stream ran out before the end.
///
/// A replay whose packets stop parsing part-way is still measured rather than
/// skipped, because a truncated stream is not a wrong one: it just ends early.
/// The direction is safe (with no `BattleEnd` every victim's fate is `Unknown`
/// and every hit against them is refused, so the replay contributes near-zero
/// samples rather than corrupt ones) but it is exactly the kind of silent
/// population change the skip tally exists to surface, so it is counted too.
struct ParsedReport {
    report: BattleReport,
    truncated: bool,
}

fn build_report(replay: &ReplayFile, provider: &GameMetadataProvider, constants: &GameConstants) -> ParsedReport {
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
    let mut world = BattleWorld::new(&replay.meta, provider, Some(constants));
    // Without this the hit history is empty and every rate is zero-sample.
    world.set_record_hit_history(true);

    let mut parser = wows_replays::packet2::Parser::with_version(provider.entity_specs(), version);
    let mut remaining = replay.packet_data.as_slice();
    let mut truncated = false;
    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => world.process(&packet),
            Err(_) => {
                truncated = true;
                break;
            }
        }
    }
    world.finish();
    ParsedReport { report: world.into_report(), truncated }
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
        // A victim the recording player interacted with but never set alight
        // carries no `fires` key at all, which is a real zero rather than a
        // missing measurement.
        let count = victim.get("fires").and_then(|v| v.as_u64()).unwrap_or(0);
        *fires.entry(ship.clone()).or_insert(0) += count;
    }
    Some(fires)
}

/// Ribbons the game raises when one of our main-battery shells strikes a ship.
///
/// HE and SAP hits raise [`Ribbon::MainCaliber`]; AP hits raise the outcome
/// ribbon instead. A shell produces exactly one of these, so the family sums to
/// the hit count the player sees.
const MAIN_BATTERY_HIT_RIBBONS: [Ribbon; 6] = [
    Ribbon::MainCaliber,
    Ribbon::Citadel,
    Ribbon::Penetration,
    Ribbon::NonPenetration,
    Ribbon::OverPenetration,
    Ribbon::Ricochet,
];

/// What the game says we hit, against what our own hit pipeline accounted for.
#[derive(Clone, Debug, Default)]
struct HitReconciliation {
    /// Game-reported main-battery hits on a ship, summed over
    /// [`MAIN_BATTERY_HIT_RIBBONS`].
    main_battery_ribbons: u32,
    /// Game-reported secondary hits. Our own secondaries arrive on the same
    /// packet path as the main battery and are only separable once a salvo has
    /// matched, so [`Self::ours_on_ship`] covers both batteries and this is the
    /// other half of what it reconciles against.
    secondary_ribbons: u32,
    /// The `Penetration` and `OverPenetration` members of the main-battery
    /// family, i.e. the game-reported hits that dealt damage. Cross-checks
    /// [`Self::damage_stat_hits`], which counts the same population through a
    /// different server message.
    damaging_ribbons: u32,
    /// Server-authoritative main-battery hit count from the self damage stats
    /// (`MainAp`/`MainHe`/`MainCs` against the enemy category). `None` when the
    /// replay carried no damage stats at all.
    damage_stat_hits: Option<i64>,
    /// Every hit in the history the packet itself says we fired that struck a
    /// ship, both batteries together. Counted off `ShotHit::owner_id`, which is
    /// in the packet, so it needs no salvo match: this is the honest answer to
    /// "how many of our hits did the client see at all".
    ours_on_ship: u32,
    /// Of [`Self::ours_on_ship`], those whose salvo did not match. The shell is
    /// unnamed there, so the eligibility model never sees them.
    ours_without_a_salvo: u32,
    /// Our main-battery hits on a ship: the subset of [`Self::ours_on_ship`]
    /// whose salvo matched and named a shell from the equipped main battery.
    /// This is the population the projection runs over.
    main_battery_on_ship: u32,
    /// Of [`Self::main_battery_on_ship`], those keyed to a ship the client was
    /// observing at that instant. The rest are the visibility carve-out.
    observed: u32,
    /// Of [`Self::observed`], those whose victim resolved to a hull with
    /// fire-section geometry and whose pose at impact was known, i.e. the ones
    /// the projection could be run on at all.
    placeable: u32,
    /// Of [`Self::placeable`], those the projection placed on a hull.
    on_hull: u32,
    /// Body-frame offset for each hit the projection placed off the hull: how
    /// far past the node span it sat longitudinally, how far above the
    /// waterline, and how far off the centreline, all in meters. Zero
    /// longitudinally means the miss was purely off-axis.
    off_hull_misses: Vec<BodyOffset>,
    /// The same three numbers for every placeable hit, refused or not. Unlike
    /// [`Self::off_hull_misses`] this is uncensored by the gate, so it is what
    /// the gate's allowances can be derived from rather than confirmed against.
    placeable_offsets: Vec<BodyOffset>,
    /// Firing range in meters for each placeable hit, split by whether the
    /// projection placed it. The victim is resolved against the ships the client
    /// holds a live position for, which is the ships inside its area of
    /// interest, so if the residual off-hull hits are shells landing on ships
    /// outside it the two distributions separate by range.
    on_hull_ranges: Vec<f32>,
    off_hull_ranges: Vec<f32>,
    /// Placeable hits during whose shell flight some vehicle's presence window
    /// closed, i.e. some ship left the client's area of interest between the gun
    /// firing and the shell landing, split by whether the projection placed the
    /// hit. A vehicle that leaves loses its `Transform3d`, so it stops being a
    /// candidate for the nearest-ship victim resolution while shells aimed at it
    /// are still in the air.
    on_hull_after_a_departure: u32,
    off_hull_after_a_departure: u32,
    /// Our main-battery shells whose collision type says they struck something
    /// that is not a ship (water, terrain, a wave, or nothing), against how many
    /// of those the projection nevertheless placed on the keyed victim's hull.
    /// `classify` reads only the shell hit type, which is `Normal` for these, so
    /// the geometry gate is the only thing standing between a shell that hit an
    /// island and the fire-chance denominator.
    terrain_hits: u32,
    terrain_on_hull: u32,
    /// Main-battery hits of ours whose collision id this build's constants
    /// table does not name. `classify` cannot tell those apart from a hit on a
    /// ship, so this bounds how much of the corpus the collision gate cannot
    /// speak for.
    unnamed_collision: u32,
    /// Refused impacts whose shell hit type rolls for fire, split by whether
    /// they struck a ship at all. A rolling hit type is what it takes to reach
    /// the geometry check, so these bound the `ImpactOffTheHull` exclusion from
    /// above and say what it is made of: one is a shell that hit a ship and was
    /// keyed to the wrong victim, the other is a shell that hit no ship. They
    /// only bound it, because `classify` also refuses a shell whose burn chance
    /// is zero before asking the geometry, which takes every AP hit out.
    off_hull_rolling: u32,
    terrain_rolling: u32,
}

/// Server-authoritative main-battery hits against enemies, from the self damage
/// stats. `None` when the replay carried no damage stats.
fn damage_stat_main_battery_hits(report: &BattleReport) -> Option<i64> {
    let stats = report.self_damage_stats();
    if stats.is_empty() {
        return None;
    }
    Some(
        stats
            .iter()
            .filter(|entry| entry.category.known() == Some(&DamageStatCategory::Enemy))
            .filter(|entry| {
                matches!(
                    entry.weapon.known(),
                    Some(DamageStatWeapon::MainAp | DamageStatWeapon::MainHe | DamageStatWeapon::MainCs)
                )
            })
            .map(|entry| entry.count)
            .sum(),
    )
}

/// Whether a hit's collision type says the shell struck a ship.
fn struck_a_ship(hit: &ResolvedShotHit) -> bool {
    matches!(hit.hit.hit_type.collision.known(), Some(CollisionType::HitEntity | CollisionType::HitEntityBB))
}

/// Whether the collision id resolved to a name at all. An unnamed one is not
/// evidence either way, so it is counted apart from both populations.
fn collision_is_named(hit: &ResolvedShotHit) -> bool {
    hit.hit.hit_type.collision.known().is_some()
}

/// Whether `classify` would carry this hit as far as the geometry check.
/// Mirrors its `rolls_for_fire`, which is private to the analysis.
fn rolls_for_fire(hit: &ResolvedShotHit) -> bool {
    matches!(
        hit.hit.hit_type.shell_hit.known(),
        Some(ShellHitType::Normal | ShellHitType::MajorHit | ShellHitType::NoPenetration)
    )
}

/// Walk the hit history and count what became of every hit of ours, so the
/// game's own hit counts have something to be reconciled against.
///
/// Deliberately independent of `EffectiveFireChance::exclusions`: that tally
/// stops at the first reason a hit failed, so a ricochet is never asked whether
/// it lands on a hull and `ImpactOffTheHull` under-counts what the projection
/// really does. The projection is geometry and does not care what the shell did
/// on arrival, so it is measured over every main-battery hit that struck a ship.
fn reconcile_hits(
    report: &BattleReport,
    input: &FireChanceInput<'_>,
    placed: &HashMap<String, FireSectionGeometry>,
) -> HitReconciliation {
    let mut reconciliation = HitReconciliation::default();

    for ribbon in report.ribbon_events() {
        let count = ribbon.count as u32;
        if MAIN_BATTERY_HIT_RIBBONS.contains(&ribbon.ribbon) {
            reconciliation.main_battery_ribbons += count;
        }
        if matches!(ribbon.ribbon, Ribbon::Penetration | Ribbon::OverPenetration) {
            reconciliation.damaging_ribbons += count;
        }
        if ribbon.ribbon == Ribbon::SecondaryHit {
            reconciliation.secondary_ribbons += count;
        }
    }
    reconciliation.damage_stat_hits = damage_stat_main_battery_hits(report);

    for hit in report.hit_history() {
        if hit.hit.owner_id != input.attacker.entity {
            continue;
        }
        if struck_a_ship(hit) {
            reconciliation.ours_on_ship += 1;
            if hit.salvo.is_none() {
                reconciliation.ours_without_a_salvo += 1;
            }
        }

        // The shell can only be named through the salvo, and only a named shell
        // can be told from a secondary.
        let Some(salvo) = hit.salvo.as_ref() else { continue };
        let Some(param) = input.params.game_param_by_id(salvo.params_id) else { continue };
        if !input.attacker.main_battery_ammo.iter().any(|name| name == param.name()) {
            continue;
        }

        let placement = input
            .victims
            .get(&hit.victim_entity_id)
            .and_then(|victim| placed.get(&victim.hull_model_path))
            .zip(hit.victim_pose.as_ref())
            .map(|(geometry, pose)| {
                let section =
                    section_for_hit(geometry, hit.hit.position, pose.position, pose.yaw, pose.pitch, pose.roll);
                (geometry, pose, section)
            });

        if !collision_is_named(hit) {
            reconciliation.unnamed_collision += 1;
        }

        if !struck_a_ship(hit) {
            reconciliation.terrain_hits += 1;
            match placement.map(|(_, _, section)| section) {
                Some(Some(_)) => reconciliation.terrain_on_hull += 1,
                Some(None) if rolls_for_fire(hit) => reconciliation.terrain_rolling += 1,
                Some(None) | None => {}
            }
            continue;
        }
        reconciliation.main_battery_on_ship += 1;

        // A point query at the impact clock, the same one `classify` makes.
        if !report.presence().continuously_observed(hit.victim_entity_id, hit.clock, hit.clock) {
            continue;
        }
        reconciliation.observed += 1;

        let Some((geometry, pose, section)) = placement else { continue };
        reconciliation.placeable += 1;
        let offset = body_offset(geometry, hit, pose);
        reconciliation.placeable_offsets.push(offset);
        let range = firing_range(hit);
        let departure = a_vehicle_left_during_flight(report, hit);
        match section {
            Some(_) => {
                reconciliation.on_hull += 1;
                reconciliation.on_hull_ranges.extend(range);
                reconciliation.on_hull_after_a_departure += u32::from(departure);
            }
            None => {
                if rolls_for_fire(hit) {
                    reconciliation.off_hull_rolling += 1;
                }
                reconciliation.off_hull_misses.push(offset);
                reconciliation.off_hull_ranges.extend(range);
                reconciliation.off_hull_after_a_departure += u32::from(departure);
            }
        }
    }

    reconciliation
}

/// How far this shell flew, in meters: gun to impact, taken off the shot's own
/// entry in its salvo. `None` for a hit whose salvo did not match or whose salvo
/// carries no shot with this id.
fn firing_range(hit: &ResolvedShotHit) -> Option<f32> {
    let salvo = hit.salvo.as_ref()?;
    let shot = salvo.shots.iter().find(|shot| shot.shot_id == hit.hit.shot_id)?;
    let flight = hit.hit.position.0 - shot.origin.0;
    Some(WorldDistance::from(flight.x.hypot(flight.z)).to_meters().value())
}

/// Whether any vehicle left the client's area of interest while this shell was
/// in the air.
///
/// A departure closes that vehicle's presence window and strips its
/// `Transform3d`, which is exactly what takes it out of the candidate set the
/// victim is resolved against. `false` for a hit whose salvo did not match, so
/// no flight interval is known.
fn a_vehicle_left_during_flight(report: &BattleReport, hit: &ResolvedShotHit) -> bool {
    let Some(fired_at) = hit.fired_at else { return false };
    report.presence().windows.values().any(|windows| {
        windows.iter().any(|window| window.left.is_some_and(|left| left >= fired_at && left <= hit.clock))
    })
}

/// Where one impact sat in the victim's body frame, in meters.
#[derive(Clone, Copy, Debug)]
struct BodyOffset {
    /// Excess past the burn-node span toward either end, zero inside it.
    past_span: f32,
    /// Height above the hull origin. Negative is below the waterline.
    vertical: f32,
    /// Distance from the centreline, unsigned: port and starboard are
    /// symmetric, so the sign carries nothing the gate reads.
    lateral: f32,
}

/// The three body-frame numbers the plausibility gate is built out of. Recorded
/// for every placeable hit, not only the refused ones, so the gate's allowances
/// can be read off the distribution instead of being confirmed by a population
/// the gate itself selected.
fn body_offset(geometry: &FireSectionGeometry, hit: &ResolvedShotHit, pose: &VictimPose) -> BodyOffset {
    let body = world_offset_to_body(hit.hit.position.0 - pose.position.0, pose.yaw, pose.pitch, pose.roll);
    let longitudinal = WorldDistance::from(body.x).to_meters().value();
    let nodes = geometry.longitudinal();
    let bow = nodes.iter().map(|node| node.value()).fold(f32::NEG_INFINITY, f32::max);
    let stern = nodes.iter().map(|node| node.value()).fold(f32::INFINITY, f32::min);
    BodyOffset {
        past_span: (longitudinal - bow).max(stern - longitudinal).max(0.0),
        vertical: WorldDistance::from(body.y).to_meters().value(),
        lateral: WorldDistance::from(body.z).to_meters().value().abs(),
    }
}

/// Quantiles of `values` at `HULL_AXIS_QUANTILES`, plus the extremes.
fn quantiles(values: &mut [f32]) -> String {
    values.sort_by(f32::total_cmp);
    if values.is_empty() {
        return "no samples".to_owned();
    }
    let at = |q: f64| {
        let index = ((values.len() as f64 - 1.0) * q).round() as usize;
        values[index]
    };
    let parts: Vec<String> = HULL_AXIS_QUANTILES.iter().map(|q| format!("p{:.5}={:.1}", q * 100.0, at(*q))).collect();
    format!("min={:.1} {} max={:.1}", values[0], parts.join(" "), values[values.len() - 1])
}

const HULL_AXIS_QUANTILES: [f64; 6] = [0.5, 0.9, 0.99, 0.999, 0.9999, 0.99999];

/// The vertical and lateral distributions the plausibility gate's allowances are
/// read off, over the impacts that sit inside the hull's longitudinal span.
///
/// Scoping to the longitudinally plausible ones is what makes the tail
/// interpretable: a hit keyed to a ship a kilometre away is off in every axis at
/// once and would otherwise dominate both distributions. Ships are tall and
/// narrow, so the two axes are printed apart rather than as one radius.
fn print_axis_distributions(totals: &HitTotals) {
    let mut vertical: Vec<f32> =
        totals.placeable_offsets.iter().filter(|o| o.past_span == 0.0).map(|o| o.vertical).collect();
    let mut lateral: Vec<f32> =
        totals.placeable_offsets.iter().filter(|o| o.past_span == 0.0).map(|o| o.lateral).collect();
    println!("impacts inside the node span, {} of {}:", vertical.len(), totals.placeable_offsets.len());
    println!("  vertical (m above the hull origin): {}", quantiles(&mut vertical));
    println!("  lateral (m off the centreline): {}", quantiles(&mut lateral));

    let mut placed = totals.on_hull_ranges.clone();
    let mut refused = totals.off_hull_ranges.clone();
    println!("firing range in meters, gun to impact:");
    println!("  placed on a hull ({} shells): {}", placed.len(), quantiles(&mut placed));
    println!("  refused ({} shells): {}", refused.len(), quantiles(&mut refused));

    let refused_count = totals.placeable - totals.on_hull;
    println!(
        "some ship left the client's area of interest during the shell's flight for {} of {} placed hits \
({:.3}) and {} of {} refused ones ({:.3})",
        totals.on_hull_after_a_departure,
        totals.on_hull,
        f64::from(totals.on_hull_after_a_departure) / f64::from(totals.on_hull.max(1)),
        totals.off_hull_after_a_departure,
        refused_count,
        f64::from(totals.off_hull_after_a_departure) / f64::from(refused_count.max(1)),
    );
}

fn measure(
    name: String,
    build: u32,
    replay: &ReplayFile,
    data: &BuildData,
    geometry: &Geometry,
    corpus: &mut Corpus,
) -> Option<Measurement> {
    let parsed =
        match std::panic::catch_unwind(AssertUnwindSafe(|| build_report(replay, &data.provider, &data.game_constants)))
        {
            Ok(parsed) => parsed,
            Err(_) => {
                *corpus.skips.entry(SkipReason::ReportUnbuildable).or_insert(0) += 1;
                corpus.skip_details.push(format!("{name}: building the report panicked"));
                return None;
            }
        };
    let ParsedReport { report, truncated } = parsed;
    if truncated {
        corpus.truncated_parses += 1;
        corpus.skip_details.push(format!("{name}: packet stream stopped parsing early, measured anyway"));
    }

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
    let reconciliation = reconcile_hits(&report, &input, &placed);
    let Some(out) = analyze(&input) else {
        *corpus.skips.entry(SkipReason::AnalysisRefused).or_insert(0) += 1;
        corpus.skip_details.push(format!("{name}: analyze refused (no placed hull, or the attacker has no tier)"));
        return None;
    };

    let (mut open_windows, mut total_windows) = (0u32, 0u32);
    for windows in report.presence().windows.values() {
        for window in windows {
            total_windows += 1;
            if window.left.is_none() {
                open_windows += 1;
            }
        }
    }

    let fires_by_ship =
        out.per_ship.iter().map(|ship| (ship.victim_ship_index.clone(), u64::from(ship.fires))).collect();
    let per_ship_trials = out
        .per_ship
        .iter()
        .map(|ship| ShipTrials {
            ship: ship.victim_ship_index.clone(),
            eligible_hits: ship.eligible_hits,
            fires: ship.fires,
            expected_fires: ship.expected_fires,
        })
        .collect();
    let server_fires_by_ship =
        data.constants_json.as_ref().and_then(|constants| server_fires_by_ship(&report, constants));

    Some(Measurement {
        name,
        build,
        he_shells_fired: out.he_shells_fired,
        shells_on_target: out.shells_on_target,
        not_applicable: out.not_applicable,
        eligible_hits: out.eligible_hits,
        fires: out.fires,
        expected_fires: out.expected_fires,
        unattributed_fires: out.unattributed_fires,
        set_fire_ribbons: report
            .ribbon_events()
            .iter()
            .filter(|event| event.ribbon == Ribbon::SetFire)
            .map(|event| event.count as u32)
            .sum(),
        exclusions: out.exclusions.clone(),
        fires_by_ship,
        per_ship_partition: out
            .per_ship
            .iter()
            .map(|ship| {
                (ship.shells_on_target, ship.eligible_hits, ship.exclusions.values().sum(), ship.not_applicable)
            })
            .collect(),
        per_ship_trials,
        server_fires_by_ship,
        section_predictions: section_pairs(&out),
        diagnostics: resolution.diagnostics(),
        open_windows,
        total_windows,
        reconciliation,
    })
}

fn section_pairs(out: &EffectiveFireChance) -> Vec<(u8, u8, bool)> {
    out.section_predictions
        .iter()
        .map(|pair| (pair.predicted.get(), pair.actual.get(), pair.is_independent_evidence()))
        .collect()
}

/// Wilson score interval lower bound at 95% for `agreed` successes in `total`
/// trials.
///
/// The gate compares this rather than the point estimate against the baseline,
/// so it asks whether the corpus establishes the positional model beats a
/// position-blind one rather than whether this particular draw happened to.
fn wilson_lower_bound(agreed: u32, total: u32) -> f64 {
    const Z: f64 = 1.96;
    let n = f64::from(total);
    let p = f64::from(agreed) / n;
    let denominator = 1.0 + Z * Z / n;
    let center = (p + Z * Z / (2.0 * n)) / denominator;
    let half_width = Z * (p * (1.0 - p) / n + Z * Z / (4.0 * n * n)).sqrt() / denominator;
    center - half_width
}

fn build_corpus() -> Corpus {
    let mut corpus = Corpus {
        measurements: Vec::new(),
        skips: BTreeMap::new(),
        skip_details: Vec::new(),
        replays_seen: 0,
        truncated_parses: 0,
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

/// Printed once per process, by whichever test reaches it first, so a failure
/// in any of the three carries the population it was measured over even when
/// that test is run on its own.
fn print_corpus_summary(corpus: &Corpus) {
    static PRINTED: std::sync::Once = std::sync::Once::new();
    PRINTED.call_once(|| print_corpus_summary_once(corpus));
}

fn print_corpus_summary_once(corpus: &Corpus) {
    println!("corpus: {} replays seen, {} measured", corpus.replays_seen, corpus.measurements.len());
    for (reason, count) in &corpus.skips {
        println!("  skipped {count} for {reason:?}");
    }
    for detail in &corpus.skip_details {
        println!("    {detail}");
    }
    println!(
        "measured replays whose packet stream stopped early: {} of {}",
        corpus.truncated_parses,
        corpus.measurements.len()
    );
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

    print_rate_summary(corpus);
}

/// The corpus totals, plus the two ways of reducing them to one number.
///
/// Neither reduction is a product statistic and the app reports neither: fire
/// resistance is the victim's, so a figure spanning several victims has to pick
/// a weighting and no weighting is the right one. The pooled ratio weights
/// every eligible hit equally, so the most heavily hit targets carry it; the
/// unweighted mean of the per-ship rates weights every target ship equally, so
/// a ship hit once carries as much as one hit eighty times. Both are printed
/// here as harness diagnostics against the distribution of sample sizes, which
/// is what shows how far apart the two definitions sit and how thin the rows
/// underneath them are.
fn print_rate_summary(corpus: &Corpus) {
    let (mut hits, mut fires) = (0u64, 0u64);
    let mut ship_rates: Vec<f32> = Vec::new();
    let mut ship_hits: Vec<f32> = Vec::new();
    for measurement in &corpus.measurements {
        hits += u64::from(measurement.eligible_hits);
        fires += u64::from(measurement.fires);
        for ship in &measurement.per_ship_trials {
            if ship.eligible_hits == 0 {
                continue;
            }
            ship_rates.push(ship.fires as f32 / ship.eligible_hits as f32);
            ship_hits.push(ship.eligible_hits as f32);
        }
    }
    println!("eligible hits {hits}, attributed fires {fires}");
    if hits > 0 {
        println!("  pooled ratio {:.4} (ignores per-ship fire resistance)", fires as f64 / hits as f64);
    }
    if !ship_rates.is_empty() {
        let mean = f64::from(ship_rates.iter().sum::<f32>()) / ship_rates.len() as f64;
        println!("  unweighted mean of per-ship rates {mean:.4} over {} target ships", ship_rates.len());
        let singles = ship_hits.iter().filter(|h| **h == 1.0).count();
        let under_five = ship_hits.iter().filter(|h| **h < 5.0).count();
        println!(
            "  eligible hits per target ship: {} ({singles} rows of exactly 1, {under_five} under 5)",
            quantiles(&mut ship_hits)
        );
    }
    print_expected_summary(corpus);
}

/// Observed fires against the model's expectation, pooled and per target ship.
///
/// Both sides count the same event, a fire the replay could show, so their
/// ratio is the model's error. It is pooled only over the replays and ships
/// whose expectation is known: a replay whose modifiers would not fold has no
/// expected side, and taking its observed fires into the ratio anyway would
/// inflate the numerator against an expectation that never covered them.
fn print_expected_summary(corpus: &Corpus) {
    let (mut hits, mut fires, mut expected) = (0u64, 0u64, 0f64);
    let mut without_expectation = 0u32;
    for measurement in &corpus.measurements {
        let Some(replay_expected) = measurement.expected_fires else {
            without_expectation += 1;
            continue;
        };
        hits += u64::from(measurement.eligible_hits);
        fires += u64::from(measurement.fires);
        expected += f64::from(replay_expected);
    }
    println!(
        "expected fires {expected:.2} against {fires} observed over {hits} eligible hits \
         ({without_expectation} replays carried no expectation)"
    );
    if expected > 0.0 {
        println!("  pooled observed/expected {:.4}", fires as f64 / expected);
    }

    let mut per_ship: BTreeMap<&str, (u64, u64, f64)> = BTreeMap::new();
    for measurement in &corpus.measurements {
        for ship in &measurement.per_ship_trials {
            let Some(ship_expected) = ship.expected_fires else { continue };
            let entry = per_ship.entry(ship.ship.as_str()).or_insert((0, 0, 0.0));
            entry.0 += u64::from(ship.eligible_hits);
            entry.1 += u64::from(ship.fires);
            entry.2 += f64::from(ship_expected);
        }
    }
    let mut rows: Vec<(&str, u64, u64, f64)> =
        per_ship.into_iter().map(|(ship, (hits, fires, expected))| (ship, hits, fires, expected)).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    println!("per target ship, at least {PER_SHIP_REPORTING_FLOOR} eligible hits (hits, observed, expected, ratio):");
    for (ship, hits, fires, expected) in rows.iter().filter(|row| row.1 >= PER_SHIP_REPORTING_FLOOR) {
        let ratio = if *expected > 0.0 { format!("{:.3}", *fires as f64 / expected) } else { "n/a".to_owned() };
        println!("  {ship}: {hits}, {fires}, {expected:.2}, {ratio}");
    }
}

/// How many eligible hits a target ship needs before its own observed/expected
/// ratio is printed. A row of two hits carries a ratio of 0 or of several
/// hundred percent and nothing in between, so printing every row would bury the
/// ones that say something.
const PER_SHIP_REPORTING_FLOOR: u64 = 30;

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
            // A ship absent from the server's interactions is a ship the
            // recording player set no fires on, so the bound to check our
            // attribution against is zero, not "unknown".
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

/// Every `SetFire` ribbon lands in exactly one bucket: attributed to a hit, or
/// unattributed.
///
/// The unattributed **rate** is printed but not asserted on. A model that
/// refuses every ambiguous hit by design leaves a large share of fires
/// unassignable, and that share is a property of how conservative the
/// eligibility rules are rather than evidence of a fault. The exclusion tally
/// printed below is what it is made of: `NotMainBattery` is our own
/// secondaries, whose fires raise a ribbon that no main-battery hit can then
/// claim; `DamageControlUnknown` is a hit that could have started a fire and
/// could not be proven to; `MergedSectionVictimBuildUnknown` is inflated
/// on this corpus by dumped archives carrying no crew skill tables (see the
/// module doc). Gating on the rate would be gating partly on the harness's own
/// data. Worth revisiting against a full-data corpus.
///
/// What is asserted is the accounting. `attribute` walks the same ribbon slice
/// emitting one outcome per ribbon-count unit, so the equality holds by loop
/// construction for most ways it could go wrong: a dropped candidate only moves
/// a fire from attributed to unattributed and leaves the sum alone, and a
/// double-consumed ribbon is not expressible, since what gets consumed is a
/// candidate. Two things it does catch, both of which have bitten this kind of
/// code before: a `count > 1` ribbon credited once instead of `count` times,
/// and any future divergence between the ribbon set `analyze` reads and the set
/// the report carries.
#[test]
#[ignore = "requires replays and a game install"]
fn ribbon_accounting_reconciles() {
    let corpus = corpus();
    print_corpus_summary(corpus);
    let (mut attributed, mut unattributed, mut observed) = (0u32, 0u32, 0u32);
    let (mut fired, mut on_target, mut not_applicable) = (0u64, 0u64, 0u64);
    let mut drivers: BTreeMap<ExclusionReason, u32> = BTreeMap::new();

    for measurement in &corpus.measurements {
        assert_eq!(
            measurement.fires + measurement.unattributed_fires,
            measurement.set_fire_ribbons,
            "{}: {} attributed + {} unattributed does not account for the {} SetFire ribbons observed",
            measurement.name,
            measurement.fires,
            measurement.unattributed_fires,
            measurement.set_fire_ribbons
        );
        // Implied by the equality above while every ribbon is accounted for,
        // and kept because it is the bound that still has to hold if the model
        // ever stops accounting for some of them.
        assert!(
            measurement.unattributed_fires <= measurement.set_fire_ribbons,
            "{}: {} unattributed exceeds the {} SetFire ribbons observed",
            measurement.name,
            measurement.unattributed_fires,
            measurement.set_fire_ribbons
        );

        let refused: u32 = measurement.exclusions.values().sum();
        assert_eq!(
            measurement.eligible_hits + refused + measurement.not_applicable,
            measurement.shells_on_target,
            "{}: {} eligible + {refused} refused + {} not applicable does not account for the {} shells on target",
            measurement.name,
            measurement.eligible_hits,
            measurement.not_applicable,
            measurement.shells_on_target
        );
        for (index, (on_target, eligible, refused, not_applicable)) in measurement.per_ship_partition.iter().enumerate()
        {
            assert_eq!(
                eligible + refused + not_applicable,
                *on_target,
                "{}: per-ship row {index} does not account for its own hits",
                measurement.name
            );
        }
        fired += u64::from(measurement.he_shells_fired);
        on_target += u64::from(measurement.shells_on_target);
        not_applicable += u64::from(measurement.not_applicable);
        attributed += measurement.fires;
        unattributed += measurement.unattributed_fires;
        observed += measurement.set_fire_ribbons;
        for (reason, count) in &measurement.exclusions {
            *drivers.entry(*reason).or_insert(0) += count;
        }
    }

    assert!(observed > 0, "corpus produced no SetFire ribbons at all");
    println!(
        "ribbons {observed}: {attributed} attributed, {unattributed} unattributed (rate {:.3})",
        f64::from(unattributed) / f64::from(observed)
    );
    // Printed rather than gated. The fired count covers only the salvos a hit
    // led us back to and the on-target count covers every shell of ours that
    // landed, HE or not, so neither bounds the other; the ratio is a diagnostic
    // on how much of the shell stream the salvo match recovers.
    println!("HE shells fired {fired}, shells on target {on_target}, of which {not_applicable} landed on a dead ship");

    let mut ranked: Vec<(ExclusionReason, u32)> = drivers.into_iter().collect();
    ranked.sort_by_key(|(reason, count)| (std::cmp::Reverse(*count), *reason));
    println!("exclusion tally, largest first:");
    for (reason, count) in ranked.iter().take(5) {
        println!("  {reason:?}: {count}");
    }
}

/// The load-bearing measurement. If nearest-node is what the server does, the
/// section we predict is the one that lights.
///
/// The gate is on the **independent** pairs. Two kinds are dropped. When one
/// transition lights several sections at once, `analyze` reports the risen bit
/// nearest the prediction as the actual one, so predicted and actual are not
/// independent there and scoring those pairs biases the rate upward; a
/// battleship salvo setting two fires inside one server tick is ordinary, so
/// that share is not negligible and is printed rather than assumed away. And a
/// hull with a single fire section can only predict section 0 and can only have
/// lit section 0, so those pairs are noise-free padding that would drag the rate
/// toward 1 whatever the positional model does. The all-pairs rate is printed
/// beside the gated one: the gap between the two is the size of the bias.
///
/// Chance is not 0.25 either. Sections are not lit uniformly, so the number to
/// beat is what the best predictor that knows the marginal distribution and
/// nothing about position would score, and the gate is against that computed
/// baseline rather than a fixed threshold.
#[test]
#[ignore = "requires replays and a game install"]
fn predicted_sections_agree_with_the_server() {
    let corpus = corpus();
    print_corpus_summary(corpus);
    let (mut agreed, mut compared) = (0u32, 0u32);
    let (mut agreed_independent, mut independent) = (0u32, 0u32);
    // confusion[predicted][actual] over the independent pairs only, so a
    // constant offset shows as a shifted diagonal and a non-positional rule as
    // a flat block, without the nearest-of-several pairs pulling it diagonal.
    let mut confusion = [[0u32; BurnNodeIndex::MAX_NODES as usize]; BurnNodeIndex::MAX_NODES as usize];
    let mut actual_marginal = [0u32; BurnNodeIndex::MAX_NODES as usize];

    for measurement in &corpus.measurements {
        if measurement.section_predictions.is_empty() {
            continue;
        }
        let replay_agreed =
            measurement.section_predictions.iter().filter(|(predicted, actual, _)| predicted == actual).count();
        println!(
            "{}: section agreement {:.3} over {} fires",
            measurement.name,
            replay_agreed as f64 / measurement.section_predictions.len() as f64,
            measurement.section_predictions.len()
        );
        for (predicted, actual, is_independent) in &measurement.section_predictions {
            compared += 1;
            if predicted == actual {
                agreed += 1;
            }
            if !is_independent {
                continue;
            }
            independent += 1;
            confusion[usize::from(*predicted)][usize::from(*actual)] += 1;
            actual_marginal[usize::from(*actual)] += 1;
            if predicted == actual {
                agreed_independent += 1;
            }
        }
    }

    assert!(compared > 50, "only {compared} attributed fires; corpus is too small to conclude");
    println!("all pairs: agreement {:.3} over {compared} fires", f64::from(agreed) / f64::from(compared));
    println!(
        "of those, {independent} lit exactly one section and {} lit several; only the first kind is \
independent evidence, since the second picks the actual section nearest the prediction",
        compared - independent
    );

    assert!(
        independent > 50,
        "only {independent} fires lit exactly one section; not enough independent evidence to conclude"
    );
    let rate = f64::from(agreed_independent) / f64::from(independent);
    let shares: Vec<f64> = actual_marginal.iter().map(|count| f64::from(*count) / f64::from(independent)).collect();
    // Two different position-blind baselines, and only the second is the bar.
    // `collision` is the chance two draws from the observed distribution match,
    // which is what a predictor sampling that distribution scores. The best
    // position-blind predictor does not sample it, it always names the most
    // common section, scoring `best_blind = max_i p_i >= collision`. Both are
    // well above the 0.25 a uniform four-section guess implies.
    let collision: f64 = shares.iter().map(|share| share * share).sum();
    let best_blind = shares.iter().copied().fold(0.0f64, f64::max);
    let lower_bound = wilson_lower_bound(agreed_independent, independent);
    println!(
        "single-section agreement {rate:.3} over {independent} fires (95% lower bound {lower_bound:.3}; \
best position-blind predictor {best_blind:.3}, distribution-sampling predictor {collision:.3})"
    );
    for (predicted, row) in confusion.iter().enumerate() {
        println!("  predicted {predicted}: {row:?}");
    }

    // Against the computed baseline, not a hard-coded number: the bar depends
    // on how unevenly this corpus's fires are distributed across sections, and
    // a fixed threshold would move relative to it on a different draw.
    assert!(
        lower_bound > best_blind,
        "section agreement {rate:.3} over {independent} fires (95% lower bound {lower_bound:.3}) does not beat \
the {best_blind:.3} a predictor that always named the most common section would score; the positional model \
is not established"
    );
}

/// Aggregate of [`HitReconciliation`] over the corpus, so the two tests below
/// state the same population once.
#[derive(Default)]
struct HitTotals {
    main_battery_ribbons: u32,
    secondary_ribbons: u32,
    damaging_ribbons: u32,
    damage_stat_hits: i64,
    ours_on_ship: u32,
    ours_without_a_salvo: u32,
    main_battery_on_ship: u32,
    observed: u32,
    placeable: u32,
    on_hull: u32,
    terrain_hits: u32,
    terrain_on_hull: u32,
    unnamed_collision: u32,
    off_hull_rolling: u32,
    terrain_rolling: u32,
    /// Hits the game credited that we never saw, summed per replay so a surplus
    /// in one match cannot cover a shortfall in another.
    shortfall: u32,
    off_hull_misses: Vec<BodyOffset>,
    placeable_offsets: Vec<BodyOffset>,
    on_hull_ranges: Vec<f32>,
    off_hull_ranges: Vec<f32>,
    on_hull_after_a_departure: u32,
    off_hull_after_a_departure: u32,
}

fn hit_totals(corpus: &Corpus) -> HitTotals {
    let mut totals = HitTotals::default();
    for measurement in &corpus.measurements {
        let r = &measurement.reconciliation;
        totals.main_battery_ribbons += r.main_battery_ribbons;
        totals.secondary_ribbons += r.secondary_ribbons;
        totals.damaging_ribbons += r.damaging_ribbons;
        totals.damage_stat_hits += r.damage_stat_hits.unwrap_or(0);
        totals.ours_on_ship += r.ours_on_ship;
        totals.ours_without_a_salvo += r.ours_without_a_salvo;
        totals.main_battery_on_ship += r.main_battery_on_ship;
        totals.observed += r.observed;
        totals.placeable += r.placeable;
        totals.on_hull += r.on_hull;
        totals.terrain_hits += r.terrain_hits;
        totals.terrain_on_hull += r.terrain_on_hull;
        totals.unnamed_collision += r.unnamed_collision;
        totals.off_hull_rolling += r.off_hull_rolling;
        totals.terrain_rolling += r.terrain_rolling;
        totals.shortfall += game_hits(r).saturating_sub(r.ours_on_ship);
        totals.off_hull_misses.extend_from_slice(&r.off_hull_misses);
        totals.placeable_offsets.extend_from_slice(&r.placeable_offsets);
        totals.on_hull_ranges.extend_from_slice(&r.on_hull_ranges);
        totals.off_hull_ranges.extend_from_slice(&r.off_hull_ranges);
        totals.on_hull_after_a_departure += r.on_hull_after_a_departure;
        totals.off_hull_after_a_departure += r.off_hull_after_a_departure;
    }
    totals
}

/// Every hit the game credited us with on a ship, both batteries. This is the
/// count [`HitReconciliation::ours_on_ship`] has to reach: `ShotHit::owner_id`
/// does not say which battery fired, so the two batteries are reconciled
/// together rather than one of them against a mixed population.
fn game_hits(reconciliation: &HitReconciliation) -> u32 {
    reconciliation.main_battery_ribbons + reconciliation.secondary_ribbons
}

/// Every hit the game credited us with is one we also saw land.
///
/// This is the external check on the hit pipeline: the hit ribbons are the
/// server's own count of our shells that struck a ship, and a hit we cannot
/// account for is a fire trial silently dropped. `ShotHit::owner_id` is in the
/// packet, so our side of the comparison needs no salvo match and covers both
/// batteries; the game side is therefore the main-battery family plus
/// `SecondaryHit`, since nothing separates the two before a salvo has matched.
///
/// Only the shortfall is bounded. A surplus is expected and large (measured
/// +9.5% over this corpus) because the client receives a `receiveShotKills`
/// record for impacts the server raises no ribbon for: shells arriving on a
/// ship that is already dead, and hits on scenario buildings and other
/// non-scoring entities. The three largest surpluses are all operations or
/// co-op, which is what that population looks like. The bound is on the summed
/// per-replay shortfall rather than on the corpus net, so a surplus in one
/// match cannot cover a shortfall in another.
///
/// Measured over 53 replays: the game credited 14988 hits and the pipeline saw
/// 16406, with a summed shortfall of 19 hits (0.13%) from three replays; the
/// worst single replay is 16 short of 354 (4.5%). The gate is 1%, seven times
/// the measured rate, which still catches any systematic loss: a battery, a hit
/// type or a game version dropping out entirely moves this by whole percent.
#[test]
#[ignore = "requires replays and a game install"]
fn every_hit_the_game_credited_is_one_we_saw() {
    let corpus = corpus();
    print_corpus_summary(corpus);
    let totals = hit_totals(corpus);
    let game = totals.main_battery_ribbons + totals.secondary_ribbons;
    assert!(game > 0, "the corpus produced no hit ribbons at all, so there is nothing to reconcile against");

    for measurement in &corpus.measurements {
        let r = &measurement.reconciliation;
        let expected = game_hits(r);
        println!(
            "{}: game credited {expected} hits ({} main battery, {} secondary), we saw {} ({} with no salvo); \
main-battery hits on a ship {}, on no ship {}; damaging ribbons {} against damage-stat hits {:?}",
            measurement.name,
            r.main_battery_ribbons,
            r.secondary_ribbons,
            r.ours_on_ship,
            r.ours_without_a_salvo,
            r.main_battery_on_ship,
            r.terrain_hits,
            r.damaging_ribbons,
            r.damage_stat_hits,
        );
    }
    for measurement in &corpus.measurements {
        let r = &measurement.reconciliation;
        let missing = game_hits(r).saturating_sub(r.ours_on_ship);
        if missing > 0 {
            println!("  {} is {missing} hits short of the {} the game credited", measurement.name, game_hits(r));
        }
    }
    println!(
        "corpus: game credited {game} hits, we saw {} ({:+.3} relative); summed per-replay shortfall {} ({:.4})",
        totals.ours_on_ship,
        f64::from(totals.ours_on_ship) / f64::from(game) - 1.0,
        totals.shortfall,
        f64::from(totals.shortfall) / f64::from(game),
    );
    println!(
        "of the hits we saw, {} arrived with no matching salvo, so the shell is unnamed and the eligibility \
model never sees them",
        totals.ours_without_a_salvo
    );

    // The game's two accountings of the same population, checked against each
    // other. `damage_stat_hits` counts main-battery hits that dealt damage,
    // which is the Penetration and OverPenetration ribbons; citadels and
    // non-penetrations are not in it. They agree exactly over this corpus, so
    // the ribbon log is not losing increments and the damage stats are not
    // counting something else.
    println!(
        "game cross-check: {} damaging main-battery ribbons against {} main-battery hits in the damage stats",
        totals.damaging_ribbons, totals.damage_stat_hits
    );
    for measurement in &corpus.measurements {
        let r = &measurement.reconciliation;
        if r.damage_stat_hits.is_some_and(|hits| hits != i64::from(r.damaging_ribbons)) {
            println!(
                "  cross-check disagrees on {}: {} damaging ribbons, {:?} damage-stat hits",
                measurement.name, r.damaging_ribbons, r.damage_stat_hits
            );
        }
    }
    let cross_check_gap = f64::from(totals.damaging_ribbons.abs_diff(totals.damage_stat_hits.max(0) as u32))
        / f64::from(totals.damaging_ribbons);
    assert!(
        cross_check_gap <= 0.05,
        "the game's two hit accountings differ by {cross_check_gap:.4}, against the 0.026 measured when this \
bound was set; neither can be used as ground truth without saying which one is wrong"
    );

    let shortfall_rate = f64::from(totals.shortfall) / f64::from(game);
    assert!(
        shortfall_rate <= 0.01,
        "{} of the {game} hits the game credited were never seen by the pipeline ({shortfall_rate:.4}), against \
the 0.0014 measured when this bound was set",
        totals.shortfall
    );
}

/// A hit we saw land on a ship we were watching is a hit we can place on that
/// ship's hull.
///
/// The projection is what turns an impact into a fire section, so an impact it
/// refuses is a trial dropped. The expectation is that essentially all of them
/// land, and they do not: **92.6% over this corpus, 12702 of 13721**.
///
/// The residual is not geometric noise, and it is not the coordinate scale
/// either. The misses are kilometres out: a median of 1447 m past the hull's
/// node span and a worst of 6265 m, with only 97 of 1019 within 200 m of the
/// hull. A wrong unit scale would move the placed population as well rather
/// than leaving this clean split between impacts on the hull and impacts a
/// ship's length away from any hull. The printed body-frame distributions say
/// the same from the other side: over impacts inside a node span the lateral
/// offset is 6.3 m at the median and 22.9 m at the 99th percentile, which is
/// hull beams, and the vertical runs 6 m under the waterline to 35 m over it,
/// which is drafts and superstructures.
///
/// The residual is a limit of what the client was told, not of the projection.
/// `receiveShotKills` names no victim, so the victim is the nearest ship the
/// client holds a live position for, and it holds one only for ships inside its
/// area of interest. A ship that leaves takes its `Transform3d` with it, so a
/// shell already in the air toward it lands with nothing near it to be keyed to.
/// Two independent measurements say that is what the refusals are:
/// - not one refused impact came from a shell fired under 3.2 km, while placed
///   impacts run down to 58 m. Short flights do not outlast a sighting.
/// - a ship left the client's area of interest during the shell's flight for
///   83% of the refused impacts against 38% of the placed ones.
///
/// Resolving the victim per hit from its own impact position, rather than once
/// per salvo from the salvo's average aim point, moved this rate by 0.24 points
/// (12680 to 12713 of 13721). It is the right model and it removes the
/// straddling-salvo failure outright, but it was never what this rate was made
/// of: guns are aimed at the ship they are shooting at, so the aim point was
/// already a fair proxy for the victim's identity whenever the victim was in the
/// candidate set at all, and no way of choosing among candidates helps when the
/// ship that was hit is not one of them.
///
/// The gate is 0.915, and the headroom under the measurement is itself
/// measured: dropping any one replay from the corpus moves the pooled rate only
/// between 0.9209 and 0.9340, so no single replay carries it and 0.915 sits
/// under that minimum. A failure is therefore a change in the model rather than
/// a corpus drawn differently. It is a regression bound on a known data limit,
/// not a claim that 92.6% is acceptable.
///
/// Two populations are held out, and both are printed, because each is a way
/// the rate could be flattered:
/// - hits on a victim the client was not observing at that instant. The
///   expectation is explicitly scoped to visible ships, and this is the size of
///   that carve-out: 36 of 13980, 0.26%. It is far too small to explain
///   anything.
/// - hits whose victim never resolved to a hull with geometry, or whose pose at
///   impact was unknown (223). The projection cannot run there at all, so
///   scoring them as misses would measure the data rather than the geometry.
#[test]
#[ignore = "requires replays and a game install"]
fn our_impacts_land_on_the_hull_we_keyed_them_to() {
    let corpus = corpus();
    print_corpus_summary(corpus);
    let totals = hit_totals(corpus);
    assert!(totals.placeable > 100, "only {} placeable hits; the corpus is too small to conclude", totals.placeable);

    for measurement in &corpus.measurements {
        let r = &measurement.reconciliation;
        if r.placeable == 0 {
            continue;
        }
        println!(
            "{}: {} main-battery hits on a ship, {} observed, {} placeable, {} on hull ({:.3})",
            measurement.name,
            r.main_battery_on_ship,
            r.observed,
            r.placeable,
            r.on_hull,
            f64::from(r.on_hull) / f64::from(r.placeable),
        );
    }

    let rate = f64::from(totals.on_hull) / f64::from(totals.placeable);
    println!(
        "on-hull rate {rate:.4} over {} placeable hits ({} placed, {} refused)",
        totals.placeable,
        totals.on_hull,
        totals.placeable - totals.on_hull
    );
    println!(
        "carve-outs: {} of {} main-battery ship hits were on a victim we were not observing ({:.4}), and {} \
more had no resolvable hull or pose",
        totals.main_battery_on_ship - totals.observed,
        totals.main_battery_on_ship,
        f64::from(totals.main_battery_on_ship - totals.observed) / f64::from(totals.main_battery_on_ship),
        totals.observed - totals.placeable,
    );

    // How far off the refused impacts sat. A projection that was merely
    // imprecise would miss by tens of meters; a mis-keyed victim misses by
    // kilometres, which is a different ship, not a different section.
    let mut past_span: Vec<f32> = totals.off_hull_misses.iter().map(|miss| miss.past_span).collect();
    past_span.sort_by(f32::total_cmp);
    if let (Some(median), Some(worst)) = (past_span.get(past_span.len() / 2), past_span.last()) {
        let near = past_span.iter().filter(|distance| **distance < 200.0).count();
        println!(
            "off-hull misses: median {median:.0} m past the node span, worst {worst:.0} m, {near} of {} within \
200 m of the hull",
            past_span.len()
        );
    }
    print_axis_distributions(&totals);

    // Impacts on water and terrain are in the same history and carry
    // `SHELL_HIT_TYPE_NORMAL`, so the shell hit type alone reads them as hits
    // that roll for fire. `classify` reads the collision type and refuses them
    // outright; what is measured here is the geometry on its own, which is the
    // second guard and no longer the only one.
    println!(
        "{} main-battery shells of ours struck something other than a ship; the projection placed {} of them \
on a hull; {} carried a collision id the build's constants table does not name, which the eligibility model \
cannot rule either way",
        totals.terrain_hits, totals.terrain_on_hull, totals.unnamed_collision
    );
    // What the `ImpactOffTheHull` exclusion is made of, in the two kinds it
    // conflates: a shell that hit a ship and was keyed to the wrong one, and a
    // shell that hit no ship at all and should never have been a trial. Both
    // counts are a superset of the tally, since `classify` refuses a shell whose
    // burn chance is zero before it ever asks the geometry, which takes every AP
    // hit out.
    println!(
        "refused impacts that would reach the geometry check: {} struck a ship and were keyed to the wrong one, \
{} struck no ship at all",
        totals.off_hull_rolling, totals.terrain_rolling
    );

    // Zero of 952 now, against 5 before the lateral allowance was separated from
    // the vertical one. The bound is the rule of three: with no event in 952
    // samples the true rate is under 3/952 at 95% confidence, so that is the
    // most this measurement can support and anything above it is a real event
    // rather than a draw.
    let terrain_rate = f64::from(totals.terrain_on_hull) / f64::from(totals.terrain_hits);
    assert!(
        terrain_rate <= 3.0 / f64::from(totals.terrain_hits),
        "{} of {} impacts that struck no ship were placed on a victim's hull ({terrain_rate:.4}), against none \
of 952 measured when this bound was set; each one is a fire trial that could never have started a fire",
        totals.terrain_on_hull,
        totals.terrain_hits
    );

    // How far any one replay carries the pooled rate. The gate has to survive a
    // corpus drawn slightly differently, so the headroom under it is measured
    // rather than assumed.
    let jackknife = leave_one_out_range(corpus);
    if let Some((low, high)) = jackknife {
        println!("leave-one-replay-out on-hull rate: {low:.4} to {high:.4}");
    }

    assert!(
        rate >= 0.915,
        "the projection placed {rate:.4} of our impacts on the hull they were keyed to, under the 0.9257 \
measured when this bound was set; the refusals are shells that landed on a ship the client had no live position \
for, so this rate is a property of what the client was told rather than of the projection"
    );
}

/// The lowest and highest pooled on-hull rate over the corpus with one replay
/// dropped, which is how much of the rate any single replay is carrying.
fn leave_one_out_range(corpus: &Corpus) -> Option<(f64, f64)> {
    let total_placeable: u32 = corpus.measurements.iter().map(|m| m.reconciliation.placeable).sum();
    let total_on_hull: u32 = corpus.measurements.iter().map(|m| m.reconciliation.on_hull).sum();
    let rates = corpus.measurements.iter().filter_map(|m| {
        let placeable = total_placeable.checked_sub(m.reconciliation.placeable).filter(|left| *left > 0)?;
        Some(f64::from(total_on_hull - m.reconciliation.on_hull) / f64::from(placeable))
    });
    let (low, high) = rates.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), r| (lo.min(r), hi.max(r)));
    low.is_finite().then_some((low, high))
}
