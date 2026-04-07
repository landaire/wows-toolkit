//! Test helper API for accessing downloaded World of Warships game data.
//!
//! Use these functions in integration tests to get VFS access to game builds.
//! Tests should skip gracefully when game data is unavailable.
//!
//! # Example
//!
//! ```ignore
//! use wows_data_mgr::{available_builds, vfs_for_build};
//!
//! #[test]
//! fn test_game_params_load() {
//!     let builds = available_builds();
//!     if builds.is_empty() {
//!         eprintln!("Skipping: no game data available");
//!         return;
//!     }
//!     for build in builds {
//!         let vfs = vfs_for_build(build).unwrap();
//!         // test with vfs...
//!     }
//! }
//! ```

pub mod dump;
pub mod manifest;
pub mod registry;

use std::path::PathBuf;

use wowsunpack::game_data;
use wowsunpack::vfs::VfsPath;

/// Returns the path to the game_data/ directory.
///
/// Checks `WOWS_GAME_DATA` env var first, then walks up from the current
/// directory to find the workspace root (identified by `game_versions.toml`).
pub fn game_data_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("WOWS_GAME_DATA") {
        let path = PathBuf::from(dir);
        if path.exists() {
            return Some(path);
        }
    }

    // Walk up from current dir to find repo root
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("game_versions.toml").exists() {
            let data_dir = dir.join("game_data");
            return Some(data_dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Returns sorted list of locally available build numbers.
///
/// Reads the local registry to find both downloaded builds
/// (in `game_data/builds/<build>/`) and registered overrides.
pub fn available_builds() -> Vec<u32> {
    let Some(data_dir) = game_data_dir() else {
        return Vec::new();
    };
    let reg = registry::load_registry(&data_dir.join("versions.toml"));
    reg.available_builds()
}

/// Returns the game root path for a specific build.
///
/// For registered overrides, returns the override path.
/// For downloaded builds, returns `game_data/builds/<build>/`.
pub fn game_dir_for_build(build: u32) -> Option<PathBuf> {
    let data_dir = game_data_dir()?;
    let reg = registry::load_registry(&data_dir.join("versions.toml"));
    reg.game_dir_for_build(build, &data_dir)
}

/// Constructs a VFS for a specific build.
///
/// Resolves path via [`game_dir_for_build`], then calls
/// [`wowsunpack::game_data::build_game_vfs`].
pub fn vfs_for_build(build: u32) -> Option<VfsPath> {
    let game_dir = game_dir_for_build(build)?;
    game_data::build_game_vfs(&game_dir).ok()
}

/// Returns the latest available build number and its VFS.
pub fn latest_build() -> Option<(u32, VfsPath)> {
    let builds = available_builds();
    let build = *builds.last()?;
    let vfs = vfs_for_build(build)?;
    Some((build, vfs))
}
