//! Read-side queries over the ECS world.

use wows_replays::types::EntityId;
use wowsunpack::data::ResourceLoader;

use crate::components::{Building, EntityKind, GameId, MinimapPlacement, SmokeScreen, Transform3d, Vehicle};
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
