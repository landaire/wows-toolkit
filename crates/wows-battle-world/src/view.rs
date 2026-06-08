//! Per-frame read API over the ECS world for the minimap renderer.
//!
//! `BattleView` exposes every accessor the renderer consumes each frame without
//! reconstructing a `QueryState` per call. The component `QueryState`s live in
//! `QueryCache`, created once and reused. `BattleWorld::view` refreshes archetype
//! metadata on the cached states (the only per-frame query cost) and hands back a
//! view that reads through `iter_manual`/`get_manual`, both of which take `&self`
//! and never allocate.
//!
//! The view is non-generic over `G: ResourceLoader`: name resolution the renderer
//! needs is already materialized into components and resources during ingestion,
//! so renderer signatures stay free of the resource-loader type parameter.

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use bevy_ecs::query::QueryState;
use bevy_ecs::world::World;
use wows_replays::Rc;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::VehicleProps;
use wows_replays::analyzer::battle_controller::state::ActiveConsumable;
use wows_replays::analyzer::battle_controller::state::ActivePlane;
use wows_replays::analyzer::battle_controller::state::ActiveShot;
use wows_replays::analyzer::battle_controller::state::ActiveTorpedo;
use wows_replays::analyzer::battle_controller::state::ActiveWard;
use wows_replays::analyzer::battle_controller::state::BuffZoneState;
use wows_replays::analyzer::battle_controller::state::CapturePointState;
use wows_replays::analyzer::battle_controller::state::CapturedBuff;
use wows_replays::analyzer::battle_controller::state::ConsumableInventory;
use wows_replays::analyzer::battle_controller::state::DeadShip;
use wows_replays::analyzer::battle_controller::state::KillRecord;
use wows_replays::analyzer::battle_controller::state::LocalWeatherZone;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::analyzer::battle_controller::state::ScoringRules;
use wows_replays::analyzer::battle_controller::state::TeamScore;
use wows_replays::analyzer::decoder::DamageStatEntry;
use wows_replays::analyzer::decoder::FinishType;
use wows_replays::analyzer::decoder::Recognized;
use wows_replays::types::ElapsedClock;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_types::BattleStage;
use wowsunpack::game_types::BattleType;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::game_types::DamageStatWeapon;
use wowsunpack::game_types::PlaneId;
use wowsunpack::game_types::Ribbon;

use crate::components::Aim;
use crate::components::BuffZoneData;
use crate::components::Building;
use crate::components::BuildingState;
use crate::components::Consumables;
use crate::components::EntityKind;
use crate::components::GameId;
use crate::components::MinimapPlacement;
use crate::components::Plane;
use crate::components::PlaneState;
use crate::components::ProjectileState;
use crate::components::SmokeScreen;
use crate::components::SmokeScreenState;
use crate::components::Transform3d;
use crate::components::Vehicle;
use crate::components::VehicleState;
use crate::components::Ward;
use crate::components::WardState;
use crate::resources::ActiveSecondaryShots;
use crate::resources::ActiveShotOrder;
use crate::resources::ActiveTorpedoOrder;
use crate::resources::CapturePointOrder;
use crate::resources::CapturedBuffs;
use crate::resources::ChatLog;
use crate::resources::Clock;
use crate::resources::DeadShips;
use crate::resources::EntityIndex;
use crate::resources::KillLog;
use crate::resources::MatchState;
use crate::resources::PlayerIndex;
use crate::resources::ScoringRules as ScoringRulesResource;
use crate::resources::SecondaryShot;
use crate::resources::SelfStats;
use crate::resources::TeamScores;
use crate::resources::WeatherZoneOrder;
use crate::units::MatchWinner;
use crate::world::BattleWorld;

/// Cached `QueryState`s reused across frames.
///
/// Constructing a `QueryState` walks every archetype, so doing it per accessor
/// would dominate per-frame cost. These are built once (lazily on the first
/// `view` call, which needs `&mut World`) and only have their archetype metadata
/// refreshed via `update_archetypes` thereafter.
pub struct QueryCache {
    positions: QueryState<(&'static GameId, &'static Transform3d)>,
    minimap: QueryState<(&'static GameId, &'static MinimapPlacement)>,
    vehicles: QueryState<(&'static GameId, &'static VehicleState)>,
    aims: QueryState<(&'static GameId, &'static Aim)>,
    consumables: QueryState<(&'static GameId, &'static Consumables)>,
    planes: QueryState<&'static PlaneState, bevy_ecs::query::With<Plane>>,
    wards: QueryState<&'static WardState, bevy_ecs::query::With<Ward>>,
    buff_zones: QueryState<(&'static GameId, &'static BuffZoneData)>,
    vehicle_kind: QueryState<&'static GameId, bevy_ecs::query::With<Vehicle>>,
    building_kind: QueryState<&'static GameId, bevy_ecs::query::With<Building>>,
    smoke_kind: QueryState<&'static GameId, bevy_ecs::query::With<SmokeScreen>>,
    building_state: QueryState<(&'static GameId, &'static BuildingState)>,
    smoke_state: QueryState<(&'static GameId, &'static SmokeScreenState)>,
    vehicle_by_entity: QueryState<&'static VehicleState>,
    aim_by_entity: QueryState<&'static Aim>,
}

impl QueryCache {
    pub(crate) fn new(world: &mut World) -> Self {
        Self {
            positions: world.query(),
            minimap: world.query(),
            vehicles: world.query(),
            aims: world.query(),
            consumables: world.query(),
            planes: world.query_filtered(),
            wards: world.query_filtered(),
            buff_zones: world.query(),
            vehicle_kind: world.query_filtered(),
            building_kind: world.query_filtered(),
            smoke_kind: world.query_filtered(),
            building_state: world.query(),
            smoke_state: world.query(),
            vehicle_by_entity: world.query(),
            aim_by_entity: world.query(),
        }
    }

    /// Refresh archetype metadata so the subsequent `*_manual` reads are sound.
    ///
    /// This is the single per-frame query cost; it walks only archetypes added
    /// since the last call, not the full set.
    pub(crate) fn update_archetypes(&mut self, world: &World) {
        self.positions.update_archetypes(world);
        self.minimap.update_archetypes(world);
        self.vehicles.update_archetypes(world);
        self.aims.update_archetypes(world);
        self.consumables.update_archetypes(world);
        self.planes.update_archetypes(world);
        self.wards.update_archetypes(world);
        self.buff_zones.update_archetypes(world);
        self.vehicle_kind.update_archetypes(world);
        self.building_kind.update_archetypes(world);
        self.smoke_kind.update_archetypes(world);
        self.building_state.update_archetypes(world);
        self.smoke_state.update_archetypes(world);
        self.vehicle_by_entity.update_archetypes(world);
        self.aim_by_entity.update_archetypes(world);
    }
}

impl<'res, 'replay, G: ResourceLoader> BattleWorld<'res, 'replay, G> {
    /// Borrow a per-frame read view over the world.
    ///
    /// Builds the cached query states on first use, then refreshes archetype
    /// metadata. The returned `BattleView` reads through `&self` query methods, so
    /// no further `QueryState` allocation happens for the lifetime of the view.
    pub fn view(&mut self) -> BattleView<'_> {
        let battle_type = BattleType::from_value(self.game_type().unwrap_or(""), self.version());
        let (world, cache) = self.view_parts();
        BattleView { world, cache, battle_type }
    }
}

/// Read-only per-frame view over the ECS world.
///
/// Holds a shared world borrow plus the shared cached query states; every
/// accessor either reads a resource or iterates via the pre-built, archetype-
/// refreshed query states.
pub struct BattleView<'w> {
    world: &'w World,
    cache: &'w QueryCache,
    battle_type: Recognized<BattleType>,
}

impl<'w> BattleView<'w> {
    /// Current replay clock.
    pub fn clock(&self) -> GameClock {
        self.world.resource::<Clock>().0
    }

    /// World-space transform per entity, keyed by game entity id.
    pub fn positions(&self) -> HashMap<EntityId, &'w Transform3d> {
        self.cache.positions.iter_manual(self.world).map(|(gid, t)| (gid.0, t)).collect()
    }

    /// Minimap placement per entity, keyed by game entity id.
    pub fn minimap_positions(&self) -> HashMap<EntityId, &'w MinimapPlacement> {
        self.cache.minimap.iter_manual(self.world).map(|(gid, m)| (gid.0, m)).collect()
    }

    /// Players parsed from the arena roster, keyed by entity id.
    pub fn player_entities(&self) -> &'w HashMap<EntityId, Rc<Player>> {
        &self.world.resource::<PlayerIndex>().0
    }

    /// Vehicle props for a single entity, if it is a vehicle.
    pub fn vehicle_props(&self, id: EntityId) -> Option<&'w VehicleProps> {
        let entity = self.entity_of(id)?;
        self.cache.vehicle_by_entity.get_manual(self.world, entity).ok().map(|vs| &vs.0)
    }

    /// All vehicle props, keyed by entity id.
    pub fn vehicle_props_all(&self) -> HashMap<EntityId, &'w VehicleProps> {
        self.cache.vehicles.iter_manual(self.world).map(|(gid, vs)| (gid.0, &vs.0)).collect()
    }

    /// Aim state (turret/target yaws, selected ammo) for a single entity.
    pub fn aim(&self, id: EntityId) -> Option<&'w Aim> {
        let entity = self.entity_of(id)?;
        self.cache.aim_by_entity.get_manual(self.world, entity).ok()
    }

    /// Coarse entity kind for a single entity, if tracked.
    pub fn entity_kind(&self, id: EntityId) -> Option<EntityKind> {
        let entity = self.entity_of(id)?;
        if self.cache.vehicle_kind.get_manual(self.world, entity).is_ok() {
            Some(EntityKind::Vehicle)
        } else if self.cache.building_kind.get_manual(self.world, entity).is_ok() {
            Some(EntityKind::Building)
        } else if self.cache.smoke_kind.get_manual(self.world, entity).is_ok() {
            Some(EntityKind::SmokeScreen)
        } else {
            None
        }
    }

    /// Entity kinds for every tracked game entity.
    pub fn entity_kinds(&self) -> HashMap<EntityId, EntityKind> {
        let mut out = HashMap::new();
        for gid in self.cache.vehicle_kind.iter_manual(self.world) {
            out.insert(gid.0, EntityKind::Vehicle);
        }
        for gid in self.cache.building_kind.iter_manual(self.world) {
            out.insert(gid.0, EntityKind::Building);
        }
        for gid in self.cache.smoke_kind.iter_manual(self.world) {
            out.insert(gid.0, EntityKind::SmokeScreen);
        }
        out
    }

    /// Building states per entity id.
    pub fn buildings(&self) -> HashMap<EntityId, &'w BuildingState> {
        self.cache.building_state.iter_manual(self.world).map(|(gid, state)| (gid.0, state)).collect()
    }

    /// Smoke screen states per entity id.
    pub fn smoke_screens(&self) -> HashMap<EntityId, &'w SmokeScreenState> {
        self.cache.smoke_state.iter_manual(self.world).map(|(gid, state)| (gid.0, state)).collect()
    }

    /// Main-battery turret yaws (radians) per entity, group 0 only.
    pub fn turret_yaws(&self) -> HashMap<EntityId, Vec<f32>> {
        self.cache
            .aims
            .iter_manual(self.world)
            .filter(|(_, aim)| !aim.turret_yaws.is_empty())
            .map(|(gid, aim)| (gid.0, aim.turret_yaws.iter().map(|r| r.0).collect()))
            .collect()
    }

    /// World-space gun aim yaw (radians) per entity.
    pub fn target_yaws(&self) -> HashMap<EntityId, f32> {
        self.cache.aims.iter_manual(self.world).filter_map(|(gid, aim)| aim.target_yaw.map(|r| (gid.0, r.0))).collect()
    }

    /// Selected artillery ammo param per entity.
    pub fn selected_ammo(&self) -> HashMap<EntityId, wows_replays::types::GameParamId> {
        use wowsunpack::game_types::WeaponType;
        self.cache
            .aims
            .iter_manual(self.world)
            .filter_map(|(gid, aim)| {
                aim.selected_ammo.get(&Recognized::Known(WeaponType::Artillery)).map(|id| (gid.0, *id))
            })
            .collect()
    }

    /// Active consumable activations per entity (only entities with at least one).
    pub fn active_consumables(&self) -> HashMap<EntityId, Vec<ActiveConsumable>> {
        self.cache
            .consumables
            .iter_manual(self.world)
            .filter(|(_, c)| !c.active.is_empty())
            .map(|(gid, c)| (gid.0, c.active.clone()))
            .collect()
    }

    /// Consumable slot inventories per entity (only entities with seeded slots).
    pub fn consumable_inventories(&self) -> HashMap<EntityId, Vec<ConsumableInventory>> {
        self.cache
            .consumables
            .iter_manual(self.world)
            .filter(|(_, c)| !c.slots.is_empty())
            .map(|(gid, c)| (gid.0, c.slots.clone()))
            .collect()
    }

    /// Active plane squadrons keyed by plane id.
    pub fn active_planes(&self) -> HashMap<PlaneId, ActivePlane> {
        self.cache
            .planes
            .iter_manual(self.world)
            .map(|state| {
                let ap = ActivePlane {
                    plane_id: state.plane_id,
                    owner_id: state.owner_id,
                    team_id: state.team_id.raw() as u32,
                    params_id: state.params_id,
                    position: state.position,
                    last_updated: state.last_updated,
                };
                (state.plane_id, ap)
            })
            .collect()
    }

    /// Active fighter patrol wards keyed by plane id.
    pub fn active_wards(&self) -> HashMap<PlaneId, ActiveWard> {
        self.cache
            .wards
            .iter_manual(self.world)
            .map(|state| {
                let aw = ActiveWard {
                    plane_id: state.plane_id,
                    position: state.position,
                    radius: state.radius,
                    owner_id: state.owner_id,
                };
                (state.plane_id, aw)
            })
            .collect()
    }

    /// In-flight artillery salvos in BattleController.active_shots order.
    pub fn active_shots(&self) -> Vec<ActiveShot> {
        self.world
            .resource::<ActiveShotOrder>()
            .0
            .iter()
            .filter_map(|&entity| match self.world.get_entity(entity).ok()?.get::<ProjectileState>()? {
                ProjectileState::Artillery { salvo, fired_at, avatar_id } => {
                    Some(ActiveShot { avatar_id: *avatar_id, salvo: salvo.clone(), fired_at: *fired_at })
                }
                ProjectileState::Torpedo { .. } => None,
            })
            .collect()
    }

    /// In-flight secondary (ATBA) shots, in fire order.
    pub fn active_secondary_shots(&self) -> Vec<SecondaryShot> {
        self.world.resource::<ActiveSecondaryShots>().0.clone()
    }

    /// In-flight torpedoes in BattleController.active_torpedoes order.
    pub fn active_torpedoes(&self) -> Vec<ActiveTorpedo> {
        self.world
            .resource::<ActiveTorpedoOrder>()
            .0
            .iter()
            .filter_map(|&entity| match self.world.get_entity(entity).ok()?.get::<ProjectileState>()? {
                ProjectileState::Torpedo { torpedo, launched_at, updated_at, avatar_id } => Some(ActiveTorpedo {
                    avatar_id: *avatar_id,
                    torpedo: torpedo.clone(),
                    launched_at: *launched_at,
                    updated_at: *updated_at,
                }),
                ProjectileState::Artillery { .. } => None,
            })
            .collect()
    }

    /// Buff zone states (arms race powerup drops) keyed by entity id.
    pub fn buff_zones(&self) -> HashMap<EntityId, BuffZoneState> {
        self.cache.buff_zones.iter_manual(self.world).map(|(gid, data)| (gid.0, data.0.clone())).collect()
    }

    /// Capture point states in control-point index order.
    ///
    /// Gap indices carry a default state so the returned length is max_index+1,
    /// matching the original controller.
    pub fn capture_points(&self) -> Vec<CapturePointState> {
        self.world
            .resource::<CapturePointOrder>()
            .0
            .iter()
            .map(|&entity| {
                self.world
                    .get_entity(entity)
                    .ok()
                    .and_then(|e| e.get::<crate::components::CapturePointData>().map(|d| d.0.clone()))
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Active local weather zones in creation order.
    pub fn local_weather_zones(&self) -> Vec<LocalWeatherZone> {
        self.world
            .resource::<WeatherZoneOrder>()
            .0
            .iter()
            .filter_map(|&entity| {
                self.world.get_entity(entity).ok()?.get::<crate::components::WeatherZoneData>().map(|d| d.0.clone())
            })
            .collect()
    }

    /// Current scores for all teams, in team_index order.
    pub fn team_scores(&self) -> &'w [TeamScore] {
        &self.world.resource::<TeamScores>().0
    }

    /// Buffs captured so far (arms race), in arrival order.
    pub fn captured_buffs(&self) -> &'w [CapturedBuff] {
        &self.world.resource::<CapturedBuffs>().0
    }

    /// Resolved shot hits recorded for the current frame (cleared each packet by the world).
    pub fn shot_hits(&self) -> &'w [ResolvedShotHit] {
        &self.world.resource::<crate::resources::ShotHitLog>().0
    }

    /// Scoring rules from BattleLogic.
    pub fn scoring_rules(&self) -> Option<&'w ScoringRules> {
        self.world.resource::<ScoringRulesResource>().0.as_ref()
    }

    /// All ship kills in arrival order.
    pub fn kills(&self) -> &'w [KillRecord] {
        &self.world.resource::<KillLog>().0
    }

    /// Dead ships keyed by victim entity id.
    pub fn dead_ships(&self) -> &'w HashMap<EntityId, DeadShip> {
        &self.world.resource::<DeadShips>().0
    }

    /// Chat messages received so far, in arrival order.
    pub fn game_chat(&self) -> &'w [GameMessage] {
        &self.world.resource::<ChatLog>().0
    }

    /// Ribbon counts for the self player.
    pub fn self_ribbons(&self) -> &'w HashMap<Ribbon, usize> {
        &self.world.resource::<SelfStats>().ribbons
    }

    /// Cumulative damage stats for the self player.
    pub fn self_damage_stats(
        &self,
    ) -> &'w HashMap<(Recognized<DamageStatWeapon>, Recognized<DamageStatCategory>), DamageStatEntry> {
        &self.world.resource::<SelfStats>().damage_stats
    }

    /// Battle type (Random, Ranked, Clan, Co-op, etc.) from replay metadata.
    pub fn battle_type(&self) -> Recognized<BattleType> {
        self.battle_type.clone()
    }

    /// Resolved battle stage.
    pub fn battle_stage(&self) -> Option<BattleStage> {
        self.world.resource::<MatchState>().battle_stage
    }

    /// Seconds remaining in the match.
    pub fn time_left(&self) -> Option<i64> {
        self.world.resource::<MatchState>().time_left.map(|s| s.0)
    }

    /// Clock when the battle stage first became `Battle` (pre-battle countdown end).
    pub fn battle_start_clock(&self) -> Option<GameClock> {
        self.world.resource::<MatchState>().battle_start_clock
    }

    /// Clock when `BattleEnd` was received.
    pub fn battle_end_clock(&self) -> Option<GameClock> {
        self.world.resource::<MatchState>().battle_end_clock
    }

    /// Winning team as an i8: 0 or 1 for a team win, -1 for draw, None if undecided.
    pub fn winning_team(&self) -> Option<i8> {
        self.world.resource::<MatchState>().winning_team.map(|mw| match mw {
            MatchWinner::Team(t) => t.raw() as i8,
            MatchWinner::Draw => -1,
        })
    }

    /// How the battle ended.
    pub fn finish_type(&self) -> Option<&'w Recognized<FinishType>> {
        self.world.resource::<MatchState>().finish_type.as_ref()
    }

    /// Convert an absolute game clock to elapsed time since battle start.
    pub fn game_clock_to_elapsed(&self, clock: GameClock) -> ElapsedClock {
        let start = self.battle_start_clock().unwrap_or(GameClock(0.0));
        clock.to_elapsed(start)
    }

    /// Convert elapsed time since battle start back to an absolute game clock.
    pub fn elapsed_to_game_clock(&self, elapsed: ElapsedClock) -> GameClock {
        let start = self.battle_start_clock().unwrap_or(GameClock(0.0));
        elapsed.to_absolute(start)
    }

    fn entity_of(&self, id: EntityId) -> Option<Entity> {
        self.world.resource::<EntityIndex>().get(id)
    }
}
