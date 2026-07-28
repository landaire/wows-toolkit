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
use wows_replays::analyzer::battle_controller::state::VictimPose;
use wows_replays::analyzer::decoder::ArtillerySalvo;
use wows_replays::analyzer::decoder::ShotHit;
use wows_replays::analyzer::decoder::TorpedoData;
use wows_replays::types::AvatarId;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::Direction;
use wowsunpack::game_types::ShotId;
use wowsunpack::game_types::Vec3;
use wowsunpack::game_types::WorldPos;

use crate::components::GameId;
use crate::components::MinimapPlacement;
use crate::components::Projectile;
use crate::components::ProjectileState;
use crate::components::Transform3d;
use crate::ids::IngestOptions;
use crate::ids::ShotTracking;
use crate::resources::ActiveShotOrder;
use crate::resources::ActiveTorpedoOrder;
use crate::resources::HitHistoryLog;
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
        let entity = world.spawn((Projectile, ProjectileState::Artillery { salvo, fired_at: clock, avatar_id })).id();
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
            .spawn((Projectile, ProjectileState::Torpedo { torpedo, launched_at: clock, updated_at: clock, avatar_id }))
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
        torpedo.direction = Direction(Vec3::new(speed * target_yaw.sin(), 0.0, speed * target_yaw.cos()));
    } else if (speed_coef - 1.0).abs() > 1e-6 {
        let dir_norm = torpedo.direction.0 * (1.0 / base_speed);
        torpedo.direction = Direction(dir_norm * speed);
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
///
/// When `options.record_hit_history` is set, each resolved hit is also pushed
/// into `HitHistoryLog`, which (unlike `ShotHitLog`) is never cleared.
pub fn handle_shot_kills(
    avatar_id: AvatarId,
    hits: Vec<ShotHit>,
    clock: GameClock,
    world: &mut World,
    options: &IngestOptions,
) {
    let record = options.shot_tracking == ShotTracking::Tracked;

    let self_ship_id =
        world.resource::<PlayerIndex>().0.iter().find(|(_, p)| p.relation().is_self()).map(|(eid, _)| *eid);

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

        let victim_entity_id = resolve_victim(world, hit.position).unwrap_or(self_ship_id);

        let resolved = ResolvedShotHit {
            clock,
            hit,
            victim_entity_id,
            salvo,
            fired_at,
            victim_pose: victim_pose(world, victim_entity_id),
        };
        if options.record_hit_history {
            world.resource_mut::<HitHistoryLog>().0.push(resolved.clone());
        }
        world.resource_mut::<ShotHitLog>().0.push(resolved);
    }

    expire_stale_salvos(world, clock);
}

fn torpedo_matches(state: &ProjectileState, owner_id: EntityId, shot_id: ShotId) -> bool {
    match state {
        ProjectileState::Torpedo { torpedo, .. } => torpedo.owner_id == owner_id && torpedo.shot_id == shot_id,
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

/// Resolve the victim entity as the ship whose last known position is closest in
/// XZ to where this shell landed.
///
/// `receiveShotKills` names no victim, so this is a guess either way, but the
/// impact position is the shell's own and the aim point is not: it is where the
/// guns were laid when the salvo was fired, seconds of flight earlier, shared by
/// every shell in the salvo. Resolving from it keyed a whole salvo to one ship,
/// so a salvo straddling two ships recorded every shell against whichever of
/// them the average happened to favour. Per-hit resolution from the impact
/// removes that, and leaves the ordinary nearest-ship failures: the victim's
/// position is its last known one rather than its position at impact, and a
/// shell landing between two ships in a tight formation can sit nearer the one
/// it missed.
///
/// `None` when no entity in the world carries a position to compare against,
/// leaving the caller to fall back to the self ship.
fn resolve_victim(world: &mut World, impact: WorldPos) -> Option<EntityId> {
    let mut q = world.query::<(&GameId, &Transform3d)>();
    q.iter(world)
        .min_by(|(_, a), (_, b)| {
            let da = a.pos.distance_xz(&impact);
            let db = b.pos.distance_xz(&impact);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(gid, _)| gid.0)
}

/// Get the victim's world pose at impact, preferring minimap-derived yaw over
/// the `Transform3d` yaw.
///
/// `None` for a victim with no `Transform3d`: `handle_entity_leave` strips it
/// when a vehicle leaves the client's AOI while keeping `MinimapPlacement`, so
/// a departed ship still has a live heading and no position at all. Reporting
/// the pose as absent is the only honest answer there; a zero position reads
/// as a ship at map centre and puts the impact hundreds of units off the hull.
fn victim_pose(world: &mut World, victim: EntityId) -> Option<VictimPose> {
    let entity = world.resource::<crate::resources::EntityIndex>().get(victim)?;
    let er = world.get_entity(entity).ok()?;
    let transform = er.get::<Transform3d>()?;

    let yaw = er
        .get::<MinimapPlacement>()
        .map(|m| std::f32::consts::FRAC_PI_2 - m.heading.0.to_radians())
        .unwrap_or(transform.yaw.0);

    Some(VictimPose { position: transform.pos, yaw, pitch: transform.pitch.0, roll: transform.roll.0 })
}

/// Drop salvos fired more than 30s before `clock`, mirroring the original's
/// retain on active_shots.
fn expire_stale_salvos(world: &mut World, clock: GameClock) {
    let cutoff = clock.seconds() - 30.0;
    let order = world.resource::<ActiveShotOrder>().0.clone();
    let mut kept: Vec<Entity> = Vec::with_capacity(order.len());
    for entity in order {
        let fired_at =
            world.get_entity(entity).ok().and_then(|er| er.get::<ProjectileState>().and_then(salvo_fired_at));
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

#[cfg(test)]
mod victim_pose_tests {
    use wows_replays::types::NormalizedPos;
    use wowsunpack::game_types::Vec2;

    use super::*;
    use crate::components::MinimapPlacement;
    use crate::components::Transform3d;
    use crate::ingest::entities::handle_entity_leave;
    use crate::resources::EntityIndex;
    use crate::resources::PresenceLog;
    use crate::units::Degrees;
    use crate::units::Radians;

    fn world_with_victim(id: EntityId) -> World {
        let mut world = World::new();
        world.insert_resource(EntityIndex::default());
        world.insert_resource(PresenceLog::default());
        let entity = world
            .spawn((
                GameId(id),
                crate::components::Vehicle,
                Transform3d {
                    pos: WorldPos::new(120.0, 0.0, -40.0),
                    yaw: Radians(0.5),
                    pitch: Radians(0.1),
                    roll: Radians(0.2),
                    last_updated: GameClock(10.0),
                },
                MinimapPlacement {
                    pos: NormalizedPos(Vec2::new(0.5, 0.5)),
                    heading: Degrees(90.0),
                    visible: true,
                    visibility_flags: None,
                    is_invisible: false,
                    last_updated: GameClock(10.0),
                },
            ))
            .id();
        world.resource_mut::<EntityIndex>().insert(id, entity);
        world
    }

    /// The live case: position and orientation come off the transform, yaw off
    /// the minimap heading.
    #[test]
    fn a_tracked_victim_has_a_pose() {
        let id = EntityId::from(5u32);
        let mut world = world_with_victim(id);

        let pose = victim_pose(&mut world, id).expect("a victim with a transform has a pose");
        assert_eq!(pose.position, WorldPos::new(120.0, 0.0, -40.0));
        assert_eq!(pose.pitch, 0.1);
        assert_eq!(pose.roll, 0.2);
        assert_eq!(pose.yaw, 0.0, "minimap heading of 90 degrees is a world yaw of 0");
    }

    /// `handle_entity_leave` strips `Transform3d` and keeps `MinimapPlacement`,
    /// so a departed victim would otherwise pair a live yaw with an origin
    /// position, which a section lookup cannot tell apart from a ship sitting
    /// at map centre.
    #[test]
    fn a_departed_victim_has_no_pose() {
        let id = EntityId::from(5u32);
        let mut world = world_with_victim(id);

        handle_entity_leave(id, GameClock(20.0), &mut world);

        assert!(victim_pose(&mut world, id).is_none());
    }

    /// An id that never resolved to an entity has nothing to report either.
    #[test]
    fn an_unknown_victim_has_no_pose() {
        let mut world = world_with_victim(EntityId::from(5u32));
        assert!(victim_pose(&mut world, EntityId::from(404u32)).is_none());
    }
}
