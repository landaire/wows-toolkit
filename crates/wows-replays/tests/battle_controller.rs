//! Integration tests for the BattleController replay processing pipeline.
//!
//! These tests parse fixture replays through the full BattleController, verifying
//! that game state is correctly built from packet data. This exercises the entire
//! chain: replay parsing → packet decoding → state accumulation → battle report.
//!
//! Tests are ignored when the required game data build is not available.

use std::path::PathBuf;

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::Parser;
use wowsunpack::data::Version;
use wowsunpack::game_data;
use wowsunpack::game_params::provider::GameMetadataProvider;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("replays")
}

/// Process an entire replay through BattleController and return it for assertions.
///
/// This is the same pipeline used by the minimap renderer and the UI replay viewer:
/// parse replay → load game data → create controller → feed all packets → finish.
fn run_replay(filename: &str) -> (ReplayFile, BattleController<'static, 'static, GameMetadataProvider>) {
    let path = fixtures_dir().join(filename);
    let replay = ReplayFile::from_file(&path).unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let game_dir = wows_data_mgr::game_dir_for_build(version.build)
        .unwrap_or_else(|| panic!("game data for build {} not available", version.build));
    let resources = game_data::load_game_resources(&game_dir, &version).expect("should load game resources");

    let game_params =
        GameMetadataProvider::from_vfs(&resources.vfs).map_err(|e| panic!("failed to load GameParams: {e:?}")).unwrap();
    let game_constants = GameConstants::from_vfs(&resources.vfs);

    // Leak to get 'static lifetimes — fine for tests.
    let game_params: &'static GameMetadataProvider = Box::leak(Box::new(game_params));
    let game_constants: &'static GameConstants = Box::leak(Box::new(game_constants));
    let replay: &'static ReplayFile = Box::leak(Box::new(replay));

    let mut controller = BattleController::new(&replay.meta, game_params, Some(game_constants));

    let mut parser = Parser::new(&resources.specs);
    let mut remaining = &replay.packet_data[..];

    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).expect("should parse packet");
        controller.process(&packet);
    }
    controller.finish();

    // SAFETY: We leaked the data above, so the references are valid for 'static.
    // Clone what we need to return owned values for the test assertions.
    // The replay ref is 'static so we can deref it.
    let replay_clone =
        ReplayFile::from_decrypted_parts(replay.raw_meta.as_bytes().to_vec(), replay.packet_data.clone())
            .expect("should reconstruct replay");
    (replay_clone, controller)
}

// =============================================================================
// v15.1 Vermont (PvP, 12v12, Tierra del Fuego)
// =============================================================================

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn vermont_players_populated() {
    let (replay, controller) = run_replay("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");

    // metadata_players should have all players from the replay
    let meta_players = controller.metadata_players();
    assert_eq!(meta_players.len(), replay.meta.vehicles.len(), "metadata player count should match replay vehicles");

    // player_entities should be populated from packet processing
    let players = controller.player_entities();
    assert!(!players.is_empty(), "player_entities should not be empty after processing");

    // The recording player should be among them
    let has_recording_player = players.values().any(|p| p.initial_state().username() == replay.meta.playerName);
    assert!(has_recording_player, "recording player '{}' should be in player_entities", replay.meta.playerName);
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn vermont_battle_completes() {
    let (_replay, controller) = run_replay("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");

    // A completed PvP replay should have a battle end
    assert!(controller.battle_end_clock().is_some(), "battle should have ended");
    assert!(controller.winning_team().is_some(), "should have a winning team");
    assert!(controller.finish_type().is_some(), "should have a finish type");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn vermont_has_positions() {
    let (_replay, controller) = run_replay("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");

    // Ships should have reported positions during the game
    let positions = controller.minimap_positions();
    assert!(!positions.is_empty(), "minimap_positions should not be empty");

    let ship_positions = controller.ship_positions();
    assert!(!ship_positions.is_empty(), "ship_positions should not be empty");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn vermont_has_kills() {
    let (_replay, controller) = run_replay("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");

    // A 12v12 PvP game should have at least some kills
    let kills = controller.kills();
    assert!(!kills.is_empty(), "a full PvP game should have kills");

    // Dead ships should track where ships died (may differ from kill count
    // since kills can involve non-ship entities or ships without tracked positions)
    let dead = controller.dead_ships();
    assert!(!dead.is_empty(), "should have dead ships");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn vermont_has_team_scores() {
    let (_replay, controller) = run_replay("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");

    let scores = controller.team_scores();
    assert!(!scores.is_empty(), "should have team scores");

    // At least one team should have scored points
    assert!(scores.iter().any(|s| s.score > 0), "at least one team should have points");
}

// =============================================================================
// v15.1 Marceau (PvP, 12v12, Tierra del Fuego)
// =============================================================================

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn marceau_full_pipeline() {
    let (replay, controller) = run_replay("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");

    // Players
    let meta_players = controller.metadata_players();
    assert_eq!(meta_players.len(), replay.meta.vehicles.len());

    let players = controller.player_entities();
    assert!(!players.is_empty());

    // Battle result
    assert!(controller.battle_end_clock().is_some(), "battle should have ended");
    assert!(controller.winning_team().is_some(), "should have a winning team");

    // Positions
    assert!(!controller.minimap_positions().is_empty());
    assert!(!controller.ship_positions().is_empty());

    // Kills and scores
    assert!(!controller.kills().is_empty(), "PvP game should have kills");
    let scores = controller.team_scores();
    assert!(scores.iter().any(|s| s.score > 0));
}

// =============================================================================
// v15.1 Narai PvE (operations)
// =============================================================================

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn narai_pve_pipeline() {
    let (replay, controller) = run_replay("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");

    // PvE has fewer human players but still has metadata entries
    let meta_players = controller.metadata_players();
    assert!(!meta_players.is_empty(), "should have metadata players");

    // Should have player entities
    let players = controller.player_entities();
    assert!(!players.is_empty(), "should have player entities");

    // PvE matches should also complete
    assert!(controller.battle_end_clock().is_some(), "PvE battle should have ended");

    // The recording player should exist
    let has_player = players.values().any(|p| p.initial_state().username() == replay.meta.playerName);
    assert!(has_player, "recording player should be present");

    // PvE should have positions
    assert!(!controller.minimap_positions().is_empty(), "should have minimap positions");
}

// =============================================================================
// v14.1 Hull DD (PvP, Sleeping Giant)
// =============================================================================

#[test]
#[cfg_attr(not(all(has_game_data, has_build_9531281)), ignore)]
fn hull_dd_pipeline() {
    let (replay, controller) = run_replay("20250206_020938_PASD720-Hull_47_Sleeping_Giant.wowsreplay");

    assert_eq!(controller.metadata_players().len(), replay.meta.vehicles.len());

    let players = controller.player_entities();
    assert!(!players.is_empty());

    // Verify recording player
    let has_player = players.values().any(|p| p.initial_state().username() == "Biiison");
    assert!(has_player, "recording player 'Biiison' should be present");

    assert!(controller.battle_end_clock().is_some(), "battle should have ended");
    assert!(!controller.minimap_positions().is_empty());
    assert!(!controller.kills().is_empty(), "PvP game should have kills");
}

// =============================================================================
// v12.3 S-189 Submarine (PvP, Neighbors)
// =============================================================================

#[test]
#[cfg_attr(not(all(has_game_data, has_build_6965290)), ignore)]
fn s189_submarine_pipeline() {
    let (replay, controller) = run_replay("20230419_203306_PRSS508-S-189_42_Neighbors.wowsreplay");

    assert_eq!(controller.metadata_players().len(), replay.meta.vehicles.len());

    let players = controller.player_entities();
    assert!(!players.is_empty());

    let has_player = players.values().any(|p| p.initial_state().username() == "TF2_Electric_Boogaloo");
    assert!(has_player, "recording player should be present");

    assert!(controller.battle_end_clock().is_some(), "battle should have ended");
    assert!(!controller.minimap_positions().is_empty());
}

// =============================================================================
// Cross-version: all fixture replays with available game data
// =============================================================================

/// For every fixture replay whose game data is available, verify the
/// BattleController pipeline doesn't panic and produces basic state.
#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn all_available_replays_produce_state() {
    let dir = fixtures_dir();
    let available_builds = wows_data_mgr::available_builds();

    let mut tested = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("wowsreplay") {
            continue;
        }

        let replay = ReplayFile::from_file(&path).unwrap();
        let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

        if !available_builds.contains(&version.build) {
            continue;
        }

        let game_dir = match wows_data_mgr::game_dir_for_build(version.build) {
            Some(d) => d,
            None => continue,
        };

        let resources = match game_data::load_game_resources(&game_dir, &version) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let game_params = match GameMetadataProvider::from_vfs(&resources.vfs) {
            Ok(gp) => gp,
            Err(_) => continue,
        };

        let game_constants = GameConstants::from_vfs(&resources.vfs);

        let game_params: &'static GameMetadataProvider = Box::leak(Box::new(game_params));
        let game_constants: &'static GameConstants = Box::leak(Box::new(game_constants));
        let replay: &'static ReplayFile = Box::leak(Box::new(replay));

        let mut controller = BattleController::new(&replay.meta, game_params, Some(game_constants));

        let mut parser = Parser::new(&resources.specs);
        let mut remaining = &replay.packet_data[..];
        while !remaining.is_empty() {
            let packet = parser
                .parse_packet(&mut remaining)
                .unwrap_or_else(|e| panic!("packet parse error in {}: {e:?}", path.display()));
            controller.process(&packet);
        }
        controller.finish();

        // Basic sanity: every replay should produce metadata players and at least some positions
        assert!(!controller.metadata_players().is_empty(), "no metadata players for {}", path.display());
        assert!(!controller.player_entities().is_empty(), "no player entities for {}", path.display());

        tested += 1;
    }

    assert!(tested > 0, "no replays were tested — check available game data");
}
