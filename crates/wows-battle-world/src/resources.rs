//! ECS resources shared across systems.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use wows_replays::Rc;
use wows_replays::VehicleInfoMeta;
use wows_replays::analyzer::battle_controller::DamageEvent;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::SharedPlayer;
use wows_replays::analyzer::battle_controller::state::CapturedBuff;
use wows_replays::analyzer::battle_controller::state::DeadShip;
use wows_replays::analyzer::battle_controller::state::KillRecord;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::analyzer::battle_controller::state::ScoringRules as ScoringRulesInner;
use wows_replays::analyzer::battle_controller::state::TeamScore;
use wows_replays::analyzer::decoder::DamageStatEntry;
use wows_replays::analyzer::decoder::FinishType;
use wows_replays::analyzer::decoder::Recognized;
use wows_replays::types::ArenaId;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GunId;
use wowsunpack::game_types::BattleStage;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::game_types::DamageStatWeapon;
use wowsunpack::game_types::PlaneId;
use wowsunpack::game_types::Ribbon;

use crate::units::MatchWinner;
use crate::units::SecondsRemaining;

/// Current replay clock.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Clock(pub GameClock);

/// Players parsed from replay metadata.
#[derive(Resource, Debug, Clone, Default)]
pub struct MetadataPlayers(pub Vec<SharedPlayer>);

/// Global match/arena state not owned by any single entity.
#[derive(Resource, Debug, Clone, Default)]
pub struct MatchState {
    pub arena_id: Option<ArenaId>,
    /// Resolved battle stage, updated from BattleLogic `battleStage` EntityProperty.
    pub battle_stage: Option<BattleStage>,
    pub battle_start_clock: Option<GameClock>,
    pub battle_end_clock: Option<GameClock>,
    /// Clock when `battleResult` was set on BattleLogic (regulation time ended).
    pub battle_result_clock: Option<GameClock>,
    pub winning_team: Option<MatchWinner>,
    pub finish_type: Option<Recognized<FinishType>>,
    /// Seconds remaining, from BattleLogic `timeLeft`.
    pub time_left: Option<SecondsRemaining>,
    pub match_finished: bool,
    /// Serialized battle results blob.
    pub battle_results: Option<String>,
}

/// Current team scores.
#[derive(Resource, Debug, Clone, Default)]
pub struct TeamScores(pub Vec<TeamScore>);

/// Scoring rules from BattleLogic (win threshold, hold reward, cap indices).
#[derive(Resource, Debug, Clone, Default)]
pub struct ScoringRules(pub Option<ScoringRulesInner>);

/// Buffs captured by teams during the match (arms race).
#[derive(Resource, Debug, Clone, Default)]
pub struct CapturedBuffs(pub Vec<CapturedBuff>);

/// Ordered chat messages received so far.
#[derive(Resource, Clone, Default)]
pub struct ChatLog(pub Vec<GameMessage>);

/// All ship kill records in arrival order.
#[derive(Resource, Debug, Clone, Default)]
pub struct KillLog(pub Vec<KillRecord>);

/// All damage events per aggressor entity id.
#[derive(Resource, Debug, Clone, Default)]
pub struct DamageLedger(pub HashMap<EntityId, Vec<DamageEvent>>);

/// Resolved projectile hits (shells matched to salvos).
#[derive(Resource, Debug, Clone, Default)]
pub struct ShotHitLog(pub Vec<ResolvedShotHit>);

/// Ordered list of in-flight artillery salvo entities.
///
/// Each `Entity` carries a `Projectile` + `ProjectileState::Artillery`. The order
/// mirrors BattleController.active_shots so salvo matching and the resulting
/// shot_hits ordering stay byte-identical to the original. ECS archetype iteration
/// order is not relied upon; this Vec is the authoritative sequence.
#[derive(Resource, Debug, Clone, Default)]
pub struct ActiveShotOrder(pub Vec<Entity>);

/// Ordered list of in-flight torpedo entities.
///
/// Mirrors BattleController.active_torpedoes, including swap_remove on hit so that
/// later index-based lookups resolve to the same element the original would find.
#[derive(Resource, Debug, Clone, Default)]
pub struct ActiveTorpedoOrder(pub Vec<Entity>);

/// A single in-flight secondary (ATBA) shot: shooter, target, fire time, and the
/// representative firing gun (lowest gunID hitting this target this clock).
///
/// Plain data, not an entity: these records carry no other state, are never
/// mutated per-frame, and are never queried. Position and ammo are resolved by
/// the renderer from live entity positions and the shooter's ship params; `gun`
/// lets it pick that gun's shell for color and pacing on mixed-caliber ships.
#[derive(Debug, Clone, Copy)]
pub struct SecondaryShot {
    pub shooter: EntityId,
    pub target: EntityId,
    pub fired_at: GameClock,
    pub gun: GunId,
}

/// In-flight secondary (ATBA) shots in fire order.
///
/// Independent of ActiveShotOrder/ActiveTorpedoOrder: a visualization-only
/// stream with no salvo matching or hit resolution.
#[derive(Resource, Debug, Clone, Default)]
pub struct ActiveSecondaryShots(pub Vec<SecondaryShot>);

/// Self-player ribbon counts and cumulative damage stats.
#[derive(Resource, Debug, Clone, Default)]
pub struct SelfStats {
    pub ribbons: HashMap<Ribbon, usize>,
    pub damage_stats: HashMap<(Recognized<DamageStatWeapon>, Recognized<DamageStatCategory>), DamageStatEntry>,
}

/// Ordered list of ECS entities for each capture point, by control-point index.
#[derive(Resource, Debug, Clone, Default)]
pub struct CapturePointOrder(pub Vec<Entity>);

/// Typed reference stored per interactive-zone entity in InteractiveZoneIndex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveZoneRef {
    CapturePoint(usize),
    BuffZone,
}

/// Maps game entity id to its interactive zone role.
#[derive(Resource, Debug, Clone, Default)]
pub struct InteractiveZoneIndex(pub HashMap<EntityId, InteractiveZoneRef>);

/// Ordered list of ECS entities for each weather zone, in creation order.
///
/// Mirrors the original's Vec<LocalWeatherZone> push/drain semantics so that
/// array indices are stable even when bevy reuses Entity indices after despawn.
#[derive(Resource, Debug, Clone, Default)]
pub struct WeatherZoneOrder(pub Vec<Entity>);

/// Pre-arrival mapping: InteractiveZone entity id -> drop GameParamId from state.drop.data.
///
/// Populated when a state.drop.data PropertyUpdate arrives before the buff zone entity exists.
/// Drained into BuffZoneData.drop_params_id on InteractiveZone create.
#[derive(Resource, Debug, Clone, Default)]
pub struct PendingDropParams(pub HashMap<EntityId, wows_replays::types::GameParamId>);

/// Maps game EntityId -> ECS Entity. The reverse lookup is available via the `GameId` component.
#[derive(Resource, Debug, Clone, Default)]
pub struct EntityIndex(HashMap<EntityId, Entity>);

impl EntityIndex {
    pub fn get(&self, id: EntityId) -> Option<Entity> {
        self.0.get(&id).copied()
    }

    pub fn insert(&mut self, id: EntityId, entity: Entity) {
        self.0.insert(id, entity);
    }

    pub fn remove(&mut self, id: EntityId) -> Option<Entity> {
        self.0.remove(&id)
    }
}

/// Maps PlaneId -> ECS Entity for active plane squadrons.
///
/// Planes are addressed by PlaneId, not EntityId, so EntityIndex cannot reach them.
#[derive(Resource, Debug, Clone, Default)]
pub struct PlaneIndex(HashMap<PlaneId, Entity>);

impl PlaneIndex {
    pub fn get(&self, id: PlaneId) -> Option<Entity> {
        self.0.get(&id).copied()
    }

    pub fn insert(&mut self, id: PlaneId, entity: Entity) {
        self.0.insert(id, entity);
    }

    pub fn remove(&mut self, id: PlaneId) -> Option<Entity> {
        self.0.remove(&id)
    }
}

/// Maps PlaneId -> ECS Entity for active fighter patrol wards.
///
/// Wards are addressed by PlaneId, not EntityId, so EntityIndex cannot reach them.
#[derive(Resource, Debug, Clone, Default)]
pub struct WardIndex(HashMap<PlaneId, Entity>);

impl WardIndex {
    pub fn get(&self, id: PlaneId) -> Option<Entity> {
        self.0.get(&id).copied()
    }

    pub fn insert(&mut self, id: PlaneId, entity: Entity) {
        self.0.insert(id, entity);
    }

    pub fn remove(&mut self, id: PlaneId) -> Option<Entity> {
        self.0.remove(&id)
    }
}

/// Dead ships tracked across the match, keyed by EntityId.
///
/// Mirrors BattleController.dead_ships. Vehicles persist after death and remain
/// queryable; this resource records their last known state at time of death.
#[derive(Resource, Debug, Clone, Default)]
pub struct DeadShips(pub HashMap<EntityId, DeadShip>);

/// Maps entity id to the Player built from the arena roster.
///
/// Mirrors BattleController.player_entities. Populated on OnArenaStateReceived
/// and NewPlayerSpawnedInBattle; empty until the first roster packet arrives.
#[derive(Resource, Clone, Default)]
pub struct PlayerIndex(pub HashMap<EntityId, Rc<Player>>);

/// Replay metadata vehicle list, used as the fallback sender-resolution path
/// for chat messages sent in the PLAYER_ID era when the sender is not yet in
/// PlayerIndex.
#[derive(Resource, Clone, Default)]
pub struct ReplayVehicles(pub Vec<VehicleInfoMeta>);
