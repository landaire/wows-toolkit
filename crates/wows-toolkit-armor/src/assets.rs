//! The data the Armor Viewer reads for every ship and model of one build:
//! the shared [`ShipAssets`] (assets.bin + camo DB, reused across every ship
//! the viewer opens) and the [`ModelAssets`] any catalogued model opens from.
//! Both loads are synchronous and CPU/IO-bound; hosts run them off their UI
//! thread.

use std::sync::Arc;

use rootcause::Report;
use wowsunpack::export::building_armor::BuildingArmorIndex;
use wowsunpack::export::model_library::ModelLibrary;
use wowsunpack::export::model_library::ModelLibraryError;
use wowsunpack::export::mount_armor::MountArmorIndex;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::vfs::VfsPath;

/// Reasons the Armor Viewer's ship data failed to load. Wraps `ShipAssets`'
/// `rootcause::Report` errors rather than surfacing the raw report type
/// across a background-task boundary.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ArmorAssetsError {
    #[error("failed to load ship assets: {0}")]
    ShipAssets(String),
}

/// Why the model library could not be built from loaded ship assets.
#[derive(Debug, thiserror::Error)]
pub enum ModelLibraryLoadError {
    #[error("failed to build the model library: {report}")]
    Build { report: Report<ModelLibraryError> },
}

/// What opening and exporting any catalogued model reads: the library over
/// `assets.bin`, the index of which ship mounts which model, where a model's
/// armor thickness comes from, and the armored buildings with their mounts.
pub struct ModelAssets {
    pub library: Arc<ModelLibrary>,
    pub mount_armor: MountArmorIndex,
    pub buildings: BuildingArmorIndex,
}

impl ModelAssets {
    /// Indexes `assets.bin` through `assets`. Separate from the ship-asset
    /// load so the ship list is usable while it runs.
    pub fn build(assets: &ShipAssets) -> Result<Self, ModelLibraryLoadError> {
        let library = assets.model_library().map_err(|report| ModelLibraryLoadError::Build { report })?;
        let mount_armor = MountArmorIndex::build(assets.metadata(), library.catalog());
        let buildings = BuildingArmorIndex::build(assets.metadata(), &library);

        Ok(Self { library: Arc::new(library), mount_armor, buildings })
    }
}

/// Loads `vfs`/`metadata`'s [`ShipAssets`]. `assets_bin_bytes` is the
/// `assets.bin` blob when the caller already holds it.
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
            .map_err(|report| ArmorAssetsError::ShipAssets(format!("{report:?}"))),
    }
}
