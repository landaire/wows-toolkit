use std::collections::HashMap;

use wowsunpack::game_types::BattleStage;
use wowsunpack::game_types::BattleType;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::game_types::DamageStatWeapon;
use wowsunpack::game_types::Ribbon;
use wowsunpack::recognized::Recognized;

use crate::Rc;
use crate::analyzer::decoder::DamageStatEntry;
use crate::analyzer::decoder::FinishType;
use crate::types::ElapsedClock;
use crate::types::EntityId;
use crate::types::GameClock;
use crate::types::GameParamId;
use crate::types::PlaneId;

use super::controller::Entity;
use super::controller::GameMessage;
use super::controller::Player;
use super::controller::SharedPlayer;
use super::state::ActiveConsumable;
use super::state::ActivePlane;
use super::state::ActiveShot;
use super::state::ActiveTorpedo;
use super::state::ActiveWard;
use super::state::BuffZoneState;
use super::state::CapturePointState;
use super::state::CapturedBuff;
use super::state::DeadShip;
use super::state::KillRecord;
use super::state::LocalWeatherZone;
use super::state::MinimapPosition;
use super::state::ResolvedShotHit;
use super::state::ScoringRules;
use super::state::ShipPosition;
use super::state::TeamScore;

/// Readonly view into BattleController state.
///
/// This trait hides the `G: ResourceLoader` generic on BattleController,
/// allowing callers to read state without being generic themselves.
pub trait BattleControllerState {
    /// Current replay clock time
    fn clock(&self) -> GameClock;

    /// Latest world-space position per ship entity
    fn ship_positions(&self) -> &HashMap<EntityId, ShipPosition>;

    /// Latest minimap position per entity
    fn minimap_positions(&self) -> &HashMap<EntityId, MinimapPosition>;

    /// Players parsed from arena state (entity_id -> Player)
    fn player_entities(&self) -> &HashMap<EntityId, Rc<Player>>;

    /// Players parsed from replay metadata
    fn metadata_players(&self) -> &[SharedPlayer];

    /// All tracked entities (vehicles, buildings, smoke screens)
    fn entities_by_id(&self) -> &HashMap<EntityId, Entity>;

    /// Current capture point states
    fn capture_points(&self) -> &[CapturePointState];

    /// Current buff zone states (arms race powerup zones)
    fn buff_zones(&self) -> &HashMap<EntityId, BuffZoneState>;

    /// Active local weather zones (squalls/storms) on the map
    fn local_weather_zones(&self) -> &[LocalWeatherZone];

    /// Buffs captured so far (arms race)
    fn captured_buffs(&self) -> &[CapturedBuff];

    /// Current team scores
    fn team_scores(&self) -> &[TeamScore];

    /// Chat messages received so far
    fn game_chat(&self) -> &[GameMessage];

    /// Active consumables per entity
    fn active_consumables(&self) -> &HashMap<EntityId, Vec<ActiveConsumable>>;

    /// Active artillery salvos in flight
    fn active_shots(&self) -> &[ActiveShot];

    /// Active torpedoes in the water
    fn active_torpedoes(&self) -> &[ActiveTorpedo];

    /// Resolved projectile hits (shells/torpedoes that impacted a target).
    /// Each hit is matched to its originating salvo when possible.
    fn shot_hits(&self) -> &[ResolvedShotHit];

    /// Active plane squadrons on the minimap
    fn active_planes(&self) -> &HashMap<PlaneId, ActivePlane>;

    /// Active fighter patrol wards (stationary patrol circles)
    fn active_wards(&self) -> &HashMap<PlaneId, ActiveWard>;

    /// All ship kills that have occurred
    fn kills(&self) -> &[KillRecord];

    /// Dead ships and their last known positions
    fn dead_ships(&self) -> &HashMap<EntityId, DeadShip>;

    /// Clock time when the battle ended, if it has ended
    fn battle_end_clock(&self) -> Option<GameClock>;

    /// Which team won the match (0 or 1), or negative for draw. None if match hasn't ended.
    fn winning_team(&self) -> Option<i8>;

    /// How the battle ended (extermination, score, timeout, etc.). None if not yet decided.
    fn finish_type(&self) -> Option<&Recognized<FinishType>>;

    /// Main battery turret yaws per entity (group 0 only).
    /// Each entry maps entity_id -> vec of turret yaws in radians (relative to ship heading).
    fn turret_yaws(&self) -> &HashMap<EntityId, Vec<f32>>;

    /// World-space gun aim yaw per entity, decoded from `targetLocalPos` EntityProperty.
    /// Updated frequently (~6000 times per match). Values are radians in [-PI, PI].
    fn target_yaws(&self) -> &HashMap<EntityId, f32>;

    /// Currently selected ammo per entity. Maps entity_id -> ammo_param_id.
    /// Only tracked for artillery (weapon_type 0).
    fn selected_ammo(&self) -> &HashMap<EntityId, GameParamId>;

    /// The battle type (Random, Ranked, Clan, Co-op, etc.)
    fn battle_type(&self) -> Recognized<BattleType>;

    /// Scoring rules parsed from BattleLogic (win score, hold reward/period, cap indices).
    /// None before the BattleLogic EntityCreate packet is processed.
    fn scoring_rules(&self) -> Option<&ScoringRules>;

    /// Seconds remaining in the match, updated from BattleLogic `timeLeft` EntityProperty.
    /// None before the first timeLeft update.
    fn time_left(&self) -> Option<i64>;

    /// Current battle stage: Waiting (pre-battle countdown), Battle (active), Ended, etc.
    /// None before the first battleStage EntityProperty update.
    fn battle_stage(&self) -> Option<BattleStage>;

    /// Clock time when the battle stage first transitioned to Battle.
    /// Backends can compute elapsed battle time as `clock - battle_start_clock`.
    /// None if the battle hasn't started yet.
    fn battle_start_clock(&self) -> Option<GameClock>;

    /// Convert an absolute game clock to elapsed time since battle start.
    /// If battle start is unknown, treats clock 0.0 as battle start.
    fn game_clock_to_elapsed(&self, clock: GameClock) -> ElapsedClock;

    /// Convert elapsed time since battle start back to an absolute game clock value.
    /// If battle start is unknown, treats clock 0.0 as battle start.
    fn elapsed_to_game_clock(&self, elapsed: ElapsedClock) -> GameClock;

    /// Ribbon counts for the self (recording) player from live `onRibbon` packets.
    fn self_ribbons(&self) -> &HashMap<Ribbon, usize>;

    /// Cumulative damage stats for the self player from `receiveDamageStat` packets.
    fn self_damage_stats(
        &self,
    ) -> &HashMap<(Recognized<DamageStatWeapon>, Recognized<DamageStatCategory>), DamageStatEntry>;
}
