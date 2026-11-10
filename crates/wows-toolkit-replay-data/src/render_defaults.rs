//! What a renderer viewport opens showing.
//!
//! Every front end's Save Defaults writes the reader's display choices to the
//! shared `render_options` row (`wows_toolkit_config::queries`), so a default
//! saved in one app is what the others open with next time they start. Each app
//! reads the row once and rewrites the whole of it on save, so with two running
//! at once the last save wins rather than the two merging.

use sqlx::sqlite::SqlitePool;
use wows_minimap_renderer::SavedRenderOptions;

/// Why the defaults could not be saved.
#[derive(Debug, thiserror::Error)]
pub enum RenderDefaultsError {
    #[error("the render options could not be serialized: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("the render options could not be written: {0}")]
    Write(#[from] sqlx::Error),
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

/// Writes `defaults` as what every viewport opens with.
pub async fn save(pool: &SqlitePool, defaults: &SavedRenderOptions) -> Result<(), RenderDefaultsError> {
    let json = serde_json::to_string(defaults)?;
    wows_toolkit_config::queries::save_render_options(pool, &json).await?;

    Ok(())
}
