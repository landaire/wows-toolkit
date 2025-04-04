use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use wowsunpack::data::idx::FileNode;
use wowsunpack::data::pkg::PkgFileLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;

use crate::error::ToolkitError;

#[derive(Debug, Serialize, Deserialize)]
struct CachedGameParams {
    app_version: String,
    game_version: usize,
    params: Vec<Param>,
}

pub fn game_params_bin_path() -> PathBuf {
    let old_cache_path = Path::new("game_params.bin");
    if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) { storage_dir.join(old_cache_path) } else { old_cache_path.to_path_buf() }
}

pub fn load_game_params(file_tree: &FileNode, pkg_loader: &PkgFileLoader, game_version: usize) -> Result<GameMetadataProvider, ToolkitError> {
    debug!("loading game params");
    let old_cache_path = Path::new("game_params.bin");

    let cache_path = if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
        let new_cache_path = storage_dir.join(old_cache_path);
        if !new_cache_path.exists() && old_cache_path.exists() {
            // Doesn't matter if this fails, we want to only use the new cache path.
            // The implication of failure here is that the user re-generates
            // the cache.
            let _ = std::fs::rename(old_cache_path, &new_cache_path);
        }

        new_cache_path
    } else {
        old_cache_path.to_path_buf()
    };

    let start = Instant::now();
    let params = cache_path
        .exists()
        .then(|| {
            let mut cache_data = std::fs::File::open(&cache_path).ok()?;
            let cached_params: CachedGameParams = bincode::serde::decode_from_std_read(&mut cache_data, bincode::config::standard()).ok()?;
            if cached_params.game_version == game_version { Some(cached_params.params) } else { None }
        })
        .flatten();

    let metadata_provider = if let Some(params) = params {
        GameMetadataProvider::from_params(params, file_tree, pkg_loader)?
    } else {
        let metadata_provider = GameMetadataProvider::from_pkg(file_tree, pkg_loader)?;
        let cached_params = CachedGameParams {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            game_version,
            // TODO: kind of unnecessarily expensive to round-trip from Arc to Owned here.
            params: metadata_provider.params().iter().map(|param| Arc::unwrap_or_clone(Arc::clone(param))).collect(),
        };

        let mut file = std::fs::File::create(cache_path).unwrap();
        bincode::serde::encode_into_std_write(&cached_params, &mut file, bincode::config::standard()).expect("failed to serialize cached game params");

        metadata_provider
    };

    let now = Instant::now();
    debug!("took {} seconds to load", (now - start).as_secs());

    Ok(metadata_provider)
}
