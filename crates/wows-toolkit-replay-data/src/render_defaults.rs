//! What a renderer viewport opens showing.
//!
//! The egui app's Save Defaults writes the reader's display choices to the
//! shared `render_options` row (`wows_toolkit_config::queries`); this reads that
//! row at startup and writes it back, so a default saved in either app is what
//! the other opens with next time it starts. Both apps read the row once and the
//! egui app rewrites the whole of it from that copy on its periodic save, so with
//! both running at once the last save wins rather than the two merging.
//!
//! Both functions are futures for the tokio runtime the store runs on.

use sqlx::sqlite::SqlitePool;
use wows_minimap_renderer::SavedRenderOptions;

/// Why the defaults were not saved.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("the options could not be serialized: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("the options could not be written: {0}")]
    Write(#[source] sqlx::Error),
}

/// Reads the stored defaults.
///
/// A row that is absent has never been written, and one that cannot be read is
/// worth no more than that: either way a viewport opens with what the renderer
/// itself defaults to.
pub async fn load(pool: &SqlitePool) -> SavedRenderOptions {
    let stored = match wows_toolkit_config::queries::get_render_options(pool).await {
        Ok(stored) => stored,
        Err(err) => {
            tracing::warn!("render defaults: the stored options could not be read: {err}");
            return SavedRenderOptions::default();
        }
    };

    let Some(json) = stored else { return SavedRenderOptions::default() };
    match serde_json::from_str(&json) {
        Ok(defaults) => defaults,
        Err(err) => {
            tracing::warn!("render defaults: the stored options could not be parsed: {err}");
            SavedRenderOptions::default()
        }
    }
}

/// The row [`save`] writes for `defaults`. Serialized apart from the write so
/// a front end can refuse a value that will not serialize before queueing it.
pub fn serialize(defaults: &SavedRenderOptions) -> Result<String, SaveError> {
    serde_json::to_string(defaults).map_err(SaveError::Serialize)
}

/// Writes a row [`serialize`] produced as the stored defaults.
pub async fn save(pool: &SqlitePool, json: &str) -> Result<(), SaveError> {
    wows_toolkit_config::queries::save_render_options(pool, json).await.map_err(SaveError::Write)
}
