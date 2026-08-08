#![cfg(feature = "battle-report")]
//! Integration golden for `NormalizedBattleReport::from_battle_report`.
//!
//! Reuses the fixture-loading harness from `inventory_seeding.rs`: it loads the
//! replay's game data via `wows_data_mgr` and builds a `BattleReport` through
//! `BattleWorld` exactly like `replayshark::run_players_query`. The wows-constants
//! JSON the builder needs is loaded from a checked-in fixture; the test skips
//! with a message when game data or the constants fixture is unavailable.

use std::path::PathBuf;

use wows_battle_world::BattleWorld;
use wows_battle_world::ids::ShotTracking;
use wows_battle_world::report::BattleReport;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_data;
use wowsunpack::game_params::provider::GameMetadataProvider;

const REPLAY: &str = "20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay";

fn shared_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("replays")
}

fn crate_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

struct Loaded {
    replay: ReplayFile,
    provider: &'static GameMetadataProvider,
    game_constants: &'static GameConstants,
    version: Version,
    constants: serde_json::Value,
}

fn try_load() -> Option<Loaded> {
    let path = shared_fixtures_dir().join(REPLAY);
    let replay = ReplayFile::from_file(&path).ok()?;
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let game_dir = wows_data_mgr::game_dir_for_build(version.build_number()?)?;
    let resources = game_data::load_game_resources(&game_dir, &version).ok()?;

    let game_params = GameMetadataProvider::from_vfs(&resources.vfs).ok()?;
    let game_constants = GameConstants::from_vfs(&resources.vfs);

    // The wows-constants results-indices JSON is not derivable from the VFS; it
    // lives in the padtrack/wows-constants repo. Loaded here from a fixture.
    let constants_path = crate_fixtures_dir().join("constants").join(format!("{}.json", version.build_number()?));
    let constants: serde_json::Value =
        std::fs::read(&constants_path).ok().and_then(|b| serde_json::from_slice(&b).ok())?;

    let game_params: &'static GameMetadataProvider = Box::leak(Box::new(game_params));
    let game_constants: &'static GameConstants = Box::leak(Box::new(game_constants));

    Some(Loaded { replay, provider: game_params, game_constants, version, constants })
}

/// Build a `BattleReport` from the replay, mirroring `run_players_query`.
fn build_report(loaded: &Loaded) -> BattleReport {
    let mut world = BattleWorld::new(&loaded.replay.meta, loaded.provider, Some(loaded.game_constants));
    world.set_shot_tracking(ShotTracking::Untracked);

    let mut parser = wows_replays::packet2::Parser::with_version(loaded.provider.entity_specs(), loaded.version);
    let mut remaining = loaded.replay.packet_data();
    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => world.process(&packet),
            Err(_) => break,
        }
    }
    world.finish();
    world.into_report()
}

#[test]
fn normalized_report_populates_server_results_and_interactions() {
    let Some(loaded) = try_load() else {
        eprintln!(
            "Skipping normalized_report golden: requires local game data for build 11965230 \
             and the constants fixture tests/fixtures/constants/11965230.json"
        );
        return;
    };

    let report = build_report(&loaded);

    // This replay carries a battle-results block; without it there is nothing to
    // assert and we would rely solely on the offline unit test.
    assert!(report.battle_results().is_some(), "fixture replay is expected to carry battle results");

    let normalized =
        NormalizedBattleReport::from_battle_report(&report, &loaded.replay.meta, loaded.provider, &loaded.constants);

    assert!(!normalized.players.is_empty(), "report should have players");

    // The recording player (self) reliably has server-provided results in a
    // completed battle. Values below are structural, not pinned magic numbers:
    // this dev machine lacks the game data to run the builder end-to-end and
    // capture exact damage, so the golden asserts shape rather than a snapshot.
    let self_player = normalized.players.iter().find(|p| p.is_self).expect("self player present");

    let sr = self_player.server_results.as_ref().expect("self player has server_results");
    assert!(sr.damage.unwrap_or(0) > 0, "self player dealt nonzero damage");

    assert!(
        sr.damage_interactions.values().any(|i| i.damage_dealt_by_type.ap.is_some()
            || i.damage_dealt_by_type.he.is_some()
            || i.damage_dealt_by_type.sap.is_some()),
        "at least one interaction carries a per-type damage breakdown"
    );

    assert!(!self_player.ribbons.is_empty(), "self player earned ribbons");
}
