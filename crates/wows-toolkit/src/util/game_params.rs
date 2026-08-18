use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tracing::debug;
use tracing::info;
use tracing::instrument;
use wowsunpack::game_params::cache;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::vfs::VfsPath;

use crate::util::error::ToolkitError;

/// Path to the old unversioned game_params.bin cache (for migration cleanup).
#[allow(dead_code)]
pub fn old_game_params_bin_path() -> PathBuf {
    let old_cache_path = std::path::Path::new("game_params.bin");
    if let Some(storage_dir) = crate::storage_dir() {
        storage_dir.join(old_cache_path)
    } else {
        old_cache_path.to_path_buf()
    }
}

pub fn game_params_bin_path(build: u32) -> PathBuf {
    let filename = format!("game_params_{build}.bin");
    if let Some(storage_dir) = crate::storage_dir() { storage_dir.join(filename) } else { PathBuf::from(filename) }
}

/// Local directory whose contents shadow a dumped build's derived artifacts.
///
/// Deliberately not the `game_params_{build}.bin` namespace next to it:
/// [`cleanup_stale_caches`] deletes those for any build absent from the live
/// install, and a dumped build never appears there, so a cache written for one
/// would be removed on the next start.
pub fn build_override_root(build: u32) -> Option<PathBuf> {
    wows_data_mgr::overrides::build_override_root(build)
}

/// Remove ALL versioned game_params cache files (for schema changes).
#[instrument]
pub fn clear_all_game_params_caches() {
    info!("Clearing gameparams cache");

    let Some(storage_dir) = crate::storage_dir() else { return };
    let _ = std::fs::remove_file(storage_dir.join("game_params.bin"));
    let Ok(entries) = std::fs::read_dir(&storage_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("game_params_") && name.to_string_lossy().ends_with(".bin") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Remove game_params cache files for builds that no longer exist in the game directory.
#[instrument(skip(available_builds), fields(build_count = available_builds.len()))]
pub fn cleanup_stale_caches(available_builds: &[u32]) {
    info!("Clearing stale caches");

    let Some(storage_dir) = crate::storage_dir() else { return };

    // Remove the old unversioned cache
    let _ = std::fs::remove_file(storage_dir.join("game_params.bin"));

    let Ok(entries) = std::fs::read_dir(&storage_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Clean up versioned game_params
        if let Some(rest) = name_str.strip_prefix("game_params_")
            && let Some(build_str) = rest.strip_suffix(".bin")
            && let Ok(build) = build_str.parse::<u32>()
            && !available_builds.contains(&build)
        {
            let _ = std::fs::remove_file(entry.path());
        }

        // Clean up versioned constants
        if let Some(rest) = name_str.strip_prefix("constants_")
            && let Some(build_str) = rest.strip_suffix(".json")
            && let Ok(build) = build_str.parse::<u32>()
            && !available_builds.contains(&build)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[instrument(skip(vfs))]
pub fn load_game_params(vfs: &VfsPath, game_version: usize) -> Result<GameMetadataProvider, ToolkitError> {
    debug!("Loading gameparams");

    let cache_path = game_params_bin_path(game_version as u32);

    let start = Instant::now();
    let cached = cache::load(&cache_path);
    if cached.is_some() {
        debug!("Loaded params from disk");
    }

    let metadata_provider = if let Some(params) = cached {
        GameMetadataProvider::from_params_with_vfs(params, vfs)?
    } else {
        info!("Loading gameparams from game files");

        let metadata_provider = GameMetadataProvider::from_vfs(vfs)?;
        let params: Vec<_> =
            metadata_provider.params().iter().map(|param| Arc::unwrap_or_clone(Arc::clone(param))).collect();
        cache::save(&cache_path, &params).expect("failed to write cached game params");

        metadata_provider
    };

    let now = Instant::now();
    debug!("took {} seconds to load", (now - start).as_secs());

    Ok(metadata_provider)
}

#[cfg(test)]
mod tests {
    /// The CLIs resolve the override root through wows-data-mgr; if the two
    /// disagree, a cache rebuilt by one tool is invisible to the other.
    #[test]
    fn override_root_matches_the_apps_storage_dir() {
        let Some(storage) = crate::storage_dir() else {
            eprintln!("skipping: no storage dir on this machine");
            return;
        };
        assert_eq!(super::build_override_root(13015811), Some(storage.join("build_overrides").join("13015811")));
    }
}
