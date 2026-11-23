//! Which of a baked frame's commands are drawn, and the canvas they are drawn
//! on.

use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::config::hide_command_parts;
use wows_minimap_renderer::config::should_draw_command;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::drawing::SidePanelLayout;
use wows_replays::types::GameClock;

use crate::load::GameDataCache;
use crate::minimap_preview::PreviewError;
use crate::minimap_preview::SharedPreviewRenderer;
use crate::minimap_preview::layout_for;
use crate::minimap_preview::renderer_for_replay;
use crate::replay_render::motion::FrameMotion;
use crate::replay_render::motion::reach_ships;
use crate::replay_render::ships::ShipChoices;
use crate::replay_render::track::Track;

/// A canvas built for one side-panel layout: the renderers that draw on it,
/// and where the map's corner sits on it.
pub struct Canvas {
    pub layout: SidePanelLayout,
    pub renderers: Vec<SharedPreviewRenderer>,
    pub map_origin: (f32, f32),
}

/// The most frames a viewport rasterises at once. One frame of a large window at a high pixel
/// density takes a single core more than two frame intervals of playback.
const MOST_DRAWS_AT_ONCE: usize = 3;

/// `renderer` and copies of it, one for each frame a viewport may rasterise at once, so a playing
/// battle is drawn at its frame rate even where one frame takes longer than a frame interval.
///
/// At most half the machine's cores, and always `renderer` itself first.
pub fn renderers_for_playback(renderer: SharedPreviewRenderer) -> Vec<SharedPreviewRenderer> {
    // One core is the safe reading of a machine that will not say how many it has.
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let count = (cores / 2).clamp(1, MOST_DRAWS_AT_ONCE);
    let copies: Vec<SharedPreviewRenderer> = {
        let held = renderer.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        (1..count).map(|_| Arc::new(Mutex::new(held.clone()))).collect()
    };

    std::iter::once(renderer).chain(copies).collect()
}

/// The commands drawn at `clock`, between `motion`'s frame and the next.
///
/// Filtered here rather than at bake time: a toggle then costs one frame
/// rather than another walk of the battle. Trails are read from the track's
/// recorded positions up to the clock, end at their ships and go first, so
/// they sit behind everything. `drawn_on` is
/// what is drawn over the battle rather than by it (annotations, placed
/// ranges): it is not filtered and goes on after the battle's own map layers,
/// so a drawn line is not buried under a ship.
pub fn raster_commands(
    track: &Track,
    motion: &FrameMotion,
    clock: GameClock,
    options: &RenderOptions,
    show_dead_ships: bool,
    choices: &ShipChoices,
    drawn_on: Vec<DrawCommand>,
) -> Vec<DrawCommand> {
    let keep =
        |command: &DrawCommand| should_draw_command(command, options, show_dead_ships) && choices.allows(command);
    let frame: Vec<DrawCommand> = motion
        .commands_at(track, clock, keep)
        .map(|mut command| {
            hide_command_parts(&mut command, options);
            command
        })
        .collect();
    let mut trails = track.trails.trails_at(clock, options);
    reach_ships(&mut trails, &frame);

    let mut shown = shown_commands(trails.into_iter(), options, show_dead_ships, choices);
    shown.reserve(frame.len() + drawn_on.len());
    shown.extend(frame);
    shown.extend(drawn_on);

    shown
}

/// What of `commands` the layer switches and the per-ship choices let through, with the parts
/// they hide cleared.
pub fn shown_commands(
    commands: impl Iterator<Item = DrawCommand>,
    options: &RenderOptions,
    show_dead_ships: bool,
    choices: &ShipChoices,
) -> Vec<DrawCommand> {
    commands
        .filter(|command| should_draw_command(command, options, show_dead_ships))
        .filter(|command| choices.allows(command))
        .map(|mut command| {
            hide_command_parts(&mut command, options);
            command
        })
        .collect()
}

/// Builds the canvas `layout` needs for the replay at `path`. Blocks while the
/// replay, the map and the fonts are read, so it runs off the UI thread.
///
/// The rosters sit in gutters either side of the map, so turning them on is
/// not a layer but a wider canvas. The renderer for one is kept, so switching
/// back and forth costs little after the first time.
pub fn build_canvas(path: &Path, game_data: &GameDataCache, layout: SidePanelLayout) -> Result<Canvas, PreviewError> {
    let (renderer, origin) = renderer_for_replay(path, game_data, layout)?;
    Ok(Canvas { layout, renderers: renderers_for_playback(renderer), map_origin: (origin.0 as f32, origin.1 as f32) })
}

/// `options` as a canvas laid out for `layout` can draw them: the gutter
/// layers it has no room for are off. Borrowed when they already fit, which
/// is every draw but those while a wider canvas is being built.
pub fn fit_to_canvas(options: &RenderOptions, layout: SidePanelLayout) -> Cow<'_, RenderOptions> {
    if layout_for(options) == layout {
        return Cow::Borrowed(options);
    }
    let mut fitted = options.clone();
    match layout {
        SidePanelLayout::None => {
            fitted.show_team_rosters = false;
            fitted.show_stats_panel = false;
        }
        SidePanelLayout::StatsPanel => {
            fitted.show_team_rosters = false;
            fitted.show_stats_panel = true;
        }
        SidePanelLayout::TeamRosters => fitted.show_team_rosters = true,
    }
    Cow::Owned(fitted)
}
