//! What a playback viewport bakes and draws of a battle.

use std::sync::Arc;

use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::draw_command::ShipConfigVisibility;

use crate::replay_render::export::ExportSettings;
use crate::replay_render::ships::ShipChoices;

/// What a playback viewport draws when it opens.
///
/// Everything it baked except the layers that are asked for rather than
/// assumed: the rosters and the stats panel take a gutter beside the map, and
/// the ranges would cover it.
pub fn playback_options_hidden() -> RenderOptions {
    RenderOptions { show_ship_config: false, show_team_rosters: false, show_stats_panel: false, ..playback_options() }
}

/// What a playback viewport bakes.
///
/// The hover preview's set plus every ship's range circles, the rosters and
/// the stats panel. They are per-frame and small, so keeping them costs
/// little, and a viewport that did not bake them could not offer them at all.
/// Trail commands are still left out: one carries every point so far, so
/// baking one per frame would cost the square of the track's length. The bake
/// records the positions they are drawn from instead.
pub fn playback_options() -> RenderOptions {
    RenderOptions {
        show_ship_config: true,
        // The panels' own commands carry fixed coordinates, so baking them
        // costs a roster and a panel per frame and nothing else; the canvas
        // they need is the target's business, not the bake's. Both are baked
        // so the reader can switch between them without another walk.
        show_team_rosters: true,
        show_stats_panel: true,
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

/// What a batch draws of what it baked, and whether dead ships are among it:
/// the reader's saved layers.
pub fn batch_options(defaults: &SavedRenderOptions) -> (RenderOptions, bool) {
    let mut options = playback_options();
    defaults.apply_to(&mut options);

    (options, defaults.show_dead_ships)
}

/// Applies what the reader last saved as the defaults to a viewport's
/// options and its dead-ship flag.
///
/// A front end applies this once the bake has landed, because the rosters and
/// the stats panel are a wider canvas rather than a layer and there is nothing
/// to widen before then.
pub fn adopt_saved_defaults(saved: &SavedRenderOptions, options: &mut RenderOptions, show_dead_ships: &mut bool) {
    saved.apply_to(options);
    *show_dead_ships = saved.show_dead_ships;
}

/// Switches a merged replay's viewport from the stats panel to the team rosters, as egui opens
/// one: the rosters are what the alt perspectives add. The saved defaults are not changed.
pub fn open_merged_on_rosters(options: &mut RenderOptions) {
    options.show_stats_panel = false;
    options.show_team_rosters = true;
}

/// Puts the saved self ranges on the recording player's ship, `self_player`, which is where the
/// egui renderer's saved self range filter applies. Nothing changes for a recording with no ship
/// of its own.
pub fn adopt_saved_self_ranges(
    saved: &SavedRenderOptions,
    self_player: Option<&str>,
    choices: &mut ShipChoices,
    options: &mut RenderOptions,
) {
    let Some(player) = self_player else { return };
    let filter = saved.self_range_filter();
    if !filter.any_enabled() {
        return;
    }

    choices.set_ranges_for(player, |ranges| *ranges = filter, options);
}

/// What a viewport showing `options` saves as the defaults, over what is
/// `stored`.
///
/// `self_ranges` are the ranges the recording player's ship shows, saved as
/// the self range filter; `None` for a recording with no ship of its own,
/// which leaves the stored filter as it was.
pub fn defaults_to_save(
    stored: &SavedRenderOptions,
    options: &RenderOptions,
    show_dead_ships: bool,
    export: &ExportSettings,
    self_ranges: Option<ShipConfigFilter>,
) -> SavedRenderOptions {
    let mut saved = stored.clone();
    saved.read_from(options);
    saved.show_dead_ships = show_dead_ships;
    saved.prefer_cpu_encoder = export.prefer_cpu;
    saved.video_codec = export.codec;
    saved.include_pre_battle = export.include_pre_battle;
    if let Some(ranges) = self_ranges {
        saved.set_self_range_filter(&ranges);
    }

    saved
}
