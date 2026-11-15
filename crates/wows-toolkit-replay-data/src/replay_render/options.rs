//! What a playback viewport bakes and draws of a battle.

use std::sync::Arc;

use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::draw_command::ShipConfigVisibility;

use crate::replay_render::export::ExportSettings;

/// What a playback viewport draws when it opens.
///
/// Everything it baked except the layers that are asked for rather than
/// assumed: the rosters take a gutter either side of the map, and the ranges
/// would cover it.
pub fn playback_options_hidden() -> RenderOptions {
    RenderOptions { show_ship_config: false, show_team_rosters: false, ..playback_options() }
}

/// What a playback viewport bakes.
///
/// The hover preview's set plus every ship's range circles. They are per-frame
/// and small, so keeping them costs little, and a viewport that did not bake
/// them could not offer them at all. Trails are still left out: a trail
/// command carries every point so far, so baking one per frame would cost the
/// square of the track's length.
pub fn playback_options() -> RenderOptions {
    RenderOptions {
        show_ship_config: true,
        // The rosters' own commands carry fixed coordinates, so baking them
        // costs a roster per frame and nothing else; the canvas they need is
        // the target's business, not the bake's.
        show_team_rosters: true,
        // Every ship's, so the reader can choose whose to look at afterwards
        // rather than re-baking to change their mind.
        ship_config_visibility: ShipConfigVisibility::Filtered(Arc::new(|_entity| {
            Some(ShipConfigFilter {
                detection: true,
                main_battery: true,
                secondary_battery: true,
                torpedo: true,
                radar: true,
                hydro: true,
            })
        })),
        ..wows_minimap_renderer::frame_track::bake_options()
    }
}

/// What a batch draws of what it baked, and whether dead ships are among it.
///
/// The reader's saved layers, less the stats panel a viewport holds off for the
/// same reason: its ship silhouettes are a load the bake skips, so its gutter
/// would be empty and the kill feed and the chat that share it would go
/// unfilmed.
pub fn batch_options(defaults: &SavedRenderOptions) -> (RenderOptions, bool) {
    let mut options = playback_options();
    defaults.apply_to(&mut options);
    options.show_stats_panel = false;

    (options, defaults.show_dead_ships)
}

/// Applies what the reader last saved as the defaults to a viewport's
/// options and its dead-ship flag.
///
/// The stats panel's ship silhouettes are one of the loads a bake skips, so
/// its gutter has nothing to draw in it and the viewport does not offer the
/// layer at all. A front end applies this once the bake has landed, because
/// the rosters are a wider canvas rather than a layer and there is nothing to
/// widen before then.
pub fn adopt_saved_defaults(saved: &SavedRenderOptions, options: &mut RenderOptions, show_dead_ships: &mut bool) {
    saved.apply_to(options);
    *show_dead_ships = saved.show_dead_ships;
    options.show_stats_panel = false;
}

/// What a viewport showing `options` saves as the defaults, over what is
/// `stored`.
///
/// What this viewport has no control over is left as it was: the self
/// ranges, which are per-ship here rather than one saved filter, and the
/// stats panel, which a bake cannot fill and which this viewport therefore
/// holds off whatever the reader chose in the egui renderer.
pub fn defaults_to_save(
    stored: &SavedRenderOptions,
    options: &RenderOptions,
    show_dead_ships: bool,
    export: &ExportSettings,
) -> SavedRenderOptions {
    let mut saved = stored.clone();
    saved.read_from(options);
    saved.show_stats_panel = stored.show_stats_panel;
    saved.show_dead_ships = show_dead_ships;
    saved.prefer_cpu_encoder = export.prefer_cpu;
    saved.video_codec = export.codec;
    saved.include_pre_battle = export.include_pre_battle;

    saved
}
