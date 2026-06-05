//! Shared fixture-loading helpers for integration tests.
#![cfg(feature = "vfs")]

use std::path::PathBuf;

use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::Version;
use wowsunpack::game_data;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::rpc::entitydefs::EntitySpec;

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

pub struct Handle {
    pub replay: ReplayFile,
    pub game_params: &'static GameMetadataProvider,
    pub game_constants: &'static GameConstants,
    pub specs: Vec<EntitySpec>,
    pub version: Version,
}

pub fn load(filename: &str) -> Handle {
    let path = fixtures_dir().join(filename);
    let replay =
        ReplayFile::from_file(&path).unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let game_dir = wows_data_mgr::game_dir_for_build(version.build)
        .unwrap_or_else(|| panic!("game data for build {} not available", version.build));
    let resources =
        game_data::load_game_resources(&game_dir, &version).expect("should load game resources");

    let game_params = GameMetadataProvider::from_vfs(&resources.vfs)
        .map_err(|e| panic!("failed to load GameParams: {e:?}"))
        .unwrap();
    let game_constants = GameConstants::from_vfs(&resources.vfs);

    let game_params: &'static GameMetadataProvider = Box::leak(Box::new(game_params));
    let game_constants: &'static GameConstants = Box::leak(Box::new(game_constants));

    let specs = resources.specs;

    Handle { replay, game_params, game_constants, specs, version }
}
