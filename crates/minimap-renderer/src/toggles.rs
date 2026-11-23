//! The layer switches a playback viewer offers over [`RenderOptions`].
//!
//! Beside the options rather than in a front end, so every viewer (the
//! desktop front ends and the web demo) offers the same switches with the same
//! meaning.

use crate::config::RenderOptions;

/// One toggle in the settings popover: what it says, what it reads, and what
/// it sets. `read` and `write` take the dead-ship flag beside the options.
pub struct Toggle {
    pub id: &'static str,
    pub label_key: &'static str,
    pub read: fn(&RenderOptions, bool) -> bool,
    pub write: fn(&mut RenderOptions, &mut bool, bool),
}

/// What the viewport can be told to draw.
///
/// Only the commands the track actually holds. A playback bake adds the team
/// rosters, the stats panel and every ship's range circles to `bake_options`.
/// Position trails are derived at raster time rather than baked.
pub const TOGGLES: &[Toggle] = &[
    Toggle {
        id: "renderer-opt-hp-bars",
        label_key: "ui-renderer-settings-hp_bars",
        read: |o, _| o.show_hp_bars,
        write: |o, _, v| o.show_hp_bars = v,
    },
    Toggle {
        id: "renderer-opt-tracers",
        label_key: "ui-renderer-settings-tracers",
        read: |o, _| o.show_tracers,
        write: |o, _, v| o.show_tracers = v,
    },
    Toggle {
        id: "renderer-opt-torpedoes",
        label_key: "ui-renderer-settings-torpedoes",
        read: |o, _| o.show_torpedoes,
        write: |o, _, v| o.show_torpedoes = v,
    },
    Toggle {
        id: "renderer-opt-planes",
        label_key: "ui-renderer-settings-planes",
        read: |o, _| o.show_planes,
        write: |o, _, v| o.show_planes = v,
    },
    Toggle {
        id: "renderer-opt-smoke",
        label_key: "ui-renderer-settings-smoke",
        read: |o, _| o.show_smoke,
        write: |o, _, v| o.show_smoke = v,
    },
    Toggle {
        id: "renderer-opt-consumables",
        label_key: "ui-renderer-settings-consumables",
        read: |o, _| o.show_consumables,
        write: |o, _, v| o.show_consumables = v,
    },
    Toggle {
        id: "renderer-opt-capture-points",
        label_key: "ui-renderer-settings-capture_points",
        read: |o, _| o.show_capture_points,
        write: |o, _, v| o.show_capture_points = v,
    },
    Toggle {
        id: "renderer-opt-buildings",
        label_key: "ui-renderer-settings-buildings",
        read: |o, _| o.show_buildings,
        write: |o, _, v| o.show_buildings = v,
    },
    Toggle {
        id: "renderer-opt-player-names",
        label_key: "ui-renderer-settings-player_names",
        read: |o, _| o.show_player_names,
        write: |o, _, v| o.show_player_names = v,
    },
    Toggle {
        id: "renderer-opt-ship-names",
        label_key: "ui-renderer-settings-ship_names",
        read: |o, _| o.show_ship_names,
        write: |o, _, v| o.show_ship_names = v,
    },
    Toggle {
        id: "renderer-opt-dead-ships",
        label_key: "ui-renderer-settings-dead_ships",
        read: |_, dead| dead,
        write: |_, dead, v| *dead = v,
    },
    Toggle {
        id: "renderer-opt-dead-ship-names",
        label_key: "ui-renderer-settings-dead_ship_names",
        read: |o, _| o.show_dead_ship_names,
        write: |o, _, v| o.show_dead_ship_names = v,
    },
    Toggle {
        id: "renderer-opt-camera-direction",
        label_key: "ui-renderer-settings-camera_direction",
        read: |o, _| o.show_camera_direction,
        write: |o, _, v| o.show_camera_direction = v,
    },
    Toggle {
        id: "renderer-opt-armament",
        label_key: "ui-renderer-settings-armament",
        read: |o, _| o.show_armament,
        write: |o, _, v| o.show_armament = v,
    },
    Toggle {
        id: "renderer-opt-score",
        label_key: "ui-renderer-settings-score_label",
        read: |o, _| o.show_score,
        write: |o, _, v| o.show_score = v,
    },
    Toggle {
        id: "renderer-opt-timer",
        label_key: "ui-renderer-settings-timer",
        read: |o, _| o.show_timer,
        write: |o, _, v| o.show_timer = v,
    },
    Toggle {
        id: "renderer-opt-kill-feed",
        label_key: "ui-renderer-settings-kill_feed",
        read: |o, _| o.show_kill_feed,
        write: |o, _, v| o.show_kill_feed = v,
    },
    Toggle {
        id: "renderer-opt-chat",
        label_key: "ui-renderer-settings-chat_label",
        read: |o, _| o.show_chat,
        write: |o, _, v| o.show_chat = v,
    },
    Toggle {
        id: "renderer-opt-battle-result",
        label_key: "ui-renderer-settings-battle_result",
        read: |o, _| o.show_battle_result,
        write: |o, _, v| o.show_battle_result = v,
    },
    Toggle {
        id: "renderer-opt-buffs",
        label_key: "ui-renderer-settings-buff_counters",
        read: |o, _| o.show_buffs,
        write: |o, _, v| o.show_buffs = v,
    },
    Toggle {
        id: "renderer-opt-trails",
        label_key: "ui-renderer-settings-heat_trail",
        read: |o, _| o.show_trails,
        write: |o, _, v| o.show_trails = v,
    },
    Toggle {
        id: "renderer-opt-speed-trails",
        label_key: "ui-renderer-settings-speed_trails",
        read: |o, _| o.show_speed_trails,
        write: |o, _, v| o.show_speed_trails = v,
    },
    Toggle {
        id: "renderer-opt-dead-trails",
        label_key: "ui-renderer-settings-dead_ship_trails",
        read: |o, _| o.show_dead_trails,
        write: |o, _, v| o.show_dead_trails = v,
    },
    Toggle {
        id: "renderer-opt-ship-ranges",
        label_key: "ui-renderer-settings-ship_ranges",
        read: |o, _| o.show_ship_config,
        write: |o, _, v| o.show_ship_config = v,
    },
    Toggle {
        id: "renderer-opt-advantage",
        label_key: "ui-renderer-settings-team_advantage",
        read: |o, _| o.show_advantage,
        write: |o, _, v| o.show_advantage = v,
    },
    Toggle {
        id: "renderer-opt-score-timers",
        label_key: "ui-renderer-settings-score_timers",
        read: |o, _| o.show_score_timer,
        write: |o, _, v| o.show_score_timer = v,
    },
    Toggle {
        id: "renderer-opt-team-rosters",
        label_key: "ui-renderer-settings-team_rosters",
        read: |o, _| o.show_team_rosters,
        // The rosters sit in gutters either side of the map, which the
        // stats panel wants for itself, so turning one on clears the other
        // as the egui checkboxes do.
        write: |o, _, v| {
            o.show_team_rosters = v;
            if v {
                o.show_stats_panel = false;
            }
        },
    },
    Toggle {
        id: "renderer-opt-stats-panel",
        label_key: "ui-renderer-settings-stats_panel",
        read: |o, _| o.show_stats_panel,
        // Shares the rosters' gutter, as the egui checkboxes do.
        write: |o, _, v| {
            o.show_stats_panel = v;
            if v {
                o.show_team_rosters = false;
            }
        },
    },
];
