//! The shared [`ShipAssets`] (assets.bin + camo DB, reused across every ship
//! the viewer opens), built from an already-loaded game data VFS and
//! `GameMetadataProvider` so `GameParams` is never parsed twice.

use std::sync::Arc;

use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::vfs::VfsPath;

use crate::error::ArmorAssetsError;

/// Loads `vfs`/`metadata`'s [`ShipAssets`]. Synchronous and CPU/IO-bound;
/// callers run it off the UI thread.
pub fn load_ship_assets(
    vfs: &VfsPath,
    assets_bin_bytes: Option<&Arc<Vec<u8>>>,
    metadata: &Arc<GameMetadataProvider>,
) -> Result<ShipAssets, ArmorAssetsError> {
    // The overlay already holds the blob; reading it again would hold 174 MiB
    // of it twice.
    match assets_bin_bytes {
        Some(bytes) => Ok(ShipAssets::from_parts(vfs, Arc::clone(metadata), Arc::clone(bytes))),
        None => ShipAssets::from_vfs_with_metadata(vfs, Arc::clone(metadata))
            .map_err(|report| ArmorAssetsError::ShipAssets { source: report.into() }),
    }
}
