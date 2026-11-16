//! Display adapter discovery and render configuration.
//!
//! The probe reads the driver registry rather than asking a graphics API, so it
//! costs nothing and, critically, loads no vendor code. That matters because
//! wgpu's adapter enumeration loads every installed ICD before any selection
//! callback runs, which is how both vendors' user-mode stacks end up resident in
//! a single-GPU render session.

pub mod probe;
pub mod select;

/// Vulkan loader variables the pin sets on this process.
///
/// `CreateProcess` copies the environment block into every child, so these
/// travel into one unless removed; `crate::hardening::prepare_child` is what
/// removes them.
pub const PIN_VARS: &[&str] =
    &["VK_LOADER_DRIVERS_SELECT", "VK_DRIVER_FILES", "VK_ICD_FILENAMES", "VK_LOADER_LAYERS_DISABLE"];
