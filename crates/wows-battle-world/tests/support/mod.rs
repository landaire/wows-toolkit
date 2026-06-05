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

/// Drive both controllers over a replay, then build both end-of-battle reports
/// (consuming each controller). Returns the original `wows_replays::BattleReport`
/// and the new `wows_battle_world::report::BattleReport` from the same run.
pub fn both_reports(
    filename: &str,
) -> (
    wows_replays::analyzer::battle_controller::BattleReport,
    wows_battle_world::report::BattleReport,
) {
    let (old, new_world) = both(filename);
    (old.build_report(), new_world.into_report())
}

/// Drive both controllers packet-by-packet, accumulating shot_hits after each
/// packet. Because Tracked clears the hit log every packet, end-of-replay state
/// only holds the final frame; accumulation validates every resolved hit across
/// the full replay.
pub fn both_stepped(
    filename: &str,
) -> (Vec<wows_replays::analyzer::battle_controller::state::ResolvedShotHit>, Vec<wows_replays::analyzer::battle_controller::state::ResolvedShotHit>) {
    use wows_replays::analyzer::battle_controller::listener::BattleControllerState;

    let path = fixtures_dir().join(filename);
    let replay = ReplayFile::from_file(&path).unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let res = resources_for_build(&version);
    let specs: &'static [EntitySpec] = res.provider.entity_specs();

    let meta: &'static wows_replays::ReplayMeta = Box::leak(Box::new(replay.meta));

    let mut old = BattleController::new(meta, res.provider, Some(res.constants));
    let mut new_world =
        wows_battle_world::BattleWorld::new(meta, res.provider, Some(res.constants));

    let mut old_acc: Vec<wows_replays::analyzer::battle_controller::state::ResolvedShotHit> = Vec::new();
    let mut new_acc: Vec<wows_replays::analyzer::battle_controller::state::ResolvedShotHit> = Vec::new();

    let mut parser = wows_replays::packet2::Parser::with_version(specs, version);
    let mut remaining = &replay.packet_data[..];
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).expect("packet parse");
        old.process(&packet);
        new_world.process(&packet);
        old_acc.extend_from_slice(old.shot_hits());
        new_acc.extend(new_world.shot_hits());
    }
    old.finish();
    new_world.finish();

    (old_acc, new_acc)
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

/// Assert full-state parity between old BattleController and new BattleWorld
/// after both have been driven over the same replay. Covers every differential-
/// comparable accessor. Panics with `label` in the message on any mismatch.
///
/// consumable_inventories: compared only when new world has populated slots
/// (seeded path). In the unseeded path both sides have empty inventories, so
/// only active_consumables is validated.
#[allow(clippy::too_many_lines)]
pub fn assert_full_parity(
    old: &BattleController<'static, 'static, GameMetadataProvider>,
    new_world: &mut wows_battle_world::BattleWorld<'static, 'static, GameMetadataProvider>,
    label: &str,
) {
    use std::collections::BTreeSet;
    use wows_replays::analyzer::battle_controller::EntityKind as OldEntityKind;
    use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
    use wows_battle_world::components::EntityKind as NewEntityKind;
    use wowsunpack::game_types::{PlaneId, WeaponType};
    use wowsunpack::recognized::Recognized;

    fn map_kind(k: OldEntityKind) -> NewEntityKind {
        match k {
            OldEntityKind::Vehicle => NewEntityKind::Vehicle,
            OldEntityKind::Building => NewEntityKind::Building,
            OldEntityKind::SmokeScreen => NewEntityKind::SmokeScreen,
        }
    }

    // entity kinds
    {
        let old_entities = old.entities_by_id();
        let new_kinds: HashMap<_, _> = new_world.entity_kinds().into_iter().collect();
        let old_ids: BTreeSet<_> = old_entities.keys().copied().collect();
        let new_ids: BTreeSet<_> = new_kinds.keys().copied().collect();
        assert_eq!(
            old_ids, new_ids,
            "[{label}] entity id sets differ: old_only={:?} new_only={:?}",
            old_ids.difference(&new_ids).collect::<Vec<_>>(),
            new_ids.difference(&old_ids).collect::<Vec<_>>(),
        );
        for (id, entity) in old_entities {
            let ok = map_kind(entity.kind());
            let nk = *new_kinds.get(id).unwrap();
            assert_eq!(ok, nk, "[{label}] entity kind mismatch id={id:?}");
        }
    }

    // positions
    {
        let old_pos = old.ship_positions();
        let new_pos: HashMap<_, _> = new_world.positions().into_iter().collect();
        let old_ids: BTreeSet<_> = old_pos.keys().copied().collect();
        let new_ids: BTreeSet<_> = new_pos.keys().copied().collect();
        assert_eq!(
            old_ids, new_ids,
            "[{label}] position id sets differ: old_only={:?} new_only={:?}",
            old_ids.difference(&new_ids).collect::<Vec<_>>(),
            new_ids.difference(&old_ids).collect::<Vec<_>>(),
        );
        for (id, sp) in old_pos {
            let t = new_pos.get(id).unwrap();
            assert_eq!(sp.position.x, t.pos.x, "[{label}] position.x id={id:?}");
            assert_eq!(sp.position.y, t.pos.y, "[{label}] position.y id={id:?}");
            assert_eq!(sp.position.z, t.pos.z, "[{label}] position.z id={id:?}");
            assert_eq!(sp.yaw, t.yaw.0, "[{label}] position.yaw id={id:?}");
            assert_eq!(sp.pitch, t.pitch.0, "[{label}] position.pitch id={id:?}");
            assert_eq!(sp.roll, t.roll.0, "[{label}] position.roll id={id:?}");
        }
    }

    // minimap
    {
        let old_mm = old.minimap_positions();
        let new_mm: HashMap<_, _> = new_world.minimap().into_iter().collect();
        let old_ids: BTreeSet<_> = old_mm.keys().copied().collect();
        let new_ids: BTreeSet<_> = new_mm.keys().copied().collect();
        assert_eq!(
            old_ids, new_ids,
            "[{label}] minimap id sets differ: old_only={:?} new_only={:?}",
            old_ids.difference(&new_ids).collect::<Vec<_>>(),
            new_ids.difference(&old_ids).collect::<Vec<_>>(),
        );
        for (id, om) in old_mm {
            let nm = new_mm.get(id).unwrap();
            assert_eq!(om.position.x, nm.pos.x, "[{label}] minimap.x id={id:?}");
            assert_eq!(om.position.y, nm.pos.y, "[{label}] minimap.y id={id:?}");
            assert_eq!(om.heading, nm.heading.0, "[{label}] minimap.heading id={id:?}");
            assert_eq!(om.visible, nm.visible, "[{label}] minimap.visible id={id:?}");
            assert_eq!(om.visibility_flags, nm.visibility_flags.0, "[{label}] minimap.visibility_flags id={id:?}");
            assert_eq!(om.is_invisible, nm.is_invisible, "[{label}] minimap.is_invisible id={id:?}");
        }
    }

    // vehicle props + aim
    {
        let old_entities = old.entities_by_id();
        let old_turret_yaws = old.turret_yaws();
        let old_target_yaws = old.target_yaws();
        let old_selected_ammo = old.selected_ammo();
        let new_vps = new_world.vehicle_props_all();
        let new_aims = new_world.aims_all();
        let empty_yaws: Vec<f32> = Vec::new();

        for (id, entity) in old_entities {
            let Some(vehicle_ref) = entity.vehicle_ref() else { continue };
            let old_props = std::cell::RefCell::borrow(vehicle_ref);
            let ovp = old_props.props();
            let nvp = new_vps
                .get(id)
                .unwrap_or_else(|| panic!("[{label}] vehicle_props missing id={id:?}"));
            assert_eq!(ovp.health(), nvp.health(), "[{label}] health id={id:?}");
            assert_eq!(ovp.max_health(), nvp.max_health(), "[{label}] max_health id={id:?}");
            assert_eq!(ovp.is_alive(), nvp.is_alive(), "[{label}] is_alive id={id:?}");
            assert_eq!(ovp.is_invisible(), nvp.is_invisible(), "[{label}] is_invisible id={id:?}");
            assert_eq!(ovp.visibility_flags(), nvp.visibility_flags(), "[{label}] visibility_flags id={id:?}");
            assert_eq!(ovp.team_id(), nvp.team_id(), "[{label}] team_id id={id:?}");
            assert_eq!(ovp.owner(), nvp.owner(), "[{label}] owner id={id:?}");
            assert_eq!(ovp.selected_weapon(), nvp.selected_weapon(), "[{label}] selected_weapon id={id:?}");
            assert_eq!(
                ovp.ship_config().ship_params_id(), nvp.ship_config().ship_params_id(),
                "[{label}] ship_params_id id={id:?}"
            );
            assert_eq!(
                ovp.crew_modifiers_compact_params().params_id(),
                nvp.crew_modifiers_compact_params().params_id(),
                "[{label}] crew params_id id={id:?}"
            );

            let old_turrets = old_turret_yaws.get(id).unwrap_or(&empty_yaws);
            let old_target = old_target_yaws.get(id).copied();
            let old_ammo = old_selected_ammo.get(id).copied();
            let aim = new_aims.get(id);
            let new_turrets: Vec<f32> =
                aim.map(|a| a.turret_yaws.iter().map(|r| r.0).collect()).unwrap_or_default();
            let new_target: Option<f32> = aim.and_then(|a| a.target_yaw.map(|r| r.0));
            let new_ammo = aim.and_then(|a| {
                a.selected_ammo.get(&Recognized::Known(WeaponType::Artillery)).copied()
            });
            assert_eq!(old_turrets.as_slice(), new_turrets.as_slice(), "[{label}] turret_yaws id={id:?}");
            assert_eq!(old_target, new_target, "[{label}] target_yaw id={id:?}");
            assert_eq!(old_ammo, new_ammo, "[{label}] selected_ammo id={id:?}");
        }
    }

    // kills
    {
        let ok = old.kills();
        let nk = new_world.kills();
        assert_eq!(ok.len(), nk.len(), "[{label}] kills len: old={} new={}", ok.len(), nk.len());
        for (i, (o, n)) in ok.iter().zip(nk.iter()).enumerate() {
            assert_eq!(o.clock, n.clock, "[{label}] kills[{i}].clock");
            assert_eq!(o.killer, n.killer, "[{label}] kills[{i}].killer");
            assert_eq!(o.victim, n.victim, "[{label}] kills[{i}].victim");
            assert_eq!(o.cause, n.cause, "[{label}] kills[{i}].cause");
        }
    }

    // dead_ships
    {
        let od = old.dead_ships();
        let nd = new_world.dead_ships();
        assert_eq!(od.len(), nd.len(), "[{label}] dead_ships len: old={} new={}", od.len(), nd.len());
        for (id, o) in od {
            let n = nd.get(id).unwrap_or_else(|| panic!("[{label}] dead_ships missing id={id:?}"));
            assert_eq!(o.clock, n.clock, "[{label}] dead_ships[{id:?}].clock");
            match (o.position, n.position) {
                (None, None) => {}
                (Some(op), Some(np)) => {
                    assert_eq!(op.x, np.x, "[{label}] dead_ships[{id:?}].position.x");
                    assert_eq!(op.y, np.y, "[{label}] dead_ships[{id:?}].position.y");
                    assert_eq!(op.z, np.z, "[{label}] dead_ships[{id:?}].position.z");
                }
                _ => panic!(
                    "[{label}] dead_ships[{id:?}] position presence mismatch: old={} new={}",
                    o.position.is_some(), n.position.is_some()
                ),
            }
            match (o.minimap_position, n.minimap_position) {
                (None, None) => {}
                (Some(om), Some(nm)) => {
                    assert_eq!(om.x, nm.x, "[{label}] dead_ships[{id:?}].minimap_position.x");
                    assert_eq!(om.y, nm.y, "[{label}] dead_ships[{id:?}].minimap_position.y");
                }
                _ => panic!(
                    "[{label}] dead_ships[{id:?}] minimap_position presence mismatch: old={} new={}",
                    o.minimap_position.is_some(), n.minimap_position.is_some()
                ),
            }
        }
    }

    // damage_dealt
    {
        let od = old.damage_dealt();
        let nd = new_world.damage_ledger();
        let mut ok: Vec<_> = od.keys().copied().collect();
        let mut nk: Vec<_> = nd.keys().copied().collect();
        ok.sort();
        nk.sort();
        assert_eq!(
            ok, nk,
            "[{label}] damage_dealt key sets differ: old_only={:?} new_only={:?}",
            ok.iter().filter(|k| !nk.contains(k)).collect::<Vec<_>>(),
            nk.iter().filter(|k| !ok.contains(k)).collect::<Vec<_>>(),
        );
        for (aggressor, oe) in od {
            let ne = nd.get(aggressor).unwrap();
            assert_eq!(oe.len(), ne.len(), "[{label}] damage_dealt[{aggressor:?}] len");
            for (j, (o, n)) in oe.iter().zip(ne.iter()).enumerate() {
                assert_eq!(o.amount, n.amount, "[{label}] damage_dealt[{aggressor:?}][{j}].amount");
                assert_eq!(o.victim, n.victim, "[{label}] damage_dealt[{aggressor:?}][{j}].victim");
                assert_eq!(o.clock, n.clock, "[{label}] damage_dealt[{aggressor:?}][{j}].clock");
            }
        }
    }

    // self_ribbons
    assert_eq!(old.self_ribbons(), new_world.self_ribbons(), "[{label}] self_ribbons");

    // self_damage_stats
    {
        let os = old.self_damage_stats();
        let ns = new_world.self_damage_stats();
        assert_eq!(os.len(), ns.len(), "[{label}] self_damage_stats len");
        for (key, oe) in os {
            let ne = ns
                .get(key)
                .unwrap_or_else(|| panic!("[{label}] self_damage_stats missing key={key:?}"));
            assert_eq!(oe.count, ne.count, "[{label}] self_damage_stats[{key:?}].count");
            assert_eq!(oe.total, ne.total, "[{label}] self_damage_stats[{key:?}].total");
            assert_eq!(oe.weapon, ne.weapon, "[{label}] self_damage_stats[{key:?}].weapon");
            assert_eq!(oe.category, ne.category, "[{label}] self_damage_stats[{key:?}].category");
        }
    }

    // player_entities
    {
        let op = old.player_entities();
        let np = new_world.player_entities();
        let mut ok: Vec<_> = op.keys().copied().collect();
        let mut nk: Vec<_> = np.keys().copied().collect();
        ok.sort();
        nk.sort();
        assert_eq!(ok, nk, "[{label}] player_entities key sets differ");
        for (id, o) in op {
            let n = np.get(id).unwrap();
            assert_eq!(
                o.initial_state().username(), n.initial_state().username(),
                "[{label}] player[{id:?}].name"
            );
            assert_eq!(
                o.initial_state().meta_ship_id(), n.initial_state().meta_ship_id(),
                "[{label}] player[{id:?}].meta_ship_id"
            );
            assert_eq!(o.relation(), n.relation(), "[{label}] player[{id:?}].relation");
            assert_eq!(o.vehicle().id(), n.vehicle().id(), "[{label}] player[{id:?}].vehicle_id");
        }
    }

    // game_chat
    {
        let oc = old.game_chat();
        let nc = new_world.chat();
        assert_eq!(oc.len(), nc.len(), "[{label}] chat len: old={} new={}", oc.len(), nc.len());
        for (i, (o, n)) in oc.iter().zip(nc.iter()).enumerate() {
            assert_eq!(o.clock, n.clock, "[{label}] chat[{i}].clock");
            assert_eq!(o.sender_name, n.sender_name, "[{label}] chat[{i}].sender_name");
            assert_eq!(o.channel, n.channel, "[{label}] chat[{i}].channel");
            assert_eq!(o.message, n.message, "[{label}] chat[{i}].message");
            assert_eq!(o.entity_id, n.entity_id, "[{label}] chat[{i}].entity_id");
            assert_eq!(o.sender_relation, n.sender_relation, "[{label}] chat[{i}].sender_relation");
            match (&o.player, &n.player) {
                (None, None) => {}
                (Some(op), Some(np)) => {
                    assert_eq!(
                        op.initial_state().username(), np.initial_state().username(),
                        "[{label}] chat[{i}].player.name"
                    );
                    assert_eq!(
                        op.initial_state().meta_ship_id(), np.initial_state().meta_ship_id(),
                        "[{label}] chat[{i}].player.meta_ship_id"
                    );
                }
                _ => panic!(
                    "[{label}] chat[{i}] player presence mismatch: old={} new={}",
                    o.player.is_some(), n.player.is_some()
                ),
            }
        }
    }

    // active_consumables (always; unseeded both sides empty)
    {
        let oa = old.active_consumables();
        let na = new_world.active_consumables();
        let mut oids: Vec<_> = oa.keys().copied().collect();
        let mut nids: Vec<_> = na.keys().copied().collect();
        oids.sort();
        nids.sort();
        assert_eq!(
            oids, nids,
            "[{label}] active_consumables key sets differ: old_only={:?} new_only={:?}",
            oids.iter().filter(|k| !nids.contains(k)).collect::<Vec<_>>(),
            nids.iter().filter(|k| !oids.contains(k)).collect::<Vec<_>>(),
        );
        for (eid, oa_list) in oa {
            let na_list = na.get(eid).unwrap();
            assert_eq!(oa_list.len(), na_list.len(), "[{label}] active_consumables[{eid:?}] len");
            for (i, (o, n)) in oa_list.iter().zip(na_list.iter()).enumerate() {
                assert_eq!(o, n, "[{label}] active_consumables[{eid:?}][{i}] mismatch");
            }
        }
    }

    // consumable_inventories: skipped in unseeded path (both sides empty)
    {
        let ni = new_world.consumable_inventories();
        if !ni.is_empty() {
            let oi = old.consumable_inventories();
            let mut oids: Vec<_> = oi.keys().copied().collect();
            let mut nids: Vec<_> = ni.keys().copied().collect();
            oids.sort();
            nids.sort();
            assert_eq!(
                oids, nids,
                "[{label}] consumable_inventories key sets differ: old_only={:?} new_only={:?}",
                oids.iter().filter(|k| !nids.contains(k)).collect::<Vec<_>>(),
                nids.iter().filter(|k| !oids.contains(k)).collect::<Vec<_>>(),
            );
            for (eid, oslots) in oi {
                let nslots = ni.get(eid).unwrap();
                assert_eq!(oslots.len(), nslots.len(), "[{label}] consumable_inventories[{eid:?}] len");
                for (i, (o, n)) in oslots.iter().zip(nslots.iter()).enumerate() {
                    assert_eq!(o, n, "[{label}] consumable_inventories[{eid:?}][{i}] mismatch");
                }
            }
        }
    }

    // active_planes
    {
        let op: HashMap<PlaneId, _> = old.active_planes().iter().map(|(k, v)| (*k, v.clone())).collect();
        let np: HashMap<PlaneId, _> = new_world.active_planes();
        assert_eq!(op.len(), np.len(), "[{label}] active_planes len: old={} new={}", op.len(), np.len());
        let mut ids: Vec<PlaneId> = op.keys().copied().collect();
        ids.sort();
        for id in &ids {
            let o = &op[id];
            let n = np.get(id).unwrap_or_else(|| panic!("[{label}] active_planes missing id={id:?}"));
            assert_eq!(o, n, "[{label}] active_planes[{id:?}] mismatch");
        }
    }

    // active_wards
    // radius is BigWorldDistance(f32) and may be NaN for old replays where the
    // ward radius packet field was absent; compare each field individually so
    // NaN == NaN holds when both sides received the same absent value.
    {
        let ow: HashMap<PlaneId, _> = old.active_wards().iter().map(|(k, v)| (*k, v.clone())).collect();
        let nw: HashMap<PlaneId, _> = new_world.active_wards();
        assert_eq!(ow.len(), nw.len(), "[{label}] active_wards len: old={} new={}", ow.len(), nw.len());
        let mut ids: Vec<PlaneId> = ow.keys().copied().collect();
        ids.sort();
        for id in &ids {
            let o = &ow[id];
            let n = nw.get(id).unwrap_or_else(|| panic!("[{label}] active_wards missing id={id:?}"));
            assert_eq!(o.plane_id, n.plane_id, "[{label}] active_wards[{id:?}].plane_id");
            assert_eq!(o.position, n.position, "[{label}] active_wards[{id:?}].position");
            assert_eq!(o.owner_id, n.owner_id, "[{label}] active_wards[{id:?}].owner_id");
            // NaN-aware comparison for radius: treat both-NaN as equal.
            let or_ = o.radius.value();
            let nr = n.radius.value();
            assert!(
                or_.total_cmp(&nr).is_eq(),
                "[{label}] active_wards[{id:?}].radius mismatch: old={or_} new={nr}",
            );
        }
    }

    // active_shots
    {
        let os = old.active_shots();
        let ns = new_world.active_shots();
        assert_eq!(os.len(), ns.len(), "[{label}] active_shots len: old={} new={}", os.len(), ns.len());
        assert_eq!(os, ns.as_slice(), "[{label}] active_shots mismatch");
    }

    // active_torpedoes
    {
        let ot = old.active_torpedoes();
        let nt = new_world.active_torpedoes();
        assert_eq!(ot.len(), nt.len(), "[{label}] active_torpedoes len: old={} new={}", ot.len(), nt.len());
        assert_eq!(ot, nt.as_slice(), "[{label}] active_torpedoes mismatch");
    }

    // capture_points
    {
        let oc = old.capture_points();
        let nc = new_world.capture_points();
        assert_eq!(oc.len(), nc.len(), "[{label}] capture_points len");
        for (i, (o, n)) in oc.iter().zip(nc.iter()).enumerate() {
            assert_eq!(o.index, n.index, "[{label}] capture_points[{i}].index");
            assert_eq!(o.position, n.position, "[{label}] capture_points[{i}].position");
            assert_eq!(o.radius, n.radius, "[{label}] capture_points[{i}].radius");
            assert_eq!(o.control_point_type, n.control_point_type, "[{label}] capture_points[{i}].control_point_type");
            assert_eq!(o.team_id, n.team_id, "[{label}] capture_points[{i}].team_id");
            assert_eq!(o.invader_team, n.invader_team, "[{label}] capture_points[{i}].invader_team");
            assert_eq!(o.progress, n.progress, "[{label}] capture_points[{i}].progress");
            assert_eq!(o.has_invaders, n.has_invaders, "[{label}] capture_points[{i}].has_invaders");
            assert_eq!(o.both_inside, n.both_inside, "[{label}] capture_points[{i}].both_inside");
            assert_eq!(o.is_enabled, n.is_enabled, "[{label}] capture_points[{i}].is_enabled");
        }
    }

    // team_scores
    {
        let os = old.team_scores();
        let ns = new_world.team_scores();
        assert_eq!(os.len(), ns.len(), "[{label}] team_scores len");
        for (i, (o, n)) in os.iter().zip(ns.iter()).enumerate() {
            assert_eq!(o.team_index, n.team_index, "[{label}] team_scores[{i}].team_index");
            assert_eq!(o.score, n.score, "[{label}] team_scores[{i}].score");
        }
    }

    // buff_zones
    {
        let ob = old.buff_zones();
        let nb = new_world.buff_zones();
        assert_eq!(ob.len(), nb.len(), "[{label}] buff_zones len");
        let mut ids: Vec<_> = ob.keys().copied().collect();
        ids.sort();
        for id in &ids {
            let o = &ob[id];
            let n = nb.get(id).unwrap_or_else(|| panic!("[{label}] buff_zones missing id={id:?}"));
            assert_eq!(o.entity_id, n.entity_id, "[{label}] buff_zones[{id:?}].entity_id");
            assert_eq!(o.position, n.position, "[{label}] buff_zones[{id:?}].position");
            assert_eq!(o.radius, n.radius, "[{label}] buff_zones[{id:?}].radius");
            assert_eq!(o.team_id, n.team_id, "[{label}] buff_zones[{id:?}].team_id");
            assert_eq!(o.is_active, n.is_active, "[{label}] buff_zones[{id:?}].is_active");
            assert_eq!(o.drop_params_id, n.drop_params_id, "[{label}] buff_zones[{id:?}].drop_params_id");
        }
    }

    // captured_buffs
    {
        let oc = old.captured_buffs();
        let nc = new_world.captured_buffs();
        assert_eq!(oc.len(), nc.len(), "[{label}] captured_buffs len");
        for (i, (o, n)) in oc.iter().zip(nc.iter()).enumerate() {
            assert_eq!(o.params_id, n.params_id, "[{label}] captured_buffs[{i}].params_id");
            assert_eq!(o.team_id, n.team_id, "[{label}] captured_buffs[{i}].team_id");
            assert_eq!(o.clock, n.clock, "[{label}] captured_buffs[{i}].clock");
        }
    }

    // local_weather_zones
    {
        let ow = old.local_weather_zones();
        let nw = new_world.local_weather_zones();
        assert_eq!(ow.len(), nw.len(), "[{label}] local_weather_zones len");
        for (i, (o, n)) in ow.iter().zip(nw.iter()).enumerate() {
            assert_eq!(o.name, n.name, "[{label}] local_weather_zones[{i}].name");
            assert_eq!(o.params_id, n.params_id, "[{label}] local_weather_zones[{i}].params_id");
            assert_eq!(o.radius, n.radius, "[{label}] local_weather_zones[{i}].radius");
            assert_eq!(o.position.x, n.position.x, "[{label}] local_weather_zones[{i}].position.x");
            assert_eq!(o.position.z, n.position.z, "[{label}] local_weather_zones[{i}].position.z");
        }
    }

    // scoring_rules
    {
        let or_ = old.scoring_rules().cloned();
        let nr = new_world.scoring_rules();
        match (&or_, &nr) {
            (None, None) => {}
            (Some(o), Some(n)) => {
                assert_eq!(o.team_win_score, n.team_win_score, "[{label}] scoring_rules.team_win_score");
                assert_eq!(o.hold_reward, n.hold_reward, "[{label}] scoring_rules.hold_reward");
                assert_eq!(o.hold_period, n.hold_period, "[{label}] scoring_rules.hold_period");
                assert_eq!(o.hold_cp_indices, n.hold_cp_indices, "[{label}] scoring_rules.hold_cp_indices");
            }
            _ => panic!(
                "[{label}] scoring_rules presence mismatch: old={} new={}",
                or_.is_some(), nr.is_some()
            ),
        }
    }

    // match state scalars
    assert_eq!(old.battle_stage(), new_world.battle_stage(), "[{label}] battle_stage");
    assert_eq!(old.time_left(), new_world.time_left(), "[{label}] time_left");
    assert_eq!(old.battle_start_clock(), new_world.battle_start_clock(), "[{label}] battle_start_clock");
    assert_eq!(old.battle_end_clock(), new_world.battle_end_clock(), "[{label}] battle_end_clock");
    assert_eq!(old.winning_team(), new_world.winning_team(), "[{label}] winning_team");
    assert_eq!(
        old.finish_type().map(|r| format!("{r:?}")),
        new_world.finish_type().map(|r| format!("{r:?}")),
        "[{label}] finish_type",
    );
}
