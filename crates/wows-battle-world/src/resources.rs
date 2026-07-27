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
use wows_replays::types::GameParamId;
use wows_replays::types::WorldPos;
use wows_replays::types::WorldPos2D;
use wowsunpack::game_types::BattleStage;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::game_types::DamageStatWeapon;
use wowsunpack::game_types::PlaneId;
use wowsunpack::game_types::Ribbon;
use wowsunpack::models::fire_nodes::BurnNodeIndex;

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

/// Self-player ribbon counts and cumulative damage stats.
#[derive(Resource, Debug, Clone, Default)]
pub struct SelfStats {
    pub ribbons: HashMap<Ribbon, usize>,
    /// Raw mirror of the avatar's `privateVehicleState.ribbons` array, indexed by
    /// array position as `(ribbon_id, count)`. Modern replays deliver ribbons as
    /// nested updates into this array (add element, then bump its `count`), so we
    /// keep the positional slots to resolve `ArrayIndex` count updates, then
    /// rebuild `ribbons` from it. Empty for legacy (`onRibbon`) replays.
    pub ribbon_slots: Vec<(i32, usize)>,
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

/// Submarine hydrophone state observed from the recording player's client.
///
/// The hydrophone is a separate channel from `Vehicle.visibilityFlags`: a
/// hydrophone contact never enters the client as a Vehicle entity and never
/// sets a vision flag, so it is invisible to detection-flag analysis.
#[derive(Resource, Debug, Clone, Default)]
pub struct Hydrophone {
    /// Whether the recording player's ship is currently held by an enemy
    /// submarine's hydrophone. `None` until the first report.
    pub detected: Option<bool>,
    /// Every transition of `detected`, in clock order.
    pub detection_changes: Vec<HydrophoneDetectionChange>,
    /// Contacts currently held, keyed by holding submarine and contact. A
    /// merged multi-perspective session can observe more than one holder.
    pub contacts: HashMap<HydrophoneContactKey, HydrophoneContact>,
}

impl Hydrophone {
    /// Drop contacts whose lifetime has elapsed by `clock`. Contacts with no
    /// lifetime are held until an explicit clear.
    pub fn expire(&mut self, clock: GameClock) {
        self.contacts.retain(|_, c| c.expires_at.is_none_or(|expiry| expiry > clock));
    }
}

/// Identifies a contact by the submarine holding it and the ship being held.
/// The zone channel reports no holder, so `holder` is `None` there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HydrophoneContactKey {
    pub holder: Option<EntityId>,
    pub target: EntityId,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HydrophoneDetectionChange {
    pub clock: GameClock,
    pub detected: bool,
}

/// A contact held by a submarine's hydrophone. Zone contacts carry only a
/// coarse minimap position; submarine contacts carry a full pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HydrophoneContact {
    pub position: HydrophoneContactPosition,
    /// When the contact lapses. `None` when the reporting RPC carried no
    /// lifetime, in which case only an explicit clear drops it.
    pub expires_at: Option<GameClock>,
    pub last_updated: GameClock,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HydrophoneContactPosition {
    /// From the zone channel: a coarse minimap position. `zone_id` is absent on
    /// the team-shared `SURFACE_BROADCAST_ZONE_INFO` variant.
    Zone { zone_id: Option<u8>, position: WorldPos2D, broadcast: bool },
    /// From `SUBMARINE_HYDROPHONE_TARGET_INFO`: full pose and ship identity.
    Pose { params_id: GameParamId, position: WorldPos, yaw: f32, pitch: f32 },
}

/// `burningFlags` bits 0-3 (ma779114d BURN_MASK). Bits 4-7 are floods, 8 acid,
/// 9 wild fire.
pub const BURN_MASK: u16 = 0x000F;

/// One change to a vehicle's burn-node bitmask.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BurnStateChange {
    pub victim: EntityId,
    pub clock: GameClock,
    /// burningFlags & BURN_MASK before the change.
    pub previous: u16,
    /// burningFlags & BURN_MASK after the change.
    pub current: u16,
}

impl BurnStateChange {
    /// Nodes that went from clear to burning in this change.
    pub fn newly_lit(&self) -> impl Iterator<Item = BurnNodeIndex> + '_ {
        let rising = self.current & !self.previous;
        (0..BurnNodeIndex::MAX_NODES).filter_map(move |i| {
            let index = BurnNodeIndex::new(i)?;
            (rising & index.bit_mask() != 0).then_some(index)
        })
    }
}

/// Ordered log of every burn-bit transition observed on any vehicle.
#[derive(Resource, Debug, Clone, Default)]
pub struct BurnStateLog(pub Vec<BurnStateChange>);
