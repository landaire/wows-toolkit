//! SQLite persistence layer.
//!
//! Replaces eframe's RON-based `app.ron` persistence with a SQLite database.
//! On first launch after the migration, `app.ron` is read and its contents are
//! written into the database. Subsequent launches read directly from SQLite.

pub mod load;
pub mod migrate_ron;
pub mod queries;
pub mod save;

use std::path::PathBuf;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqliteJournalMode;
use sqlx::sqlite::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::sqlite::SqliteSynchronous;
use tracing::error;
use tracing::info;

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
    sqlx::migrate!("src/db/migrations").run(&pool).await.map_err(|e| {
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
pub fn load_main_window_settings() -> Option<crate::tab_state::WindowSettings> {
    use std::collections::HashMap;

    let db_path = db_path();
    if !db_path.exists() {
        return None;
    }

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
    let pool = rt.block_on(async {
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .read_only(true)
            .busy_timeout(std::time::Duration::from_secs(1));
        SqlitePoolOptions::new().max_connections(1).connect_with(options).await.ok()
    })?;

    let sizes: HashMap<crate::tab_state::WindowKind, crate::tab_state::WindowSettings> =
        rt.block_on(queries::get_setting(&pool, "window_sizes"))?;

    sizes.get(&crate::tab_state::WindowKind::Main).copied()
}

/// Check whether the one-time migration from `app.ron` has already been performed.
pub async fn is_migrated(pool: &SqlitePool) -> bool {
    queries::get_setting::<bool>(pool, "migration_completed").await.unwrap_or(false)
}

/// Mark the one-time migration as completed.
pub async fn set_migrated(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    queries::set_setting(pool, "migration_completed", &true).await
}
