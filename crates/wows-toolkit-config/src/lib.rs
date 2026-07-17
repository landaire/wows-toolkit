//! Shared persistence and leaf settings types for WoWs Toolkit.
//!
//! This crate holds the egui-free portions of the app's SQLite persistence
//! layer and the serializable settings/window types, so both the shipping
//! egui app and the GPUI port read the same on-disk database and formats.

mod db;
pub mod queries;
mod settings;
mod window;

use std::path::PathBuf;

pub use db::db_path;
pub use db::is_migrated;
pub use db::load_main_window_settings;
pub use db::open_db;
pub use db::set_migrated;
pub use settings::ReplayExportFormat;
pub use settings::ReplayGrouping;
pub use settings::ReplaySettings;
pub use window::WindowKind;
pub use window::WindowSettings;

/// Application name used to derive the on-disk storage directory.
pub const APP_NAME: &str = "WoWs Toolkit";

/// App data directory, matching eframe's `storage_dir()` layout so existing
/// data is found after removing the `persistence` feature.
///
/// - Windows: `%APPDATA%\WoWs Toolkit\data`
/// - macOS:   `~/Library/Application Support/WoWs-Toolkit`
/// - Linux:   `$XDG_DATA_HOME/wowstoolkit` or `~/.local/share/wowstoolkit`
pub fn storage_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // %APPDATA% = roaming appdata, same as eframe's FOLDERID_RoamingAppData
        std::env::var_os("APPDATA").map(PathBuf::from).map(|p| p.join(APP_NAME).join("data"))
    }
    #[cfg(target_os = "macos")]
    {
        home::home_dir().map(|p| {
            p.join("Library").join("Application Support").join(APP_NAME.replace(|c: char| c.is_ascii_whitespace(), "-"))
        })
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home::home_dir().map(|p| p.join(".local").join("share")))
            .map(|p| p.join(APP_NAME.to_lowercase().replace(|c: char| c.is_ascii_whitespace(), "")))
    }
}
