//! What a renderer viewport opens showing: the shared `render_options` row
//! (`wows_toolkit_replay_data::render_defaults`), held as a global once the
//! startup read lands.

use gpui_kit::App;
use gpui_kit::AsyncApp;
use gpui_kit::Global;
use wows_minimap_renderer::SavedRenderOptions;

use crate::runtime;

/// The defaults every viewport opened after the startup read uses.
struct Defaults(SavedRenderOptions);

impl Global for Defaults {}

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
    cx.set_global(Defaults(defaults.clone()));

    let Some(pool) = crate::settings_store::pool(cx) else {
        tracing::warn!("render defaults: nothing was saved because the config database is not open");
        return false;
    };

    cx.spawn(async move |cx: &mut AsyncApp| {
        let written =
            runtime::spawn(cx, async move { wows_toolkit_replay_data::render_defaults::save(&pool, &defaults).await })
                .await;

        match written {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::error!("render defaults: {err}"),
            Err(err) => tracing::error!("render defaults: the write did not complete: {err}"),
        }
    })
    .detach();
    true
}
