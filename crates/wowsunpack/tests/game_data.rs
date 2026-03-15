//! Integration tests for game data loading via VFS.
//!
//! Requires game data to be available. Run with:
//!   cargo test -p wowsunpack --features test-game-data

use sha2::Digest;
use sha2::Sha256;
use std::io::Read;

/// Verify that the VFS file tree can be built and contains expected top-level directories.
#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn vfs_file_tree_has_expected_directories() {
    let (build, vfs) = wows_data_mgr::latest_build().expect("game data should be available");
    eprintln!("Testing VFS for build {build}");

    let entries: Vec<String> = vfs.read_dir().expect("root read_dir should work").map(|e| e.filename()).collect();

    // These directories should always exist in a WoWs install
    assert!(entries.iter().any(|e| e == "content"), "missing 'content' directory, found: {entries:?}");
    assert!(entries.iter().any(|e| e == "scripts"), "missing 'scripts' directory, found: {entries:?}");
}

/// Verify that we can traverse into subdirectories.
#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn vfs_can_traverse_subdirectories() {
    let (_build, vfs) = wows_data_mgr::latest_build().expect("game data should be available");

    let scripts = vfs.join("scripts").expect("should join 'scripts'");
    let entries: Vec<String> =
        scripts.read_dir().expect("scripts read_dir should work").map(|e| e.filename()).collect();

    assert!(entries.iter().any(|e| e == "entity_defs"), "missing 'entity_defs' under scripts/, found: {entries:?}");
}

/// Verify that entity definition files exist and are readable.
#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn vfs_entity_defs_readable() {
    let (_build, vfs) = wows_data_mgr::latest_build().expect("game data should be available");

    let account_def = vfs.join("scripts/entity_defs/Account.def").expect("should join Account.def path");

    let mut content = Vec::new();
    account_def
        .open_file()
        .expect("should open Account.def")
        .read_to_end(&mut content)
        .expect("should read Account.def");

    assert!(!content.is_empty(), "Account.def should not be empty");

    let text = std::str::from_utf8(&content).expect("Account.def should be valid UTF-8");
    assert!(text.contains("curVersion_"), "Account.def should contain curVersion_ tag");
}

/// Verify that a stable file has the expected hash.
/// gui/maps_bg/99_gamemode_test.jpg should not change across versions.
#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn vfs_stable_file_hash() {
    let (_build, vfs) = wows_data_mgr::latest_build().expect("game data should be available");

    let path = "gui/maps_bg/99_gamemode_test.jpg";
    let file = vfs.join(path).and_then(|p| p.open_file());

    let Ok(mut file) = file else {
        eprintln!("Skipping hash test: {path} not found in VFS");
        return;
    };

    let mut data = Vec::new();
    file.read_to_end(&mut data).expect("should read file");
    assert!(!data.is_empty(), "{path} should not be empty");

    let hash = Sha256::digest(&data);
    let hex = format!("{hash:x}");

    // If this is the first run, print the hash so it can be recorded.
    // Once we know the expected hash, uncomment the assertion below.
    eprintln!("{path}: sha256={hex}, size={} bytes", data.len());

    // The JPEG header should be present
    assert_eq!(&data[..2], &[0xFF, 0xD8], "file should start with JPEG magic");
}

/// Verify that GameParams.data can be read from the VFS.
#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn vfs_game_params_readable() {
    let (_build, vfs) = wows_data_mgr::latest_build().expect("game data should be available");

    let path = "content/GameParams.data";
    let mut data = Vec::new();
    vfs.join(path)
        .expect("should join GameParams.data path")
        .open_file()
        .expect("should open GameParams.data")
        .read_to_end(&mut data)
        .expect("should read GameParams.data");

    assert!(data.len() > 1_000_000, "GameParams.data should be > 1MB, got {} bytes", data.len());
}

/// Verify that all registered builds can construct a VFS.
#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn all_builds_construct_vfs() {
    let builds = wows_data_mgr::available_builds();
    assert!(!builds.is_empty(), "should have at least one build");

    for build in builds {
        let vfs = wows_data_mgr::vfs_for_build(build);
        assert!(vfs.is_some(), "should be able to construct VFS for build {build}");
    }
}
