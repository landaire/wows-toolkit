//! The inspector hover popup: a looping minimap preview shown above the
//! existing text tooltip when a replay row is hovered.

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use rust_i18n::t;

use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::renderer::RenderOptions;
use wowsunpack::data::Version;
use wowsunpack::vfs::VfsPath;
use wt_collab_egui::draw_commands::DrawCommandTextures;

use crate::LocalizedTextResolver;
use crate::data::wows_data::WoWsDataMap;
use crate::db::index::rows::RowSummary;
use crate::replay::renderer::IconTextures;
use crate::replay::renderer::MINIMAP_BACKGROUND;
use crate::replay::renderer::RendererAssetCache;
use crate::replay::renderer::RendererTextureCache;
use crate::replay::renderer::ReplayRendererAssets;
use crate::replay::renderer::preview::PREVIEW_FPS;
use crate::replay::renderer::preview::PreviewCache;
use crate::replay::renderer::preview::PreviewKey;
use crate::replay::renderer::preview::PreviewState;
use crate::replay::renderer::preview::poll_preview;

use super::listing_row;

/// What a preview draws. At 384x384 the canvas scales to about 0.48x, so ship
/// names render around 5px tall - readable at a glance but only just. Player
/// names and the HUD panels stay off to keep the map uncluttered.
///
/// Tracks are baked with the full command set, so this is a paint-time filter
/// and changing it invalidates nothing.
pub(crate) fn preview_options() -> RenderOptions {
    RenderOptions {
        show_stats_panel: false,
        show_team_rosters: false,
        show_kill_feed: false,
        show_chat: false,
        show_player_names: false,
        show_ship_names: true,
        show_dead_ship_names: false,
        show_ship_config: false,
        show_battle_result: false,
        ..RenderOptions::default()
    }
}

pub(crate) const PREVIEW_SIZE: f32 = 384.0;

/// Everything the popup needs that it cannot reach through a hover closure.
///
/// Every field is owned so this is `'static`: the grouped listing attaches
/// its tooltip inside an `egui_ltreeview` `label_ui` move closure that cannot
/// borrow the tab. Callers hold this behind an `Arc` so cloning it into a
/// per-leaf closure is one refcount bump, not a `WoWsDataMap` deep copy.
pub(crate) struct PreviewDeps {
    pub cache: Arc<Mutex<PreviewCache>>,
    pub asset_cache: Arc<Mutex<RendererAssetCache>>,
    /// The map background is resolved lazily, per hovered row, rather than
    /// for every map name the listing shows: a mature replay library covers
    /// most maps in the game, and eagerly uploading all of them would pay for
    /// VFS reads and textures nobody hovers.
    pub texture_cache: Arc<Mutex<RendererTextureCache>>,
    pub data_map: WoWsDataMap,
    pub icons: Arc<IconTextures>,
    pub vfs: VfsPath,
    pub version: Option<Version>,
}

/// Borrow the shared icon set as the widget's texture bundle.
///
/// The preview has no ship silhouette and no detected-teammate outlines to
/// draw, so those stay `None`.
fn preview_textures(icons: &IconTextures) -> DrawCommandTextures<'_> {
    DrawCommandTextures {
        ship_icons: &icons.ship_icons,
        ship_icon_outlines: None,
        plane_icons: &icons.plane_icons,
        building_icons: Some(&icons.building_icons),
        consumable_icons: Some(&icons.consumable_icons),
        ribbon_icons: Some(&icons.ribbon_icons),
        subribbon_icons: Some(&icons.subribbon_icons),
        death_cause_icons: Some(&icons.death_cause_icons),
        powerup_icons: Some(&icons.powerup_icons),
        silhouette_texture: None,
    }
}

/// `PreviewKey` for `path`, or `None` when no mtime can be found.
///
/// Prefers the indexed row's `RowSummary::file_mtime` when one is in hand:
/// the grouped listing draws every leaf of every open group each frame (not
/// just the visible ones the way `show_rows` bounds the ungrouped listing),
/// so a per-row `fs::metadata` call there would mean hundreds of blocking
/// stats on the UI thread per frame for a large open group. Falls back to a
/// direct file read only for rows that have no summary yet.
pub(crate) fn preview_key(path: &Path, summary: Option<&RowSummary>) -> Option<PreviewKey> {
    let mtime_secs = summary.and_then(|s| s.file_mtime).or_else(|| listing_row::file_mtime_secs(path))?;
    Some(PreviewKey { path: path.to_path_buf(), mtime_secs })
}

/// Assemble the renderer assets a preview bake needs, mirroring the call
/// shape `playback_thread` uses for the same eleven icon sets. `map_image` is
/// left `None`: map backgrounds are resolved separately, per hovered row's
/// map name, by `RendererTextureCache::get_or_upload_map`.
pub(crate) fn build_preview_assets(
    asset_cache: &Arc<Mutex<RendererAssetCache>>,
    vfs: &VfsPath,
    version: Option<&Version>,
    dump_dir: Option<&Path>,
) -> ReplayRendererAssets {
    let mut cache = asset_cache.lock();
    ReplayRendererAssets {
        map_image: None,
        ship_icons: cache.get_or_load_ship_icons(vfs, version, dump_dir),
        plane_icons: cache.get_or_load_plane_icons(vfs, version, dump_dir),
        building_icons: cache.get_or_load_building_icons(vfs, version, dump_dir),
        consumable_icons: cache.get_or_load_consumable_icons(vfs, version, dump_dir),
        ribbon_icons: cache.get_or_load_ribbon_icons(vfs, version, dump_dir),
        subribbon_icons: cache.get_or_load_subribbon_icons(vfs, version, dump_dir),
        death_cause_icons: cache.get_or_load_death_cause_icons(vfs, version, dump_dir),
        powerup_icons: cache.get_or_load_powerup_icons(vfs, version, dump_dir),
        crew_skill_icons: cache.get_or_load_crew_skill_icons(vfs, version, dump_dir),
        modernization_icons: cache.get_or_load_modernization_icons(vfs, version, dump_dir),
        signal_flag_icons: cache.get_or_load_signal_flag_icons(vfs, version, dump_dir),
    }
}

pub(crate) fn preview_tooltip(ui: &mut egui::Ui, deps: &PreviewDeps, key: PreviewKey, map_name: &str, hover: &str) {
    let now = ui.input(|i| i.time);
    let state = poll_preview(&deps.cache, key.clone(), now, &deps.data_map, &deps.asset_cache);

    let options = preview_options();
    // The map is drawn from the first frame the tooltip opens, whether or not
    // a track exists yet: an empty command slice still paints the background.
    let empty: Vec<DrawCommand> = Vec::new();
    let frames: &[Vec<DrawCommand>] = match state {
        PreviewState::Ready(ref track) if !track.frames.is_empty() => &track.frames,
        _ => std::slice::from_ref(&empty),
    };
    // Keyed on wall-clock session, not on absolute time: without this, a row
    // hovered away from and back onto resumes wherever the clock landed,
    // which looks exactly like it kept playing unattended.
    let index = deps.cache.lock().anim_index(&key, now, frames.len());

    // Once a track exists, its own baked name (read straight from the replay
    // file) corrects a stale indexed map name; before that, the row's name is
    // all there is to key the texture lookup with.
    let map_name = match state {
        PreviewState::Ready(ref track) => track.map_name.as_str(),
        _ => map_name,
    };
    let map_texture = {
        let mut tex = deps.texture_cache.lock();
        tex.get_or_upload_map(ui.ctx(), deps.version, map_name, &deps.asset_cache, &deps.vfs)
    };

    let textures = preview_textures(&deps.icons);
    // The preview has no input to interleave, so it uses the allocating
    // `show` wrapper rather than `show_in`. The output is unused: the popup
    // layers nothing on top and has no roster panels to hover.
    let (response, _out) = wt_collab_egui::player::MinimapView {
        commands: &frames[index],
        textures: &textures,
        map_texture: map_texture.map(|t| t.id()),
        options: &options,
        show_dead_ships: true,
        zoom: 1.0,
        pan: egui::Vec2::ZERO,
        filter: &wt_collab_egui::player::NoCommandFilter,
        text_resolver: &LocalizedTextResolver,
        grid: None,
        background: MINIMAP_BACKGROUND,
    }
    .show(ui, egui::Vec2::splat(PREVIEW_SIZE), egui::Sense::hover());

    match state {
        PreviewState::Ready(ref track) if track.frames.is_empty() => {
            ui.label(t!("ui.replay.preview_empty"));
        }
        PreviewState::Ready(_) => {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f32(1.0 / PREVIEW_FPS));
        }
        PreviewState::Baking | PreviewState::Idle => {
            // Centred over the map rather than in the row below it, so the
            // spinner reads as "this map is loading" instead of a stray
            // widget under a static picture. Painted directly rather than
            // via `ui.put`: `put` ends in `advance_cursor_after_rect`, which
            // *assigns* (not maxes) the layout cursor's y to the placed
            // rect's bottom edge. The spinner's rect sits near the map's
            // vertical centre, so that assignment would yank the cursor back
            // up and the loading label plus the hover text below would paint
            // over the map's lower half. `Spinner::paint_at` draws straight
            // to the painter and never touches layout, so the cursor stays
            // exactly where `MinimapView::show`'s own allocation left it:
            // below the map.
            let spinner_rect = egui::Rect::from_center_size(response.rect.center(), egui::Vec2::splat(24.0));
            egui::Spinner::new().paint_at(ui, spinner_rect);
            ui.label(t!("ui.replay.preview_loading"));
            ui.ctx().request_repaint();
        }
        PreviewState::Unavailable(ref err) => {
            ui.label(err.to_string());
        }
    }

    ui.label(hover);
}
