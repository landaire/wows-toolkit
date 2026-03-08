use std::cell::RefCell;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

use tracing::Level;
use tracing::debug;
use tracing::span;
use tracing::trace;
use tracing::warn;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
pub use wowsunpack::data::ship_config::ShipConfig;
use wowsunpack::data::ship_config::parse_ship_config;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::BattleStage;
use wowsunpack::game_types::BattleType;
use wowsunpack::game_types::Ribbon;
use wowsunpack::rpc::typedefs::ArgValue;

static TIME_UNTIL_GAME_START: Duration = Duration::from_secs(30);

use crate::Rc;
use crate::ReplayMeta;
use crate::RwCellExt;
use crate::analyzer::analyzer::Analyzer;
use crate::analyzer::decoder::BuoyancyState;
use crate::analyzer::decoder::ChatMessageExtra;
use crate::analyzer::decoder::DamageStatCategory;
use crate::analyzer::decoder::DamageStatEntry;
use crate::analyzer::decoder::DamageStatWeapon;
use crate::analyzer::decoder::DeathCause;
use crate::analyzer::decoder::FinishType;
use crate::analyzer::decoder::PlayerStateData;
use crate::analyzer::decoder::Recognized;
use crate::analyzer::decoder::WeaponType;
use crate::game_constants::GameConstants;
use crate::nested_property_path::PropertyNestLevel;
use crate::nested_property_path::UpdateAction;
use crate::packet2::EntityCreatePacket;
use crate::packet2::Packet;
use crate::types::AccountId;
use crate::types::ElapsedClock;
use crate::types::EntityId;
use crate::types::GameClock;
use crate::types::GameParamId;
use crate::types::PlaneId;
use crate::types::Relation;
use crate::types::WorldPos;

use super::listener::BattleControllerState;
use super::state::ActiveConsumable;
use super::state::ActivePlane;
use super::state::ActiveShot;
use super::state::ActiveTorpedo;
use super::state::ActiveWard;
use super::state::BuffZoneState;
use super::state::BuildingEntity;
use super::state::CapturePointState;
use super::state::CapturedBuff;
use super::state::ControlPointType;
use super::state::DeadShip;
use super::state::InteractiveZoneType;
use super::state::KillRecord;
use super::state::LocalWeatherZone;
use super::state::MinimapPosition;
use super::state::ResolvedShotHit;
use super::state::ScoringRules;
use super::state::ShipPosition;
use super::state::SmokeScreenEntity;
use super::state::TeamScore;

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
    fn from_arena_player<G: ResourceLoader>(
        player: &PlayerStateData,
        metadata_player: &MetadataPlayer,
        resources: &G,
    ) -> Player {
        Player {
            initial_state: player.clone(),
            end_state: crate::RwCell::new(player.clone()),
            vehicle_entity: None,
            connection_change_info: crate::RwCell::new(Vec::new()),
            vehicle: resources.game_param_by_id(metadata_player.vehicle().id()).expect("could not find player vehicle"),
            relation: metadata_player.relation(),
        }
    }

    /// Create a Player from a mid-battle spawn (e.g. Operations reinforcement wave).
    /// Uses the ship_params_id from the PlayerStateData directly since these players
    /// are not in the replay's JSON metadata.
    fn from_spawned_player<G: ResourceLoader>(player: &PlayerStateData, resources: &G) -> Option<Player> {
        let ship_params_id = player.ship_params_id()?;
        let vehicle = resources.game_param_by_id(ship_params_id)?;
        let relation = Relation::new(player.team_id() as u32);
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

    fn connection_change_info_mut(&self) -> crate::RwCellWriteGuard<'_, Vec<ConnectionChangeInfo>> {
        self.connection_change_info.write_ref()
    }

    pub fn end_state(&self) -> crate::RwCellReadGuard<'_, PlayerStateData> {
        self.end_state.read_ref()
    }

    fn end_state_mut(&self) -> crate::RwCellWriteGuard<'_, PlayerStateData> {
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
    id: AccountId,
    name: String,
    relation: Relation,
    vehicle: Rc<Param>,
}

impl MetadataPlayer {
    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    pub fn relation(&self) -> Relation {
        self.relation
    }

    pub fn vehicle(&self) -> &Param {
        self.vehicle.as_ref()
    }

    pub fn id(&self) -> AccountId {
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

#[derive(Copy, Clone, Serialize)]
#[serde(tag = "type", content = "team_id")]
pub enum BattleResult {
    /// A win, and which team won (inferred to be the team of the player)
    Win(i8),
    /// A loss, and which other team won
    Loss(i8),
    Draw,
}

#[derive(Serialize)]
pub struct BattleReport {
    arena_id: i64,
    self_player: Rc<Player>,
    version: Version,
    map_name: String,
    game_mode: String,
    game_type: Recognized<BattleType>,
    match_group: String,
    players: Vec<Rc<Player>>,
    game_chat: Vec<GameMessage>,
    battle_results: Option<String>,
    frags: HashMap<Rc<Player>, Vec<DeathInfo>>,
    match_result: Option<BattleResult>,
    finish_type: Option<Recognized<FinishType>>,
    capture_points: Vec<CapturePointState>,
    buff_zones: HashMap<EntityId, BuffZoneState>,
    captured_buffs: Vec<CapturedBuff>,
    team_scores: Vec<TeamScore>,
    buildings: Vec<BuildingEntity>,
    local_weather_zones: Vec<LocalWeatherZone>,
    battle_start_clock: Option<GameClock>,
    self_damage_stats: Vec<DamageStatEntry>,
}

impl BattleReport {
    pub fn self_player(&self) -> &Rc<Player> {
        &self.self_player
    }

    pub fn game_chat(&self) -> &[GameMessage] {
        self.game_chat.as_ref()
    }

    pub fn match_group(&self) -> &str {
        self.match_group.as_ref()
    }

    pub fn map_name(&self) -> &str {
        self.map_name.as_ref()
    }

    pub fn version(&self) -> Version {
        self.version
    }

    pub fn game_mode(&self) -> &str {
        self.game_mode.as_ref()
    }

    pub fn game_type(&self) -> &Recognized<BattleType> {
        &self.game_type
    }

    pub fn battle_results(&self) -> Option<&str> {
        self.battle_results.as_deref()
    }

    pub fn players(&self) -> &[Rc<Player>] {
        &self.players
    }

    pub fn arena_id(&self) -> i64 {
        self.arena_id
    }

    /// Returns a map of players and their frags.
    pub fn frags(&self) -> &HashMap<Rc<Player>, Vec<DeathInfo>> {
        &self.frags
    }

    /// The result of the battle. This may be `None` if the player left the match before it finished.
    pub fn battle_result(&self) -> Option<&BattleResult> {
        self.match_result.as_ref()
    }

    pub fn capture_points(&self) -> &[CapturePointState] {
        &self.capture_points
    }

    pub fn buff_zones(&self) -> &HashMap<EntityId, BuffZoneState> {
        &self.buff_zones
    }

    pub fn local_weather_zones(&self) -> &[LocalWeatherZone] {
        &self.local_weather_zones
    }

    pub fn captured_buffs(&self) -> &[CapturedBuff] {
        &self.captured_buffs
    }

    pub fn team_scores(&self) -> &[TeamScore] {
        &self.team_scores
    }

    pub fn buildings(&self) -> &[BuildingEntity] {
        &self.buildings
    }

    pub fn finish_type(&self) -> Option<&Recognized<FinishType>> {
        self.finish_type.as_ref()
    }

    /// Server-authoritative per-weapon damage stats for the self player.
    /// Only populated from `receiveDamageStat` on the Avatar entity.
    pub fn self_damage_stats(&self) -> &[DamageStatEntry] {
        &self.self_damage_stats
    }

    /// Convert an absolute game clock to elapsed time since battle start.
    /// If battle start is unknown, treats clock 0.0 as battle start.
    pub fn game_clock_to_elapsed(&self, clock: GameClock) -> ElapsedClock {
        let start = self.battle_start_clock.unwrap_or(GameClock(0.0));
        clock.to_elapsed(start)
    }

    /// Convert elapsed time since battle start back to an absolute game clock value.
    /// If battle start is unknown, treats clock 0.0 as battle start.
    pub fn elapsed_to_game_clock(&self, elapsed: ElapsedClock) -> GameClock {
        let start = self.battle_start_clock.unwrap_or(GameClock(0.0));
        elapsed.to_absolute(start)
    }
}

#[allow(dead_code)]
struct DamageEvent {
    amount: f32,
    victim: EntityId,
    clock: GameClock,
}

pub struct BattleController<'res, 'replay, G> {
    game_meta: &'replay ReplayMeta,
    game_resources: &'res G,
    metadata_players: Vec<SharedPlayer>,
    player_entities: HashMap<EntityId, Rc<Player>>,
    entities_by_id: HashMap<EntityId, Entity>,
    damage_dealt: HashMap<EntityId, Vec<DamageEvent>>,
    frags: HashMap<EntityId, Vec<Death>>,
    game_chat: Vec<GameMessage>,
    version: Version,
    battle_results: Option<String>,
    match_finished: bool,
    battle_end_clock: Option<GameClock>,
    winning_team: Option<i8>,
    finish_type: Option<Recognized<FinishType>>,
    arena_id: i64,
    current_clock: GameClock,

    // World state
    ship_positions: HashMap<EntityId, ShipPosition>,
    minimap_positions: HashMap<EntityId, MinimapPosition>,
    capture_points: Vec<CapturePointState>,
    /// InteractiveZone data indexed by control point index.
    /// Populated from EntityCreate, consumed when PropertyUpdates fill capture_points.
    /// Maps InteractiveZone entity_id -> index in capture_points vec
    interactive_zone_indices: HashMap<EntityId, usize>,
    /// Buff zones (arms race powerup drops). Maps entity_id -> BuffZoneState.
    buff_zones: HashMap<EntityId, BuffZoneState>,
    /// Maps zone entity_id -> Drop GameParam ID (from BattleLogic drop.data)
    zone_drop_params: HashMap<EntityId, GameParamId>,
    /// Buffs captured by teams during the match.
    captured_buffs: Vec<CapturedBuff>,
    team_scores: Vec<TeamScore>,
    active_consumables: HashMap<EntityId, Vec<ActiveConsumable>>,
    active_shots: Vec<ActiveShot>,
    active_torpedoes: Vec<ActiveTorpedo>,
    /// Resolved projectile hits matched to their originating salvos.
    shot_hits: Vec<ResolvedShotHit>,
    /// When false, artillery shot tracking (active_shots, shot_hits) is skipped.
    /// Used during frame-building passes that don't need shot data.
    track_shots: bool,
    active_planes: HashMap<PlaneId, ActivePlane>,
    active_wards: HashMap<PlaneId, ActiveWard>,
    kills: Vec<KillRecord>,
    dead_ships: HashMap<EntityId, DeadShip>,
    /// Main battery turret yaws per entity (group 0 only).
    /// Maps entity_id -> vec of turret yaws in radians (relative to ship heading).
    turret_yaws: HashMap<EntityId, Vec<f32>>,
    /// Currently selected ammo per entity. Maps entity_id -> ammo_param_id.
    selected_ammo: HashMap<EntityId, GameParamId>,

    /// World-space gun aim yaw per entity, decoded from `targetLocalPos` EntityProperty.
    /// The value is a packed u16: lo byte = yaw, hi byte = pitch.
    /// Yaw decoding: `(lo_byte / 256) * 2*PI - PI` gives world-space radians.
    target_yaws: HashMap<EntityId, f32>,

    /// Local weather zones (squalls/storms) from BattleLogic state.weather.localWeather.
    local_weather_zones: Vec<LocalWeatherZone>,

    /// Scoring rules parsed from BattleLogic EntityCreate (teamWinScore, hold reward/period).
    scoring_rules: Option<ScoringRules>,

    /// Seconds remaining in the match, updated from BattleLogic `timeLeft` EntityProperty.
    time_left: Option<i64>,

    /// Current battle stage raw value: 1 = pre-battle countdown, 0 = battle active, 3 = results.
    /// Note: wowsunpack maps 0=Waiting, 1=Battle — names don't match game semantics.
    battle_stage: Option<i64>,

    /// Clock time when battleStage first transitioned to 0 (battle active / BattleStage::Waiting).
    /// Allows backends to compute elapsed battle time from any absolute GameClock.
    battle_start_clock: Option<GameClock>,

    /// Optional game constants loaded from game data for resolving
    /// death causes, camera modes, entity types, etc.
    game_constants: Option<&'res GameConstants>,

    /// Server-authoritative cumulative damage stats for the self player,
    /// from `receiveDamageStat` on the Avatar entity.
    /// Only `DamageStatCategory::Enemy` entries represent actual damage dealt.
    self_damage_stats: HashMap<(Recognized<DamageStatWeapon>, Recognized<DamageStatCategory>), DamageStatEntry>,

    /// Ribbon counts for the self player, from `onRibbon` Avatar RPC.
    /// All ribbon packets in a replay are for the recording player.
    self_ribbons: HashMap<Ribbon, usize>,
}

impl<'res, 'replay, G> BattleController<'res, 'replay, G>
where
    G: ResourceLoader,
{
    pub fn new(
        game_meta: &'replay ReplayMeta,
        game_resources: &'res G,
        game_constants: Option<&'res GameConstants>,
    ) -> Self {
        let players: Vec<SharedPlayer> = game_meta
            .vehicles
            .iter()
            .map(|vehicle| {
                Rc::new(MetadataPlayer {
                    id: vehicle.id,
                    name: vehicle.name.clone(),
                    relation: Relation::new(vehicle.relation),
                    vehicle: game_resources.game_param_by_id(vehicle.shipId).expect("could not find vehicle"),
                })
            })
            .collect();

        Self {
            game_meta,
            game_resources,
            metadata_players: players,
            player_entities: HashMap::default(),
            entities_by_id: Default::default(),

            game_chat: Default::default(),
            version: Version::from_client_exe(&game_meta.clientVersionFromExe),
            damage_dealt: Default::default(),
            frags: Default::default(),
            battle_results: Default::default(),
            match_finished: false,
            battle_end_clock: None,
            winning_team: None,
            finish_type: None,
            arena_id: 0,
            current_clock: GameClock::default(),
            ship_positions: HashMap::default(),
            minimap_positions: HashMap::default(),
            capture_points: Vec::new(),
            interactive_zone_indices: HashMap::default(),
            buff_zones: HashMap::default(),
            zone_drop_params: HashMap::default(),
            captured_buffs: Vec::new(),
            team_scores: Vec::new(),
            active_consumables: HashMap::default(),
            active_shots: Vec::new(),
            active_torpedoes: Vec::new(),
            shot_hits: Vec::new(),
            track_shots: true,
            active_planes: HashMap::default(),
            active_wards: HashMap::default(),
            kills: Vec::new(),
            dead_ships: HashMap::default(),
            turret_yaws: HashMap::default(),
            selected_ammo: HashMap::default(),
            target_yaws: HashMap::default(),
            local_weather_zones: Vec::new(),
            scoring_rules: None,
            time_left: None,
            battle_stage: None,
            battle_start_clock: None,
            game_constants,
            self_damage_stats: HashMap::default(),
            self_ribbons: HashMap::default(),
        }
    }

    /// Reset all mutable state for seeking (re-parse from start).
    /// Keeps config: game_meta, game_resources, metadata_players, version.
    pub fn reset(&mut self) {
        self.player_entities.clear();
        self.entities_by_id.clear();
        self.damage_dealt.clear();
        self.frags.clear();
        self.game_chat.clear();
        self.battle_results = None;
        self.match_finished = false;
        self.battle_end_clock = None;
        self.winning_team = None;
        self.finish_type = None;
        self.arena_id = 0;
        self.current_clock = GameClock::default();
        self.ship_positions.clear();
        self.minimap_positions.clear();
        self.capture_points.clear();
        self.interactive_zone_indices.clear();
        self.buff_zones.clear();
        self.zone_drop_params.clear();
        self.captured_buffs.clear();
        self.team_scores.clear();
        self.active_consumables.clear();
        self.active_shots.clear();
        self.active_torpedoes.clear();
        self.shot_hits.clear();
        self.active_planes.clear();
        self.kills.clear();
        self.dead_ships.clear();
        self.turret_yaws.clear();
        self.target_yaws.clear();
        self.local_weather_zones.clear();
        self.scoring_rules = None;
        self.time_left = None;
        self.battle_stage = None;
        self.battle_start_clock = None;
        self.self_damage_stats.clear();
        self.self_ribbons.clear();
        // Note: track_shots is config, not state -- preserved across reset.
    }

    /// Suppress or enable shot tracking. When disabled, `ArtilleryShots`
    /// and `ShotKills` processing is skipped (except torpedo cleanup).
    /// Saves memory and CPU during passes that don't need shot data.
    pub fn set_track_shots(&mut self, enabled: bool) {
        self.track_shots = enabled;
    }

    pub fn players(&self) -> &[SharedPlayer] {
        self.metadata_players.as_ref()
    }

    pub fn game_mode(&self) -> String {
        let id = format!("IDS_SCENARIO_{}", self.game_meta.scenario.to_uppercase());
        self.game_resources.localized_name_from_id(&id).unwrap_or_else(|| self.game_meta.scenario.clone())
    }

    pub fn map_name(&self) -> String {
        let id = format!("IDS_{}", self.game_meta.mapName.to_uppercase());
        self.game_resources.localized_name_from_id(&id).unwrap_or_else(|| self.game_meta.mapName.clone())
    }

    pub fn player_name(&self) -> &str {
        self.game_meta.playerName.as_ref()
    }

    pub fn match_group(&self) -> &str {
        self.game_meta.matchGroup.as_ref()
    }

    pub fn game_version(&self) -> &str {
        self.game_meta.clientVersionFromExe.as_ref()
    }

    pub fn game_type(&self) -> Recognized<BattleType> {
        BattleType::from_value(&self.game_meta.gameType, self.version)
    }

    fn constants(&self) -> &GameConstants {
        self.game_constants.unwrap_or(&crate::game_constants::DEFAULT_GAME_CONSTANTS)
    }

    /// Translate a string if it starts with `IDS_` using the game's localization data.
    /// Returns the original string unchanged if it doesn't start with `IDS_` or if
    /// no translation is found.
    fn translate_ids(&self, text: &str) -> String {
        if text.starts_with("IDS_") {
            self.game_resources.localized_name_from_id(text).unwrap_or_else(|| text.to_string())
        } else {
            text.to_string()
        }
    }

    fn handle_chat_message(
        &mut self,
        entity_id: EntityId,
        sender_id: AccountId,
        audience: &str,
        message: &str,
        _extra_data: Option<ChatMessageExtra>,
        clock: GameClock,
    ) {
        // System messages
        if sender_id.raw() == 0 {
            return;
        }

        let channel = match audience {
            "battle_common" => ChatChannel::Global,
            "battle_team" => ChatChannel::Team,
            "battle_prebattle" => ChatChannel::Division,
            other => ChatChannel::Unknown(other.to_string()),
        };

        let mut sender_team = None;
        let mut sender_name = "Unknown".to_owned();
        let mut player = None;
        for meta_vehicle in &self.game_meta.vehicles {
            if meta_vehicle.id == sender_id {
                sender_name = meta_vehicle.name.clone();
                sender_team = Some(Relation::new(meta_vehicle.relation));
                player = self
                    .player_entities
                    .values()
                    .find(|player| player.initial_state.meta_ship_id() == sender_id)
                    .cloned();
            }
        }

        // Translate bot/NPC names and messages (they use IDS_ localization keys).
        // If we can't resolve the player, default to treating them as a bot.
        let is_bot = player.as_ref().map(|p| p.is_bot()).unwrap_or(true);
        if is_bot {
            sender_name = self.translate_ids(&sender_name);
        }
        let message_text = if is_bot { self.translate_ids(message) } else { message.to_string() };
        debug!("chat message from sender {sender_name} in channel {channel:?}: {message_text}");

        let message = GameMessage {
            clock,
            sender_relation: sender_team,
            sender_name,
            channel,
            message: message_text,
            entity_id,
            player,
        };

        self.game_chat.push(message.clone());
        debug!("{:p} game chat len: {}", &self.game_chat, self.game_chat.len());
    }

    fn handle_entity_create<'packet>(&mut self, clock: GameClock, packet: &EntityCreatePacket<'packet>) {
        self.handle_entity_create_with_clock(clock, packet);
    }

    pub fn game_chat(&self) -> &[GameMessage] {
        self.game_chat.as_slice()
    }

    pub fn build_report(mut self) -> BattleReport {
        // Update vehicle damage from damage events (receiveDamagesOnShip).
        // For non-self players, this is the only damage source available.
        for (aggressor, damage_events) in &self.damage_dealt {
            if let Some(aggressor_entity) = self.entities_by_id.get_mut(aggressor) {
                let Some(vehicle) = aggressor_entity.vehicle_ref() else {
                    warn!("aggressor is not a vehicle: {:?}", aggressor_entity.kind());
                    continue;
                };

                let mut vehicle = vehicle.borrow_mut();
                vehicle.damage += damage_events.iter().fold(0.0f64, |accum, event| {
                    accum + event.amount as f64
                });
            }
        }

        // Override the self player's damage with the server-authoritative total
        // from receiveDamageStat (Avatar method). DamageReceived events only cover
        // visible targets, missing DoT on ships that left the client's AoI.
        // Only Enemy category entries represent actual damage dealt.
        if !self.self_damage_stats.is_empty() {
            let authoritative_damage: f64 = self
                .self_damage_stats
                .values()
                .filter(|entry| entry.category == Recognized::Known(DamageStatCategory::Enemy))
                .map(|entry| entry.total)
                .sum();

            if let Some(self_entity_id) = self
                .player_entities
                .iter()
                .find(|(_, p)| p.relation().is_self())
                .map(|(eid, _)| *eid)
                && let Some(entity) = self.entities_by_id.get_mut(&self_entity_id)
                    && let Some(vehicle) = entity.vehicle_ref() {
                        vehicle.borrow_mut().damage = authoritative_damage;
                    }
        }

        // Update vehicle death info
        self.entities_by_id.values().for_each(|entity| {
            if let Some(vehicle) = entity.vehicle_ref() {
                let mut vehicle = vehicle.borrow_mut();

                if let Some(death) =
                    self.frags.values().find_map(|deaths| deaths.iter().find(|death| death.victim == vehicle.id))
                {
                    vehicle.death_info = Some(death.into());
                }
            }
        });

        let parsed_battle_results =
            self.battle_results.as_ref().and_then(|results| serde_json::Value::from_str(results.as_str()).ok());

        // Build final Player objects with owned VehicleEntity.
        // Players without a matching entity (e.g. disconnected, bots without EntityCreate)
        // are still included with vehicle_entity = None.
        let players: Vec<Rc<Player>> = self
            .player_entities
            .values()
            .map(|player| {
                let vehicle = self
                    .entities_by_id
                    .get(&player.initial_state.entity_id())
                    .and_then(|entity| entity.vehicle_ref())
                    .map(|vehicle_rc| {
                        let mut vehicle: VehicleEntity = RefCell::borrow(vehicle_rc).clone();

                        // Add battle results info to vehicle
                        if let Some(battle_results) =
                            parsed_battle_results.as_ref().and_then(|results| results.as_object())
                        {
                            vehicle.results_info = battle_results.get("playersPublicInfo").and_then(|infos| {
                                infos.as_object().and_then(|infos| {
                                    infos.get(player.initial_state.db_id.to_string().as_str()).cloned()
                                })
                            });

                            if let Some(frags) = self.frags.get(&player.initial_state.entity_id()) {
                                vehicle.frags = frags.iter().map(DeathInfo::from).collect();
                            }
                        }

                        vehicle
                    });

                let mut final_player = player.as_ref().clone();
                final_player.vehicle_entity = vehicle;
                Rc::new(final_player)
            })
            .collect();

        let frags: HashMap<Rc<Player>, Vec<DeathInfo>> =
            HashMap::from_iter(self.frags.drain().filter_map(|(entity_id, kills)| {
                let player = players.iter().find(|p| p.initial_state.entity_id() == entity_id)?;
                let kills: Vec<DeathInfo> = kills.iter().map(DeathInfo::from).collect();
                Some((Rc::clone(player), kills))
            }));

        let self_player =
            players.iter().find(|player| player.relation.is_self()).cloned().expect("could not find self_player");

        // Collect building entities
        let buildings: Vec<BuildingEntity> =
            self.entities_by_id.values().filter_map(|e| e.building_ref().map(|b| RefCell::borrow(b).clone())).collect();

        BattleReport {
            arena_id: self.arena_id,
            match_result: if self.match_finished {
                self.winning_team.map(|team| {
                    if team == self_player.initial_state.team_id as i8 {
                        BattleResult::Win(team)
                    } else if team >= 0 {
                        BattleResult::Loss(1)
                    } else {
                        BattleResult::Draw
                    }
                })
            } else {
                None
            },
            self_player,
            version: Version::from_client_exe(self.game_version()),
            match_group: self.match_group().to_owned(),
            map_name: self.map_name(),
            game_mode: self.game_mode(),
            game_type: self.game_type(),
            players,
            game_chat: self.game_chat,
            battle_results: self.battle_results,
            finish_type: self.finish_type,
            frags,
            capture_points: self.capture_points,
            buff_zones: self.buff_zones,
            captured_buffs: self.captured_buffs,
            team_scores: self.team_scores,
            buildings,
            local_weather_zones: self.local_weather_zones,
            battle_start_clock: self.battle_start_clock,
            self_damage_stats: self.self_damage_stats.into_values().collect(),
        }
    }

    pub fn battle_results(&self) -> Option<&String> {
        self.battle_results.as_ref()
    }

    fn handle_property_update(&mut self, _clock: GameClock, update: &crate::packet2::PropertyUpdatePacket<'_>) {
        if update.property != "state" {
            return;
        }

        let levels = &update.update_cmd.levels;
        let action = &update.update_cmd.action;

        // Match: state -> missions -> teamsScore -> [N] -> SetKey{score}
        if levels.len() == 3
            && let PropertyNestLevel::DictKey("missions") = &levels[0]
            && let PropertyNestLevel::DictKey("teamsScore") = &levels[1]
            && let PropertyNestLevel::ArrayIndex(team_idx) = &levels[2]
            && let UpdateAction::SetKey { key: "score", value } = action
            && let Some(score) = value.as_i64()
        {
            while self.team_scores.len() <= *team_idx {
                self.team_scores.push(TeamScore { team_index: self.team_scores.len(), ..Default::default() });
            }
            self.team_scores[*team_idx].score = score;
        }

        // Match: state -> drop -> data -> SetRange{values: [{zoneId, paramsId, ...}]}
        if levels.len() == 2
            && let PropertyNestLevel::DictKey("drop") = &levels[0]
            && let PropertyNestLevel::DictKey("data") = &levels[1]
            && let UpdateAction::SetRange { values, .. } = action
        {
            for value in values {
                if let ArgValue::FixedDict(dict) = value {
                    let zone_id = dict.get("zoneId").and_then(|v| v.as_i64()).map(|v| EntityId::from(v as i32));
                    let params_id = dict.get("paramsId").and_then(|v| v.as_i64()).map(GameParamId::from);

                    if let (Some(zone_id), Some(params_id)) = (zone_id, params_id) {
                        self.zone_drop_params.insert(zone_id, params_id);
                        if let Some(bz) = self.buff_zones.get_mut(&zone_id) {
                            bz.drop_params_id = Some(params_id);
                        }
                    }
                }
            }
        }

        // Match: state -> drop -> picked -> SetRange{values: [{paramsId, owners: [entity_ids]}]}
        if levels.len() == 2
            && let PropertyNestLevel::DictKey("drop") = &levels[0]
            && let PropertyNestLevel::DictKey("picked") = &levels[1]
            && let UpdateAction::SetRange { values, .. } = action
        {
            for value in values {
                if let ArgValue::FixedDict(dict) = value {
                    let params_id = dict.get("paramsId").and_then(|v| v.as_i64()).map(GameParamId::from);
                    let owners: Option<Vec<EntityId>> = dict.get("owners").and_then(|v| {
                        if let ArgValue::Array(arr) = v {
                            Some(
                                arr.iter()
                                    .filter_map(|o| o.as_i64().map(|id| EntityId::from(id as i32)))
                                    .collect::<Vec<_>>(),
                            )
                        } else {
                            None
                        }
                    });

                    if let (Some(params_id), Some(owners)) = (params_id, owners) {
                        // Determine team from first owner entity
                        let team_id = owners.first().and_then(|owner_id| {
                            self.player_entities
                                .values()
                                .find(|p| p.initial_state.entity_id() == *owner_id)
                                .map(|p| p.initial_state.team_id)
                        });

                        if let Some(team_id) = team_id {
                            self.captured_buffs.push(CapturedBuff { params_id, team_id, clock: _clock });
                        }
                    }
                }
            }
        }

        // Match: state -> weather -> localWeather -> SetRange{values: [{position, radius, name, paramsId}]}
        if levels.len() == 2
            && let PropertyNestLevel::DictKey("weather") = &levels[0]
            && let PropertyNestLevel::DictKey("localWeather") = &levels[1]
        {
            match action {
                UpdateAction::SetRange { start, stop: _, values } => {
                    // Grow vec if needed
                    let needed = start + values.len();
                    while self.local_weather_zones.len() < needed {
                        self.local_weather_zones.push(LocalWeatherZone {
                            name: String::new(),
                            position: WorldPos::default(),
                            radius: 0.0,
                            params_id: GameParamId::default(),
                            entity_id: None,
                        });
                    }
                    for (i, value) in values.iter().enumerate() {
                        if let Some(mut zone) = Self::parse_weather_zone(value) {
                            // Preserve entity_id if already linked
                            if let Some(existing) = self.local_weather_zones.get(start + i) {
                                zone.entity_id = existing.entity_id;
                            }
                            self.local_weather_zones[start + i] = zone;
                        }
                    }
                }
                UpdateAction::SetElement { index, value } => {
                    while self.local_weather_zones.len() <= *index {
                        self.local_weather_zones.push(LocalWeatherZone {
                            name: String::new(),
                            position: WorldPos::default(),
                            radius: 0.0,
                            params_id: GameParamId::default(),
                            entity_id: None,
                        });
                    }
                    if let Some(mut zone) = Self::parse_weather_zone(value) {
                        // Preserve entity_id if already linked
                        if let Some(existing) = self.local_weather_zones.get(*index) {
                            zone.entity_id = existing.entity_id;
                        }
                        self.local_weather_zones[*index] = zone;
                    }
                }
                UpdateAction::RemoveRange { start, stop } => {
                    let end = (*stop).min(self.local_weather_zones.len());
                    if *start < end {
                        self.local_weather_zones.drain(*start..end);
                    }
                }
                _ => {}
            }
        }

        // Match: state -> weather -> localWeather -> ArrayIndex(N) -> SetKey{position|radius|name|paramsId}
        // This handles individual field updates when a weather zone moves or changes size.
        if levels.len() == 3
            && let PropertyNestLevel::DictKey("weather") = &levels[0]
            && let PropertyNestLevel::DictKey("localWeather") = &levels[1]
            && let PropertyNestLevel::ArrayIndex(idx) = &levels[2]
            && let UpdateAction::SetKey { key, value } = action
            && *idx < self.local_weather_zones.len()
        {
            let zone = &mut self.local_weather_zones[*idx];
            match *key {
                "position" => {
                    if let Some(pos) = Self::extract_weather_position(value) {
                        zone.position = pos;
                    }
                }
                "radius" => {
                    if let Some(r) = value.float_32_ref().copied() {
                        zone.radius = r;
                    }
                }
                "name" => {
                    zone.name = match value {
                        ArgValue::Array(arr) => {
                            let bytes: Vec<u8> = arr.iter().filter_map(|v| v.as_i64().map(|i| i as u8)).collect();
                            String::from_utf8(bytes).unwrap_or_default()
                        }
                        ArgValue::String(s) => String::from_utf8_lossy(s).into_owned(),
                        _ => String::new(),
                    };
                }
                "paramsId" => {
                    if let Some(id) = value.as_i64() {
                        zone.params_id = GameParamId::from(id);
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_entity_create_with_clock(&mut self, _clock: GameClock, packet: &EntityCreatePacket<'_>) {
        let entity_type = EntityType::from_str(packet.entity_type).unwrap_or_else(|_| {
            panic!("failed to convert entity type {} to a string", packet.entity_type);
        });

        match entity_type {
            EntityType::Vehicle => {
                let mut props = VehicleProps::default();
                props.update_from_args(&packet.props, self.version, self.constants());

                let captain_id = props.crew_modifiers_compact_params.params_id;
                let captain = if captain_id.raw() != 0 {
                    Some(self.game_resources.game_param_by_id(captain_id).expect("failed to get captain"))
                } else {
                    None
                };

                let vehicle = Rc::new(RefCell::new(VehicleEntity {
                    id: packet.entity_id,
                    props,
                    visibility_changed_at: 0.0,
                    captain,
                    damage: 0.0,
                    death_info: None,
                    results_info: None,
                    frags: Vec::default(),
                }));

                self.entities_by_id.insert(packet.entity_id, Entity::Vehicle(vehicle.clone()));
            }
            EntityType::Building => {
                let mut is_alive = true;
                let mut is_hidden = false;
                let mut is_suppressed = false;
                let mut team_id: i8 = 0;
                let mut params_id: u32 = 0;

                if let Some(v) = packet.props.get("isAlive") {
                    is_alive = v.uint_8_ref().map(|v| *v != 0).unwrap_or(true);
                }
                if let Some(v) = packet.props.get("isHidden") {
                    is_hidden = v.uint_8_ref().map(|v| *v != 0).unwrap_or(false);
                }
                if let Some(v) = packet.props.get("isSuppressed") {
                    is_suppressed = v.uint_8_ref().map(|v| *v != 0).unwrap_or(false);
                }
                if let Some(v) = packet.props.get("teamId") {
                    team_id = v.int_8_ref().copied().unwrap_or(0);
                }
                if let Some(v) = packet.props.get("paramsId") {
                    params_id = v.uint_32_ref().copied().unwrap_or(0);
                }

                let building = BuildingEntity {
                    id: packet.entity_id,
                    position: WorldPos { x: packet.position.x, y: packet.position.y, z: packet.position.z },
                    is_alive,
                    is_hidden,
                    is_suppressed,
                    team_id,
                    params_id: GameParamId::from(params_id),
                };

                self.entities_by_id.insert(packet.entity_id, Entity::Building(Rc::new(RefCell::new(building.clone()))));
            }
            EntityType::SmokeScreen => {
                let radius = BigWorldDistance::from(
                    packet.props.get("radius").and_then(|v| v.float_32_ref().copied()).unwrap_or(0.0),
                );

                let position = WorldPos { x: packet.position.x, y: packet.position.y, z: packet.position.z };

                let smoke = SmokeScreenEntity { id: packet.entity_id, radius, position, points: vec![position] };

                self.entities_by_id.insert(packet.entity_id, Entity::SmokeScreen(Rc::new(RefCell::new(smoke))));
            }
            EntityType::BattleLogic => {
                debug!("BattleLogic create");
                if let Some(state) = packet.props.get("state")
                    && let Some(state_dict) = Self::as_dict(state)
                    && let Some(missions) = state_dict.get("missions")
                    && let Some(missions_dict) = Self::as_dict(missions)
                {
                    // Extract initial team scores from state.missions.teamsScore
                    if let Some(ArgValue::Array(teams)) = missions_dict.get("teamsScore") {
                        for (idx, entry) in teams.iter().enumerate() {
                            if let Some(entry_dict) = Self::as_dict(entry) {
                                let score = entry_dict.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
                                while self.team_scores.len() <= idx {
                                    self.team_scores
                                        .push(TeamScore { team_index: self.team_scores.len(), ..Default::default() });
                                }
                                self.team_scores[idx].score = score;
                            }
                        }
                    }

                    // Extract scoring rules: teamWinScore, hold reward/period/cpIndices
                    let team_win_score = missions_dict.get("teamWinScore").and_then(|v| v.as_i64()).unwrap_or(1000);

                    let mut hold_reward: i64 = 3;
                    let mut hold_period: f32 = 5.0;
                    let mut hold_cp_indices: Vec<usize> = Vec::new();

                    if let Some(ArgValue::Array(holds)) = missions_dict.get("hold")
                        && let Some(first_hold) = holds.first()
                        && let Some(hold_dict) = Self::as_dict(first_hold)
                    {
                        if let Some(v) = hold_dict.get("reward").and_then(|v| v.as_i64()) {
                            hold_reward = v;
                        }
                        if let Some(v) = hold_dict.get("period") {
                            hold_period = v.float_32_ref().copied().unwrap_or(5.0);
                        }
                        if let Some(ArgValue::Array(indices)) = hold_dict.get("cpIndices") {
                            for idx in indices {
                                if let Some(i) = idx.as_i64() {
                                    hold_cp_indices.push(i as usize);
                                }
                            }
                        }
                    }

                    self.scoring_rules =
                        Some(ScoringRules { team_win_score, hold_reward, hold_period, hold_cp_indices });
                }

                // Extract initial local weather zones from state.weather.localWeather
                if let Some(state) = packet.props.get("state")
                    && let Some(state_dict) = Self::as_dict(state)
                    && let Some(weather) = state_dict.get("weather")
                    && let Some(weather_dict) = Self::as_dict(weather)
                    && let Some(ArgValue::Array(local_weather)) = weather_dict.get("localWeather")
                {
                    for entry in local_weather {
                        if let Some(zone) = Self::parse_weather_zone(entry) {
                            self.local_weather_zones.push(zone);
                        }
                    }
                }
            }
            EntityType::InteractiveZone => {
                let position = WorldPos { x: packet.position.x, y: packet.position.y, z: packet.position.z };
                let radius = packet.props.get("radius").and_then(|v| v.float_32_ref().copied()).unwrap_or(0.0);
                let team_id = packet.props.get("teamId").and_then(|v| v.as_i64()).unwrap_or(-1);

                let zone_type =
                    packet.props.get("type").and_then(|v| v.as_i64()).and_then(|id| {
                        InteractiveZoneType::from_id(id as i32, self.constants().battle(), self.version)
                    });
                let is_weather =
                    zone_type.as_ref().and_then(|r| r.known().copied()) == Some(InteractiveZoneType::WeatherZone);

                // Weather zone (squall/storm): link to existing localWeather entry or create one
                if is_weather {
                    let name = match packet.props.get("name") {
                        Some(ArgValue::Array(arr)) => {
                            let bytes: Vec<u8> = arr.iter().filter_map(|v| v.as_i64().map(|i| i as u8)).collect();
                            String::from_utf8(bytes).unwrap_or_default()
                        }
                        Some(ArgValue::String(s)) => String::from_utf8_lossy(s).into_owned(),
                        _ => String::new(),
                    };

                    // Try to match to an existing localWeather zone by name
                    let mut matched = false;
                    for zone in &mut self.local_weather_zones {
                        if zone.name == name && zone.entity_id.is_none() {
                            zone.entity_id = Some(packet.entity_id);
                            zone.position = position;
                            zone.radius = radius;
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        // No BattleLogic localWeather entry yet — create one
                        self.local_weather_zones.push(LocalWeatherZone {
                            name,
                            position,
                            radius,
                            params_id: GameParamId::default(),
                            entity_id: Some(packet.entity_id),
                        });
                    }
                } else {
                    // Extract index, type, and initial capture state from componentsState
                    // Note: inner dicts are NullableFixedDict, not FixedDict
                    let mut cp_index: Option<usize> = None;
                    let mut cp_type: Option<Recognized<ControlPointType>> = None;
                    let mut has_invaders = false;
                    let mut invader_team: i64 = -1;
                    let mut progress: f64 = 0.0;
                    let mut both_inside = false;

                    if let Some(cs) = packet.props.get("componentsState")
                        && let Some(cs_dict) = Self::as_dict(cs)
                    {
                        // Extract control point index and type
                        if let Some(cp) = cs_dict.get("controlPoint")
                            && let Some(cp_dict) = Self::as_dict(cp)
                        {
                            if let Some(idx) = cp_dict.get("index") {
                                cp_index = idx.as_i64().map(|v| v as usize);
                            }
                            if let Some(t) = cp_dict.get("type") {
                                cp_type = t.as_i64().and_then(|id| {
                                    ControlPointType::from_id(id as i32, self.constants().battle(), self.version)
                                });
                            }
                        }
                        // Extract initial capture logic state
                        if let Some(cl) = cs_dict.get("captureLogic")
                            && let Some(cl_dict) = Self::as_dict(cl)
                        {
                            if let Some(v) = cl_dict.get("hasInvaders") {
                                has_invaders = v.as_i64().unwrap_or(0) != 0;
                            }
                            if let Some(v) = cl_dict.get("invaderTeam") {
                                invader_team = v.as_i64().unwrap_or(-1);
                            }
                            if let Some(v) = cl_dict.get("progress") {
                                progress = v.float_32_ref().map(|f| *f as f64).unwrap_or(0.0);
                            }
                            if let Some(v) = cl_dict.get("bothInside") {
                                both_inside = v.as_i64().unwrap_or(0) != 0;
                            }
                        }
                    }

                    // Extract isEnabled from captureLogic
                    let mut is_enabled = true;
                    if let Some(cs) = packet.props.get("componentsState")
                        && let Some(cs_dict) = Self::as_dict(cs)
                        && let Some(cl) = cs_dict.get("captureLogic")
                        && let Some(cl_dict) = Self::as_dict(cl)
                        && let Some(v) = cl_dict.get("isEnabled")
                    {
                        is_enabled = v.as_i64().unwrap_or(1) != 0;
                    }

                    if let Some(idx) = cp_index {
                        // Capture point: has controlPoint with valid index
                        while self.capture_points.len() <= idx {
                            self.capture_points.push(CapturePointState::default());
                        }
                        self.capture_points[idx] = CapturePointState {
                            index: idx,
                            position: Some(position),
                            radius,
                            control_point_type: cp_type,
                            team_id,
                            invader_team,
                            progress: (progress, 0.0),
                            has_invaders,
                            both_inside,
                            is_enabled,
                        };
                        self.interactive_zone_indices.insert(packet.entity_id, idx);
                    } else {
                        // Buff zone: controlPoint is null, this is an arms race powerup drop
                        // Check if we already have a drop params mapping for this zone
                        let drop_params_id = self.zone_drop_params.get(&packet.entity_id).copied();
                        self.buff_zones.insert(
                            packet.entity_id,
                            BuffZoneState {
                                entity_id: packet.entity_id,
                                position,
                                radius,
                                team_id,
                                is_active: is_enabled,
                                drop_params_id,
                            },
                        );
                        // Also register for PropertyUpdate tracking
                        self.interactive_zone_indices.insert(packet.entity_id, usize::MAX);
                    }
                } // end else (non-weather InteractiveZone)
            }
            EntityType::BattleEntity => debug!("BattleEntity create"),
        }
    }

    /// Extract a 2D world position from a weather zone value (Vector2 or 2-element Array).
    fn extract_weather_position(value: &ArgValue<'_>) -> Option<WorldPos> {
        match value {
            ArgValue::Vector2((x, z)) => Some(WorldPos { x: *x, y: 0.0, z: *z }),
            ArgValue::Array(arr) if arr.len() >= 2 => {
                let x = arr[0].float_32_ref().copied().unwrap_or(0.0);
                let z = arr[1].float_32_ref().copied().unwrap_or(0.0);
                Some(WorldPos { x, y: 0.0, z })
            }
            _ => None,
        }
    }

    /// Parse a local weather zone entry from a FixedDict ArgValue.
    ///
    /// Expected fields: `name` (bytes), `position` (Vector2 [x,z]), `radius` (f32),
    /// `paramsId` (integer).
    fn parse_weather_zone(value: &ArgValue<'_>) -> Option<LocalWeatherZone> {
        let dict = Self::as_dict(value)?;

        let name = match dict.get("name") {
            Some(ArgValue::Array(arr)) => {
                let bytes: Vec<u8> = arr.iter().filter_map(|v| v.as_i64().map(|i| i as u8)).collect();
                String::from_utf8(bytes).unwrap_or_default()
            }
            Some(ArgValue::String(s)) => String::from_utf8_lossy(s).into_owned(),
            _ => String::new(),
        };

        let position = match dict.get("position") {
            Some(v) => Self::extract_weather_position(v)?,
            _ => return None,
        };

        let radius = dict.get("radius").and_then(|v| v.float_32_ref().copied()).unwrap_or(0.0);

        let params_id = dict.get("paramsId").and_then(|v| v.as_i64()).map(GameParamId::from).unwrap_or_default();

        Some(LocalWeatherZone { name, position, radius, params_id, entity_id: None })
    }

    /// Extract a dict reference from either FixedDict or NullableFixedDict(Some(...)).
    fn as_dict<'a, 'b>(value: &'a ArgValue<'b>) -> Option<&'a HashMap<&'b str, ArgValue<'b>>> {
        match value {
            ArgValue::FixedDict(d) => Some(d),
            ArgValue::NullableFixedDict(Some(d)) => Some(d),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

trait UpdateFromReplayArgs {
    fn update_by_name(&mut self, name: &str, value: &ArgValue<'_>, version: Version, constants: &GameConstants) {
        // This is far from optimal, but is an easy solution for now
        let mut dict = HashMap::with_capacity(1);
        dict.insert(name, value.clone());
        self.update_from_args(&dict, version, constants);
    }

    fn update_from_args(&mut self, args: &HashMap<&str, ArgValue<'_>>, version: Version, constants: &GameConstants);
}

macro_rules! set_arg_value {
    ($set_var:expr, $args:ident, $key:expr, String) => {
        $set_var = (*value.string_ref().unwrap_or_else(|| panic!("{} is not a string", $key))).clone()
    };
    ($set_var:expr, $args:ident, $key:expr, i8) => {
        set_arg_value!($set_var, $args, $key, int_8_ref, i8)
    };
    ($set_var:expr, $args:ident, $key:expr, i16) => {
        set_arg_value!($set_var, $args, $key, int_16_ref, i16)
    };
    ($set_var:expr, $args:ident, $key:expr, i32) => {
        set_arg_value!($set_var, $args, $key, int_32_ref, i32)
    };
    ($set_var:expr, $args:ident, $key:expr, u8) => {
        set_arg_value!($set_var, $args, $key, uint_8_ref, u8)
    };
    ($set_var:expr, $args:ident, $key:expr, u16) => {
        set_arg_value!($set_var, $args, $key, uint_16_ref, u16)
    };
    ($set_var:expr, $args:ident, $key:expr, u32) => {
        set_arg_value!($set_var, $args, $key, uint_32_ref, u32)
    };
    ($set_var:expr, $args:ident, $key:expr, f32) => {
        set_arg_value!($set_var, $args, $key, float_32_ref, f32)
    };
    ($set_var:expr, $args:ident, $key:expr, bool) => {
        if let Some(value) = $args.get($key) {
            $set_var = (*value.uint_8_ref().unwrap_or_else(|| panic!("{} is not a u8", $key))) != 0
        }
    };
    ($set_var:expr, $args:ident, $key:expr, Vec<u8>) => {
        if let Some(value) = $args.get($key) {
            $set_var = value.blob_ref().unwrap_or_else(|| panic!("{} is not a u8", $key)).clone()
        }
    };
    ($set_var:expr, $args:ident, $key:expr, &[()]) => {
        set_arg_value!($set_var, $args, $key, array_ref, &[()])
    };
    ($set_var:expr, $args:ident, $key:expr, $conversion_func:ident, $ty:ty) => {
        if let Some(value) = $args.get($key) {
            $set_var =
                value.$conversion_func().unwrap_or_else(|| panic!("{} is not a {}", $key, stringify!($ty))).clone()
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
    fn update_from_args(&mut self, args: &HashMap<&str, ArgValue<'_>>, _version: Version, _constants: &GameConstants) {
        const PARAMS_ID_KEY: &str = "paramsId";
        const IS_IN_ADAPTION_KEY: &str = "isInAdaption";
        const LEARNED_SKILLS_KEY: &str = "learnedSkills";

        if args.contains_key(PARAMS_ID_KEY) {
            self.params_id = GameParamId::from(arg_value_to_type!(args, PARAMS_ID_KEY, u32));
        }
        if args.contains_key(IS_IN_ADAPTION_KEY) {
            self.is_in_adaption = arg_value_to_type!(args, IS_IN_ADAPTION_KEY, bool);
        }

        if args.contains_key(LEARNED_SKILLS_KEY) {
            let learned_skills = arg_value_to_type!(args, LEARNED_SKILLS_KEY, &[()]);
            let skills_from_idx = |idx: usize| -> Vec<u8> {
                learned_skills[idx].array_ref().unwrap().iter().map(|idx| *(*idx).uint_8_ref().unwrap()).collect()
            };

            let skills = Skills {
                aircraft_carrier: skills_from_idx(0),
                battleship: skills_from_idx(1),
                cruiser: skills_from_idx(2),
                destroyer: skills_from_idx(3),
                auxiliary: skills_from_idx(4),
                submarine: skills_from_idx(5),
            };

            self.learned_skills = skills;
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
    visibility_flags: u32,
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
            visibility_flags: 0,
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

    pub fn visibility_flags(&self) -> u32 {
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
}

impl UpdateFromReplayArgs for VehicleProps {
    fn update_from_args(&mut self, args: &HashMap<&str, ArgValue<'_>>, version: Version, constants: &GameConstants) {
        const IGNORE_MAP_BORDERS_KEY: &str = "ignoreMapBorders";
        const AIR_DEFENSE_DISPERSION_RADIUS_KEY: &str = "airDefenseDispRadius";
        const DEATH_SETTINGS_KEY: &str = "deathSettings";
        const OWNER_KEY: &str = "owner";
        const ATBA_TARGETS_KEY: &str = "atbaTargets";
        const EFFECTS_KEY: &str = "effects";
        const CREW_MODIFIERS_COMPACT_PARAMS_KEY: &str = "crewModifiersCompactParams";
        const LASER_TARGET_LOCAL_POS_KEY: &str = "laserTargetLocalPos";

        const SELECTED_WEAPON_KEY: &str = "selectedWeapon";

        const IS_ON_FORSAGE_KEY: &str = "isOnForsage";
        const IS_IN_RAGE_MODE_KEY: &str = "isInRageMode";
        const HAS_AIR_TARGETS_IN_RANGE_KEY: &str = "hasAirTargetsInRange";
        const TORPEDO_LOCAL_POS_KEY: &str = "torpedoLocalPos";

        const BUOYANCY_KEY: &str = "buoyancy";
        const MAX_HEALTH_KEY: &str = "maxHealth";
        const DRAUGHT_KEY: &str = "draught";
        const RUDDERS_ANGLE_KEY: &str = "ruddersAngle";
        const TARGET_LOCAL_POSITION_KEY: &str = "targetLocalPos";
        const TRIGGERED_SKILLS_DATA_KEY: &str = "triggeredSkillsData";
        const REGENERATED_HEALTH_KEY: &str = "regeneratedHealth";
        const BLOCKED_CONTROLS_KEY: &str = "blockedControls";
        const IS_INVISIBLE_KEY: &str = "isInvisible";
        const IS_FOG_HORN_ON_KEY: &str = "isFogHornOn";
        const SERVER_SPEED_RAW_KEY: &str = "serverSpeedRaw";
        const REGEN_CREW_HP_LIMIT_KEY: &str = "regenCrewHpLimit";

        const BUOYANCY_CURRENT_WATERLINE_KEY: &str = "buoyancyCurrentWaterline";
        const IS_ALIVE_KEY: &str = "isAlive";
        const IS_BOT_KEY: &str = "isBot";
        const VISIBILITY_FLAGS_KEY: &str = "visibilityFlags";

        const BUOYANCY_RUDDER_INDEX_KEY: &str = "buoyancyRudderIndex";
        const IS_ANTI_AIR_MODE_KEY: &str = "isAntiAirMode";
        const SPEED_SIGN_DIR_KEY: &str = "speedSignDir";
        const OIL_LEAK_STATE_KEY: &str = "oilLeakState";

        const SHIP_CONFIG_KEY: &str = "shipConfig";
        const WAVE_LOCAL_POS_KEY: &str = "waveLocalPos";
        const HAS_ACTIVE_MAIN_SQUADRON_KEY: &str = "hasActiveMainSquadron";
        const WEAPON_LOCK_FLAGS_KEY: &str = "weaponLockFlags";
        const DEEP_RUDDERS_ANGLE_KEY: &str = "deepRuddersAngle";

        const HEALTH_KEY: &str = "health";
        const ENGINE_DIR_KEY: &str = "engineDir";

        const TEAM_ID_KEY: &str = "teamId";
        const BUOYANCY_CURRENT_STATE_KEY: &str = "buoyancyCurrentState";
        const UI_ENABLED_KEY: &str = "uiEnabled";
        const RESPAWN_TIME_KEY: &str = "respawnTime";
        const ENGINE_POWER_KEY: &str = "enginePower";
        const MAX_SERVER_SPEED_RAW_KEY: &str = "maxServerSpeedRaw";
        const BURNING_FLAGS_KEY: &str = "burningFlags";

        set_arg_value!(self.ignore_map_borders, args, IGNORE_MAP_BORDERS_KEY, bool);
        set_arg_value!(self.air_defense_dispersion_radius, args, AIR_DEFENSE_DISPERSION_RADIUS_KEY, f32);

        set_arg_value!(self.death_settings, args, DEATH_SETTINGS_KEY, Vec<u8>);
        if args.contains_key(OWNER_KEY) {
            let value: u32 = arg_value_to_type!(args, OWNER_KEY, i32) as u32;
            self.owner = value;
        }

        if args.contains_key(ATBA_TARGETS_KEY) {
            let value: Vec<u32> = arg_value_to_type!(args, ATBA_TARGETS_KEY, &[()])
                .iter()
                .map(|elem| *elem.uint_32_ref().expect("atbaTargets elem is not a u32"))
                .collect();
            self.atba_targets = value;
        }

        if args.contains_key(EFFECTS_KEY) {
            let value: Vec<String> = arg_value_to_type!(args, EFFECTS_KEY, &[()])
                .iter()
                .map(|elem| {
                    String::from_utf8(elem.string_ref().expect("effects elem is not a string").clone())
                        .expect("could not convert effects elem to string")
                })
                .collect();
            self.effects = value;
        }

        if args.contains_key(CREW_MODIFIERS_COMPACT_PARAMS_KEY) {
            self.crew_modifiers_compact_params.update_from_args(
                arg_value_to_type!(args, CREW_MODIFIERS_COMPACT_PARAMS_KEY, HashMap<(), ()>),
                version,
                constants,
            );
        }

        set_arg_value!(self.laser_target_local_pos, args, LASER_TARGET_LOCAL_POS_KEY, u16);

        // TODO: AntiAirAuras
        if args.contains_key(SELECTED_WEAPON_KEY)
            && let Some(wt) = WeaponType::from_id(
                arg_value_to_type!(args, SELECTED_WEAPON_KEY, u32) as i32,
                constants.ships(),
                version,
            )
        {
            self.selected_weapon = wt;
        }

        set_arg_value!(self.is_on_forsage, args, IS_ON_FORSAGE_KEY, bool);

        set_arg_value!(self.is_in_rage_mode, args, IS_IN_RAGE_MODE_KEY, bool);

        set_arg_value!(self.has_air_targets_in_range, args, HAS_AIR_TARGETS_IN_RANGE_KEY, bool);

        set_arg_value!(self.torpedo_local_pos, args, TORPEDO_LOCAL_POS_KEY, u16);

        // TODO: airDefenseTargetIds

        set_arg_value!(self.buoyancy, args, BUOYANCY_KEY, f32);

        set_arg_value!(self.max_health, args, MAX_HEALTH_KEY, f32);

        set_arg_value!(self.draught, args, DRAUGHT_KEY, f32);

        set_arg_value!(self.rudders_angle, args, RUDDERS_ANGLE_KEY, f32);

        set_arg_value!(self.target_local_pos, args, TARGET_LOCAL_POSITION_KEY, u16);

        set_arg_value!(self.triggered_skills_data, args, TRIGGERED_SKILLS_DATA_KEY, Vec<u8>);

        set_arg_value!(self.regenerated_health, args, REGENERATED_HEALTH_KEY, f32);

        set_arg_value!(self.blocked_controls, args, BLOCKED_CONTROLS_KEY, u8);

        set_arg_value!(self.is_invisible, args, IS_INVISIBLE_KEY, bool);

        set_arg_value!(self.is_fog_horn_on, args, IS_FOG_HORN_ON_KEY, bool);

        set_arg_value!(self.server_speed_raw, args, SERVER_SPEED_RAW_KEY, u16);

        set_arg_value!(self.regen_crew_hp_limit, args, REGEN_CREW_HP_LIMIT_KEY, f32);

        // TODO: miscs_presets_status

        set_arg_value!(self.buoyancy_current_waterline, args, BUOYANCY_CURRENT_WATERLINE_KEY, f32);
        set_arg_value!(self.is_alive, args, IS_ALIVE_KEY, bool);
        set_arg_value!(self.is_bot, args, IS_BOT_KEY, bool);
        set_arg_value!(self.visibility_flags, args, VISIBILITY_FLAGS_KEY, u32);

        // TODO: heatInfos

        set_arg_value!(self.buoyancy_rudder_index, args, BUOYANCY_RUDDER_INDEX_KEY, u8);
        set_arg_value!(self.is_anti_air_mode, args, IS_ANTI_AIR_MODE_KEY, bool);
        set_arg_value!(self.speed_sign_dir, args, SPEED_SIGN_DIR_KEY, i8);
        set_arg_value!(self.oil_leak_state, args, OIL_LEAK_STATE_KEY, u8);

        // TODO: sounds

        if args.contains_key(SHIP_CONFIG_KEY) {
            let ship_config = parse_ship_config(arg_value_to_type!(args, SHIP_CONFIG_KEY, &[u8]), &version)
                .expect("failed to parse ship config");

            self.ship_config = ship_config;
        }

        set_arg_value!(self.wave_local_pos, args, WAVE_LOCAL_POS_KEY, u16);
        set_arg_value!(self.has_active_main_squadron, args, HAS_ACTIVE_MAIN_SQUADRON_KEY, bool);
        set_arg_value!(self.weapon_lock_flags, args, WEAPON_LOCK_FLAGS_KEY, u16);
        set_arg_value!(self.deep_rudders_angle, args, DEEP_RUDDERS_ANGLE_KEY, f32);

        // TODO: debugText

        set_arg_value!(self.health, args, HEALTH_KEY, f32);
        set_arg_value!(self.engine_dir, args, ENGINE_DIR_KEY, i8);

        // TODO: state

        set_arg_value!(self.team_id, args, TEAM_ID_KEY, i8);
        if args.contains_key(BUOYANCY_CURRENT_STATE_KEY)
            && let Some(ds) = BuoyancyState::from_id(
                arg_value_to_type!(args, BUOYANCY_CURRENT_STATE_KEY, u8) as i32,
                constants.battle(),
                version,
            )
        {
            self.buoyancy_current_state = ds;
        }
        set_arg_value!(self.ui_enabled, args, UI_ENABLED_KEY, bool);
        set_arg_value!(self.respawn_time, args, RESPAWN_TIME_KEY, u16);
        set_arg_value!(self.engine_power, args, ENGINE_POWER_KEY, u8);
        set_arg_value!(self.max_server_speed_raw, args, MAX_SERVER_SPEED_RAW_KEY, u32);
        set_arg_value!(self.burning_flags, args, BURNING_FLAGS_KEY, u16);
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

impl From<&Death> for DeathInfo {
    fn from(death: &Death) -> Self {
        // Can occur if the player rejoins a game
        let time_lived = if death.timestamp > TIME_UNTIL_GAME_START {
            death.timestamp - TIME_UNTIL_GAME_START
        } else {
            Duration::from_secs(0)
        };

        DeathInfo { time_lived, killer: death.killer, cause: death.cause.clone() }
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
            .map(|skill_type| captain.skill_by_type(*skill_type as u32).expect("could not get skill type"))
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

#[derive(Debug)]
struct Death {
    timestamp: Duration,
    killer: EntityId,
    victim: EntityId,
    cause: Recognized<DeathCause>,
}

impl<'res, 'replay, G> BattleControllerState for BattleController<'res, 'replay, G>
where
    G: ResourceLoader,
{
    fn clock(&self) -> GameClock {
        self.current_clock
    }

    fn ship_positions(&self) -> &HashMap<EntityId, ShipPosition> {
        &self.ship_positions
    }

    fn minimap_positions(&self) -> &HashMap<EntityId, MinimapPosition> {
        &self.minimap_positions
    }

    fn player_entities(&self) -> &HashMap<EntityId, Rc<Player>> {
        &self.player_entities
    }

    fn metadata_players(&self) -> &[SharedPlayer] {
        &self.metadata_players
    }

    fn entities_by_id(&self) -> &HashMap<EntityId, Entity> {
        &self.entities_by_id
    }

    fn capture_points(&self) -> &[CapturePointState] {
        &self.capture_points
    }

    fn buff_zones(&self) -> &HashMap<EntityId, BuffZoneState> {
        &self.buff_zones
    }

    fn local_weather_zones(&self) -> &[LocalWeatherZone] {
        &self.local_weather_zones
    }

    fn captured_buffs(&self) -> &[CapturedBuff] {
        &self.captured_buffs
    }

    fn team_scores(&self) -> &[TeamScore] {
        &self.team_scores
    }

    fn game_chat(&self) -> &[GameMessage] {
        &self.game_chat
    }

    fn active_consumables(&self) -> &HashMap<EntityId, Vec<ActiveConsumable>> {
        &self.active_consumables
    }

    fn active_shots(&self) -> &[ActiveShot] {
        &self.active_shots
    }

    fn active_torpedoes(&self) -> &[ActiveTorpedo] {
        &self.active_torpedoes
    }

    fn shot_hits(&self) -> &[ResolvedShotHit] {
        &self.shot_hits
    }

    fn active_planes(&self) -> &HashMap<PlaneId, ActivePlane> {
        &self.active_planes
    }

    fn active_wards(&self) -> &HashMap<PlaneId, ActiveWard> {
        &self.active_wards
    }

    fn kills(&self) -> &[KillRecord] {
        &self.kills
    }

    fn dead_ships(&self) -> &HashMap<EntityId, DeadShip> {
        &self.dead_ships
    }

    fn battle_end_clock(&self) -> Option<GameClock> {
        self.battle_end_clock
    }

    fn winning_team(&self) -> Option<i8> {
        self.winning_team
    }

    fn finish_type(&self) -> Option<&Recognized<FinishType>> {
        self.finish_type.as_ref()
    }

    fn turret_yaws(&self) -> &HashMap<EntityId, Vec<f32>> {
        &self.turret_yaws
    }

    fn target_yaws(&self) -> &HashMap<EntityId, f32> {
        &self.target_yaws
    }

    fn selected_ammo(&self) -> &HashMap<EntityId, GameParamId> {
        &self.selected_ammo
    }

    fn battle_type(&self) -> Recognized<BattleType> {
        BattleType::from_value(&self.game_meta.gameType, self.version)
    }

    fn scoring_rules(&self) -> Option<&ScoringRules> {
        self.scoring_rules.as_ref()
    }

    fn time_left(&self) -> Option<i64> {
        self.time_left
    }

    fn battle_stage(&self) -> Option<BattleStage> {
        let raw = self.battle_stage?;
        self.constants().common().battle_stage(raw as i32).cloned()
    }

    fn battle_start_clock(&self) -> Option<GameClock> {
        self.battle_start_clock
    }

    fn game_clock_to_elapsed(&self, clock: GameClock) -> ElapsedClock {
        let start = self.battle_start_clock.unwrap_or(GameClock(0.0));
        clock.to_elapsed(start)
    }

    fn elapsed_to_game_clock(&self, elapsed: ElapsedClock) -> GameClock {
        let start = self.battle_start_clock.unwrap_or(GameClock(0.0));
        elapsed.to_absolute(start)
    }

    fn self_ribbons(&self) -> &HashMap<Ribbon, usize> {
        &self.self_ribbons
    }

    fn self_damage_stats(
        &self,
    ) -> &HashMap<(Recognized<DamageStatWeapon>, Recognized<DamageStatCategory>), DamageStatEntry> {
        &self.self_damage_stats
    }
}

impl<'res, 'replay, G> Analyzer for BattleController<'res, 'replay, G>
where
    G: ResourceLoader,
{
    fn process(&mut self, packet: &Packet<'_, '_>) {
        let span = span!(Level::TRACE, "packet processing");
        let _enter = span.enter();

        self.current_clock = packet.clock;
        if self.track_shots {
            self.shot_hits.clear();
        }

        let default_constants = &*crate::game_constants::DEFAULT_GAME_CONSTANTS;
        let constants = self.game_constants.unwrap_or(default_constants);
        let packet_decoder = crate::analyzer::decoder::PacketDecoder::builder()
            .version(self.version)
            .battle_constants(constants.battle())
            .common_constants(constants.common())
            .ships_constants(constants.ships())
            .build();
        let decoded = packet_decoder.decode(packet);
        match decoded.payload {
            crate::analyzer::decoder::DecodedPacketPayload::Chat {
                entity_id,
                sender_id,
                audience,
                message,
                extra_data,
            } => {
                self.handle_chat_message(entity_id, sender_id, audience, message, extra_data, packet.clock);
            }
            crate::analyzer::decoder::DecodedPacketPayload::VoiceLine { .. } => {
                trace!("HANDLE VOICE LINE");
            }
            crate::analyzer::decoder::DecodedPacketPayload::Ribbon(ribbon) => {
                *self.self_ribbons.entry(ribbon).or_insert(0) += 1;
            }
            crate::analyzer::decoder::DecodedPacketPayload::Position(pos) => {
                let world_pos = WorldPos { x: pos.position.x, y: pos.position.y, z: pos.position.z };
                let ship_pos = ShipPosition {
                    entity_id: pos.pid,
                    position: world_pos,
                    yaw: pos.rotation.yaw,
                    pitch: pos.rotation.pitch,
                    roll: pos.rotation.roll,
                    last_updated: packet.clock,
                };
                self.ship_positions.insert(pos.pid, ship_pos);
            }
            crate::analyzer::decoder::DecodedPacketPayload::PlayerOrientation(ref orientation) => {
                if orientation.parent_id == EntityId::from(0u32) {
                    let ship_pos = ShipPosition {
                        entity_id: orientation.pid,
                        position: WorldPos {
                            x: orientation.position.x,
                            y: orientation.position.y,
                            z: orientation.position.z,
                        },
                        yaw: orientation.rotation.yaw,
                        pitch: orientation.rotation.pitch,
                        roll: orientation.rotation.roll,
                        last_updated: packet.clock,
                    };
                    self.ship_positions.insert(orientation.pid, ship_pos);
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::DamageStat(ref entries) => {
                for entry in entries {
                    self.self_damage_stats.insert((entry.weapon.clone(), entry.category.clone()), entry.clone());
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::ShipDestroyed { killer, victim, cause } => {
                self.frags.entry(killer).or_default().push(Death {
                    timestamp: packet.clock.to_duration(),
                    killer,
                    victim,
                    cause: cause.clone(),
                });
                self.kills.push(KillRecord { clock: packet.clock, killer, victim, cause });
                // Record dead ship position from both sources when available.
                let world_pos = self.ship_positions.get(&victim).map(|sp| sp.position);
                let minimap_pos = self.minimap_positions.get(&victim).map(|mm| mm.position);
                let is_known_player = self.player_entities.values().any(|p| p.initial_state.entity_id() == victim);
                let has_vehicle = self.entities_by_id.get(&victim).and_then(|e| e.vehicle_ref()).is_some();
                debug!(
                    ?victim,
                    ?killer,
                    has_world_pos = world_pos.is_some(),
                    has_minimap_pos = minimap_pos.is_some(),
                    is_known_player,
                    has_vehicle,
                    clock = %packet.clock,
                    "ShipDestroyed"
                );
                self.dead_ships.insert(
                    victim,
                    DeadShip { clock: packet.clock, position: world_pos, minimap_position: minimap_pos },
                );
            }
            crate::analyzer::decoder::DecodedPacketPayload::EntityMethod(method) => {
                debug!("ENTITY METHOD, {:#?}", method)
            }
            crate::analyzer::decoder::DecodedPacketPayload::EntityProperty(prop) => {
                let entity_id = prop.entity_id;
                if let Some(entity) = self.entities_by_id.get(&entity_id)
                    && let Some(vehicle) = entity.vehicle_ref()
                {
                    let mut vehicle = RefCell::borrow_mut(vehicle);
                    vehicle.props.update_by_name(prop.property, &prop.value, self.version, self.constants());
                }
                // Handle targetLocalPos — packed turret aim direction
                if prop.property == "targetLocalPos"
                    && let Some(val) = prop.value.as_i64()
                {
                    let lo = (val & 0xFF) as f32;
                    // lo byte encodes world-space yaw: (lo/256)*2*PI - PI
                    let yaw = (lo / 256.0) * std::f32::consts::TAU - std::f32::consts::PI;
                    self.target_yaws.insert(entity_id, yaw);
                }
                // Handle InteractiveZone teamId changes (packet type 0x7)
                if prop.property == "teamId"
                    && let Some(&cp_idx) = self.interactive_zone_indices.get(&entity_id)
                    && let Some(v) = prop.value.as_i64()
                {
                    self.capture_points[cp_idx].team_id = v;
                }
                // Handle BattleLogic timeLeft property (seconds remaining)
                if prop.property == "timeLeft"
                    && let Some(v) = prop.value.as_i64()
                {
                    self.time_left = Some(v);
                }
                // Handle BattleLogic battleStage property
                if prop.property == "battleStage"
                    && let Some(v) = prop.value.as_i64()
                {
                    // Record when battle becomes active (raw value 0 = BattleStage::Waiting)
                    if self.battle_start_clock.is_none() {
                        let resolved = constants.common().battle_stage(v as i32).copied();
                        if matches!(resolved, Some(BattleStage::Waiting)) {
                            self.battle_start_clock = Some(packet.clock);
                        }
                    }
                    self.battle_stage = Some(v);
                }
                // Handle BattleLogic battleResult property (winning team + finish reason)
                if prop.property == "battleResult"
                    && let Some(dict) = Self::as_dict(&prop.value)
                {
                    // winnerTeamId: -2 = undecided (initial), -1 = draw, 0/1 = team won
                    if let Some(winner) = dict.get("winnerTeamId").and_then(|v| v.as_i64())
                        && winner >= -1
                    {
                        self.winning_team = Some(winner as i8);
                    }
                    if let Some(reason) = dict.get("finishReason").and_then(|v| v.as_i64())
                        && reason > 0
                    {
                        self.finish_type = FinishType::from_id(reason as i32, constants.battle(), self.version);
                    }
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::BasePlayerCreate(_base) => {
                trace!("BASE PLAYER CREATE");
            }
            crate::analyzer::decoder::DecodedPacketPayload::CellPlayerCreate(_cell) => {
                trace!("CELL PLAYER CREATE");
            }
            crate::analyzer::decoder::DecodedPacketPayload::EntityEnter(_e) => {
                trace!("ENTITY ENTER")
            }
            crate::analyzer::decoder::DecodedPacketPayload::EntityLeave(leave) => {
                let entity_id = leave.entity_id;
                if self.entities_by_id.get(&entity_id).and_then(|e| e.smoke_screen_ref()).is_some() {
                    self.entities_by_id.remove(&entity_id);
                }
                // Remove buff zones on EntityLeave (arms race: zone consumed)
                self.buff_zones.remove(&entity_id);
                // Remove world position so the ship stops rendering at a stale location.
                // The minimap entry is kept (if any) so undetected ships can still
                // be drawn with a different icon.
                self.ship_positions.remove(&entity_id);
            }
            crate::analyzer::decoder::DecodedPacketPayload::EntityCreate(entity_create) => {
                self.handle_entity_create(packet.clock, entity_create);
            }
            crate::analyzer::decoder::DecodedPacketPayload::OnArenaStateReceived {
                arena_id: arg0,
                team_build_type_id: _,
                pre_battles_info: _,
                player_states: players,
                bot_states: bots,
            } => {
                debug!("OnArenaStateReceived");
                self.arena_id = arg0;
                for player in players.iter().chain(bots.iter()) {
                    let metadata_player = self
                        .metadata_players
                        .iter()
                        .find(|meta_player| meta_player.id == player.meta_ship_id())
                        .expect("could not map arena player to metadata player");

                    let battle_player =
                        Player::from_arena_player(player, metadata_player.as_ref(), self.game_resources);

                    let player_has_died = self
                        .entities_by_id
                        .get(&player.entity_id())
                        .map(|vehicle| {
                            let Some(vehicle) = vehicle.vehicle_ref() else {
                                return false;
                            };
                            let vehicle = RefCell::borrow(vehicle);

                            self.frags.values().any(|deaths| deaths.iter().any(|death| death.victim == vehicle.id()))
                        })
                        .unwrap_or_default();

                    if player.is_connected() {
                        battle_player.connection_change_info_mut().push(ConnectionChangeInfo {
                            at_game_duration: packet.clock.to_duration(),
                            event_kind: ConnectionChangeKind::Connected,
                            had_death_event: player_has_died,
                        });
                    }

                    let battle_player = Rc::new(battle_player);

                    self.player_entities.insert(battle_player.initial_state.entity_id(), battle_player);
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::NewPlayerSpawnedInBattle {
                player_states: players,
                bot_states: bots,
            } => {
                debug!("NewPlayerSpawnedInBattle: {} players, {} bots", players.len(), bots.len());
                for player in players.iter().chain(bots.iter()) {
                    if let Some(battle_player) = Player::from_spawned_player(player, self.game_resources) {
                        let entity_id = player.entity_id();
                        // Add to metadata_players so future OnArenaStateReceived/OnGameRoomStateChanged
                        // can find this player.
                        self.metadata_players.push(Rc::new(MetadataPlayer {
                            id: player.meta_ship_id(),
                            name: player.username().to_string(),
                            relation: battle_player.relation(),
                            vehicle: Rc::clone(&battle_player.vehicle),
                        }));
                        let battle_player = Rc::new(battle_player);
                        self.player_entities.insert(entity_id, battle_player);
                    }
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::CheckPing(_) => trace!("CHECK PING"),
            crate::analyzer::decoder::DecodedPacketPayload::DamageReceived { victim, ref aggressors } => {
                for damage in aggressors {
                    self.damage_dealt
                        .entry(damage.aggressor)
                        .or_default()
                        .push(DamageEvent { amount: damage.damage, victim, clock: packet.clock });
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::MinimapUpdate { ref updates, arg1: _ } => {
                for update in updates {
                    // Minimap pings (hydrophone etc.) are one-shot position
                    // flashes that should not be treated as sustained detection.
                    let visible = !update.is_sentinel && !update.is_minimap_ping();
                    // When not visible, preserve last known position and heading
                    // so undetected ships render at their last location.
                    let (position, heading) = if !visible {
                        let prev = self.minimap_positions.get(&update.entity_id);
                        (
                            prev.map(|p| p.position).unwrap_or(update.position),
                            prev.map(|p| p.heading).unwrap_or(update.heading),
                        )
                    } else {
                        (update.position, update.heading)
                    };
                    // Pull visibility_flags and is_invisible from the Vehicle
                    // entity props (tracks radar/hydro detection and submarine
                    // submerged state).
                    let (visibility_flags, is_invisible) = self
                        .entities_by_id
                        .get(&update.entity_id)
                        .and_then(|e| e.vehicle_ref())
                        .map(|v| {
                            let v = v.borrow();
                            (v.props().visibility_flags(), v.props().is_invisible())
                        })
                        .unwrap_or((0, false));
                    self.minimap_positions.insert(
                        update.entity_id,
                        MinimapPosition {
                            entity_id: update.entity_id,
                            position,
                            heading,
                            visible,
                            visibility_flags,
                            is_invisible,
                            last_updated: packet.clock,
                        },
                    );
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::PropertyUpdate(update) => {
                if self.entities_by_id.contains_key(&update.entity_id) {
                    debug!("PROPERTY UPDATE: {:#?}", update);
                }
                // Handle smoke screen point mutations
                if update.property == "points"
                    && let Some(entity) = self.entities_by_id.get(&update.entity_id)
                    && let Some(smoke_ref) = entity.smoke_screen_ref()
                {
                    let mut smoke = RefCell::borrow_mut(smoke_ref);
                    match &update.update_cmd.action {
                        UpdateAction::SetRange { start, values, .. } => {
                            while smoke.points.len() < start + values.len() {
                                smoke.points.push(WorldPos::default());
                            }
                            for (i, v) in values.iter().enumerate() {
                                if let ArgValue::Vector3((x, y, z)) = v {
                                    smoke.points[start + i] = WorldPos { x: *x, y: *y, z: *z };
                                }
                            }
                        }
                        UpdateAction::RemoveRange { start, stop } => {
                            let end = (*stop).min(smoke.points.len());
                            smoke.points.drain(*start..end);
                        }
                        _ => {}
                    }
                    drop(smoke);
                }
                // Handle InteractiveZone (capture point) state updates
                // PropertyUpdates arrive as: property="componentsState", levels=[captureLogic], action=SetKey{key, value}
                if update.property == "componentsState"
                    && let Some(&cp_idx) = self.interactive_zone_indices.get(&update.entity_id)
                    && matches!(update.update_cmd.levels.first(), Some(PropertyNestLevel::DictKey("captureLogic")))
                    && let UpdateAction::SetKey { key, value } = &update.update_cmd.action
                    && cp_idx != usize::MAX
                {
                    // Capture point update
                    match *key {
                        "hasInvaders" => {
                            if let Some(v) = value.as_i64() {
                                self.capture_points[cp_idx].has_invaders = v != 0;
                            }
                        }
                        "invaderTeam" => {
                            if let Some(v) = value.as_i64() {
                                self.capture_points[cp_idx].invader_team = v;
                            }
                        }
                        "progress" => {
                            if let Some(f) = value.float_32_ref() {
                                self.capture_points[cp_idx].progress = (*f as f64, 0.0);
                            }
                        }
                        "bothInside" => {
                            if let Some(v) = value.as_i64() {
                                self.capture_points[cp_idx].both_inside = v != 0;
                            }
                        }
                        "teamId" | "invaderTeamId" => {
                            if let Some(v) = value.as_i64() {
                                self.capture_points[cp_idx].invader_team = v;
                            }
                        }
                        "isEnabled" => {
                            if let Some(v) = value.as_i64() {
                                self.capture_points[cp_idx].is_enabled = v != 0;
                            }
                        }
                        _ => {}
                    }
                }

                self.handle_property_update(packet.clock, update);
            }
            crate::analyzer::decoder::DecodedPacketPayload::BattleEnd { winning_team, finish_type } => {
                self.match_finished = true;
                self.battle_end_clock = Some(packet.clock);
                // Only overwrite if BattleEnd carries values (modern replays have None;
                // winning_team is already set from battleResult EntityProperty)
                if winning_team.is_some() {
                    self.winning_team = winning_team;
                }
                if finish_type.is_some() {
                    self.finish_type = finish_type;
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::Consumable { entity, consumable, duration } => {
                self.active_consumables.entry(entity).or_default().push(ActiveConsumable {
                    consumable,
                    activated_at: packet.clock,
                    duration,
                });
            }
            crate::analyzer::decoder::DecodedPacketPayload::ArtilleryShots { avatar_id, salvos } => {
                if self.track_shots {
                    for salvo in salvos {
                        self.active_shots.push(ActiveShot { avatar_id, salvo, fired_at: packet.clock });
                    }
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::TorpedoesReceived { avatar_id, torpedoes } => {
                for torpedo in torpedoes {
                    self.active_torpedoes.push(ActiveTorpedo {
                        avatar_id,
                        torpedo,
                        launched_at: packet.clock,
                        updated_at: packet.clock,
                    });
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::TorpedoDirection {
                owner_id,
                shot_id,
                position,
                target_yaw,
                speed_coef,
            } => {
                // Update homing torpedo: reset origin to current position and optionally
                // update heading. target_yaw ≈ 2π is a sentinel meaning "keep current heading".
                if let Some(torp) = self
                    .active_torpedoes
                    .iter_mut()
                    .find(|t| t.torpedo.owner_id == owner_id && t.torpedo.shot_id == shot_id)
                {
                    let base_speed = (torp.torpedo.direction.x.powi(2) + torp.torpedo.direction.z.powi(2)).sqrt();
                    let speed = base_speed * speed_coef;
                    torp.torpedo.origin = position;
                    // 2π sentinel = no target, keep current heading
                    if (target_yaw - std::f32::consts::TAU).abs() > 0.01 {
                        torp.torpedo.direction =
                            WorldPos { x: speed * target_yaw.sin(), y: 0.0, z: speed * target_yaw.cos() };
                    } else if (speed_coef - 1.0).abs() > 1e-6 {
                        // Keep heading but apply speed change
                        let dir_norm = torp.torpedo.direction * (1.0 / base_speed);
                        torp.torpedo.direction = dir_norm * speed;
                    }
                    torp.torpedo.maneuver_dump = None;
                    torp.updated_at = packet.clock;
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::PlanePosition { entity_id: _, plane_id, position } => {
                if let Some(plane) = self.active_planes.get_mut(&plane_id) {
                    plane.position = position;
                    plane.last_updated = packet.clock;
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::PlaneAdded {
                entity_id,
                plane_id,
                team_id,
                params_id,
                position,
            } => {
                self.active_planes.insert(
                    plane_id,
                    ActivePlane {
                        plane_id,
                        owner_id: entity_id,
                        team_id,
                        params_id,
                        position,
                        last_updated: packet.clock,
                    },
                );
            }
            crate::analyzer::decoder::DecodedPacketPayload::PlaneRemoved { entity_id: _, plane_id } => {
                self.active_planes.remove(&plane_id);
            }
            crate::analyzer::decoder::DecodedPacketPayload::WardAdded {
                plane_id, position, radius, owner_id, ..
            } => {
                self.active_wards.insert(plane_id, ActiveWard { plane_id, position, radius, owner_id });
            }
            crate::analyzer::decoder::DecodedPacketPayload::WardRemoved { plane_id, .. } => {
                self.active_wards.remove(&plane_id);
            }
            crate::analyzer::decoder::DecodedPacketPayload::GunSync { entity_id, group, turret, yaw, .. } => {
                // Only track main battery (group 0)
                if group == 0 {
                    let turrets = self.turret_yaws.entry(entity_id).or_default();
                    let idx = turret as usize;
                    if turrets.len() <= idx {
                        turrets.resize(idx + 1, 0.0);
                    }
                    turrets[idx] = yaw;
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::SetAmmoForWeapon {
                entity_id,
                weapon_type,
                ammo_param_id,
                is_reload: _,
            } => {
                // Track artillery ammo selection (weapon_type 0 = artillery)
                if weapon_type == 0 {
                    self.selected_ammo.insert(entity_id, ammo_param_id);
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::CruiseState { .. } => {
                trace!("CRUISE STATE")
            }
            crate::analyzer::decoder::DecodedPacketPayload::Map(_) => trace!("MAP"),
            crate::analyzer::decoder::DecodedPacketPayload::Version(_) => trace!("VERSION"),
            crate::analyzer::decoder::DecodedPacketPayload::Camera(_) => trace!("CAMERA"),
            crate::analyzer::decoder::DecodedPacketPayload::CameraMode(_) => {
                trace!("CAMERA MODE")
            }
            crate::analyzer::decoder::DecodedPacketPayload::CameraFreeLook(_) => {
                trace!("CAMERA FREE LOOK")
            }
            crate::analyzer::decoder::DecodedPacketPayload::ShotKills { avatar_id, hits } => {
                // receiveShotKills is called on the recording player's Avatar entity
                // for ALL shell impacts the client needs to know about — both incoming
                // hits on the recording player AND the recording player's outgoing hits
                // on enemies. The victim is NOT explicit in the packet.
                let self_ship_id =
                    self.player_entities.iter().find(|(_, p)| p.relation().is_self()).map(|(eid, _)| *eid);

                if self_ship_id.is_none() {
                    tracing::warn!("ShotKills received but self-player not yet known (avatar={avatar_id})");
                    return;
                }

                for hit in hits {
                    // Remove matching torpedo if this is a torpedo hit
                    if let Some(idx) = self
                        .active_torpedoes
                        .iter()
                        .position(|t| t.torpedo.owner_id == hit.owner_id && t.torpedo.shot_id == hit.shot_id)
                    {
                        self.active_torpedoes.swap_remove(idx);
                    }

                    // Try to match this hit to an active artillery salvo and remove it
                    let matched = self.active_shots.iter().find(|s| {
                        s.salvo.owner_id == hit.owner_id && s.salvo.shots.iter().any(|shot| shot.shot_id == hit.shot_id)
                    });
                    let (salvo, fired_at) = match matched {
                        Some(s) => (Some(s.salvo.clone()), Some(s.fired_at)),
                        None => (None, None),
                    };

                    // Resolve victim: find the ship closest to the salvo's average
                    // target position. If no salvo matched, fall back to self-player.
                    let victim_entity_id = salvo
                        .as_ref()
                        .and_then(|s| {
                            let n = s.shots.len() as f32;
                            if n < 1.0 {
                                return None;
                            }
                            let avg_target: WorldPos = s.shots.iter().map(|sh| sh.target).sum::<WorldPos>() / n;
                            self.ship_positions
                                .iter()
                                .min_by(|(_, a), (_, b)| {
                                    let da = a.position.distance_xz(&avg_target);
                                    let db = b.position.distance_xz(&avg_target);
                                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                                })
                                .map(|(eid, _)| *eid)
                        })
                        .or(self_ship_id)
                        .unwrap();

                    // Get victim ship position/yaw at impact time.
                    let victim_position =
                        self.ship_positions.get(&victim_entity_id).map(|sp| sp.position).unwrap_or_default();
                    let victim_yaw = self
                        .minimap_positions
                        .get(&victim_entity_id)
                        .map(|mm| std::f32::consts::FRAC_PI_2 - mm.heading.to_radians())
                        .or_else(|| self.ship_positions.get(&victim_entity_id).map(|sp| sp.yaw))
                        .unwrap_or(0.0);
                    let (victim_pitch, victim_roll) =
                        self.ship_positions.get(&victim_entity_id).map(|sp| (sp.pitch, sp.roll)).unwrap_or((0.0, 0.0));

                    self.shot_hits.push(ResolvedShotHit {
                        clock: packet.clock,
                        hit,
                        victim_entity_id,
                        salvo,
                        fired_at,
                        victim_position,
                        victim_yaw,
                        victim_pitch,
                        victim_roll,
                    });
                }

                // Expire stale active_shots (shells in flight > 30s are certainly landed)
                let cutoff = packet.clock.seconds() - 30.0;
                self.active_shots.retain(|s| s.fired_at.seconds() > cutoff);
            }
            crate::analyzer::decoder::DecodedPacketPayload::EntityControl(_) => {}
            crate::analyzer::decoder::DecodedPacketPayload::NonVolatilePosition(sd) => {
                let new_pos = WorldPos { x: sd.position.x, y: sd.position.y, z: sd.position.z };
                // Update SmokeScreen entity position
                if let Some(entity) = self.entities_by_id.get(&sd.entity_id)
                    && let Some(smoke_ref) = entity.smoke_screen_ref()
                {
                    RefCell::borrow_mut(smoke_ref).position = new_pos;
                }
                // Update weather zone position if linked
                for zone in &mut self.local_weather_zones {
                    if zone.entity_id == Some(sd.entity_id) {
                        zone.position = new_pos;
                        break;
                    }
                }
            }
            crate::analyzer::decoder::DecodedPacketPayload::PlayerNetStats(_) => {}
            crate::analyzer::decoder::DecodedPacketPayload::ServerTimestamp(_) => {}
            crate::analyzer::decoder::DecodedPacketPayload::OwnShip(_) => {}
            crate::analyzer::decoder::DecodedPacketPayload::SetWeaponLock(_) => {}
            crate::analyzer::decoder::DecodedPacketPayload::ServerTick(_) => {}
            crate::analyzer::decoder::DecodedPacketPayload::SubController(_) => {}
            crate::analyzer::decoder::DecodedPacketPayload::ShotTracking(_) => {}
            crate::analyzer::decoder::DecodedPacketPayload::GunMarker(_) => {}
            crate::analyzer::decoder::DecodedPacketPayload::SyncShipCracks { .. } => {}
            crate::analyzer::decoder::DecodedPacketPayload::InitFlag(_) => {}
            crate::analyzer::decoder::DecodedPacketPayload::InitMarker => {}
            crate::analyzer::decoder::DecodedPacketPayload::Unknown(_) => trace!("UNKNOWN"),
            crate::analyzer::decoder::DecodedPacketPayload::Invalid(_) => trace!("INVALID"),
            crate::analyzer::decoder::DecodedPacketPayload::Audit(_) => trace!("AUDIT"),
            crate::analyzer::decoder::DecodedPacketPayload::BattleResults(json) => {
                self.battle_results = Some(json.to_owned());
            }
            crate::analyzer::decoder::DecodedPacketPayload::OnGameRoomStateChanged { player_states } => {
                for player_state in &player_states {
                    let Some(meta_ship_id) = player_state.get(PlayerStateData::KEY_ID) else {
                        continue;
                    };

                    let meta_ship_id = *meta_ship_id.i64_ref().expect("player_id is not an i64");

                    let Some(player) = self
                        .player_entities
                        .values()
                        .find(|player| player.initial_state().meta_ship_id() == AccountId::from(meta_ship_id))
                    else {
                        debug!("Failed to find player with meta ship ID {meta_ship_id:?}");
                        continue;
                    };

                    {
                        player.end_state_mut().update_from_dict(player_state);
                    }

                    let player_has_died = self
                        .entities_by_id
                        .get(&player.initial_state().entity_id())
                        .map(|vehicle| {
                            let Some(vehicle) = vehicle.vehicle_ref() else {
                                return false;
                            };
                            let vehicle = RefCell::borrow(vehicle);

                            self.frags.values().any(|deaths| deaths.iter().any(|death| death.victim == vehicle.id()))
                        })
                        .unwrap_or_default();

                    let connection_event_kind = if player.end_state().is_connected() {
                        ConnectionChangeKind::Connected
                    } else {
                        ConnectionChangeKind::Disconnected
                    };

                    if (player.connection_change_info().is_empty()
                        && connection_event_kind != ConnectionChangeKind::Disconnected)
                        || player
                            .connection_change_info()
                            .last()
                            .map(|info| info.event_kind != connection_event_kind)
                            .unwrap_or_default()
                    {
                        player.connection_change_info_mut().push(ConnectionChangeInfo {
                            at_game_duration: packet.clock.to_duration(),
                            event_kind: connection_event_kind,
                            had_death_event: player_has_died,
                        });
                    }
                }
            }
        }
    }

    fn finish(&mut self) {}
}
