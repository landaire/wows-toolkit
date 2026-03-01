use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Read;
use std::sync::LazyLock;

use crate::game_types::BuoyancyState;
use crate::game_types::CameraMode;
use crate::game_types::CollisionType;
use crate::game_types::Consumable;
use crate::game_types::ControlPointType;
use crate::game_types::DeathCause;
use crate::game_types::FinishType;
use crate::game_types::InteractiveZoneType;
use crate::game_types::ShellHitType;
use crate::game_types::WeaponType;

fn read_vfs_file(vfs: &vfs::VfsPath, path: &str) -> Result<Vec<u8>, vfs::VfsError> {
    let mut buf = Vec::new();
    vfs.join(path)?.open_file()?.read_to_end(&mut buf).map_err(vfs::VfsError::from)?;
    Ok(buf)
}

/// Default battle constants (hardcoded, no game files needed).
pub static DEFAULT_BATTLE_CONSTANTS: LazyLock<BattleConstants> = LazyLock::new(BattleConstants::defaults);

/// Default ships constants (hardcoded, no game files needed).
pub static DEFAULT_SHIPS_CONSTANTS: LazyLock<ShipsConstants> = LazyLock::new(ShipsConstants::defaults);

/// Default common constants (hardcoded, no game files needed).
pub static DEFAULT_COMMON_CONSTANTS: LazyLock<CommonConstants> = LazyLock::new(CommonConstants::defaults);

/// Constants parsed from `gui/data/constants/battle.xml`.
#[derive(Clone)]
pub struct BattleConstants {
    camera_modes: HashMap<i32, Cow<'static, str>>,
    death_reasons: HashMap<i32, Cow<'static, str>>,
    game_modes: HashMap<i32, Cow<'static, str>>,
    battle_results: HashMap<i32, Cow<'static, str>>,
    player_relations: HashMap<i32, Cow<'static, str>>,
    damage_modules: HashMap<i32, Cow<'static, str>>,
    finish_types: HashMap<i32, Cow<'static, str>>,
    consumable_states: HashMap<i32, Cow<'static, str>>,
    planes_types: HashMap<i32, Cow<'static, str>>,
    diplomacy_relations: HashMap<i32, Cow<'static, str>>,
    modules_states: HashMap<i32, Cow<'static, str>>,
    entity_types: HashMap<i32, Cow<'static, str>>,
    entity_states: HashMap<i32, Cow<'static, str>>,
    battery_states: HashMap<i32, Cow<'static, str>>,
    depth_states: HashMap<i32, Cow<'static, str>>,
    building_types: HashMap<i32, Cow<'static, str>>,
    torpedo_marker_types: HashMap<i32, Cow<'static, str>>,
    interactive_zone_types: HashMap<i32, Cow<'static, str>>,
    control_point_types: HashMap<i32, Cow<'static, str>>,
}

impl BattleConstants {
    /// Load from game files, falling back to defaults if the file can't be read.
    pub fn load(vfs: &vfs::VfsPath) -> Self {
        if let Ok(buf) = read_vfs_file(vfs, BATTLE_CONSTANTS_PATH) { Self::from_xml(&buf) } else { Self::defaults() }
    }

    /// Parse from raw XML bytes. Falls back to defaults per-field on parse failure.
    pub fn from_xml(xml: &[u8]) -> Self {
        let xml_str = match std::str::from_utf8(xml) {
            Ok(s) => s,
            Err(_) => return Self::defaults(),
        };

        let defaults = Self::defaults();
        Self {
            camera_modes: parse_integer_enum(xml_str, "CAMERA_MODE").unwrap_or(defaults.camera_modes),
            death_reasons: parse_positional_enum(xml_str, "DEATH_REASON_NAME").unwrap_or(defaults.death_reasons),
            game_modes: parse_integer_enum(xml_str, "GAME_MODE").unwrap_or(defaults.game_modes),
            battle_results: parse_integer_enum(xml_str, "BATTLE_RESULT").unwrap_or(defaults.battle_results),
            player_relations: parse_integer_enum(xml_str, "PLAYER_RELATION").unwrap_or(defaults.player_relations),
            damage_modules: parse_integer_enum(xml_str, "DAMAGE_MODULES").unwrap_or(defaults.damage_modules),
            finish_types: parse_integer_enum(xml_str, "FINISH_TYPE").unwrap_or(defaults.finish_types),
            consumable_states: parse_integer_enum(xml_str, "CONSUMABLE_STATES").unwrap_or(defaults.consumable_states),
            planes_types: parse_integer_enum(xml_str, "PLANES_TYPES").unwrap_or(defaults.planes_types),
            diplomacy_relations: parse_integer_enum(xml_str, "DIPLOMACY_RELATIONS")
                .unwrap_or(defaults.diplomacy_relations),
            modules_states: parse_integer_enum(xml_str, "MODULES_STATES").unwrap_or(defaults.modules_states),
            entity_types: parse_integer_enum(xml_str, "ENTITY_TYPES").unwrap_or(defaults.entity_types),
            entity_states: parse_integer_enum(xml_str, "ENTITY_STATES").unwrap_or(defaults.entity_states),
            battery_states: parse_integer_enum(xml_str, "BATTERY_STATE").unwrap_or(defaults.battery_states),
            depth_states: parse_integer_enum(xml_str, "DEPTH_STATE").unwrap_or(defaults.depth_states),
            building_types: parse_positional_enum(xml_str, "BUILDING_TYPES").unwrap_or(defaults.building_types),
            torpedo_marker_types: parse_positional_enum(xml_str, "TORPEDO_MARKER_TYPE")
                .unwrap_or(defaults.torpedo_marker_types),
            interactive_zone_types: defaults.interactive_zone_types,
            control_point_types: defaults.control_point_types,
        }
    }

    /// Hardcoded defaults matching known game versions (v15.0/v15.1).
    pub fn defaults() -> Self {
        Self {
            camera_modes: HashMap::from([
                (1, Cow::Borrowed(CameraMode::Airplanes.name())),
                (2, Cow::Borrowed(CameraMode::Dock.name())),
                (3, Cow::Borrowed(CameraMode::OverheadMap.name())),
                (4, Cow::Borrowed(CameraMode::DevFree.name())),
                (5, Cow::Borrowed(CameraMode::FollowingShells.name())),
                (6, Cow::Borrowed(CameraMode::FollowingPlanes.name())),
                (7, Cow::Borrowed(CameraMode::DockModule.name())),
                (8, Cow::Borrowed(CameraMode::FollowingShip.name())),
                (9, Cow::Borrowed(CameraMode::FreeFlying.name())),
                (10, Cow::Borrowed(CameraMode::ReplayFpc.name())),
                (11, Cow::Borrowed(CameraMode::FollowingSubmarine.name())),
                (12, Cow::Borrowed(CameraMode::TacticalConsumables.name())),
                (13, Cow::Borrowed(CameraMode::RespawnMap.name())),
                (19, Cow::Borrowed(CameraMode::DockFlags.name())),
                (20, Cow::Borrowed(CameraMode::DockEnsign.name())),
                (21, Cow::Borrowed(CameraMode::DockLootbox.name())),
                (22, Cow::Borrowed(CameraMode::DockNavalFlag.name())),
                (23, Cow::Borrowed(CameraMode::IdleGame.name())),
            ]),
            death_reasons: HashMap::from([
                (0, Cow::Borrowed(DeathCause::None.name())),
                (1, Cow::Borrowed(DeathCause::Artillery.name())),
                (2, Cow::Borrowed(DeathCause::Secondaries.name())),
                (3, Cow::Borrowed(DeathCause::Torpedo.name())),
                (4, Cow::Borrowed(DeathCause::DiveBomber.name())),
                (5, Cow::Borrowed(DeathCause::AerialTorpedo.name())),
                (6, Cow::Borrowed(DeathCause::Fire.name())),
                (7, Cow::Borrowed(DeathCause::Ramming.name())),
                (8, Cow::Borrowed(DeathCause::Terrain.name())),
                (9, Cow::Borrowed(DeathCause::Flooding.name())),
                (10, Cow::Borrowed(DeathCause::Mirror.name())),
                (11, Cow::Borrowed(DeathCause::SeaMine.name())),
                (12, Cow::Borrowed(DeathCause::Special.name())),
                (13, Cow::Borrowed(DeathCause::DepthCharge.name())),
                (14, Cow::Borrowed(DeathCause::AerialRocket.name())),
                (15, Cow::Borrowed(DeathCause::Detonation.name())),
                (16, Cow::Borrowed(DeathCause::Health.name())),
                (17, Cow::Borrowed(DeathCause::ApShell.name())),
                (18, Cow::Borrowed(DeathCause::HeShell.name())),
                (19, Cow::Borrowed(DeathCause::CsShell.name())),
                (20, Cow::Borrowed(DeathCause::Fel.name())),
                (21, Cow::Borrowed(DeathCause::Portal.name())),
                (22, Cow::Borrowed(DeathCause::SkipBombs.name())),
                (23, Cow::Borrowed(DeathCause::SectorWave.name())),
                (24, Cow::Borrowed(DeathCause::Acid.name())),
                (25, Cow::Borrowed(DeathCause::Laser.name())),
                (26, Cow::Borrowed(DeathCause::Match.name())),
                (27, Cow::Borrowed(DeathCause::Timer.name())),
                (28, Cow::Borrowed(DeathCause::AerialDepthCharge.name())),
                (29, Cow::Borrowed(DeathCause::Event1.name())),
                (30, Cow::Borrowed(DeathCause::Event2.name())),
                (31, Cow::Borrowed(DeathCause::Event3.name())),
                (32, Cow::Borrowed(DeathCause::Event4.name())),
                (33, Cow::Borrowed(DeathCause::Event5.name())),
                (34, Cow::Borrowed(DeathCause::Event6.name())),
                (35, Cow::Borrowed(DeathCause::Missile.name())),
            ]),
            game_modes: HashMap::from([
                (-1, Cow::Borrowed("INVALID")),
                (0, Cow::Borrowed("TEST")),
                (1, Cow::Borrowed("STANDART")),
                (2, Cow::Borrowed("SINGLEBASE")),
                (7, Cow::Borrowed("DOMINATION")),
                (8, Cow::Borrowed("TUTORIAL")),
                (9, Cow::Borrowed("MEGABASE")),
                (10, Cow::Borrowed("FORTS")),
                (11, Cow::Borrowed("STANDARD_DOMINATION")),
                (12, Cow::Borrowed("EPICENTER")),
                (13, Cow::Borrowed("ASSAULT_DEFENSE")),
                (14, Cow::Borrowed("PVE")),
                (15, Cow::Borrowed("ARMS_RACE")),
                (16, Cow::Borrowed("EPICENTER_RING")),
                (17, Cow::Borrowed("ANTI_STANDARD")),
                (18, Cow::Borrowed("ATTACK_DEFENSE")),
                (19, Cow::Borrowed("TORPEDO_BEAT")),
                (20, Cow::Borrowed("TEAM_BATTLE_ROYALE")),
                (21, Cow::Borrowed("ESCAPE_TO_PORTAL")),
                (22, Cow::Borrowed("DOMINATION_ASYMM")),
                (23, Cow::Borrowed("KEY_BATTLE")),
                (24, Cow::Borrowed("PORTAL_2021")),
                (25, Cow::Borrowed("TEAM_BATTLE_ROYALE_2021")),
                (26, Cow::Borrowed("CONVOY_EVENT")),
                (27, Cow::Borrowed("CONVOY_AIRSHIP")),
                (28, Cow::Borrowed("TWO_TEAMS_BATTLE_ROYALE")),
                (29, Cow::Borrowed("PINATA_EVENT")),
                (30, Cow::Borrowed("RESPAWNS")),
                (31, Cow::Borrowed("RESPAWNS_SECTORS")),
            ]),
            battle_results: HashMap::from([
                (0, Cow::Borrowed("DEFEAT")),
                (1, Cow::Borrowed("VICTORY")),
                (2, Cow::Borrowed("DRAW")),
                (3, Cow::Borrowed("SUCCESS")),
                (4, Cow::Borrowed("FAILURE")),
                (5, Cow::Borrowed("PORTAL")),
                (6, Cow::Borrowed("MATCH")),
                (7, Cow::Borrowed("DEATH")),
                (8, Cow::Borrowed("TEAM_LADDER_WINNER")),
                (9, Cow::Borrowed("TEAM_LADDER_LOSER")),
            ]),
            player_relations: HashMap::from([
                (0, Cow::Borrowed("SELF")),
                (1, Cow::Borrowed("ALLY")),
                (2, Cow::Borrowed("ENEMY")),
                (3, Cow::Borrowed("NEUTRAL")),
            ]),
            damage_modules: HashMap::from([
                (0, Cow::Borrowed("ENGINE")),
                (1, Cow::Borrowed("MAIN_CALIBER")),
                (2, Cow::Borrowed("ATBA_GUN")),
                (3, Cow::Borrowed("AVIATION")),
                (4, Cow::Borrowed("AIR_DEFENSE")),
                (5, Cow::Borrowed("OBSERVATION")),
                (6, Cow::Borrowed("TORPEDO_TUBE")),
                (7, Cow::Borrowed("PATH_CONTROL")),
                (8, Cow::Borrowed("DEPTH_CHARGE_GUN")),
                (9, Cow::Borrowed("BURN")),
                (10, Cow::Borrowed("FLOOD")),
                (11, Cow::Borrowed("ACID")),
                (12, Cow::Borrowed("HEATED")),
                (13, Cow::Borrowed("WAVED")),
                (14, Cow::Borrowed("PINGER")),
                (15, Cow::Borrowed("OIL_LEAK")),
                (16, Cow::Borrowed("OIL_LEAK_PENDING")),
                (17, Cow::Borrowed("WILD_FIRE")),
            ]),
            finish_types: HashMap::from([
                (0, Cow::Borrowed(FinishType::Unknown.name())),
                (1, Cow::Borrowed(FinishType::Extermination.name())),
                (2, Cow::Borrowed(FinishType::BaseCaptured.name())),
                (3, Cow::Borrowed(FinishType::Timeout.name())),
                (4, Cow::Borrowed(FinishType::Failure.name())),
                (5, Cow::Borrowed(FinishType::Technical.name())),
                (8, Cow::Borrowed(FinishType::Score.name())),
                (9, Cow::Borrowed(FinishType::ScoreOnTimeout.name())),
                (10, Cow::Borrowed(FinishType::PveMainTaskSucceeded.name())),
                (11, Cow::Borrowed(FinishType::PveMainTaskFailed.name())),
                (12, Cow::Borrowed(FinishType::ScoreZero.name())),
                (13, Cow::Borrowed(FinishType::ScoreExcess.name())),
            ]),
            consumable_states: HashMap::from([
                (0, Cow::Borrowed("READY")),
                (1, Cow::Borrowed("SELECTED")),
                (2, Cow::Borrowed("AT_WORK")),
                (3, Cow::Borrowed("RELOAD")),
                (4, Cow::Borrowed("NO_AMMO")),
                (5, Cow::Borrowed("PREPARATION")),
                (6, Cow::Borrowed("REGENERATION")),
            ]),
            planes_types: HashMap::from([
                (0, Cow::Borrowed("SCOUT")),
                (1, Cow::Borrowed("DIVEBOMBER")),
                (2, Cow::Borrowed("TORPEDOBOMBER")),
                (3, Cow::Borrowed("FIGHTER")),
                (4, Cow::Borrowed("AUXILIARY")),
                (5, Cow::Borrowed("SKIP_BOMBER")),
                (6, Cow::Borrowed("AIR_SUPPORT")),
                (7, Cow::Borrowed("AIRSHIP")),
            ]),
            diplomacy_relations: HashMap::from([
                (0, Cow::Borrowed("SELF")),
                (1, Cow::Borrowed("ALLY")),
                (2, Cow::Borrowed("NEUTRAL")),
                (3, Cow::Borrowed("ENEMY")),
                (4, Cow::Borrowed("AGGRESSOR")),
            ]),
            modules_states: HashMap::from([
                (0, Cow::Borrowed("NORMAL")),
                (1, Cow::Borrowed("DAMAGED")),
                (2, Cow::Borrowed("CRIT")),
                (3, Cow::Borrowed("BROKEN")),
                (4, Cow::Borrowed("DEAD")),
            ]),
            entity_types: HashMap::from([
                (-1, Cow::Borrowed("INVALID")),
                (0, Cow::Borrowed("SHIP")),
                (1, Cow::Borrowed("PLANE")),
                (2, Cow::Borrowed("TORPEDO")),
                (3, Cow::Borrowed("BUILDING")),
                (11, Cow::Borrowed("CAPTURE_POINT")),
                (12, Cow::Borrowed("PLAYER")),
                (13, Cow::Borrowed("EPICENTER")),
                (14, Cow::Borrowed("SCENARIO_OBJECT")),
                (15, Cow::Borrowed("DROP_ZONE")),
                (16, Cow::Borrowed("ATTENTION_POINT")),
                (17, Cow::Borrowed("KEY_OBJECT")),
                (18, Cow::Borrowed("INTERACTIVE_ZONE")),
                (27, Cow::Borrowed("PINATA_SHIP")),
                (28, Cow::Borrowed("STARTREK_SCRAP")),
                (29, Cow::Borrowed("MISSILE")),
                (99, Cow::Borrowed("NAVPOINT")),
                (100, Cow::Borrowed("EMPTY")),
            ]),
            entity_states: HashMap::from([
                (0, Cow::Borrowed("EMPTY")),
                (1, Cow::Borrowed("REPAIR")),
                (2, Cow::Borrowed("GATHERING_SURVIVORS")),
                (3, Cow::Borrowed("FILTH")),
                (4, Cow::Borrowed("FROZEN")),
                (5, Cow::Borrowed("UNLOADING_MARINES")),
                (6, Cow::Borrowed("INSIDE_WEATHER")),
                (7, Cow::Borrowed("NEAR_WEATHER")),
                (8, Cow::Borrowed("ILLUMINATED")),
                (9, Cow::Borrowed("BY_NIGHT")),
                (10, Cow::Borrowed("INSIDE_MINEFIELD")),
                (11, Cow::Borrowed("NEAR_MINEFIELD")),
                (12, Cow::Borrowed("CAPTURING_LOCKED")),
                (13, Cow::Borrowed("DANGER")),
                (14, Cow::Borrowed("TEAM_01")),
                (15, Cow::Borrowed("TEAM_02")),
                (16, Cow::Borrowed("TEAM_03")),
                (17, Cow::Borrowed("TEAM_11")),
                (18, Cow::Borrowed("TEAM_12")),
                (19, Cow::Borrowed("TEAM_13")),
                (20, Cow::Borrowed("TEAM_21")),
                (21, Cow::Borrowed("TEAM_22")),
                (22, Cow::Borrowed("TEAM_23")),
                (23, Cow::Borrowed("TEAM_31")),
                (24, Cow::Borrowed("TEAM_32")),
                (25, Cow::Borrowed("TEAM_33")),
                (26, Cow::Borrowed("SPOTTED_BY_ENEMY")),
                (27, Cow::Borrowed("INVULNERABLE")),
                (28, Cow::Borrowed("FLAGSHIP")),
                (29, Cow::Borrowed("MIGNON")),
                (30, Cow::Borrowed("REPAIR_SHIP")),
                (100, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_BROKEN_MODULES")),
                (101, Cow::Borrowed("BURN")),
                (102, Cow::Borrowed("FLOOD")),
                (103, Cow::Borrowed("HOLD_RESOURCE")),
                (104, Cow::Borrowed("HOLD_RESOURCE_FILTHIOR_TEAM_0")),
                (105, Cow::Borrowed("HOLD_RESOURCE_FILTHIOR_TEAM_1")),
                (106, Cow::Borrowed("HOLD_RESOURCE_FILTHIOR_TEAM_2")),
                (107, Cow::Borrowed("HOLD_RESOURCE_FILTHIOR_TEAM_3")),
                (108, Cow::Borrowed("HOLD_RESOURCE_POINTS")),
                (109, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_CRIT_MODULES")),
                (110, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_SPECIAL_MODULES")),
                (111, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_MODIFIERS")),
                (112, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_TALENTS")),
                (113, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_ATBA_ACCURACY_PERK")),
                (114, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_PERKS")),
                (115, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_BUFFS")),
                (116, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_WEATHER")),
                (117, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_INTERACTIVE_ZONE")),
                (118, Cow::Borrowed("EMERGENCY_SURFACING")),
                (119, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_BATTERY_STATE")),
                (120, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_DEPTH")),
                (121, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_ARTILLERY_FIRE_MODE")),
                (122, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_RAGE_MODE")),
                (123, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_CONSUMABLES")),
                (124, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_NOT_SPECIAL_MODULES")),
                (125, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_CONSUMABLE_LOCKER")),
                (126, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_ANTI_ABUSE_SYSTEM")),
                (127, Cow::Borrowed("BATTLE_CARD_SELECTOR_AVAILABLE")),
                (128, Cow::Borrowed("SHIP_PARAMS_CHANGE_BY_INNATE_SKILLS")),
            ]),
            battery_states: HashMap::from([
                (0, Cow::Borrowed("IDLE")),
                (1, Cow::Borrowed("CHARGING")),
                (2, Cow::Borrowed("FROZEN")),
                (3, Cow::Borrowed("SPENDING_NORMAL")),
                (4, Cow::Borrowed("SPENDING_WARNING")),
                (5, Cow::Borrowed("SPENDING_CRITICAL")),
                (6, Cow::Borrowed("BURNING")),
                (7, Cow::Borrowed("EMPTY")),
            ]),
            depth_states: HashMap::from([
                (-1, Cow::Borrowed(BuoyancyState::Invalid.name())),
                (0, Cow::Borrowed(BuoyancyState::Surface.name())),
                (1, Cow::Borrowed(BuoyancyState::Periscope.name())),
                (2, Cow::Borrowed(BuoyancyState::SemiDeepWater.name())),
                (3, Cow::Borrowed(BuoyancyState::DeepWater.name())),
                (4, Cow::Borrowed(BuoyancyState::DeepWaterInvul.name())),
            ]),
            building_types: HashMap::from([
                (0, Cow::Borrowed("ANTI_AIRCRAFT")),
                (1, Cow::Borrowed("AIR_BASE")),
                (2, Cow::Borrowed("COASTAL_ARTILLERY")),
                (3, Cow::Borrowed("MILITARY")),
                (4, Cow::Borrowed("SENSOR_TOWER")),
                (5, Cow::Borrowed("COMPLEX")),
                (6, Cow::Borrowed("RAY_TOWER")),
                (7, Cow::Borrowed("GENERATOR")),
                (8, Cow::Borrowed("SPACE_STATION")),
            ]),
            torpedo_marker_types: HashMap::from([
                (0, Cow::Borrowed("NORMAL")),
                (1, Cow::Borrowed("DEEP_WATER")),
                (2, Cow::Borrowed("ACOUSTIC")),
                (3, Cow::Borrowed("MAGNETIC")),
                (4, Cow::Borrowed("NOT_DANGEROUS")),
            ]),
            interactive_zone_types: HashMap::from([
                (0, Cow::Borrowed(InteractiveZoneType::NoType.name())),
                (1, Cow::Borrowed(InteractiveZoneType::ResourceZone.name())),
                (2, Cow::Borrowed(InteractiveZoneType::ConvoyZone.name())),
                (3, Cow::Borrowed(InteractiveZoneType::RepairZone.name())),
                (4, Cow::Borrowed(InteractiveZoneType::FelZone.name())),
                (5, Cow::Borrowed(InteractiveZoneType::WeatherZone.name())),
                (6, Cow::Borrowed(InteractiveZoneType::DropZone.name())),
                (7, Cow::Borrowed(InteractiveZoneType::ConsumableZone.name())),
                (8, Cow::Borrowed(InteractiveZoneType::ColoredByRelation.name())),
                (9, Cow::Borrowed(InteractiveZoneType::ControlPoint.name())),
                (10, Cow::Borrowed(InteractiveZoneType::RescueZone.name())),
                (11, Cow::Borrowed(InteractiveZoneType::OrbitalStrikeZone.name())),
            ]),
            control_point_types: HashMap::from([
                (1, Cow::Borrowed(ControlPointType::Control.name())),
                (2, Cow::Borrowed(ControlPointType::Base.name())),
                (3, Cow::Borrowed(ControlPointType::MegaBase.name())),
                (4, Cow::Borrowed(ControlPointType::BuildingCp.name())),
                (5, Cow::Borrowed(ControlPointType::BaseWithPoints.name())),
                (6, Cow::Borrowed(ControlPointType::EpicenterCp.name())),
            ]),
        }
    }

    pub fn camera_mode(&self, id: i32) -> Option<&str> {
        self.camera_modes.get(&id).map(|s| s.as_ref())
    }

    pub fn death_reason(&self, id: i32) -> Option<&str> {
        self.death_reasons.get(&id).map(|s| s.as_ref())
    }

    pub fn game_mode(&self, id: i32) -> Option<&str> {
        self.game_modes.get(&id).map(|s| s.as_ref())
    }

    pub fn battle_result(&self, id: i32) -> Option<&str> {
        self.battle_results.get(&id).map(|s| s.as_ref())
    }

    pub fn player_relation(&self, id: i32) -> Option<&str> {
        self.player_relations.get(&id).map(|s| s.as_ref())
    }

    pub fn damage_module(&self, id: i32) -> Option<&str> {
        self.damage_modules.get(&id).map(|s| s.as_ref())
    }

    pub fn finish_type(&self, id: i32) -> Option<&str> {
        self.finish_types.get(&id).map(|s| s.as_ref())
    }

    pub fn consumable_state(&self, id: i32) -> Option<&str> {
        self.consumable_states.get(&id).map(|s| s.as_ref())
    }

    pub fn planes_type(&self, id: i32) -> Option<&str> {
        self.planes_types.get(&id).map(|s| s.as_ref())
    }

    pub fn diplomacy_relation(&self, id: i32) -> Option<&str> {
        self.diplomacy_relations.get(&id).map(|s| s.as_ref())
    }

    pub fn modules_state(&self, id: i32) -> Option<&str> {
        self.modules_states.get(&id).map(|s| s.as_ref())
    }

    pub fn entity_type(&self, id: i32) -> Option<&str> {
        self.entity_types.get(&id).map(|s| s.as_ref())
    }

    pub fn entity_state(&self, id: i32) -> Option<&str> {
        self.entity_states.get(&id).map(|s| s.as_ref())
    }

    pub fn battery_state(&self, id: i32) -> Option<&str> {
        self.battery_states.get(&id).map(|s| s.as_ref())
    }

    pub fn depth_state(&self, id: i32) -> Option<&str> {
        self.depth_states.get(&id).map(|s| s.as_ref())
    }

    pub fn building_type(&self, id: i32) -> Option<&str> {
        self.building_types.get(&id).map(|s| s.as_ref())
    }

    pub fn torpedo_marker_type(&self, id: i32) -> Option<&str> {
        self.torpedo_marker_types.get(&id).map(|s| s.as_ref())
    }

    pub fn interactive_zone_type(&self, id: i32) -> Option<&str> {
        self.interactive_zone_types.get(&id).map(|s| s.as_ref())
    }

    pub fn control_point_type(&self, id: i32) -> Option<&str> {
        self.control_point_types.get(&id).map(|s| s.as_ref())
    }

    pub fn camera_modes_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.camera_modes
    }

    pub fn death_reasons_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.death_reasons
    }

    pub fn game_modes_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.game_modes
    }

    pub fn battle_results_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.battle_results
    }

    pub fn player_relations_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.player_relations
    }

    pub fn damage_modules_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.damage_modules
    }

    pub fn finish_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.finish_types
    }

    pub fn consumable_states_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.consumable_states
    }

    pub fn planes_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.planes_types
    }

    pub fn diplomacy_relations_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.diplomacy_relations
    }

    pub fn modules_states_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.modules_states
    }

    pub fn entity_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.entity_types
    }

    pub fn entity_states_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.entity_states
    }

    pub fn battery_states_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.battery_states
    }

    pub fn depth_states_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.depth_states
    }

    pub fn building_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.building_types
    }

    pub fn torpedo_marker_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.torpedo_marker_types
    }

    pub fn interactive_zone_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.interactive_zone_types
    }

    pub fn control_point_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.control_point_types
    }

    pub fn camera_modes(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.camera_modes
    }

    pub fn death_reasons(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.death_reasons
    }

    pub fn game_modes(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.game_modes
    }

    pub fn battle_results(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.battle_results
    }

    pub fn player_relations(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.player_relations
    }

    pub fn damage_modules(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.damage_modules
    }

    pub fn finish_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.finish_types
    }

    pub fn consumable_states(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.consumable_states
    }

    pub fn planes_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.planes_types
    }

    pub fn diplomacy_relations(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.diplomacy_relations
    }

    pub fn modules_states(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.modules_states
    }

    pub fn entity_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.entity_types
    }

    pub fn entity_states(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.entity_states
    }

    pub fn battery_states(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.battery_states
    }

    pub fn depth_states(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.depth_states
    }

    pub fn building_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.building_types
    }

    pub fn torpedo_marker_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.torpedo_marker_types
    }

    pub fn interactive_zone_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.interactive_zone_types
    }

    pub fn control_point_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.control_point_types
    }
}

/// Constants parsed from `gui/data/constants/ships.xml`.
#[derive(Clone)]
pub struct ShipsConstants {
    weapon_types: HashMap<i32, Cow<'static, str>>,
    module_types: HashMap<i32, Cow<'static, str>>,
    shell_hit_types: HashMap<i32, Cow<'static, str>>,
    collision_types: HashMap<i32, Cow<'static, str>>,
}

impl ShipsConstants {
    /// Load from game files, falling back to defaults if the file can't be read.
    pub fn load(vfs: &vfs::VfsPath) -> Self {
        if let Ok(buf) = read_vfs_file(vfs, SHIPS_CONSTANTS_PATH) { Self::from_xml(&buf) } else { Self::defaults() }
    }

    /// Parse from raw XML bytes. Falls back to defaults on parse failure.
    pub fn from_xml(xml: &[u8]) -> Self {
        let xml_str = match std::str::from_utf8(xml) {
            Ok(s) => s,
            Err(_) => return Self::defaults(),
        };

        let defaults = Self::defaults();
        Self {
            weapon_types: parse_integer_enum(xml_str, "SHIP_WEAPON_TYPES").unwrap_or(defaults.weapon_types),
            module_types: parse_integer_enum(xml_str, "SHIP_MODULE_TYPES").unwrap_or(defaults.module_types),
            shell_hit_types: defaults.shell_hit_types,
            collision_types: defaults.collision_types,
        }
    }

    /// Hardcoded defaults matching known game versions (v15.0/v15.1).
    pub fn defaults() -> Self {
        Self {
            weapon_types: HashMap::from([
                (-1, Cow::Borrowed("NONE")),
                (0, Cow::Borrowed(WeaponType::Artillery.name())),
                (1, Cow::Borrowed(WeaponType::Secondaries.name())),
                (2, Cow::Borrowed(WeaponType::Torpedoes.name())),
                (3, Cow::Borrowed(WeaponType::Planes.name())),
                (4, Cow::Borrowed("AIRDEFENCE")),
                (5, Cow::Borrowed("DEPTH_CHARGES")),
                (6, Cow::Borrowed(WeaponType::Pinger.name())),
                (7, Cow::Borrowed("CHARGE_LASER")),
                (8, Cow::Borrowed("IMPULSE_LASER")),
                (9, Cow::Borrowed("AXIS_LASER")),
                (10, Cow::Borrowed("PHASER_LASER")),
                (11, Cow::Borrowed("WAVES")),
                (12, Cow::Borrowed("AIR_SUPPORT")),
                (13, Cow::Borrowed("ANTI_MISSILE")),
                (14, Cow::Borrowed("MISSILES")),
                (100, Cow::Borrowed("SQUADRON")),
                (200, Cow::Borrowed("PULSE_PHASERS")),
            ]),
            module_types: HashMap::from([
                (0, Cow::Borrowed("ARTILLERY")),
                (1, Cow::Borrowed("HULL")),
                (2, Cow::Borrowed("TORPEDOES")),
                (3, Cow::Borrowed("SUO")),
                (4, Cow::Borrowed("ENGINE")),
                (5, Cow::Borrowed("TORPEDO_BOMBER")),
                (6, Cow::Borrowed("DIVE_BOMBER")),
                (7, Cow::Borrowed("FIGHTER")),
                (8, Cow::Borrowed("FLIGHT_CONTROLL")),
                (9, Cow::Borrowed("HYDROPHONE")),
                (10, Cow::Borrowed("SKIP_BOMBER")),
                (11, Cow::Borrowed("PRIMARY_WEAPONS")),
                (12, Cow::Borrowed("SECONDARY_WEAPONS")),
                (13, Cow::Borrowed("ABILITIES")),
            ]),
            shell_hit_types: HashMap::from([
                (0, Cow::Borrowed(ShellHitType::Normal.name())),
                (1, Cow::Borrowed(ShellHitType::Ricochet.name())),
                (2, Cow::Borrowed(ShellHitType::MajorHit.name())),
                (3, Cow::Borrowed(ShellHitType::NoPenetration.name())),
                (4, Cow::Borrowed(ShellHitType::Overpenetration.name())),
                (5, Cow::Borrowed(ShellHitType::None.name())),
                (6, Cow::Borrowed(ShellHitType::ExitOverpenetration.name())),
                (7, Cow::Borrowed(ShellHitType::Underwater.name())),
            ]),
            collision_types: HashMap::from([
                (0, Cow::Borrowed(CollisionType::NoHit.name())),
                (1, Cow::Borrowed(CollisionType::HitWater.name())),
                (2, Cow::Borrowed(CollisionType::HitGround.name())),
                (3, Cow::Borrowed(CollisionType::HitEntity.name())),
                (4, Cow::Borrowed(CollisionType::HitEntityBB.name())),
                (5, Cow::Borrowed(CollisionType::HitWave.name())),
            ]),
        }
    }

    pub fn weapon_type(&self, id: i32) -> Option<&str> {
        self.weapon_types.get(&id).map(|s| s.as_ref())
    }

    pub fn module_type(&self, id: i32) -> Option<&str> {
        self.module_types.get(&id).map(|s| s.as_ref())
    }

    pub fn weapon_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.weapon_types
    }

    pub fn module_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.module_types
    }

    pub fn weapon_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.weapon_types
    }

    pub fn module_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.module_types
    }

    pub fn shell_hit_type(&self, id: i32) -> Option<&str> {
        self.shell_hit_types.get(&id).map(|s| s.as_ref())
    }

    pub fn collision_type(&self, id: i32) -> Option<&str> {
        self.collision_types.get(&id).map(|s| s.as_ref())
    }

    pub fn shell_hit_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.shell_hit_types
    }

    pub fn collision_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.collision_types
    }

    pub fn shell_hit_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.shell_hit_types
    }

    pub fn collision_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.collision_types
    }
}

/// Constants parsed from `gui/data/constants/weapons.xml`.
#[derive(Clone)]
pub struct WeaponsConstants {
    gun_states: HashMap<i32, Cow<'static, str>>,
}

impl WeaponsConstants {
    /// Load from game files, falling back to defaults if the file can't be read.
    pub fn load(vfs: &vfs::VfsPath) -> Self {
        if let Ok(buf) = read_vfs_file(vfs, WEAPONS_CONSTANTS_PATH) { Self::from_xml(&buf) } else { Self::defaults() }
    }

    /// Parse from raw XML bytes. Falls back to defaults on parse failure.
    pub fn from_xml(xml: &[u8]) -> Self {
        let xml_str = match std::str::from_utf8(xml) {
            Ok(s) => s,
            Err(_) => return Self::defaults(),
        };

        let defaults = Self::defaults();
        Self { gun_states: parse_integer_enum(xml_str, "GUN_STATE").unwrap_or(defaults.gun_states) }
    }

    /// Hardcoded defaults matching known game versions (v15.0/v15.1).
    pub fn defaults() -> Self {
        Self {
            gun_states: HashMap::from([
                (1, Cow::Borrowed("READY")),
                (2, Cow::Borrowed("WORK")),
                (3, Cow::Borrowed("RELOAD")),
                (4, Cow::Borrowed("SWITCHING_AMMO")),
                (5, Cow::Borrowed("RELOAD_STOPPED")),
                (6, Cow::Borrowed("CHARGE")),
                (7, Cow::Borrowed("CRITICAL")),
                (8, Cow::Borrowed("DESTROYED")),
                (9, Cow::Borrowed("SWITCHING_CRITICAL")),
                (10, Cow::Borrowed("DISABLED")),
                (11, Cow::Borrowed("BROKEN")),
            ]),
        }
    }

    pub fn gun_state(&self, id: i32) -> Option<&str> {
        self.gun_states.get(&id).map(|s| s.as_ref())
    }

    pub fn gun_states(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.gun_states
    }

    pub fn gun_states_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.gun_states
    }
}

/// Constants parsed from `gui/data/constants/common.xml`.
#[derive(Clone)]
pub struct CommonConstants {
    plane_ammo_types: HashMap<i32, Cow<'static, str>>,
    torpedo_types: HashMap<i32, Cow<'static, str>>,
    consumable_types: HashMap<i32, Cow<'static, str>>,
    battle_stages: HashMap<i32, crate::game_types::BattleStage>,
}

impl CommonConstants {
    /// Load from game files, falling back to defaults if the file can't be read.
    pub fn load(vfs: &vfs::VfsPath) -> Self {
        if let Ok(buf) = read_vfs_file(vfs, COMMON_CONSTANTS_PATH) { Self::from_xml(&buf) } else { Self::defaults() }
    }

    /// Parse from raw XML bytes. Falls back to defaults on parse failure.
    pub fn from_xml(xml: &[u8]) -> Self {
        let xml_str = match std::str::from_utf8(xml) {
            Ok(s) => s,
            Err(_) => return Self::defaults(),
        };

        let defaults = Self::defaults();
        Self {
            plane_ammo_types: parse_integer_enum(xml_str, "PLANE_AMMO_TYPES").unwrap_or(defaults.plane_ammo_types),
            torpedo_types: parse_integer_enum(xml_str, "TORPEDO_TYPE").unwrap_or(defaults.torpedo_types),
            consumable_types: defaults.consumable_types,
            battle_stages: defaults.battle_stages,
        }
    }

    /// Hardcoded defaults matching known game versions (v15.0/v15.1).
    pub fn defaults() -> Self {
        Self {
            plane_ammo_types: HashMap::from([
                (-1, Cow::Borrowed("NONE")),
                (0, Cow::Borrowed("PROJECTILE")),
                (1, Cow::Borrowed("BOMB_HE")),
                (2, Cow::Borrowed("BOMB_AP")),
                (3, Cow::Borrowed("SKIP_BOMB_HE")),
                (4, Cow::Borrowed("SKIP_BOMB_AP")),
                (5, Cow::Borrowed("TORPEDO")),
                (6, Cow::Borrowed("TORPEDO_DEEPWATER")),
                (7, Cow::Borrowed("PROJECTILE_AP")),
                (8, Cow::Borrowed("DEPTH_CHARGE")),
                (9, Cow::Borrowed("MINE")),
                (10, Cow::Borrowed("SMOKE")),
            ]),
            torpedo_types: HashMap::from([
                (0, Cow::Borrowed("COMMON")),
                (1, Cow::Borrowed("SUBMARINE")),
                (2, Cow::Borrowed("PHOTON")),
            ]),
            consumable_types: HashMap::from([
                (0, Cow::Borrowed(Consumable::DamageControl.name())),
                (1, Cow::Borrowed(Consumable::SpottingAircraft.name())),
                (2, Cow::Borrowed(Consumable::DefensiveAntiAircraft.name())),
                (3, Cow::Borrowed(Consumable::SpeedBoost.name())),
                (4, Cow::Borrowed(Consumable::MainBatteryReloadBooster.name())),
                (6, Cow::Borrowed(Consumable::Smoke.name())),
                (8, Cow::Borrowed(Consumable::RepairParty.name())),
                (9, Cow::Borrowed(Consumable::CatapultFighter.name())),
                (10, Cow::Borrowed(Consumable::HydroacousticSearch.name())),
                (11, Cow::Borrowed(Consumable::TorpedoReloadBooster.name())),
                (12, Cow::Borrowed(Consumable::Radar.name())),
                (13, Cow::Borrowed(Consumable::Trigger1.name())),
                (14, Cow::Borrowed(Consumable::Trigger2.name())),
                (15, Cow::Borrowed(Consumable::Trigger3.name())),
                (16, Cow::Borrowed(Consumable::Trigger4.name())),
                (17, Cow::Borrowed(Consumable::Trigger5.name())),
                (18, Cow::Borrowed(Consumable::Trigger6.name())),
                (19, Cow::Borrowed(Consumable::Invulnerable.name())),
                (20, Cow::Borrowed(Consumable::HealForsage.name())),
                (21, Cow::Borrowed(Consumable::CallFighters.name())),
                (22, Cow::Borrowed(Consumable::RegenerateHealth.name())),
                (23, Cow::Borrowed(Consumable::SubsOxygenRegen.name())),
                (24, Cow::Borrowed(Consumable::SubsWaveGunBoost.name())),
                (25, Cow::Borrowed(Consumable::SubsFourthState.name())),
                (26, Cow::Borrowed(Consumable::DepthCharges.name())),
                (27, Cow::Borrowed(Consumable::Trigger7.name())),
                (28, Cow::Borrowed(Consumable::Trigger8.name())),
                (29, Cow::Borrowed(Consumable::Trigger9.name())),
                (30, Cow::Borrowed(Consumable::Buff.name())),
                (31, Cow::Borrowed(Consumable::BuffsShift.name())),
                (32, Cow::Borrowed(Consumable::CircleWave.name())),
                (33, Cow::Borrowed(Consumable::GoDeep.name())),
                (34, Cow::Borrowed(Consumable::WeaponReloadBooster.name())),
                (35, Cow::Borrowed(Consumable::Hydrophone.name())),
                (36, Cow::Borrowed(Consumable::EnhancedRudders.name())),
                (37, Cow::Borrowed(Consumable::ReserveBattery.name())),
                (38, Cow::Borrowed(Consumable::GroupAuraBuff.name())),
                (39, Cow::Borrowed(Consumable::AffectedBuffAura.name())),
                (40, Cow::Borrowed(Consumable::InvisibilityExtraBuff.name())),
                (41, Cow::Borrowed(Consumable::SubmarineSurveillance.name())),
                (42, Cow::Borrowed(Consumable::PlaneSmokeGenerator.name())),
                (44, Cow::Borrowed(Consumable::Minefield.name())),
                (45, Cow::Borrowed(Consumable::TacticalTrigger1.name())),
                (46, Cow::Borrowed(Consumable::TacticalTrigger2.name())),
                (47, Cow::Borrowed(Consumable::TacticalTrigger3.name())),
                (48, Cow::Borrowed(Consumable::TacticalTrigger4.name())),
                (49, Cow::Borrowed(Consumable::TacticalTrigger5.name())),
                (50, Cow::Borrowed(Consumable::TacticalTrigger6.name())),
                (51, Cow::Borrowed(Consumable::ReconnaissanceSquad.name())),
                (52, Cow::Borrowed(Consumable::SmokePlane.name())),
                (53, Cow::Borrowed(Consumable::TacticalBuff.name())),
                (54, Cow::Borrowed(Consumable::PlaneTrigger1.name())),
                (55, Cow::Borrowed(Consumable::PlaneTrigger2.name())),
                (56, Cow::Borrowed(Consumable::PlaneTrigger3.name())),
                (57, Cow::Borrowed(Consumable::PlaneBuff.name())),
                (58, Cow::Borrowed(Consumable::Any.name())),
                (59, Cow::Borrowed(Consumable::All.name())),
                (60, Cow::Borrowed(Consumable::Special.name())),
            ]),
            battle_stages: HashMap::from([
                (0, crate::game_types::BattleStage::Waiting),
                (1, crate::game_types::BattleStage::Battle),
                (2, crate::game_types::BattleStage::Results),
                (3, crate::game_types::BattleStage::Finishing),
                (4, crate::game_types::BattleStage::Ended),
            ]),
        }
    }

    pub fn plane_ammo_type(&self, id: i32) -> Option<&str> {
        self.plane_ammo_types.get(&id).map(|s| s.as_ref())
    }

    pub fn torpedo_type(&self, id: i32) -> Option<&str> {
        self.torpedo_types.get(&id).map(|s| s.as_ref())
    }

    pub fn consumable_type(&self, id: i32) -> Option<&str> {
        self.consumable_types.get(&id).map(|s| s.as_ref())
    }

    pub fn plane_ammo_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.plane_ammo_types
    }

    pub fn torpedo_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.torpedo_types
    }

    pub fn consumable_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.consumable_types
    }

    pub fn plane_ammo_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.plane_ammo_types
    }

    pub fn torpedo_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.torpedo_types
    }

    pub fn consumable_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.consumable_types
    }

    pub fn battle_stage(&self, id: i32) -> Option<&crate::game_types::BattleStage> {
        self.battle_stages.get(&id)
    }

    pub fn battle_stages(&self) -> &HashMap<i32, crate::game_types::BattleStage> {
        &self.battle_stages
    }

    pub fn battle_stages_mut(&mut self) -> &mut HashMap<i32, crate::game_types::BattleStage> {
        &mut self.battle_stages
    }
}

/// Constants parsed from `gui/data/constants/channel.xml`.
#[derive(Clone)]
pub struct ChannelConstants {
    battle_chat_channel_types: HashMap<i32, Cow<'static, str>>,
    channel_type_idents: HashMap<i32, Cow<'static, str>>,
}

impl ChannelConstants {
    /// Load from game files, falling back to defaults if the file can't be read.
    pub fn load(vfs: &vfs::VfsPath) -> Self {
        if let Ok(buf) = read_vfs_file(vfs, CHANNEL_CONSTANTS_PATH) { Self::from_xml(&buf) } else { Self::defaults() }
    }

    /// Parse from raw XML bytes. Falls back to defaults on parse failure.
    pub fn from_xml(xml: &[u8]) -> Self {
        let xml_str = match std::str::from_utf8(xml) {
            Ok(s) => s,
            Err(_) => return Self::defaults(),
        };

        let defaults = Self::defaults();
        Self {
            battle_chat_channel_types: parse_integer_enum(xml_str, "BATTLE_CHAT_CHANNEL_TYPE")
                .unwrap_or(defaults.battle_chat_channel_types),
            channel_type_idents: parse_positional_enum(xml_str, "CHANNEL_TYPE_IDENT_VALUE")
                .unwrap_or(defaults.channel_type_idents),
        }
    }

    /// Hardcoded defaults matching known game versions (v15.0/v15.1).
    pub fn defaults() -> Self {
        Self {
            battle_chat_channel_types: HashMap::from([
                (0, Cow::Borrowed("GENERAL")),
                (1, Cow::Borrowed("TEAM")),
                (2, Cow::Borrowed("DIVISION")),
                (3, Cow::Borrowed("SYSTEM")),
            ]),
            channel_type_idents: HashMap::from([
                (0, Cow::Borrowed("UNKNOWN")),
                (1, Cow::Borrowed("GROUP_OPEN")),
                (2, Cow::Borrowed("GROUP_CLOSED")),
                (3, Cow::Borrowed("PREBATTLE")),
                (4, Cow::Borrowed("COMMON")),
                (5, Cow::Borrowed("PRIVATE")),
                (6, Cow::Borrowed("CLAN")),
                (7, Cow::Borrowed("TRAINING_ROOM")),
            ]),
        }
    }

    pub fn battle_chat_channel_type(&self, id: i32) -> Option<&str> {
        self.battle_chat_channel_types.get(&id).map(|s| s.as_ref())
    }

    pub fn channel_type_ident(&self, id: i32) -> Option<&str> {
        self.channel_type_idents.get(&id).map(|s| s.as_ref())
    }

    pub fn battle_chat_channel_types(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.battle_chat_channel_types
    }

    pub fn channel_type_idents(&self) -> &HashMap<i32, Cow<'static, str>> {
        &self.channel_type_idents
    }

    pub fn battle_chat_channel_types_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.battle_chat_channel_types
    }

    pub fn channel_type_idents_mut(&mut self) -> &mut HashMap<i32, Cow<'static, str>> {
        &mut self.channel_type_idents
    }
}

/// Parse an `<enum type="Integer">` block from XML.
fn parse_integer_enum(xml: &str, enum_name: &str) -> Option<HashMap<i32, Cow<'static, str>>> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let enum_node = doc.descendants().find(|n| n.has_tag_name("enum") && n.attribute("name") == Some(enum_name))?;

    let mut map = HashMap::new();
    for child in enum_node.children() {
        if child.has_tag_name("const")
            && let (Some(name), Some(value_str)) = (child.attribute("name"), child.attribute("value"))
            && let Ok(value) = value_str.trim().parse::<i32>()
        {
            map.insert(value, Cow::Owned(name.to_string()));
        }
    }

    if map.is_empty() { None } else { Some(map) }
}

/// Parse an `<enum type="String">` block from XML (positional indexing).
fn parse_positional_enum(xml: &str, enum_name: &str) -> Option<HashMap<i32, Cow<'static, str>>> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let enum_node = doc.descendants().find(|n| n.has_tag_name("enum") && n.attribute("name") == Some(enum_name))?;

    let mut map = HashMap::new();
    let mut index = 0i32;
    for child in enum_node.children() {
        if child.has_tag_name("const") {
            if let Some(name) = child.attribute("name") {
                map.insert(index, Cow::Owned(name.to_string()));
            }
            index += 1;
        }
    }

    if map.is_empty() { None } else { Some(map) }
}

/// The file path within the game's `res/` directory for battle constants.
pub const BATTLE_CONSTANTS_PATH: &str = "gui/data/constants/battle.xml";

/// The file path within the game's `res/` directory for ship constants.
pub const SHIPS_CONSTANTS_PATH: &str = "gui/data/constants/ships.xml";

/// The file path within the game's `res/` directory for weapons constants.
pub const WEAPONS_CONSTANTS_PATH: &str = "gui/data/constants/weapons.xml";

/// The file path within the game's `res/` directory for common constants.
pub const COMMON_CONSTANTS_PATH: &str = "gui/data/constants/common.xml";

/// The file path within the game's `res/` directory for channel constants.
pub const CHANNEL_CONSTANTS_PATH: &str = "gui/data/constants/channel.xml";
