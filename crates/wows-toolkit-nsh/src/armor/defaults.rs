//! The persisted viewer defaults (`armor_viewer_defaults`) and export
//! defaults (`model_export_defaults`) in the shared settings database.

use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::mpsc;

use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_viewmodel::armor::export::ExportDefaults;

use super::error::ArmorError;

/// Runs `work` against the settings database on a runtime of its own.
/// Blocking.
fn with_database<T>(
    work: impl AsyncFnOnce(&sqlx::SqlitePool) -> Result<T, ArmorError>,
) -> Result<T, ArmorError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| ArmorError::Runtime { source })?;
    let pool = runtime.block_on(wows_toolkit_config::open_db()).map_err(|source| ArmorError::Database { source })?;
    let result = runtime.block_on(work(&pool));
    runtime.block_on(pool.close());

    result
}

/// The saved row, `None` before anything was saved. Blocking.
pub(super) fn load_viewer_defaults() -> Result<Option<ArmorViewerDefaultsRow>, ArmorError> {
    with_database(async |pool| {
        wows_toolkit_config::queries::get_armor_viewer_defaults(pool)
            .await
            .map_err(|source| ArmorError::Database { source })
    })
}

/// What the last export asked for. Blocking.
pub(super) fn load_export_defaults() -> Result<ExportDefaults, ArmorError> {
    with_database(async |pool| Ok(ExportDefaults::load(pool).await))
}

/// Remembers what an export asked for. Blocking.
pub(super) fn save_export_defaults(defaults: ExportDefaults) -> Result<(), ArmorError> {
    with_database(async |pool| defaults.save(pool).await.map_err(|source| ArmorError::Database { source }))
}

/// Queues `row` to be written. Rows are written in order on one thread, and a
/// row superseded before its turn is skipped, so a slider drag writes the row
/// it ends on rather than every row on the way.
pub(super) fn save_viewer_defaults(row: ArmorViewerDefaultsRow) {
    static WRITER: LazyLock<Mutex<Option<mpsc::Sender<ArmorViewerDefaultsRow>>>> =
        LazyLock::new(|| Mutex::new(spawn_writer()));

    let writer = WRITER.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(sender) = writer.as_ref() else {
        tracing::warn!("armor viewer: no defaults writer; the defaults were not saved");
        return;
    };
    if sender.send(row).is_err() {
        tracing::warn!("armor viewer: the defaults writer stopped; the defaults were not saved");
    }
}

fn spawn_writer() -> Option<mpsc::Sender<ArmorViewerDefaultsRow>> {
    let (sender, rows) = mpsc::channel::<ArmorViewerDefaultsRow>();
    let spawned = std::thread::Builder::new().name("armor-defaults".to_owned()).spawn(move || {
        while let Ok(first) = rows.recv() {
            let latest = rows.try_iter().last().unwrap_or(first);
            let written = with_database(async |pool| {
                wows_toolkit_config::queries::save_armor_viewer_defaults(pool, &latest)
                    .await
                    .map_err(|source| ArmorError::Database { source })
            });
            if let Err(error) = written {
                tracing::warn!("armor viewer: the defaults could not be written: {error}");
            }
        }
    });
    match spawned {
        Ok(_) => Some(sender),
        Err(error) => {
            tracing::warn!("armor viewer: no thread for the defaults writer: {error}");
            None
        }
    }
}
