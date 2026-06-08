//! Secondary (ATBA) fire ingestion.
//!
//! Expands a per-gun fire bitmask against the shooter's synced atbaTargets and
//! records one SecondaryShotState entity per firing gun that has a target. Guns
//! with no target (atbaTargets[gunID] == 0) are skipped. Stale shots are pruned
//! so the ordered list does not grow without bound. Ammo and positions are
//! resolved later by the renderer.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::GunBits;
use wowsunpack::game_types::GunId;
use wowsunpack::game_types::WeaponType;
use wowsunpack::recognized::Recognized;

use crate::components::SecondaryShotState;
use crate::components::VehicleState;
use crate::ids::ShotTracking;
use crate::resources::ActiveSecondaryShotOrder;
use crate::resources::EntityIndex;

/// Seconds after which a recorded secondary shot is dropped. Comfortably exceeds
/// any secondary flight time.
const SECONDARY_SHOT_TTL_S: f32 = 30.0;

pub fn handle_weapon_fired(
    shooter: EntityId,
    weapon_type: Recognized<WeaponType, u32>,
    gun_bits: GunBits,
    clock: GameClock,
    world: &mut World,
    tracking: ShotTracking,
) {
    if tracking != ShotTracking::Tracked {
        return;
    }
    // Only secondaries drive this path; ARTILLERY shootOnClient keeps its own
    // receiveArtilleryShots path.
    if weapon_type != Recognized::Known(WeaponType::Secondaries) {
        return;
    }

    let Some(shooter_entity) = world.resource::<EntityIndex>().get(shooter) else {
        return;
    };
    let Ok(shooter_ref) = world.get_entity(shooter_entity) else {
        return;
    };
    let Some(vehicle) = shooter_ref.get::<VehicleState>() else {
        return;
    };
    // atbaTargets is indexed by gun id; 0 means no target.
    let targets: Vec<EntityId> =
        vehicle.0.atba_targets().iter().map(|&t| EntityId::from(t)).collect();

    expire_stale(world, clock);

    for (_gun, target) in firing_targets(gun_bits, &targets) {
        // Dots share the ship-center origin, so guns firing at the same target
        // this clock trace identical paths. Collapsing them to one (keyed by
        // shooter, target, fired_at) yields the intended single-line look and
        // also dedupes a fire reported by both shootOnClient and shootATBAGuns.
        if secondary_exists(world, shooter, target, clock) {
            continue;
        }
        let entity = world.spawn(SecondaryShotState { shooter, target, fired_at: clock }).id();
        world.resource_mut::<ActiveSecondaryShotOrder>().0.push(entity);
    }
}

/// (gun, target) pairs for guns that fired and have a non-zero target.
fn firing_targets(gun_bits: GunBits, targets: &[EntityId]) -> Vec<(GunId, EntityId)> {
    gun_bits
        .gun_ids()
        .filter_map(|gun| {
            let target = *targets.get(gun.index())?;
            (target.raw() != 0).then_some((gun, target))
        })
        .collect()
}

fn secondary_exists(world: &mut World, shooter: EntityId, target: EntityId, fired_at: GameClock) -> bool {
    let order = world.resource::<ActiveSecondaryShotOrder>().0.clone();
    order.iter().any(|&e| {
        world
            .get_entity(e)
            .ok()
            .and_then(|er| er.get::<SecondaryShotState>())
            .map(|s| s.shooter == shooter && s.target == target && s.fired_at == fired_at)
            .unwrap_or(false)
    })
}

fn expire_stale(world: &mut World, clock: GameClock) {
    let cutoff = clock.seconds() - SECONDARY_SHOT_TTL_S;
    let order = world.resource::<ActiveSecondaryShotOrder>().0.clone();
    let mut kept: Vec<Entity> = Vec::with_capacity(order.len());
    for entity in order {
        let fired_at = world
            .get_entity(entity)
            .ok()
            .and_then(|er| er.get::<SecondaryShotState>().map(|s| s.fired_at));
        match fired_at {
            Some(fired_at) if fired_at.seconds() > cutoff => kept.push(entity),
            _ => {
                if world.get_entity(entity).is_ok() {
                    world.despawn(entity);
                }
            }
        }
    }
    world.resource_mut::<ActiveSecondaryShotOrder>().0 = kept;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fired_guns_with_targets_are_selected() {
        // guns 0,1,3 fired; gun0 -> 50, gun1 -> 0 (none), gun3 -> 70.
        let targets = vec![
            EntityId::from(50u32),
            EntityId::from(0u32),
            EntityId::from(60u32),
            EntityId::from(70u32),
        ];
        let pairs = firing_targets(GunBits::from(0b1011u32), &targets);
        let got: Vec<(u32, u32)> = pairs.iter().map(|(g, t)| (g.raw(), t.raw())).collect();
        assert_eq!(got, vec![(0, 50), (3, 70)]);
    }

    #[test]
    fn fired_gun_beyond_targets_len_is_skipped() {
        let targets = vec![EntityId::from(50u32)];
        let pairs = firing_targets(GunBits::from(0b10u32), &targets);
        assert!(pairs.is_empty());
    }
}
