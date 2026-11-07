//! Position and minimap ingestion handlers.

use bevy_ecs::world::World;
use wows_replays::analyzer::decoder::MinimapUpdate;
use wows_replays::packet2::PlayerOrientationPacket;
use wows_replays::packet2::PositionPacket;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::WorldPos;

use crate::components::BuffZone;
use crate::components::BuffZoneData;
use crate::components::CapturePoint;
use crate::components::CapturePointData;
use crate::components::GameId;
use crate::components::MinimapPlacement;
use crate::components::SmokeScreen;
use crate::components::SmokeScreenState;
use crate::components::Transform3d;
use crate::components::VehicleState;
use crate::components::WeatherZone;
use crate::components::WeatherZoneData;
use crate::ids::SourceTeam;
use crate::resources::EntityIndex;
use crate::units::Degrees;
use crate::units::Radians;

/// Handle a Position packet: update Transform3d for the entity.
pub fn handle_position(pos: &PositionPacket, world: &mut World, clock: GameClock) {
    let entity = spawn_or_get(world, pos.pid);
    let t = Transform3d {
        pos: WorldPos::new(pos.position.x, pos.position.y, pos.position.z),
        yaw: Radians::from(pos.rotation.yaw),
        pitch: Radians::from(pos.rotation.pitch),
        roll: Radians::from(pos.rotation.roll),
        last_updated: clock,
    };
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(t);
    }
}

/// Handle a PlayerOrientation packet: update Transform3d only when parent_id == 0.
///
/// Non-zero parent_id indicates the entity is attached (e.g. camera to ship);
/// only the free-floating case maps to a ship world position.
pub fn handle_player_orientation(orient: &PlayerOrientationPacket, world: &mut World, clock: GameClock) {
    if orient.parent_id != EntityId::from(0u32) {
        return;
    }
    let entity = spawn_or_get(world, orient.pid);
    let t = Transform3d {
        pos: WorldPos::new(orient.position.x, orient.position.y, orient.position.z),
        yaw: Radians::from(orient.rotation.yaw),
        pitch: Radians::from(orient.rotation.pitch),
        roll: Radians::from(orient.rotation.roll),
        last_updated: clock,
    };
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(t);
    }
}

/// Handle MinimapUpdate entries.
///
/// Source-team filtering: when `source_team` is Some, only updates for entities
/// belonging to that team are applied. Entities not yet known fall through so
/// their first sighting still registers.
pub fn handle_minimap_updates(updates: &[MinimapUpdate], world: &mut World, clock: GameClock, source_team: SourceTeam) {
    for update in updates {
        // Source-team filter.
        if let Some(team) = source_team.0
            && let Some(ecs_entity) = world.resource::<EntityIndex>().get(update.entity_id)
            && let Ok(er) = world.get_entity(ecs_entity)
            && let Some(vs) = er.get::<VehicleState>()
        {
            use wows_replays::types::TeamId;
            let entity_team = TeamId::from(vs.0.team_id() as i64);
            if entity_team != team {
                continue;
            }
        }

        // Minimap pings are one-shot flashes; do not treat them as sustained detection.
        let visible = !update.is_sentinel && !update.is_minimap_ping();

        // When not visible, preserve last known position and heading.
        let (position, heading) = if !visible {
            let prev = world
                .resource::<EntityIndex>()
                .get(update.entity_id)
                .and_then(|e| world.get_entity(e).ok())
                .and_then(|er| er.get::<MinimapPlacement>().map(|m| (m.pos, m.heading)));
            prev.unwrap_or((update.position, Degrees::from(update.heading)))
        } else {
            (update.position, Degrees::from(update.heading))
        };

        // Pull visibility_flags and is_invisible from the vehicle's current VehicleState.
        let (visibility_flags, is_invisible) = world
            .resource::<EntityIndex>()
            .get(update.entity_id)
            .and_then(|e| world.get_entity(e).ok())
            .and_then(|er| er.get::<VehicleState>().map(|vs| (vs.0.visibility_flags(), vs.0.is_invisible())))
            // No VehicleState yet means nothing is known about this entity's
            // detection state, which is what `None` already encodes.
            .unwrap_or((None, false));

        let placement =
            MinimapPlacement { pos: position, heading, visible, visibility_flags, is_invisible, last_updated: clock };

        let entity = spawn_or_get(world, update.entity_id);
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert(placement);
        }
    }
}

/// Handle NonVolatilePosition: update the position of a smoke screen or interactive zone.
///
/// Interactive zones can be created at a staging position and moved into place later,
/// as event capture points are.
pub fn handle_non_volatile_position(entity_id: EntityId, position: WorldPos, world: &mut World) {
    let Some(ecs_entity) = world.resource::<EntityIndex>().get(entity_id) else { return };
    let Ok(mut er) = world.get_entity_mut(ecs_entity) else { return };
    if er.contains::<SmokeScreen>() {
        if let Some(mut state) = er.get_mut::<SmokeScreenState>() {
            state.position = position;
        }
    } else if er.contains::<WeatherZone>() {
        if let Some(mut data) = er.get_mut::<WeatherZoneData>() {
            data.0.position = position;
        }
    } else if er.contains::<CapturePoint>() {
        if let Some(mut data) = er.get_mut::<CapturePointData>() {
            data.0.position = Some(position);
        }
    } else if er.contains::<BuffZone>()
        && let Some(mut data) = er.get_mut::<BuffZoneData>()
    {
        data.0.position = position;
    }
}

fn spawn_or_get(world: &mut World, id: EntityId) -> bevy_ecs::entity::Entity {
    if let Some(entity) = world.resource::<EntityIndex>().get(id) {
        return entity;
    }
    let entity = world.spawn((GameId(id),)).id();
    world.resource_mut::<EntityIndex>().insert(id, entity);
    entity
}

#[cfg(test)]
mod tests {
    use wows_replays::analyzer::battle_controller::state::CapturePointState;
    use wowsunpack::rpc::typedefs::ArgValue;

    use super::*;
    use crate::ingest::zones::handle_entity_property_zone;

    /// Event capture points are created at a staging row along the map edge and moved
    /// into place with NonVolatilePosition and a radius EntityProperty at battle start.
    #[test]
    fn capture_point_follows_non_volatile_position_and_radius() {
        let mut world = World::new();
        world.insert_resource(EntityIndex::default());

        let id = EntityId::from(512790u32);
        let staging = CapturePointState {
            position: Some(WorldPos::new(-451.7, 0.0, 649.6)),
            radius: Some(110.0),
            is_enabled: true,
            ..Default::default()
        };
        let entity = spawn_or_get(&mut world, id);
        world.entity_mut(entity).insert((CapturePoint, CapturePointData(staging)));

        let placed = WorldPos::new(-277.0, 0.0, 336.4);
        handle_non_volatile_position(id, placed, &mut world);
        handle_entity_property_zone(id, "radius", &ArgValue::Float32(80.0), &mut world);

        let cp = &world.entity(entity).get::<CapturePointData>().unwrap().0;
        assert_eq!(cp.position, Some(placed));
        assert_eq!(cp.radius, Some(80.0));
    }
}
