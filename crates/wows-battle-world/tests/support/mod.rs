//! Shared fixture-loading helpers for integration tests.
//!
//! Loads game resources from a dumped build archive (the layout produced by
//! wows-data-mgr: a build dir with `vfs/`, `game_params.rkyv`, `constants.json`),
//! resolved via `wows_data_mgr::game_dir_for_build`. This mirrors replayshark's
//! extracted-dir loader, not the raw-install `load_game_resources` path.
//!
//! Parsed game resources are cached per build so a corpus of fixtures sharing a
//! build parses GameParams only once per test binary.
#![cfg(feature = "vfs")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::rpc::entitydefs::EntitySpec;
use wowsunpack::vfs::VfsPath;
use wowsunpack::vfs::impls::physical::PhysicalFS;

pub type SeededPair = (
    BattleController<'static, 'static, GameMetadataProvider>,
    wows_battle_world::BattleWorld<'static, 'static, GameMetadataProvider>,
);

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

#[derive(Clone, Copy)]
struct BuildResources {
    provider: &'static GameMetadataProvider,
    constants: &'static GameConstants,
}

pub struct Handle {
    pub replay: ReplayFile,
    pub game_params: &'static GameMetadataProvider,
    pub game_constants: &'static GameConstants,
    pub specs: &'static [EntitySpec],
    pub version: Version,
}

fn build_cache() -> &'static Mutex<HashMap<u32, BuildResources>> {
    static CACHE: OnceLock<Mutex<HashMap<u32, BuildResources>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn resources_for_build(version: &Version) -> BuildResources {
    if let Some(res) = build_cache().lock().unwrap().get(&version.build) {
        return *res;
    }

    let dir = wows_data_mgr::game_dir_for_build(version.build)
        .unwrap_or_else(|| panic!("game data for build {} not available", version.build));
    let vfs_root = dir.join("vfs");
    assert!(vfs_root.exists(), "vfs dir not found at {}", vfs_root.display());
    let vfs = VfsPath::new(PhysicalFS::new(&vfs_root));

    let rkyv_path = dir.join("game_params.rkyv");
    let provider = match wowsunpack::game_params::cache::load(&rkyv_path) {
        Some(params) => GameMetadataProvider::from_params_with_vfs(params, &vfs)
            .unwrap_or_else(|e| panic!("failed to build game metadata for build {}: {e:?}", version.build)),
        None => GameMetadataProvider::from_vfs(&vfs)
            .unwrap_or_else(|e| panic!("failed to load GameParams for build {}: {e:?}", version.build)),
    };
    let constants = GameConstants::from_vfs(&vfs);

    let res = BuildResources {
        provider: Box::leak(Box::new(provider)),
        constants: Box::leak(Box::new(constants)),
    };
    build_cache().lock().unwrap().insert(version.build, res);
    res
}

pub fn load(filename: &str) -> Handle {
    let path = fixtures_dir().join(filename);
    let replay = ReplayFile::from_file(&path).unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let res = resources_for_build(&version);
    let specs: &'static [EntitySpec] = res.provider.entity_specs();

    Handle { replay, game_params: res.provider, game_constants: res.constants, specs, version }
}

/// Drive both BattleController and BattleWorld over the same packet stream.
///
/// The replay meta is leaked to 'static so both analyzers can borrow it with the
/// same lifetime. Game resources are resolved from the per-build cache (same as
/// `load`), so a test binary that calls `both` and `load` for the same fixture
/// only parses GameParams once.
pub fn both(
    filename: &str,
) -> (
    BattleController<'static, 'static, GameMetadataProvider>,
    wows_battle_world::BattleWorld<'static, 'static, GameMetadataProvider>,
) {
    let path = fixtures_dir().join(filename);
    let replay = ReplayFile::from_file(&path).unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let res = resources_for_build(&version);
    let specs: &'static [EntitySpec] = res.provider.entity_specs();

    let meta: &'static wows_replays::ReplayMeta = Box::leak(Box::new(replay.meta));

    let mut old = BattleController::new(meta, res.provider, Some(res.constants));
    let mut new_world =
        wows_battle_world::BattleWorld::new(meta, res.provider, Some(res.constants));

    let mut parser = wows_replays::packet2::Parser::with_version(specs, version);
    let mut remaining = &replay.packet_data[..];
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).expect("packet parse");
        old.process(&packet);
        new_world.process(&packet);
    }
    old.finish();
    new_world.finish();

    (old, new_world)
}

/// Like `both`, but seeds consumable inventories on both controllers before the
/// packet loop. Uses `gather_replay_facts` + `build_inventory_from_facts` to
/// derive per-entity slot definitions without requiring a two-pass parse.
///
/// This mirrors the production path in the toolkit renderer, which scans for
/// VehicleFacts first then seeds before replaying packets.
pub fn both_seeded(filename: &str) -> SeededPair {
    use wows_replay_insights::build::{build_inventory_from_facts, seed_consumable_inventories_from_facts};
    use wows_replays::analyzer::battle_controller::merged::gather_replay_facts;

    let path = fixtures_dir().join(filename);
    let replay = ReplayFile::from_file(&path).unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let res = resources_for_build(&version);
    let specs: &'static [EntitySpec] = res.provider.entity_specs();

    let facts = gather_replay_facts(res.constants, version, specs, &[&replay]);

    let meta: &'static wows_replays::ReplayMeta = Box::leak(Box::new(replay.meta));

    let mut old = BattleController::new(meta, res.provider, Some(res.constants));
    let mut new_world =
        wows_battle_world::BattleWorld::new(meta, res.provider, Some(res.constants));

    seed_consumable_inventories_from_facts(&mut old, &facts, res.provider, version);
    for (entity_id, fact) in &facts {
        let inv = build_inventory_from_facts(fact, res.provider, version);
        if !inv.is_empty() {
            new_world.set_consumable_inventory(*entity_id, inv);
        }
    }

    let mut parser = wows_replays::packet2::Parser::with_version(specs, version);
    let mut remaining = &replay.packet_data[..];
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).expect("packet parse");
        old.process(&packet);
        new_world.process(&packet);
    }
    old.finish();
    new_world.finish();

    (old, new_world)
}
