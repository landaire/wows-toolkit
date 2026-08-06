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
pub const PIN_VARS: &[&str] =
    &["VK_LOADER_DRIVERS_SELECT", "VK_DRIVER_FILES", "VK_ICD_FILENAMES", "VK_LOADER_LAYERS_DISABLE"];

/// Strip this process's Vulkan pin from a child command.
///
/// `CreateProcess` copies the environment block into every child, so without
/// this a child inherits both the driver pin and the layer suppression. That
/// matters most for the game: opening a replay must not silently move World of
/// Warships onto whichever adapter this app happens to have descended to, nor
/// strip its Steam overlay and capture hooks. It matters for the updater
/// relaunch too, where an inherited pin would outlive the mode that set it.
pub fn unpin_child(command: &mut std::process::Command) -> &mut std::process::Command {
    for name in PIN_VARS {
        command.env_remove(name);
    }
    command
}
