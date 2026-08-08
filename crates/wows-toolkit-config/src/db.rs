//! SQLite persistence plumbing shared between the egui app and the GPUI port.
//!
//! Opens the application database, runs embedded migrations, and exposes the
//! synchronous window-geometry read used at startup.

use std::path::PathBuf;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqliteJournalMode;
use sqlx::sqlite::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::sqlite::SqliteSynchronous;
use tracing::error;
use tracing::info;

use crate::queries;
use crate::window::WindowKind;
use crate::window::WindowSettings;

/// Open (or create) the application database and run pending migrations.
///
/// The database lives alongside other app data in the eframe storage directory.
/// We use WAL journal mode for better read concurrency (although this is a
/// single-writer desktop app, WAL is still faster for mixed read/write).
pub async fn open_db() -> Result<SqlitePool, sqlx::Error> {
    let db_path = db_path();
    info!("Opening database at {}", db_path.display());

    // Ensure parent directory exists.
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        // Moderate busy timeout so concurrent reads don't immediately fail.
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await?;

    // Run embedded migrations.
    sqlx::migrate!("./migrations").run(&pool).await.map_err(|e| {
        error!("Failed to run database migrations: {e}");
        sqlx::Error::Protocol(format!("migration error: {e}"))
    })?;

    info!("Database ready");
    Ok(pool)
}

/// Returns the path to the SQLite database file.
pub fn db_path() -> PathBuf {
    crate::storage_dir().unwrap_or_else(|| PathBuf::from(".")).join("wows_toolkit.db")
}

/// Load just the main window settings from the database, synchronously.
///
/// Used in `main()` to set the initial viewport position/size on the
/// `ViewportBuilder` before the app is created (position can only be set
/// at builder time, not via viewport commands).
pub fn load_main_window_settings() -> Option<WindowSettings> {
    use std::collections::HashMap;

    let sizes: HashMap<WindowKind, WindowSettings> = load_startup_setting("window_sizes").ok().flatten()?;

    sizes.get(&WindowKind::Main).copied()
}

/// Why a startup-time settings read did not produce a value.
///
/// Separate from the value being absent, because the two call for different
/// answers: an absent setting has a correct default, and an unreadable one has
/// none.
#[derive(Debug, thiserror::Error)]
pub enum StartupSettingError {
    #[error("no tokio runtime could be built to read {key}")]
    Runtime { key: String },
    #[error("the settings database at {path} could not be opened")]
    Open { path: PathBuf },
}

/// Read one setting from the database, synchronously, before the app exists.
///
/// For decisions `main()` has to make ahead of `WowsToolkitApp`, which owns the
/// real connection pool: the initial viewport geometry, and which process
/// mitigations to apply. Read-only, so it cannot race the running app's writer.
///
/// `Ok(None)` is a genuine absence: a first launch with no database at all, or
/// a key that was never written. `Err` means the read could not be performed,
/// which callers must not treat as absence.
pub fn load_startup_setting<T: serde::de::DeserializeOwned>(key: &str) -> Result<Option<T>, StartupSettingError> {
    let db_path = db_path();
    if !db_path.exists() {
        return Ok(None);
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| StartupSettingError::Runtime { key: key.to_string() })?;
    let pool = rt
        .block_on(async {
            let options = SqliteConnectOptions::new()
                .filename(&db_path)
                .read_only(true)
                .busy_timeout(std::time::Duration::from_secs(1));
            SqlitePoolOptions::new().max_connections(1).connect_with(options).await.ok()
        })
        .ok_or(StartupSettingError::Open { path: db_path })?;

    Ok(rt.block_on(queries::get_setting(&pool, key)))
}

/// Check whether the one-time migration from `app.ron` has already been performed.
pub async fn is_migrated(pool: &SqlitePool) -> bool {
    queries::get_setting::<bool>(pool, "migration_completed").await.unwrap_or(false)
}

/// Mark the one-time migration as completed.
pub async fn set_migrated(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    queries::set_setting(pool, "migration_completed", &true).await
}
