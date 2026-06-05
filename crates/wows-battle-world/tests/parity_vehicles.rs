#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use std::collections::HashMap;

use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::types::EntityId;
use wowsunpack::game_types::WeaponType;
use wowsunpack::recognized::Recognized;

fn run_parity(filename: &str) {
    let (old, mut new_world) = support::both(filename);

    let old_entities = old.entities_by_id();
    let old_turret_yaws = old.turret_yaws();
    let old_target_yaws = old.target_yaws();
    let old_selected_ammo = old.selected_ammo();

    // Collect vehicle ids from old controller.
    let vehicle_ids: Vec<EntityId> = old_entities
        .iter()
        .filter_map(|(id, entity)| {
            if entity.vehicle_ref().is_some() { Some(*id) } else { None }
        })
        .collect();

    // Index new world VehicleState and Aim by entity id.
    let new_vehicle_states: HashMap<EntityId, wows_replays::analyzer::battle_controller::VehicleProps> =
        new_world.vehicle_props_all();
    let new_aims: HashMap<EntityId, wows_battle_world::components::Aim> = new_world.aims_all();

    for id in &vehicle_ids {
        // VehicleState parity.
        let old_vehicle = old_entities.get(id).unwrap().vehicle_ref().unwrap();
        let old_props = std::cell::RefCell::borrow(old_vehicle);
        let old_vp = old_props.props();

        let new_vp = new_vehicle_states.get(id).unwrap_or_else(|| {
            panic!("VehicleState missing for id={id:?}");
        });

        assert_eq!(
            old_vp.health(), new_vp.health(),
            "health mismatch id={id:?}: old={} new={}", old_vp.health(), new_vp.health()
        );
        assert_eq!(
            old_vp.max_health(), new_vp.max_health(),
            "max_health mismatch id={id:?}"
        );
        assert_eq!(
            old_vp.is_alive(), new_vp.is_alive(),
            "is_alive mismatch id={id:?}"
        );
        assert_eq!(
            old_vp.is_invisible(), new_vp.is_invisible(),
            "is_invisible mismatch id={id:?}"
        );
        assert_eq!(
            old_vp.visibility_flags(), new_vp.visibility_flags(),
            "visibility_flags mismatch id={id:?}"
        );
        assert_eq!(
            old_vp.team_id(), new_vp.team_id(),
            "team_id mismatch id={id:?}"
        );
        assert_eq!(
            old_vp.owner(), new_vp.owner(),
            "owner mismatch id={id:?}"
        );
        assert_eq!(
            old_vp.selected_weapon(), new_vp.selected_weapon(),
            "selected_weapon mismatch id={id:?}"
        );
        assert_eq!(
            old_vp.ship_config().ship_params_id(), new_vp.ship_config().ship_params_id(),
            "ship_config ship_params_id mismatch id={id:?}"
        );
        assert_eq!(
            old_vp.crew_modifiers_compact_params().params_id(),
            new_vp.crew_modifiers_compact_params().params_id(),
            "crew params_id mismatch id={id:?}"
        );

        // Aim parity.
        let empty_yaws: Vec<f32> = Vec::new();
        let old_turrets = old_turret_yaws.get(id).unwrap_or(&empty_yaws);
        let old_target = old_target_yaws.get(id).copied();
        let old_ammo = old_selected_ammo.get(id).copied();

        let aim = new_aims.get(id);

        let new_turrets: Vec<f32> = aim
            .map(|a| a.turret_yaws.iter().map(|r| r.0).collect())
            .unwrap_or_default();
        let new_target: Option<f32> = aim.and_then(|a| a.target_yaw.map(|r| r.0));
        let new_ammo = aim.and_then(|a| {
            a.selected_ammo.get(&Recognized::Known(WeaponType::Artillery)).copied()
        });

        assert_eq!(
            old_turrets.as_slice(), new_turrets.as_slice(),
            "turret_yaws mismatch id={id:?}: old={old_turrets:?} new={new_turrets:?}"
        );
        assert_eq!(
            old_target, new_target,
            "target_yaw mismatch id={id:?}: old={old_target:?} new={new_target:?}"
        );
        assert_eq!(
            old_ammo, new_ammo,
            "selected_ammo mismatch id={id:?}: old={old_ammo:?} new={new_ammo:?}"
        );
    }
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_vehicles_vermont_pvp() {
    run_parity("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_8260685)), ignore)]
fn parity_vehicles_v170_pvp() {
    run_parity("20240422_161541_PGSD104-V-170_08_NE_passage.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn parity_vehicles_shimakaze_v0_9() {
    run_parity("20200117_205708_PJSD012-Shimakaze-1943_45_Zigzag.wowsreplay");
}
