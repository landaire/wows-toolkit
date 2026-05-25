//! Cross-replay state merging.
//!
//! [`MergedBattleController`] presents a unified [`BattleControllerState`] view
//! that combines several underlying controllers, each driven by a different
//! player's replay of the same match. The primary controller defines the
//! perspective (relation tags, self-stats); secondary controllers fill in
//! whatever the primary's client could not see.

use std::collections::HashMap;
use std::collections::HashSet;

use wowsunpack::game_types::BattleStage;
use wowsunpack::game_types::BattleType;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::game_types::DamageStatWeapon;
use wowsunpack::game_types::Ribbon;
use wowsunpack::recognized::Recognized;

use crate::Rc;
use crate::analyzer::decoder::DamageStatEntry;
use crate::analyzer::decoder::FinishType;
use crate::types::AvatarId;
use crate::types::ElapsedClock;
use crate::types::EntityId;
use crate::types::GameClock;
use crate::types::GameParamId;
use crate::types::PlaneId;
use crate::types::ShotId;

use super::controller::Entity;
use super::controller::GameMessage;
use super::controller::Player;
use super::controller::SharedPlayer;
use super::listener::BattleControllerState;
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

/// Equality between two events that may have arrived through different
/// perspectives. Implementations deliberately ignore fields that the recording
/// client itself fills in (timestamps, the receiving entity, etc.) and compare
/// only the parts that uniquely identify the underlying server event.
pub trait PerspectiveEq {
    fn perspective_eq(&self, other: &Self) -> bool;
}

impl PerspectiveEq for KillRecord {
    fn perspective_eq(&self, other: &Self) -> bool {
        self.killer == other.killer && self.victim == other.victim && self.cause == other.cause
    }
}

impl PerspectiveEq for GameMessage {
    fn perspective_eq(&self, other: &Self) -> bool {
        self.entity_id == other.entity_id && self.channel == other.channel && self.message == other.message
    }
}

impl PerspectiveEq for CapturedBuff {
    fn perspective_eq(&self, other: &Self) -> bool {
        self.params_id == other.params_id && self.team_id == other.team_id
    }
}

impl PerspectiveEq for ResolvedShotHit {
    fn perspective_eq(&self, other: &Self) -> bool {
        // A shell impact is uniquely identified by the originating salvo plus
        // the victim. `fired_at` may differ slightly between perspectives so
        // we exclude it here.
        self.victim_entity_id == other.victim_entity_id
            && self.salvo.as_ref().map(|s| s.params_id) == other.salvo.as_ref().map(|s| s.params_id)
            && self.salvo.as_ref().map(|s| s.shots.len()) == other.salvo.as_ref().map(|s| s.shots.len())
    }
}

fn union_events<T: Clone + PerspectiveEq>(out: &mut Vec<T>, sources: &[&dyn BattleControllerState], extract: impl Fn(&dyn BattleControllerState) -> &[T]) {
    out.clear();
    for src in sources {
        for ev in extract(*src) {
            if out.iter().any(|existing| existing.perspective_eq(ev)) {
                continue;
            }
            out.push(ev.clone());
        }
    }
}

fn clone_entity(entity: &Entity) -> Entity {
    match entity {
        Entity::Vehicle(v) => Entity::Vehicle(Rc::clone(v)),
        Entity::Building(b) => Entity::Building(Rc::clone(b)),
        Entity::SmokeScreen(s) => Entity::SmokeScreen(Rc::clone(s)),
    }
}

/// A read-only [`BattleControllerState`] that merges several underlying sources.
///
/// Call [`refresh`](Self::refresh) once per render tick after every source
/// controller has consumed all its packets up to the target clock. After that,
/// trait methods read from the precomputed merged snapshot.
pub struct MergedBattleController<'a> {
    sources: Vec<&'a dyn BattleControllerState>,

    clock: GameClock,
    ship_positions: HashMap<EntityId, ShipPosition>,
    minimap_positions: HashMap<EntityId, MinimapPosition>,
    entities_by_id: HashMap<EntityId, Entity>,
    capture_points: Vec<CapturePointState>,
    buff_zones: HashMap<EntityId, BuffZoneState>,
    local_weather_zones: Vec<LocalWeatherZone>,
    captured_buffs: Vec<CapturedBuff>,
    team_scores: Vec<TeamScore>,
    active_consumables: HashMap<EntityId, Vec<ActiveConsumable>>,
    active_shots: Vec<ActiveShot>,
    active_torpedoes: Vec<ActiveTorpedo>,
    shot_hits: Vec<ResolvedShotHit>,
    active_planes: HashMap<PlaneId, ActivePlane>,
    active_wards: HashMap<PlaneId, ActiveWard>,
    kills: Vec<KillRecord>,
    dead_ships: HashMap<EntityId, DeadShip>,
    turret_yaws: HashMap<EntityId, Vec<f32>>,
    target_yaws: HashMap<EntityId, f32>,
    selected_ammo: HashMap<EntityId, GameParamId>,
    game_chat: Vec<GameMessage>,

    empty_player_entities: HashMap<EntityId, Rc<Player>>,
    empty_metadata_players: Vec<SharedPlayer>,
}

impl<'a> MergedBattleController<'a> {
    /// Build a merger. The first source is the primary perspective; everything
    /// else is treated as a secondary perspective used to fill in fog-of-war
    /// gaps. `refresh()` must be called before any trait method is read.
    pub fn new(primary: &'a dyn BattleControllerState, secondaries: Vec<&'a dyn BattleControllerState>) -> Self {
        let mut sources = Vec::with_capacity(1 + secondaries.len());
        sources.push(primary);
        sources.extend(secondaries);
        Self {
            sources,
            clock: GameClock(0.0),
            ship_positions: HashMap::new(),
            minimap_positions: HashMap::new(),
            entities_by_id: HashMap::new(),
            capture_points: Vec::new(),
            buff_zones: HashMap::new(),
            local_weather_zones: Vec::new(),
            captured_buffs: Vec::new(),
            team_scores: Vec::new(),
            active_consumables: HashMap::new(),
            active_shots: Vec::new(),
            active_torpedoes: Vec::new(),
            shot_hits: Vec::new(),
            active_planes: HashMap::new(),
            active_wards: HashMap::new(),
            kills: Vec::new(),
            dead_ships: HashMap::new(),
            turret_yaws: HashMap::new(),
            target_yaws: HashMap::new(),
            selected_ammo: HashMap::new(),
            game_chat: Vec::new(),
            empty_player_entities: HashMap::new(),
            empty_metadata_players: Vec::new(),
        }
    }

    /// Primary perspective. All "self"-flavoured methods (ribbons, damage
    /// stats, relation tagging, scoring config) read from this source.
    fn primary(&self) -> &dyn BattleControllerState {
        self.sources[0]
    }

    /// Recompute the merged snapshot. Cheap relative to packet processing but
    /// linear in the total number of tracked entities across all sources, so
    /// callers should call it at most once per render tick.
    pub fn refresh(&mut self) {
        self.clock = self.primary().clock();

        merge_freshest_map(&mut self.ship_positions, &self.sources, |s| s.ship_positions(), |p| p.last_updated);
        merge_freshest_map(&mut self.minimap_positions, &self.sources, |s| s.minimap_positions(), |p| p.last_updated);

        self.entities_by_id.clear();
        for src in &self.sources {
            for (id, entity) in src.entities_by_id() {
                self.entities_by_id.entry(*id).or_insert_with(|| clone_entity(entity));
            }
        }

        // Globally-consistent state: trust the primary.
        self.capture_points = self.primary().capture_points().to_vec();
        self.team_scores = self.primary().team_scores().to_vec();
        self.local_weather_zones = self.primary().local_weather_zones().to_vec();

        // Per-perspective AOI state: union, primary wins on collision.
        self.buff_zones.clear();
        for src in &self.sources {
            for (id, zone) in src.buff_zones() {
                self.buff_zones.entry(*id).or_insert_with(|| zone.clone());
            }
        }

        merge_freshest_map(&mut self.active_planes, &self.sources, |s| s.active_planes(), |p| p.last_updated);

        self.active_wards.clear();
        for src in &self.sources {
            for (id, w) in src.active_wards() {
                self.active_wards.entry(*id).or_insert_with(|| w.clone());
            }
        }

        self.dead_ships.clear();
        for src in &self.sources {
            for (id, ship) in src.dead_ships() {
                self.dead_ships
                    .entry(*id)
                    .and_modify(|existing| {
                        if ship.clock.0 < existing.clock.0 {
                            *existing = ship.clone();
                        }
                    })
                    .or_insert_with(|| ship.clone());
            }
        }

        self.turret_yaws.clear();
        for src in &self.sources {
            for (id, yaws) in src.turret_yaws() {
                self.turret_yaws.entry(*id).or_insert_with(|| yaws.clone());
            }
        }

        self.target_yaws.clear();
        for src in &self.sources {
            for (id, yaw) in src.target_yaws() {
                self.target_yaws.entry(*id).or_insert(*yaw);
            }
        }

        self.selected_ammo.clear();
        for src in &self.sources {
            for (id, ammo) in src.selected_ammo() {
                self.selected_ammo.entry(*id).or_insert(*ammo);
            }
        }

        self.active_consumables.clear();
        for src in &self.sources {
            for (id, list) in src.active_consumables() {
                self.active_consumables.entry(*id).or_insert_with(|| list.clone());
            }
        }

        // Active shots: union, dedupe by (avatar, fired_at bits).
        self.active_shots.clear();
        let mut shot_keys: HashSet<(AvatarId, u32)> = HashSet::new();
        for src in &self.sources {
            for shot in src.active_shots() {
                let key = (shot.avatar_id, shot.fired_at.0.to_bits());
                if shot_keys.insert(key) {
                    self.active_shots.push(shot.clone());
                }
            }
        }

        // Active torpedoes: union, dedupe by (avatar, torpedo shot_id).
        self.active_torpedoes.clear();
        let mut torp_keys: HashSet<(AvatarId, ShotId)> = HashSet::new();
        for src in &self.sources {
            for t in src.active_torpedoes() {
                let key = (t.avatar_id, t.torpedo.shot_id);
                if torp_keys.insert(key) {
                    self.active_torpedoes.push(t.clone());
                }
            }
        }

        union_events(&mut self.kills, &self.sources, |s| s.kills());
        union_events(&mut self.captured_buffs, &self.sources, |s| s.captured_buffs());
        union_events(&mut self.shot_hits, &self.sources, |s| s.shot_hits());
        union_events(&mut self.game_chat, &self.sources, |s| s.game_chat());
    }
}

fn merge_freshest_map<K: Eq + std::hash::Hash + Copy, V: Clone>(
    out: &mut HashMap<K, V>,
    sources: &[&dyn BattleControllerState],
    extract: impl Fn(&dyn BattleControllerState) -> &HashMap<K, V>,
    last_updated: impl Fn(&V) -> GameClock,
) {
    out.clear();
    for src in sources {
        for (id, value) in extract(*src) {
            let take = match out.get(id) {
                Some(existing) => last_updated(value).0 > last_updated(existing).0,
                None => true,
            };
            if take {
                out.insert(*id, value.clone());
            }
        }
    }
}

impl<'a> BattleControllerState for MergedBattleController<'a> {
    fn clock(&self) -> GameClock {
        self.clock
    }

    fn ship_positions(&self) -> &HashMap<EntityId, ShipPosition> {
        &self.ship_positions
    }

    fn minimap_positions(&self) -> &HashMap<EntityId, MinimapPosition> {
        &self.minimap_positions
    }

    fn player_entities(&self) -> &HashMap<EntityId, Rc<Player>> {
        // Player relations are recorded relative to each replay's recording
        // player; only the primary's tagging is meaningful for the merged view.
        // If the primary somehow has no players (parse error), fall back to
        // an empty map rather than mixing perspectives.
        let primary = self.primary().player_entities();
        if primary.is_empty() { &self.empty_player_entities } else { primary }
    }

    fn metadata_players(&self) -> &[SharedPlayer] {
        let primary = self.primary().metadata_players();
        if primary.is_empty() { &self.empty_metadata_players } else { primary }
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
        self.primary().battle_end_clock()
    }

    fn winning_team(&self) -> Option<i8> {
        self.primary().winning_team()
    }

    fn finish_type(&self) -> Option<&Recognized<FinishType>> {
        self.primary().finish_type()
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
        self.primary().battle_type()
    }

    fn scoring_rules(&self) -> Option<&ScoringRules> {
        self.primary().scoring_rules()
    }

    fn time_left(&self) -> Option<i64> {
        self.primary().time_left()
    }

    fn battle_stage(&self) -> Option<BattleStage> {
        self.primary().battle_stage()
    }

    fn battle_start_clock(&self) -> Option<GameClock> {
        self.primary().battle_start_clock()
    }

    fn game_clock_to_elapsed(&self, clock: GameClock) -> ElapsedClock {
        self.primary().game_clock_to_elapsed(clock)
    }

    fn elapsed_to_game_clock(&self, elapsed: ElapsedClock) -> GameClock {
        self.primary().elapsed_to_game_clock(elapsed)
    }

    fn self_ribbons(&self) -> &HashMap<Ribbon, usize> {
        self.primary().self_ribbons()
    }

    fn self_damage_stats(
        &self,
    ) -> &HashMap<(Recognized<DamageStatWeapon>, Recognized<DamageStatCategory>), DamageStatEntry> {
        self.primary().self_damage_stats()
    }
}
