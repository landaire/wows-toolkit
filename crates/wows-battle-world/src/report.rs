//! End-of-battle report extraction.
//!
//! `BattleReport` is an owned snapshot of the battle state at `finish()` time.
//! It outlives the ECS world: consumers hold it after the world is dropped. It
//! reuses the value types from `wows_replays::analyzer::battle_controller` so
//! consumers read the same shapes regardless of which crate assembled them.

use std::collections::HashMap;
use std::str::FromStr;

use wows_replays::Rc;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::DeathInfo;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::VehicleEntity;
use wows_replays::analyzer::battle_controller::state::ActiveConsumable;
use wows_replays::analyzer::battle_controller::state::BuffZoneState;
use wows_replays::analyzer::battle_controller::state::BuildingEntity;
use wows_replays::analyzer::battle_controller::state::CapturePointState;
use wows_replays::analyzer::battle_controller::state::CapturedBuff;
use wows_replays::analyzer::battle_controller::state::LocalWeatherZone;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::analyzer::battle_controller::state::TeamScore;
use wows_replays::analyzer::decoder::DamageStatEntry;
use wows_replays::analyzer::decoder::FinishType;
use wows_replays::analyzer::decoder::Recognized;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::data::Version;
use wowsunpack::game_types::BattleType;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::game_types::ElapsedClock;

use crate::components::BuildingState;
use crate::components::Captain;
use crate::components::Division;
use crate::components::GameId;
use crate::components::VehicleState;
use crate::resources::BurnStateChange;
use crate::resources::BurnStateLog;
use crate::resources::ChatLog;
use crate::resources::DamageLedger;
use crate::resources::EntityIndex;
use crate::resources::HitHistoryLog;
use crate::resources::KillLog;
use crate::resources::MatchState;
use crate::resources::PlayerIndex;
use crate::resources::PresenceLog;
use crate::resources::RibbonEvent;
use crate::resources::RibbonLog;
use crate::resources::SelfStats;
use crate::world::BattleWorld;

/// Owned snapshot of the battle at finish time.
///
/// Logical field set mirrors the original `BattleReport`; consumers read it
/// through the getters below.
pub struct BattleReport {
    arena_id: ArenaId,
    self_player: Rc<Player>,
    version: Version,
    map_name: String,
    game_mode: String,
    game_type: Recognized<BattleType>,
    match_group: String,
    players: Vec<Rc<Player>>,
    divisions: HashMap<EntityId, char>,
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
    active_consumables: HashMap<EntityId, Vec<ActiveConsumable>>,
    burn_state_changes: Vec<BurnStateChange>,
    ribbon_events: Vec<RibbonEvent>,
    presence: PresenceLog,
    hit_history: Vec<ResolvedShotHit>,
    max_duration: u32,
    played_duration: Option<f32>,
    extra_duration: Option<f32>,
}

impl BattleReport {
    pub fn arena_id(&self) -> ArenaId {
        self.arena_id
    }

    pub fn self_player(&self) -> &Rc<Player> {
        &self.self_player
    }

    pub fn version(&self) -> Version {
        self.version
    }

    pub fn map_name(&self) -> &str {
        self.map_name.as_ref()
    }

    pub fn game_mode(&self) -> &str {
        self.game_mode.as_ref()
    }

    pub fn game_type(&self) -> &Recognized<BattleType> {
        &self.game_type
    }

    pub fn match_group(&self) -> &str {
        self.match_group.as_ref()
    }

    pub fn players(&self) -> &[Rc<Player>] {
        &self.players
    }

    /// In-game division label (A, B, C...) keyed by vehicle entity id.
    ///
    /// Only contains entries for players in a division; look up by
    /// `player.initial_state().entity_id()`.
    pub fn divisions(&self) -> &HashMap<EntityId, char> {
        &self.divisions
    }

    pub fn game_chat(&self) -> &[GameMessage] {
        self.game_chat.as_ref()
    }

    pub fn battle_results(&self) -> Option<&str> {
        self.battle_results.as_deref()
    }

    /// A map of players to the deaths they caused.
    ///
    /// `Player` keys carry interior mutability (mirroring the original report);
    /// the keys are never mutated, so the map's invariants hold.
    #[allow(clippy::mutable_key_type)]
    pub fn frags(&self) -> &HashMap<Rc<Player>, Vec<DeathInfo>> {
        &self.frags
    }

    /// The result of the battle. `None` if the player left before it finished.
    pub fn battle_result(&self) -> Option<&BattleResult> {
        self.match_result.as_ref()
    }

    pub fn finish_type(&self) -> Option<&Recognized<FinishType>> {
        self.finish_type.as_ref()
    }

    pub fn capture_points(&self) -> &[CapturePointState] {
        &self.capture_points
    }

    pub fn buff_zones(&self) -> &HashMap<EntityId, BuffZoneState> {
        &self.buff_zones
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

    pub fn local_weather_zones(&self) -> &[LocalWeatherZone] {
        &self.local_weather_zones
    }

    pub fn battle_start_clock(&self) -> Option<GameClock> {
        self.battle_start_clock
    }

    /// Server-authoritative per-weapon damage stats for the self player.
    pub fn self_damage_stats(&self) -> &[DamageStatEntry] {
        &self.self_damage_stats
    }

    /// All consumable activations observed during the match, keyed by avatar id.
    pub fn active_consumables(&self) -> &HashMap<EntityId, Vec<ActiveConsumable>> {
        &self.active_consumables
    }

    /// Burn-bit transitions observed on any vehicle. See `BurnStateLog`'s doc
    /// comment: not a complete history, since a vehicle that re-enters the
    /// recording client's AOI after a gap can arrive with a changed burn mask
    /// and emit no transition for whatever happened while unobserved. Bound
    /// any reconstruction with `presence()`.
    pub fn burn_state_changes(&self) -> &[BurnStateChange] {
        &self.burn_state_changes
    }

    /// Self-player ribbon increments with clocks. Self-player only; see
    /// `RibbonLog`'s doc comment for the update shapes that produce no event.
    pub fn ribbon_events(&self) -> &[RibbonEvent] {
        &self.ribbon_events
    }

    /// AOI observation windows per vehicle, from `EntityCreate` to
    /// `EntityLeave`. See `PresenceLog`'s doc comment: an entry is proof of
    /// observation, but its absence is not proof of the converse in every
    /// case (an undetected `EntityLeave` or entity-id reuse can extend a
    /// window silently).
    pub fn presence(&self) -> &PresenceLog {
        &self.presence
    }

    /// Every resolved shell hit for the whole match, in arrival order. Empty
    /// unless `IngestOptions::record_hit_history` was set during ingest, and
    /// also empty under `ShotTracking::Untracked`; see `HitHistoryLog`'s doc
    /// comment.
    pub fn hit_history(&self) -> &[ResolvedShotHit] {
        &self.hit_history
    }

    /// Maximum match duration from replay metadata (time limit), in seconds.
    pub fn max_duration(&self) -> u32 {
        self.max_duration
    }

    /// Played duration of the battle phase (battle start to battle end), in seconds.
    pub fn played_duration(&self) -> Option<f32> {
        self.played_duration
    }

    /// Time between battle end and last recorded packet, in seconds.
    pub fn extra_duration(&self) -> Option<f32> {
        self.extra_duration
    }

    pub fn game_clock_to_elapsed(&self, clock: GameClock) -> ElapsedClock {
        let start = self.battle_start_clock.unwrap_or(GameClock(0.0));
        clock.to_elapsed(start)
    }

    pub fn elapsed_to_game_clock(&self, elapsed: ElapsedClock) -> GameClock {
        let start = self.battle_start_clock.unwrap_or(GameClock(0.0));
        elapsed.to_absolute(start)
    }
}

impl<'res, 'replay, G: ResourceLoader> BattleWorld<'res, 'replay, G> {
    /// Consume the world and assemble the finish-time battle report.
    #[allow(clippy::mutable_key_type)]
    pub fn into_report(mut self) -> BattleReport {
        // Per-vehicle damage from receiveDamagesOnShip, folded per aggressor.
        // For non-self players this is the only damage source available.
        let damage_by_entity: HashMap<EntityId, f64> = self
            .world()
            .resource::<DamageLedger>()
            .0
            .iter()
            .map(|(aggressor, events)| {
                let total = events.iter().fold(0.0f64, |accum, event| accum + event.amount as f64);
                (*aggressor, total)
            })
            .collect();

        // Server-authoritative override for the self player. DamageReceived events
        // only cover visible targets, missing DoT on ships outside the client AoI.
        // Only Enemy category entries represent actual damage dealt.
        let self_damage_stats = &self.world().resource::<SelfStats>().damage_stats;
        let authoritative_self_damage: Option<f64> = if self_damage_stats.is_empty() {
            None
        } else {
            Some(
                self_damage_stats
                    .values()
                    .filter(|entry| entry.category == Recognized::Known(DamageStatCategory::Enemy))
                    .map(|entry| entry.total)
                    .sum(),
            )
        };

        // Frags by killer entity and the death of each victim, from the kill log.
        // Frags are attributed unconditionally so kills show on older replays that
        // carry no post-battle results blob.
        let mut frags_by_killer: HashMap<EntityId, Vec<DeathInfo>> = HashMap::new();
        // First kill per victim in packet order; deterministic unlike the original's
        // HashMap iteration (only differs when a victim id appears in multiple
        // ShipDestroyed events, e.g. operation respawns).
        let mut death_by_victim: HashMap<EntityId, DeathInfo> = HashMap::new();
        for kill in &self.world().resource::<KillLog>().0 {
            frags_by_killer.entry(kill.killer).or_default().push(DeathInfo::from(kill));
            death_by_victim.entry(kill.victim).or_insert_with(|| DeathInfo::from(kill));
        }

        let parsed_battle_results = self
            .world()
            .resource::<MatchState>()
            .battle_results
            .as_ref()
            .and_then(|results| serde_json::Value::from_str(results.as_str()).ok());

        let player_entities: Vec<Rc<Player>> = self.world().resource::<PlayerIndex>().0.values().cloned().collect();

        // Build final Player objects with an owned VehicleEntity. Players without a
        // matching vehicle entity (disconnected, bots without EntityCreate) keep
        // vehicle_entity = None.
        let players: Vec<Rc<Player>> = player_entities
            .iter()
            .map(|player| {
                let entity_id = player.initial_state().entity_id();
                let db_id = player.initial_state().db_id();
                let vehicle = self.build_vehicle_entity(
                    entity_id,
                    db_id,
                    &damage_by_entity,
                    authoritative_self_damage,
                    player.relation().is_self(),
                    &death_by_victim,
                    &frags_by_killer,
                    parsed_battle_results.as_ref(),
                );

                let mut final_player = player.as_ref().clone();
                final_player.set_vehicle_entity(vehicle);
                Rc::new(final_player)
            })
            .collect();

        let frags: HashMap<Rc<Player>, Vec<DeathInfo>> =
            HashMap::from_iter(frags_by_killer.into_iter().filter_map(|(entity_id, kills)| {
                let player = players.iter().find(|p| p.initial_state().entity_id() == entity_id)?;
                Some((Rc::clone(player), kills))
            }));

        // Pre-0.9 replays carry no roster RPC, so no player is ever tagged Self
        // and the report cannot be built. Fail fast and loud, matching the original.
        let self_player = players.iter().find(|player| player.relation().is_self()).cloned().expect(
            "could not resolve the recording (self) player: replay carries no roster RPC \
             (pre-0.9 format, e.g. build 8.5.1 / 0.8.5)",
        );

        let match_state = self.world().resource::<MatchState>();
        let battle_start_clock = match_state.battle_start_clock;
        let battle_end_clock = match_state.battle_end_clock;
        let match_finished = match_state.match_finished;
        // The match result clock (battleResult property) marks regulation end.
        // Fall back to BattleEnd packet clock if battleResult wasn't observed.
        let match_end = match_state.battle_result_clock.or(battle_end_clock);
        let finish_type = match_state.finish_type.clone();
        let battle_results = match_state.battle_results.clone();

        let played_duration = match (battle_start_clock, match_end) {
            (Some(start), Some(end)) => Some(end.seconds() - start.seconds()),
            _ => None,
        };

        let extra_duration = match (match_end, battle_end_clock) {
            (Some(result), Some(end)) if end.seconds() > result.seconds() => Some(end.seconds() - result.seconds()),
            _ => None,
        };

        let self_team_id = self_player.initial_state().team_id() as i8;
        let match_result = if match_finished {
            self.winning_team().map(|team| {
                if team == self_team_id {
                    BattleResult::Win(team)
                } else if team >= 0 {
                    BattleResult::Loss(team)
                } else {
                    BattleResult::Draw
                }
            })
        } else {
            None
        };

        let arena_id = self.arena_id().unwrap_or_else(|| ArenaId::from(0));
        let version = self.report_version();
        let match_group = self.report_match_group();
        let map_name = self.report_map_name();
        let game_mode = self.report_game_mode();
        let game_type = self.report_game_type();
        let max_duration = self.meta().duration;
        let game_chat = self.world().resource::<ChatLog>().0.clone();
        let self_damage_stats: Vec<DamageStatEntry> =
            self.world().resource::<SelfStats>().damage_stats.values().cloned().collect();

        // Division labels were materialized onto vehicle entities during ingest.
        let divisions: HashMap<EntityId, char> = {
            let mut query = self.world_mut().query::<(&GameId, &Division)>();
            query.iter(self.world()).map(|(gid, d)| (gid.0, d.letter)).collect()
        };

        let buildings = self.report_buildings();
        let capture_points = self.capture_points();
        let team_scores = self.team_scores();
        let captured_buffs = self.captured_buffs();
        let local_weather_zones = self.local_weather_zones();
        let buff_zones = self.buff_zones();
        let active_consumables = self.active_consumables();

        // Moved out, not cloned: HitHistoryLog in particular can hold every hit
        // of the match, and the world is dropped right after this call anyway.
        let burn_state_changes = std::mem::take(&mut self.world_mut().resource_mut::<BurnStateLog>().0);
        let ribbon_events = std::mem::take(&mut self.world_mut().resource_mut::<RibbonLog>().0);
        let presence = std::mem::take(&mut *self.world_mut().resource_mut::<PresenceLog>());
        let hit_history = std::mem::take(&mut self.world_mut().resource_mut::<HitHistoryLog>().0);

        BattleReport {
            arena_id,
            self_player,
            version,
            map_name,
            game_mode,
            game_type,
            match_group,
            players,
            divisions,
            game_chat,
            battle_results,
            frags,
            match_result,
            finish_type,
            capture_points,
            buff_zones,
            captured_buffs,
            team_scores,
            buildings,
            local_weather_zones,
            battle_start_clock,
            self_damage_stats,
            active_consumables,
            burn_state_changes,
            ribbon_events,
            presence,
            hit_history,
            max_duration,
            played_duration,
            extra_duration,
        }
    }

    /// Build a populated VehicleEntity for one player's entity, or None if the
    /// player has no vehicle in the world (disconnected / unspawned bot).
    #[allow(clippy::too_many_arguments)]
    fn build_vehicle_entity(
        &self,
        entity_id: EntityId,
        db_id: AccountId,
        damage_by_entity: &HashMap<EntityId, f64>,
        authoritative_self_damage: Option<f64>,
        is_self: bool,
        death_by_victim: &HashMap<EntityId, DeathInfo>,
        frags_by_killer: &HashMap<EntityId, Vec<DeathInfo>>,
        parsed_battle_results: Option<&serde_json::Value>,
    ) -> Option<VehicleEntity> {
        let ecs_entity = self.world().resource::<EntityIndex>().get(entity_id)?;
        let entity_ref = self.world().get_entity(ecs_entity).ok()?;
        let props = entity_ref.get::<VehicleState>()?.0.clone();
        // Read the captain frozen at EntityCreate time, mirroring BattleController
        // which resolves captain from create-time props and never refreshes it.
        let captain = entity_ref.get::<Captain>().and_then(|c| c.0.clone());

        let damage = if is_self {
            authoritative_self_damage.unwrap_or_else(|| damage_by_entity.get(&entity_id).copied().unwrap_or(0.0))
        } else {
            damage_by_entity.get(&entity_id).copied().unwrap_or(0.0)
        };

        let death_info = death_by_victim.get(&entity_id).cloned();

        let results_info = parsed_battle_results.and_then(|results| results.as_object()).and_then(|results| {
            results
                .get("playersPublicInfo")
                .and_then(|infos| infos.as_object().and_then(|infos| infos.get(db_id.to_string().as_str()).cloned()))
        });

        let frags = frags_by_killer.get(&entity_id).cloned().unwrap_or_default();

        Some(VehicleEntity::new(entity_id, 0.0, props, captain, damage, death_info, results_info, frags))
    }

    fn report_version(&self) -> Version {
        Version::from_client_exe(&self.meta().clientVersionFromExe)
    }

    fn report_match_group(&self) -> String {
        self.meta().matchGroup.clone().unwrap_or_default()
    }

    fn report_map_name(&self) -> String {
        let id = format!("IDS_{}", self.meta().mapName.to_uppercase());
        self.resources().localized_name_from_id(&TranslationKey::new(id)).unwrap_or_else(|| self.meta().mapName.clone())
    }

    fn report_game_mode(&self) -> String {
        let id = format!("IDS_SCENARIO_{}", self.meta().scenario.to_uppercase());
        self.resources()
            .localized_name_from_id(&TranslationKey::new(id))
            .unwrap_or_else(|| self.meta().scenario.clone())
    }

    fn report_game_type(&self) -> Recognized<BattleType> {
        BattleType::from_value(self.meta().gameType.as_deref().unwrap_or(""), self.version())
    }

    /// Building entities reconstructed from world `BuildingState`.
    fn report_buildings(&mut self) -> Vec<BuildingEntity> {
        let world = self.world_mut();
        let mut q = world.query::<(&GameId, &BuildingState)>();
        q.iter(world)
            .map(|(gid, bs)| BuildingEntity {
                id: gid.0,
                position: bs.position,
                is_alive: bs.is_alive,
                is_hidden: bs.is_hidden,
                is_suppressed: bs.is_suppressed,
                team_id: bs.team_id.raw() as i8,
                params_id: bs.params_id,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::world::World;
    use wows_replays::ReplayMeta;
    use wows_replays::analyzer::battle_controller::MetadataPlayer;
    use wows_replays::analyzer::decoder::PlayerStateData;
    use wows_replays::types::GameParamId;
    use wows_replays::types::Relation;
    use wowsunpack::game_params::types::Achievement;
    use wowsunpack::game_params::types::Param;
    use wowsunpack::game_params::types::ParamData;
    use wowsunpack::game_types::Ribbon;
    use wowsunpack::rpc::entitydefs::EntitySpec;

    use super::*;

    /// Stands in for a real `ResourceLoader`: always resolves to the same
    /// fixture `Param` regardless of id, since the test never looks up a real
    /// ship. Mirrors the `NoResources` double in `ingest::entities`'s tests,
    /// except this one must succeed so `Player::from_arena_player` resolves.
    struct StubResources(Rc<Param>);

    impl ResourceLoader for StubResources {
        fn localized_name_from_param(&self, _param: &Param) -> Option<String> {
            None
        }

        fn localized_name_from_id(&self, _id: &TranslationKey) -> Option<String> {
            None
        }

        fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
            Some(self.0.clone())
        }

        fn entity_specs(&self) -> &[EntitySpec] {
            &[]
        }
    }

    fn fixture_param() -> Rc<Param> {
        Rc::new(
            Param::builder()
                .id(GameParamId::from(1u32))
                .index("IDX".to_string())
                .name("Fixture".to_string())
                .nation("USA".to_string())
                .data(ParamData::Achievement(
                    Achievement::builder()
                        .is_group(false)
                        .one_per_battle(false)
                        .ui_type("x".to_string())
                        .ui_name("x".to_string())
                        .build(),
                ))
                .build(),
        )
    }

    fn minimal_meta() -> ReplayMeta {
        ReplayMeta {
            matchGroup: None,
            gameMode: 0,
            gameType: None,
            clientVersionFromExe: "0, 15, 5, 12668706".to_string(),
            scenarioUiCategoryId: None,
            mapDisplayName: String::new(),
            mapId: 0,
            clientVersionFromXml: String::new(),
            weatherParams: None,
            duration: 0,
            gameLogic: None,
            name: String::new(),
            scenario: String::new(),
            playerID: AccountId::from(1u32),
            vehicles: Vec::new(),
            playersPerTeam: 1,
            dateTime: String::new(),
            mapName: String::new(),
            playerName: "self".to_string(),
            scenarioConfigId: 0,
            teamsCount: 2,
            logic: None,
            playerVehicle: String::new(),
            battleDuration: None,
        }
    }

    // Minimal PlayerStateData built via its derived Deserialize impl: its
    // fields are private to wows_replays, so this is the only way to
    // construct one from outside that crate. `raw`/`raw_with_names` are
    // `#[serde(skip_deserializing)]` and default to empty.
    const SELF_PLAYER_JSON: &str = r#"{
        "username": "self",
        "clan": "",
        "clan_id": 0,
        "clan_color": 0,
        "db_id": 1,
        "realm": null,
        "meta_ship_id": 1,
        "entity_id": 3,
        "team_id": 0,
        "max_health": 50000,
        "is_abuser": false,
        "is_hidden": false,
        "is_bot": false,
        "human_properties": null
    }"#;

    /// Builds a `BattleReport` from a synthetic world: a self player resolved
    /// through the same `Player::from_arena_player` path production ingest
    /// uses, plus whatever `seed` pushes directly onto the ECS `World` (e.g.
    /// log entries). No real vfs/GameParams load; `resources` is a single
    /// fixture `Param` returned unconditionally.
    fn report_from_synthetic_world(seed: impl FnOnce(&mut World)) -> BattleReport {
        let meta = minimal_meta();
        let param = fixture_param();
        let resources = StubResources(param.clone());
        let mut world = BattleWorld::new(&meta, &resources, None);

        let player_state: PlayerStateData =
            serde_json::from_str(SELF_PLAYER_JSON).expect("fixture matches PlayerStateData's shape");
        let metadata_player = MetadataPlayer::new(AccountId::from(1u32), "self".to_string(), Relation::new(0), param);
        let player = Player::from_arena_player(&player_state, &metadata_player, &resources)
            .expect("stub resources always resolve a vehicle");
        world.world_mut().resource_mut::<PlayerIndex>().0.insert(player_state.entity_id(), Rc::new(player));

        seed(world.world_mut());

        world.into_report()
    }

    /// The report is what the analysis sees. A log that reaches finish() but not
    /// the report is invisible to every consumer.
    #[test]
    fn report_carries_the_fire_analysis_logs() {
        let report = report_from_synthetic_world(|world| {
            world.resource_mut::<BurnStateLog>().0.push(BurnStateChange {
                victim: EntityId::from(3u32),
                clock: GameClock(12.0),
                previous: 0,
                current: 1,
            });
            world.resource_mut::<RibbonLog>().0.push(RibbonEvent {
                clock: GameClock(12.0),
                ribbon: Ribbon::SetFire,
                count: 1,
            });
        });

        assert_eq!(report.burn_state_changes().len(), 1);
        assert_eq!(report.ribbon_events().len(), 1);
        assert_eq!(report.burn_state_changes()[0].victim, EntityId::from(3u32));
    }
}
