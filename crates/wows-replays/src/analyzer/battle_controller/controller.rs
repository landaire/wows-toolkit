use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

use tracing::warn;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
pub use wowsunpack::data::ship_config::ShipConfig;
use wowsunpack::data::ship_config::parse_ship_config_value;
use wowsunpack::game_params::types::CAPTAIN_SKILL_REWORK_VERSION;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;
use wowsunpack::rpc::typedefs::ArgValue;

static TIME_UNTIL_GAME_START: Duration = Duration::from_secs(30);

use crate::Rc;
use crate::RwCellExt;
use crate::analyzer::decoder::BuoyancyState;
use crate::analyzer::decoder::DeathCause;
use crate::analyzer::decoder::PlayerStateData;
use crate::analyzer::decoder::Recognized;
use crate::analyzer::decoder::WeaponType;
use crate::game_constants::GameConstants;
use crate::types::EntityId;
use crate::types::GameClock;
use crate::types::GameParamId;
use crate::types::PlayerId;
use crate::types::Relation;
use crate::types::VisibilityFlags;

use super::state::BuildingEntity;
use super::state::KillRecord;
use super::state::SmokeScreenEntity;

#[derive(Debug, Default, Clone, Serialize)]
pub struct Skills {
    aircraft_carrier: Vec<u8>,
    battleship: Vec<u8>,
    cruiser: Vec<u8>,
    destroyer: Vec<u8>,
    auxiliary: Vec<u8>,
    submarine: Vec<u8>,
}

impl Skills {
    pub fn submarine(&self) -> &[u8] {
        self.submarine.as_ref()
    }

    pub fn auxiliary(&self) -> &[u8] {
        self.auxiliary.as_ref()
    }

    pub fn destroyer(&self) -> &[u8] {
        self.destroyer.as_ref()
    }

    pub fn cruiser(&self) -> &[u8] {
        self.cruiser.as_ref()
    }

    pub fn battleship(&self) -> &[u8] {
        self.battleship.as_ref()
    }

    pub fn aircraft_carrier(&self) -> &[u8] {
        self.aircraft_carrier.as_ref()
    }

    pub fn for_species(&self, species: &Species) -> &[u8] {
        match species {
            Species::AirCarrier => &self.aircraft_carrier,
            Species::Battleship => &self.battleship,
            Species::Cruiser => &self.cruiser,
            Species::Destroyer => &self.destroyer,
            Species::Submarine => &self.submarine,
            Species::Auxiliary => &self.auxiliary,
            _ => &[],
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ShipLoadout {
    config: Option<ShipConfig>,
    skills: Option<Skills>,
}

impl ShipLoadout {
    pub fn skills(&self) -> Option<&Skills> {
        self.skills.as_ref()
    }

    pub fn config(&self) -> Option<&ShipConfig> {
        self.config.as_ref()
    }
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, Eq)]
pub enum ConnectionChangeKind {
    Connected,
    Disconnected,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConnectionChangeInfo {
    /// Duration from start of arena when the connection change
    /// event occurred
    at_game_duration: Duration,
    event_kind: ConnectionChangeKind,
    /// Whether or not this player had a death event when this connection change
    /// occurred
    had_death_event: bool,
}

impl ConnectionChangeInfo {
    pub fn new(at_game_duration: Duration, event_kind: ConnectionChangeKind, had_death_event: bool) -> Self {
        Self { at_game_duration, event_kind, had_death_event }
    }

    pub fn at_game_duration(&self) -> Duration {
        self.at_game_duration
    }

    pub fn event_kind(&self) -> ConnectionChangeKind {
        self.event_kind
    }

    pub fn had_death_event(&self) -> bool {
        self.had_death_event
    }
}

/// Players that were received from parsing the replay packets
pub struct Player {
    initial_state: PlayerStateData,
    end_state: crate::RwCell<PlayerStateData>,
    connection_change_info: crate::RwCell<Vec<ConnectionChangeInfo>>,
    vehicle: Rc<Param>,
    vehicle_entity: Option<VehicleEntity>,
    /// The relation of this player to the recording player
    relation: Relation,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let end_state = self.end_state.read_ref();
        let connection_change_info = self.connection_change_info.read_ref();
        f.debug_struct("Player")
            .field("initial_state", &self.initial_state)
            .field("end_state", &*end_state)
            .field("connection_change_info", &*connection_change_info)
            .field("vehicle", &self.vehicle)
            .field("vehicle_entity", &self.vehicle_entity)
            .field("relation", &self.relation)
            .finish()
    }
}

impl Clone for Player {
    fn clone(&self) -> Self {
        Self {
            initial_state: self.initial_state.clone(),
            end_state: crate::RwCell::new(self.end_state.read_ref().clone()),
            connection_change_info: crate::RwCell::new(self.connection_change_info.read_ref().clone()),
            vehicle: self.vehicle.clone(),
            vehicle_entity: self.vehicle_entity.clone(),
            relation: self.relation,
        }
    }
}

impl Serialize for Player {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let end_state = self.end_state.read_ref();
        let connection_change_info = self.connection_change_info.read_ref();
        let mut state = serializer.serialize_struct("Player", 5)?;
        state.serialize_field("initial_state", &self.initial_state)?;
        state.serialize_field("end_state", &*end_state)?;
        state.serialize_field("connection_change_info", &*connection_change_info)?;
        state.serialize_field("vehicle", &self.vehicle)?;
        state.serialize_field("relation", &self.relation)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Player {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PlayerHelper {
            initial_state: PlayerStateData,
            end_state: PlayerStateData,
            connection_change_info: Vec<ConnectionChangeInfo>,
            vehicle: Rc<Param>,
            relation: Relation,
        }

        let helper = PlayerHelper::deserialize(deserializer)?;
        Ok(Player {
            initial_state: helper.initial_state,
            end_state: crate::RwCell::new(helper.end_state),
            connection_change_info: crate::RwCell::new(helper.connection_change_info),
            vehicle: helper.vehicle,
            vehicle_entity: None,
            relation: helper.relation,
        })
    }
}

impl std::cmp::PartialEq for Player {
    fn eq(&self, other: &Self) -> bool {
        self.initial_state.db_id == other.initial_state.db_id && self.initial_state.realm == other.initial_state.realm
    }
}

impl std::cmp::Eq for Player {}

impl std::hash::Hash for Player {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.initial_state.db_id.hash(state);
        self.initial_state.realm.hash(state);
    }
}

impl Player {
    pub fn from_arena_player<G: ResourceLoader>(
        player: &PlayerStateData,
        metadata_player: &MetadataPlayer,
        resources: &G,
    ) -> Option<Player> {
        let vehicle = resources.game_param_by_id(metadata_player.vehicle().id()).or_else(|| {
            warn!(
                "could not find vehicle for player {:?} (shipId={})",
                metadata_player.name(),
                metadata_player.vehicle().id()
            );
            None
        })?;
        Some(Player {
            initial_state: player.clone(),
            end_state: crate::RwCell::new(player.clone()),
            vehicle_entity: None,
            connection_change_info: crate::RwCell::new(Vec::new()),
            vehicle,
            relation: metadata_player.relation(),
        })
    }

    /// Create a Player from a mid-battle spawn (e.g. Operations reinforcement wave).
    /// Uses the ship_params_id from the PlayerStateData directly since these players
    /// are not in the replay's JSON metadata.
    pub fn from_spawned_player<G: ResourceLoader>(
        player: &PlayerStateData,
        resources: &G,
        relation: Relation,
    ) -> Option<Player> {
        let ship_params_id = player.ship_params_id()?;
        let vehicle = resources.game_param_by_id(ship_params_id)?;
        Some(Player {
            initial_state: player.clone(),
            end_state: crate::RwCell::new(player.clone()),
            vehicle_entity: None,
            connection_change_info: crate::RwCell::new(Vec::new()),
            vehicle,
            relation,
        })
    }

    /// A list of events for when this player connected or disconnected
    /// from the match.
    pub fn connection_change_info(&self) -> crate::RwCellReadGuard<'_, Vec<ConnectionChangeInfo>> {
        self.connection_change_info.read_ref()
    }

    pub fn connection_change_info_mut(&self) -> crate::RwCellWriteGuard<'_, Vec<ConnectionChangeInfo>> {
        self.connection_change_info.write_ref()
    }

    pub fn end_state(&self) -> crate::RwCellReadGuard<'_, PlayerStateData> {
        self.end_state.read_ref()
    }

    pub fn end_state_mut(&self) -> crate::RwCellWriteGuard<'_, PlayerStateData> {
        self.end_state.write_ref()
    }

    pub fn initial_state(&self) -> &PlayerStateData {
        &self.initial_state
    }

    pub fn relation(&self) -> Relation {
        self.relation
    }

    pub fn vehicle_entity(&self) -> Option<&VehicleEntity> {
        self.vehicle_entity.as_ref()
    }

    pub fn set_vehicle_entity(&mut self, vehicle_entity: Option<VehicleEntity>) {
        self.vehicle_entity = vehicle_entity;
    }

    pub fn vehicle(&self) -> &Param {
        &self.vehicle
    }

    pub fn is_bot(&self) -> bool {
        self.initial_state.is_bot
    }
}

#[derive(Debug)]
/// Players that were parsed from just the replay metadata
pub struct MetadataPlayer {
    id: PlayerId,
    name: String,
    relation: Relation,
    vehicle: Rc<Param>,
}

impl MetadataPlayer {
    pub fn new(id: PlayerId, name: String, relation: Relation, vehicle: Rc<Param>) -> Self {
        Self { id, name, relation, vehicle }
    }

    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    pub fn relation(&self) -> Relation {
        self.relation
    }

    pub fn vehicle(&self) -> &Param {
        self.vehicle.as_ref()
    }

    pub fn id(&self) -> PlayerId {
        self.id
    }
}

pub type SharedPlayer = Rc<MetadataPlayer>;
#[allow(dead_code)]
type MethodName = String;

#[derive(Debug, Clone, Copy)]
pub enum EntityType {
    Building,
    BattleEntity,
    BattleLogic,
    Vehicle,
    InteractiveZone,
    SmokeScreen,
}

impl std::str::FromStr for EntityType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Building" => Ok(Self::Building),
            "BattleEntity" => Ok(Self::BattleEntity),
            "BattleLogic" => Ok(Self::BattleLogic),
            "Vehicle" => Ok(Self::Vehicle),
            "InteractiveZone" => Ok(Self::InteractiveZone),
            "SmokeScreen" => Ok(Self::SmokeScreen),
            _ => Err(format!("Unknown entity type: {s}")),
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize)]
#[serde(tag = "type", content = "team_id")]
pub enum BattleResult {
    /// A win, and which team won (inferred to be the team of the player)
    Win(i8),
    /// A loss, and which other team won
    Loss(i8),
    Draw,
}

#[derive(Debug, Clone, Copy)]
pub struct DamageEvent {
    pub amount: f32,
    pub victim: EntityId,
    pub clock: GameClock,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatChannel {
    Division,
    Global,
    Team,
    System,
    Unknown(String),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GameMessage {
    pub clock: GameClock,
    pub sender_relation: Option<Relation>,
    pub sender_name: String,
    pub channel: ChatChannel,
    pub message: String,
    pub entity_id: EntityId,
    pub player: Option<Rc<Player>>,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct AAAura {
    id: u32,
    enabled: bool,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct VehicleState {
    /// TODO
    buffs: Option<()>,
    vehicle_visual_state: u8,
    /// TODO
    battery: Option<()>,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct CrewModifiersCompactParams {
    params_id: GameParamId,
    is_in_adaption: bool,
    learned_skills: Skills,
}

impl CrewModifiersCompactParams {
    pub fn params_id(&self) -> GameParamId {
        self.params_id
    }

    pub fn learned_skills(&self) -> &Skills {
        &self.learned_skills
    }
}

/// A property update's arguments: either the decoded `props` map an entity
/// create carries, or the single named value an entity property packet carries.
///
/// The single case exists because it is the overwhelmingly common one: an
/// `update_from_args` body probes every key it knows about (around fifty for
/// `VehicleProps`) to find the one that is present. Holding that one value in a
/// map made each probe hash a string, and getting it into the map cost an
/// allocation and a deep clone of a value that can be a nested tree. Answering
/// from the pair directly makes a probe a string comparison against a borrow.
#[derive(Clone, Copy)]
enum PropertyArgs<'a, 'argtype> {
    Map(&'a HashMap<&'argtype str, ArgValue<'argtype>>),
    Single { name: &'a str, value: &'a ArgValue<'argtype> },
}

impl<'a, 'argtype> PropertyArgs<'a, 'argtype> {
    fn get(&self, key: &str) -> Option<&'a ArgValue<'argtype>> {
        match *self {
            Self::Map(map) => map.get(key),
            Self::Single { name, value } => (name == key).then_some(value),
        }
    }

    fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

trait UpdateFromReplayArgs {
    fn update_by_name(&mut self, name: &str, value: &ArgValue<'_>, version: Version, constants: &GameConstants) {
        self.update_from_args(PropertyArgs::Single { name, value }, version, constants);
    }

    fn update_from_args(&mut self, args: PropertyArgs<'_, '_>, version: Version, constants: &GameConstants);
}

/// Lenient typed setter for a property value already matched by name: a
/// field present with an unexpected type (common across game versions) is
/// left at its default instead of panicking.
macro_rules! apply_prop_value {
    ($set_var:expr, $value:ident, bool) => {
        if let Some(b) = $value.uint_8_ref() {
            $set_var = (*b) != 0
        }
    };
    ($set_var:expr, $value:ident, f32) => {
        // Accept Float32, Float64, or integer encodings. Older clients vary the
        // wire type (e.g. 0.9.10 sends `health` as Float64); a strict
        // `float_32_ref()` would silently drop those, freezing the value (HP bars
        // never moved). `as_f32()` converts any numeric variant.
        if let Some(converted) = $value.as_f32() {
            $set_var = converted
        }
    };
    ($set_var:expr, $value:ident, Vec<u8>) => {
        if let Some(blob) = $value.blob_ref() {
            $set_var = blob.to_vec()
        }
    };
    ($set_var:expr, $value:ident, i8) => {
        apply_prop_value!($set_var, $value, int_8_ref)
    };
    ($set_var:expr, $value:ident, u8) => {
        apply_prop_value!($set_var, $value, uint_8_ref)
    };
    ($set_var:expr, $value:ident, u16) => {
        apply_prop_value!($set_var, $value, uint_16_ref)
    };
    ($set_var:expr, $value:ident, u32) => {
        apply_prop_value!($set_var, $value, uint_32_ref)
    };
    ($set_var:expr, $value:ident, $conversion_func:ident) => {
        if let Some(converted) = $value.$conversion_func() {
            $set_var = converted.clone()
        }
    };
}

macro_rules! arg_value_to_type {
    ($args:ident, $key:expr, String) => {
        arg_value_to_type!($args, $key, string_ref, String).clone()
    };
    ($args:ident, $key:expr, i8) => {
        *arg_value_to_type!($args, $key, int_8_ref, i8)
    };
    ($args:ident, $key:expr, i16) => {
        *arg_value_to_type!($args, $key, int_16_ref, i16)
    };
    ($args:ident, $key:expr, i32) => {
        *arg_value_to_type!($args, $key, int_32_ref, i32)
    };
    ($args:ident, $key:expr, u8) => {
        *arg_value_to_type!($args, $key, uint_8_ref, u8)
    };
    ($args:ident, $key:expr, u16) => {
        *arg_value_to_type!($args, $key, uint_16_ref, u16)
    };
    ($args:ident, $key:expr, u32) => {
        *arg_value_to_type!($args, $key, uint_32_ref, u32)
    };
    ($args:ident, $key:expr, bool) => {
        (*arg_value_to_type!($args, $key, uint_8_ref, u8)) != 0
    };
    ($args:ident, $key:expr, &[()]) => {
        arg_value_to_type!($args, $key, array_ref, &[()])
    };
    ($args:ident, $key:expr, &[u8]) => {
        arg_value_to_type!($args, $key, blob_ref, &[()]).as_ref()
    };
    ($args:ident, $key:expr, HashMap<(), ()>) => {
        arg_value_to_type!($args, $key, fixed_dict_ref, HashMap<(), ()>)
    };
    ($args:ident, $key:expr, $conversion_func:ident, $ty:ty) => {
        $args
            .get($key)
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .$conversion_func()
            .unwrap_or_else(|| panic!("{} is not a {}", $key, stringify!($ty)))
    };
}

impl UpdateFromReplayArgs for CrewModifiersCompactParams {
    fn update_from_args(&mut self, args: PropertyArgs<'_, '_>, version: Version, _constants: &GameConstants) {
        const PARAMS_ID_KEY: &str = "paramsId";
        const IS_IN_ADAPTION_KEY: &str = "isInAdaption";
        const LEARNED_SKILLS_KEY: &str = "learnedSkills";

        if args.contains_key(PARAMS_ID_KEY) {
            self.params_id = GameParamId::from(arg_value_to_type!(args, PARAMS_ID_KEY, u32));
        }
        if args.contains_key(IS_IN_ADAPTION_KEY) {
            self.is_in_adaption = arg_value_to_type!(args, IS_IN_ADAPTION_KEY, bool);
        }

        // The captain-skill rework changed the `learnedSkills` shape. Gate on the
        // version (the boundary is exact -- 0.9.x is always a bitmask, 0.10.0+ is
        // always per-species arrays) and still type-check inside each branch so a
        // malformed/missing value yields an empty skill set rather than a panic.
        let rework = Version::base(
            CAPTAIN_SKILL_REWORK_VERSION.0,
            CAPTAIN_SKILL_REWORK_VERSION.1,
            CAPTAIN_SKILL_REWORK_VERSION.2,
        );
        if version.is_at_least(&rework) {
            // Post-rework: per-species arrays of skill-type ids.
            if let Some(learned_skills) = args.get(LEARNED_SKILLS_KEY).and_then(|v| v.array_ref()) {
                let skills_from_idx = |idx: usize| -> Vec<u8> {
                    learned_skills
                        .get(idx)
                        .and_then(|v| v.array_ref())
                        .map(|a| a.iter().filter_map(|x| x.uint_8_ref().copied()).collect())
                        .unwrap_or_default()
                };

                self.learned_skills = Skills {
                    aircraft_carrier: skills_from_idx(0),
                    battleship: skills_from_idx(1),
                    cruiser: skills_from_idx(2),
                    destroyer: skills_from_idx(3),
                    auxiliary: skills_from_idx(4),
                    submarine: skills_from_idx(5),
                };
            }
        } else if let Some(ArgValue::Uint64(mask)) = args.get(LEARNED_SKILLS_KEY) {
            // Pre-rework clients (<= 0.9.x) encode learned skills as a single bitmask
            // over skill-type ids -- with no per-species split (a captain carried one
            // shared skill set). The mask is 1-indexed: `skillType` values start at 1,
            // so bit `i` set means skill type `i + 1` is learned. (Decoding bit `i` as
            // skill type `i` resolves to wrong/nonexistent skills -- e.g. a Destroyer
            // captain appearing to have CV skills.) Apply the decoded list to every
            // species so it resolves regardless of ship type.
            let ids: Vec<u8> = (0..64u32).filter(|i| mask & (1u64 << i) != 0).map(|i| (i + 1) as u8).collect();
            self.learned_skills = Skills {
                aircraft_carrier: ids.clone(),
                battleship: ids.clone(),
                cruiser: ids.clone(),
                destroyer: ids.clone(),
                auxiliary: ids.clone(),
                submarine: ids,
            };
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct VehicleProps {
    ignore_map_borders: bool,
    air_defense_dispersion_radius: f32,
    death_settings: Vec<u8>,
    owner: u32,
    atba_targets: Vec<u32>,
    effects: Vec<String>,
    crew_modifiers_compact_params: CrewModifiersCompactParams,
    laser_target_local_pos: u16,
    anti_air_auras: Vec<AAAura>,
    selected_weapon: Recognized<WeaponType>,
    regeneration_health: f32,
    is_on_forsage: bool,
    is_in_rage_mode: bool,
    has_air_targets_in_range: bool,
    torpedo_local_pos: u16,
    /// TODO
    air_defense_target_ids: Vec<()>,
    buoyancy: f32,
    max_health: f32,
    max_health_explicit: bool,
    rudders_angle: f32,
    draught: f32,
    target_local_pos: u16,
    triggered_skills_data: Vec<u8>,
    regenerated_health: f32,
    blocked_controls: u8,
    is_invisible: bool,
    is_fog_horn_on: bool,
    server_speed_raw: u16,
    regen_crew_hp_limit: f32,
    /// TODO
    miscs_presets_status: Vec<()>,
    buoyancy_current_waterline: f32,
    is_alive: bool,
    is_bot: bool,
    /// `None` until a `visibilityFlags` value is seen. The property first
    /// appears on the Vehicle entity in build 4462593 (10.8.0), so on older
    /// replays it stays `None`, which is not the same as "never detected".
    visibility_flags: Option<VisibilityFlags>,
    heat_infos: Vec<()>,
    buoyancy_rudder_index: u8,
    is_anti_air_mode: bool,
    speed_sign_dir: i8,
    oil_leak_state: u8,
    /// TODO
    sounds: Vec<()>,
    ship_config: ShipConfig,
    wave_local_pos: u16,
    has_active_main_squadron: bool,
    weapon_lock_flags: u16,
    deep_rudders_angle: f32,
    /// TODO
    debug_text: Vec<()>,
    health: f32,
    engine_dir: i8,
    state: VehicleState,
    team_id: i8,
    buoyancy_current_state: Recognized<BuoyancyState>,
    ui_enabled: bool,
    respawn_time: u16,
    engine_power: u8,
    max_server_speed_raw: u32,
    burning_flags: u16,
}

impl Default for VehicleProps {
    fn default() -> Self {
        Self {
            ignore_map_borders: false,
            air_defense_dispersion_radius: 0.0,
            death_settings: Vec::new(),
            owner: 0,
            atba_targets: Vec::new(),
            effects: Vec::new(),
            crew_modifiers_compact_params: CrewModifiersCompactParams::default(),
            laser_target_local_pos: 0,
            anti_air_auras: Vec::new(),
            selected_weapon: Recognized::Known(WeaponType::Artillery),
            regeneration_health: 0.0,
            is_on_forsage: false,
            is_in_rage_mode: false,
            has_air_targets_in_range: false,
            torpedo_local_pos: 0,
            air_defense_target_ids: Vec::new(),
            buoyancy: 0.0,
            max_health: 0.0,
            max_health_explicit: false,
            rudders_angle: 0.0,
            draught: 0.0,
            target_local_pos: 0,
            triggered_skills_data: Vec::new(),
            regenerated_health: 0.0,
            blocked_controls: 0,
            is_invisible: false,
            is_fog_horn_on: false,
            server_speed_raw: 0,
            regen_crew_hp_limit: 0.0,
            miscs_presets_status: Vec::new(),
            buoyancy_current_waterline: 0.0,
            is_alive: false,
            is_bot: false,
            visibility_flags: None,
            heat_infos: Vec::new(),
            buoyancy_rudder_index: 0,
            is_anti_air_mode: false,
            speed_sign_dir: 0,
            oil_leak_state: 0,
            sounds: Vec::new(),
            ship_config: ShipConfig::default(),
            wave_local_pos: 0,
            has_active_main_squadron: false,
            weapon_lock_flags: 0,
            deep_rudders_angle: 0.0,
            debug_text: Vec::new(),
            health: 0.0,
            engine_dir: 0,
            state: VehicleState::default(),
            team_id: 0,
            buoyancy_current_state: Recognized::Known(BuoyancyState::Surface),
            ui_enabled: false,
            respawn_time: 0,
            engine_power: 0,
            max_server_speed_raw: 0,
            burning_flags: 0,
        }
    }
}

impl VehicleProps {
    /// Build a `VehicleProps` from a raw EntityCreate `props` map. Public so
    /// scanners (see `merged::scan_vehicle_facts`) can extract per-entity
    /// initial-state facts without going through a full controller.
    pub fn from_create_props(props: &HashMap<&str, ArgValue<'_>>, version: Version, constants: &GameConstants) -> Self {
        let mut vp = VehicleProps::default();
        vp.update_from_args(props, version, constants);
        vp
    }

    pub fn ignore_map_borders(&self) -> bool {
        self.ignore_map_borders
    }

    pub fn air_defense_dispersion_radius(&self) -> f32 {
        self.air_defense_dispersion_radius
    }

    pub fn death_settings(&self) -> &[u8] {
        self.death_settings.as_ref()
    }

    pub fn owner(&self) -> u32 {
        self.owner
    }

    pub fn atba_targets(&self) -> &[u32] {
        self.atba_targets.as_ref()
    }

    pub fn effects(&self) -> &[String] {
        self.effects.as_ref()
    }

    pub fn crew_modifiers_compact_params(&self) -> &CrewModifiersCompactParams {
        &self.crew_modifiers_compact_params
    }

    pub fn laser_target_local_pos(&self) -> u16 {
        self.laser_target_local_pos
    }

    pub fn anti_air_auras(&self) -> &[AAAura] {
        self.anti_air_auras.as_ref()
    }

    pub fn selected_weapon(&self) -> &Recognized<WeaponType> {
        &self.selected_weapon
    }

    pub fn regeneration_health(&self) -> f32 {
        self.regeneration_health
    }

    pub fn is_on_forsage(&self) -> bool {
        self.is_on_forsage
    }

    pub fn is_in_rage_mode(&self) -> bool {
        self.is_in_rage_mode
    }

    pub fn has_air_targets_in_range(&self) -> bool {
        self.has_air_targets_in_range
    }

    pub fn torpedo_local_pos(&self) -> u16 {
        self.torpedo_local_pos
    }

    pub fn air_defense_target_ids(&self) -> &[()] {
        self.air_defense_target_ids.as_ref()
    }

    pub fn buoyancy(&self) -> f32 {
        self.buoyancy
    }

    pub fn max_health(&self) -> f32 {
        self.max_health
    }

    pub fn rudders_angle(&self) -> f32 {
        self.rudders_angle
    }

    pub fn draught(&self) -> f32 {
        self.draught
    }

    pub fn target_local_pos(&self) -> u16 {
        self.target_local_pos
    }

    pub fn triggered_skills_data(&self) -> &[u8] {
        self.triggered_skills_data.as_ref()
    }

    pub fn regenerated_health(&self) -> f32 {
        self.regenerated_health
    }

    pub fn blocked_controls(&self) -> u8 {
        self.blocked_controls
    }

    pub fn is_invisible(&self) -> bool {
        self.is_invisible
    }

    pub fn is_fog_horn_on(&self) -> bool {
        self.is_fog_horn_on
    }

    pub fn server_speed_raw(&self) -> u16 {
        self.server_speed_raw
    }

    pub fn regen_crew_hp_limit(&self) -> f32 {
        self.regen_crew_hp_limit
    }

    pub fn miscs_presets_status(&self) -> &[()] {
        self.miscs_presets_status.as_ref()
    }

    pub fn buoyancy_current_waterline(&self) -> f32 {
        self.buoyancy_current_waterline
    }

    pub fn is_alive(&self) -> bool {
        self.is_alive
    }

    pub fn is_bot(&self) -> bool {
        self.is_bot
    }

    pub fn visibility_flags(&self) -> Option<VisibilityFlags> {
        self.visibility_flags
    }

    pub fn heat_infos(&self) -> &[()] {
        self.heat_infos.as_ref()
    }

    pub fn buoyancy_rudder_index(&self) -> u8 {
        self.buoyancy_rudder_index
    }

    pub fn is_anti_air_mode(&self) -> bool {
        self.is_anti_air_mode
    }

    pub fn speed_sign_dir(&self) -> i8 {
        self.speed_sign_dir
    }

    pub fn oil_leak_state(&self) -> u8 {
        self.oil_leak_state
    }

    pub fn sounds(&self) -> &[()] {
        self.sounds.as_ref()
    }

    pub fn ship_config(&self) -> &ShipConfig {
        &self.ship_config
    }

    pub fn wave_local_pos(&self) -> u16 {
        self.wave_local_pos
    }

    pub fn has_active_main_squadron(&self) -> bool {
        self.has_active_main_squadron
    }

    pub fn weapon_lock_flags(&self) -> u16 {
        self.weapon_lock_flags
    }

    pub fn deep_rudders_angle(&self) -> f32 {
        self.deep_rudders_angle
    }

    pub fn debug_text(&self) -> &[()] {
        self.debug_text.as_ref()
    }

    pub fn health(&self) -> f32 {
        self.health
    }

    pub fn engine_dir(&self) -> i8 {
        self.engine_dir
    }

    pub fn state(&self) -> &VehicleState {
        &self.state
    }

    pub fn team_id(&self) -> i8 {
        self.team_id
    }

    pub fn buoyancy_current_state(&self) -> &Recognized<BuoyancyState> {
        &self.buoyancy_current_state
    }

    pub fn ui_enabled(&self) -> bool {
        self.ui_enabled
    }

    pub fn respawn_time(&self) -> u16 {
        self.respawn_time
    }

    pub fn engine_power(&self) -> u8 {
        self.engine_power
    }

    pub fn max_server_speed_raw(&self) -> u32 {
        self.max_server_speed_raw
    }

    pub fn burning_flags(&self) -> u16 {
        self.burning_flags
    }

    /// Apply a single named property update (mirrors `UpdateFromReplayArgs::update_by_name`).
    pub fn update_by_name(&mut self, name: &str, value: &ArgValue<'_>, version: Version, constants: &GameConstants) {
        <Self as UpdateFromReplayArgs>::update_by_name(self, name, value, version, constants);
    }

    /// Apply a batch of named property updates (mirrors `UpdateFromReplayArgs::update_from_args`).
    pub fn update_from_args(
        &mut self,
        args: &HashMap<&str, ArgValue<'_>>,
        version: Version,
        constants: &GameConstants,
    ) {
        <Self as UpdateFromReplayArgs>::update_from_args(self, PropertyArgs::Map(args), version, constants);
    }

    /// Seed health from max_health when arena state omits a live health value.
    pub fn seed_initial_health(&mut self) {
        if self.health == 0.0 && self.max_health > 0.0 {
            self.health = self.max_health;
        }
    }
}

impl VehicleProps {
    /// Apply one named property. Dispatching on the name once (instead of
    /// probing every known key against the incoming one, as the old
    /// `update_from_args` body did) makes a single-property update one string
    /// match rather than ~fifty comparisons.
    fn apply_prop(&mut self, name: &str, value: &ArgValue<'_>, version: Version, constants: &GameConstants) {
        match name {
            "ignoreMapBorders" => apply_prop_value!(self.ignore_map_borders, value, bool),
            "airDefenseDispRadius" => apply_prop_value!(self.air_defense_dispersion_radius, value, f32),
            "deathSettings" => apply_prop_value!(self.death_settings, value, Vec<u8>),
            "owner" => {
                self.owner = *value.int_32_ref().unwrap_or_else(|| panic!("owner is not a i32")) as u32;
            }
            "atbaTargets" => {
                self.atba_targets = value
                    .array_ref()
                    .unwrap_or_else(|| panic!("atbaTargets is not an array"))
                    .iter()
                    .map(|elem| *elem.uint_32_ref().expect("atbaTargets elem is not a u32"))
                    .collect();
            }
            "effects" => {
                self.effects = value
                    .array_ref()
                    .unwrap_or_else(|| panic!("effects is not an array"))
                    .iter()
                    .map(|elem| {
                        String::from_utf8(elem.string_ref().expect("effects elem is not a string").to_vec())
                            .expect("could not convert effects elem to string")
                    })
                    .collect();
            }
            "crewModifiersCompactParams" => {
                let dict = value.fixed_dict_ref().unwrap_or_else(|| panic!("crewModifiersCompactParams is not a dict"));
                self.crew_modifiers_compact_params.update_from_args(PropertyArgs::Map(dict), version, constants);
            }
            "laserTargetLocalPos" => apply_prop_value!(self.laser_target_local_pos, value, u16),
            // TODO: AntiAirAuras
            "selectedWeapon" => {
                let id = *value.uint_32_ref().unwrap_or_else(|| panic!("selectedWeapon is not a u32"));
                if let Some(wt) = WeaponType::from_id(id as i32, constants.ships(), version) {
                    self.selected_weapon = wt;
                }
            }
            "isOnForsage" => apply_prop_value!(self.is_on_forsage, value, bool),
            "isInRageMode" => apply_prop_value!(self.is_in_rage_mode, value, bool),
            "hasAirTargetsInRange" => apply_prop_value!(self.has_air_targets_in_range, value, bool),
            "torpedoLocalPos" => apply_prop_value!(self.torpedo_local_pos, value, u16),
            // TODO: airDefenseTargetIds
            "buoyancy" => apply_prop_value!(self.buoyancy, value, f32),
            "maxHealth" => {
                apply_prop_value!(self.max_health, value, f32);
                self.max_health_explicit = true;
            }
            "draught" => apply_prop_value!(self.draught, value, f32),
            "ruddersAngle" => apply_prop_value!(self.rudders_angle, value, f32),
            "targetLocalPos" => apply_prop_value!(self.target_local_pos, value, u16),
            "triggeredSkillsData" => apply_prop_value!(self.triggered_skills_data, value, Vec<u8>),
            "regeneratedHealth" => apply_prop_value!(self.regenerated_health, value, f32),
            "regenerationHealth" => apply_prop_value!(self.regeneration_health, value, f32),
            "blockedControls" => apply_prop_value!(self.blocked_controls, value, u8),
            "isInvisible" => apply_prop_value!(self.is_invisible, value, bool),
            "isFogHornOn" => apply_prop_value!(self.is_fog_horn_on, value, bool),
            "serverSpeedRaw" => apply_prop_value!(self.server_speed_raw, value, u16),
            "regenCrewHpLimit" => apply_prop_value!(self.regen_crew_hp_limit, value, f32),
            // TODO: miscs_presets_status
            "buoyancyCurrentWaterline" => apply_prop_value!(self.buoyancy_current_waterline, value, f32),
            "isAlive" => apply_prop_value!(self.is_alive, value, bool),
            "isBot" => apply_prop_value!(self.is_bot, value, bool),
            // Read leniently: a field present with an unexpected width across game
            // versions leaves the flags untouched rather than aborting the parse.
            "visibilityFlags" => {
                if let Some(flags) = value.as_u32() {
                    self.visibility_flags = Some(VisibilityFlags::new(flags));
                }
            }
            // TODO: heatInfos
            "buoyancyRudderIndex" => apply_prop_value!(self.buoyancy_rudder_index, value, u8),
            "isAntiAirMode" => apply_prop_value!(self.is_anti_air_mode, value, bool),
            "speedSignDir" => apply_prop_value!(self.speed_sign_dir, value, i8),
            "oilLeakState" => apply_prop_value!(self.oil_leak_state, value, u8),
            // TODO: sounds
            // Modern builds encode the ship config as a top-level blob; <=0.11.x wrap it
            // in a FIXED_DICT { shipId, cd }. `parse_ship_config_value` handles both.
            // Skip rather than panic if absent or unparseable.
            "shipConfig" => {
                if let Some(ship_config) = parse_ship_config_value(value, &version) {
                    self.ship_config = ship_config;
                }
            }
            "waveLocalPos" => apply_prop_value!(self.wave_local_pos, value, u16),
            "hasActiveMainSquadron" => apply_prop_value!(self.has_active_main_squadron, value, bool),
            "weaponLockFlags" => apply_prop_value!(self.weapon_lock_flags, value, u16),
            "deepRuddersAngle" => apply_prop_value!(self.deep_rudders_angle, value, f32),
            // TODO: debugText
            "health" => {
                apply_prop_value!(self.health, value, f32);
                // Old clients never broadcast maxHealth; treat the highest observed
                // health as the max so the HP-bar fraction stays valid. Gated on
                // never having seen an explicit maxHealth so a legitimate mid-match
                // decrease is not overridden.
                if !self.max_health_explicit && self.health > self.max_health {
                    self.max_health = self.health;
                }
            }
            "engineDir" => apply_prop_value!(self.engine_dir, value, i8),
            // TODO: state
            "teamId" => apply_prop_value!(self.team_id, value, i8),
            "buoyancyCurrentState" => {
                let id = *value.uint_8_ref().unwrap_or_else(|| panic!("buoyancyCurrentState is not a u8"));
                if let Some(ds) = BuoyancyState::from_id(id as i32, constants.battle(), version) {
                    self.buoyancy_current_state = ds;
                }
            }
            "uiEnabled" => apply_prop_value!(self.ui_enabled, value, bool),
            "respawnTime" => apply_prop_value!(self.respawn_time, value, u16),
            "enginePower" => apply_prop_value!(self.engine_power, value, u8),
            "maxServerSpeedRaw" => apply_prop_value!(self.max_server_speed_raw, value, u32),
            // Read leniently, matching visibilityFlags: a strict u16 read drops the
            // field on any build that encodes it at another width, and a burn mask
            // stuck at 0 reads downstream as "nothing ever burned" rather than
            // "unknown". Bits 0-9 are all that is defined, so narrowing to u16
            // keeps every meaningful bit.
            "burningFlags" => {
                if let Some(flags) = value.as_u32() {
                    self.burning_flags = flags as u16;
                }
            }
            _ => {}
        }
    }
}

impl UpdateFromReplayArgs for VehicleProps {
    fn update_by_name(&mut self, name: &str, value: &ArgValue<'_>, version: Version, constants: &GameConstants) {
        self.apply_prop(name, value, version, constants);
    }

    fn update_from_args(&mut self, args: PropertyArgs<'_, '_>, version: Version, constants: &GameConstants) {
        match args {
            PropertyArgs::Single { name, value } => self.apply_prop(name, value, version, constants),
            PropertyArgs::Map(map) => {
                for (name, value) in map.iter() {
                    self.apply_prop(name, value, version, constants);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeathInfo {
    /// Time lived in the game. This may not be accurate if a game rejoin occurs
    /// as there's no known way to detect this event.
    time_lived: Duration,
    killer: EntityId,
    cause: Recognized<DeathCause>,
}

impl DeathInfo {
    pub fn time_lived(&self) -> Duration {
        self.time_lived
    }

    pub fn killer(&self) -> EntityId {
        self.killer
    }

    pub fn cause(&self) -> &Recognized<DeathCause> {
        &self.cause
    }
}

impl From<&KillRecord> for DeathInfo {
    fn from(kill: &KillRecord) -> Self {
        let timestamp = kill.clock.to_duration();
        let time_lived =
            if timestamp > TIME_UNTIL_GAME_START { timestamp - TIME_UNTIL_GAME_START } else { Duration::from_secs(0) };

        DeathInfo { time_lived, killer: kill.killer, cause: kill.cause.clone() }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VehicleEntity {
    id: EntityId,
    visibility_changed_at: f32,
    props: VehicleProps,
    captain: Option<Rc<Param>>,
    damage: f64,
    death_info: Option<DeathInfo>,
    results_info: Option<serde_json::Value>,
    frags: Vec<DeathInfo>,
}

impl VehicleEntity {
    /// Assemble a fully-populated vehicle record from externally accumulated state.
    /// `visibility_changed_at` is always 0.0 in the controller; callers pass 0.0.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: EntityId,
        visibility_changed_at: f32,
        props: VehicleProps,
        captain: Option<Rc<Param>>,
        damage: f64,
        death_info: Option<DeathInfo>,
        results_info: Option<serde_json::Value>,
        frags: Vec<DeathInfo>,
    ) -> Self {
        Self { id, visibility_changed_at, props, captain, damage, death_info, results_info, frags }
    }

    pub fn id(&self) -> EntityId {
        self.id
    }

    pub fn props(&self) -> &VehicleProps {
        &self.props
    }

    pub fn commander_id(&self) -> GameParamId {
        self.props.crew_modifiers_compact_params.params_id
    }

    pub fn commander_skills(&self, vehicle_species: Species) -> Option<Vec<&CrewSkill>> {
        let skills = &self.props.crew_modifiers_compact_params.learned_skills;
        let skills_for_species = match vehicle_species {
            Species::AirCarrier => skills.aircraft_carrier.as_slice(),
            Species::Battleship => skills.battleship.as_slice(),
            Species::Cruiser => skills.cruiser.as_slice(),
            Species::Destroyer => skills.destroyer.as_slice(),
            Species::Submarine => skills.submarine.as_slice(),
            _ => return None,
        };

        let captain = self.captain()?.data().crew_ref().expect("captain is not a crew?");

        let skills = skills_for_species
            .iter()
            .filter_map(|skill_type| {
                let skill = captain.skill_by_type(wowsunpack::game_params::types::CrewSkillType::from(*skill_type));
                if skill.is_none() {
                    tracing::warn!(
                        skill_type,
                        captain_id = %self.commander_id(),
                        "captain definition is missing learned skill type"
                    );
                }
                skill
            })
            .collect();

        Some(skills)
    }

    pub fn commander_skills_raw(&self, vehicle_species: Species) -> &[u8] {
        let skills = &self.props.crew_modifiers_compact_params.learned_skills;
        match vehicle_species {
            Species::AirCarrier => skills.aircraft_carrier.as_slice(),
            Species::Battleship => skills.battleship.as_slice(),
            Species::Cruiser => skills.cruiser.as_slice(),
            Species::Destroyer => skills.destroyer.as_slice(),
            Species::Submarine => skills.submarine.as_slice(),
            _ => &[],
        }
    }

    pub fn captain(&self) -> Option<&Param> {
        self.captain.as_ref().map(|rc| rc.as_ref())
    }

    pub fn damage(&self) -> f64 {
        self.damage
    }

    pub fn death_info(&self) -> Option<&DeathInfo> {
        self.death_info.as_ref()
    }

    pub fn results_info(&self) -> Option<&serde_json::Value> {
        self.results_info.as_ref()
    }

    pub fn frags(&self) -> &[DeathInfo] {
        &self.frags
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntityKind {
    Vehicle,
    Building,
    SmokeScreen,
}

#[derive(Debug)]
pub enum Entity {
    Vehicle(Rc<RefCell<VehicleEntity>>),
    Building(Rc<RefCell<BuildingEntity>>),
    SmokeScreen(Rc<RefCell<SmokeScreenEntity>>),
}

impl Entity {
    pub fn vehicle_ref(&self) -> Option<&Rc<RefCell<VehicleEntity>>> {
        match self {
            Entity::Vehicle(v) => Some(v),
            _ => None,
        }
    }

    pub fn building_ref(&self) -> Option<&Rc<RefCell<BuildingEntity>>> {
        match self {
            Entity::Building(b) => Some(b),
            _ => None,
        }
    }

    pub fn smoke_screen_ref(&self) -> Option<&Rc<RefCell<SmokeScreenEntity>>> {
        match self {
            Entity::SmokeScreen(s) => Some(s),
            _ => None,
        }
    }

    pub fn kind(&self) -> EntityKind {
        match self {
            Entity::Vehicle(_ref_cell) => EntityKind::Vehicle,
            Entity::Building(_ref_cell) => EntityKind::Building,
            Entity::SmokeScreen(_ref_cell) => EntityKind::SmokeScreen,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use wowsunpack::data::Version;
    use wowsunpack::rpc::typedefs::ArgValue;

    use crate::game_constants::GameConstants;

    use super::VehicleProps;

    #[test]
    fn update_from_args_sets_regeneration_health() {
        let mut props = VehicleProps::default();
        let version = Version::default();
        let constants = GameConstants::defaults();
        let args: HashMap<&str, ArgValue<'_>> =
            [("regenerationHealth", ArgValue::Float32(2295.0))].into_iter().collect();
        props.update_from_args(&args, version, &constants);
        assert_eq!(props.regeneration_health(), 2295.0);
    }

    /// `burningFlags` must be read leniently across builds. A strict `Uint16`
    /// read leaves the mask at 0 on any other integer width, and a zero burn
    /// mask does not read as "unknown" downstream, it reads as "nothing was
    /// ever burning" and biases the fire statistic instead of emptying it.
    #[test]
    fn burning_flags_accepts_every_integer_width() {
        let version = Version::default();
        let constants = GameConstants::defaults();
        for value in
            [ArgValue::Uint8(0b0101), ArgValue::Uint16(0b0101), ArgValue::Uint32(0b0101), ArgValue::Int32(0b0101)]
        {
            let mut props = VehicleProps::default();
            let args: HashMap<&str, ArgValue<'_>> = [("burningFlags", value.clone())].into_iter().collect();
            props.update_from_args(&args, version, &constants);
            assert_eq!(props.burning_flags(), 0b0101, "burningFlags dropped for {value:?}");
        }
    }
}
