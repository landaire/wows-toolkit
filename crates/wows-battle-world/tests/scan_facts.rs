//! Golden snapshot of gather_replay_facts + gather_damage_events.
//!
//! Safety-net: captures current output so a later refactor can prove "no
//! behavior change" without actually exercising any new logic.
#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use serde::Serialize;
use wows_battle_world::merged::gather_damage_events;
use wows_battle_world::merged::gather_replay_facts;

fn r3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

/// Stable scalar for ShipConfig: the params-id (u64) plus the count of
/// ability slots. Full ability lists are tested elsewhere in corpus_golden;
/// here we only need enough to detect regressions.
#[derive(Serialize)]
struct FactEntry {
    entity_id: u32,
    vehicle_id: u64,
    max_health: f32,
    ship_params_id: u64,
    ability_count: usize,
    crew_params_id: u64,
}

/// Stable scalar for a single damage event.
#[derive(Serialize)]
struct DmgEntry {
    aggressor: u32,
    victim: u32,
    clock: f32,
    amount: f32,
}

#[derive(Serialize)]
struct Snapshot {
    facts: Vec<FactEntry>,
    damage: Vec<DmgEntry>,
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn facts_and_damage_golden() {
    let h = support::load("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");

    let facts_map = gather_replay_facts(h.game_constants, h.version, h.specs, &[&h.replay]);
    let mut facts: Vec<FactEntry> = facts_map
        .iter()
        .map(|(entity_id, f)| FactEntry {
            entity_id: entity_id.raw(),
            vehicle_id: f.vehicle_id.raw(),
            max_health: r3(f.max_health),
            ship_params_id: f.ship_config.ship_params_id().raw(),
            ability_count: f.ship_config.abilities().len(),
            crew_params_id: f.crew.params_id().raw(),
        })
        .collect();
    facts.sort_by_key(|e| e.entity_id);

    let damage_map = gather_damage_events(h.game_params, h.game_constants, h.version, h.specs, &[&h.replay]);
    let mut damage: Vec<DmgEntry> = damage_map
        .iter()
        .flat_map(|(aggressor, events)| {
            events.iter().map(|ev| DmgEntry {
                aggressor: aggressor.raw(),
                victim: ev.victim.raw(),
                clock: r3(ev.clock.0),
                amount: r3(ev.amount),
            })
        })
        .collect();
    damage.sort_by(|a, b| (a.aggressor, a.victim, a.clock.to_bits()).cmp(&(b.aggressor, b.victim, b.clock.to_bits())));

    insta::assert_yaml_snapshot!(Snapshot { facts, damage });
}
