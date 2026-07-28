//! Vehicle property and weapon aim ingestion handlers.

use bevy_ecs::world::World;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wowsunpack::data::Version;
use wowsunpack::game_types::WeaponType;
use wowsunpack::recognized::Recognized;
use wowsunpack::rpc::typedefs::ArgValue;

use crate::components::Aim;
use crate::components::Vehicle;
use crate::components::VehicleState;
use crate::resources::BURN_MASK;
use crate::resources::BurnStateChange;
use crate::resources::BurnStateLog;
use crate::resources::EntityIndex;
use crate::units::Radians;

/// Update `VehicleState` for a known vehicle entity from an EntityProperty packet.
///
/// Also handles `targetLocalPos` -> `Aim.target_yaw` (packed lo-byte decode).
pub fn handle_vehicle_property(
    entity_id: EntityId,
    property: &str,
    value: &ArgValue<'_>,
    world: &mut World,
    version: Version,
    constants: &GameConstants,
    clock: GameClock,
) {
    let Some(ecs_entity) = world.resource::<EntityIndex>().get(entity_id) else {
        return;
    };

    // Guard: only vehicles carry VehicleState.
    let is_vehicle = world.get_entity(ecs_entity).map(|er| er.contains::<Vehicle>()).unwrap_or(false);
    if !is_vehicle {
        return;
    }

    // The entity borrow (er/vs) must be dropped before BurnStateLog can be
    // borrowed, so the pending change is staged here and pushed afterward.
    let mut burn_change: Option<BurnStateChange> = None;
    if let Ok(mut er) = world.get_entity_mut(ecs_entity)
        && let Some(mut vs) = er.get_mut::<VehicleState>()
    {
        // Compare masked values so a flood or acid bit change does not log as a
        // fire transition; bits 4-9 share this property (ma779114d BURN_MASK).
        let previous = vs.0.burning_flags() & BURN_MASK;
        vs.0.update_by_name(property, value, version, constants);
        let current = vs.0.burning_flags() & BURN_MASK;
        if previous != current {
            burn_change = Some(BurnStateChange { victim: entity_id, clock, previous, current });
        }
    }
    if let Some(change) = burn_change {
        world.resource_mut::<BurnStateLog>().0.push(change);
    }

    // targetLocalPos: lo byte encodes world-space yaw as (lo/256)*TAU - PI.
    if property == "targetLocalPos"
        && let Some(val) = value.as_i64()
    {
        let lo = (val & 0xFF) as f32;
        let yaw = (lo / 256.0) * std::f32::consts::TAU - std::f32::consts::PI;
        if let Ok(mut er) = world.get_entity_mut(ecs_entity) {
            let mut aim = er.get_mut::<Aim>();
            if let Some(ref mut a) = aim {
                a.target_yaw = Some(Radians(yaw));
            } else {
                er.insert(Aim {
                    turret_yaws: Vec::new(),
                    target_yaw: Some(Radians(yaw)),
                    selected_ammo: std::collections::HashMap::new(),
                });
            }
        }
    }
}

/// Handle a GunSync packet: update main battery turret yaws on the vehicle's `Aim`.
pub fn handle_gun_sync(entity_id: EntityId, weapon_type: u32, gun_id: u32, yaw: f32, world: &mut World) {
    if WeaponType::from_raw(weapon_type) != Recognized::Known(WeaponType::Artillery) {
        return;
    }
    let Some(ecs_entity) = world.resource::<EntityIndex>().get(entity_id) else {
        return;
    };
    if let Ok(mut er) = world.get_entity_mut(ecs_entity) {
        let idx = gun_id as usize;
        let mut aim = er.get_mut::<Aim>();
        if let Some(ref mut a) = aim {
            if a.turret_yaws.len() <= idx {
                a.turret_yaws.resize(idx + 1, Radians(0.0));
            }
            a.turret_yaws[idx] = Radians(yaw);
        } else {
            let mut turret_yaws = Vec::new();
            turret_yaws.resize(idx + 1, Radians(0.0));
            turret_yaws[idx] = Radians(yaw);
            er.insert(Aim { turret_yaws, target_yaw: None, selected_ammo: std::collections::HashMap::new() });
        }
    }
}

/// Handle a SetAmmoForWeapon packet: record selected ammo keyed by weapon type.
///
/// The game sends this only for SELECTABLE_AMMO_WEAPONS (artillery, torpedo, air support).
pub fn handle_set_ammo_for_weapon(
    entity_id: EntityId,
    weapon_type: u32,
    ammo_param_id: GameParamId,
    world: &mut World,
) {
    let key = WeaponType::from_raw(weapon_type);
    let Some(ecs_entity) = world.resource::<EntityIndex>().get(entity_id) else {
        return;
    };
    if let Ok(mut er) = world.get_entity_mut(ecs_entity) {
        let mut aim = er.get_mut::<Aim>();
        if let Some(ref mut a) = aim {
            a.selected_ammo.insert(key, ammo_param_id);
        } else {
            let mut selected_ammo = std::collections::HashMap::new();
            selected_ammo.insert(key, ammo_param_id);
            er.insert(Aim { turret_yaws: Vec::new(), target_yaw: None, selected_ammo });
        }
    }
}

/// Apply a `VehicleProps` update from a `BasePlayerCreate` or `CellPlayerCreate` bundle.
///
/// Mirrors `BattleController::apply_player_create_props`: folds OWN_CLIENT properties
/// (notably `shipConfig` in some replay versions) into the existing `VehicleState`.
pub fn apply_player_create_props(
    entity_id: EntityId,
    props: &std::collections::HashMap<&str, ArgValue<'_>>,
    world: &mut World,
    version: Version,
    constants: &GameConstants,
) {
    let Some(ecs_entity) = world.resource::<EntityIndex>().get(entity_id) else {
        return;
    };
    let is_vehicle = world.get_entity(ecs_entity).map(|er| er.contains::<Vehicle>()).unwrap_or(false);
    if !is_vehicle {
        return;
    }
    if let Ok(mut er) = world.get_entity_mut(ecs_entity)
        && let Some(mut vs) = er.get_mut::<VehicleState>()
    {
        vs.0.update_from_args(props, version, constants);
    }
}

#[cfg(test)]
mod burn_state_tests {
    use bevy_ecs::world::World;
    use wows_replays::analyzer::battle_controller::VehicleProps;
    use wows_replays::game_constants::GameConstants;
    use wows_replays::types::EntityId;
    use wows_replays::types::GameClock;
    use wowsunpack::data::Version;
    use wowsunpack::rpc::typedefs::ArgValue;

    use super::*;
    use crate::components::GameId;
    use crate::resources::BurnStateLog;
    use crate::resources::EntityIndex;

    /// Spawn a vehicle entity carrying `Vehicle` + `VehicleState`, indexed in
    /// `EntityIndex`, matching the shape `handle_vehicle_property` expects.
    fn test_world_with_vehicle(id: EntityId) -> World {
        let mut world = World::new();
        world.insert_resource(EntityIndex::default());
        world.insert_resource(BurnStateLog::default());
        let entity = world.spawn((GameId(id), Vehicle, VehicleState(VehicleProps::default()))).id();
        world.resource_mut::<EntityIndex>().insert(id, entity);
        world
    }

    fn set_burning_flags(world: &mut World, id: EntityId, flags: u16, clock: GameClock) {
        let version = Version::default();
        let constants = GameConstants::defaults();
        handle_vehicle_property(id, "burningFlags", &ArgValue::Uint16(flags), world, version, &constants, clock);
    }

    /// Only burn bits (0-3) produce entries. A flood starting must not read as
    /// a fire: bits 4-7 are floods and share the same property.
    #[test]
    fn only_burn_bits_produce_transitions() {
        let mut world = test_world_with_vehicle(EntityId::from(7u32));

        set_burning_flags(&mut world, EntityId::from(7u32), 0b0000_0001, GameClock(10.0));
        set_burning_flags(&mut world, EntityId::from(7u32), 0b0001_0001, GameClock(11.0));
        set_burning_flags(&mut world, EntityId::from(7u32), 0b0001_0101, GameClock(12.0));

        let log = &world.resource::<BurnStateLog>().0;
        assert_eq!(log.len(), 2, "the flood-only change must not log: {log:?}");
        assert_eq!(log[0].current, 0b0001);
        // Pins the recorded clock to the value passed to handle_vehicle_property,
        // not the Clock resource (which this test never touches).
        assert_eq!(log[0].clock, GameClock(10.0));
        assert_eq!(log[1].previous, 0b0001);
        assert_eq!(log[1].current, 0b0101);
        assert_eq!(log[1].clock, GameClock(12.0));
    }

    #[test]
    fn newly_lit_reports_only_the_rising_bits() {
        let change =
            BurnStateChange { victim: EntityId::from(1u32), clock: GameClock(1.0), previous: 0b0001, current: 0b0101 };
        let lit: Vec<u8> = change.newly_lit().map(|n| n.get()).collect();
        assert_eq!(lit, vec![2]);
    }

    /// Several bits can rise in one change (e.g. a burst hit igniting more than
    /// one section). Asserts both the full set and the yielded order, which a
    /// consumer may rely on.
    #[test]
    fn newly_lit_reports_every_rising_bit_in_order() {
        let change =
            BurnStateChange { victim: EntityId::from(1u32), clock: GameClock(1.0), previous: 0b0000, current: 0b1011 };
        let lit: Vec<u8> = change.newly_lit().map(|n| n.get()).collect();
        assert_eq!(lit, vec![0, 1, 3]);
    }

    /// A fire going out is a transition too: the DCP model reads extinguish
    /// events, so a falling edge must not be dropped.
    #[test]
    fn extinguish_is_logged() {
        let mut world = test_world_with_vehicle(EntityId::from(7u32));
        set_burning_flags(&mut world, EntityId::from(7u32), 0b0011, GameClock(10.0));
        set_burning_flags(&mut world, EntityId::from(7u32), 0b0000, GameClock(15.0));

        let log = &world.resource::<BurnStateLog>().0;
        assert_eq!(log.len(), 2);
        assert_eq!(log[1].previous, 0b0011);
        assert_eq!(log[1].current, 0b0000);
    }
}
