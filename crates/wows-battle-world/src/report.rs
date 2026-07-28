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
use crate::resources::BurnFlagsObserved;
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
    burn_state_observed: bool,
    ribbon_events: Vec<RibbonEvent>,
    presence: PresenceLog,
    hit_history: Vec<ResolvedShotHit>,
    deaths: HashMap<EntityId, GameClock>,
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

    /// Burn-bit transitions observed on any vehicle, including the baseline
    /// pushed when a vehicle is created (or re-created on AOI re-entry) while
    /// already alight. See `BurnStateLog`'s doc comment for the invariant this
    /// buys: the log is complete over any range `presence()` accepts, so bound
    /// every reconstruction with `continuously_observed` rather than reading
    /// the log unguarded.
    pub fn burn_state_changes(&self) -> &[BurnStateChange] {
        &self.burn_state_changes
    }

    /// Whether the `burningFlags` property was replicated at all during this
    /// parse, which is what separates "nothing ever burned" from "this build
    /// never told us". False with a non-empty [`Self::burn_state_changes`] is
    /// impossible; false with an empty one means every burn-derived count is
    /// unknown rather than zero, and a consumer must report it as such.
    ///
    /// See `BurnFlagsObserved` for how the signal is collected.
    pub fn burn_state_observed(&self) -> bool {
        self.burn_state_observed
    }

    /// Self-player ribbon increments with clocks. Self-player only; see
    /// `RibbonLog`'s doc comment for the two documented gaps: some update
    /// shapes produce no event at all, and a falling total silently rebases
    /// a slot's baseline downward, so a later rise measured from that lower
    /// baseline can under-report the true increment.
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
    ///
    /// `victim_entity_id` is a heuristic, not a decoded field. It is the ship
    /// whose last known position is nearest in XZ to the mean target point of
    /// the salvo the shell was matched to, which is renderer-grade resolution:
    /// good enough to draw a splash marker, not good enough to key state by.
    /// When no salvo matched, `salvo` is `None` and `victim_entity_id` falls
    /// back to the self ship, which is wrong for any hit that was not actually
    /// on the self ship. Salvos expire 30 seconds after they were fired, so
    /// that fallback also captures every long-flight shell (battleship fire at
    /// maximum range) whose salvo aged out before the hit arrived. A consumer
    /// keying burn state or presence by `victim_entity_id` must filter on
    /// `salvo.is_some()`; even then the nearest-ship match can pick a
    /// neighbour in a tight formation.
    pub fn hit_history(&self) -> &[ResolvedShotHit] {
        &self.hit_history
    }

    /// When each vehicle died, keyed by victim entity id, from `KillLog`.
    ///
    /// The clock is the `ShipDestroyed` packet's own clock, not a duration
    /// reconstructed from `DeathInfo::time_lived`, so it is directly comparable
    /// to every other clock in this report.
    ///
    /// **Absence is not proof of survival.** The log only holds deaths this
    /// replay actually observed, so a match whose recording stopped early (the
    /// player left, or the packet stream is truncated) is missing every death
    /// after that point. A consumer that reads absence as "survived" must first
    /// establish that the recording ran to the end of the match;
    /// [`Self::battle_result`] being `Some` is that evidence, since it is only
    /// set once the match was observed to finish.
    ///
    /// Only the first death per victim is kept, in packet order, so a scenario
    /// with respawns reports when the vehicle first died.
    pub fn deaths_by_victim(&self) -> &HashMap<EntityId, GameClock> {
        &self.deaths
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
        // The clock beside it, kept because `DeathInfo` carries only a duration
        // measured from the battle start constant, which is not comparable to
        // the packet clocks every other log in this report is keyed by.
        let mut death_clock_by_victim: HashMap<EntityId, GameClock> = HashMap::new();
        for kill in &self.world().resource::<KillLog>().0 {
            frags_by_killer.entry(kill.killer).or_default().push(DeathInfo::from(kill));
            death_by_victim.entry(kill.victim).or_insert_with(|| DeathInfo::from(kill));
            death_clock_by_victim.entry(kill.victim).or_insert(kill.clock);
        }

        let parsed_battle_results = self
            .world()
            .resource::<MatchState>()
            .battle_results
            .as_ref()
            .and_then(|results| serde_json::Value::from_str(results.as_str()).ok());

        // Sorted because PlayerIndex is a HashMap: its iteration order varies per
        // process, which would otherwise reach every consumer of `players()` and
        // any snapshot taken of one. Entity id breaks ties so bots sharing an
        // account id of 0 keep a stable order too.
        let mut player_entities: Vec<Rc<Player>> =
            self.world().resource::<PlayerIndex>().0.values().cloned().collect();
        player_entities.sort_by_key(|player| (player.initial_state().db_id(), player.initial_state().entity_id()));

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
        let burn_state_observed = self.world().resource::<BurnFlagsObserved>().0;
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
            burn_state_observed,
            ribbon_events,
            presence,
            hit_history,
            deaths: death_clock_by_victim,
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
    use wows_replays::analyzer::battle_controller::state::KillRecord;
    use wows_replays::analyzer::decoder::HitType;
    use wows_replays::analyzer::decoder::ShotHit;
    use wows_replays::types::ShotId;
    use wows_replays::types::WorldPos;
    use wowsunpack::game_types::Ribbon;

    use super::*;
    use crate::resources::PresenceWindow;
    use crate::test_support::StubResources;
    use crate::test_support::fixture_param;
    use crate::test_support::minimal_meta;
    use crate::test_support::self_player;

    /// Builds a `BattleReport` from a synthetic world: a self player resolved
    /// through the same `Player::from_arena_player` path production ingest
    /// uses, plus whatever `seed` pushes directly onto the ECS `World` (e.g.
    /// log entries). No real vfs/GameParams load; `resources` is a single
    /// fixture `Param` returned unconditionally.
    fn report_from_synthetic_world(seed: impl FnOnce(&mut World)) -> BattleReport {
        let meta = minimal_meta();
        let resources = StubResources(fixture_param());
        let mut world = BattleWorld::new(&meta, &resources, None);

        let (entity_id, player) = self_player(&resources);
        world.world_mut().resource_mut::<PlayerIndex>().0.insert(entity_id, Rc::new(player));

        seed(world.world_mut());

        world.into_report()
    }

    /// A fixture `ResolvedShotHit` with all optional/unmatched fields left
    /// empty; only `clock` and the two entity ids are asserted on.
    fn fixture_shot_hit() -> ResolvedShotHit {
        let victim = EntityId::from(3u32);
        ResolvedShotHit {
            clock: GameClock(12.0),
            hit: ShotHit {
                owner_id: victim,
                hit_type: HitType {
                    collision: Recognized::Unknown("0".to_string()),
                    shell_hit: Recognized::Unknown("0".to_string()),
                    raw: 0,
                },
                shot_id: ShotId::from(1u32),
                position: WorldPos::new(0.0, 0.0, 0.0),
                terminal_ballistics: None,
            },
            victim_entity_id: victim,
            salvo: None,
            fired_at: None,
            victim_pose: None,
        }
    }

    /// The report is what the analysis sees. A log that reaches finish() but not
    /// the report is invisible to every consumer. Covers all four logs this
    /// task carries: burn state and ribbons via their `Vec` resources,
    /// presence via its window map, and hit history via its own log, so a
    /// regression dropping any one `mem::take` line or struct-literal field
    /// fails a test rather than passing silently.
    #[test]
    fn report_carries_the_fire_analysis_logs() {
        let victim = EntityId::from(3u32);
        let report = report_from_synthetic_world(|world| {
            world.resource_mut::<BurnStateLog>().0.push(BurnStateChange {
                victim,
                clock: GameClock(12.0),
                previous: 0,
                current: 1,
            });
            world.resource_mut::<RibbonLog>().0.push(RibbonEvent {
                clock: GameClock(12.0),
                ribbon: Ribbon::SetFire,
                count: 1,
            });
            let mut presence = world.resource_mut::<PresenceLog>();
            presence.windows.entry(victim).or_default().push(PresenceWindow { entered: GameClock(0.0), left: None });
            presence.note_seen(victim, GameClock(20.0));
            world.resource_mut::<HitHistoryLog>().0.push(fixture_shot_hit());
        });

        assert_eq!(report.burn_state_changes().len(), 1);
        assert_eq!(report.ribbon_events().len(), 1);
        assert_eq!(report.burn_state_changes()[0].victim, victim);
        assert!(report.presence().continuously_observed(victim, GameClock(0.0), GameClock(20.0)));
        assert_eq!(report.hit_history().len(), 1);
        assert_eq!(report.hit_history()[0].victim_entity_id, victim);
    }

    /// The fire analysis excludes hits landing at or after a victim's death, so
    /// it needs the death clock on the same scale as every other log. Only the
    /// first record per victim counts, so a scenario respawn cannot move the
    /// clock later and re-admit post-death hits.
    #[test]
    fn deaths_are_reported_by_victim_at_the_first_kill_clock() {
        let victim = EntityId::from(9u32);
        let killer = EntityId::from(3u32);
        let report = report_from_synthetic_world(|world| {
            let log = &mut world.resource_mut::<KillLog>().0;
            log.push(KillRecord {
                clock: GameClock(120.0),
                killer,
                victim,
                cause: Recognized::Unknown("0".to_string()),
            });
            log.push(KillRecord {
                clock: GameClock(300.0),
                killer,
                victim,
                cause: Recognized::Unknown("0".to_string()),
            });
        });

        assert_eq!(report.deaths_by_victim().get(&victim), Some(&GameClock(120.0)));
        assert_eq!(report.deaths_by_victim().get(&EntityId::from(11u32)), None);
    }
}
