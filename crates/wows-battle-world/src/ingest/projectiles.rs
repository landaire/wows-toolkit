//! Projectile ingestion: artillery salvos, torpedoes, and shot-hit resolution.
//!
//! In-flight projectiles are `Projectile` entities carrying a `ProjectileState`.
//! The authoritative ordering lives in the `ActiveShotOrder` / `ActiveTorpedoOrder`
//! resources (Vec<Entity>), which mirror BattleController.active_shots /
//! active_torpedoes exactly, including swap_remove and retain semantics. Relying
//! on archetype iteration order would diverge from the original Vec order and
//! break both salvo matching and the produced shot_hits sequence.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::analyzer::decoder::ArtillerySalvo;
use wows_replays::analyzer::decoder::ShotHit;
use wows_replays::analyzer::decoder::TorpedoData;
use wows_replays::types::AvatarId;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::ShotId;
use wowsunpack::game_types::WorldPos;

use crate::components::GameId;
use crate::components::MinimapPlacement;
use crate::components::Projectile;
use crate::components::ProjectileState;
use crate::components::Transform3d;
use crate::ids::ShotTracking;
use crate::resources::ActiveShotOrder;
use crate::resources::ActiveTorpedoOrder;
use crate::resources::PlayerIndex;
use crate::resources::ShotHitLog;

/// Spawn one `Projectile` entity per salvo and append to the ordered list.
///
/// Gated on `Tracked`, mirroring BattleController's `track_shots` guard on the
/// ArtilleryShots arm.
pub fn handle_artillery_shots(
    avatar_id: AvatarId,
    salvos: Vec<ArtillerySalvo>,
    clock: GameClock,
    world: &mut World,
    tracking: ShotTracking,
) {
    if tracking != ShotTracking::Tracked {
        return;
    }
    for salvo in salvos {
        let entity = world
            .spawn((Projectile, ProjectileState::Artillery { salvo, fired_at: clock, avatar_id }))
            .id();
        world.resource_mut::<ActiveShotOrder>().0.push(entity);
    }
}

/// Spawn one `Projectile` entity per torpedo and append to the ordered list.
///
/// Not gated on shot tracking: BattleController always records torpedoes.
pub fn handle_torpedoes_received(
    avatar_id: AvatarId,
    torpedoes: Vec<TorpedoData>,
    clock: GameClock,
    world: &mut World,
) {
    for torpedo in torpedoes {
        let entity = world
            .spawn((
                Projectile,
                ProjectileState::Torpedo {
                    torpedo,
                    launched_at: clock,
                    updated_at: clock,
                    avatar_id,
                },
            ))
            .id();
        world.resource_mut::<ActiveTorpedoOrder>().0.push(entity);
    }
}

/// Update a homing torpedo's origin/direction in response to a direction packet.
///
/// `target_yaw` near 2*PI is a sentinel meaning "keep current heading".
pub fn handle_torpedo_direction(
    owner_id: EntityId,
    shot_id: ShotId,
    position: WorldPos,
    target_yaw: f32,
    speed_coef: f32,
    clock: GameClock,
    world: &mut World,
) {
    let order = world.resource::<ActiveTorpedoOrder>().0.clone();
    let target = order.into_iter().find(|&e| {
        world
            .get_entity(e)
            .ok()
            .and_then(|er| er.get::<ProjectileState>().map(|s| torpedo_matches(s, owner_id, shot_id)))
            .unwrap_or(false)
    });

    let Some(entity) = target else { return };
    let Ok(mut er) = world.get_entity_mut(entity) else { return };
    let Some(mut state) = er.get_mut::<ProjectileState>() else { return };
    let ProjectileState::Torpedo { torpedo, updated_at, .. } = &mut *state else { return };

    let base_speed = (torpedo.direction.x.powi(2) + torpedo.direction.z.powi(2)).sqrt();
    let speed = base_speed * speed_coef;
    torpedo.origin = position;
    if (target_yaw - std::f32::consts::TAU).abs() > 0.01 {
        torpedo.direction =
            WorldPos { x: speed * target_yaw.sin(), y: 0.0, z: speed * target_yaw.cos() };
    } else if (speed_coef - 1.0).abs() > 1e-6 {
        let dir_norm = torpedo.direction * (1.0 / base_speed);
        torpedo.direction = dir_norm * speed;
    }
    torpedo.maneuver_dump = None;
    *updated_at = clock;
}

/// Resolve a batch of shot hits against active salvos.
///
/// Mirrors BattleController's ShotKills arm: removes the matched torpedo, matches
/// each hit to its originating salvo, resolves the victim ship and its pose at
/// impact, pushes a `ResolvedShotHit`, then expires salvos older than 30s.
///
/// The ResolvedShotHit recording is suppressed under `Untracked` (the ECS log is
/// never populated in that mode). Torpedo cleanup and salvo expiry still run, so
/// active projectiles do not leak; under Untracked the active lists are empty
/// anyway because the spawn arms that feed them are gated off.
pub fn handle_shot_kills(
    avatar_id: AvatarId,
    hits: Vec<ShotHit>,
    clock: GameClock,
    world: &mut World,
    tracking: ShotTracking,
) {
    let record = tracking == ShotTracking::Tracked;

    let self_ship_id = world
        .resource::<PlayerIndex>()
        .0
        .iter()
        .find(|(_, p)| p.relation().is_self())
        .map(|(eid, _)| *eid);

    let Some(self_ship_id) = self_ship_id else {
        tracing::warn!("ShotKills received but self-player not yet known (avatar={avatar_id:?})");
        return;
    };

    for hit in hits {
        remove_matching_torpedo(world, hit.owner_id, hit.shot_id);

        if !record {
            continue;
        }

        let (salvo, fired_at) = match_active_salvo(world, hit.owner_id, hit.shot_id);

        let victim_entity_id =
            resolve_victim(world, salvo.as_ref()).unwrap_or(self_ship_id);

        let (victim_position, victim_yaw, victim_pitch, victim_roll) =
            victim_pose(world, victim_entity_id);

        world.resource_mut::<ShotHitLog>().0.push(ResolvedShotHit {
            clock,
            hit,
            victim_entity_id,
            salvo,
            fired_at,
            victim_position,
            victim_yaw,
            victim_pitch,
            victim_roll,
        });
    }

    expire_stale_salvos(world, clock);
}

fn torpedo_matches(state: &ProjectileState, owner_id: EntityId, shot_id: ShotId) -> bool {
    match state {
        ProjectileState::Torpedo { torpedo, .. } => {
            torpedo.owner_id == owner_id && torpedo.shot_id == shot_id
        }
        ProjectileState::Artillery { .. } => false,
    }
}

/// Remove the first torpedo matching owner/shot, mirroring `Vec::swap_remove`.
fn remove_matching_torpedo(world: &mut World, owner_id: EntityId, shot_id: ShotId) {
    let order = world.resource::<ActiveTorpedoOrder>().0.clone();
    let idx = order.iter().position(|&e| {
        world
            .get_entity(e)
            .ok()
            .and_then(|er| er.get::<ProjectileState>().map(|s| torpedo_matches(s, owner_id, shot_id)))
            .unwrap_or(false)
    });
    if let Some(idx) = idx {
        let removed = world.resource_mut::<ActiveTorpedoOrder>().0.swap_remove(idx);
        if world.get_entity(removed).is_ok() {
            world.despawn(removed);
        }
    }
}

/// Find the first active salvo whose owner matches and which contains a shell with
/// `shot_id`. Returns a clone of the salvo and its fire time, or `(None, None)`.
fn match_active_salvo(
    world: &mut World,
    owner_id: EntityId,
    shot_id: ShotId,
) -> (Option<ArtillerySalvo>, Option<GameClock>) {
    let order = world.resource::<ActiveShotOrder>().0.clone();
    for entity in order {
        let Ok(er) = world.get_entity(entity) else { continue };
        let Some(state) = er.get::<ProjectileState>() else { continue };
        if let ProjectileState::Artillery { salvo, fired_at, .. } = state
            && salvo.owner_id == owner_id
            && salvo.shots.iter().any(|shot| shot.shot_id == shot_id)
        {
            return (Some(salvo.clone()), Some(*fired_at));
        }
    }
    (None, None)
}

/// Resolve the victim entity as the ship closest to the salvo's average target.
/// Returns `None` when no salvo matched or the salvo has no shots, leaving the
/// caller to fall back to the self ship.
fn resolve_victim(world: &mut World, salvo: Option<&ArtillerySalvo>) -> Option<EntityId> {
    let salvo = salvo?;
    let n = salvo.shots.len() as f32;
    if n < 1.0 {
        return None;
    }
    let avg_target: WorldPos = salvo.shots.iter().map(|sh| sh.target).sum::<WorldPos>() / n;

    let mut q = world.query::<(&GameId, &Transform3d)>();
    q.iter(world)
        .min_by(|(_, a), (_, b)| {
            let da = a.pos.distance_xz(&avg_target);
            let db = b.pos.distance_xz(&avg_target);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(gid, _)| gid.0)
}

/// Get victim world position and orientation at impact, mirroring the original's
/// preference for minimap-derived yaw with a Transform3d fallback.
fn victim_pose(world: &mut World, victim: EntityId) -> (WorldPos, f32, f32, f32) {
    let entity = world.resource::<crate::resources::EntityIndex>().get(victim);
    let Some(entity) = entity else { return (WorldPos::default(), 0.0, 0.0, 0.0) };
    let Ok(er) = world.get_entity(entity) else { return (WorldPos::default(), 0.0, 0.0, 0.0) };

    let transform = er.get::<Transform3d>();
    let position = transform.map(|t| t.pos).unwrap_or_default();
    let (pitch, roll) = transform.map(|t| (t.pitch.0, t.roll.0)).unwrap_or((0.0, 0.0));

    let yaw = er
        .get::<MinimapPlacement>()
        .map(|m| std::f32::consts::FRAC_PI_2 - m.heading.0.to_radians())
        .or_else(|| transform.map(|t| t.yaw.0))
        .unwrap_or(0.0);

    (position, yaw, pitch, roll)
}

/// Drop salvos fired more than 30s before `clock`, mirroring the original's
/// retain on active_shots.
fn expire_stale_salvos(world: &mut World, clock: GameClock) {
    let cutoff = clock.seconds() - 30.0;
    let order = world.resource::<ActiveShotOrder>().0.clone();
    let mut kept: Vec<Entity> = Vec::with_capacity(order.len());
    for entity in order {
        let fired_at = world
            .get_entity(entity)
            .ok()
            .and_then(|er| er.get::<ProjectileState>().and_then(salvo_fired_at));
        match fired_at {
            Some(fired_at) if fired_at.seconds() > cutoff => kept.push(entity),
            _ => {
                if world.get_entity(entity).is_ok() {
                    world.despawn(entity);
                }
            }
        }
    }
    world.resource_mut::<ActiveShotOrder>().0 = kept;
}

fn salvo_fired_at(state: &ProjectileState) -> Option<GameClock> {
    match state {
        ProjectileState::Artillery { fired_at, .. } => Some(*fired_at),
        ProjectileState::Torpedo { .. } => None,
    }
}
