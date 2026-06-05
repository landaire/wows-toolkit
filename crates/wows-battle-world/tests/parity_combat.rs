#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use wows_replays::analyzer::battle_controller::listener::BattleControllerState;

fn run_parity(filename: &str) {
    let (old, new_world) = support::both(filename);

    let old_kills = old.kills();
    let new_kills = new_world.kills();
    assert_eq!(
        old_kills.len(),
        new_kills.len(),
        "kills length mismatch in {filename}: old={} new={}",
        old_kills.len(),
        new_kills.len(),
    );
    for (i, (o, n)) in old_kills.iter().zip(new_kills.iter()).enumerate() {
        assert_eq!(o.clock, n.clock, "kills[{i}] clock mismatch in {filename}");
        assert_eq!(o.killer, n.killer, "kills[{i}] killer mismatch in {filename}");
        assert_eq!(o.victim, n.victim, "kills[{i}] victim mismatch in {filename}");
        assert_eq!(o.cause, n.cause, "kills[{i}] cause mismatch in {filename}");
    }

    let old_dead = old.dead_ships();
    let new_dead = new_world.dead_ships();
    assert_eq!(
        old_dead.len(),
        new_dead.len(),
        "dead_ships length mismatch in {filename}: old={} new={}",
        old_dead.len(),
        new_dead.len(),
    );
    for (id, o) in old_dead {
        let n = new_dead
            .get(id)
            .unwrap_or_else(|| panic!("dead_ships missing id={id:?} in {filename}"));
        assert_eq!(o.clock, n.clock, "dead_ships[{id:?}] clock mismatch in {filename}");
        match (o.position, n.position) {
            (None, None) => {}
            (Some(op), Some(np)) => {
                assert_eq!(op.x, np.x, "dead_ships[{id:?}] position.x mismatch in {filename}");
                assert_eq!(op.y, np.y, "dead_ships[{id:?}] position.y mismatch in {filename}");
                assert_eq!(op.z, np.z, "dead_ships[{id:?}] position.z mismatch in {filename}");
            }
            _ => panic!(
                "dead_ships[{id:?}] position presence mismatch in {filename}: old={:?} new={:?}",
                o.position.is_some(),
                n.position.is_some(),
            ),
        }
        match (o.minimap_position, n.minimap_position) {
            (None, None) => {}
            (Some(om), Some(nm)) => {
                assert_eq!(
                    om.x, nm.x,
                    "dead_ships[{id:?}] minimap_position.x mismatch in {filename}"
                );
                assert_eq!(
                    om.y, nm.y,
                    "dead_ships[{id:?}] minimap_position.y mismatch in {filename}"
                );
            }
            _ => panic!(
                "dead_ships[{id:?}] minimap_position presence mismatch in {filename}: old={:?} new={:?}",
                o.minimap_position.is_some(),
                n.minimap_position.is_some(),
            ),
        }
    }

    let old_damage = old.damage_dealt();
    let new_damage = new_world.damage_ledger();
    {
        let mut old_keys: Vec<_> = old_damage.keys().copied().collect();
        let mut new_keys: Vec<_> = new_damage.keys().copied().collect();
        old_keys.sort();
        new_keys.sort();
        assert_eq!(
            old_keys, new_keys,
            "damage_dealt aggressor key sets differ in {filename}: old_only={:?} new_only={:?}",
            old_keys.iter().filter(|k| !new_keys.contains(k)).collect::<Vec<_>>(),
            new_keys.iter().filter(|k| !old_keys.contains(k)).collect::<Vec<_>>(),
        );
    }
    assert_eq!(
        old_damage.len(),
        new_damage.len(),
        "damage_dealt aggressor count mismatch in {filename}: old={} new={}",
        old_damage.len(),
        new_damage.len(),
    );
    for (aggressor, old_events) in old_damage {
        let new_events = new_damage
            .get(aggressor)
            .unwrap_or_else(|| panic!("damage_dealt missing aggressor={aggressor:?} in {filename}"));
        assert_eq!(
            old_events.len(),
            new_events.len(),
            "damage_dealt[{aggressor:?}] event count mismatch in {filename}",
        );
        for (j, (oe, ne)) in old_events.iter().zip(new_events.iter()).enumerate() {
            assert_eq!(
                oe.amount, ne.amount,
                "damage_dealt[{aggressor:?}][{j}] amount mismatch in {filename}"
            );
            assert_eq!(
                oe.victim, ne.victim,
                "damage_dealt[{aggressor:?}][{j}] victim mismatch in {filename}"
            );
            assert_eq!(
                oe.clock, ne.clock,
                "damage_dealt[{aggressor:?}][{j}] clock mismatch in {filename}"
            );
        }
    }

    let old_ribbons = old.self_ribbons();
    let new_ribbons = new_world.self_ribbons();
    assert_eq!(
        old_ribbons, new_ribbons,
        "self_ribbons mismatch in {filename}",
    );

    let old_stats = old.self_damage_stats();
    let new_stats = new_world.self_damage_stats();
    assert_eq!(
        old_stats.len(),
        new_stats.len(),
        "self_damage_stats entry count mismatch in {filename}",
    );
    for (key, oe) in old_stats {
        let ne = new_stats
            .get(key)
            .unwrap_or_else(|| panic!("self_damage_stats missing key={key:?} in {filename}"));
        assert_eq!(
            oe.count, ne.count,
            "self_damage_stats[{key:?}] count mismatch in {filename}"
        );
        assert_eq!(
            oe.total, ne.total,
            "self_damage_stats[{key:?}] total mismatch in {filename}"
        );
        assert_eq!(
            oe.weapon, ne.weapon,
            "self_damage_stats[{key:?}] weapon mismatch in {filename}"
        );
        assert_eq!(
            oe.category, ne.category,
            "self_damage_stats[{key:?}] category mismatch in {filename}"
        );
    }
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_combat_vermont_pvp() {
    run_parity("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_combat_narai_operation() {
    run_parity("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_8260685)), ignore)]
fn parity_combat_v170_pvp() {
    run_parity("20240422_161541_PGSD104-V-170_08_NE_passage.wowsreplay");
}
