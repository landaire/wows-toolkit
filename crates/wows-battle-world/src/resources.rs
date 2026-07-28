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

/// Log of burn-bit transitions observed on any vehicle.
///
/// Three ingest paths can move a vehicle's `burningFlags`, and all three are
/// diffed into this log: `handle_vehicle_property` for `EntityProperty`
/// updates, `apply_player_create_props` for the `BasePlayerCreate`/
/// `CellPlayerCreate` fold on the self ship, and `handle_vehicle_create`,
/// which is the one path that replaces `VehicleState` wholesale. That create
/// path diffs the incoming mask against whatever the vehicle last held (zero
/// when it held nothing) and pushes the resulting baseline before opening the
/// vehicle's presence window. `seed_vehicles_from_arena_state` needs no such
/// treatment: it skips any entity that already exists, and the ships it does
/// create get no presence window at all, so nothing certifies a range over
/// them.
///
/// The invariant that buys: **this log is complete over any range
/// `PresenceLog::continuously_observed` accepts.** What stays unknown across
/// an unobserved gap is *when* a section was lit, not whether it was burning
/// at a given clock: a window's baseline pins the mask at the moment the
/// window opens, and every later change inside the window is logged. The
/// analysis asks "was this section burning when the shell landed", so a
/// baseline at window-open is sufficient. A consumer that reconstructs a mask
/// over a range `continuously_observed` rejects gets no such guarantee.
///
/// Entries are pushed in packet order, so `clock` is non-decreasing, not
/// strictly increasing: multiple entries (same vehicle or different ones) can
/// share a clock value, since several packets can land on one tick. A
/// time-keyed lookup over this log must tolerate duplicate clocks rather than
/// assume a unique match. `clock` is the raw `packet.clock` seen by
/// `ingest::dispatch`, not the `Clock` resource: `Clock` additionally refuses
/// to rewind to zero once it has advanced past zero (see `world.rs`'s
/// `Analyzer::process`), so the two can disagree on pre-battle packets.
#[derive(Resource, Debug, Clone, Default)]
pub struct BurnStateLog(pub Vec<BurnStateChange>);

/// A window during which the recording client received updates for a
/// vehicle, from the `EntityCreate` that proved it was observed to the
/// matching `EntityLeave`.
///
/// `left: None` means the window is still open as of the last packet
/// processed, either because the vehicle is still present or because the
/// match ended (or the parse stopped) before an `EntityLeave` arrived for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresenceWindow {
    pub entered: GameClock,
    pub left: Option<GameClock>,
}

/// Windows during which the recording client observed each vehicle, keyed by
/// game `EntityId`.
///
/// A window opens in `handle_vehicle_create` (the `EntityCreate` handler for
/// `EntityType::Vehicle`) and closes in `handle_entity_leave`; opening is
/// idempotent, so a vehicle that already holds an open window gets no second
/// one. Carrying the `Vehicle` marker component is necessary but not
/// sufficient for an entry here: smoke screens, buff zones, capture points,
/// and buildings never get the marker and so never appear in this map, but
/// `seed_vehicles_from_arena_state` also attaches `Vehicle` to ships it only
/// pre-creates from the match roster, and that path deliberately never opens
/// a window (see below). So an entry, when one exists, is proof of
/// observation: it exists only where the client's own `EntityCreate` for
/// that vehicle was actually seen. `OnArenaStateReceived` lists every match
/// participant, including ships the client's AOI never detects at all;
/// opening a window there would mark a never-observed ship as present for
/// the whole match, which is exactly backwards. A vehicle with no windows
/// was never observed, not merely untracked.
///
/// Entries within one vehicle's `Vec` are pushed in the order the client
/// observed them, so `entered` is non-decreasing across the `Vec`, not
/// strictly increasing: two `EntityCreate` packets for different vehicles can
/// land on the same clock. As with `BurnStateLog`, `clock` in both `entered`
/// and `left` is the raw `packet.clock` seen by `ingest::dispatch`, not the
/// `Clock` resource, which can disagree with it on pre-battle packets.
///
/// Coverage boundary: a gap between two windows for one vehicle is a real,
/// provable blind spot, since an `EntityLeave` was actually observed for it.
/// The converse is not guaranteed. If an `EntityLeave` for a vehicle is
/// silently dropped, or the vehicle's underlying game entity id is reused
/// without one, the window-open call sees an already-open window and does
/// nothing (that is what idempotency means), so the log reports
/// uninterrupted presence straight across a gap it has no way to detect.
/// `PresenceLog` does not resolve that case, it only makes the gaps that were
/// actually observed (a real `EntityLeave` followed by a later create)
/// usable by `continuously_observed`. Separately, `DecodedPacketPayload::EntityEnter`
/// is currently a no-op in `ingest::dispatch`: if the server ever signals a
/// vehicle's AOI re-entry with `EntityEnter` rather than a fresh
/// `EntityCreate`, no window reopens and presence stays closed for the rest
/// of the match. That direction only loses samples rather than corrupting
/// one, since a closed window can only make `continuously_observed` too
/// strict, never too lenient.
#[derive(Resource, Debug, Clone, Default)]
pub struct PresenceLog(pub HashMap<EntityId, Vec<PresenceWindow>>);

impl PresenceLog {
    /// True when `[from, to]` lies inside a single unbroken window: some
    /// window's `entered` is at or before `from`, and that same window's
    /// `left` is either still open (`None`) or at or after `to`. Both
    /// comparisons are inclusive: a window opens at its `entered` clock and
    /// closes at its `left` clock, so a query touching either boundary is
    /// still fully inside it. An entity with no recorded windows was never
    /// observed and is never continuously observed, for any range.
    ///
    /// Callers must pass `from <= to`; an inverted range is checked only in
    /// debug builds (`debug_assert!`) because it makes both comparisons
    /// easier to satisfy, silently turning "not observed" into "observed".
    pub fn continuously_observed(&self, entity: EntityId, from: GameClock, to: GameClock) -> bool {
        debug_assert!(from <= to, "continuously_observed requires from <= to, got {from:?}..{to:?}");
        self.0
            .get(&entity)
            .is_some_and(|windows| windows.iter().any(|w| w.entered <= from && w.left.is_none_or(|left| left >= to)))
    }
}

/// Every resolved hit for the whole match, in arrival order.
///
/// Unlike `ShotHitLog`, which `Analyzer::process` clears at the start of
/// every packet in `Tracked` mode so renderers see only the current frame's
/// hits, this log is never cleared and accumulates for the whole parse.
///
/// Populated only when `IngestOptions::record_hit_history` is set; it
/// defaults to `false`, so an empty (or short) log does not mean few hits
/// occurred, it may mean history recording was never turned on for this
/// parse. It is also empty under `ShotTracking::Untracked`, same as
/// `ShotHitLog`: no `ResolvedShotHit` is constructed at all in that mode.
/// Entries share `ShotHitLog`'s clock semantics: `clock` is the raw
/// `packet.clock`, non-decreasing but not strictly increasing, since every
/// hit in one `ShotKills` packet shares a clock.
#[derive(Resource, Debug, Clone, Default)]
pub struct HitHistoryLog(pub Vec<ResolvedShotHit>);

/// One observed increment of a self-player ribbon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RibbonEvent {
    pub clock: GameClock,
    pub ribbon: Ribbon,
    /// How many were earned at this clock. Legacy `onRibbon` calls always
    /// contribute 1; modern replays deliver absolute running totals per
    /// ribbon, so this is the delta against the previous total.
    pub count: usize,
}

/// Every self-player ribbon increment observed, in the order it was earned.
///
/// Fed by two intake paths with different shapes: legacy `onRibbon` pushes
/// one event per call with `count: 1`; modern `privateVehicleState.ribbons`
/// nested updates carry absolute running totals, so an event there is the
/// delta against the previous total for that ribbon, and an unchanged total
/// (the server re-sending an array slot) logs nothing.
///
/// Entries are pushed in packet order, so `clock` is non-decreasing, not
/// strictly increasing, the same as [`BurnStateLog`]: several events can
/// share a clock value, and a time-keyed lookup must tolerate duplicates
/// rather than assume a unique match. `clock` is the raw `packet.clock` seen
/// by `ingest::dispatch`, not the `Clock` resource. When one modern update
/// raises more than one ribbon's total at once, the events it produces share
/// that clock and are ordered by `Ribbon`'s declaration order (its derived
/// `Ord`), not by any real-world sub-tick ordering, since the update carries
/// no such information; this keeps the log reproducible rather than
/// dependent on `HashMap` iteration order.
///
/// Only self-player ribbons are covered: `onRibbon` and `privateVehicleState`
/// are both scoped to the recording player's own avatar, so there is no path
/// to observe another player's ribbons. Within that scope, two paths can
/// still earn a ribbon without producing a log entry:
/// - `handle_ribbon_property_update` matches only `SetRange`, `SetElement`,
///   and array-index `SetKey { key: "count" }`; any other update shape to the
///   `ribbons` array is ignored, so a total that moves through an unmatched
///   shape produces neither a `RibbonEvent` nor a `SelfStats.ribbons` update.
/// - A slot's total can only fall by being overwritten (`SetRange`/
///   `SetElement`) with a lower count or a different `ribbonId`, never by an
///   explicit decrement message. A fall is not logged and rebases that
///   ribbon's baseline downward, so a later rise from the new, lower baseline
///   under-reports the true increment if the two changes belong to different
///   ribbons that shared a slot.
#[derive(Resource, Debug, Clone, Default)]
pub struct RibbonLog(pub Vec<RibbonEvent>);
