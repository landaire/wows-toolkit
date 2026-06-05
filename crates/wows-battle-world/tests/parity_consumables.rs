#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::types::EntityId;

/// Assert that active_consumables and consumable_inventories match between old
/// and new controllers after full packet processing with seeded inventories.
///
/// Validated: ActiveConsumable (consumable, activated_at, duration, usage_params)
/// and ConsumableInventory (slot_index, consumable, charges_used, active_until,
/// work_time, reload_time, total_charges, consumable_type_raw, icon_key).
/// Both structs derive PartialEq so field-by-field equality is exact.
///
/// Inventory seeding: gather_replay_facts + build_inventory_from_facts (same path
/// the toolkit renderer uses). Both controllers receive identical slot definitions
/// before the packet loop runs, so charges_used and active_until reflect real
/// activations observed in the replay.
fn run_parity(filename: &str) {
    let (old, mut new_world) = support::both_seeded(filename);

    // Guard: both sides must have at least one entity with a seeded inventory.
    assert!(
        !old.consumable_inventories().is_empty(),
        "consumable_inventories must be non-empty after seeding in {filename}",
    );
    assert!(
        !new_world.consumable_inventories().is_empty(),
        "new consumable_inventories must be non-empty after seeding in {filename}",
    );

    let old_active = old.active_consumables();
    let new_active = new_world.active_consumables();

    let mut old_active_ids: Vec<EntityId> = old_active.keys().copied().collect();
    let mut new_active_ids: Vec<EntityId> = new_active.keys().copied().collect();
    old_active_ids.sort();
    new_active_ids.sort();
    assert_eq!(
        old_active_ids, new_active_ids,
        "active_consumables key sets differ in {filename}: old_only={:?} new_only={:?}",
        old_active_ids.iter().filter(|k| !new_active_ids.contains(k)).collect::<Vec<_>>(),
        new_active_ids.iter().filter(|k| !old_active_ids.contains(k)).collect::<Vec<_>>(),
    );

    for (entity_id, old_activations) in old_active {
        let new_activations = new_active
            .get(entity_id)
            .unwrap_or_else(|| panic!("active_consumables missing id={entity_id:?} in {filename}"));
        assert_eq!(
            old_activations.len(),
            new_activations.len(),
            "active_consumables[{entity_id:?}] len mismatch in {filename}: old={} new={}",
            old_activations.len(),
            new_activations.len(),
        );
        for (i, (o, n)) in old_activations.iter().zip(new_activations.iter()).enumerate() {
            assert_eq!(
                o, n,
                "active_consumables[{entity_id:?}][{i}] mismatch in {filename}: old={o:?} new={n:?}",
            );
        }
    }

    let old_inv = old.consumable_inventories();
    let new_inv = new_world.consumable_inventories();

    let mut old_inv_ids: Vec<EntityId> = old_inv.keys().copied().collect();
    let mut new_inv_ids: Vec<EntityId> = new_inv.keys().copied().collect();
    old_inv_ids.sort();
    new_inv_ids.sort();
    assert_eq!(
        old_inv_ids, new_inv_ids,
        "consumable_inventories key sets differ in {filename}: old_only={:?} new_only={:?}",
        old_inv_ids.iter().filter(|k| !new_inv_ids.contains(k)).collect::<Vec<_>>(),
        new_inv_ids.iter().filter(|k| !old_inv_ids.contains(k)).collect::<Vec<_>>(),
    );

    for (entity_id, old_slots) in old_inv {
        let new_slots = new_inv
            .get(entity_id)
            .unwrap_or_else(|| panic!("consumable_inventories missing id={entity_id:?} in {filename}"));
        assert_eq!(
            old_slots.len(),
            new_slots.len(),
            "consumable_inventories[{entity_id:?}] slot count mismatch in {filename}: old={} new={}",
            old_slots.len(),
            new_slots.len(),
        );
        for (i, (o, n)) in old_slots.iter().zip(new_slots.iter()).enumerate() {
            assert_eq!(
                o, n,
                "consumable_inventories[{entity_id:?}][{i}] mismatch in {filename}: old={o:?} new={n:?}",
            );
        }
    }
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_consumables_vermont_pvp() {
    run_parity("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_consumables_narai_operation() {
    run_parity("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_consumables_marceau_pvp() {
    run_parity("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
}
