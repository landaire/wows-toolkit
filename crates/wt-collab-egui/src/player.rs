//! Shared minimap frame painting: which commands a set of render options
//! allows, how the canvas is laid out, and the widget that paints one frame.

use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::STATS_PANEL_WIDTH;
use wows_minimap_renderer::TEAM_ROSTER_WIDTH;
use wows_minimap_renderer::draw_command::DrawCommand;

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
