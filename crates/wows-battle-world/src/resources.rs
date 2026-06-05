//! ECS resources shared across systems.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use wows_replays::analyzer::battle_controller::DamageEvent;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::SharedPlayer;
use wows_replays::analyzer::battle_controller::state::CapturedBuff;
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
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::game_types::DamageStatWeapon;
use wowsunpack::game_types::Ribbon;
use wowsunpack::recognized::Recognized as WuRecognized;

// Re-export so users of this module see a consistent Recognized type.
// Both aliases point to the same type; we just need one import in this file.
// wows_replays re-exports wowsunpack::recognized::Recognized as decoder::Recognized.
// Use the decoder path for FinishType and the wowsunpack path for DamageStatWeapon/Category.

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
    /// Raw server battle stage value (1=pre-battle, 0=active, 3=results).
    pub battle_stage: Option<i64>,
    pub battle_start_clock: Option<GameClock>,
    pub battle_end_clock: Option<GameClock>,
    /// Clock when `battleResult` was set on BattleLogic (regulation time ended).
    pub battle_result_clock: Option<GameClock>,
    pub winning_team: Option<i8>,
    pub finish_type: Option<Recognized<FinishType>>,
    /// Seconds remaining, from BattleLogic `timeLeft`.
    pub time_left: Option<i64>,
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

/// Self-player ribbon counts and cumulative damage stats.
#[derive(Resource, Debug, Clone, Default)]
pub struct SelfStats {
    pub ribbons: HashMap<Ribbon, usize>,
    pub damage_stats:
        HashMap<(WuRecognized<DamageStatWeapon>, WuRecognized<DamageStatCategory>), DamageStatEntry>,
}

/// Ordered list of ECS entities for each capture point, by control-point index.
#[derive(Resource, Debug, Clone, Default)]
pub struct CapturePointOrder(pub Vec<Entity>);

/// Maps game entity id to position-index in the capture point / interactive zone list.
#[derive(Resource, Debug, Clone, Default)]
pub struct InteractiveZoneIndex(pub HashMap<EntityId, usize>);

/// Bidirectional map between game entity ids and ECS entities.
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
