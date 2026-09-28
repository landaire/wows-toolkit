//! The persisted form of [`RenderOptions`].
//!
//! Both front ends read and write the same `render_options` row, so the stored
//! shape and its mapping onto [`RenderOptions`] live beside the options
//! themselves rather than in either app.

use serde::Deserialize;
use serde::Serialize;

use crate::codec::VideoCodec;
use crate::config::RenderOptions;
use crate::draw_command::ShipConfigFilter;

const fn yes() -> bool {
    true
}

/// What a renderer opens showing, and what an export is encoded with.
///
/// Every field carries a serde default so a row written by an older build still
/// loads, with the layers it did not know about at their own defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedRenderOptions {
    #[serde(default = "yes")]
    pub show_hp_bars: bool,
    #[serde(default = "yes")]
    pub show_tracers: bool,
    #[serde(default = "yes")]
    pub show_torpedoes: bool,
    #[serde(default = "yes")]
    pub show_planes: bool,
    #[serde(default = "yes")]
    pub show_smoke: bool,
    #[serde(default = "yes")]
    pub show_score: bool,
    #[serde(default = "yes")]
    pub show_timer: bool,
    #[serde(default = "yes")]
    pub show_kill_feed: bool,
    #[serde(default)]
    pub show_player_names: bool,
    #[serde(default = "yes")]
    pub show_ship_names: bool,
    #[serde(default = "yes")]
    pub show_capture_points: bool,
    #[serde(default = "yes")]
    pub show_buildings: bool,
    #[serde(default = "yes", alias = "show_turret_direction")]
    pub show_camera_direction: bool,
    #[serde(default = "yes")]
    pub show_consumables: bool,
    #[serde(default = "yes")]
    pub show_dead_ships: bool,
    #[serde(default)]
    pub show_dead_ship_names: bool,
    #[serde(default)]
    pub show_armament: bool,
    #[serde(default)]
    pub show_trails: bool,
    #[serde(default)]
    pub show_dead_trails: bool,
    #[serde(default)]
    pub show_speed_trails: bool,
    #[serde(default = "yes")]
    pub show_battle_result: bool,
    #[serde(default = "yes")]
    pub show_buffs: bool,
    #[serde(default)]
    pub show_ship_config: bool,
    #[serde(default)]
    pub show_self_detection_range: bool,
    #[serde(default)]
    pub show_self_main_battery_range: bool,
    #[serde(default)]
    pub show_self_secondary_range: bool,
    #[serde(default)]
    pub show_self_torpedo_range: bool,
    #[serde(default)]
    pub show_self_radar_range: bool,
    #[serde(default)]
    pub show_self_hydro_range: bool,
    #[serde(default = "yes")]
    pub show_chat: bool,
    #[serde(default = "yes")]
    pub show_advantage: bool,
    #[serde(default = "yes")]
    pub show_score_timer: bool,
    #[serde(default = "yes")]
    pub show_stats_panel: bool,
    #[serde(default = "yes")]
    pub show_team_rosters: bool,
    /// Encode in software even where the GPU could do it.
    #[serde(default)]
    pub prefer_cpu_encoder: bool,
    /// `None` means the best codec the backend can manage.
    #[serde(default, with = "tolerant_codec")]
    pub video_codec: Option<VideoCodec>,
    /// Start an exported video at the loading screen rather than at the battle.
    #[serde(default)]
    pub include_pre_battle: bool,
}

impl Default for SavedRenderOptions {
    fn default() -> Self {
        Self {
            show_hp_bars: true,
            show_tracers: true,
            show_torpedoes: true,
            show_planes: true,
            show_smoke: true,
            show_score: true,
            show_timer: true,
            show_kill_feed: true,
            show_player_names: false,
            show_ship_names: true,
            show_capture_points: true,
            show_buildings: true,
            show_camera_direction: true,
            show_consumables: true,
            show_dead_ships: true,
            show_dead_ship_names: false,
            show_armament: true,
            show_trails: false,
            show_dead_trails: false,
            show_speed_trails: false,
            show_battle_result: true,
            show_buffs: true,
            show_ship_config: false,
            show_self_detection_range: false,
            show_self_main_battery_range: false,
            show_self_secondary_range: false,
            show_self_torpedo_range: false,
            show_self_radar_range: false,
            show_self_hydro_range: false,
            show_chat: false,
            show_advantage: true,
            show_score_timer: true,
            // The rosters take the gutters the stats panel would use, and are
            // swapped in when a merged replay is loaded.
            show_stats_panel: true,
            show_team_rosters: false,
            prefer_cpu_encoder: false,
            video_codec: None,
            include_pre_battle: false,
        }
    }
}

impl SavedRenderOptions {
    /// Self ship range visibility as a [`ShipConfigFilter`].
    pub fn self_range_filter(&self) -> ShipConfigFilter {
        ShipConfigFilter {
            detection: self.show_self_detection_range,
            main_battery: self.show_self_main_battery_range,
            secondary_battery: self.show_self_secondary_range,
            torpedo: self.show_self_torpedo_range,
            radar: self.show_self_radar_range,
            hydro: self.show_self_hydro_range,
        }
    }

    /// Records self ship range visibility from a [`ShipConfigFilter`].
    pub fn set_self_range_filter(&mut self, filter: &ShipConfigFilter) {
        self.show_self_detection_range = filter.detection;
        self.show_self_main_battery_range = filter.main_battery;
        self.show_self_secondary_range = filter.secondary_battery;
        self.show_self_torpedo_range = filter.torpedo;
        self.show_self_radar_range = filter.radar;
        self.show_self_hydro_range = filter.hydro;
    }

    /// Whether any self range is asked for.
    pub fn any_self_range_enabled(&self) -> bool {
        self.self_range_filter().any_enabled()
    }

    /// Writes these layers onto `options`.
    ///
    /// `show_dead_ships` has no field in [`RenderOptions`] and is read from
    /// here directly; `ship_config_visibility` is left as it was, because which
    /// ships get their circles is the viewer's business rather than a saved
    /// layer.
    pub fn apply_to(&self, options: &mut RenderOptions) {
        options.show_hp_bars = self.show_hp_bars;
        options.show_tracers = self.show_tracers;
        options.show_torpedoes = self.show_torpedoes;
        options.show_planes = self.show_planes;
        options.show_smoke = self.show_smoke;
        options.show_score = self.show_score;
        options.show_timer = self.show_timer;
        options.show_kill_feed = self.show_kill_feed;
        options.show_player_names = self.show_player_names;
        options.show_ship_names = self.show_ship_names;
        options.show_capture_points = self.show_capture_points;
        options.show_buildings = self.show_buildings;
        // There is no saved flag for the weather layer, so it follows the
        // buildings one: turning buildings off hides weather zones with them.
        options.show_weather = self.show_buildings;
        options.show_camera_direction = self.show_camera_direction;
        options.show_consumables = self.show_consumables;
        options.show_armament = self.show_armament;
        options.show_trails = self.show_trails;
        options.show_dead_trails = self.show_dead_trails;
        options.show_speed_trails = self.show_speed_trails;
        options.show_ship_config = self.show_ship_config;
        options.show_dead_ship_names = self.show_dead_ship_names;
        options.show_battle_result = self.show_battle_result;
        options.show_buffs = self.show_buffs;
        options.show_chat = self.show_chat;
        options.show_advantage = self.show_advantage;
        options.show_score_timer = self.show_score_timer;
        options.show_stats_panel = self.show_stats_panel;
        options.show_team_rosters = self.show_team_rosters;
    }

    /// Takes these layers from `options`, leaving every field the options do not
    /// carry -- the dead ships, the self ranges and the export settings -- as it
    /// is.
    pub fn read_from(&mut self, options: &RenderOptions) {
        self.show_hp_bars = options.show_hp_bars;
        self.show_tracers = options.show_tracers;
        self.show_torpedoes = options.show_torpedoes;
        self.show_planes = options.show_planes;
        self.show_smoke = options.show_smoke;
        self.show_score = options.show_score;
        self.show_timer = options.show_timer;
        self.show_kill_feed = options.show_kill_feed;
        self.show_player_names = options.show_player_names;
        self.show_ship_names = options.show_ship_names;
        self.show_capture_points = options.show_capture_points;
        self.show_buildings = options.show_buildings;
        self.show_camera_direction = options.show_camera_direction;
        self.show_consumables = options.show_consumables;
        self.show_armament = options.show_armament;
        self.show_trails = options.show_trails;
        self.show_dead_trails = options.show_dead_trails;
        self.show_speed_trails = options.show_speed_trails;
        self.show_ship_config = options.show_ship_config;
        self.show_dead_ship_names = options.show_dead_ship_names;
        self.show_battle_result = options.show_battle_result;
        self.show_buffs = options.show_buffs;
        self.show_chat = options.show_chat;
        self.show_advantage = options.show_advantage;
        self.show_score_timer = options.show_score_timer;
        self.show_stats_panel = options.show_stats_panel;
        self.show_team_rosters = options.show_team_rosters;
    }
}

/// Reads the stored codec name, treating one this build does not know as no
/// codec at all.
///
/// The whole row would otherwise fail to deserialize over a single unrecognised
/// name, losing every layer choice with it.
mod tolerant_codec {
    use serde::Deserialize as _;

    use super::VideoCodec;

    pub fn serialize<S: serde::Serializer>(codec: &Option<VideoCodec>, out: S) -> Result<S::Ok, S::Error> {
        match codec {
            Some(codec) => out.serialize_some(codec),
            None => out.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<VideoCodec>, D::Error> {
        let Some(named) = Option::<String>::deserialize(input)? else { return Ok(None) };
        Ok(named.parse().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::SavedRenderOptions;
    use crate::config::RenderOptions;

    /// Every layer written out comes back, so a reader's choices survive a
    /// round trip through the options the renderer actually draws with.
    #[test]
    fn every_layer_survives_a_round_trip() {
        // Each layer the opposite of its default, so a field dropped from both
        // directions cannot pass by agreeing with what it started as.
        let saved = SavedRenderOptions {
            show_hp_bars: false,
            show_tracers: false,
            show_torpedoes: false,
            show_planes: false,
            show_smoke: false,
            show_score: false,
            show_timer: false,
            show_kill_feed: false,
            show_player_names: true,
            show_ship_names: false,
            show_capture_points: false,
            show_buildings: false,
            show_camera_direction: false,
            show_consumables: false,
            show_dead_ships: false,
            show_dead_ship_names: true,
            show_armament: false,
            show_trails: true,
            show_dead_trails: true,
            show_speed_trails: true,
            show_battle_result: false,
            show_buffs: false,
            show_ship_config: true,
            show_chat: true,
            show_advantage: false,
            show_score_timer: false,
            show_stats_panel: false,
            show_team_rosters: true,
            ..SavedRenderOptions::default()
        };

        let mut options = RenderOptions::default();
        saved.apply_to(&mut options);

        let mut read_back = SavedRenderOptions { show_dead_ships: saved.show_dead_ships, ..Default::default() };
        read_back.read_from(&options);
        assert_eq!(read_back, saved);
    }

    /// The fields the options do not carry are the caller's to set, so reading
    /// the options back does not clear them.
    #[test]
    fn the_ranges_and_the_export_settings_are_left_alone() {
        let mut saved = SavedRenderOptions {
            show_self_radar_range: true,
            prefer_cpu_encoder: true,
            include_pre_battle: true,
            video_codec: Some(crate::codec::VideoCodec::Av1),
            ..SavedRenderOptions::default()
        };

        saved.read_from(&RenderOptions::default());

        assert!(saved.show_self_radar_range);
        assert!(saved.any_self_range_enabled());
        assert!(saved.prefer_cpu_encoder);
        assert!(saved.include_pre_battle);
        assert_eq!(saved.video_codec, Some(crate::codec::VideoCodec::Av1));
    }

    /// A codec name this build does not know costs the codec choice and nothing
    /// else.
    #[test]
    fn an_unknown_codec_does_not_take_the_row_with_it() {
        let saved: SavedRenderOptions =
            serde_json::from_str(r#"{"show_hp_bars":false,"video_codec":"h266"}"#).expect("the row parses");

        assert_eq!(saved.video_codec, None, "the unknown name reads as no choice");
        assert!(!saved.show_hp_bars, "and the layers around it are kept");
    }

    /// A stored codec this build does know is read as itself, and written back
    /// as the name it was stored under.
    #[test]
    fn a_known_codec_round_trips_under_its_stored_name() {
        let saved: SavedRenderOptions = serde_json::from_str(r#"{"video_codec":"av1"}"#).expect("the row parses");
        assert_eq!(saved.video_codec, Some(crate::codec::VideoCodec::Av1));

        let written = serde_json::to_string(&saved).expect("the row is written");
        assert!(written.contains(r#""video_codec":"av1""#), "got {written}");
    }

    /// A row from a build that knew fewer layers still loads, with the rest at
    /// their own defaults.
    #[test]
    fn a_shorter_row_still_loads() {
        let saved: SavedRenderOptions =
            serde_json::from_str(r#"{"show_hp_bars":false,"show_turret_direction":false}"#).expect("the row parses");

        assert!(!saved.show_hp_bars);
        assert!(!saved.show_camera_direction, "the old name for the camera arc is still read");
        assert_eq!(saved.show_ship_names, SavedRenderOptions::default().show_ship_names);
    }
}
