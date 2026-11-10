//! Runs `wows_toolkit_armor::load_ship` loads on the background executor and
//! reports finished GLB exports.

use std::sync::Arc;

use gpui_kit::App;
use gpui_kit::AppContext;
use gpui_kit::Task;
use rust_i18n::t;
use wows_toolkit_armor::load_ship::ExportGlbError;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::ShipLoadError;
use wows_toolkit_armor::load_ship::ShipLoadOptions;
use wows_toolkit_armor::load_ship::load_ship_armor_by_param;
use wows_toolkit_armor::load_ship::load_ship_armor_with_options;

/// Kicks off a ship's armor load on the background executor. `bundle` is the
/// Armor Viewer's already-loaded [`ArmorAssetsBundle`](super::assets::ArmorAssetsBundle)
/// (shared `Arc`, cheap to clone into the background task); this never
/// triggers a second game-data or `ShipAssets` load.
pub fn spawn_load_ship_armor(
    bundle: Arc<super::assets::ArmorAssetsBundle>,
    param_index: String,
    display_name: String,
    cx: &App,
) -> Task<Result<LoadedShipArmor, ShipLoadError>> {
    cx.background_spawn(async move { load_ship_armor_by_param(&bundle.assets, &param_index, &display_name) })
}

/// Kicks off a background reload of `param_index`'s armor under `options` --
/// a hull-upgrade/LOD/module selection change from the hull popover
/// (`popover.rs`), driven by `viewport_view::ViewportView::reload_ship`. Like
/// [`spawn_load_ship_armor`], but the caller controls `options` (build it via
/// `build_reload_options`) instead of getting the always-default load;
/// `bundle` is the same shared `ArmorAssetsBundle`, so this never triggers a
/// second game-data or `ShipAssets` load either.
pub fn spawn_reload_ship_armor(
    bundle: Arc<super::assets::ArmorAssetsBundle>,
    param_index: String,
    options: ShipLoadOptions,
    cx: &App,
) -> Task<Result<LoadedShipArmor, ShipLoadError>> {
    cx.background_spawn(async move { load_ship_armor_with_options(&bundle.assets, &param_index, options) })
}

/// Reports a finished export: what was written and how large it is, or why it
/// was not.
///
/// Shared by the two surfaces that export the same file (`ArmorViewerPane` and
/// the viewport's own confirmation), since a written file is otherwise invisible
/// and both say it in the egui app's words. The destination is logged rather
/// than shown: the reader chose it in the save dialog, and a support log is
/// where it is asked for afterwards.
pub(crate) fn report_export(
    written: Result<(std::path::PathBuf, Option<u64>), ExportGlbError>,
    ship: &str,
    window: &mut gpui_kit::Window,
    cx: &mut App,
) {
    match written {
        Ok((path, size)) => {
            tracing::info!("armor viewer: exported {ship} to {}", path.display());
            let message = match size {
                Some(bytes) => t!(
                    "ui.armor.export.exported_with_size",
                    ship = ship,
                    size = wows_toolkit_viewmodel::formatting::byte_size(bytes)
                )
                .into_owned(),
                None => t!("ui.armor.export.exported", ship = ship).into_owned(),
            };
            crate::toast::ok(message, window, cx);
        }
        Err(err) => {
            tracing::error!("armor viewer: failed to export {ship}: {err}");
            crate::toast::failed(t!("ui.armor.export.export_failed", error = err.to_string()).into_owned(), window, cx);
        }
    }
}
