#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::collections::HashMap;

use wows_replays::analyzer::battle_controller::BattleReport as OldReport;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::DeathInfo;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::decoder::DamageStatEntry;

use wows_battle_world::report::BattleReport as NewReport;

/// DeathInfo lacks PartialEq; compare its load-bearing fields.
fn death_info_eq(a: &DeathInfo, b: &DeathInfo) -> bool {
    a.killer() == b.killer()
        && a.time_lived() == b.time_lived()
        && format!("{:?}", a.cause()) == format!("{:?}", b.cause())
}

/// BattleResult lacks PartialEq and Debug; compare via its Serialize encoding.
fn battle_result_str(r: Option<&BattleResult>) -> Option<String> {
    r.map(|r| serde_json::to_string(r).unwrap())
}

/// AccountId is not Ord; key by its inner i64 instead.
fn players_by_account(players: &[wows_replays::Rc<Player>]) -> HashMap<i64, &Player> {
    players.iter().map(|p| (p.initial_state().db_id().0, p.as_ref())).collect()
}

fn sorted_keys(map: &HashMap<i64, &Player>) -> Vec<i64> {
    let mut ids: Vec<i64> = map.keys().copied().collect();
    ids.sort();
    ids
}

/// Compare DamageStatEntry vectors as sets keyed by (weapon, category); the
/// vectors are collected from HashMaps so order is not load-bearing.
fn assert_damage_stats_eq(old: &[DamageStatEntry], new: &[DamageStatEntry], label: &str) {
    let key = |e: &DamageStatEntry| (format!("{:?}", e.weapon), format!("{:?}", e.category));
    let om: HashMap<_, _> = old.iter().map(|e| (key(e), (e.count, e.total))).collect();
    let nm: HashMap<_, _> = new.iter().map(|e| (key(e), (e.count, e.total))).collect();
    assert_eq!(om.len(), nm.len(), "[{label}] self_damage_stats len");
    for (k, ov) in &om {
        let nv = nm.get(k).unwrap_or_else(|| panic!("[{label}] self_damage_stats missing {k:?}"));
        assert_eq!(ov, nv, "[{label}] self_damage_stats[{k:?}] (count,total)");
    }
}

fn assert_report_parity(old: &OldReport, new: &NewReport, label: &str) {
    // Scalar identity fields.
    assert_eq!(old.arena_id(), new.arena_id(), "[{label}] arena_id");
    assert_eq!(old.version(), new.version(), "[{label}] version");
    assert_eq!(old.map_name(), new.map_name(), "[{label}] map_name");
    assert_eq!(old.game_mode(), new.game_mode(), "[{label}] game_mode");
    assert_eq!(
        format!("{:?}", old.game_type()),
        format!("{:?}", new.game_type()),
        "[{label}] game_type"
    );
    assert_eq!(old.match_group(), new.match_group(), "[{label}] match_group");
    assert_eq!(old.battle_results(), new.battle_results(), "[{label}] battle_results");

    // self_player identity.
    assert_eq!(
        old.self_player().initial_state().db_id(),
        new.self_player().initial_state().db_id(),
        "[{label}] self_player.db_id"
    );
    assert_eq!(
        old.self_player().initial_state().username(),
        new.self_player().initial_state().username(),
        "[{label}] self_player.name"
    );

    // match_result / finish_type.
    assert_eq!(
        battle_result_str(old.battle_result()),
        battle_result_str(new.battle_result()),
        "[{label}] battle_result"
    );
    assert_eq!(
        old.finish_type().map(|r| format!("{r:?}")),
        new.finish_type().map(|r| format!("{r:?}")),
        "[{label}] finish_type"
    );

    // Durations.
    assert_eq!(old.max_duration(), new.max_duration(), "[{label}] max_duration");
    assert_eq!(old.played_duration(), new.played_duration(), "[{label}] played_duration");
    assert_eq!(old.extra_duration(), new.extra_duration(), "[{label}] extra_duration");
    // battle_start_clock is private on the original report; it is the basis of
    // game_clock_to_elapsed, so a round-trip on a fixed clock verifies parity.
    {
        use wows_replays::types::GameClock;
        let probe = GameClock(123.5);
        assert_eq!(
            old.game_clock_to_elapsed(probe).0,
            new.game_clock_to_elapsed(probe).0,
            "[{label}] battle_start_clock (via game_clock_to_elapsed)"
        );
    }

    // Players: same set by account id, same end_state key fields, frags counts,
    // death_info, and per-vehicle damage.
    {
        let op = players_by_account(old.players());
        let np = players_by_account(new.players());
        assert_eq!(sorted_keys(&op), sorted_keys(&np), "[{label}] player account id sets differ");

        for (id, o) in &op {
            let n = np.get(id).unwrap();

            assert_eq!(
                o.end_state().username(),
                n.end_state().username(),
                "[{label}] player[{id:?}].end_state.username"
            );
            assert_eq!(
                o.end_state().meta_ship_id(),
                n.end_state().meta_ship_id(),
                "[{label}] player[{id:?}].end_state.meta_ship_id"
            );
            assert_eq!(
                o.end_state().team_id(),
                n.end_state().team_id(),
                "[{label}] player[{id:?}].end_state.team_id"
            );
            assert_eq!(o.relation(), n.relation(), "[{label}] player[{id:?}].relation");

            let ov = o.vehicle_entity();
            let nv = n.vehicle_entity();
            assert_eq!(
                ov.is_some(),
                nv.is_some(),
                "[{label}] player[{id:?}] vehicle_entity presence"
            );
            if let (Some(ov), Some(nv)) = (ov, nv) {
                assert_eq!(ov.id(), nv.id(), "[{label}] player[{id:?}].vehicle.id");
                assert_eq!(
                    ov.captain().map(|p| p.id()),
                    nv.captain().map(|p| p.id()),
                    "[{label}] player[{id:?}].vehicle.captain id"
                );
                // The self player's damage is summed from a HashMap of damage
                // stats; iteration order differs between the two HashMaps, so the
                // f64 accumulation rounds in the last bit. The mathematical sum is
                // identical, so compare with a relative epsilon. Non-self damage
                // folds a Vec in deterministic order and matches exactly.
                let (od, nd) = (ov.damage(), nv.damage());
                let tol = od.abs() * 1e-12 + 1e-6;
                assert!(
                    (od - nd).abs() <= tol,
                    "[{label}] player[{id:?}].vehicle.damage: old={od} new={nd}"
                );
                assert_eq!(
                    ov.frags().len(),
                    nv.frags().len(),
                    "[{label}] player[{id:?}].vehicle.frags len"
                );
                for (i, (of, nf)) in ov.frags().iter().zip(nv.frags().iter()).enumerate() {
                    assert!(
                        death_info_eq(of, nf),
                        "[{label}] player[{id:?}].vehicle.frags[{i}] mismatch: old={of:?} new={nf:?}"
                    );
                }
                match (ov.death_info(), nv.death_info()) {
                    (None, None) => {}
                    (Some(od), Some(nd)) => assert!(
                        death_info_eq(od, nd),
                        "[{label}] player[{id:?}].vehicle.death_info mismatch: old={od:?} new={nd:?}"
                    ),
                    _ => panic!(
                        "[{label}] player[{id:?}].vehicle.death_info presence mismatch: old={} new={}",
                        ov.death_info().is_some(),
                        nv.death_info().is_some()
                    ),
                }
                assert_eq!(
                    ov.results_info(),
                    nv.results_info(),
                    "[{label}] player[{id:?}].vehicle.results_info"
                );
            }
        }
    }

    // frags map: per-player Vec<DeathInfo>, keyed by account id (Player Eq is by db_id).
    {
        let of: HashMap<i64, &Vec<DeathInfo>> =
            old.frags().iter().map(|(p, d)| (p.initial_state().db_id().0, d)).collect();
        let nf: HashMap<i64, &Vec<DeathInfo>> =
            new.frags().iter().map(|(p, d)| (p.initial_state().db_id().0, d)).collect();
        let mut oids: Vec<_> = of.keys().copied().collect();
        let mut nids: Vec<_> = nf.keys().copied().collect();
        oids.sort();
        nids.sort();
        assert_eq!(oids, nids, "[{label}] frags map key sets differ");
        for (id, od) in &of {
            let nd = nf.get(id).unwrap();
            assert_eq!(od.len(), nd.len(), "[{label}] frags[{id:?}] len");
            for (i, (o, n)) in od.iter().zip(nd.iter()).enumerate() {
                assert!(
                    death_info_eq(o, n),
                    "[{label}] frags[{id:?}][{i}] mismatch: old={o:?} new={n:?}"
                );
            }
        }
    }

    // capture_points.
    assert_eq!(old.capture_points(), new.capture_points(), "[{label}] capture_points");

    // buff_zones.
    {
        let ob = old.buff_zones();
        let nb = new.buff_zones();
        assert_eq!(ob.len(), nb.len(), "[{label}] buff_zones len");
        for (id, o) in ob {
            let n = nb.get(id).unwrap_or_else(|| panic!("[{label}] buff_zones missing {id:?}"));
            assert_eq!(o, n, "[{label}] buff_zones[{id:?}]");
        }
    }

    // team_scores.
    assert_eq!(old.team_scores(), new.team_scores(), "[{label}] team_scores");

    // captured_buffs.
    assert_eq!(old.captured_buffs(), new.captured_buffs(), "[{label}] captured_buffs");

    // local_weather_zones.
    assert_eq!(old.local_weather_zones(), new.local_weather_zones(), "[{label}] local_weather_zones");

    // buildings: BuildingEntity lacks PartialEq; compare by id with field-by-field.
    {
        let ob: BTreeMap<_, _> = old.buildings().iter().map(|b| (b.id, b)).collect();
        let nb: BTreeMap<_, _> = new.buildings().iter().map(|b| (b.id, b)).collect();
        let oids: Vec<_> = ob.keys().copied().collect();
        let nids: Vec<_> = nb.keys().copied().collect();
        assert_eq!(oids, nids, "[{label}] building id sets differ");
        for (id, o) in &ob {
            let n = nb.get(id).unwrap();
            assert_eq!(o.position, n.position, "[{label}] building[{id:?}].position");
            assert_eq!(o.is_alive, n.is_alive, "[{label}] building[{id:?}].is_alive");
            assert_eq!(o.is_hidden, n.is_hidden, "[{label}] building[{id:?}].is_hidden");
            assert_eq!(o.is_suppressed, n.is_suppressed, "[{label}] building[{id:?}].is_suppressed");
            assert_eq!(o.team_id, n.team_id, "[{label}] building[{id:?}].team_id");
            assert_eq!(o.params_id, n.params_id, "[{label}] building[{id:?}].params_id");
        }
    }

    // self_damage_stats (order-independent set).
    assert_damage_stats_eq(old.self_damage_stats(), new.self_damage_stats(), label);

    // active_consumables.
    {
        let oa = old.active_consumables();
        let na = new.active_consumables();
        assert_eq!(oa.len(), na.len(), "[{label}] active_consumables len");
        for (id, ol) in oa {
            let nl = na.get(id).unwrap_or_else(|| panic!("[{label}] active_consumables missing {id:?}"));
            assert_eq!(ol, nl, "[{label}] active_consumables[{id:?}]");
        }
    }
}

fn run_report_parity(filename: &str) {
    let (old, new) = support::both_reports(filename);
    assert_report_parity(&old, &new, filename);
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn report_parity_vermont_pvp() {
    run_report_parity("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn report_parity_marceau_pvp() {
    run_report_parity("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn report_parity_narai_operation() {
    run_report_parity("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
}

// v0.10.0, pvp, Domination_3point
#[test]
#[cfg_attr(not(all(has_game_data, has_build_3343484)), ignore)]
fn report_parity_v0_10_0_jean_bart_pvp() {
    run_report_parity("20210202_105419_PFSB518-Jean-Bart_44_Path_warrior.wowsreplay");
}

// v0.11.0, ranked, Ranked_Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_5045210)), ignore)]
fn report_parity_v0_11_0_grossdeutschland_ranked() {
    run_report_parity("20220210_003215_PGSB110-Grossdeutschland_15_NE_north.wowsreplay");
}

// v13.2, pvp, Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_8151735)), ignore)]
fn report_parity_v13_2_annapolis_pvp() {
    run_report_parity("20240402_192304_PASC111-Annapolis_22_tierra_del_fuego.wowsreplay");
}
