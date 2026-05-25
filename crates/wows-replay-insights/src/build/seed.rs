use wowsunpack::data::Version;
use wowsunpack::game_params::types::GameParamProvider;

use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::analyzer::battle_controller::state::ConsumableInventory;
use wows_replays::types::EntityId;

use super::ResolvedBuild;

/// Build a [`ConsumableInventory`] list for one player from their resolved
/// build. Returns an empty Vec when the player has no vehicle entity yet.
pub fn build_inventory_for_player<P: GameParamProvider>(
    player: &Player,
    gp: &P,
    version: Version,
) -> Vec<ConsumableInventory> {
    let Some(build) = ResolvedBuild::from_player(player, gp, version) else {
        return Vec::new();
    };
    build
        .slots
        .iter()
        .map(|slot| ConsumableInventory {
            slot_index: slot.slot_index,
            consumable_type_raw: slot.consumable_type_raw.clone(),
            consumable: slot.consumable_type.clone(),
            icon_key: slot.icon_key.clone(),
            total_charges: slot.total_charges,
            charges_used: 0,
            work_time: slot.work_time.as_secs_f32(),
            reload_time: slot.reload_time.as_secs_f32(),
            active_until: None,
        })
        .collect()
}

/// Walk every player-controlled vehicle in `controller`, build their
/// inventory, and seed it. Entities without a parsed `vehicle_entity` yet
/// are skipped silently; callers can re-invoke after more packets have
/// been processed.
pub fn seed_consumable_inventories<P, G>(
    controller: &mut BattleController<'_, '_, G>,
    gp: &P,
    version: Version,
) where
    P: GameParamProvider,
    G: wowsunpack::data::ResourceLoader,
{
    // Snapshot the (entity_id, Rc<Player>) pairs first so we can mutate the
    // controller while iterating without holding an active borrow. The `Rc`
    // alias resolves to `Arc` when wows_replays is built with `arc`.
    let pairs: Vec<(EntityId, wows_replays::Rc<Player>)> = controller
        .player_entities()
        .iter()
        .map(|(id, p)| (*id, wows_replays::Rc::clone(p)))
        .collect();

    for (entity_id, player) in pairs {
        let inv = build_inventory_for_player(&player, gp, version);
        if !inv.is_empty() {
            controller.set_consumable_inventory(entity_id, inv);
        }
    }
}
