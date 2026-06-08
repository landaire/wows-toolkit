//! Secondary (ATBA) fire ingestion.
//!
//! Expands a per-gun fire bitmask against the shooter's synced atbaTargets and
//! appends plain SecondaryShot records to ActiveSecondaryShots, pruning stale
//! ones so the list stays bounded. Ammo and positions are resolved later by the
//! renderer.

use bevy_ecs::world::World;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::GunBits;
use wowsunpack::game_types::GunId;
use wowsunpack::game_types::WeaponType;
use wowsunpack::recognized::Recognized;

use crate::components::VehicleState;
use crate::ids::ShotTracking;
use crate::resources::ActiveSecondaryShots;
use crate::resources::EntityIndex;
use crate::resources::SecondaryShot;

/// Seconds after which a recorded secondary shot is dropped so the list stays
/// bounded. Comfortably exceeds any secondary flight time.
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
    // atbaTargets is indexed by gun id; 0 means no target. Collect owned so the
    // immutable world borrow ends before taking the resource mutably.
    let targets: Vec<EntityId> = vehicle.0.atba_targets().iter().map(|&t| EntityId::from(t)).collect();

    let new_shots = firing_targets(gun_bits, &targets);
    if new_shots.is_empty() {
        return;
    }

    let cutoff = clock.seconds() - SECONDARY_SHOT_TTL_S;
    let mut active = world.resource_mut::<ActiveSecondaryShots>();
    active.0.retain(|s| s.fired_at.seconds() > cutoff);
    for (gun, target) in new_shots {
        // Dots share the ship-center origin, so guns firing at the same target
        // this clock trace identical paths. Collapsing them (keyed by shooter,
        // target, fired_at) yields the intended single-line look and dedupes a
        // fire reported by both shootOnClient and shootATBAGuns.
        if active.0.iter().any(|s| s.shooter == shooter && s.target == target && s.fired_at == clock) {
            continue;
        }
        active.0.push(SecondaryShot { shooter, target, fired_at: clock, gun });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fired_guns_with_targets_are_selected() {
        let targets = vec![EntityId::from(50u32), EntityId::from(0u32), EntityId::from(60u32), EntityId::from(70u32)];
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
