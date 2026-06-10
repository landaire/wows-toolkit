//! Ingestion handlers for kill, damage, ribbon, and self-stat packets.

use bevy_ecs::world::World;
use wows_replays::analyzer::battle_controller::DamageEvent;
use wows_replays::analyzer::battle_controller::state::DeadShip;
use wows_replays::analyzer::battle_controller::state::KillRecord;
use wows_replays::analyzer::decoder::DamageReceived as AggressorDamage;
use wows_replays::analyzer::decoder::DamageStatEntry;
use wows_replays::analyzer::decoder::DeathCause;
use wows_replays::analyzer::decoder::Recognized;
use wows_replays::nested_property_path::PropertyNestLevel;
use wows_replays::nested_property_path::UpdateAction;
use wows_replays::packet2::PropertyUpdatePacket;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::NormalizedPos;
use wows_replays::types::WorldPos;
use wowsunpack::game_types::Ribbon;
use wowsunpack::rpc::typedefs::ArgValue;

use crate::components::MinimapPlacement;
use crate::components::Transform3d;
use crate::resources::DamageLedger;
use crate::resources::DeadShips;
use crate::resources::EntityIndex;
use crate::resources::KillLog;
use crate::resources::SelfStats;

/// Increment the ribbon count for the self player (legacy `onRibbon` path,
/// pre-modern replays).
pub fn handle_ribbon(ribbon: Ribbon, world: &mut World) {
    let mut self_stats = world.resource_mut::<SelfStats>();
    *self_stats.ribbons.entry(ribbon).or_insert(0) += 1;
}

/// Apply a nested update to the avatar's `privateVehicleState.ribbons` array
/// (modern replays). Elements are `{ribbonId, count}` where `count` is the
/// absolute running total per ribbon; `SetRange`/`SetElement` add an element,
/// `SetKey count` bumps it. We mirror the array in `SelfStats.ribbon_slots`,
/// then rebuild `SelfStats.ribbons` via [`Ribbon::from_id`].
pub fn handle_ribbon_property_update(update: &PropertyUpdatePacket<'_>, world: &mut World) {
    // `privateVehicleState` is an OWN_CLIENT property: the server only sends it to
    // the recording player's own avatar, so any such update belongs to the self
    // player. In merged (multi-replay) mode, secondary-perspective property
    // updates are already filtered to vehicle entities, and avatars carry no
    // vehicle component, so other players' private state never reaches here.
    if update.property != "privateVehicleState" {
        return;
    }
    let levels = &update.update_cmd.levels;
    if !matches!(levels.first(), Some(PropertyNestLevel::DictKey("ribbons"))) {
        return;
    }

    let mut stats = world.resource_mut::<SelfStats>();
    match (&levels[1..], &update.update_cmd.action) {
        ([], UpdateAction::SetRange { start, values, .. }) => {
            for (offset, value) in values.iter().enumerate() {
                if let Some(elem) = ribbon_element(value) {
                    set_ribbon_slot(&mut stats.ribbon_slots, start + offset, elem);
                }
            }
        }
        ([], UpdateAction::SetElement { index, value }) => {
            if let Some(elem) = ribbon_element(value) {
                set_ribbon_slot(&mut stats.ribbon_slots, *index, elem);
            }
        }
        ([PropertyNestLevel::ArrayIndex(index)], UpdateAction::SetKey { key: "count", value }) => {
            if let (Some(slot), Some(count)) = (stats.ribbon_slots.get_mut(*index), value.as_i32()) {
                slot.1 = count.max(0) as usize;
            }
        }
        _ => return,
    }

    let mut rebuilt: std::collections::HashMap<Ribbon, usize> = std::collections::HashMap::new();
    for (ribbon_id, count) in &stats.ribbon_slots {
        if *count > 0 {
            *rebuilt.entry(Ribbon::from_id(*ribbon_id)).or_insert(0) += *count;
        }
    }
    stats.ribbons = rebuilt;
}

/// Extract `(ribbonId, count)` from a `{ribbonId, count}` array element.
fn ribbon_element(value: &ArgValue<'_>) -> Option<(i32, usize)> {
    let map = match value {
        ArgValue::FixedDict(map) => map,
        ArgValue::NullableFixedDict(Some(map)) => map,
        _ => return None,
    };
    let ribbon_id = map.get("ribbonId")?.as_i32()?;
    let count = map.get("count")?.as_i32()?.max(0) as usize;
    Some((ribbon_id, count))
}

fn set_ribbon_slot(slots: &mut Vec<(i32, usize)>, index: usize, elem: (i32, usize)) {
    if index >= slots.len() {
        slots.resize(index + 1, (0, 0));
    }
    slots[index] = elem;
}

/// Replace (or insert) a damage-stat entry for the self player.
pub fn handle_damage_stat(entries: &[DamageStatEntry], world: &mut World) {
    let mut self_stats = world.resource_mut::<SelfStats>();
    for entry in entries {
        self_stats.damage_stats.insert((entry.weapon.clone(), entry.category.clone()), entry.clone());
    }
}

/// Record a ship kill and the dead ship's last known positions.
pub fn handle_ship_destroyed(
    killer: EntityId,
    victim: EntityId,
    cause: Recognized<DeathCause>,
    clock: GameClock,
    world: &mut World,
) {
    world.resource_mut::<KillLog>().0.push(KillRecord { clock, killer, victim, cause });

    let world_pos: Option<WorldPos> = victim_world_pos(victim, world);
    let minimap_pos: Option<NormalizedPos> = victim_minimap_pos(victim, world);

    world
        .resource_mut::<DeadShips>()
        .0
        .insert(victim, DeadShip { clock, position: world_pos, minimap_position: minimap_pos });
}

/// Append damage events from a DamageReceived packet, keyed by aggressor.
pub fn handle_damage_received(victim: EntityId, aggressors: &[AggressorDamage], clock: GameClock, world: &mut World) {
    let mut ledger = world.resource_mut::<DamageLedger>();
    for dmg in aggressors {
        ledger.0.entry(dmg.aggressor).or_default().push(DamageEvent { amount: dmg.damage, victim, clock });
    }
}

fn victim_world_pos(victim: EntityId, world: &mut World) -> Option<WorldPos> {
    let ecs_entity = world.resource::<EntityIndex>().get(victim)?;
    world.get_entity(ecs_entity).ok()?.get::<Transform3d>().map(|t| t.pos)
}

fn victim_minimap_pos(victim: EntityId, world: &mut World) -> Option<NormalizedPos> {
    let ecs_entity = world.resource::<EntityIndex>().get(victim)?;
    world.get_entity(ecs_entity).ok()?.get::<MinimapPlacement>().map(|m| m.pos)
}

#[cfg(test)]
mod ribbon_property_tests {
    use super::*;
    use std::collections::HashMap;
    use wows_replays::nested_property_path::PropertyNesting;
    use wows_replays::types::EntityId;

    fn ribbon_value(ribbon_id: i8, count: i32) -> ArgValue<'static> {
        let mut map: HashMap<&'static str, ArgValue<'static>> = HashMap::new();
        map.insert("ribbonId", ArgValue::Int8(ribbon_id));
        map.insert("count", ArgValue::Int32(count));
        ArgValue::FixedDict(map)
    }

    fn private_state_update(
        levels: Vec<PropertyNestLevel<'static>>,
        action: UpdateAction<'static>,
    ) -> PropertyUpdatePacket<'static> {
        PropertyUpdatePacket {
            entity_id: EntityId::from(1u32),
            property: "privateVehicleState",
            update_cmd: PropertyNesting { levels, action },
        }
    }

    #[test]
    fn ribbons_accumulate_from_private_vehicle_state() {
        let mut world = World::new();
        world.insert_resource(SelfStats::default());

        // Add penetration (id 15) and fire (id 6), one each.
        let add = private_state_update(
            vec![PropertyNestLevel::DictKey("ribbons")],
            UpdateAction::SetRange { start: 0, stop: 2, values: vec![ribbon_value(15, 1), ribbon_value(6, 1)] },
        );
        handle_ribbon_property_update(&add, &mut world);

        // Bump penetration's running count to 5 via the count SetKey path.
        let bump = private_state_update(
            vec![PropertyNestLevel::DictKey("ribbons"), PropertyNestLevel::ArrayIndex(0)],
            UpdateAction::SetKey { key: "count", value: ArgValue::Int32(5) },
        );
        handle_ribbon_property_update(&bump, &mut world);

        let ribbons = &world.resource::<SelfStats>().ribbons;
        assert_eq!(ribbons.get(&Ribbon::Penetration), Some(&5));
        assert_eq!(ribbons.get(&Ribbon::SetFire), Some(&1));
        assert_eq!(ribbons.len(), 2);
    }

    #[test]
    fn non_ribbon_property_updates_are_ignored() {
        let mut world = World::new();
        world.insert_resource(SelfStats::default());
        let other = PropertyUpdatePacket {
            entity_id: EntityId::from(1u32),
            property: "state",
            update_cmd: PropertyNesting {
                levels: vec![PropertyNestLevel::DictKey("battery")],
                action: UpdateAction::SetKey { key: "energy", value: ArgValue::Float32(1.0) },
            },
        };
        handle_ribbon_property_update(&other, &mut world);
        assert!(world.resource::<SelfStats>().ribbons.is_empty());
    }
}
