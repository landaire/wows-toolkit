//! Vehicle property and weapon aim ingestion handlers.

use bevy_ecs::world::World;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::EntityId;
use wows_replays::types::GameParamId;
use wowsunpack::data::Version;
use wowsunpack::rpc::typedefs::ArgValue;

use crate::components::{Aim, Vehicle, VehicleState};
use crate::resources::EntityIndex;
use crate::units::{Radians, WeaponGroup};

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
) {
    let Some(ecs_entity) = world.resource::<EntityIndex>().get(entity_id) else {
        return;
    };

    // Guard: only vehicles carry VehicleState.
    let is_vehicle = world
        .get_entity(ecs_entity)
        .map(|er| er.contains::<Vehicle>())
        .unwrap_or(false);
    if !is_vehicle {
        return;
    }

    if let Ok(mut er) = world.get_entity_mut(ecs_entity)
        && let Some(mut vs) = er.get_mut::<VehicleState>()
    {
        vs.0.update_by_name(property, value, version, constants);
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
                drop(aim);
                er.insert(Aim {
                    turret_yaws: Vec::new(),
                    target_yaw: Some(Radians(yaw)),
                    selected_ammo: None,
                });
            }
        }
    }
}

/// Handle a GunSync packet: update main battery turret yaws on the vehicle's `Aim`.
pub fn handle_gun_sync(
    entity_id: EntityId,
    weapon_type: u32,
    gun_id: u32,
    yaw: f32,
    world: &mut World,
) {
    if WeaponGroup(weapon_type) != WeaponGroup::MAIN_BATTERY {
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
            drop(aim);
            let mut turret_yaws = Vec::new();
            turret_yaws.resize(idx + 1, Radians(0.0));
            turret_yaws[idx] = Radians(yaw);
            er.insert(Aim { turret_yaws, target_yaw: None, selected_ammo: None });
        }
    }
}

/// Handle a SetAmmoForWeapon packet: record selected ammo for the main battery.
pub fn handle_set_ammo_for_weapon(
    entity_id: EntityId,
    weapon_type: u32,
    ammo_param_id: GameParamId,
    world: &mut World,
) {
    if WeaponGroup(weapon_type) != WeaponGroup::MAIN_BATTERY {
        return;
    }
    let Some(ecs_entity) = world.resource::<EntityIndex>().get(entity_id) else {
        return;
    };
    if let Ok(mut er) = world.get_entity_mut(ecs_entity) {
        let mut aim = er.get_mut::<Aim>();
        if let Some(ref mut a) = aim {
            a.selected_ammo = Some(ammo_param_id);
        } else {
            drop(aim);
            er.insert(Aim { turret_yaws: Vec::new(), target_yaw: None, selected_ammo: Some(ammo_param_id) });
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
    let is_vehicle = world
        .get_entity(ecs_entity)
        .map(|er| er.contains::<Vehicle>())
        .unwrap_or(false);
    if !is_vehicle {
        return;
    }
    if let Ok(mut er) = world.get_entity_mut(ecs_entity)
        && let Some(mut vs) = er.get_mut::<VehicleState>()
    {
        vs.0.update_from_args(props, version, constants);
    }
}

