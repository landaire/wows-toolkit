//! Snapshot tests for replay packet decoding.
//!
//! These tests parse fixture replays, decode packets using game data,
//! and snapshot the output with insta. Each packet gets its own snapshot
//! file under `snapshots/<replay_filename>/packet_NNN.snap`.
//!
//! Tests are ignored when the required game data build is not available.
//! When no game data is discovered at all, every test in this file is skipped.

use std::path::PathBuf;
use serde_json::Value;
use wows_replays::analyzer::decoder::PacketDecoder;
use wows_replays::packet2::Parser;
use wows_replays::ReplayFile;
use wowsunpack::data::Version;
use wowsunpack::game_data;

/// Recursively sort all object keys in a JSON value so snapshots are deterministic
/// regardless of HashMap iteration order.
fn sort_json_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: serde_json::Map<String, Value> = map
                .into_iter()
                .map(|(k, v)| (k, sort_json_keys(v)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect();
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(sort_json_keys).collect()),
        other => other,
    }
}

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

/// Helper: parse a replay and snapshot each of the first N packets individually.
///
/// Snapshots are written to `snapshots/<replay_stem>/packet_NNN.snap`.
fn snapshot_first_n_packets(replay_filename: &str, n: usize) {
    let path = fixtures_dir().join(replay_filename);
    let replay = ReplayFile::from_file(&path).expect("should parse replay");
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let game_dir = wows_game_data_dl::game_dir_for_build(version.build)
        .unwrap_or_else(|| panic!("game data for build {} not available", version.build));

    let resources =
        game_data::load_game_resources(&game_dir, &version).expect("should load game resources");

    let mut parser = Parser::new(&resources.specs);
    let decoder = PacketDecoder::builder().version(version).build();

    let replay_stem = replay_filename.trim_end_matches(".wowsreplay");
    let snapshot_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(replay_stem);

    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(&snapshot_dir);

    let mut remaining = &replay.packet_data[..];
    let mut i = 0;

    settings.bind(|| {
        while !remaining.is_empty() && i < n {
            let packet = parser
                .parse_packet(&mut remaining)
                .expect("should parse packet");
            let decoded = decoder.decode(&packet);
            let value =
                serde_json::to_value(&decoded).expect("should serialize decoded packet");
            let value = sort_json_keys(value);
            insta::assert_yaml_snapshot!(format!("packet_{i:03}"), value);
            i += 1;
        }
    });
}

/// Snapshot the first 50 decoded packets from the 2026 Vermont replay (v15.1, build 11965230).
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn snapshot_vermont_packets() {
    snapshot_first_n_packets(
        "20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay",
        50,
    );
}

/// Snapshot the first 50 decoded packets from the 2026 Marceau replay (v15.1, build 11965230).
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn snapshot_marceau_packets() {
    snapshot_first_n_packets(
        "20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay",
        50,
    );
}

/// Snapshot the first 50 decoded packets from the 2026 Narai PvE replay (v15.1, build 11965230).
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn snapshot_narai_pve_packets() {
    snapshot_first_n_packets(
        "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay",
        50,
    );
}

/// Snapshot the first 50 decoded packets from the 2025 Hull DD replay (v14.1, build 9531281).
#[test]
#[cfg_attr(not(all(has_game_data, has_build_9531281)), ignore)]
fn snapshot_hull_packets() {
    snapshot_first_n_packets(
        "20250206_020938_PASD720-Hull_47_Sleeping_Giant.wowsreplay",
        50,
    );
}

/// Snapshot the first 50 decoded packets from the 2023 S-189 submarine replay (v12.3, build 6965290).
#[test]
#[cfg_attr(not(all(has_game_data, has_build_6965290)), ignore)]
fn snapshot_s189_submarine_packets() {
    snapshot_first_n_packets(
        "20230419_203306_PRSS508-S-189_42_Neighbors.wowsreplay",
        50,
    );
}
