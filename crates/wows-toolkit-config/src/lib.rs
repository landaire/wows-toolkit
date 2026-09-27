//! Shared persistence and leaf settings types for WoWs Toolkit.
//!
//! This crate holds the egui-free portions of the app's SQLite persistence
//! layer and the serializable settings/window types, so both the shipping
//! egui app and the GPUI port read the same on-disk database and formats.

mod db;
pub mod index;
pub mod queries;
mod settings;
pub mod tracker;
mod window;

use std::path::PathBuf;

pub use db::StartupSettingError;
pub use db::db_path;
pub use db::is_migrated;
pub use db::load_main_window_settings;
pub use db::load_startup_setting;
pub use db::open_db;
pub use db::open_db_at;
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
    if let Some(chosen) = storage_override::get() {
        return Some(chosen);
    }

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

/// An empty database with every migration applied, for a test that needs real
/// tables rather than hand-written ones.
#[cfg(feature = "test-support")]
pub async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("an in-memory database opens");
    sqlx::migrate!("./migrations").run(&pool).await.expect("the migrations apply");
    pool
}

/// A storage directory chosen for this process in place of the platform one.
///
/// Tests set it so nothing reads or writes the running user's own data: the
/// settings tab measures the game-data cache under [`storage_dir`], and
/// against a real install that is a walk of several gigabytes.
#[cfg(feature = "test-support")]
pub mod storage_override {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    static CHOSEN: OnceLock<PathBuf> = OnceLock::new();

    /// Points every [`super::storage_dir`] caller in this process at `dir`.
    ///
    /// Takes effect once: a process has one storage directory, and a test
    /// that could move it out from under another would be worse than one
    /// that shares a temporary directory with it.
    pub fn set(dir: PathBuf) {
        let _ = CHOSEN.set(dir);
    }

    pub(crate) fn get() -> Option<PathBuf> {
        CHOSEN.get().cloned()
    }
}

#[cfg(not(feature = "test-support"))]
mod storage_override {
    use std::path::PathBuf;

    pub(crate) fn get() -> Option<PathBuf> {
        None
    }
}

/// Where dumped game data is cached when the reader has not chosen a
/// directory.
///
/// `None` when there is no storage directory to hang it off, which is the
/// same absence [`storage_dir`] reports.
pub fn game_data_dump_base() -> Option<PathBuf> {
    storage_dir().map(|dir| dir.join("game_data"))
}

/// Where dumped game data is cached, preferring the reader's own directory.
///
/// An empty setting means none was chosen. A relative one is refused rather
/// than resolved against whatever the process happens to be running in, since
/// the cache outlives the session that wrote the setting.
pub fn game_data_dump_base_with_override(custom_dir: &str) -> Option<PathBuf> {
    let chosen = PathBuf::from(custom_dir);
    if chosen.is_absolute() {
        return Some(chosen);
    }
    game_data_dump_base()
}

#[cfg(test)]
mod dump_base_tests {
    use super::*;

    #[test]
    fn an_absolute_override_is_taken_as_given() {
        let absolute = if cfg!(windows) { r"C:\game-data-elsewhere" } else { "/game-data-elsewhere" };

        assert_eq!(game_data_dump_base_with_override(absolute), Some(PathBuf::from(absolute)));
    }

    #[test]
    fn an_empty_or_relative_override_falls_back_to_the_default() {
        // A relative path is refused for the same reason an empty one is: it
        // names no fixed place for a cache that outlives the session.
        assert_eq!(game_data_dump_base_with_override(""), game_data_dump_base());
        assert_eq!(game_data_dump_base_with_override("game_data"), game_data_dump_base());
    }
}
