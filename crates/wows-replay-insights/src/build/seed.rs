use std::collections::HashMap;

use wowsunpack::data::Version;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;

use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::analyzer::battle_controller::merged::VehicleFacts;
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
pub fn seed_consumable_inventories<P, G>(controller: &mut BattleController<'_, '_, G>, gp: &P, version: Version)
where
    P: GameParamProvider,
    G: wowsunpack::data::ResourceLoader,
{
    // Snapshot the (entity_id, Rc<Player>) pairs first so we can mutate the
    // controller while iterating without holding an active borrow. The `Rc`
    // alias resolves to `Arc` when wows_replays is built with `arc`.
    let pairs: Vec<(EntityId, wows_replays::Rc<Player>)> =
        controller.player_entities().iter().map(|(id, p)| (*id, wows_replays::Rc::clone(p))).collect();

    for (entity_id, player) in pairs {
        let inv = build_inventory_for_player(&player, gp, version);
        if !inv.is_empty() {
            controller.set_consumable_inventory(entity_id, inv);
        }
    }
}

/// Seed inventories from a pre-scanned `VehicleFacts` cache. Use this when
/// the cache was built via `wows_replays::analyzer::battle_controller::merged::gather_replay_facts`,
/// so per-entity ship config is known from any perspective regardless of
/// whether the merged controller has surfaced that entity yet.
pub fn seed_consumable_inventories_from_facts<P, G>(
    controller: &mut BattleController<'_, '_, G>,
    facts: &HashMap<EntityId, VehicleFacts>,
    gp: &P,
    version: Version,
) where
    P: GameParamProvider,
    G: wowsunpack::data::ResourceLoader,
{
    let mut seeded = 0usize;
    let mut empty = 0usize;
    for (entity_id, fact) in facts {
        let inv = build_inventory_from_facts(fact, gp, version);
        if inv.is_empty() {
            empty += 1;
            tracing::debug!(
                ?entity_id,
                vehicle_id = ?fact.vehicle_id,
                abilities = fact.ship_config.abilities().len(),
                "build_inventory_from_facts returned empty"
            );
        } else {
            seeded += 1;
            controller.set_consumable_inventory(*entity_id, inv);
        }
    }
    tracing::info!(seeded, empty, total = facts.len(), "seed_consumable_inventories_from_facts");
}

/// Resolve a build from cached facts and convert into a list of inventory
/// slots. Returns empty when the ship param, species, or ship_config can't
/// be resolved.
pub fn build_inventory_from_facts<P: GameParamProvider>(
    facts: &VehicleFacts,
    gp: &P,
    version: Version,
) -> Vec<ConsumableInventory> {
    let Some(ship) = gp.game_param_by_id(facts.vehicle_id) else {
        return Vec::new();
    };
    let Some(species) = ship.species().and_then(|s| s.known()).copied() else {
        return Vec::new();
    };
    let cfg = &facts.ship_config;
    let captain_id = if facts.crew.params_id().raw() != 0 { Some(facts.crew.params_id()) } else { None };
    let skill_types = facts.crew.learned_skills().for_species(&species);
    let Some(build) = ResolvedBuild::from_ids(
        cfg.ship_params_id(),
        cfg.units(),
        cfg.modernization(),
        captain_id,
        skill_types,
        cfg.exteriors(),
        cfg.abilities(),
        species,
        version,
        gp,
    ) else {
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

/// Helper kept for callers that want species-specific facts lookups (e.g.
/// the renderer needs species to interpret captain skills).
pub fn facts_species<P: GameParamProvider>(facts: &VehicleFacts, gp: &P) -> Option<Species> {
    gp.game_param_by_id(facts.vehicle_id)?.species()?.known().copied()
}
