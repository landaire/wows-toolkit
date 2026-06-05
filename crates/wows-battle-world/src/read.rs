//! Read-side queries over the ECS world.

use std::collections::HashMap;

use wows_replays::Rc;
use wows_replays::analyzer::battle_controller::DamageEvent;
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
use wows_replays::types::ArenaId;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_types::BattleStage;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::game_types::DamageStatWeapon;
use wowsunpack::game_types::Ribbon;

use wowsunpack::game_types::PlaneId;

use crate::components::{Aim, Building, BuffZoneData, CapturePointData, Consumables, EntityKind, GameId, MinimapPlacement, Plane, PlaneState, ProjectileState, SmokeScreen, Transform3d, Vehicle, VehicleState, Ward, WardState, WeatherZoneData};
use crate::resources::{ActiveShotOrder, ActiveTorpedoOrder, CapturePointOrder, CapturedBuffs, ChatLog, DamageLedger, DeadShips, KillLog, MatchState, PlayerIndex, ScoringRules as ScoringRulesResource, SelfStats, ShotHitLog, TeamScores, WeatherZoneOrder};
use crate::units::MatchWinner;
use crate::world::BattleWorld;

impl<'res, 'replay, G: ResourceLoader> BattleWorld<'res, 'replay, G> {
    /// World-space positions for every entity that has one.
    pub fn positions(&mut self) -> Vec<(EntityId, Transform3d)> {
        let world = self.world_mut();
        let mut q = world.query::<(&GameId, &Transform3d)>();
        q.iter(world).map(|(gid, t)| (gid.0, t.clone())).collect()
    }

    /// Minimap placements for every entity that has one.
    pub fn minimap(&mut self) -> Vec<(EntityId, MinimapPlacement)> {
        let world = self.world_mut();
        let mut q = world.query::<(&GameId, &MinimapPlacement)>();
        q.iter(world).map(|(gid, m)| (gid.0, m.clone())).collect()
    }

    /// Cloned `VehicleProps` for a single vehicle entity, if present.
    pub fn vehicle_props(&mut self, id: EntityId) -> Option<VehicleProps> {
        let world = self.world_mut();
        let ecs_entity = world.resource::<crate::resources::EntityIndex>().get(id)?;
        world.get_entity(ecs_entity).ok()?.get::<VehicleState>().map(|vs| vs.0.clone())
    }

    /// All vehicle `VehicleProps` indexed by entity id.
    pub fn vehicle_props_all(&mut self) -> HashMap<EntityId, VehicleProps> {
        let world = self.world_mut();
        let mut q = world.query::<(&GameId, &VehicleState)>();
        q.iter(world).map(|(gid, vs)| (gid.0, vs.0.clone())).collect()
    }

    /// `Aim` for a single vehicle entity, if present.
    pub fn aim(&mut self, id: EntityId) -> Option<Aim> {
        let world = self.world_mut();
        let ecs_entity = world.resource::<crate::resources::EntityIndex>().get(id)?;
        world.get_entity(ecs_entity).ok()?.get::<Aim>().cloned()
    }

    /// All `Aim` components indexed by entity id.
    pub fn aims_all(&mut self) -> HashMap<EntityId, Aim> {
        let world = self.world_mut();
        let mut q = world.query::<(&GameId, &Aim)>();
        q.iter(world).map(|(gid, aim)| (gid.0, aim.clone())).collect()
    }

    /// All ship kills in arrival order.
    pub fn kills(&self) -> &[KillRecord] {
        &self.world().resource::<KillLog>().0
    }

    /// Dead ships keyed by victim entity id.
    pub fn dead_ships(&self) -> &HashMap<EntityId, DeadShip> {
        &self.world().resource::<DeadShips>().0
    }

    /// Damage events per aggressor entity id.
    pub fn damage_ledger(&self) -> &HashMap<EntityId, Vec<DamageEvent>> {
        &self.world().resource::<DamageLedger>().0
    }

    /// Ribbon counts for the self player.
    pub fn self_ribbons(&self) -> &HashMap<Ribbon, usize> {
        &self.world().resource::<SelfStats>().ribbons
    }

    /// Cumulative damage stats for the self player.
    pub fn self_damage_stats(
        &self,
    ) -> &HashMap<(Recognized<DamageStatWeapon>, Recognized<DamageStatCategory>), DamageStatEntry>
    {
        &self.world().resource::<SelfStats>().damage_stats
    }

    /// Chat messages received so far, in arrival order.
    pub fn chat(&self) -> &[GameMessage] {
        &self.world().resource::<ChatLog>().0
    }

    /// Players built from the arena roster, keyed by entity id.
    ///
    /// Populated after OnArenaStateReceived; empty before that packet arrives.
    pub fn player_entities(&self) -> &HashMap<EntityId, Rc<Player>> {
        &self.world().resource::<PlayerIndex>().0
    }

    /// Active consumable activations keyed by entity id.
    pub fn active_consumables(&mut self) -> HashMap<EntityId, Vec<ActiveConsumable>> {
        let world = self.world_mut();
        let mut q = world.query::<(&GameId, &Consumables)>();
        q.iter(world)
            .filter(|(_, c)| !c.active.is_empty())
            .map(|(gid, c)| (gid.0, c.active.clone()))
            .collect()
    }

    /// Consumable inventory slots keyed by entity id.
    pub fn consumable_inventories(&mut self) -> HashMap<EntityId, Vec<ConsumableInventory>> {
        let world = self.world_mut();
        let mut q = world.query::<(&GameId, &Consumables)>();
        q.iter(world)
            .filter(|(_, c)| !c.slots.is_empty())
            .map(|(gid, c)| (gid.0, c.slots.clone()))
            .collect()
    }

    /// Active plane squadrons keyed by plane id.
    pub fn active_planes(&mut self) -> HashMap<PlaneId, ActivePlane> {
        let world = self.world_mut();
        let mut q = world.query::<(&PlaneState, &Plane)>();
        q.iter(world)
            .map(|(state, _)| {
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
    pub fn active_wards(&mut self) -> HashMap<PlaneId, ActiveWard> {
        let world = self.world_mut();
        let mut q = world.query::<(&WardState, &Ward)>();
        q.iter(world)
            .map(|(state, _)| {
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

    /// In-flight artillery salvos, in BattleController.active_shots order.
    pub fn active_shots(&mut self) -> Vec<ActiveShot> {
        let order = self.world().resource::<ActiveShotOrder>().0.clone();
        let world = self.world();
        order
            .into_iter()
            .filter_map(|entity| {
                let state = world.get_entity(entity).ok()?.get::<ProjectileState>()?;
                match state {
                    ProjectileState::Artillery { salvo, fired_at, avatar_id } => Some(ActiveShot {
                        avatar_id: *avatar_id,
                        salvo: salvo.clone(),
                        fired_at: *fired_at,
                    }),
                    ProjectileState::Torpedo { .. } => None,
                }
            })
            .collect()
    }

    /// In-flight torpedoes, in BattleController.active_torpedoes order.
    pub fn active_torpedoes(&mut self) -> Vec<ActiveTorpedo> {
        let order = self.world().resource::<ActiveTorpedoOrder>().0.clone();
        let world = self.world();
        order
            .into_iter()
            .filter_map(|entity| {
                let state = world.get_entity(entity).ok()?.get::<ProjectileState>()?;
                match state {
                    ProjectileState::Torpedo { torpedo, launched_at, updated_at, avatar_id } => {
                        Some(ActiveTorpedo {
                            avatar_id: *avatar_id,
                            torpedo: torpedo.clone(),
                            launched_at: *launched_at,
                            updated_at: *updated_at,
                        })
                    }
                    ProjectileState::Artillery { .. } => None,
                }
            })
            .collect()
    }

    /// Resolved shot hits recorded for the current frame (Tracked clears each packet).
    pub fn shot_hits(&self) -> Vec<ResolvedShotHit> {
        self.world().resource::<ShotHitLog>().0.clone()
    }

    /// Entity kinds (Vehicle/Building/SmokeScreen) for every tracked game entity.
    pub fn entity_kinds(&mut self) -> Vec<(EntityId, EntityKind)> {
        let mut out = Vec::new();
        {
            let world = self.world_mut();
            let mut q = world.query::<(&GameId, &Vehicle)>();
            for (gid, _) in q.iter(world) {
                out.push((gid.0, EntityKind::Vehicle));
            }
        }
        {
            let world = self.world_mut();
            let mut q = world.query::<(&GameId, &Building)>();
            for (gid, _) in q.iter(world) {
                out.push((gid.0, EntityKind::Building));
            }
        }
        {
            let world = self.world_mut();
            let mut q = world.query::<(&GameId, &SmokeScreen)>();
            for (gid, _) in q.iter(world) {
                out.push((gid.0, EntityKind::SmokeScreen));
            }
        }
        out
    }

    /// Capture point states in control-point index order.
    ///
    /// Gap indices carry a default CapturePointState so the returned Vec has
    /// length == max_index+1, matching the original controller's semantics.
    pub fn capture_points(&self) -> Vec<CapturePointState> {
        let order = &self.world().resource::<CapturePointOrder>().0;
        order
            .iter()
            .map(|&ecs_entity| {
                self.world()
                    .get_entity(ecs_entity)
                    .ok()
                    .and_then(|er| er.get::<CapturePointData>().map(|d| d.0.clone()))
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Current scores for all teams, in team_index order.
    pub fn team_scores(&self) -> Vec<TeamScore> {
        self.world().resource::<TeamScores>().0.clone()
    }

    /// Buff zone states (arms race powerup drop zones) keyed by game entity id.
    pub fn buff_zones(&mut self) -> HashMap<EntityId, BuffZoneState> {
        let world = self.world_mut();
        let mut q = world.query::<(&GameId, &BuffZoneData)>();
        q.iter(world).map(|(gid, data)| (gid.0, data.0.clone())).collect()
    }

    /// All buffs captured so far (arms race), in arrival order.
    pub fn captured_buffs(&self) -> Vec<CapturedBuff> {
        self.world().resource::<CapturedBuffs>().0.clone()
    }

    /// Active local weather zones (squalls/storms) in creation order.
    ///
    /// Uses WeatherZoneOrder as the authoritative sequence so bevy entity index
    /// reuse after despawn cannot corrupt the logical array indices.
    pub fn local_weather_zones(&self) -> Vec<LocalWeatherZone> {
        let order = self.world().resource::<WeatherZoneOrder>().0.clone();
        order
            .into_iter()
            .filter_map(|ecs_entity| {
                self.world()
                    .get_entity(ecs_entity)
                    .ok()?
                    .get::<WeatherZoneData>()
                    .map(|d| d.0.clone())
            })
            .collect()
    }

    /// Scoring rules from BattleLogic (win threshold, hold reward/period, cap indices).
    pub fn scoring_rules(&self) -> Option<ScoringRules> {
        self.world().resource::<ScoringRulesResource>().0.clone()
    }

    /// Resolved battle stage, updated from BattleLogic `battleStage` EntityProperty.
    pub fn battle_stage(&self) -> Option<BattleStage> {
        self.world().resource::<MatchState>().battle_stage.clone()
    }

    /// Seconds remaining in the match from BattleLogic `timeLeft`.
    pub fn time_left(&self) -> Option<i64> {
        self.world().resource::<MatchState>().time_left.map(|s| s.0)
    }

    /// Clock when the battle stage first became `Waiting` (pre-battle start).
    pub fn battle_start_clock(&self) -> Option<GameClock> {
        self.world().resource::<MatchState>().battle_start_clock
    }

    /// Clock when `BattleEnd` was received.
    pub fn battle_end_clock(&self) -> Option<GameClock> {
        self.world().resource::<MatchState>().battle_end_clock
    }

    /// Clock when `battleResult` was set on BattleLogic (regulation time ended).
    pub fn battle_result_clock(&self) -> Option<GameClock> {
        self.world().resource::<MatchState>().battle_result_clock
    }

    /// Winning team as an i8: 0 or 1 for a team win, -1 for draw, None if undecided.
    pub fn winning_team(&self) -> Option<i8> {
        self.world().resource::<MatchState>().winning_team.map(|mw| match mw {
            MatchWinner::Team(t) => t.raw() as i8,
            MatchWinner::Draw => -1,
        })
    }

    /// How the battle ended.
    pub fn finish_type(&self) -> Option<&Recognized<FinishType>> {
        self.world().resource::<MatchState>().finish_type.as_ref()
    }

    /// Arena id set from the first `OnArenaStateReceived` packet.
    pub fn arena_id(&self) -> Option<ArenaId> {
        self.world().resource::<MatchState>().arena_id
    }

    /// Serialized battle results JSON blob.
    pub fn battle_results(&self) -> Option<&String> {
        self.world().resource::<MatchState>().battle_results.as_ref()
    }
}
