use std::collections::{HashMap, HashSet};

use image::RgbaImage;
use wowsunpack::data::{ResourceLoader as _, Version};
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::{GameParamProvider, Meters, PlaneCategory, Species};

use wows_replays::analyzer::decoder::{BattleStage, BuoyancyState, FinishType, Recognized, TorpedoData, WeaponType};

use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::analyzer::battle_controller::state::ControlPointType;
use wows_replays::analyzer::decoder::Consumable;
use wows_replays::types::{EntityId, GameClock, GameParamId, PlaneId, Relation, WorldPos};

use crate::assets::GameFonts;
use crate::draw_command::{
    ChatEntry, DrawCommand, FontHint, KillFeedEntry, ShipConfigCircleKind, ShipConfigVisibility, ShipVisibility,
};
use crate::map_data;

use crate::MINIMAP_SIZE;

// How long various effects persist in game-seconds
const TRACER_LEN: f32 = 0.12; // fraction of total shot path length
const KILL_FEED_DURATION: f32 = 10.0;

// Visual constants
const SMOKE_COLOR: [u8; 3] = [200, 200, 200];
const SMOKE_ALPHA: f32 = 0.5;
const HP_BAR_FULL_COLOR: [u8; 3] = [0, 255, 0];
const HP_BAR_MID_COLOR: [u8; 3] = [255, 255, 0];
const HP_BAR_LOW_COLOR: [u8; 3] = [255, 0, 0];
const HP_BAR_BG_COLOR: [u8; 3] = [50, 50, 50];
const HP_BAR_BG_ALPHA: f32 = 0.7;
const UNDETECTED_OPACITY: f32 = 0.4;
const TEAM0_COLOR: [u8; 3] = [76, 232, 170]; // Green
const TEAM1_COLOR: [u8; 3] = [254, 77, 42]; // Red

/// Per-consumable radius circle color, with friendly/enemy variants.
fn consumable_radius_color(consumable: &Recognized<Consumable>, is_friendly: bool) -> [u8; 3] {
    match (consumable.known(), is_friendly) {
        (Some(Consumable::Radar), true) => [40, 80, 200],  // Dark blue
        (Some(Consumable::Radar), false) => [180, 40, 50], // Maroon
        (Some(Consumable::HydroacousticSearch), true) => [40, 180, 170], // Teal
        (Some(Consumable::HydroacousticSearch), false) => [200, 90, 30], // Dark orange
        (Some(Consumable::Hydrophone), true) => [70, 110, 180], // Slate blue
        (Some(Consumable::Hydrophone), false) => [170, 70, 50], // Rust
        (Some(Consumable::SubmarineSurveillance), true) => [60, 60, 190], // Indigo
        (Some(Consumable::SubmarineSurveillance), false) => [160, 30, 60], // Dark crimson
        (_, true) => TEAM0_COLOR,
        (_, false) => TEAM1_COLOR,
    }
}

/// Configurable rendering options.
#[derive(Clone, Debug)]
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
    pub show_turret_direction: bool,
    pub show_consumables: bool,
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
    /// Controls which ships have their config circles rendered when show_ship_config is true.
    /// Defaults to SelfOnly (only the replay owner's circles).
    pub ship_config_visibility: ShipConfigVisibility,
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
            show_kill_feed: true,
            show_player_names: true,
            show_ship_names: true,
            show_capture_points: true,
            show_buildings: true,
            show_weather: true,
            show_turret_direction: true,
            show_consumables: true,
            show_armament: false,
            show_trails: false,
            show_dead_trails: true,
            show_speed_trails: false,
            show_ship_config: false,
            show_dead_ship_names: false,
            show_battle_result: true,
            show_buffs: true,
            show_chat: true,
            show_advantage: true,
            show_score_timer: true,
            ship_config_visibility: ShipConfigVisibility::default(),
        }
    }
}

struct SquadronInfo {
    icon_base: String,
    icon_dir: &'static str,
    /// True for consumable-spawned planes (catapult fighters, spotter planes)
    is_consumable: bool,
}

/// Streaming minimap renderer.
///
/// Reads live state from `BattleControllerState` at each frame boundary
/// and emits `DrawCommand`s to a `RenderTarget`. No timelines are stored.
pub struct MinimapRenderer<'a> {
    // Config (immutable after construction)
    map_info: Option<map_data::MapInfo>,
    game_params: &'a GameMetadataProvider,
    version: Version,
    pub options: RenderOptions,

    // Caches populated lazily from controller state
    squadron_info: HashMap<PlaneId, SquadronInfo>,
    player_species: HashMap<EntityId, String>,
    player_names: HashMap<EntityId, String>,
    ship_param_ids: HashMap<EntityId, GameParamId>,
    ship_display_names: HashMap<EntityId, String>,
    player_relations: HashMap<EntityId, Relation>,
    /// Per-ship consumable icon names: (entity_id, Consumable) -> PCY name (e.g. "PCY015_SpeedBoosterPremium")
    ship_ability_icons: HashMap<(EntityId, Recognized<Consumable>), String>,
    /// Per-ship consumable variants for detection radius lookup: (entity_id, Consumable) -> (ability_name, variant_name)
    ship_ability_variants: HashMap<(EntityId, Recognized<Consumable>), (String, String)>,
    /// Per-player clan tag: entity_id -> clan tag string
    player_clan_tags: HashMap<EntityId, String>,
    /// Per-player clan color: entity_id -> RGB color (None = use team color)
    player_clan_colors: HashMap<EntityId, Option<[u8; 3]>>,
    /// Track which entities we've already resolved ability icons for
    resolved_entities: HashSet<EntityId>,
    /// Entity IDs of players in the recording player's division (excluding self).
    division_mates: HashSet<EntityId>,
    players_populated: bool,
    /// Raw team_id of the recording player (0 or 1). Used to map cap point/building
    /// team_ids to relative colors (friendly vs enemy).
    self_team_id: Option<i64>,

    /// Position history per entity for trail rendering: (position, game_clock, speed_raw)
    position_history: HashMap<EntityId, Vec<(map_data::MinimapPos, GameClock, u16)>>,

    /// Game fonts for CJK fallback selection on chat messages.
    fonts: Option<GameFonts>,

    /// Flag icons for base-type capture points, keyed by "ally"/"enemy"/"neutral".
    flag_icons: HashMap<String, RgbaImage>,
}

impl<'a> MinimapRenderer<'a> {
    pub fn new(
        map_info: Option<map_data::MapInfo>,
        game_params: &'a GameMetadataProvider,
        version: Version,
        options: RenderOptions,
    ) -> Self {
        Self {
            map_info,
            game_params,
            version,
            options,
            squadron_info: HashMap::new(),
            player_species: HashMap::new(),
            player_names: HashMap::new(),
            ship_param_ids: HashMap::new(),
            ship_display_names: HashMap::new(),
            player_relations: HashMap::new(),
            ship_ability_icons: HashMap::new(),
            ship_ability_variants: HashMap::new(),
            player_clan_tags: HashMap::new(),
            player_clan_colors: HashMap::new(),
            resolved_entities: HashSet::new(),
            division_mates: HashSet::new(),
            players_populated: false,
            self_team_id: None,
            position_history: HashMap::new(),
            fonts: None,
            flag_icons: HashMap::new(),
        }
    }

    /// Set the flag icons for base-type capture points.
    pub fn set_flag_icons(&mut self, icons: HashMap<String, RgbaImage>) {
        self.flag_icons = icons;
    }

    /// Set the game fonts for CJK fallback selection on chat messages.
    pub fn set_fonts(&mut self, fonts: GameFonts) {
        self.fonts = Some(fonts);
    }

    /// Reset all cached state, allowing the renderer to be reused after a seek.
    pub fn reset(&mut self) {
        self.squadron_info.clear();
        self.player_species.clear();
        self.player_names.clear();
        self.ship_param_ids.clear();
        self.ship_display_names.clear();
        self.player_relations.clear();
        self.ship_ability_icons.clear();
        self.ship_ability_variants.clear();
        self.player_clan_tags.clear();
        self.player_clan_colors.clear();
        self.resolved_entities.clear();
        self.division_mates.clear();
        self.players_populated = false;
        self.self_team_id = None;
        self.position_history.clear();
    }

    /// Populate player info from controller state (once).
    ///
    /// Uses `player_entities` (populated from onArenaStateReceived packet parsing).
    pub fn populate_players(&mut self, controller: &dyn BattleControllerState) {
        if self.players_populated {
            return;
        }

        let players = controller.player_entities();
        if players.is_empty() {
            return;
        }

        for (entity_id, player) in players {
            self.player_relations.insert(*entity_id, player.relation());
            if let Some(species) = player.vehicle().species().and_then(|s| s.known()) {
                self.player_species.insert(*entity_id, species.name().to_string());
            }

            let player_name = {
                let raw_name = player.initial_state().username();
                if player.is_bot() && raw_name.starts_with("IDS_") {
                    self.game_params.localized_name_from_id(raw_name).unwrap_or_else(|| raw_name.to_string())
                } else {
                    raw_name.to_string()
                }
            };
            self.player_names.insert(*entity_id, player_name);
            // Cache clan info
            let clan_tag = player.initial_state().clan().to_string();
            if !clan_tag.is_empty() {
                self.player_clan_tags.insert(*entity_id, clan_tag);
            }
            let clan_color_raw = player.initial_state().clan_color();
            let clan_color = if clan_color_raw != 0 {
                Some([
                    ((clan_color_raw & 0xFF0000) >> 16) as u8,
                    ((clan_color_raw & 0xFF00) >> 8) as u8,
                    (clan_color_raw & 0xFF) as u8,
                ])
            } else {
                None
            };
            self.player_clan_colors.insert(*entity_id, clan_color);
            self.ship_param_ids.insert(*entity_id, player.vehicle().id());
            if let Some(name) = self.game_params.localized_name_from_param(player.vehicle()) {
                self.ship_display_names.insert(*entity_id, name.to_string());
            }

            // Cache consumable variants for detection radius lookup.
            // Iterate ship ability slots, look up each ability's consumableType from GameParams.
            let ship_id = player.vehicle().id();
            let ship_param = GameParamProvider::game_param_by_id(self.game_params, ship_id);
            if let Some(vehicle) = ship_param.as_ref().and_then(|p| p.vehicle())
                && let Some(abilities) = vehicle.abilities()
            {
                for slot in abilities {
                    for (ability_name, variant_name) in slot {
                        let Some(param) = GameParamProvider::game_param_by_name(self.game_params, ability_name) else {
                            continue;
                        };
                        let Some(ability) = param.ability() else {
                            continue;
                        };

                        let Some(cat) = ability.categories().values().next() else {
                            continue;
                        };
                        let consumable = cat.consumable_type(self.version);

                        self.ship_ability_variants
                            .insert((*entity_id, consumable), (ability_name.clone(), variant_name.clone()));
                    }
                }
            }
        }
        // Determine the recording player's raw team_id for relative coloring
        if self.self_team_id.is_none() {
            for (entity_id, player) in players {
                if player.relation().is_self() {
                    if let Some(entity) = controller.entities_by_id().get(entity_id)
                        && let Some(vehicle) = entity.vehicle_ref()
                    {
                        self.self_team_id = Some(vehicle.borrow().props().team_id() as i64);
                    }
                    break;
                }
            }
        }

        // Cache division mate entity IDs (skip in clan battles where the whole team is one div)
        if !controller.battle_type().known().is_some_and(|bt| bt.is_clan_battle()) {
            let self_state = players.values().find(|p| p.relation().is_self()).map(|p| p.initial_state());
            if let Some(self_state) = self_state {
                for (entity_id, player) in players {
                    if self_state.is_division_mate(player.initial_state()) {
                        self.division_mates.insert(*entity_id);
                    }
                }
            }
        }

        self.players_populated = true;
    }

    /// Resolve per-ship ability icon names from entity vehicle data.
    ///
    /// For each vehicle entity, reads `ship_config().abilities()` (equipped GameParam IDs),
    /// looks up each ability in GameParams to get its `consumable_type` and `name`,
    /// and maps `(EntityId, Consumable)` → PCY name for icon lookup.
    pub fn update_ship_abilities(&mut self, controller: &dyn BattleControllerState) {
        for (entity_id, entity) in controller.entities_by_id() {
            if self.resolved_entities.contains(entity_id) {
                continue;
            }
            let Some(vehicle) = entity.vehicle_ref() else {
                continue;
            };
            let vehicle = vehicle.borrow();
            let abilities = vehicle.props().ship_config().abilities();
            if abilities.is_empty() {
                continue;
            }
            self.resolved_entities.insert(*entity_id);
            for &ability_id in abilities {
                let Some(param) = GameParamProvider::game_param_by_id(self.game_params, ability_id) else {
                    continue;
                };
                let Some(ability) = param.ability() else {
                    continue;
                };
                // Get consumable_type from the first category
                let Some(cat) = ability.categories().values().next() else {
                    continue;
                };
                let consumable_type = cat.consumable_type_raw().to_string();
                let consumable = Consumable::from_consumable_type(&consumable_type, self.version);
                self.ship_ability_icons.insert((*entity_id, consumable), param.name().to_string());
            }
        }
    }

    /// Get the icon key for a consumable on a specific ship.
    ///
    /// Uses the per-ship ability mapping if available, falling back to the
    /// hardcoded base PCY name.
    fn consumable_icon_key(&self, entity_id: EntityId, consumable: Recognized<Consumable>) -> Option<String> {
        if let Some(name) = self.ship_ability_icons.get(&(entity_id, consumable.clone())) {
            return Some(name.clone());
        }
        consumable.into_known().and_then(consumable_to_base_icon_key)
    }

    /// Look up detection radius for a consumable on a specific ship from GameParams.
    ///
    /// Returns radius in meters, or None if not a detection consumable
    /// or if the lookup fails.
    fn get_consumable_radius(&self, entity_id: EntityId, consumable: Recognized<Consumable>) -> Option<Meters> {
        // Look up ship-specific ability variant (cached from populate_players)
        let (ability_name, variant_name) = self.ship_ability_variants.get(&(entity_id, consumable))?;
        let param = GameParamProvider::game_param_by_name(self.game_params, ability_name)?;
        let ability = param.ability()?;
        let cat = ability.get_category(variant_name)?;
        cat.detection_radius()
    }

    /// Update squadron info for any new planes in the controller.
    pub fn update_squadron_info(&mut self, controller: &dyn BattleControllerState) {
        // Clean up stale entries for removed planes so reused IDs get fresh data
        let active = controller.active_planes();
        self.squadron_info.retain(|id, _| active.contains_key(id));

        for (plane_id, plane) in active {
            if self.squadron_info.contains_key(plane_id) {
                continue;
            }
            let param = GameParamProvider::game_param_by_id(self.game_params, plane.params_id);
            let aircraft = param.as_ref().and_then(|p| p.aircraft());
            let category = aircraft.map(|a| a.category()).unwrap_or(&PlaneCategory::Controllable);
            let is_consumable = matches!(category, PlaneCategory::Consumable | PlaneCategory::Airsupport);
            let ammo_type = aircraft.map(|a| a.ammo_type()).unwrap_or("");
            let icon_base = param
                .as_ref()
                .and_then(|p| p.species())
                .and_then(|sp| sp.known().cloned())
                .map(|sp| species_to_icon_base(sp, is_consumable, ammo_type))
                .unwrap_or_else(|| "fighter".to_string());
            let icon_dir = match category {
                PlaneCategory::Consumable => "consumables",
                PlaneCategory::Airsupport => "airsupport",
                PlaneCategory::Controllable => "controllable",
            };
            let is_consumable = matches!(category, PlaneCategory::Consumable);
            self.squadron_info.insert(*plane_id, SquadronInfo { icon_base, icon_dir, is_consumable });
        }
    }

    /// Get the armament/ammo label for a ship based on its selected weapon and ammo.
    /// Get the armament color for a ship based on its selected weapon/ammo.
    fn get_armament_color(&self, entity_id: &EntityId, controller: &dyn BattleControllerState) -> Option<[u8; 3]> {
        const COLOR_AP: [u8; 3] = [140, 200, 255]; // light blue
        const COLOR_HE: [u8; 3] = [255, 180, 80]; // orange
        const COLOR_SAP: [u8; 3] = [255, 100, 100]; // pinkish red
        const COLOR_TORP: [u8; 3] = [100, 255, 160]; // green
        const COLOR_PLANES: [u8; 3] = [200, 160, 255]; // lavender
        const COLOR_SONAR: [u8; 3] = [100, 220, 255]; // cyan

        let vehicle = controller.entities_by_id().get(entity_id)?.vehicle_ref()?;
        let vehicle = vehicle.borrow();
        let weapon = vehicle.props().selected_weapon().known()?;
        match weapon {
            WeaponType::Artillery => {
                let ammo_param_id = controller.selected_ammo().get(entity_id)?;
                let param = GameParamProvider::game_param_by_id(self.game_params, *ammo_param_id)?;
                let projectile = param.projectile()?;
                let color = match projectile.ammo_type() {
                    "AP" => COLOR_AP,
                    "HE" => COLOR_HE,
                    "CS" => COLOR_SAP,
                    _ => COLOR_AP,
                };
                Some(color)
            }
            WeaponType::Torpedoes => Some(COLOR_TORP),
            WeaponType::Planes => Some(COLOR_PLANES),
            WeaponType::Pinger => Some(COLOR_SONAR),
            WeaponType::Secondaries => Some(COLOR_HE),
        }
    }

    /// Get the depth suffix for a submarine (e.g. " (Scope)", " (30m)").
    fn get_depth_suffix(&self, entity_id: &EntityId, controller: &dyn BattleControllerState) -> Option<&'static str> {
        let vehicle = controller.entities_by_id().get(entity_id)?.vehicle_ref()?;
        let vehicle = vehicle.borrow();
        match vehicle.props().buoyancy_current_state().known()? {
            BuoyancyState::Periscope => Some(" (Scope)"),
            BuoyancyState::SemiDeepWater => Some(" (30m)"),
            BuoyancyState::DeepWater => Some(" (60m)"),
            BuoyancyState::DeepWaterInvul => Some(" (60m*)"),
            _ => None,
        }
    }

    /// Record a position in the trail history for an entity.
    pub fn record_position(
        &mut self,
        entity_id: EntityId,
        pos: map_data::MinimapPos,
        clock: GameClock,
        speed_raw: u16,
    ) {
        let history = self.position_history.entry(entity_id).or_default();
        // Deduplicate: skip if same pixel as last recorded position
        if let Some(last) = history.last()
            && last.0.x == pos.x
            && last.0.y == pos.y
        {
            return;
        }
        history.push((pos, clock, speed_raw));
    }

    /// Record ship positions from controller state without emitting draw commands.
    /// Called during replay parsing to accumulate trail history.
    /// The `filter` closure is called for each entity ID; only entities for which
    /// it returns `true` will have their positions recorded.
    pub fn record_positions(
        &mut self,
        controller: &dyn BattleControllerState,
        clock: GameClock,
        filter: impl Fn(&EntityId) -> bool,
    ) {
        let Some(map_info) = self.map_info.clone() else {
            return;
        };
        let entities = controller.entities_by_id();
        let ship_positions = controller.ship_positions();
        let minimap_positions = controller.minimap_positions();
        for (entity_id, ship_pos) in ship_positions {
            if !filter(entity_id) {
                continue;
            }
            let px = map_info.world_to_minimap(ship_pos.position, MINIMAP_SIZE);
            let speed_raw = entities
                .get(entity_id)
                .and_then(|e| e.vehicle_ref())
                .map(|v| v.borrow().props().server_speed_raw())
                .unwrap_or(0);
            self.record_position(*entity_id, px, clock, speed_raw);
        }
        for (entity_id, mm) in minimap_positions {
            if !filter(entity_id) {
                continue;
            }
            if !ship_positions.contains_key(entity_id) {
                let px = map_info.normalized_to_minimap(&mm.position, MINIMAP_SIZE);
                let speed_raw = entities
                    .get(entity_id)
                    .and_then(|e| e.vehicle_ref())
                    .map(|v| v.borrow().props().server_speed_raw())
                    .unwrap_or(0);
                self.record_position(*entity_id, px, clock, speed_raw);
            }
        }
    }

    /// Calculate team advantage from current controller state.
    ///
    /// The result is normalized so that team0 = friendly (replay owner's team)
    /// and team1 = enemy. When the replay owner is on internal team 1, all
    /// per-team values are swapped. See TEAM_ADVANTAGE_SCORING.md for details.
    fn calculate_team_advantage(&self, controller: &dyn BattleControllerState) -> crate::advantage::AdvantageResult {
        use crate::advantage::{ScoringParams, TeamState, calculate_advantage, swap_breakdown};
        use std::cell::RefCell;

        let players = controller.player_entities();
        let entities = controller.entities_by_id();
        let swap = self.self_team_id == Some(1);

        let mut teams = [TeamState::new(), TeamState::new()];

        // Scores
        let scores = controller.team_scores();
        if scores.len() >= 2 {
            teams[0].score = scores[0].score;
            teams[1].score = scores[1].score;
        }

        // Count uncontested caps per team
        for cp in controller.capture_points() {
            if !cp.is_enabled || cp.has_invaders {
                continue;
            }
            if cp.team_id == 0 {
                teams[0].uncontested_caps += 1;
            } else if cp.team_id == 1 {
                teams[1].uncontested_caps += 1;
            }
        }

        // Aggregate ship HP, counts, and per-class data
        for (entity_id, player) in players {
            let team = player.initial_state().team_id() as usize;
            if team > 1 {
                continue;
            }
            teams[team].ships_total += 1;

            // Determine ship class for per-class tracking
            let species = self.player_species.get(entity_id).map(|s| s.as_str());
            let class_count = match species {
                Some("Destroyer") => Some(&mut teams[team].destroyers),
                Some("Cruiser") => Some(&mut teams[team].cruisers),
                Some("Battleship") => Some(&mut teams[team].battleships),
                Some("Submarine") => Some(&mut teams[team].submarines),
                Some("AirCarrier") => Some(&mut teams[team].carriers),
                _ => None,
            };
            if let Some(cc) = class_count {
                cc.total += 1;
            }

            if let Some(entity) = entities.get(entity_id)
                && let Some(vehicle) = entity.vehicle_ref()
            {
                let v = RefCell::borrow(vehicle);
                let props = v.props();
                teams[team].ships_known += 1;
                teams[team].max_hp += props.max_health();
                if props.is_alive() {
                    teams[team].ships_alive += 1;
                    teams[team].total_hp += props.health();
                    // Update per-class alive counts and HP
                    let class_count = match species {
                        Some("Destroyer") => Some(&mut teams[team].destroyers),
                        Some("Cruiser") => Some(&mut teams[team].cruisers),
                        Some("Battleship") => Some(&mut teams[team].battleships),
                        Some("Submarine") => Some(&mut teams[team].submarines),
                        Some("AirCarrier") => Some(&mut teams[team].carriers),
                        _ => None,
                    };
                    if let Some(cc) = class_count {
                        cc.alive += 1;
                        cc.hp += props.health();
                        cc.max_hp += props.max_health();
                    }
                }
            }
        }

        let scoring = controller.scoring_rules().map(|r| ScoringParams {
            team_win_score: r.team_win_score,
            hold_reward: r.hold_reward,
            hold_period: r.hold_period,
        });
        let scoring = scoring.unwrap_or(ScoringParams { team_win_score: 1000, hold_reward: 3, hold_period: 5.0 });

        let mut result = calculate_advantage(&teams[0], &teams[1], &scoring, controller.time_left());

        // Normalize perspective: swap so team0 = friendly, team1 = enemy
        if swap {
            result.advantage = match result.advantage {
                crate::advantage::TeamAdvantage::Team0(level) => crate::advantage::TeamAdvantage::Team1(level),
                crate::advantage::TeamAdvantage::Team1(level) => crate::advantage::TeamAdvantage::Team0(level),
                other => other,
            };
            swap_breakdown(&mut result.breakdown);
        }
        result
    }

    /// Produce draw commands for the current frame from controller state.
    pub fn draw_frame(&mut self, controller: &dyn BattleControllerState) -> Vec<DrawCommand> {
        let Some(map_info) = self.map_info.clone() else {
            return Vec::new();
        };

        let clock = controller.clock();
        let mut commands = Vec::new();

        // 1. Score bar
        let max_score = controller.scoring_rules().map(|r| r.team_win_score as i32).unwrap_or(1000);
        if self.options.show_score {
            let scores = controller.team_scores();
            if scores.len() >= 2 {
                // Show friendly score on left (green), enemy on right (red)
                let swap = self.self_team_id == Some(1);
                let (friendly_idx, enemy_idx) = if swap { (1, 0) } else { (0, 1) };

                // Score timers: time to win from cap income
                let (team0_timer, team1_timer) = if self.options.show_score_timer {
                    let result = self.calculate_team_advantage(controller);
                    let bd = &result.breakdown;
                    let friendly_pps = if swap { bd.team1_pps } else { bd.team0_pps };
                    let enemy_pps = if swap { bd.team0_pps } else { bd.team1_pps };
                    (
                        format_score_timer(scores[friendly_idx].score, max_score as i64, friendly_pps),
                        format_score_timer(scores[enemy_idx].score, max_score as i64, enemy_pps),
                    )
                } else {
                    (None, None)
                };

                // Team advantage indicator
                let (advantage_label, advantage_team, advantage_breakdown) = if self.options.show_advantage {
                    let result = self.calculate_team_advantage(controller);
                    match result.advantage {
                        crate::advantage::TeamAdvantage::Team0(level) => {
                            (level.label().to_string(), 0, Some(result.breakdown))
                        }
                        crate::advantage::TeamAdvantage::Team1(level) => {
                            (level.label().to_string(), 1, Some(result.breakdown))
                        }
                        crate::advantage::TeamAdvantage::Even => (String::new(), -1, Some(result.breakdown)),
                    }
                } else {
                    (String::new(), -1, None)
                };

                commands.push(DrawCommand::ScoreBar {
                    team0: scores[friendly_idx].score as i32,
                    team1: scores[enemy_idx].score as i32,
                    team0_color: TEAM0_COLOR,
                    team1_color: TEAM1_COLOR,
                    max_score,
                    team0_timer,
                    team1_timer,
                    advantage_label: advantage_label.clone(),
                    advantage_team,
                });

                if let Some(breakdown) = advantage_breakdown {
                    commands.push(DrawCommand::TeamAdvantage {
                        label: advantage_label,
                        color: match advantage_team {
                            0 => TEAM0_COLOR,
                            1 => TEAM1_COLOR,
                            _ => [255, 255, 255],
                        },
                        breakdown,
                    });
                }
            }
        }

        // 1b. Team buff indicators (arms race)
        {
            let captured = controller.captured_buffs();
            if !captured.is_empty() {
                let swap = self.self_team_id == Some(1);
                let friendly_team = if swap { 1i64 } else { 0i64 };

                // Aggregate: (team_id, marker_name) -> (count, sorting)
                let mut buff_counts: HashMap<(i64, String), (u32, i64)> = HashMap::new();
                for buff in captured {
                    let drop_info =
                        GameParamProvider::game_param_by_id(self.game_params, buff.params_id).and_then(|p| {
                            let d = p.drop_data()?;
                            Some((d.marker_name_active().to_string(), d.sorting()))
                        });
                    if let Some((marker_name, sorting)) = drop_info {
                        let entry = buff_counts.entry((buff.team_id, marker_name)).or_insert((0, sorting));
                        entry.0 += 1;
                    }
                }

                // Split into friendly and enemy, sorted by sorting
                let mut friendly_buffs: Vec<(String, u32)> = Vec::new();
                let mut enemy_buffs: Vec<(String, u32)> = Vec::new();
                let mut friendly_sorted: Vec<_> =
                    buff_counts.iter().filter(|((team, _), _)| *team == friendly_team).collect();
                friendly_sorted.sort_by_key(|(_, (_, sorting))| *sorting);
                for ((_, marker), (count, _)) in &friendly_sorted {
                    friendly_buffs.push((marker.clone(), *count));
                }

                let mut enemy_sorted: Vec<_> =
                    buff_counts.iter().filter(|((team, _), _)| *team != friendly_team).collect();
                enemy_sorted.sort_by_key(|(_, (_, sorting))| *sorting);
                for ((_, marker), (count, _)) in &enemy_sorted {
                    enemy_buffs.push((marker.clone(), *count));
                }

                if !friendly_buffs.is_empty() || !enemy_buffs.is_empty() {
                    commands.push(DrawCommand::TeamBuffs { friendly_buffs, enemy_buffs });
                }
            }
        }

        // 2. Capture points (drawn early so they're behind everything)
        if self.options.show_capture_points {
            for cp in controller.capture_points() {
                if !cp.is_enabled {
                    continue;
                }
                let Some(pos) = cp.position else {
                    continue;
                };
                let px = map_info.world_to_minimap(pos, MINIMAP_SIZE);
                let px_radius = (cp.radius / map_info.space_size as f32 * MINIMAP_SIZE as f32) as i32;
                let color = cap_point_color(cp.team_id, self.self_team_id);
                let is_base = cp
                    .control_point_type
                    .as_ref()
                    .and_then(|r| r.known().copied())
                    .map(|t| {
                        matches!(
                            t,
                            ControlPointType::Base | ControlPointType::BaseWithPoints | ControlPointType::MegaBase
                        )
                    })
                    .unwrap_or(false);
                let label = if is_base {
                    "\u{2691}".to_string() // flag character (fallback if no icon)
                } else {
                    let letter = (b'A' + cp.index as u8) as char;
                    letter.to_string()
                };
                let flag_icon = if is_base {
                    let key = cap_point_flag_key(cp.team_id, self.self_team_id);
                    self.flag_icons.get(key).cloned()
                } else {
                    None
                };
                let progress = cp.progress.0 as f32;
                let invader_color = if cp.has_invaders && cp.invader_team >= 0 {
                    Some(cap_point_color(cp.invader_team, self.self_team_id))
                } else {
                    None
                };
                commands.push(DrawCommand::CapturePoint {
                    pos: px,
                    radius: px_radius.max(5),
                    color,
                    alpha: 0.15,
                    label,
                    progress,
                    invader_color,
                    flag_icon,
                });
            }
        }

        // 2a. Buff zones (arms race powerups, drawn behind ships)
        if self.options.show_capture_points {
            for bz in controller.buff_zones().values() {
                if !bz.is_active {
                    continue;
                }
                let px = map_info.world_to_minimap(bz.position, MINIMAP_SIZE);
                let px_radius = (bz.radius / map_info.space_size as f32 * MINIMAP_SIZE as f32) as i32;
                let color = cap_point_color(bz.team_id, self.self_team_id);
                let marker_name = bz.drop_params_id.and_then(|id| {
                    let param = GameParamProvider::game_param_by_id(self.game_params, id)?;
                    let drop = param.drop_data()?;
                    if bz.team_id >= 0 {
                        Some(drop.marker_name_active().to_string())
                    } else {
                        Some(drop.marker_name_inactive().to_string())
                    }
                });
                commands.push(DrawCommand::BuffZone {
                    pos: px,
                    radius: px_radius.max(5),
                    color,
                    alpha: 0.15,
                    marker_name,
                });
            }
        }

        // 2b. Position trails (drawn early so they appear behind everything else)
        if self.options.show_trails || self.options.show_speed_trails {
            let dead_ships = controller.dead_ships();
            for (entity_id, history) in &self.position_history {
                if history.len() < 2 {
                    continue;
                }
                // Skip dead ship trails if disabled
                if !self.options.show_dead_trails
                    && let Some(dead) = dead_ships.get(entity_id)
                    && clock >= dead.clock
                {
                    continue;
                }

                let player_name = self.player_names.get(entity_id).cloned();

                if self.options.show_speed_trails {
                    // Speed trail: color by serverSpeedRaw relative to observed max
                    let max_speed = history.iter().map(|(_, _, s)| *s as f32).fold(0.0f32, f32::max);

                    let points: Vec<_> = history
                        .iter()
                        .map(|(pos, _, speed_raw)| {
                            let frac =
                                if max_speed > 0.0 { (*speed_raw as f32 / max_speed).clamp(0.0, 1.0) } else { 0.0 };
                            // Cold (blue) = 0 speed, Hot (red) = max speed
                            let color = hue_to_rgb(240.0 * (1.0 - frac));
                            (*pos, color)
                        })
                        .collect();
                    commands.push(DrawCommand::PositionTrail { entity_id: *entity_id, player_name, points });
                } else {
                    // Time trail: blue (oldest) → red (newest)
                    let len = history.len();
                    let points: Vec<_> = history
                        .iter()
                        .enumerate()
                        .map(|(i, (pos, _, _))| {
                            let frac = i as f32 / (len - 1) as f32;
                            let color = hue_to_rgb(240.0 * (1.0 - frac));
                            (*pos, color)
                        })
                        .collect();
                    commands.push(DrawCommand::PositionTrail { entity_id: *entity_id, player_name, points });
                }
            }
        }

        // 3. Artillery shot tracers
        if self.options.show_tracers {
            for shot in controller.active_shots() {
                let owner = shot.salvo.owner_id;
                let relation = self.player_relations.get(&owner).copied().unwrap_or(Relation::new(2));
                let color = ship_color_rgb(relation, self.division_mates.contains(&owner));
                for shot_data in &shot.salvo.shots {
                    let origin = shot_data.origin;
                    let target = shot_data.target;
                    let dx = target.x - origin.x;
                    let dz = target.z - origin.z;
                    let distance = (dx * dx + dz * dz).sqrt();
                    let flight_duration = if shot_data.speed > 0.0 { distance / shot_data.speed } else { 3.0 };

                    let elapsed = clock - shot.fired_at;
                    if elapsed < 0.0 || elapsed > flight_duration {
                        continue;
                    }
                    let frac = elapsed / flight_duration;
                    let head = origin.lerp(target, frac);
                    let tail = origin.lerp(target, (frac - TRACER_LEN).max(0.0));
                    commands.push(DrawCommand::ShotTracer {
                        from: map_info.world_to_minimap(tail, MINIMAP_SIZE),
                        to: map_info.world_to_minimap(head, MINIMAP_SIZE),
                        color,
                    });
                }
            }
        }

        // 3. Torpedoes
        if self.options.show_torpedoes {
            let half_space = map_info.space_size as f32 / 2.0;
            for torp in controller.active_torpedoes() {
                let elapsed = clock - torp.updated_at;
                if elapsed < 0.0 {
                    continue;
                }
                let world = torpedo_position(&torp.torpedo, elapsed);
                if world.x.abs() > half_space || world.z.abs() > half_space {
                    continue;
                }
                let relation = self.player_relations.get(&torp.torpedo.owner_id).copied().unwrap_or(Relation::new(2));
                let is_div = self.division_mates.contains(&torp.torpedo.owner_id);
                let color = ship_color_rgb(relation, is_div);
                commands.push(DrawCommand::Torpedo { pos: map_info.world_to_minimap(world, MINIMAP_SIZE), color });
            }
        }

        // 4. Smoke screens
        if self.options.show_smoke {
            for entity in controller.entities_by_id().values() {
                if let Some(smoke_ref) = entity.smoke_screen_ref() {
                    let smoke = smoke_ref.borrow();
                    let px_radius = (smoke.radius.value() / map_info.space_size as f32 * MINIMAP_SIZE as f32) as i32;
                    for point in &smoke.points {
                        let px = map_info.world_to_minimap(*point, MINIMAP_SIZE);
                        commands.push(DrawCommand::Smoke {
                            pos: px,
                            radius: px_radius.max(3),
                            color: SMOKE_COLOR,
                            alpha: SMOKE_ALPHA,
                        });
                    }
                }
            }
        }

        // 5. Weather zones
        if self.options.show_weather {
            for zone in controller.local_weather_zones() {
                let px = map_info.world_to_minimap(zone.position, MINIMAP_SIZE);
                let px_radius = (zone.radius / map_info.space_size as f32 * MINIMAP_SIZE as f32) as i32;
                commands.push(DrawCommand::WeatherZone { pos: px, radius: px_radius.max(5) });
            }
        }

        // 6. Buildings
        if self.options.show_buildings {
            for entity in controller.entities_by_id().values() {
                if let Some(building_ref) = entity.building_ref() {
                    let building = building_ref.borrow();
                    if building.is_hidden {
                        continue;
                    }
                    let px = map_info.world_to_minimap(building.position, MINIMAP_SIZE);
                    let color = if building.is_alive {
                        cap_point_color(building.team_id as i64, self.self_team_id)
                    } else {
                        [40, 40, 40]
                    };
                    commands.push(DrawCommand::Building { pos: px, color, is_alive: building.is_alive });
                }
            }
        }

        // 6. Ships
        let ship_positions = controller.ship_positions();
        let minimap_positions = controller.minimap_positions();

        // Collect all entity IDs that have either world or minimap positions
        let mut all_ship_ids: Vec<EntityId> = ship_positions.keys().chain(minimap_positions.keys()).copied().collect();
        all_ship_ids.sort();
        all_ship_ids.dedup();

        let dead_ships = controller.dead_ships();

        // Lazily resolve species for non-player vehicle entities (e.g. NPC enemies
        // in Operations) by looking up ship_params_id from shipConfig in GameParams.
        let entities = controller.entities_by_id();
        for entity_id in &all_ship_ids {
            if !self.player_species.contains_key(entity_id)
                && let Some(vehicle_ref) = entities.get(entity_id).and_then(|e| e.vehicle_ref())
            {
                let ship_id = vehicle_ref.borrow().props().ship_config().ship_params_id();
                if ship_id.raw() != 0
                    && let Some(param) = GameParamProvider::game_param_by_id(self.game_params, ship_id)
                    && let Some(species) = param.species().and_then(|s| s.known())
                {
                    self.player_species.insert(*entity_id, species.name().to_string());
                }
            }
        }

        for entity_id in &all_ship_ids {
            // Skip dead ships (they get an X marker below)
            if let Some(dead) = dead_ships.get(entity_id)
                && clock >= dead.clock
            {
                continue;
            }

            let relation = self.player_relations.get(entity_id).copied().unwrap_or(Relation::new(2));
            let color = ship_color_rgb(relation, self.division_mates.contains(entity_id));
            let species = self.player_species.get(entity_id).cloned();
            let player_name =
                if self.options.show_player_names { self.player_names.get(entity_id).cloned() } else { None };
            let ship_name = if self.options.show_ship_names {
                let base = self.ship_display_names.get(entity_id).cloned();
                // Append depth suffix for submarines
                match (base, self.get_depth_suffix(entity_id, controller)) {
                    (Some(name), Some(suffix)) => Some(format!("{}{}", name, suffix)),
                    (base, _) => base,
                }
            } else {
                None
            };

            let name_color =
                if self.options.show_armament { self.get_armament_color(entity_id, controller) } else { None };

            let minimap = minimap_positions.get(entity_id);
            let world = ship_positions.get(entity_id);
            let detected = minimap.map(|m| m.visible).unwrap_or(false);

            // visibility_flags from the Vehicle entity: bitmask of detection
            // reasons (radar, hydro, direct vision, etc.). Non-zero means the
            // ship is confirmed detected through game mechanics.
            let vis_flags = controller
                .entities_by_id()
                .get(entity_id)
                .and_then(|e| e.vehicle_ref())
                .map(|v| v.borrow().props().visibility_flags())
                .unwrap_or(0);

            // Get health fraction from entity
            let health_fraction =
                controller.entities_by_id().get(entity_id).and_then(|e| e.vehicle_ref()).and_then(|v| {
                    let v = v.borrow();
                    let max = v.props().max_health();
                    if max > 0.0 { Some((v.props().health() / max).clamp(0.0, 1.0)) } else { None }
                });

            // Compute yaw: prefer minimap heading (more accurate for icon rotation)
            let minimap_yaw = minimap.map(|mm| std::f32::consts::FRAC_PI_2 - mm.heading.to_radians());
            let world_yaw = world.map(|sp| sp.yaw);

            // A ship is "spotted" when its visibility_flags are non-zero (game mechanic)
            let is_spotted = vis_flags != 0;

            // Detected teammate = spotted ally (not self)
            let is_detected_teammate = is_spotted && !relation.is_enemy();

            if detected {
                let yaw = minimap_yaw.or(world_yaw).unwrap_or(0.0);
                if let Some(mm) = minimap {
                    // Use minimap position — it's authoritative for the minimap view
                    // and avoids stale world positions from previous detections.
                    let px = map_info.normalized_to_minimap(&mm.position, MINIMAP_SIZE);
                    let speed_raw = controller
                        .entities_by_id()
                        .get(entity_id)
                        .and_then(|e| e.vehicle_ref())
                        .map(|v| v.borrow().props().server_speed_raw())
                        .unwrap_or(0);
                    self.record_position(*entity_id, px, clock, speed_raw);
                    commands.push(DrawCommand::Ship {
                        entity_id: *entity_id,
                        pos: px,
                        yaw,
                        species: species.clone(),
                        color: Some(color),
                        visibility: ShipVisibility::Visible,
                        opacity: 1.0,
                        is_self: relation.is_self(),
                        player_name: player_name.clone(),
                        ship_name: ship_name.clone(),
                        is_detected_teammate,
                        name_color,
                    });
                    if self.options.show_hp_bars
                        && let Some(frac) = health_fraction
                    {
                        let fill_color = hp_bar_color(frac);
                        commands.push(DrawCommand::HealthBar {
                            entity_id: *entity_id,
                            pos: px,
                            fraction: frac,
                            fill_color,
                            background_color: HP_BAR_BG_COLOR,
                            background_alpha: HP_BAR_BG_ALPHA,
                        });
                    }
                }
            } else {
                // Undetected — use minimap position (last known)
                let yaw = minimap_yaw.or(world_yaw).unwrap_or(0.0);
                let px = if let Some(mm) = minimap {
                    map_info.normalized_to_minimap(&mm.position, MINIMAP_SIZE)
                } else {
                    continue;
                };
                commands.push(DrawCommand::Ship {
                    entity_id: *entity_id,
                    pos: px,
                    yaw,
                    species: species.clone(),
                    color: None,
                    visibility: ShipVisibility::Undetected,
                    opacity: UNDETECTED_OPACITY,
                    is_self: relation.is_self(),
                    player_name: None,
                    ship_name: None,
                    is_detected_teammate: false,
                    name_color: None,
                });
            }
        }

        // 6. Turret direction indicators (from targetLocalPos EntityProperty)
        if self.options.show_turret_direction {
            let target_yaws = controller.target_yaws();
            for (entity_id, &world_yaw) in target_yaws {
                // Skip dead ships
                if let Some(dead) = dead_ships.get(entity_id)
                    && clock >= dead.clock
                {
                    continue;
                }
                // Skip undetected ships — aim data is stale
                let detected = minimap_positions.get(entity_id).map(|m| m.visible).unwrap_or(false);
                if !detected {
                    continue;
                }
                // Need a position for this ship
                let px = if let Some(mm) = minimap_positions.get(entity_id) {
                    map_info.normalized_to_minimap(&mm.position, MINIMAP_SIZE)
                } else {
                    continue;
                };
                // targetLocalPos yaw is compass bearing (0=north, CW positive).
                // Convert to screen math coords: screen_yaw = PI/2 - compass_yaw
                let screen_yaw = std::f32::consts::FRAC_PI_2 - world_yaw;
                let relation = self.player_relations.get(entity_id).copied().unwrap_or(Relation::new(2));
                let color = ship_color_rgb(relation, self.division_mates.contains(entity_id));
                commands.push(DrawCommand::TurretDirection {
                    entity_id: *entity_id,
                    pos: px,
                    yaw: screen_yaw,
                    color,
                    length: 18,
                });
            }
        }

        // 7. Dead ship markers
        for (entity_id, dead) in dead_ships {
            if clock >= dead.clock {
                let px = map_info.world_to_minimap(dead.position, MINIMAP_SIZE);
                let species = self.player_species.get(entity_id).cloned();
                // Use last known heading from minimap positions
                let yaw = minimap_positions
                    .get(entity_id)
                    .map(|mm| std::f32::consts::FRAC_PI_2 - mm.heading.to_radians())
                    .or_else(|| ship_positions.get(entity_id).map(|sp| sp.yaw))
                    .unwrap_or(0.0);
                let relation = self.player_relations.get(entity_id).copied().unwrap_or(Relation::new(2));
                let player_name =
                    if self.options.show_player_names { self.player_names.get(entity_id).cloned() } else { None };
                let ship_name =
                    if self.options.show_ship_names { self.ship_display_names.get(entity_id).cloned() } else { None };
                commands.push(DrawCommand::DeadShip {
                    entity_id: *entity_id,
                    pos: px,
                    yaw,
                    species,
                    color: None,
                    is_self: relation.is_self(),
                    player_name,
                    ship_name,
                });
            }
        }

        // 7. Planes
        if self.options.show_planes {
            for (plane_id, plane) in controller.active_planes() {
                let px = map_info.world_to_minimap(plane.position.to_world_pos(), MINIMAP_SIZE);

                let info = self.squadron_info.get(plane_id);
                // Use player_relations to determine if the plane is enemy.
                // PlaneId::owner_id() extracts the ship entity_id from the packed plane ID.
                let owner_entity = plane.plane_id.owner_id();
                let is_enemy = self.player_relations.get(&owner_entity).map(|r| r.is_enemy()).unwrap_or_else(|| {
                    // Fallback: compare plane's absolute team_id against self player's team
                    self.self_team_id.map(|self_team| plane.team_id != self_team as u32).unwrap_or(false)
                });

                let icon_base = info.map(|i| i.icon_base.as_str()).unwrap_or("fighter");
                let icon_dir = info.map(|i| i.icon_dir).unwrap_or("consumables");
                let suffix = if is_enemy { "enemy" } else { "ally" };
                let icon_key = format!("{}/{}_{}", icon_dir, icon_base, suffix);

                // Draw patrol circle from ward data (if this plane has an active ward)
                if let Some(ward) = controller.active_wards().get(plane_id) {
                    let ward_px = map_info.world_to_minimap(ward.position, MINIMAP_SIZE);
                    let space_size = map_info.space_size as f32;
                    let px_radius = (ward.radius.value() / space_size * MINIMAP_SIZE as f32) as i32;
                    let color = if is_enemy { TEAM1_COLOR } else { TEAM0_COLOR };
                    commands.push(DrawCommand::PatrolRadius {
                        plane_id: *plane_id,
                        pos: ward_px,
                        radius_px: px_radius,
                        color,
                        alpha: 0.12,
                    });
                }

                // Skip labels for consumable planes (catapult fighters, spotters) — too noisy
                let is_consumable = info.map(|i| i.is_consumable).unwrap_or(false);
                let player_name = if self.options.show_player_names && !is_consumable {
                    self.player_names.get(&owner_entity).cloned()
                } else {
                    None
                };
                let ship_name = if self.options.show_ship_names && !is_consumable {
                    self.ship_display_names.get(&owner_entity).cloned()
                } else {
                    None
                };

                commands.push(DrawCommand::Plane {
                    plane_id: *plane_id,
                    owner_entity_id: owner_entity,
                    pos: px,
                    icon_key,
                    player_name,
                    ship_name,
                });
            }
        }

        // 8. Active consumables
        if self.options.show_consumables {
            let all_consumables = controller.active_consumables();
            for (entity_id, consumables) in all_consumables {
                // Skip dead ships
                if let Some(dead) = dead_ships.get(entity_id)
                    && clock >= dead.clock
                {
                    continue;
                }
                // Skip ships not currently visible on the minimap
                let visible = minimap_positions.get(entity_id).map(|m| m.visible).unwrap_or(false);
                if !visible {
                    continue;
                }
                // Get ship position (prefer world position, fall back to minimap)
                let pos = if let Some(sp) = ship_positions.get(entity_id) {
                    Some(map_info.world_to_minimap(sp.position, MINIMAP_SIZE))
                } else {
                    minimap_positions
                        .get(entity_id)
                        .map(|mm| map_info.normalized_to_minimap(&mm.position, MINIMAP_SIZE))
                };
                let Some(pos) = pos else { continue };

                let relation = self.player_relations.get(entity_id).copied().unwrap_or(Relation::new(2));
                let is_friendly = relation.is_self() || relation.is_ally();

                // Check if this entity has an HP bar rendered
                let has_hp_bar = self.options.show_hp_bars
                    && controller
                        .entities_by_id()
                        .get(entity_id)
                        .and_then(|e| e.vehicle_ref())
                        .map(|v| {
                            let v = v.borrow();
                            v.props().max_health() > 0.0
                        })
                        .unwrap_or(false);

                let mut icon_keys = Vec::new();
                for active in consumables {
                    let still_active = clock.seconds() < active.activated_at.seconds() + active.duration;
                    let past_start = clock.seconds() >= active.activated_at.seconds();
                    if still_active && past_start {
                        // Collect icon key
                        if let Some(icon_key) = self.consumable_icon_key(*entity_id, active.consumable.clone()) {
                            icon_keys.push(icon_key);
                        }

                        // Emit radius for detection consumables (radar, hydro, hydrophone)
                        // Skip fighter consumables — their patrol radius is drawn at the plane position, not the ship
                        if matches!(
                            active.consumable.known(),
                            Some(Consumable::CallFighters | Consumable::CatapultFighter)
                        ) {
                            // no detection radius for fighters
                        } else if let Some(radius) = self.get_consumable_radius(*entity_id, active.consumable.clone()) {
                            let space_size = map_info.space_size as f32;
                            let px_radius = (radius.value() / 30.0 / space_size * MINIMAP_SIZE as f32) as i32;
                            let color = consumable_radius_color(&active.consumable, is_friendly);
                            commands.push(DrawCommand::ConsumableRadius {
                                entity_id: *entity_id,
                                pos,
                                radius_px: px_radius,
                                color,
                                alpha: 0.15,
                            });
                        }
                    }
                }

                if !icon_keys.is_empty() {
                    commands.push(DrawCommand::ConsumableIcons {
                        entity_id: *entity_id,
                        pos,
                        icon_keys,
                        is_friendly,
                        has_hp_bar,
                    });
                }
            }
        }

        // 8b. Ship config circles (detection, main battery, secondary, radar, hydro, torpedo)
        if self.options.show_ship_config {
            for entity_id in &all_ship_ids {
                // Skip dead ships
                if let Some(dead) = dead_ships.get(entity_id)
                    && clock >= dead.clock
                {
                    continue;
                }

                // Get ship position
                let pos = if let Some(ship_pos) = ship_positions.get(entity_id) {
                    map_info.world_to_minimap(ship_pos.position, MINIMAP_SIZE)
                } else if let Some(mm) = minimap_positions.get(entity_id) {
                    map_info.normalized_to_minimap(&mm.position, MINIMAP_SIZE)
                } else {
                    continue;
                };

                let Some(player_name) = self.player_names.get(entity_id) else {
                    continue;
                };
                let player_name = player_name.clone();
                let is_self = self.player_relations.get(entity_id).map(|r| r.is_self()).unwrap_or(false);

                let Some(&ship_param_id) = self.ship_param_ids.get(entity_id) else {
                    continue;
                };
                let Some(ship_param) = GameParamProvider::game_param_by_id(self.game_params, ship_param_id) else {
                    continue;
                };
                let Some(vehicle) = ship_param.vehicle() else {
                    continue;
                };
                let species = ship_param.species().and_then(|s| s.known()).cloned();

                // Get vehicle entity for ship config (modernizations, skills)
                let vehicle_entity = controller.entities_by_id().get(entity_id).and_then(|e| e.vehicle_ref());

                // Look up the equipped hull upgrade name from replay data
                let hull_name = vehicle_entity.as_ref().and_then(|v| {
                    let v = v.borrow();
                    let hull_id = v.props().ship_config().hull();
                    GameParamProvider::game_param_by_id(self.game_params, hull_id).map(|p| p.name().to_string())
                });

                // Use Vehicle::resolve_ranges to get all range data
                let mut ranges = vehicle.resolve_ranges(Some(self.game_params), hull_name.as_deref(), self.version);

                // Apply build modifiers (modernizations + captain skills)
                if let Some(ref species) = species {
                    let mut vis_coeff: f32 = 1.0;
                    let mut gm_max_dist: f32 = 1.0;
                    let mut gs_max_dist: f32 = 1.0;

                    if let Some(v_ref) = &vehicle_entity {
                        let v = v_ref.borrow();

                        // Modernization modifiers
                        for mod_id in v.props().ship_config().modernization() {
                            let Some(mod_param) = GameParamProvider::game_param_by_id(self.game_params, *mod_id) else {
                                continue;
                            };
                            let Some(modernization) = mod_param.modernization() else {
                                continue;
                            };
                            for modifier in modernization.modifiers() {
                                match modifier.name() {
                                    "visibilityDistCoeff" => vis_coeff *= modifier.get_for_species(species),
                                    "GMMaxDist" => gm_max_dist *= modifier.get_for_species(species),
                                    "GSMaxDist" => gs_max_dist *= modifier.get_for_species(species),
                                    _ => {}
                                }
                            }
                        }

                        // Captain skill modifiers
                        let crew_params = v.props().crew_modifiers_compact_params();
                        if let Some(crew_param) =
                            GameParamProvider::game_param_by_id(self.game_params, crew_params.params_id())
                            && let Some(crew) = crew_param.crew()
                        {
                            for &skill_id in crew_params.learned_skills().for_species(species) {
                                let Some(skill) = crew.skill_by_type(skill_id as u32) else {
                                    continue;
                                };
                                let Some(modifiers) = skill.modifiers() else {
                                    continue;
                                };
                                for modifier in modifiers {
                                    match modifier.name() {
                                        "visibilityDistCoeff" => vis_coeff *= modifier.get_for_species(species),
                                        "GMMaxDist" => gm_max_dist *= modifier.get_for_species(species),
                                        "GSMaxDist" => gs_max_dist *= modifier.get_for_species(species),
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }

                    // Apply coefficients
                    ranges.detection_km = ranges.detection_km.map(|km| km * vis_coeff);
                    ranges.air_detection_km = ranges.air_detection_km.map(|km| km * vis_coeff);
                    ranges.main_battery_m = ranges.main_battery_m.map(|m| m * gm_max_dist);
                    ranges.secondary_battery_m = ranges.secondary_battery_m.map(|m| m * gs_max_dist);
                }

                let space_size = map_info.space_size as f32;

                // Helper: convert meters to minimap pixel radius
                let meters_to_px = |m: f32| -> f32 { m / 30.0 / space_size * MINIMAP_SIZE as f32 };

                // Helper: convert km to minimap pixel radius
                let km_to_px = |km: f32| -> f32 { km * 1000.0 / 30.0 / space_size * MINIMAP_SIZE as f32 };

                // Check visibility filter for this ship
                if let Some(filter) = self.options.ship_config_visibility.filter_for(is_self, *entity_id) {
                    // Detection circle
                    if let Some(detection_km) = ranges.detection_km
                        && filter.detection
                    {
                        commands.push(DrawCommand::ShipConfigCircle {
                            entity_id: *entity_id,
                            pos,
                            radius_px: km_to_px(detection_km.value()),
                            color: [135, 206, 235], // light blue
                            alpha: 0.6,
                            dashed: true,
                            label: Some(format!("{:.1} km", detection_km.value())),
                            kind: ShipConfigCircleKind::Detection,
                            player_name: player_name.clone(),
                            is_self,
                        });
                    }

                    // Main battery range
                    if let Some(main_battery_m) = ranges.main_battery_m
                        && filter.main_battery
                    {
                        commands.push(DrawCommand::ShipConfigCircle {
                            entity_id: *entity_id,
                            pos,
                            radius_px: meters_to_px(main_battery_m.value()),
                            color: [180, 180, 180], // light gray
                            alpha: 0.5,
                            dashed: false,
                            label: Some(format!("{:.1} km", main_battery_m.to_km().value())),
                            kind: ShipConfigCircleKind::MainBattery,
                            player_name: player_name.clone(),
                            is_self,
                        });
                    }

                    // Secondary battery range
                    if let Some(secondary_m) = ranges.secondary_battery_m
                        && filter.secondary_battery
                    {
                        commands.push(DrawCommand::ShipConfigCircle {
                            entity_id: *entity_id,
                            pos,
                            radius_px: meters_to_px(secondary_m.value()),
                            color: [255, 165, 0], // orange
                            alpha: 0.5,
                            dashed: false,
                            label: Some(format!("{:.1} km", secondary_m.to_km().value())),
                            kind: ShipConfigCircleKind::SecondaryBattery,
                            player_name: player_name.clone(),
                            is_self,
                        });
                    }

                    // Torpedo range
                    if let Some(torpedo_m) = ranges.torpedo_range_m
                        && filter.torpedo
                    {
                        commands.push(DrawCommand::ShipConfigCircle {
                            entity_id: *entity_id,
                            pos,
                            radius_px: meters_to_px(torpedo_m.value()),
                            color: [0, 200, 200], // cyan/teal
                            alpha: 0.5,
                            dashed: false,
                            label: Some(format!("{:.1} km", torpedo_m.to_km().value())),
                            kind: ShipConfigCircleKind::TorpedoRange,
                            player_name: player_name.clone(),
                            is_self,
                        });
                    }

                    // Radar range
                    if let Some(radar_m) = ranges.radar_m
                        && filter.radar
                    {
                        commands.push(DrawCommand::ShipConfigCircle {
                            entity_id: *entity_id,
                            pos,
                            radius_px: meters_to_px(radar_m.value()),
                            color: [255, 255, 100], // yellow
                            alpha: 0.5,
                            dashed: false,
                            label: Some(format!("{:.1} km", radar_m.to_km().value())),
                            kind: ShipConfigCircleKind::Radar,
                            player_name: player_name.clone(),
                            is_self,
                        });
                    }

                    // Hydro range
                    if let Some(hydro_m) = ranges.hydro_m
                        && filter.hydro
                    {
                        commands.push(DrawCommand::ShipConfigCircle {
                            entity_id: *entity_id,
                            pos,
                            radius_px: meters_to_px(hydro_m.value()),
                            color: [100, 255, 100], // green
                            alpha: 0.5,
                            dashed: false,
                            label: Some(format!("{:.1} km", hydro_m.to_km().value())),
                            kind: ShipConfigCircleKind::Hydro,
                            player_name: player_name.clone(),
                            is_self,
                        });
                    }
                }
            }
        }

        // 9. Kill feed
        if self.options.show_kill_feed {
            let kills = controller.kills();
            let mut recent_kills = Vec::new();
            for kill in kills.iter().rev() {
                if clock >= kill.clock && clock <= kill.clock + KILL_FEED_DURATION {
                    let killer_name =
                        self.player_names.get(&kill.killer).cloned().unwrap_or_else(|| format!("#{}", kill.killer));
                    let victim_name =
                        self.player_names.get(&kill.victim).cloned().unwrap_or_else(|| format!("#{}", kill.victim));
                    let killer_relation = self.player_relations.get(&kill.killer).copied().unwrap_or(Relation::new(2));
                    let victim_relation = self.player_relations.get(&kill.victim).copied().unwrap_or(Relation::new(2));
                    recent_kills.push(KillFeedEntry {
                        killer_name,
                        killer_species: self.player_species.get(&kill.killer).cloned(),
                        killer_ship_name: self.ship_display_names.get(&kill.killer).cloned(),
                        killer_color: ship_color_rgb(killer_relation, self.division_mates.contains(&kill.killer)),
                        victim_name,
                        victim_species: self.player_species.get(&kill.victim).cloned(),
                        victim_ship_name: self.ship_display_names.get(&kill.victim).cloned(),
                        victim_color: ship_color_rgb(victim_relation, self.division_mates.contains(&kill.victim)),
                        cause: kill.cause.clone(),
                    });
                    if recent_kills.len() >= 5 {
                        break;
                    }
                }
            }
            if !recent_kills.is_empty() {
                recent_kills.reverse();
                commands.push(DrawCommand::KillFeed { entries: recent_kills });
            }
        }

        // 9b. Chat overlay
        if self.options.show_chat {
            let chat = controller.game_chat();
            let fade_duration = 5.0f32; // seconds to fade out
            let visible_duration = 30.0f32; // seconds before fading starts
            let max_messages = 10usize;

            let mut chat_entries = Vec::new();
            for msg in chat.iter().rev() {
                let age = clock.seconds() - msg.clock.seconds();
                if age < 0.0 {
                    continue;
                }
                let total_visible = visible_duration + fade_duration;
                if age > total_visible {
                    continue;
                }
                let opacity = if age > visible_duration {
                    1.0 - ((age - visible_duration) / fade_duration).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let sender_entity = msg.player.as_ref().map(|p| p.initial_state().entity_id());
                let is_div_mate = sender_entity.map(|eid| self.division_mates.contains(&eid)).unwrap_or(false);
                let team_color = msg.sender_relation.map(|r| ship_color_rgb(r, is_div_mate)).unwrap_or([255, 255, 255]);
                let (clan_tag, clan_color, ship_species, ship_name) = if let Some(ref player) = msg.player {
                    let state = player.initial_state();
                    let tag = state.clan().to_string();
                    let color_raw = state.clan_color();
                    let color = if color_raw != 0 {
                        Some([
                            ((color_raw & 0xFF0000) >> 16) as u8,
                            ((color_raw & 0xFF00) >> 8) as u8,
                            (color_raw & 0xFF) as u8,
                        ])
                    } else {
                        None
                    };
                    let species = player.vehicle().species().and_then(species_key);
                    let name = self.game_params.localized_name_from_param(player.vehicle()).map(|s| s.to_string());
                    (tag, color, species, name)
                } else {
                    (String::new(), None, None, None)
                };
                let message_color = match msg.channel {
                    ChatChannel::Division => [255, 215, 0], // gold
                    ChatChannel::Team => [140, 255, 140],   // light green
                    ChatChannel::Global => [255, 255, 255], // white
                    _ => [200, 200, 200],                   // gray fallback
                };
                let font_hint = self
                    .fonts
                    .as_ref()
                    .and_then(|f| f.font_hint_for_text(&msg.message))
                    .map(FontHint::Fallback)
                    .unwrap_or(FontHint::Primary);
                chat_entries.push(ChatEntry {
                    clan_tag,
                    clan_color,
                    player_name: msg.sender_name.clone(),
                    team_color,
                    ship_species,
                    ship_name,
                    message: msg.message.clone(),
                    message_color,
                    opacity,
                    font_hint,
                });
                if chat_entries.len() >= max_messages {
                    break;
                }
            }
            if !chat_entries.is_empty() {
                chat_entries.reverse();
                commands.push(DrawCommand::ChatOverlay { entries: chat_entries });
            }
        }

        // 10. Timer / Pre-battle countdown
        if self.options.show_timer {
            let stage = controller.battle_stage();

            match stage {
                Some(BattleStage::Battle) => {
                    // BattleStage::Battle (raw value 1) = pre-battle countdown period
                    if let Some(time_left) = controller.time_left()
                        && time_left > 0
                    {
                        commands.push(DrawCommand::PreBattleCountdown { seconds: time_left });
                    }
                }
                _ => {
                    // BattleStage::Waiting (raw value 0) = battle active, or stage unknown
                    let elapsed = clock.to_elapsed(controller.battle_start_clock().unwrap_or(GameClock(0.0)));
                    commands.push(DrawCommand::Timer { time_remaining: controller.time_left(), elapsed });
                }
            }
        }

        // 11. Battle result overlay (shown as soon as winner is known)
        if let Some(wt) = controller.winning_team() {
            let (text, color) = match (self.self_team_id, wt) {
                (Some(self_t), wt) if wt >= 0 && wt == self_t as i8 => {
                    ("VICTORY".to_string(), [76, 232, 170]) // green
                }
                (Some(_), wt) if wt >= 0 => {
                    ("DEFEAT".to_string(), [254, 77, 42]) // red
                }
                _ => ("DRAW".to_string(), [255, 165, 0]), // orange
            };
            let subtitle = controller.finish_type().map(|ft| finish_type_description(ft).to_uppercase());
            commands.push(DrawCommand::BattleResultOverlay { text, subtitle, color, subtitle_above: false });
        }

        commands
    }
}

/// Human-readable description for how the battle ended.
fn finish_type_description(ft: &Recognized<FinishType>) -> String {
    match ft.known() {
        Some(FinishType::Extermination) => "All enemy ships destroyed".into(),
        Some(FinishType::BaseCaptured) => "Base captured".into(),
        Some(FinishType::Timeout) => "Time expired".into(),
        Some(FinishType::Score) => "Score limit reached".into(),
        Some(FinishType::ScoreOnTimeout) => "Leading on points at timeout".into(),
        Some(FinishType::ScoreZero) => "Points depleted".into(),
        Some(FinishType::ScoreExcess) => "Score limit exceeded".into(),
        Some(FinishType::Failure) => "Mission failed".into(),
        Some(FinishType::Technical) => "Technical finish".into(),
        Some(FinishType::PveMainTaskSucceeded) => "Mission accomplished".into(),
        Some(FinishType::PveMainTaskFailed) => "Mission failed".into(),
        _ => "Battle ended".into(),
    }
}

/// Format time-to-win as "M:SS" or "-:--" if no cap income.
fn format_score_timer(current_score: i64, win_score: i64, pps: f64) -> Option<String> {
    let remaining = win_score - current_score;
    if remaining <= 0 {
        return Some("0:00".to_string());
    }
    if pps <= 0.0 {
        return Some("-:--".to_string());
    }
    let seconds = (remaining as f64 / pps).ceil() as i64;
    let mins = seconds / 60;
    let secs = seconds % 60;
    Some(format!("{}:{:02}", mins, secs))
}

/// Get the capture point / building color relative to the recording player.
///
/// `team_id` is the raw game team (0 or 1), `self_team_id` is the recording player's
/// raw team. Same team = green (friendly), other team = red (enemy), neutral = white.
/// Map a capture point's team_id to a flag icon key ("ally", "enemy", or "neutral").
fn cap_point_flag_key(team_id: i64, self_team_id: Option<i64>) -> &'static str {
    if team_id < 0 {
        return "neutral";
    }
    match self_team_id {
        Some(self_team) if team_id == self_team => "ally",
        Some(_) => "enemy",
        None => "neutral",
    }
}

fn cap_point_color(team_id: i64, self_team_id: Option<i64>) -> [u8; 3] {
    if team_id < 0 {
        return [255, 255, 255]; // neutral
    }
    match self_team_id {
        Some(self_team) if team_id == self_team => TEAM0_COLOR, // friendly
        Some(_) => TEAM1_COLOR,                                 // enemy
        None => {
            // Fallback before we know self_team_id: use raw mapping
            match team_id {
                0 => TEAM0_COLOR,
                _ => TEAM1_COLOR,
            }
        }
    }
}

/// Get the ship color as an RGB array based on relation and division membership.
fn ship_color_rgb(relation: Relation, is_division_mate: bool) -> [u8; 3] {
    if relation.is_self() {
        [255, 255, 255]
    } else if is_division_mate {
        [255, 215, 0] // Gold
    } else if relation.is_ally() {
        [76, 232, 170]
    } else {
        [254, 77, 42]
    }
}

/// Get the health bar fill color based on health fraction.
fn hp_bar_color(fraction: f32) -> [u8; 3] {
    if fraction > 0.66 {
        HP_BAR_FULL_COLOR
    } else if fraction > 0.33 {
        HP_BAR_MID_COLOR
    } else {
        HP_BAR_LOW_COLOR
    }
}

/// Convert HSV hue (0-360) to RGB with full saturation and value.
/// Used for position trail rainbow coloring (240=blue → 0=red).
fn hue_to_rgb(hue: f32) -> [u8; 3] {
    let h = hue / 60.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let q = (1.0 - f) * 255.0;
    let t = f * 255.0;
    match i % 6 {
        0 => [255, t as u8, 0],
        1 => [q as u8, 255, 0],
        2 => [0, 255, t as u8],
        3 => [0, q as u8, 255],
        4 => [t as u8, 0, 255],
        _ => [255, 0, q as u8],
    }
}

fn species_key(species: &Recognized<Species>) -> Option<String> {
    species.known().map(|s| s.name()).or_else(|| species.unknown().map(String::as_str)).map(String::from)
}

/// Compute the world position of a torpedo at `elapsed` seconds after launch.
///
/// For straight-running torpedoes: `origin + direction * elapsed`.
/// For maneuvering (S-turn) torpedoes: the torpedo turns from its initial heading
/// toward `target_yaw` at `yaw_speed` rad/s, then continues straight.
/// The position during the turn is computed analytically via arc integration.
fn torpedo_position(torp: &TorpedoData, elapsed: f32) -> WorldPos {
    let maneuver = match torp.maneuver_dump {
        Some(ref m) => m,
        None => return torp.origin + torp.direction * elapsed,
    };

    let speed = (torp.direction.x * torp.direction.x + torp.direction.z * torp.direction.z).sqrt();
    if speed < 1e-6 {
        return torp.origin;
    }

    let initial_yaw = torp.direction.x.atan2(torp.direction.z);
    let yaw_delta = maneuver.target_yaw - initial_yaw;
    if yaw_delta.abs() < 1e-6 || maneuver.yaw_speed.abs() < 1e-6 {
        // No actual turn needed
        return torp.origin + torp.direction * elapsed;
    }

    let sign: f32 = if yaw_delta > 0.0 { 1.0 } else { -1.0 };
    let w = sign * maneuver.yaw_speed; // signed angular velocity
    let turn_dur = yaw_delta.abs() / maneuver.yaw_speed;

    if elapsed < turn_dur {
        // During the turn: analytical arc integral
        // x(t) = ox + (speed/w) * (-cos(initial_yaw + w*t) + cos(initial_yaw))
        // z(t) = oz + (speed/w) * ( sin(initial_yaw + w*t) - sin(initial_yaw))
        let ratio = speed / w;
        let yaw_t = initial_yaw + w * elapsed;
        WorldPos {
            x: torp.origin.x + ratio * (-yaw_t.cos() + initial_yaw.cos()),
            y: torp.origin.y,
            z: torp.origin.z + ratio * (yaw_t.sin() - initial_yaw.sin()),
        }
    } else {
        // After the turn: compute turn endpoint, then extrapolate straight
        let ratio = speed / w;
        let turn_end = WorldPos {
            x: torp.origin.x + ratio * (-maneuver.target_yaw.cos() + initial_yaw.cos()),
            y: torp.origin.y,
            z: torp.origin.z + ratio * (maneuver.target_yaw.sin() - initial_yaw.sin()),
        };
        let straight_t = elapsed - turn_dur;
        WorldPos {
            x: turn_end.x + speed * maneuver.target_yaw.sin() * straight_t,
            y: turn_end.y,
            z: turn_end.z + speed * maneuver.target_yaw.cos() * straight_t,
        }
    }
}

/// Build the icon base name from species, consumable flag, and ammo type.
fn species_to_icon_base(species: Species, is_consumable: bool, ammo_type: &str) -> String {
    use convert_case::{Case, Casing};

    let snake = ammo_type.to_case(Case::Snake);
    let ammo = match snake.as_str() {
        "sea_mine" => "mine".to_string(),
        "depthcharge" => "depth_charge".to_string(),
        other => other.to_string(),
    };
    if is_consumable {
        match species {
            Species::Dive => format!("bomber_{ammo}"),
            _ => {
                let species_name = species.name();
                species_name.to_case(Case::Snake)
            }
        }
    } else {
        match species {
            Species::Fighter => format!("fighter_{ammo}"),
            Species::Dive => format!("bomber_{ammo}"),
            Species::Bomber => match ammo.as_str() {
                "torpedo_deepwater" => "torpedo_deepwater".to_string(),
                _ => "torpedo_regular".to_string(),
            },
            Species::Skip => format!("skip_{ammo}"),
            Species::Airship => "auxiliary".to_string(),
            _ => format!("fighter_{ammo}"),
        }
    }
}

/// Map a Consumable enum to its base (default) PCY icon name.
///
/// Used as fallback when per-ship ability data is not available.
/// Returns None for consumables that don't have a meaningful icon display.
fn consumable_to_base_icon_key(c: Consumable) -> Option<String> {
    let key = match c {
        Consumable::DamageControl => "PCY001_CrashCrew",
        Consumable::RepairParty => "PCY002_RegenCrew",
        Consumable::DefensiveAntiAircraft => "PCY003_AirDefenseDisp",
        Consumable::CatapultFighter => "PCY004_Fighter",
        Consumable::SpottingAircraft => "PCY005_Spotter",
        Consumable::Smoke => "PCY006_SmokeGenerator",
        Consumable::SpeedBoost => "PCY007_SpeedBooster",
        Consumable::HydroacousticSearch => "PCY008_SonarSearch",
        Consumable::TorpedoReloadBooster => "PCY017_TorpedoReloader",
        Consumable::Radar => "PCY019_RLSSearch",
        Consumable::MainBatteryReloadBooster => "PCY021_ArtilleryBooster",
        Consumable::CallFighters => "PCY004_Fighter",
        Consumable::RegenerateHealth => "PCY002_RegenCrew",
        Consumable::Hydrophone => "PCY045_Hydrophone",
        Consumable::EnhancedRudders => "PCY046_FastDeepRudders",
        Consumable::SubmarineSurveillance => "PCY048_SubmarineLocator",
        _ => return None,
    };
    Some(key.to_string())
}
