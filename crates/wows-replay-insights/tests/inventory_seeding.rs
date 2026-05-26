//! End-to-end checks that `gather_replay_facts` plus
//! `build_inventory_from_facts` produces non-empty consumable inventories for
//! the recording player.
//!
//! Skipped when local game data for the replay's build isn't available.

use std::path::PathBuf;

use wows_replay_insights::build::build_inventory_from_facts;
use wows_replays::ReplayFile;
use wows_replays::analyzer::battle_controller::merged::gather_replay_facts;
use wows_replays::game_constants::GameConstants;
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

fn try_load(filename: &str) -> Option<(ReplayFile, &'static GameMetadataProvider, &'static GameConstants, Version)> {
    let path = fixtures_dir().join(filename);
    let replay = ReplayFile::from_file(&path).ok()?;
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let game_dir = wows_data_mgr::game_dir_for_build(version.build)?;
    let resources = game_data::load_game_resources(&game_dir, &version).ok()?;

    let game_params = GameMetadataProvider::from_vfs(&resources.vfs).ok()?;
    let game_constants = GameConstants::from_vfs(&resources.vfs);

    // Leak to obtain 'static refs for the test runtime.
    let game_params: &'static GameMetadataProvider = Box::leak(Box::new(game_params));
    let game_constants: &'static GameConstants = Box::leak(Box::new(game_constants));

    Some((replay, game_params, game_constants, version))
}

#[test]
fn recording_player_inventory_is_non_empty() {
    let Some((replay, game_params, game_constants, version)) =
        try_load("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay")
    else {
        eprintln!("Skipping: game data for build not available");
        return;
    };

    let resources =
        game_data::load_game_resources(&wows_data_mgr::game_dir_for_build(version.build).unwrap(), &version).unwrap();

    let facts = gather_replay_facts(game_constants, version, &resources.specs, &[&replay]);

    // Find the recording player's entity by looking at facts whose ship_id
    // matches the recording player's ship in the replay metadata. EntityIds
    // come from the live packet stream so we can't derive them from metadata
    // directly.
    let recording_ship_id = replay
        .meta
        .vehicles
        .iter()
        .find(|v| v.name == replay.meta.playerName)
        .expect("recording player should be in metadata")
        .shipId;
    let recording_facts = facts
        .values()
        .find(|f| f.vehicle_id == recording_ship_id)
        .expect("recording player's facts should be in cache");

    assert!(
        recording_facts.max_health > 0.0,
        "recording player's max_health should be populated, got {}",
        recording_facts.max_health
    );
    assert!(
        recording_facts.vehicle_id.raw() != 0,
        "recording player's vehicle_id should be non-zero (ship_config not captured?)"
    );
    assert!(
        !recording_facts.ship_config.abilities().is_empty(),
        "recording player's ship_config.abilities() should not be empty"
    );

    let inv = build_inventory_from_facts(recording_facts, game_params, version);
    assert!(!inv.is_empty(), "build_inventory_from_facts should produce non-empty slots for the recording player");
    for slot in &inv {
        assert!(!slot.icon_key.is_empty(), "icon_key should be populated");
        assert!(!slot.consumable_type_raw.is_empty(), "consumable_type_raw should be populated");
    }
}
