use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "models")]
pub mod camouflage;
#[cfg(feature = "models")]
pub mod gltf_export;
#[cfg(feature = "models")]
pub mod ship;
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
