//! Validates that BattleView returns identical data to BattleControllerState for
//! every accessor the renderer consumes. This is the contract the renderer
//! migration depends on: the read API must be byte-identical to the old trait.

#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeSet, HashMap};

use wows_replays::analyzer::battle_controller::listener::BattleControllerState;

fn run_parity(filename: &str) {
    let (old, mut new_world) = support::both(filename);

    let view = new_world.view();

    // clock
    assert_eq!(old.clock(), view.clock(), "[{filename}] clock");

    // player_entities: key set + per-player fields
    {
        let op = old.player_entities();
        let np = view.player_entities();
        let mut ok: Vec<_> = op.keys().copied().collect();
        let mut nk: Vec<_> = np.keys().copied().collect();
        ok.sort();
        nk.sort();
        assert_eq!(ok, nk, "[{filename}] player_entities key sets differ");
        for (id, o) in op {
            let n = np.get(id).unwrap();
            assert_eq!(
                o.initial_state().username(),
                n.initial_state().username(),
                "[{filename}] player[{id:?}].name"
            );
            assert_eq!(o.relation(), n.relation(), "[{filename}] player[{id:?}].relation");
            assert_eq!(o.vehicle().id(), n.vehicle().id(), "[{filename}] player[{id:?}].vehicle_id");
        }
    }

    // positions (ship_positions in old API)
    {
        let old_pos = old.ship_positions();
        let new_pos: HashMap<_, _> = view.positions().into_iter().collect();
        let old_ids: BTreeSet<_> = old_pos.keys().copied().collect();
        let new_ids: BTreeSet<_> = new_pos.keys().copied().collect();
        assert_eq!(
            old_ids, new_ids,
            "[{filename}] position id sets differ: old_only={:?} new_only={:?}",
            old_ids.difference(&new_ids).collect::<Vec<_>>(),
            new_ids.difference(&old_ids).collect::<Vec<_>>(),
        );
        for (id, sp) in old_pos {
            let t = new_pos.get(id).unwrap();
            assert_eq!(sp.position.x, t.pos.x, "[{filename}] pos.x id={id:?}");
            assert_eq!(sp.position.y, t.pos.y, "[{filename}] pos.y id={id:?}");
            assert_eq!(sp.position.z, t.pos.z, "[{filename}] pos.z id={id:?}");
            assert_eq!(sp.yaw, t.yaw.0, "[{filename}] pos.yaw id={id:?}");
            assert_eq!(sp.pitch, t.pitch.0, "[{filename}] pos.pitch id={id:?}");
            assert_eq!(sp.roll, t.roll.0, "[{filename}] pos.roll id={id:?}");
        }
    }

    // minimap_positions
    {
        let old_mm = old.minimap_positions();
        let new_mm: HashMap<_, _> = view.minimap_positions().into_iter().collect();
        let old_ids: BTreeSet<_> = old_mm.keys().copied().collect();
        let new_ids: BTreeSet<_> = new_mm.keys().copied().collect();
        assert_eq!(
            old_ids, new_ids,
            "[{filename}] minimap id sets differ: old_only={:?} new_only={:?}",
            old_ids.difference(&new_ids).collect::<Vec<_>>(),
            new_ids.difference(&old_ids).collect::<Vec<_>>(),
        );
        for (id, om) in old_mm {
            let nm = new_mm.get(id).unwrap();
            assert_eq!(om.position.x, nm.pos.x, "[{filename}] mm.x id={id:?}");
            assert_eq!(om.position.y, nm.pos.y, "[{filename}] mm.y id={id:?}");
            assert_eq!(om.heading, nm.heading.0, "[{filename}] mm.heading id={id:?}");
            assert_eq!(om.visible, nm.visible, "[{filename}] mm.visible id={id:?}");
            assert_eq!(
                om.visibility_flags,
                nm.visibility_flags.0,
                "[{filename}] mm.visibility_flags id={id:?}"
            );
            assert_eq!(om.is_invisible, nm.is_invisible, "[{filename}] mm.is_invisible id={id:?}");
        }
    }

    // vehicle_props_all
    {
        let old_entities = old.entities_by_id();
        let new_vps: HashMap<_, _> = view.vehicle_props_all().into_iter().collect();
        for (id, entity) in old_entities {
            let Some(vref) = entity.vehicle_ref() else { continue };
            let ovp = std::cell::RefCell::borrow(vref).props().clone();
            let nvp = new_vps.get(id).unwrap_or_else(|| panic!("[{filename}] vehicle_props missing id={id:?}"));
            assert_eq!(ovp.health(), nvp.health(), "[{filename}] health id={id:?}");
            assert_eq!(ovp.max_health(), nvp.max_health(), "[{filename}] max_health id={id:?}");
            assert_eq!(ovp.is_alive(), nvp.is_alive(), "[{filename}] is_alive id={id:?}");
            assert_eq!(ovp.team_id(), nvp.team_id(), "[{filename}] team_id id={id:?}");
            assert_eq!(ovp.visibility_flags(), nvp.visibility_flags(), "[{filename}] vflags id={id:?}");
        }
    }

    // turret_yaws / target_yaws / selected_ammo via aim accessors
    {
        let old_turret_yaws = old.turret_yaws();
        let old_target_yaws = old.target_yaws();
        let old_ammos = old.selected_ammo();
        let new_turret_yaws: HashMap<_, _> = view.turret_yaws().into_iter().collect();
        let new_target_yaws: HashMap<_, _> = view.target_yaws().into_iter().collect();
        let new_ammos: HashMap<_, _> = view.selected_ammo().into_iter().collect();
        let empty: Vec<f32> = Vec::new();
        for (id, _) in old.entities_by_id() {
            let oty = old_turret_yaws.get(id).map(|v| v.as_slice()).unwrap_or(empty.as_slice());
            let nty: Vec<f32> = new_turret_yaws.get(id).cloned().unwrap_or_default();
            assert_eq!(oty, nty.as_slice(), "[{filename}] turret_yaws id={id:?}");

            let ott = old_target_yaws.get(id).copied();
            let ntt = new_target_yaws.get(id).copied();
            assert_eq!(ott, ntt, "[{filename}] target_yaw id={id:?}");

            let oa = old_ammos.get(id).copied();
            let na = new_ammos.get(id).copied();
            assert_eq!(oa, na, "[{filename}] selected_ammo id={id:?}");
        }
    }

    // active_shots
    {
        let os = old.active_shots();
        let ns = view.active_shots();
        assert_eq!(os.len(), ns.len(), "[{filename}] active_shots len");
        assert_eq!(os, ns.as_slice(), "[{filename}] active_shots mismatch");
    }

    // active_torpedoes
    {
        let ot = old.active_torpedoes();
        let nt = view.active_torpedoes();
        assert_eq!(ot.len(), nt.len(), "[{filename}] active_torpedoes len");
        assert_eq!(ot, nt.as_slice(), "[{filename}] active_torpedoes mismatch");
    }

    // active_consumables
    {
        let oa = old.active_consumables();
        let na: HashMap<_, _> = view.active_consumables().into_iter().collect();
        let mut oids: Vec<_> = oa.keys().copied().collect();
        let mut nids: Vec<_> = na.keys().copied().collect();
        oids.sort();
        nids.sort();
        assert_eq!(
            oids, nids,
            "[{filename}] active_consumables key sets differ"
        );
        for (eid, list) in oa {
            let nlist = na.get(eid).unwrap();
            assert_eq!(list.len(), nlist.len(), "[{filename}] active_consumables[{eid:?}] len");
            for (i, (o, n)) in list.iter().zip(nlist.iter()).enumerate() {
                assert_eq!(o, n, "[{filename}] active_consumables[{eid:?}][{i}]");
            }
        }
    }

    // active_planes
    {
        let op = old.active_planes();
        let np: HashMap<_, _> = view.active_planes().into_iter().collect();
        assert_eq!(op.len(), np.len(), "[{filename}] active_planes len");
        let mut ids: Vec<_> = op.keys().copied().collect();
        ids.sort();
        for id in &ids {
            let o = &op[id];
            let n = np.get(id).unwrap_or_else(|| panic!("[{filename}] active_planes missing {id:?}"));
            assert_eq!(o, n, "[{filename}] active_planes[{id:?}]");
        }
    }

    // active_wards (NaN-aware radius comparison)
    {
        let ow = old.active_wards();
        let nw: HashMap<_, _> = view.active_wards().into_iter().collect();
        assert_eq!(ow.len(), nw.len(), "[{filename}] active_wards len");
        let mut ids: Vec<_> = ow.keys().copied().collect();
        ids.sort();
        for id in &ids {
            let o = &ow[id];
            let n = nw.get(id).unwrap_or_else(|| panic!("[{filename}] active_wards missing {id:?}"));
            assert_eq!(o.plane_id, n.plane_id, "[{filename}] ward.plane_id {id:?}");
            assert_eq!(o.position, n.position, "[{filename}] ward.position {id:?}");
            assert_eq!(o.owner_id, n.owner_id, "[{filename}] ward.owner_id {id:?}");
            let or_ = o.radius.value();
            let nr = n.radius.value();
            assert!(
                or_.total_cmp(&nr).is_eq(),
                "[{filename}] ward.radius {id:?}: old={or_} new={nr}",
            );
        }
    }

    // buff_zones
    {
        let ob = old.buff_zones();
        let nb: HashMap<_, _> = view.buff_zones().into_iter().collect();
        assert_eq!(ob.len(), nb.len(), "[{filename}] buff_zones len");
        let mut ids: Vec<_> = ob.keys().copied().collect();
        ids.sort();
        for id in &ids {
            let o = &ob[id];
            let n = nb.get(id).unwrap_or_else(|| panic!("[{filename}] buff_zones missing {id:?}"));
            assert_eq!(o.entity_id, n.entity_id, "[{filename}] buff_zones[{id:?}].entity_id");
            assert_eq!(o.position, n.position, "[{filename}] buff_zones[{id:?}].position");
            assert_eq!(o.radius, n.radius, "[{filename}] buff_zones[{id:?}].radius");
            assert_eq!(o.team_id, n.team_id, "[{filename}] buff_zones[{id:?}].team_id");
            assert_eq!(o.is_active, n.is_active, "[{filename}] buff_zones[{id:?}].is_active");
            assert_eq!(o.drop_params_id, n.drop_params_id, "[{filename}] buff_zones[{id:?}].drop_params_id");
        }
    }

    // capture_points
    {
        let oc = old.capture_points();
        let nc = view.capture_points();
        assert_eq!(oc.len(), nc.len(), "[{filename}] capture_points len");
        for (i, (o, n)) in oc.iter().zip(nc.iter()).enumerate() {
            assert_eq!(o.index, n.index, "[{filename}] cp[{i}].index");
            assert_eq!(o.position, n.position, "[{filename}] cp[{i}].position");
            assert_eq!(o.radius, n.radius, "[{filename}] cp[{i}].radius");
            assert_eq!(o.control_point_type, n.control_point_type, "[{filename}] cp[{i}].control_point_type");
            assert_eq!(o.team_id, n.team_id, "[{filename}] cp[{i}].team_id");
            assert_eq!(o.invader_team, n.invader_team, "[{filename}] cp[{i}].invader_team");
            assert_eq!(o.progress, n.progress, "[{filename}] cp[{i}].progress");
            assert_eq!(o.has_invaders, n.has_invaders, "[{filename}] cp[{i}].has_invaders");
            assert_eq!(o.is_enabled, n.is_enabled, "[{filename}] cp[{i}].is_enabled");
        }
    }

    // local_weather_zones
    {
        let ow = old.local_weather_zones();
        let nw = view.local_weather_zones();
        assert_eq!(ow.len(), nw.len(), "[{filename}] local_weather_zones len");
        for (i, (o, n)) in ow.iter().zip(nw.iter()).enumerate() {
            assert_eq!(o.name, n.name, "[{filename}] weather[{i}].name");
            assert_eq!(o.radius, n.radius, "[{filename}] weather[{i}].radius");
            assert_eq!(o.position.x, n.position.x, "[{filename}] weather[{i}].position.x");
            assert_eq!(o.position.z, n.position.z, "[{filename}] weather[{i}].position.z");
        }
    }

    // team_scores
    {
        let os = old.team_scores();
        let ns = view.team_scores();
        assert_eq!(os.len(), ns.len(), "[{filename}] team_scores len");
        for (i, (o, n)) in os.iter().zip(ns.iter()).enumerate() {
            assert_eq!(o.team_index, n.team_index, "[{filename}] team_scores[{i}].team_index");
            assert_eq!(o.score, n.score, "[{filename}] team_scores[{i}].score");
        }
    }

    // captured_buffs
    {
        let oc = old.captured_buffs();
        let nc = view.captured_buffs();
        assert_eq!(oc.len(), nc.len(), "[{filename}] captured_buffs len");
        for (i, (o, n)) in oc.iter().zip(nc.iter()).enumerate() {
            assert_eq!(o.params_id, n.params_id, "[{filename}] captured_buffs[{i}].params_id");
            assert_eq!(o.team_id, n.team_id, "[{filename}] captured_buffs[{i}].team_id");
            assert_eq!(o.clock, n.clock, "[{filename}] captured_buffs[{i}].clock");
        }
    }

    // scoring_rules
    {
        let or_ = old.scoring_rules().cloned();
        let nr = view.scoring_rules().cloned();
        match (&or_, &nr) {
            (None, None) => {}
            (Some(o), Some(n)) => {
                assert_eq!(o.team_win_score, n.team_win_score, "[{filename}] scoring_rules.team_win_score");
                assert_eq!(o.hold_reward, n.hold_reward, "[{filename}] scoring_rules.hold_reward");
                assert_eq!(o.hold_period, n.hold_period, "[{filename}] scoring_rules.hold_period");
            }
            _ => panic!(
                "[{filename}] scoring_rules presence mismatch: old={} new={}",
                or_.is_some(),
                nr.is_some()
            ),
        }
    }

    // kills
    {
        let ok = old.kills();
        let nk = view.kills();
        assert_eq!(ok.len(), nk.len(), "[{filename}] kills len");
        for (i, (o, n)) in ok.iter().zip(nk.iter()).enumerate() {
            assert_eq!(o.clock, n.clock, "[{filename}] kills[{i}].clock");
            assert_eq!(o.killer, n.killer, "[{filename}] kills[{i}].killer");
            assert_eq!(o.victim, n.victim, "[{filename}] kills[{i}].victim");
            assert_eq!(o.cause, n.cause, "[{filename}] kills[{i}].cause");
        }
    }

    // dead_ships
    {
        let od = old.dead_ships();
        let nd = view.dead_ships();
        assert_eq!(od.len(), nd.len(), "[{filename}] dead_ships len");
        for (id, o) in od {
            let n = nd.get(id).unwrap_or_else(|| panic!("[{filename}] dead_ships missing {id:?}"));
            assert_eq!(o.clock, n.clock, "[{filename}] dead_ships[{id:?}].clock");
        }
    }

    // game_chat
    {
        let oc = old.game_chat();
        let nc = view.game_chat();
        assert_eq!(oc.len(), nc.len(), "[{filename}] chat len");
        for (i, (o, n)) in oc.iter().zip(nc.iter()).enumerate() {
            assert_eq!(o.clock, n.clock, "[{filename}] chat[{i}].clock");
            assert_eq!(o.sender_name, n.sender_name, "[{filename}] chat[{i}].sender_name");
            assert_eq!(o.channel, n.channel, "[{filename}] chat[{i}].channel");
            assert_eq!(o.message, n.message, "[{filename}] chat[{i}].message");
        }
    }

    // self_ribbons
    assert_eq!(old.self_ribbons(), view.self_ribbons(), "[{filename}] self_ribbons");

    // self_damage_stats
    {
        let os = old.self_damage_stats();
        let ns = view.self_damage_stats();
        assert_eq!(os.len(), ns.len(), "[{filename}] self_damage_stats len");
        for (key, oe) in os {
            let ne = ns.get(key).unwrap_or_else(|| panic!("[{filename}] self_damage_stats missing {key:?}"));
            assert_eq!(oe.count, ne.count, "[{filename}] self_damage_stats[{key:?}].count");
            assert_eq!(oe.total, ne.total, "[{filename}] self_damage_stats[{key:?}].total");
        }
    }

    // battle_type
    assert_eq!(
        format!("{:?}", old.battle_type()),
        format!("{:?}", view.battle_type()),
        "[{filename}] battle_type"
    );

    // match state scalars
    assert_eq!(old.battle_stage(), view.battle_stage(), "[{filename}] battle_stage");
    assert_eq!(old.time_left(), view.time_left(), "[{filename}] time_left");
    assert_eq!(old.battle_start_clock(), view.battle_start_clock(), "[{filename}] battle_start_clock");
    assert_eq!(old.battle_end_clock(), view.battle_end_clock(), "[{filename}] battle_end_clock");
    assert_eq!(old.winning_team(), view.winning_team(), "[{filename}] winning_team");
    assert_eq!(
        old.finish_type().map(|r| format!("{r:?}")),
        view.finish_type().map(|r| format!("{r:?}")),
        "[{filename}] finish_type"
    );

    // game_clock_to_elapsed / elapsed_to_game_clock round-trip
    {
        let test_clock = old.clock();
        let old_elapsed = old.game_clock_to_elapsed(test_clock);
        let new_elapsed = view.game_clock_to_elapsed(test_clock);
        assert_eq!(old_elapsed, new_elapsed, "[{filename}] game_clock_to_elapsed");

        let old_abs = old.elapsed_to_game_clock(old_elapsed);
        let new_abs = view.elapsed_to_game_clock(new_elapsed);
        assert_eq!(old_abs, new_abs, "[{filename}] elapsed_to_game_clock");
    }
}

// v15.1, pvp, Domination (Vermont BB)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_view_vermont_pvp() {
    run_parity("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

// v15.1, pvp, Standard (Marceau DD) -- active consumables, planes
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_view_marceau_pvp() {
    run_parity("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
}

// v15.1, pve, Operation Narai
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_view_narai_operation() {
    run_parity("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
}

// v13.3, pvp (V-170 DD)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_8260685)), ignore)]
fn parity_view_v170_pvp() {
    run_parity("20240422_161541_PGSD104-V-170_08_NE_passage.wowsreplay");
}

// v0.9.0, pvp (Shimakaze)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn parity_view_shimakaze_v0_9() {
    run_parity("20200117_205708_PJSD012-Shimakaze-1943_45_Zigzag.wowsreplay");
}

// v0.11.9, pvp, ArmsRace (Cossack) -- buff_zones, captured_buffs
#[test]
#[cfg_attr(not(all(has_game_data, has_build_6359964)), ignore)]
fn parity_view_cossack_armsrace() {
    run_parity("20221101_004346_PBSD517-Cossack_37_Ridge.wowsreplay");
}
