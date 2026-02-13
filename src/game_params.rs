use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tracing::debug;
use wowsunpack::data::idx::FileNode;
use wowsunpack::data::pkg::PkgFileLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;

use crate::error::ToolkitError;

/// Path to the old unversioned game_params.bin cache (for migration cleanup).
pub fn old_game_params_bin_path() -> PathBuf {
    let old_cache_path = std::path::Path::new("game_params.bin");
    if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
        storage_dir.join(old_cache_path)
    } else {
        old_cache_path.to_path_buf()
    }
}

pub fn game_params_bin_path(build: u32) -> PathBuf {
    let filename = format!("game_params_{build}.bin");
    if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
        storage_dir.join(filename)
    } else {
        PathBuf::from(filename)
    }
}

/// Remove game_params cache files for builds that no longer exist in the game directory.
pub fn cleanup_stale_caches(available_builds: &[u32]) {
    let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) else { return };

    // Remove the old unversioned cache
    let _ = std::fs::remove_file(storage_dir.join("game_params.bin"));

    let Ok(entries) = std::fs::read_dir(&storage_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Clean up versioned game_params
        if let Some(rest) = name_str.strip_prefix("game_params_") {
            if let Some(build_str) = rest.strip_suffix(".bin") {
                if let Ok(build) = build_str.parse::<u32>() {
                    if !available_builds.contains(&build) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }

        // Clean up versioned constants
        if let Some(rest) = name_str.strip_prefix("constants_") {
            if let Some(build_str) = rest.strip_suffix(".json") {
                if let Ok(build) = build_str.parse::<u32>() {
                    if !available_builds.contains(&build) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

pub fn load_game_params(
    file_tree: &FileNode,
    pkg_loader: &PkgFileLoader,
    game_version: usize,
) -> Result<GameMetadataProvider, ToolkitError> {
    debug!("loading game params for build {}", game_version);

    let cache_path = game_params_bin_path(game_version as u32);

    let start = Instant::now();
    let params = cache_path
        .exists()
        .then(|| {
            let cache_data = std::fs::read(&cache_path).ok()?;
            let params: Vec<Param> = rkyv::from_bytes::<Vec<Param>, rkyv::rancor::Error>(&cache_data).ok()?;
            Some(params)
        })
        .flatten();

    let metadata_provider = if let Some(params) = params {
        GameMetadataProvider::from_params(params, file_tree, pkg_loader)?
    } else {
        let metadata_provider = GameMetadataProvider::from_pkg(file_tree, pkg_loader)?;
        let params: Vec<Param> =
            metadata_provider.params().iter().map(|param| Arc::unwrap_or_clone(Arc::clone(param))).collect();

        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&params).expect("failed to serialize cached game params");
        std::fs::write(&cache_path, &bytes).expect("failed to write cached game params");

        metadata_provider
    };

    let now = Instant::now();
    debug!("took {} seconds to load", (now - start).as_secs());

    Ok(metadata_provider)
}
