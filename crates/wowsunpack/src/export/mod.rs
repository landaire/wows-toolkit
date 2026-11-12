use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[cfg(all(feature = "models", feature = "rkyv"))]
pub mod armor_extract;
#[cfg(feature = "models")]
pub mod armor_ray;
#[cfg(feature = "models")]
pub mod building_armor;
#[cfg(feature = "models")]
pub mod camo_composite;
#[cfg(feature = "models")]
pub mod camo_textures;
#[cfg(feature = "models")]
pub mod camouflage;
#[cfg(feature = "models")]
pub mod gltf_export;
#[cfg(feature = "models")]
pub mod hardpoint;
#[cfg(feature = "models")]
pub mod model_catalog;
#[cfg(feature = "models")]
pub mod model_library;
#[cfg(feature = "models")]
pub mod mount_armor;
#[cfg(feature = "models")]
pub mod part_group;
#[cfg(feature = "models")]
pub mod ship;
#[cfg(feature = "models")]
pub mod ship_parts;
#[cfg(feature = "models")]
pub mod size_estimate;
#[cfg(feature = "models")]
pub mod texture;

/// When true, export functions emit verbose diagnostic output to stderr
/// (e.g. per-vertex UV decoding details).
static DEBUG_OUTPUT: AtomicBool = AtomicBool::new(false);

/// Enable or disable verbose debug output for export operations.
pub fn set_debug(enabled: bool) {
    DEBUG_OUTPUT.store(enabled, Ordering::Relaxed);
}

#[allow(dead_code)]
pub(crate) fn debug_enabled() -> bool {
    DEBUG_OUTPUT.load(Ordering::Relaxed)
}
