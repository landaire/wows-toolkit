//! Position and minimap ingestion handlers.

use bevy_ecs::world::World;
use wows_replays::analyzer::decoder::MinimapUpdate;
use wows_replays::packet2::PlayerOrientationPacket;
use wows_replays::packet2::PositionPacket;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::WorldPos;

use crate::components::{
    GameId, MinimapPlacement, SmokeScreen, SmokeScreenState, Transform3d, VehicleState,
    WeatherZone, WeatherZoneData,
};
use crate::ids::SourceTeam;
use crate::resources::EntityIndex;
use crate::units::{Degrees, Radians, VisibilityFlags};

/// Handle a Position packet: update Transform3d for the entity.
pub fn handle_position(pos: &PositionPacket, world: &mut World, clock: GameClock) {
    let entity = spawn_or_get(world, pos.pid);
    let t = Transform3d {
        pos: WorldPos { x: pos.position.x, y: pos.position.y, z: pos.position.z },
        yaw: Radians(pos.rotation.yaw),
        pitch: Radians(pos.rotation.pitch),
        roll: Radians(pos.rotation.roll),
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
pub fn handle_player_orientation(
    orient: &PlayerOrientationPacket,
    world: &mut World,
    clock: GameClock,
) {
    if orient.parent_id != EntityId::from(0u32) {
        return;
    }
    let entity = spawn_or_get(world, orient.pid);
    let t = Transform3d {
        pos: WorldPos { x: orient.position.x, y: orient.position.y, z: orient.position.z },
        yaw: Radians(orient.rotation.yaw),
        pitch: Radians(orient.rotation.pitch),
        roll: Radians(orient.rotation.roll),
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
pub fn handle_minimap_updates(
    updates: &[MinimapUpdate],
    world: &mut World,
    clock: GameClock,
    source_team: SourceTeam,
) {
    for update in updates {
        // Source-team filter.
        if let Some(team) = source_team.0
            && let Some(ecs_entity) = world.resource::<EntityIndex>().get(update.entity_id)
                && let Ok(er) = world.get_entity(ecs_entity)
                    && let Some(vs) = er.get::<VehicleState>() {
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
            prev.unwrap_or((update.position, Degrees(update.heading)))
        } else {
            (update.position, Degrees(update.heading))
        };

        // Pull visibility_flags and is_invisible from the vehicle's current VehicleState.
        let (visibility_flags, is_invisible) = world
            .resource::<EntityIndex>()
            .get(update.entity_id)
            .and_then(|e| world.get_entity(e).ok())
            .and_then(|er| {
                er.get::<VehicleState>()
                    .map(|vs| (VisibilityFlags(vs.0.visibility_flags()), vs.0.is_invisible()))
            })
            .unwrap_or((VisibilityFlags(0), false));

        let placement = MinimapPlacement {
            pos: position,
            heading,
            visible,
            visibility_flags,
            is_invisible,
            last_updated: clock,
        };

        let entity = spawn_or_get(world, update.entity_id);
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert(placement);
        }
    }
}

/// Handle NonVolatilePosition: update SmokeScreen or WeatherZone entity position.
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
