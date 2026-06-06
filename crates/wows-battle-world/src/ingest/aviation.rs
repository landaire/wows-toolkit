//! Plane squadron and fighter ward ingestion handlers.

use bevy_ecs::world::World;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_replays::types::TeamId;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::game_types::PlaneId;
use wowsunpack::game_types::WorldPos;
use wowsunpack::game_types::WorldPos2D;

use crate::components::Plane;
use crate::components::PlaneState;
use crate::components::Ward;
use crate::components::WardState;
use crate::ids::SourceTeam;
use crate::resources::PlaneIndex;
use crate::resources::WardIndex;

pub fn handle_plane_added(
    entity_id: EntityId,
    plane_id: PlaneId,
    team_id: u32,
    params_id: GameParamId,
    position: WorldPos2D,
    clock: GameClock,
    world: &mut World,
) {
    let entity = spawn_or_get_plane(world, plane_id);
    let state = PlaneState {
        plane_id,
        owner_id: entity_id,
        team_id: TeamId::from(team_id),
        params_id,
        position,
        last_updated: clock,
    };
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(state);
    }
}

pub fn handle_plane_position(plane_id: PlaneId, position: WorldPos2D, clock: GameClock, world: &mut World) {
    let Some(entity) = world.resource::<PlaneIndex>().get(plane_id) else { return };
    if let Ok(mut e) = world.get_entity_mut(entity)
        && let Some(mut state) = e.get_mut::<PlaneState>()
    {
        state.position = position;
        state.last_updated = clock;
    }
}

pub fn handle_plane_removed(plane_id: PlaneId, source_team: SourceTeam, world: &mut World) {
    let owner_team = world
        .resource::<PlaneIndex>()
        .get(plane_id)
        .and_then(|ent| world.get_entity(ent).ok())
        .and_then(|e| e.get::<PlaneState>())
        .map(|s| s.team_id);

    if let (Some(src), Some(owner)) = (source_team.0, owner_team)
        && src != owner
    {
        return;
    }

    if let Some(entity) = world.resource_mut::<PlaneIndex>().remove(plane_id)
        && world.get_entity(entity).is_ok()
    {
        world.despawn(entity);
    }
}

pub fn handle_ward_added(
    plane_id: PlaneId,
    position: WorldPos,
    radius: BigWorldDistance,
    owner_id: EntityId,
    world: &mut World,
) {
    let entity = spawn_or_get_ward(world, plane_id);
    let state = WardState { plane_id, position, radius, owner_id };
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(state);
    }
}

pub fn handle_ward_removed(plane_id: PlaneId, world: &mut World) {
    if let Some(entity) = world.resource_mut::<WardIndex>().remove(plane_id)
        && world.get_entity(entity).is_ok()
    {
        world.despawn(entity);
    }
}

fn spawn_or_get_plane(world: &mut World, plane_id: PlaneId) -> bevy_ecs::entity::Entity {
    if let Some(entity) = world.resource::<PlaneIndex>().get(plane_id) {
        return entity;
    }
    let entity = world.spawn(Plane).id();
    world.resource_mut::<PlaneIndex>().insert(plane_id, entity);
    entity
}

fn spawn_or_get_ward(world: &mut World, plane_id: PlaneId) -> bevy_ecs::entity::Entity {
    if let Some(entity) = world.resource::<WardIndex>().get(plane_id) {
        return entity;
    }
    let entity = world.spawn(Ward).id();
    world.resource_mut::<WardIndex>().insert(plane_id, entity);
    entity
}
