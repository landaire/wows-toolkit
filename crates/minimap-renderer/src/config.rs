use serde::Deserialize;
use serde::Serialize;

use crate::draw_command::ShipConfigVisibility;
use crate::renderer::RenderOptions;

/// CLI override flags for renderer configuration.
///
/// Fields mirror the `--no-*` / `--show-*` CLI flags. Pass this to
/// [`RendererConfig::apply_cli_overrides`] to apply them on top of
/// a config-file or default configuration.
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    pub no_player_names: bool,
    pub no_ship_names: bool,
    pub no_capture_points: bool,
    pub no_buildings: bool,
    pub no_turret_direction: bool,
    pub no_armament: bool,
    pub show_trails: bool,
    pub no_dead_trails: bool,
    pub show_speed_trails: bool,
    pub show_ship_config: bool,
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
    pub show_turret_direction: bool,
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
    // New features (default false)
    pub show_armament: bool,
    pub show_trails: bool,
    pub show_dead_trails: bool,
    pub show_speed_trails: bool,
    pub show_ship_config: bool,
    pub show_advantage: bool,
    pub show_score_timer: bool,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            show_player_names: true,
            show_ship_names: true,
            show_capture_points: true,
            show_buildings: true,
            show_turret_direction: true,
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
            show_armament: true,
            show_trails: false,
            show_dead_trails: true,
            show_speed_trails: false,
            show_ship_config: false,
            show_advantage: true,
            show_score_timer: true,
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

    /// Convert into RenderOptions for the renderer.
    pub fn into_render_options(self) -> RenderOptions {
        RenderOptions {
            show_player_names: self.show_player_names,
            show_ship_names: self.show_ship_names,
            show_capture_points: self.show_capture_points,
            show_buildings: self.show_buildings,
            show_turret_direction: self.show_turret_direction,
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
            ship_config_visibility: ShipConfigVisibility::default(),
        }
    }

    /// Generate a commented default TOML config string.
    pub fn generate_default_toml() -> String {
        r#"# Minimap Renderer Configuration
# Place this file as minimap_renderer.toml next to the executable,
# or specify with --config <path>.

# Display toggles (true = show, false = hide)

# Show player names above ship icons
show_player_names = true

# Show ship type names above ship icons
show_ship_names = true

# Show capture point zones with progress
show_capture_points = true

# Show building markers (e.g. shipyard structures)
show_buildings = true

# Show turret direction indicators
show_turret_direction = true

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

"#
        .to_string()
    }

    /// Apply CLI flag overrides from parsed arguments.
    pub fn apply_cli_overrides(&mut self, overrides: &CliOverrides) {
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
        if overrides.no_turret_direction {
            self.show_turret_direction = false;
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
    }
}
