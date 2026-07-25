//! Gated: requires build-matched game data on disk (see wows-replay-insights
//! test harness). No-ops when data is absent so CI without game data stays
//! green.
//!
//! `map_rows` requires a `Replay` with both `battle_report` and `ui_report`
//! populated. Building `ui_report` (`UiReport::new`) needs the full GUI-app
//! pipeline: a `SharedWoWsData` with loaded icon caches and `egui::TextureHandle`s,
//! plus `ReplayDependencies` (background-task channel, twitch state, sort
//! order). None of that is constructible from a headless integration test, and
//! it is intentionally not part of this crate's public API. So this test can
//! only exercise the `battle_report`-only path (`ui_report: None`), which is a
//! real and useful assertion: `map_rows` must return `None` until both reports
//! are present, matching `Replay::battle_results_are_pending`-style gating
//! elsewhere in the app.
//!
//! The pure helpers (`outcome_from`, `relation_from`) are the enforced unit
//! coverage; see `crates/wows-toolkit/src/data/replay_index.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use jiff::Timestamp;
use wows_battle_world::BattleWorld;
use wows_battle_world::ids::ShotTracking;
use wows_battle_world::report::BattleReport;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::game_constants::GameConstants;
use wows_toolkit::Replay;
use wows_toolkit::replay_index::map_rows;
use wows_toolkit_config::index::rows::SourceId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_data;
use wowsunpack::game_params::provider::GameMetadataProvider;

const REPLAY: &str = "20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay";

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().join("tests").join("fixtures")
}

struct Loaded {
    replay_file: ReplayFile,
    provider: GameMetadataProvider,
    game_constants: GameConstants,
}

/// Mirrors `crates/wows-replay-insights/tests/normalized_report.rs::try_load`.
fn try_load() -> Option<Loaded> {
    let path = fixtures_dir().join("replays").join(REPLAY);
    let replay_file = ReplayFile::from_file(&path).ok()?;
    let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);

    let game_dir = wows_data_mgr::game_dir_for_build(version.build_number()?)?;
    let resources = game_data::load_game_resources(&game_dir, &version).ok()?;

    let provider = GameMetadataProvider::from_vfs(&resources.vfs).ok()?;
    let game_constants = GameConstants::from_vfs(&resources.vfs);

    Some(Loaded { replay_file, provider, game_constants })
}

/// Build a `BattleReport` from the replay, mirroring `run_players_query`.
fn build_report(loaded: &Loaded) -> BattleReport {
    let mut world = BattleWorld::new(&loaded.replay_file.meta, &loaded.provider, Some(&loaded.game_constants));
    world.set_shot_tracking(ShotTracking::Untracked);

    let mut parser = wows_replays::packet2::Parser::with_version(
        loaded.provider.entity_specs(),
        Version::from_client_exe(&loaded.replay_file.meta.clientVersionFromExe),
    );
    let mut remaining = loaded.replay_file.packet_data.as_slice();
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
fn map_rows_requires_ui_report() {
    let Some(loaded) = try_load() else {
        eprintln!("Skipping replay_index_mapper smoke test: requires local game data for the fixture replay's build");
        return;
    };

    let report = build_report(&loaded);
    let provider = Arc::new(loaded.provider);
    let mut replay = Replay::new(loaded.replay_file, provider);
    replay.battle_report = Some(report);
    // replay.ui_report intentionally left None (see module docs above).

    let rows = map_rows(&replay, SourceId(1), Timestamp::now());
    assert!(rows.is_none(), "map_rows must require ui_report, not just battle_report");
}
