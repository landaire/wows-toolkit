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
use wows_replays::analyzer::battle_controller::BattleReport;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::analyzer::decoder::DecodedPacketPayload;
use wows_replays::analyzer::decoder::PacketDecoder;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::Parser;
use wowsunpack::battle_results::resolve_battle_results;
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
// v13.2 Annapolis (PvP, Tierra del Fuego)
// =============================================================================

#[test]
#[cfg_attr(not(all(has_game_data, has_build_8151735)), ignore)]
fn annapolis_pipeline() {
    let (replay, controller) = run_replay("20240402_192304_PASC111-Annapolis_22_tierra_del_fuego.wowsreplay");

    assert_eq!(controller.metadata_players().len(), replay.meta.vehicles.len());

    let players = controller.player_entities();
    assert!(!players.is_empty());

    let has_player = players.values().any(|p| p.initial_state().username() == "ChineseTechAbuser");
    assert!(has_player, "recording player should be present");

    assert!(controller.battle_end_clock().is_some(), "battle should have ended");
    assert!(!controller.minimap_positions().is_empty());
}

// =============================================================================
// v13.10 Colbert (PvP, Path Warrior)
// =============================================================================

#[test]
#[cfg_attr(not(all(has_game_data, has_build_9129736)), ignore)]
fn colbert_pipeline() {
    let (replay, controller) = run_replay("20241112_172819_PFSC510-Colbert_44_Path_warrior.wowsreplay");

    assert_eq!(controller.metadata_players().len(), replay.meta.vehicles.len());

    let players = controller.player_entities();
    assert!(!players.is_empty());

    let has_player = players.values().any(|p| p.initial_state().username() == "John_The_Ruthless");
    assert!(has_player, "recording player should be present");

    assert!(controller.battle_end_clock().is_some(), "battle should have ended");
    assert!(!controller.minimap_positions().is_empty());
}

// =============================================================================
// v14.2 Oland (PvP, NE North)
// =============================================================================

#[test]
#[cfg_attr(not(all(has_game_data, has_build_9643943)), ignore)]
fn oland_pipeline() {
    let (replay, controller) = run_replay("20250117_004534_PWSD108-Oland_15_NE_north.wowsreplay");

    assert_eq!(controller.metadata_players().len(), replay.meta.vehicles.len());

    let players = controller.player_entities();
    assert!(!players.is_empty());

    let has_player = players.values().any(|p| p.initial_state().username() == "awesome101_21x");
    assert!(has_player, "recording player should be present");

    assert!(controller.battle_end_clock().is_some(), "battle should have ended");
    assert!(!controller.minimap_positions().is_empty());
}

// =============================================================================
// v14.9 Ocean CV (Event, Naval Mission)
// =============================================================================

#[test]
#[cfg_attr(not(all(has_game_data, has_build_10695045)), ignore)]
fn ocean_cv_event_pipeline() {
    let (_replay, controller) = run_replay("20251001_145225_PBSA710-Ocean_28_naval_mission.wowsreplay");

    let meta_players = controller.metadata_players();
    assert!(!meta_players.is_empty());

    let players = controller.player_entities();
    assert!(!players.is_empty());

    let has_player = players.values().any(|p| p.initial_state().username() == "seaznutz");
    assert!(has_player, "recording player should be present");

    assert!(controller.battle_end_clock().is_some(), "battle should have ended");
    assert!(!controller.minimap_positions().is_empty());
}

// =============================================================================
// v15.0 Forrest Sherman (PvP, Angel Wings)
// =============================================================================

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11791718)), ignore)]
fn forrest_sherman_pipeline() {
    let (replay, controller) = run_replay("20260127_185500_PASD610-Forrest-Sherman_56_AngelWings.wowsreplay");

    assert_eq!(controller.metadata_players().len(), replay.meta.vehicles.len());

    let players = controller.player_entities();
    assert!(!players.is_empty());

    let has_player = players.values().any(|p| p.initial_state().username() == "QUIDPROQUOWINKWINK");
    assert!(has_player, "recording player should be present");

    // This replay was quit before the battle ended, so no battle_end_clock.
    assert!(!controller.minimap_positions().is_empty());
}

// =============================================================================
// Observed vs reported damage accuracy tests
// =============================================================================

/// Process an entire replay and return the finalized BattleReport (with vehicle damage computed).
fn run_replay_report(filename: &str) -> (ReplayFile, BattleReport) {
    let path = fixtures_dir().join(filename);
    let replay = ReplayFile::from_file(&path).unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let game_dir = wows_data_mgr::game_dir_for_build(version.build)
        .unwrap_or_else(|| panic!("game data for build {} not available", version.build));
    let resources = game_data::load_game_resources(&game_dir, &version).expect("should load game resources");

    let game_params =
        GameMetadataProvider::from_vfs(&resources.vfs).map_err(|e| panic!("failed to load GameParams: {e:?}")).unwrap();
    let game_constants = GameConstants::from_vfs(&resources.vfs);

    let game_params: &'static GameMetadataProvider = Box::leak(Box::new(game_params));
    let game_constants: &'static GameConstants = Box::leak(Box::new(game_constants));
    let replay_leaked: &'static ReplayFile = Box::leak(Box::new(replay));

    let mut controller = BattleController::new(&replay_leaked.meta, game_params, Some(game_constants));

    let mut parser = Parser::new(&resources.specs);
    let mut remaining = &replay_leaked.packet_data[..];

    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).expect("should parse packet");
        controller.process(&packet);
    }
    controller.finish();

    let report = controller.build_report();
    let replay_clone =
        ReplayFile::from_decrypted_parts(replay_leaked.raw_meta.as_bytes().to_vec(), replay_leaked.packet_data.clone())
            .expect("should reconstruct replay");
    (replay_clone, report)
}

fn constants() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("constants.json");
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read constants.json: {e}"));
    serde_json::from_slice(&data).expect("failed to parse constants.json")
}

/// Extract the self player's reported `damage` field from resolved battle results.
fn self_player_reported_damage(report: &BattleReport, constants: &serde_json::Value) -> u64 {
    let db_id_str = report.self_player().initial_state().db_id().0.to_string();

    let raw_results = report.battle_results().expect("replay should have battle results");
    let parsed: serde_json::Value = serde_json::from_str(raw_results).expect("battle results should be valid JSON");
    let resolved = resolve_battle_results(parsed, constants);

    resolved
        .pointer(&format!("/playersPublicInfo/{db_id_str}/damage"))
        .unwrap_or_else(|| panic!("no damage field for self player {db_id_str}"))
        .as_u64()
        .expect("damage should be a number")
}

/// Find the self player's observed damage from the server-authoritative DamageStat.
///
/// DamageStat provides f64 per-type cumulative totals from the server. The game's
/// battle results report an integer, but the exact rounding algorithm used by the
/// server is not consistent (sometimes ceil-of-total, sometimes ceil-per-type).
/// The result may differ from the battle results integer by ±1.
fn self_player_observed_damage(report: &BattleReport) -> u64 {
    let vehicle = report.self_player().vehicle_entity().expect("self player should have a vehicle entity");
    vehicle.damage().ceil() as u64
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn vermont_observed_damage_matches_reported() {
    let (replay, report) = run_replay_report("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
    let constants = constants();

    let reported = self_player_reported_damage(&report, &constants);
    let observed = self_player_observed_damage(&report);
    let delta = (observed as i64 - reported as i64).unsigned_abs();

    assert!(
        delta <= 1,
        "Vermont ({}) observed damage ({observed}) != reported damage ({reported}), delta = {}",
        replay.meta.playerName,
        observed as i64 - reported as i64,
    );
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn marceau_observed_damage_matches_reported() {
    let (replay, report) = run_replay_report("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
    let constants = constants();

    let reported = self_player_reported_damage(&report, &constants);
    let observed = self_player_observed_damage(&report);
    let delta = (observed as i64 - reported as i64).unsigned_abs();

    assert!(
        delta <= 1,
        "Marceau ({}) observed damage ({observed}) != reported damage ({reported}), delta = {}",
        replay.meta.playerName,
        observed as i64 - reported as i64,
    );
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn narai_observed_damage_matches_reported() {
    let (replay, report) = run_replay_report("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
    let constants = constants();

    let reported = self_player_reported_damage(&report, &constants);
    let observed = self_player_observed_damage(&report);
    let delta = (observed as i64 - reported as i64).unsigned_abs();

    assert!(
        delta <= 1,
        "Narai ({}) observed damage ({observed}) != reported damage ({reported}), delta = {}",
        replay.meta.playerName,
        observed as i64 - reported as i64,
    );
}

// =============================================================================
// DamageStat per-packet regression tests
//
// These snapshot every decoded DamageStat packet from each fixture replay.
// Each receiveDamageStat RPC produces a DamageStat(Vec<DamageStatEntry>) with
// cumulative (weapon, category, count, total) tuples. The weapon/category are
// resolved through BattleConstants ID→name→enum mappings.
//
// If those mappings change (e.g. we update defaults and forget a version check),
// weapons will resolve as Unknown("1") instead of Known(MainAp), and these
// snapshots will fail — showing exactly which packet and which entry changed.
// =============================================================================

/// Parse a replay and collect all decoded DamageStat packets.
///
/// Uses `PacketDecoder` with default constants (what we're testing hasn't drifted).
/// Returns entries sorted by (weapon, category) for deterministic snapshots.
fn collect_damage_stat_packets(filename: &str) -> Vec<Vec<wows_replays::analyzer::decoder::DamageStatEntry>> {
    let path = fixtures_dir().join(filename);
    let replay = ReplayFile::from_file(&path).expect("should parse replay");
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let game_dir = wows_data_mgr::game_dir_for_build(version.build)
        .unwrap_or_else(|| panic!("game data for build {} not available", version.build));
    let resources = game_data::load_game_resources(&game_dir, &version).expect("should load game resources");

    let mut parser = Parser::new(&resources.specs);
    let decoder = PacketDecoder::builder().version(version).build();

    let mut remaining = &replay.packet_data[..];
    let mut damage_stats = Vec::new();

    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).expect("should parse packet");
        let decoded = decoder.decode(&packet);

        if let DecodedPacketPayload::DamageStat(ref entries) = decoded.payload {
            let mut sorted = entries.clone();
            sorted.sort_by_key(|e| (format!("{:?}", e.weapon), format!("{:?}", e.category)));
            damage_stats.push(sorted);
        }
    }

    damage_stats
}

/// Vermont v15.1 — battleship, AP main battery only.
/// Snapshots every receiveDamageStat packet to catch enum mapping regressions.
/// Each packet is a partial cumulative update — the snapshot captures every
/// individual decoded packet so changes in ID→enum mappings are visible.
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn vermont_damage_stat_packets() {
    let packets = collect_damage_stat_packets("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");

    assert!(!packets.is_empty(), "should have DamageStat packets");

    // Snapshot all decoded DamageStat packets. If enum mappings change,
    // entries will show Unknown("1") instead of Known(MainAp), etc.
    insta::assert_yaml_snapshot!("vermont_damage_stat_packets", &packets);

    // Every entry across all packets must resolve to Known variants.
    for (i, entries) in packets.iter().enumerate() {
        for entry in entries {
            assert!(
                entry.weapon.is_known() && entry.category.is_known(),
                "packet {i}: unknown mapping: {:?} / {:?}",
                entry.weapon,
                entry.category,
            );
        }
    }
}

/// Marceau v15.1 — destroyer with HE, torpedoes, fire, and ram damage.
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn marceau_damage_stat_packets() {
    let packets = collect_damage_stat_packets("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");

    assert!(!packets.is_empty(), "should have DamageStat packets");
    insta::assert_yaml_snapshot!("marceau_damage_stat_packets", &packets);

    for (i, entries) in packets.iter().enumerate() {
        for entry in entries {
            assert!(
                entry.weapon.is_known() && entry.category.is_known(),
                "packet {i}: unknown mapping: {:?} / {:?}",
                entry.weapon,
                entry.category,
            );
        }
    }
}

/// Narai v15.1 — PvE operation with AP, HE, and fire damage.
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn narai_damage_stat_packets() {
    let packets = collect_damage_stat_packets("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");

    assert!(!packets.is_empty(), "should have DamageStat packets");
    insta::assert_yaml_snapshot!("narai_damage_stat_packets", &packets);

    for (i, entries) in packets.iter().enumerate() {
        for entry in entries {
            assert!(
                entry.weapon.is_known() && entry.category.is_known(),
                "packet {i}: unknown mapping: {:?} / {:?}",
                entry.weapon,
                entry.category,
            );
        }
    }
}
