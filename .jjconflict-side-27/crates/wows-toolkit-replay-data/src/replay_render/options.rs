//! What a playback viewport bakes and draws of a battle.

use std::sync::Arc;

use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::draw_command::ShipConfigVisibility;

use crate::replay_render::export::ExportSettings;

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
/// rosters and every ship's range circles to `bake_options`; the stats panel
/// is still left out, because its silhouettes are one of the loads a bake
/// skips to stay cheap, so a switch for it would draw an empty gutter and is
/// not offered. Position trails are derived at raster time rather than baked.
pub const TOGGLES: &[Toggle] = &[
    Toggle {
        id: "renderer-opt-hp-bars",
        label_key: "ui.renderer.settings.hp_bars",
        read: |o, _| o.show_hp_bars,
        write: |o, _, v| o.show_hp_bars = v,
    },
    Toggle {
        id: "renderer-opt-tracers",
        label_key: "ui.renderer.settings.tracers",
        read: |o, _| o.show_tracers,
        write: |o, _, v| o.show_tracers = v,
    },
    Toggle {
        id: "renderer-opt-torpedoes",
        label_key: "ui.renderer.settings.torpedoes",
        read: |o, _| o.show_torpedoes,
        write: |o, _, v| o.show_torpedoes = v,
    },
    Toggle {
        id: "renderer-opt-planes",
        label_key: "ui.renderer.settings.planes",
        read: |o, _| o.show_planes,
        write: |o, _, v| o.show_planes = v,
    },
    Toggle {
        id: "renderer-opt-smoke",
        label_key: "ui.renderer.settings.smoke",
        read: |o, _| o.show_smoke,
        write: |o, _, v| o.show_smoke = v,
    },
    Toggle {
        id: "renderer-opt-consumables",
        label_key: "ui.renderer.settings.consumables",
        read: |o, _| o.show_consumables,
        write: |o, _, v| o.show_consumables = v,
    },
    Toggle {
        id: "renderer-opt-capture-points",
        label_key: "ui.renderer.settings.capture_points",
        read: |o, _| o.show_capture_points,
        write: |o, _, v| o.show_capture_points = v,
    },
    Toggle {
        id: "renderer-opt-buildings",
        label_key: "ui.renderer.settings.buildings",
        read: |o, _| o.show_buildings,
        write: |o, _, v| o.show_buildings = v,
    },
    Toggle {
        id: "renderer-opt-player-names",
        label_key: "ui.renderer.settings.player_names",
        read: |o, _| o.show_player_names,
        write: |o, _, v| o.show_player_names = v,
    },
    Toggle {
        id: "renderer-opt-ship-names",
        label_key: "ui.renderer.settings.ship_names",
        read: |o, _| o.show_ship_names,
        write: |o, _, v| o.show_ship_names = v,
    },
    Toggle {
        id: "renderer-opt-dead-ships",
        label_key: "ui.renderer.settings.dead_ships",
        read: |_, dead| dead,
        write: |_, dead, v| *dead = v,
    },
    Toggle {
        id: "renderer-opt-dead-ship-names",
        label_key: "ui.renderer.settings.dead_ship_names",
        read: |o, _| o.show_dead_ship_names,
        write: |o, _, v| o.show_dead_ship_names = v,
    },
    Toggle {
        id: "renderer-opt-camera-direction",
        label_key: "ui.renderer.settings.camera_direction",
        read: |o, _| o.show_camera_direction,
        write: |o, _, v| o.show_camera_direction = v,
    },
    Toggle {
        id: "renderer-opt-armament",
        label_key: "ui.renderer.settings.armament",
        read: |o, _| o.show_armament,
        write: |o, _, v| o.show_armament = v,
    },
    Toggle {
        id: "renderer-opt-score",
        label_key: "ui.renderer.settings.score_label",
        read: |o, _| o.show_score,
        write: |o, _, v| o.show_score = v,
    },
    Toggle {
        id: "renderer-opt-timer",
        label_key: "ui.renderer.settings.timer",
        read: |o, _| o.show_timer,
        write: |o, _, v| o.show_timer = v,
    },
    Toggle {
        id: "renderer-opt-kill-feed",
        label_key: "ui.renderer.settings.kill_feed",
        read: |o, _| o.show_kill_feed,
        write: |o, _, v| o.show_kill_feed = v,
    },
    Toggle {
        id: "renderer-opt-chat",
        label_key: "ui.renderer.settings.chat_label",
        read: |o, _| o.show_chat,
        write: |o, _, v| o.show_chat = v,
    },
    Toggle {
        id: "renderer-opt-battle-result",
        label_key: "ui.renderer.settings.battle_result",
        read: |o, _| o.show_battle_result,
        write: |o, _, v| o.show_battle_result = v,
    },
    Toggle {
        id: "renderer-opt-buffs",
        label_key: "ui.renderer.settings.buff_counters",
        read: |o, _| o.show_buffs,
        write: |o, _, v| o.show_buffs = v,
    },
    Toggle {
        id: "renderer-opt-trails",
        label_key: "ui.renderer.settings.heat_trail",
        read: |o, _| o.show_trails,
        write: |o, _, v| o.show_trails = v,
    },
    Toggle {
        id: "renderer-opt-ship-ranges",
        label_key: "ui.renderer.settings.ship_ranges",
        read: |o, _| o.show_ship_config,
        write: |o, _, v| o.show_ship_config = v,
    },
    Toggle {
        id: "renderer-opt-advantage",
        label_key: "ui.renderer.settings.team_advantage",
        read: |o, _| o.show_advantage,
        write: |o, _, v| o.show_advantage = v,
    },
    Toggle {
        id: "renderer-opt-team-rosters",
        label_key: "ui.renderer.settings.team_rosters",
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
];

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
/// The reader's saved layers, less the two a viewport holds off for the same
/// reasons: the stats panel's ship silhouettes are a load the bake skips, so its
/// gutter would be empty and the kill feed and the chat that share it would go
/// unfilmed; and which ships have their range circles drawn is a per-ship choice
/// a batch is never given, so drawing every ship's would bury the map.
pub fn batch_options(defaults: &SavedRenderOptions) -> (RenderOptions, bool) {
    let mut options = playback_options();
    defaults.apply_to(&mut options);
    options.show_stats_panel = false;
    options.show_ship_config = false;

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
