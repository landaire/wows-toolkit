use serde::Deserialize;
use serde::Serialize;
use wows_core::pseudonym::NameDisplay;

use crate::draw_command::DrawCommand;
use crate::draw_command::ShipConfigVisibility;
use crate::error::TimingError;
use crate::timing::ExportSpeed;
use crate::timing::ExportTiming;
use crate::timing::VideoLength;

/// Configurable rendering options.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderOptions {
    pub show_hp_bars: bool,
    pub show_tracers: bool,
    pub show_torpedoes: bool,
    pub show_planes: bool,
    pub show_smoke: bool,
    pub show_score: bool,
    pub show_timer: bool,
    pub show_kill_feed: bool,
    pub show_player_names: bool,
    pub show_ship_names: bool,
    pub show_capture_points: bool,
    pub show_buildings: bool,
    pub show_weather: bool,
    pub show_camera_direction: bool,
    pub show_consumables: bool,
    pub show_submarine_surveillance: bool,
    pub show_submarine_status: bool,
    pub show_armament: bool,
    pub show_trails: bool,
    pub show_dead_trails: bool,
    pub show_speed_trails: bool,
    pub show_ship_config: bool,
    pub show_dead_ship_names: bool,
    pub show_battle_result: bool,
    pub show_buffs: bool,
    pub show_chat: bool,
    pub show_advantage: bool,
    pub show_score_timer: bool,
    pub show_stats_panel: bool,
    pub show_team_rosters: bool,
    /// Controls which ships have their config circles rendered when show_ship_config is true.
    /// Defaults to SelfOnly (only the replay owner's circles).
    pub ship_config_visibility: ShipConfigVisibility,
    /// Whether player names and clan tags are drawn as they are or as
    /// pseudonyms. Read once, when the renderer first caches the players.
    pub player_names: NameDisplay,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            show_hp_bars: true,
            show_tracers: true,
            show_torpedoes: true,
            show_planes: true,
            show_smoke: true,
            show_score: true,
            show_timer: true,
            show_kill_feed: false,
            show_player_names: true,
            show_ship_names: true,
            show_capture_points: true,
            show_buildings: true,
            show_weather: true,
            show_camera_direction: true,
            show_consumables: true,
            show_submarine_surveillance: true,
            show_submarine_status: true,
            show_armament: false,
            show_trails: false,
            show_dead_trails: true,
            show_speed_trails: false,
            show_ship_config: false,
            show_dead_ship_names: false,
            show_battle_result: true,
            show_buffs: true,
            show_chat: false,
            show_advantage: true,
            show_score_timer: true,
            show_stats_panel: true,
            show_team_rosters: false,
            ship_config_visibility: ShipConfigVisibility::default(),
            player_names: NameDisplay::Real,
        }
    }
}

impl RenderOptions {
    /// True when the self-perspective stats panel is actually rendered.
    ///
    /// Team rosters replace the stats panel when both flags are set, so the
    /// raw `show_stats_panel` toggle alone isn't sufficient for deciding
    /// whether overlays that overlap the panel area (kill feed, chat) should
    /// hide themselves. Use this method for any "is the panel really showing"
    /// decision.
    pub fn stats_panel_visible(&self) -> bool {
        self.show_stats_panel && !self.show_team_rosters
    }
}

/// CLI override flags for renderer configuration.
///
/// Fields mirror the `--no-*` / `--show-*` CLI flags. Pass this to
/// [`RendererConfig::apply_cli_overrides`] to apply them on top of
/// a config-file or default configuration.
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    pub show_player_names: bool,
    pub no_player_names: bool,
    pub no_ship_names: bool,
    pub no_capture_points: bool,
    pub no_buildings: bool,
    pub no_camera_direction: bool,
    pub no_armament: bool,
    pub no_kill_feed: bool,
    pub no_chat: bool,
    pub show_trails: bool,
    pub no_dead_trails: bool,
    pub show_speed_trails: bool,
    pub show_ship_config: bool,
    pub team_rosters: bool,
    pub no_team_rosters: bool,
    pub stats_panel: bool,
    pub no_stats_panel: bool,
    pub include_pre_battle: bool,
    /// `--video-length`. Replaces the config file's export timing when it or
    /// `speed` is given.
    pub video_length: Option<VideoLength>,
    /// `--speed`.
    pub speed: Option<ExportSpeed>,
}

/// Renderer configuration, loadable from a TOML file.
///
/// All fields default to their standard values. CLI flags override config file values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RendererConfig {
    // Display toggles (all default true)
    pub show_player_names: bool,
    pub show_ship_names: bool,
    pub show_capture_points: bool,
    pub show_buildings: bool,
    #[serde(alias = "show_turret_direction")]
    pub show_camera_direction: bool,
    pub show_hp_bars: bool,
    pub show_tracers: bool,
    pub show_torpedoes: bool,
    pub show_planes: bool,
    pub show_smoke: bool,
    pub show_score: bool,
    pub show_timer: bool,
    pub show_kill_feed: bool,
    pub show_chat: bool,
    pub show_consumables: bool,
    pub show_submarine_surveillance: bool,
    pub show_submarine_status: bool,
    // New features (default false)
    pub show_armament: bool,
    pub show_trails: bool,
    pub show_dead_trails: bool,
    pub show_speed_trails: bool,
    pub show_ship_config: bool,
    pub show_advantage: bool,
    pub show_score_timer: bool,
    pub show_stats_panel: bool,
    pub show_team_rosters: bool,
    /// Include the pre-battle phase (spawn and countdown) at the start of the
    /// video. When false, rendering begins at battle start.
    pub include_pre_battle: bool,
    /// Fit the exported stretch of the replay into a video this many seconds
    /// long. Exclusive with `speed`.
    pub video_length: Option<VideoLength>,
    /// Play the export at this many game seconds per video second. Exclusive
    /// with `video_length`.
    pub speed: Option<ExportSpeed>,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            // Mirrors `SavedRenderOptions::default()` so CLI and desktop
            // renders look the same out of the box.
            show_player_names: false,
            show_ship_names: true,
            show_capture_points: true,
            show_buildings: true,
            show_camera_direction: true,
            show_hp_bars: true,
            show_tracers: true,
            show_torpedoes: true,
            show_planes: true,
            show_smoke: true,
            show_score: true,
            show_timer: true,
            show_kill_feed: true,
            show_chat: true,
            show_consumables: true,
            show_submarine_surveillance: true,
            show_submarine_status: true,
            show_armament: true,
            show_trails: false,
            show_dead_trails: true,
            show_speed_trails: false,
            show_ship_config: false,
            show_advantage: true,
            show_score_timer: true,
            show_stats_panel: true,
            show_team_rosters: false,
            include_pre_battle: false,
            video_length: None,
            speed: None,
        }
    }
}

impl RendererConfig {
    /// Load config from a TOML file.
    #[cfg(feature = "bin")]
    pub fn load(path: &std::path::Path) -> Result<Self, rootcause::Report> {
        use rootcause::prelude::*;
        let contents = std::fs::read_to_string(path).context("Failed to read config file")?;
        let config: Self = toml::from_str(&contents).context("Failed to parse config file")?;
        Ok(config)
    }

    /// How the export maps game time onto video time. With neither a length
    /// nor a speed it is the default length.
    pub fn export_timing(&self) -> Result<ExportTiming, TimingError> {
        match (self.video_length, self.speed) {
            (Some(length), Some(speed)) => Err(TimingError::LengthAndSpeed {
                seconds: length.seconds(),
                multiplier: speed.game_seconds_per_second(),
            }),
            (Some(length), None) => Ok(ExportTiming::Length(length)),
            (None, Some(speed)) => Ok(ExportTiming::Speed(speed)),
            (None, None) => Ok(ExportTiming::default()),
        }
    }

    /// Convert into RenderOptions for the renderer.
    pub fn into_render_options(self) -> RenderOptions {
        // Stats panel and team rosters share the same gutter, so if a config
        // file enables both the rosters win (matching the desktop behavior).
        let show_team_rosters = self.show_team_rosters;
        let show_stats_panel = self.show_stats_panel && !show_team_rosters;
        RenderOptions {
            show_player_names: self.show_player_names,
            show_ship_names: self.show_ship_names,
            show_capture_points: self.show_capture_points,
            show_buildings: self.show_buildings,
            show_camera_direction: self.show_camera_direction,
            show_hp_bars: self.show_hp_bars,
            show_tracers: self.show_tracers,
            show_torpedoes: self.show_torpedoes,
            show_planes: self.show_planes,
            show_smoke: self.show_smoke,
            show_score: self.show_score,
            show_timer: self.show_timer,
            show_kill_feed: self.show_kill_feed,
            show_chat: self.show_chat,
            show_consumables: self.show_consumables,
            show_submarine_surveillance: self.show_submarine_surveillance,
            show_submarine_status: self.show_submarine_status,
            show_armament: self.show_armament,
            show_trails: self.show_trails,
            show_dead_trails: self.show_dead_trails,
            show_speed_trails: self.show_speed_trails,
            show_ship_config: self.show_ship_config,
            show_dead_ship_names: false,
            show_battle_result: true,
            show_buffs: true,
            show_weather: true,
            show_advantage: true,
            show_score_timer: true,
            show_stats_panel,
            show_team_rosters,
            ship_config_visibility: ShipConfigVisibility::default(),
            player_names: NameDisplay::Real,
        }
    }

    /// Generate a commented default TOML config string.
    pub fn generate_default_toml() -> String {
        r#"# Minimap Renderer Configuration
# Place this file as minimap_renderer.toml next to the executable,
# or specify with --config <path>.

# Display toggles (true = show, false = hide)

# Show player names above ship icons
show_player_names = false

# Show ship type names above ship icons
show_ship_names = true

# Show capture point zones with progress
show_capture_points = true

# Show building markers (e.g. shipyard structures)
show_buildings = true

# Show camera/look direction indicators
show_camera_direction = true

# Show health bars below ship icons
show_hp_bars = true

# Show artillery shell tracers
show_tracers = true

# Show torpedo markers
show_torpedoes = true

# Show plane squadron icons
show_planes = true

# Show smoke screen clouds
show_smoke = true

# Show team score bar at top
show_score = true

# Show game timer
show_timer = true

# Show kill feed in top-right corner
show_kill_feed = true

# Show chat messages on the left side of the minimap
show_chat = true

# Show active consumable icons below ships
show_consumables = true

# Show active submarine surveillance ranges
show_submarine_surveillance = true

# Show submarine dive capacity and depth
show_submarine_status = true

# Show selected armament/ammo type below ship icons (e.g. AP, HE, SAP, Torp)
show_armament = true

# Show position trail heatmap (rainbow: blue=oldest, red=newest)
show_trails = false

# Show trails for dead ships (only relevant when show_trails or show_speed_trails is true)
show_dead_trails = true

# Show speed-based position trails (blue=slow, red=fast relative to ship max speed)
show_speed_trails = false

# Show ship config range circles (detection, main battery, secondary, etc.)
show_ship_config = false

# Show the self-perspective stats panel on the right side of the canvas. Hidden
# automatically when team rosters are enabled.
show_stats_panel = true

# Show team roster panels on either side of the minimap (HP, frags, damage,
# consumables). Mutually exclusive with the stats panel.
show_team_rosters = false

# Include the pre-battle phase (spawn and countdown) at the start of the video.
# When false, rendering begins at battle start.
include_pre_battle = false

# Video timing. Set at most one of these; with neither, the video is 60 seconds.

# Fit the exported stretch of the replay into a video this many seconds long,
# from 1 to 600.
video_length = 60

# Or play the replay at this many game seconds per video second, at least 1
# (the live playback speeds are 1, 5, 10, 20, 40 and 60). Remove video_length
# to use it.
# speed = 20

"#
        .to_string()
    }

    /// Apply CLI flag overrides from parsed arguments.
    pub fn apply_cli_overrides(&mut self, overrides: &CliOverrides) {
        if overrides.show_player_names {
            self.show_player_names = true;
        }
        if overrides.no_player_names {
            self.show_player_names = false;
        }
        if overrides.no_ship_names {
            self.show_ship_names = false;
        }
        if overrides.no_capture_points {
            self.show_capture_points = false;
        }
        if overrides.no_buildings {
            self.show_buildings = false;
        }
        if overrides.no_camera_direction {
            self.show_camera_direction = false;
        }
        if overrides.no_armament {
            self.show_armament = false;
        }
        if overrides.show_trails {
            self.show_trails = true;
        }
        if overrides.no_dead_trails {
            self.show_dead_trails = false;
        }
        if overrides.show_speed_trails {
            self.show_speed_trails = true;
        }
        if overrides.show_ship_config {
            self.show_ship_config = true;
        }
        if overrides.no_kill_feed {
            self.show_kill_feed = false;
        }
        if overrides.no_chat {
            self.show_chat = false;
        }
        // Team rosters and the stats panel are exclusive, as the viewer's
        // checkboxes are. When both arrive enabled (e.g. team_rosters via CLI
        // plus stats_panel from a config file), team rosters win.
        if overrides.team_rosters {
            self.show_team_rosters = true;
            self.show_stats_panel = false;
        }
        if overrides.no_team_rosters {
            self.show_team_rosters = false;
        }
        if overrides.stats_panel && !self.show_team_rosters {
            self.show_stats_panel = true;
        }
        if overrides.no_stats_panel {
            self.show_stats_panel = false;
        }
        if overrides.include_pre_battle {
            self.include_pre_battle = true;
        }
        if overrides.video_length.is_some() || overrides.speed.is_some() {
            self.video_length = overrides.video_length;
            self.speed = overrides.speed;
        }
    }
}

/// Whether a drawn command survives the options in force.
///
/// Beside the options it reads rather than in a front end, so every app that
/// rasterises a command stream hides the same things: a toggle that means one
/// thing in one viewer and another elsewhere is worse than no toggle.
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
        DrawCommand::SubmarineSurveillanceRadius { .. } => opts.show_submarine_surveillance,
        DrawCommand::SubmarineStatus { .. } => opts.show_submarine_status,
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
        // The rosters take the gutter when both are on, as the canvas does.
        DrawCommand::StatsPanel { .. }
        | DrawCommand::StatsSilhouette { .. }
        | DrawCommand::StatsDamage { .. }
        | DrawCommand::StatsRibbons { .. }
        | DrawCommand::StatsActivityFeed { .. } => opts.stats_panel_visible(),
        DrawCommand::TeamRoster { .. } => opts.show_team_rosters,
        // Not a layer of the battle but what a reader drew over it, so no
        // display option hides it: they put it there deliberately.
        DrawCommand::Annotation { .. } => true,
    }
}

/// Clears what the options in force hide of a drawn command, for the layers
/// that are fields of another command rather than commands of their own.
///
/// Beside [`should_draw_command`] for the same reason: every app that
/// rasterises a command stream hides the same things.
pub fn hide_command_parts(cmd: &mut DrawCommand, opts: &RenderOptions) {
    match cmd {
        DrawCommand::Ship { player_name, ship_name, .. } | DrawCommand::Plane { player_name, ship_name, .. } => {
            hide_names(player_name, ship_name, opts);
        }
        DrawCommand::DeadShip { player_name, ship_name, .. } => {
            if opts.show_dead_ship_names {
                hide_names(player_name, ship_name, opts);
            } else {
                *player_name = None;
                *ship_name = None;
            }
        }
        DrawCommand::ScoreBar { team0_timer, team1_timer, .. } if !opts.show_score_timer => {
            *team0_timer = None;
            *team1_timer = None;
        }
        _ => {}
    }
}

fn hide_names(player_name: &mut Option<String>, ship_name: &mut Option<String>, opts: &RenderOptions) {
    if !opts.show_player_names {
        *player_name = None;
    }
    if !opts.show_ship_names {
        *ship_name = None;
    }
}

#[cfg(all(test, feature = "bin"))]
mod tests {
    use super::*;

    fn timing_of(toml_text: &str) -> Result<ExportTiming, TimingError> {
        let config: RendererConfig = toml::from_str(toml_text).expect("the config parses");

        config.export_timing()
    }

    #[test]
    fn the_default_config_fits_a_minute() {
        let timing = timing_of(&RendererConfig::generate_default_toml());

        assert_eq!(timing, Ok(ExportTiming::Length(VideoLength::DEFAULT)));
        assert_eq!(timing_of(""), Ok(ExportTiming::default()));
    }

    #[test]
    fn a_speed_alone_plays_at_that_speed() {
        let timing = timing_of("speed = 40");

        assert_eq!(timing, Ok(ExportTiming::Speed(ExportSpeed::new(40.0).expect("an export speed"))));
    }

    #[test]
    fn a_length_and_a_speed_together_are_refused() {
        let timing = timing_of("video_length = 30\nspeed = 20");

        assert_eq!(timing, Err(TimingError::LengthAndSpeed { seconds: 30.0, multiplier: 20.0 }));
    }

    #[test]
    fn a_timing_out_of_range_does_not_parse() {
        assert!(toml::from_str::<RendererConfig>("speed = 0").is_err());
        assert!(toml::from_str::<RendererConfig>("speed = 0.5").is_err());
        assert!(toml::from_str::<RendererConfig>("video_length = -1").is_err());
        assert!(toml::from_str::<RendererConfig>("video_length = 601").is_err());
    }

    /// A timing flag replaces the config file's timing rather than joining it.
    #[test]
    fn a_cli_speed_replaces_a_configured_length() {
        let mut config: RendererConfig = toml::from_str("video_length = 30").expect("the config parses");
        let speed = ExportSpeed::new(10.0).expect("an export speed");

        config.apply_cli_overrides(&CliOverrides { speed: Some(speed), ..CliOverrides::default() });

        assert_eq!(config.export_timing(), Ok(ExportTiming::Speed(speed)));
    }
}
