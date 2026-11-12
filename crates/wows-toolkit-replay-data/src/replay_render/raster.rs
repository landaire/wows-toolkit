//! Which of a baked frame's commands are drawn, and the canvas they are drawn
//! on.

use std::path::Path;

use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::config::should_draw_command;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::drawing::SidePanelLayout;
use wows_minimap_renderer::frame_track::trails_through;

use crate::load::GameDataCache;
use crate::minimap_preview::SharedPreviewRenderer;
use crate::minimap_preview::layout_for;
use crate::minimap_preview::renderer_for_replay;
use crate::replay_render::ships::ShipChoices;
use crate::replay_render::track::FrameIndex;
use crate::replay_render::track::Track;

/// What following the options onto the canvas they need came to.
pub enum LayoutChange {
    /// The canvas already suits them.
    Unchanged,
    /// A different canvas, and the renderer that draws on it. The frame on
    /// screen was drawn on the old canvas, so it is the wrong shape until the
    /// next one is drawn, and the track's map origin moves to `map_origin`.
    Moved { layout: SidePanelLayout, renderer: SharedPreviewRenderer, map_origin: (f32, f32) },
    /// No canvas could be built for the gutter layers, so they were turned
    /// off rather than drawn on a canvas with no room for them.
    Refused,
}

/// The commands frame `at` is rasterised from.
///
/// Filtered here rather than at bake time: a toggle then costs one frame
/// rather than another walk of the battle. Trails are derived from the track
/// so far and go first, so they sit behind everything, as the egui renderer
/// draws them. `drawn_on` is what is drawn over the battle rather than by it
/// (annotations, placed ranges): it is not filtered and goes on after the
/// battle's own map layers, so a drawn line is not buried under a ship.
/// `None` for a frame past the end of the track.
pub fn raster_commands(
    track: &Track,
    at: FrameIndex,
    options: &RenderOptions,
    show_dead_ships: bool,
    choices: &ShipChoices,
    drawn_on: Vec<DrawCommand>,
) -> Option<Vec<DrawCommand>> {
    let commands = track.commands(at)?;
    let trails = if options.show_trails { trails_through(&track.frames[..=at.get()]) } else { Vec::new() };

    let mut shown: Vec<DrawCommand> = trails
        .into_iter()
        .chain(commands.iter().cloned())
        .filter(|command| should_draw_command(command, options, show_dead_ships))
        .filter(|command| choices.allows(command))
        .collect();
    shown.extend(drawn_on);

    Some(shown)
}

/// Moves a viewport onto the canvas `options` now need. Blocks while a new
/// renderer is built.
///
/// The rosters sit in gutters either side of the map, so turning them on is
/// not a layer but a wider canvas. The renderer for one is kept, so switching
/// back and forth costs nothing after the first time.
pub fn follow_layout(
    path: &Path,
    game_data: &GameDataCache,
    options: &mut RenderOptions,
    current: SidePanelLayout,
) -> LayoutChange {
    let wanted = layout_for(options);
    if wanted == current {
        return LayoutChange::Unchanged;
    }

    match renderer_for_replay(path, game_data, wanted) {
        Ok((renderer, origin)) => {
            LayoutChange::Moved { layout: wanted, renderer, map_origin: (origin.0 as f32, origin.1 as f32) }
        }
        Err(reason) => {
            tracing::warn!("replay renderer: the canvas for {wanted:?} could not be built: {reason}");
            options.show_team_rosters = false;
            options.show_stats_panel = false;
            LayoutChange::Refused
        }
    }
}
