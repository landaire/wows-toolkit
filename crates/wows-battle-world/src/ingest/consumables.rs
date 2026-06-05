use bevy_ecs::world::World;
use wows_replays::analyzer::battle_controller::state::ActiveConsumable;
use wows_replays::analyzer::battle_controller::state::ConsumableInventory;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::ChargeCount;
use wowsunpack::game_types::ConsumableUsageParams;
use wowsunpack::game_types::Consumable;
use wowsunpack::recognized::Recognized;

use crate::components::Consumables;
use crate::components::GameId;
use crate::resources::EntityIndex;

pub fn handle_consumable(
    entity: EntityId,
    consumable: Recognized<Consumable>,
    duration: f32,
    usage_params: Option<ConsumableUsageParams>,
    clock: GameClock,
    world: &mut World,
) {
    let ecs_entity = {
        if let Some(e) = world.resource::<EntityIndex>().get(entity) {
            e
        } else {
            let e = world.spawn((GameId(entity),)).id();
            world.resource_mut::<EntityIndex>().insert(entity, e);
            e
        }
    };

    let activation = ActiveConsumable { consumable: consumable.clone(), activated_at: clock, duration, usage_params };

    let Ok(mut entity_ref) = world.get_entity_mut(ecs_entity) else { return };

    if let Some(mut cons) = entity_ref.get_mut::<Consumables>() {
        cons.active.push(activation);
        if let Some(slot) = pick_inventory_slot(&mut cons.slots, &consumable) {
            slot.charges_used = slot.charges_used.saturating_add(1);
            slot.active_until = Some(GameClock(clock.0 + duration));
        }
    } else {
        entity_ref.insert(Consumables { active: vec![activation], slots: Vec::new() });
    }
}

fn pick_inventory_slot<'a>(
    inv: &'a mut [ConsumableInventory],
    consumable: &Recognized<Consumable>,
) -> Option<&'a mut ConsumableInventory> {
    inv.iter_mut().find(|slot| {
        slot.consumable.known() == consumable.known()
            && match slot.charges_remaining() {
                ChargeCount::Unlimited => true,
                ChargeCount::Finite(n) => n > 0,
            }
    })
}
