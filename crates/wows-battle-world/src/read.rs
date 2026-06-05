//! Read-side queries over the ECS world.

use std::collections::HashMap;

use wows_replays::Rc;
use wows_replays::analyzer::battle_controller::DamageEvent;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::VehicleProps;
use wows_replays::analyzer::battle_controller::state::ActiveConsumable;
use wows_replays::analyzer::battle_controller::state::ConsumableInventory;
use wows_replays::analyzer::battle_controller::state::DeadShip;
use wows_replays::analyzer::battle_controller::state::KillRecord;
use wows_replays::analyzer::decoder::DamageStatEntry;
use wows_replays::analyzer::decoder::Recognized;
use wows_replays::types::EntityId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::game_types::DamageStatWeapon;
use wowsunpack::game_types::Ribbon;

use crate::components::{Aim, Building, Consumables, EntityKind, GameId, MinimapPlacement, SmokeScreen, Transform3d, Vehicle, VehicleState};
use crate::resources::{ChatLog, DamageLedger, DeadShips, KillLog, PlayerIndex, SelfStats};
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
}
