//! Shared minimap frame painting: which commands a set of render options
//! allows, how the canvas is laid out, and the widget that paints one frame.

use wows_minimap_renderer::RenderOptions;
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
