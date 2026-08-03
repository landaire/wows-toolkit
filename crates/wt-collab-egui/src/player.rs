//! Shared minimap frame painting: which commands a set of render options
//! allows, how the canvas is laid out, and the widget that paints one frame.

use std::collections::HashSet;

use egui::Color32;
use egui::CornerRadius;
use egui::Rect;
use egui::Vec2;
use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::STATS_PANEL_WIDTH;
use wows_minimap_renderer::TEAM_ROSTER_WIDTH;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_replays::types::EntityId;
use wt_translations::TextResolver;

use crate::draw_commands::ConsumableHoverRegion;
use crate::draw_commands::DrawCommandLabelOptions;
use crate::draw_commands::DrawCommandTextures;
use crate::draw_commands::PlayerBuildHoverRegion;
use crate::draw_commands::draw_command_to_shapes;
use crate::rendering::draw_grid;
use crate::rendering::draw_map_background;
use crate::transforms::CanvasLayout;
use crate::transforms::MapTransform;
use crate::transforms::compute_canvas_layout;
use crate::transforms::compute_map_clip_rect;
use crate::types::GridStyle;

/// Check whether a DrawCommand should be drawn given the current RenderOptions.
pub fn should_draw_command(cmd: &DrawCommand, opts: &RenderOptions, show_dead_ships: bool) -> bool {
    match cmd {
        DrawCommand::ShotTracer { .. } => opts.show_tracers,
        DrawCommand::ShotTracerTip { .. } => opts.show_tracers,
        DrawCommand::SecondaryShotTracer { .. } => opts.show_tracers,
        DrawCommand::SecondaryShotTracerTip { .. } => opts.show_tracers,
        DrawCommand::Torpedo { .. } => opts.show_torpedoes,
        DrawCommand::Smoke { .. } => opts.show_smoke,
        DrawCommand::Ship { .. } => true, // ships always drawn; name visibility handled below
        DrawCommand::HealthBar { .. } => opts.show_hp_bars,
        DrawCommand::DeadShip { .. } => show_dead_ships,
        DrawCommand::Plane { .. } => opts.show_planes,
        DrawCommand::ScoreBar { .. } => opts.show_score,
        DrawCommand::Timer { .. } => opts.show_timer,
        DrawCommand::PreBattleCountdown { .. } => opts.show_timer,
        DrawCommand::KillFeed { .. } => opts.show_kill_feed && !opts.stats_panel_visible(),
        DrawCommand::CapturePoint { .. } => opts.show_capture_points,
        DrawCommand::Building { .. } => opts.show_buildings,
        DrawCommand::CameraDirection { .. } => opts.show_camera_direction,
        DrawCommand::ConsumableRadius { .. } => opts.show_consumables,
        DrawCommand::PatrolRadius { .. } => opts.show_planes,
        DrawCommand::ConsumableIcons { .. } => opts.show_consumables,
        DrawCommand::PositionTrail { .. } => opts.show_trails || opts.show_speed_trails,
        DrawCommand::ShipConfigCircle { .. } => opts.show_ship_config,
        DrawCommand::BuffZone { .. } => opts.show_capture_points,
        DrawCommand::TeamBuffs { .. } => opts.show_buffs,
        DrawCommand::BattleResultOverlay { .. } => opts.show_battle_result,
        DrawCommand::ChatOverlay { .. } => opts.show_chat && !opts.stats_panel_visible(),
        DrawCommand::TeamAdvantage { .. } => opts.show_advantage,
        DrawCommand::WeatherZone { .. } => opts.show_weather,
        DrawCommand::StatsPanel { .. }
        | DrawCommand::StatsSilhouette { .. }
        | DrawCommand::StatsDamage { .. }
        | DrawCommand::StatsRibbons { .. }
        | DrawCommand::StatsActivityFeed { .. } => opts.show_stats_panel,
        DrawCommand::TeamRoster { .. } => opts.show_team_rosters,
    }
}

/// Canvas widths derived from which side panels are enabled.
///
/// Team rosters reserve a gutter on each side and replace the self-perspective
/// stats panel; the stats panel takes a right-hand strip only when rosters are
/// off. HUD commands are emitted in canvas space starting at x=0, so the score
/// bar spans the gutters but never the stats strip.
pub struct CanvasGeometry {
    pub roster_gutter: f32,
    pub stats_strip: f32,
    /// Distance from the canvas origin to the minimap's left edge.
    pub map_x_offset: f32,
    pub canvas_width: f32,
    pub hud_width: f32,
    /// Width map content is clipped and fill-scaled to, when a panel would
    /// otherwise let zoomed content bleed into it.
    pub map_width: Option<f32>,
}

pub fn canvas_geometry(opts: &RenderOptions) -> CanvasGeometry {
    let rosters = opts.show_team_rosters;
    let stats = opts.stats_panel_visible();
    let roster_gutter = if rosters { TEAM_ROSTER_WIDTH as f32 } else { 0.0 };
    let stats_strip = if stats { STATS_PANEL_WIDTH as f32 } else { 0.0 };
    let hud_width = MINIMAP_SIZE as f32 + roster_gutter * 2.0;
    CanvasGeometry {
        roster_gutter,
        stats_strip,
        map_x_offset: roster_gutter,
        canvas_width: hud_width + stats_strip,
        hud_width,
        map_width: (stats || rosters).then_some(MINIMAP_SIZE as f32),
    }
}

/// Per-ship overrides the caller layers on top of `RenderOptions`.
///
/// The renderer window supplies trail-hidden ships and per-ship range-circle
/// visibility, both of which are driven by right-click state the widget knows
/// nothing about. `alive_ships` is passed so an implementation can drop
/// commands belonging to sunk ships without rescanning the frame.
pub trait CommandFilter {
    fn keep(&self, cmd: &DrawCommand, alive_ships: &HashSet<EntityId>) -> bool;
}

/// Keeps every command `RenderOptions` already allowed.
pub struct NoCommandFilter;

impl CommandFilter for NoCommandFilter {
    fn keep(&self, _cmd: &DrawCommand, _alive_ships: &HashSet<EntityId>) -> bool {
        true
    }
}

/// One frame of minimap content, painted into an allocated region.
///
/// Owns layout, the map background, the grid, command dispatch, and the
/// unzoomed stats/roster pass. It deliberately owns no input handling: zoom
/// and pan arrive as values, and annotations, pings, cursors, tooltips and
/// controls are the caller's, layered on the returned transform.
pub struct MinimapView<'a> {
    pub commands: &'a [DrawCommand],
    pub textures: &'a DrawCommandTextures<'a>,
    pub map_texture: Option<egui::TextureId>,
    pub options: &'a RenderOptions,
    pub show_dead_ships: bool,
    pub zoom: f32,
    pub pan: Vec2,
    pub filter: &'a dyn CommandFilter,
    pub text_resolver: &'a dyn TextResolver,
    /// Builds the grid style once the widget knows its window scale, which the
    /// grid's label font is sized against. `None` draws no grid.
    pub grid: Option<GridStyleFn<'a>>,
    /// Fill behind the map, painted across the whole allocated rect.
    pub background: Color32,
}

/// Builds a grid style for a resolved window scale.
pub type GridStyleFn<'a> = &'a dyn Fn(f32) -> GridStyle;

/// What the caller needs to layer its own content on top of a painted frame.
pub struct MinimapViewOutput {
    pub transform: MapTransform,
    pub consumable_hover_regions: Vec<ConsumableHoverRegion>,
    pub player_build_regions: Vec<PlayerBuildHoverRegion>,
}

impl MinimapView<'_> {
    /// Allocate space, lay out, and paint. For callers with no input to interleave
    /// between layout and paint.
    pub fn show(self, ui: &mut egui::Ui, size: Vec2, sense: egui::Sense) -> (egui::Response, MinimapViewOutput) {
        let ctx = ui.ctx().clone();
        let geom = canvas_geometry(self.options);
        let logical_canvas = Vec2::new(geom.canvas_width, CANVAS_HEIGHT as f32);
        let (response, painter) = ui.allocate_painter(size, sense);
        let layout = compute_canvas_layout(size, logical_canvas, 1.0, response.rect.min, geom.map_width);
        let out = self.show_in(&ctx, &response, &painter, &layout);
        (response, out)
    }

    /// Paint into an already-allocated region.
    ///
    /// The caller allocates and computes the layout so it can run input handling
    /// against `response` and `layout` before painting; the frame then paints with
    /// this frame's input.
    pub fn show_in(
        self,
        ctx: &egui::Context,
        response: &egui::Response,
        painter: &egui::Painter,
        layout: &CanvasLayout,
    ) -> MinimapViewOutput {
        let geom = canvas_geometry(self.options);
        let window_scale = layout.window_scale;

        let transform = MapTransform {
            origin: layout.origin,
            window_scale,
            zoom: self.zoom,
            pan: self.pan,
            hud_height: HUD_HEIGHT as f32,
            canvas_height: CANVAS_HEIGHT as f32,
            canvas_width: geom.canvas_width,
            hud_width: geom.hud_width,
            map_x_offset: geom.map_x_offset,
        };

        painter.rect_filled(response.rect, CornerRadius::ZERO, self.background);

        let map_clip = compute_map_clip_rect(layout, HUD_HEIGHT as f32, geom.map_width, geom.map_x_offset);
        let map_painter = painter.with_clip_rect(map_clip);

        draw_map_background(&map_painter, &transform, self.map_texture);
        if let Some(grid) = self.grid {
            draw_grid(&map_painter, &transform, &grid(window_scale));
        }

        let alive_ships: HashSet<EntityId> = self
            .commands
            .iter()
            .filter_map(|cmd| if let DrawCommand::Ship { entity_id, .. } = cmd { Some(*entity_id) } else { None })
            .collect();

        let label_opts = DrawCommandLabelOptions {
            show_player_names: self.options.show_player_names,
            show_ship_names: self.options.show_ship_names,
            show_dead_ship_names: self.options.show_dead_ship_names,
            show_armament_color: self.options.show_armament,
        };

        let stats_visible = self.options.stats_panel_visible();
        let mut placed_labels: Vec<Rect> = Vec::new();
        for cmd in self.commands {
            // Stats commands ride the separate unzoomed pass below.
            if stats_visible && cmd.is_stats() {
                continue;
            }
            if !should_draw_command(cmd, self.options, self.show_dead_ships) {
                continue;
            }
            if !self.filter.keep(cmd, &alive_ships) {
                continue;
            }
            let is_hud = cmd.is_hud();
            let shapes = draw_command_to_shapes(
                cmd,
                &transform,
                self.textures,
                ctx,
                &label_opts,
                Some(&mut placed_labels),
                self.text_resolver,
                None,
                None,
            );
            let target = if is_hud { painter } else { &map_painter };
            for shape in shapes {
                target.add(shape);
            }
        }

        let mut consumable_hover_regions = Vec::new();
        let mut player_build_regions = Vec::new();
        if stats_visible || self.options.show_team_rosters {
            let stats_transform = MapTransform {
                origin: layout.origin,
                window_scale,
                zoom: 1.0,
                pan: Vec2::ZERO,
                hud_height: HUD_HEIGHT as f32,
                canvas_height: CANVAS_HEIGHT as f32,
                canvas_width: geom.canvas_width,
                hud_width: geom.hud_width,
                map_x_offset: geom.map_x_offset,
            };
            let mut stats_placed = Vec::new();
            for cmd in self.commands {
                if !cmd.is_stats() {
                    continue;
                }
                let shapes = draw_command_to_shapes(
                    cmd,
                    &stats_transform,
                    self.textures,
                    ctx,
                    &label_opts,
                    Some(&mut stats_placed),
                    self.text_resolver,
                    Some(&mut consumable_hover_regions),
                    Some(&mut player_build_regions),
                );
                for shape in shapes {
                    painter.add(shape);
                }
            }
        }

        MinimapViewOutput { transform, consumable_hover_regions, player_build_regions }
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;
    use wows_minimap_renderer::MinimapPos;
    use wows_minimap_renderer::draw_command::RosterSide;
    use wows_replays::types::ElapsedClock;

    fn timer() -> DrawCommand {
        DrawCommand::Timer { time_remaining: Some(600), elapsed: ElapsedClock(42.0) }
    }

    fn torpedo() -> DrawCommand {
        DrawCommand::Torpedo { pos: MinimapPos { x: 50.0, y: 60.0 }, color: [0, 255, 0] }
    }

    fn stats_panel() -> DrawCommand {
        DrawCommand::StatsPanel { x: 768, width: 280 }
    }

    fn team_roster() -> DrawCommand {
        DrawCommand::TeamRoster { side: RosterSide::Friendly, x: 0, y: 0, width: 280, height: 768, rows: Vec::new() }
    }

    #[test]
    fn a_toggled_off_option_drops_its_command() {
        let on = RenderOptions { show_timer: true, ..RenderOptions::default() };
        let off = RenderOptions { show_timer: false, ..RenderOptions::default() };
        assert!(should_draw_command(&timer(), &on, true));
        assert!(!should_draw_command(&timer(), &off, true));
    }

    #[test]
    fn each_option_gates_only_its_own_command() {
        let no_torps = RenderOptions { show_torpedoes: false, ..RenderOptions::default() };
        assert!(!should_draw_command(&torpedo(), &no_torps, true));
        assert!(should_draw_command(&timer(), &no_torps, true), "show_torpedoes must not gate the timer");
    }

    #[test]
    fn the_kill_feed_hides_behind_a_visible_stats_panel() {
        let feed = DrawCommand::KillFeed { entries: Vec::new() };
        let panel = RenderOptions {
            show_kill_feed: true,
            show_stats_panel: true,
            show_team_rosters: false,
            ..RenderOptions::default()
        };
        let no_panel = RenderOptions {
            show_kill_feed: true,
            show_stats_panel: false,
            show_team_rosters: false,
            ..RenderOptions::default()
        };
        assert!(!should_draw_command(&feed, &panel, true));
        assert!(should_draw_command(&feed, &no_panel, true));
    }

    #[test]
    fn rosters_replacing_the_stats_panel_bring_the_kill_feed_back() {
        // `stats_panel_visible` is false when rosters win, so the feed is not
        // suppressed even though `show_stats_panel` is still set.
        let feed = DrawCommand::KillFeed { entries: Vec::new() };
        let both = RenderOptions {
            show_kill_feed: true,
            show_stats_panel: true,
            show_team_rosters: true,
            ..RenderOptions::default()
        };
        assert!(should_draw_command(&feed, &both, true));
    }

    #[test]
    fn panel_commands_follow_their_own_toggles() {
        let opts = RenderOptions { show_stats_panel: true, show_team_rosters: false, ..RenderOptions::default() };
        assert!(should_draw_command(&stats_panel(), &opts, true));
        assert!(!should_draw_command(&team_roster(), &opts, true));
    }

    #[test]
    fn dead_ships_follow_the_separate_flag_not_render_options() {
        let dead = DrawCommand::DeadShip {
            entity_id: wows_replays::types::EntityId::from(1u32),
            pos: MinimapPos { x: 10.0, y: 10.0 },
            yaw: 0.0,
            species: None,
            color: None,
            is_self: false,
            player_name: None,
            ship_name: None,
        };
        let opts = RenderOptions::default();
        assert!(should_draw_command(&dead, &opts, true));
        assert!(!should_draw_command(&dead, &opts, false));
    }

    #[test]
    fn a_trail_survives_if_either_trail_option_is_set() {
        let trail = DrawCommand::PositionTrail {
            entity_id: wows_replays::types::EntityId::from(1u32),
            player_name: Some("Player".to_string()),
            points: Vec::new(),
        };
        let speed_only = RenderOptions { show_trails: false, show_speed_trails: true, ..RenderOptions::default() };
        let neither = RenderOptions { show_trails: false, show_speed_trails: false, ..RenderOptions::default() };
        assert!(should_draw_command(&trail, &speed_only, true));
        assert!(!should_draw_command(&trail, &neither, true));
    }

    #[test]
    fn ships_are_drawn_regardless_of_options() {
        let ship = DrawCommand::Ship {
            entity_id: wows_replays::types::EntityId::from(1u32),
            pos: MinimapPos { x: 10.0, y: 10.0 },
            yaw: 0.0,
            species: None,
            color: None,
            visibility: wows_minimap_renderer::draw_command::ShipVisibility::Visible,
            opacity: 1.0,
            is_self: false,
            player_name: None,
            ship_name: None,
            is_detected_teammate: false,
            is_disconnected: false,
            name_color: None,
        };
        assert!(should_draw_command(&ship, &RenderOptions::default(), false));
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::*;
    use wows_minimap_renderer::MINIMAP_SIZE;
    use wows_minimap_renderer::STATS_PANEL_WIDTH;
    use wows_minimap_renderer::TEAM_ROSTER_WIDTH;

    fn opts(stats: bool, rosters: bool) -> RenderOptions {
        RenderOptions { show_stats_panel: stats, show_team_rosters: rosters, ..RenderOptions::default() }
    }

    #[test]
    fn no_panels_leaves_the_canvas_exactly_the_minimap() {
        let g = canvas_geometry(&opts(false, false));
        assert_eq!(g.roster_gutter, 0.0);
        assert_eq!(g.stats_strip, 0.0);
        assert_eq!(g.map_x_offset, 0.0);
        assert_eq!(g.canvas_width, MINIMAP_SIZE as f32);
        assert_eq!(g.hud_width, MINIMAP_SIZE as f32);
        assert_eq!(g.map_width, None);
    }

    #[test]
    fn the_stats_panel_takes_a_right_strip_the_hud_does_not_span() {
        let g = canvas_geometry(&opts(true, false));
        assert_eq!(g.roster_gutter, 0.0);
        assert_eq!(g.stats_strip, STATS_PANEL_WIDTH as f32);
        assert_eq!(g.map_x_offset, 0.0);
        assert_eq!(g.canvas_width, MINIMAP_SIZE as f32 + STATS_PANEL_WIDTH as f32);
        assert_eq!(g.hud_width, MINIMAP_SIZE as f32);
        assert_eq!(g.map_width, Some(MINIMAP_SIZE as f32));
    }

    #[test]
    fn rosters_take_both_gutters_and_the_hud_spans_all_of_them() {
        let g = canvas_geometry(&opts(false, true));
        assert_eq!(g.roster_gutter, TEAM_ROSTER_WIDTH as f32);
        assert_eq!(g.stats_strip, 0.0);
        assert_eq!(g.map_x_offset, TEAM_ROSTER_WIDTH as f32);
        assert_eq!(g.canvas_width, MINIMAP_SIZE as f32 + TEAM_ROSTER_WIDTH as f32 * 2.0);
        assert_eq!(g.hud_width, g.canvas_width);
        assert_eq!(g.map_width, Some(MINIMAP_SIZE as f32));
    }

    #[test]
    fn rosters_win_over_the_stats_panel_when_both_are_set() {
        let both = canvas_geometry(&opts(true, true));
        let rosters_only = canvas_geometry(&opts(false, true));
        assert_eq!(both.stats_strip, 0.0);
        assert_eq!(both.canvas_width, rosters_only.canvas_width);
        assert_eq!(both.hud_width, rosters_only.hud_width);
    }
}
