//! Read-side queries over the ECS world.

use std::collections::HashMap;

use wows_replays::analyzer::battle_controller::VehicleProps;
use wows_replays::types::EntityId;
use wowsunpack::data::ResourceLoader;

use crate::components::{Aim, Building, EntityKind, GameId, MinimapPlacement, SmokeScreen, Transform3d, Vehicle, VehicleState};
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
