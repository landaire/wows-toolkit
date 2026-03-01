//! Integration tests for replay file parsing.
//!
//! These tests use committed fixture replays spanning game versions 0.8.5 (2019)
//! through 15.1 (2026) to verify that metadata parsing does not regress.

use std::path::PathBuf;
use wows_replays::ReplayFile;

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

/// Parse all fixture replays without panicking.
#[test]
fn parse_all_fixtures() {
    let dir = fixtures_dir();
    assert!(dir.exists(), "fixtures dir missing: {}", dir.display());

    let mut count = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("wowsreplay") {
            continue;
        }
        let replay = ReplayFile::from_file(&path);
        assert!(replay.is_ok(), "failed to parse {}: {:?}", path.display(), replay.err());
        count += 1;
    }
    assert!(count >= 10, "expected at least 10 fixture replays, found {count}");
}

/// Verify metadata for a 2025 DD replay (version 14.1).
#[test]
fn meta_2025_hull() {
    let path = fixtures_dir().join("20250206_020938_PASD720-Hull_47_Sleeping_Giant.wowsreplay");
    let replay = ReplayFile::from_file(&path).unwrap();
    let m = &replay.meta;

    assert_eq!(m.playerName, "Biiison");
    assert_eq!(m.playerVehicle, "PASD720-Hull");
    assert_eq!(m.mapDisplayName, "47_Sleeping_Giant");
    assert_eq!(m.matchGroup, "pvp");
    assert_eq!(m.mapId, 29);
    assert_eq!(m.vehicles.len(), 24);
    assert!(m.clientVersionFromExe.starts_with("14,1,"));
}

/// Verify metadata for a 2026 DD replay (version 15.0).
#[test]
fn meta_2026_forrest_sherman() {
    let path = fixtures_dir().join("20260127_185500_PASD610-Forrest-Sherman_56_AngelWings.wowsreplay");
    let replay = ReplayFile::from_file(&path).unwrap();
    let m = &replay.meta;

    assert_eq!(m.playerName, "QUIDPROQUOWINKWINK");
    assert_eq!(m.playerVehicle, "PASD610-Forrest-Sherman");
    assert_eq!(m.mapDisplayName, "56_AngelWings");
    assert_eq!(m.matchGroup, "pvp");
    assert_eq!(m.mapId, 36);
    assert_eq!(m.vehicles.len(), 24);
    assert!(m.clientVersionFromExe.starts_with("15,0,"));
}

/// Verify metadata for a 2023 submarine replay (version 12.3).
#[test]
fn meta_2023_s189_submarine() {
    let path = fixtures_dir().join("20230419_203306_PRSS508-S-189_42_Neighbors.wowsreplay");
    let replay = ReplayFile::from_file(&path).unwrap();
    let m = &replay.meta;

    assert_eq!(m.playerName, "TF2_Electric_Boogaloo");
    assert_eq!(m.playerVehicle, "PRSS508-S-189");
    assert_eq!(m.mapDisplayName, "42_Neighbors");
    assert_eq!(m.matchGroup, "pvp");
    assert_eq!(m.vehicles.len(), 24);
    assert!(m.clientVersionFromExe.starts_with("12,3,"));
}

/// Verify metadata for a 2024 supership replay (version 13.2).
#[test]
fn meta_2024_annapolis() {
    let path = fixtures_dir().join("20240402_192304_PASC111-Annapolis_22_tierra_del_fuego.wowsreplay");
    let replay = ReplayFile::from_file(&path).unwrap();
    let m = &replay.meta;

    assert_eq!(m.playerName, "ChineseTechAbuser");
    assert_eq!(m.playerVehicle, "PASC111-Annapolis");
    assert_eq!(m.mapDisplayName, "22_tierra_del_fuego");
    assert_eq!(m.matchGroup, "pvp");
    assert_eq!(m.vehicles.len(), 24);
    assert!(m.clientVersionFromExe.starts_with("13,2,"));
}

/// Verify metadata for a late 2024 replay (version 13.10).
#[test]
fn meta_2024_colbert() {
    let path = fixtures_dir().join("20241112_172819_PFSC510-Colbert_44_Path_warrior.wowsreplay");
    let replay = ReplayFile::from_file(&path).unwrap();
    let m = &replay.meta;

    assert_eq!(m.playerName, "John_The_Ruthless");
    assert_eq!(m.playerVehicle, "PFSC510-Colbert");
    assert_eq!(m.mapDisplayName, "44_Path_warrior");
    assert_eq!(m.matchGroup, "pvp");
    assert!(m.clientVersionFromExe.starts_with("13,10,"));
}

/// Verify metadata for an early 2025 replay (version 14.2).
#[test]
fn meta_2025_oland() {
    let path = fixtures_dir().join("20250117_004534_PWSD108-Oland_15_NE_north.wowsreplay");
    let replay = ReplayFile::from_file(&path).unwrap();
    let m = &replay.meta;

    assert_eq!(m.playerName, "awesome101_21x");
    assert_eq!(m.playerVehicle, "PWSD108-Oland");
    assert_eq!(m.mapDisplayName, "15_NE_north");
    assert_eq!(m.matchGroup, "pvp");
    assert!(m.clientVersionFromExe.starts_with("14,2,"));
}

/// Verify metadata for a carrier in an event mode (version 14.9).
#[test]
fn meta_2025_ocean_cv_event() {
    let path = fixtures_dir().join("20251001_145225_PBSA710-Ocean_28_naval_mission.wowsreplay");
    let replay = ReplayFile::from_file(&path).unwrap();
    let m = &replay.meta;

    assert_eq!(m.playerName, "seaznutz");
    assert_eq!(m.playerVehicle, "PBSA710-Ocean");
    assert_eq!(m.matchGroup, "event");
    assert_eq!(m.vehicles.len(), 17);
    assert!(m.clientVersionFromExe.starts_with("14,9,"));
}

/// Verify metadata for a 2026 battleship replay (version 15.1).
#[test]
fn meta_2026_vermont() {
    let path = fixtures_dir().join("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
    let replay = ReplayFile::from_file(&path).unwrap();
    let m = &replay.meta;

    assert_eq!(m.playerName, "P_yk");
    assert_eq!(m.playerVehicle, "PASB110-Vermont");
    assert_eq!(m.mapDisplayName, "22_tierra_del_fuego");
    assert_eq!(m.matchGroup, "pvp");
    assert!(m.clientVersionFromExe.starts_with("15,1,"));
}

/// Verify metadata for a 2026 PvE/operations replay (version 15.1).
#[test]
fn meta_2026_narai_pve() {
    let path = fixtures_dir().join("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
    let replay = ReplayFile::from_file(&path).unwrap();
    let m = &replay.meta;

    assert_eq!(m.playerName, "Minotaur_of_the_Seas");
    assert_eq!(m.playerVehicle, "PZSC718-Narai");
    assert_eq!(m.mapDisplayName, "s06_Atoll");
    assert_eq!(m.matchGroup, "pve");
    assert_eq!(m.mapId, 42);
}

/// Verify packet data is non-empty for all fixtures.
#[test]
fn packet_data_present() {
    let dir = fixtures_dir();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("wowsreplay") {
            continue;
        }
        let replay = ReplayFile::from_file(&path).unwrap();
        assert!(!replay.packet_data.is_empty(), "packet_data empty for {}", path.display());
    }
}

/// Verify vehicle info contains expected fields for all fixtures.
#[test]
fn vehicle_info_valid() {
    let dir = fixtures_dir();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("wowsreplay") {
            continue;
        }
        let replay = ReplayFile::from_file(&path).unwrap();
        assert!(!replay.meta.vehicles.is_empty(), "no vehicles in {}", path.display());
        for v in &replay.meta.vehicles {
            assert!(!v.name.is_empty(), "empty player name in {}", path.display());
            assert!(v.relation <= 2, "invalid relation {} in {}", v.relation, path.display());
        }
    }
}
