#![cfg(feature = "battle-report")]
//! Version-matrix regression for `resolve_battle_results` +
//! `NormalizedBattleReport::from_battle_report`.
//!
//! Each entry pairs a real `replay_cache` replay with its game build. Game data
//! is loaded from a dumped build archive (a dir with `vfs/` + `game_params.rkyv`,
//! resolved via `wows_data_mgr::game_dir_for_build`), the same loader
//! `wows-battle-world`'s test support uses. Entries whose build has no local
//! game data skip with a message, so the suite stays green on machines without
//! the archives.
//!
//! For each runnable entry the test builds the report, checks version-invariant
//! structural properties, then compares the serialized report to a committed
//! golden as `serde_json::Value` (order-insensitive, so the `damage_interactions`
//! `HashMap` serialization order does not matter).
//!
//! Goldens are captured snapshots. To (re)generate them after a deliberate
//! builder change, run with the game data present:
//!   REGEN_GOLDENS=1 cargo test -p wows-replay-insights --features battle-report \
//!     --test version_regression
//! which overwrites `tests/fixtures/normalized/<build>.json` for every runnable
//! entry without comparing. Commit the updated goldens.

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
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;
use wowsunpack::vfs::impls::physical::PhysicalFS;

struct Entry {
    version_label: &'static str,
    replay_filename: &'static str,
    build: u32,
}

const MATRIX: &[Entry] = &[
    Entry {
        version_label: "13.11.0",
        replay_filename: "10000_1736020571_20250101_215420_PVSB018-Ipiranga_41_Conquest.wowsreplay",
        build: 9251401,
    },
    Entry {
        version_label: "14.5.0",
        replay_filename: "1005_1751197418_20250629_132500_PHSC010-Utrecht_54_Faroe.wowsreplay",
        build: 10087791,
    },
    Entry {
        version_label: "14.11.0",
        replay_filename: "10000_1767829698_20260107_182737_PJSB018-Yamato-1944_50_Gold_harbor.wowsreplay",
        build: 11189791,
    },
    Entry {
        version_label: "15.1.0",
        replay_filename: "10000_1773241614_20260214_215653_PVSB719-Valparaiso_42_Neighbors.wowsreplay",
        build: 11965230,
    },
    Entry {
        version_label: "15.4.0",
        replay_filename: "1003_1780609677_20260604_172336_PASD008-Benson-1945_52_Britain.wowsreplay",
        build: 12506899,
    },
    Entry {
        version_label: "15.5.0",
        replay_filename: "1148_1782049673_20260619_175139_PRSB909-East-Navarin_44_Path_warrior.wowsreplay",
        build: 12668706,
    },
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

struct Loaded {
    replay: ReplayFile,
    provider: &'static GameMetadataProvider,
    game_constants: &'static GameConstants,
    version: Version,
    constants: serde_json::Value,
}

/// Load the replay, its dumped game data, and its constants fixture. Returns
/// `None` (skip) when the build's game data or constants fixture is unavailable.
fn try_load(entry: &Entry) -> Option<Loaded> {
    let path = fixtures_dir().join("replays").join(entry.replay_filename);
    let replay = ReplayFile::from_file(&path).ok()?;
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let game_dir = wows_data_mgr::game_dir_for_build(entry.build)?;
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

    let constants_path = fixtures_dir().join("constants").join(format!("{}.json", entry.build));
    let constants: serde_json::Value =
        std::fs::read(&constants_path).ok().and_then(|b| serde_json::from_slice(&b).ok())?;

    let provider: &'static GameMetadataProvider = Box::leak(Box::new(provider));
    let game_constants: &'static GameConstants = Box::leak(Box::new(game_constants));

    Some(Loaded { replay, provider, game_constants, version, constants })
}

/// Build a `BattleReport` from the replay, mirroring `run_players_query`.
fn build_report(loaded: &Loaded) -> BattleReport {
    let mut world = BattleWorld::new(&loaded.replay.meta, loaded.provider, Some(loaded.game_constants));
    world.set_shot_tracking(ShotTracking::Untracked);

    let mut parser = wows_replays::packet2::Parser::with_version(loaded.provider.entity_specs(), loaded.version);
    let mut remaining = loaded.replay.packet_data.as_slice();
    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => world.process(&packet),
            Err(_) => break,
        }
    }
    world.finish();
    world.into_report()
}

/// Version-invariant properties that must hold regardless of the exact numbers,
/// so a regenerated golden is still meaningful.
fn assert_structural_invariants(entry: &Entry, report: &BattleReport, normalized: &NormalizedBattleReport) {
    let ctx = entry.version_label;

    let self_player = normalized.players.iter().find(|p| p.is_self);
    assert!(self_player.is_some(), "{ctx}: a self player must exist");
    let self_player = self_player.unwrap();

    if report.battle_results().is_some() {
        let has_server_damage =
            normalized.players.iter().any(|p| p.server_results.as_ref().map(|s| s.damage.is_some()).unwrap_or(false));
        assert!(has_server_damage, "{ctx}: with battle results, some player must have server_results with damage");
    }

    // Self controller fallbacks are always populated for the recording player.
    assert!(self_player.controller_spotting_damage.is_some(), "{ctx}: self controller_spotting_damage must be Some");
    assert!(self_player.controller_potential_damage.is_some(), "{ctx}: self controller_potential_damage must be Some");

    // Any AirCarrier with server results carries the aircraft `hits` scalar.
    for cv in normalized.players.iter().filter(|p| p.ship_class == Species::AirCarrier) {
        if let Some(sr) = cv.server_results.as_ref() {
            assert!(sr.hits.is_some(), "{ctx}: AirCarrier with server_results must carry hits");
        }
    }

    // If any player recorded interactions, at least one carries a per-type map.
    let any_interactions = normalized
        .players
        .iter()
        .any(|p| p.server_results.as_ref().map(|s| !s.damage_interactions.is_empty()).unwrap_or(false));
    if any_interactions {
        let has_typed = normalized.players.iter().any(|p| {
            p.server_results
                .as_ref()
                .map(|s| {
                    s.damage_interactions.values().any(|i| {
                        i.damage_dealt_by_type.ap.is_some()
                            || i.damage_dealt_by_type.he.is_some()
                            || i.damage_dealt_by_type.sap.is_some()
                    })
                })
                .unwrap_or(false)
        });
        assert!(has_typed, "{ctx}: interactions present but no per-type damage breakdown");
    }
}

fn run_entry(entry: &Entry) {
    let Some(loaded) = try_load(entry) else {
        eprintln!("skipping {}: game data not installed", entry.version_label);
        return;
    };

    let report = build_report(&loaded);
    let normalized =
        NormalizedBattleReport::from_battle_report(&report, &loaded.replay.meta, loaded.provider, &loaded.constants);

    assert_structural_invariants(entry, &report, &normalized);

    let mut produced = serde_json::to_value(&normalized).expect("normalized report serializes");
    sort_players(&mut produced);
    let golden_path = fixtures_dir().join("normalized").join(format!("{}.json", entry.build));

    if std::env::var_os("REGEN_GOLDENS").is_some() {
        std::fs::create_dir_all(golden_path.parent().unwrap()).unwrap();
        std::fs::write(&golden_path, serde_json::to_string_pretty(&produced).unwrap()).unwrap();
        eprintln!("captured golden for build {} ({})", entry.build, entry.version_label);
        return;
    }

    let golden_bytes = std::fs::read(&golden_path).unwrap_or_else(|_| {
        panic!(
            "missing golden for build {} ({}); run REGEN_GOLDENS=1 cargo test -p wows-replay-insights \
             --features battle-report --test version_regression",
            entry.build, entry.version_label
        )
    });
    let mut golden: serde_json::Value = serde_json::from_slice(&golden_bytes).expect("golden parses as JSON");
    sort_players(&mut golden);

    if let Err(path) = approx_eq(&produced, &golden) {
        panic!("{} (build {}): normalized report does not match golden at {}", entry.version_label, entry.build, path);
    }
}

/// Sort the `players` array by each element's canonical serialization. The
/// builder's player order comes from a hash map and is not stable run-to-run;
/// each player object's content is stable, so sorting by its full serialization
/// yields a deterministic order for the golden compare.
fn sort_players(value: &mut serde_json::Value) {
    if let Some(players) = value.get_mut("players").and_then(|p| p.as_array_mut()) {
        players.sort_by_cached_key(|p| serde_json::to_string(p).unwrap_or_default());
    }
}

/// Deep structural compare of two JSON values, order-insensitive for objects and
/// tolerant of tiny float drift. Percentage fields are ratios summed over a hash
/// map, so their last f64 bits vary run-to-run; a 1e-9 tolerance absorbs that
/// while staying far below any real regression. Returns `Err(path)` at the first
/// divergence.
fn approx_eq(a: &serde_json::Value, b: &serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x == y {
                return Ok(());
            }
            match (x.as_f64(), y.as_f64()) {
                (Some(fx), Some(fy)) => {
                    let diff = (fx - fy).abs();
                    let tol = 1e-9 * fx.abs().max(fy.abs()).max(1.0);
                    if diff <= tol { Ok(()) } else { Err(format!("<number {fx} vs {fy}>")) }
                }
                _ => Err(format!("<number {x} vs {y}>")),
            }
        }
        (Value::Array(xs), Value::Array(ys)) => {
            if xs.len() != ys.len() {
                return Err(format!("<array len {} vs {}>", xs.len(), ys.len()));
            }
            for (i, (xi, yi)) in xs.iter().zip(ys).enumerate() {
                approx_eq(xi, yi).map_err(|p| format!("[{i}]{p}"))?;
            }
            Ok(())
        }
        (Value::Object(xs), Value::Object(ys)) => {
            if xs.len() != ys.len() {
                return Err(format!(
                    "<object keys {:?} vs {:?}>",
                    xs.keys().collect::<Vec<_>>(),
                    ys.keys().collect::<Vec<_>>()
                ));
            }
            for (k, xv) in xs {
                match ys.get(k) {
                    Some(yv) => approx_eq(xv, yv).map_err(|p| format!(".{k}{p}"))?,
                    None => return Err(format!(".{k}<missing in golden>")),
                }
            }
            Ok(())
        }
        _ if a == b => Ok(()),
        _ => Err(format!("<{a} vs {b}>")),
    }
}

#[test]
fn version_matrix_regression() {
    for entry in MATRIX {
        run_entry(entry);
    }
}
