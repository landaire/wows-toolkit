//! What a renderer viewport opens showing.
//!
//! The egui app's Save Defaults writes the reader's display choices to the
//! shared `render_options` row (`wows_toolkit_config::queries`); this reads that
//! row at startup and writes it back, so a default saved in either app is what
//! the other opens with next time it starts. Both apps read the row once and the
//! egui app rewrites the whole of it from that copy on its periodic save, so with
//! both running at once the last save wins rather than the two merging.

use gpui_kit::App;
use gpui_kit::AsyncApp;
use gpui_kit::Global;
use sqlx::sqlite::SqlitePool;
use wows_minimap_renderer::SavedRenderOptions;

use crate::runtime;

/// The defaults every viewport opened after the startup read uses.
struct Defaults(SavedRenderOptions);

impl Global for Defaults {}

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

/// Adopts what the database holds. Called once, when the startup read lands.
pub fn adopt(defaults: SavedRenderOptions, cx: &mut App) {
    cx.set_global(Defaults(defaults));
}

/// What a viewport should open showing.
///
/// Before the startup read has landed, and in a test with no database, this is
/// the renderer's own defaults.
pub fn defaults(cx: &App) -> SavedRenderOptions {
    cx.try_global::<Defaults>().map(|held| held.0.clone()).unwrap_or_default()
}

/// Remembers `defaults`, for the viewports opened after this one and for the
/// next run of either app.
///
/// Returns whether a write was queued. The write itself is fire and forget, as
/// settings edits are: a failure that late is logged rather than raised.
pub fn remember(defaults: SavedRenderOptions, cx: &mut App) -> bool {
    let json = match serde_json::to_string(&defaults) {
        Ok(json) => json,
        Err(err) => {
            tracing::error!("render defaults: the options could not be serialized: {err}");
            return false;
        }
    };
    cx.set_global(Defaults(defaults));

    let Some(pool) = crate::settings_store::pool(cx) else {
        tracing::warn!("render defaults: nothing was saved because the config database is not open");
        return false;
    };

    cx.spawn(async move |cx: &mut AsyncApp| {
        let written =
            runtime::spawn(cx, async move { wows_toolkit_config::queries::save_render_options(&pool, &json).await })
                .await;

        match written {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::error!("render defaults: the options could not be written: {err}"),
            Err(err) => tracing::error!("render defaults: the write did not complete: {err}"),
        }
    })
    .detach();
    true
}
