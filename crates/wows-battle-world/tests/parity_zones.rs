#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use wows_replays::analyzer::battle_controller::listener::BattleControllerState;

fn run_parity(filename: &str) {
    let (old, mut new_world) = support::both(filename);

    let old_caps = old.capture_points();
    let new_caps = new_world.capture_points();
    assert_eq!(
        old_caps.len(),
        new_caps.len(),
        "capture_points len mismatch in {filename}: old={} new={}",
        old_caps.len(),
        new_caps.len(),
    );
    for (i, (o, n)) in old_caps.iter().zip(new_caps.iter()).enumerate() {
        assert_eq!(
            o.index, n.index,
            "capture_points[{i}].index mismatch in {filename}"
        );
        assert_eq!(
            o.position, n.position,
            "capture_points[{i}].position mismatch in {filename}"
        );
        assert_eq!(
            o.radius, n.radius,
            "capture_points[{i}].radius mismatch in {filename}"
        );
        assert_eq!(
            o.control_point_type, n.control_point_type,
            "capture_points[{i}].control_point_type mismatch in {filename}"
        );
        assert_eq!(
            o.team_id, n.team_id,
            "capture_points[{i}].team_id mismatch in {filename}"
        );
        assert_eq!(
            o.invader_team, n.invader_team,
            "capture_points[{i}].invader_team mismatch in {filename}"
        );
        assert_eq!(
            o.progress, n.progress,
            "capture_points[{i}].progress mismatch in {filename}"
        );
        assert_eq!(
            o.has_invaders, n.has_invaders,
            "capture_points[{i}].has_invaders mismatch in {filename}"
        );
        assert_eq!(
            o.both_inside, n.both_inside,
            "capture_points[{i}].both_inside mismatch in {filename}"
        );
        assert_eq!(
            o.is_enabled, n.is_enabled,
            "capture_points[{i}].is_enabled mismatch in {filename}"
        );
    }

    let old_scores = old.team_scores();
    let new_scores = new_world.team_scores();
    assert_eq!(
        old_scores.len(),
        new_scores.len(),
        "team_scores len mismatch in {filename}: old={} new={}",
        old_scores.len(),
        new_scores.len(),
    );
    for (i, (o, n)) in old_scores.iter().zip(new_scores.iter()).enumerate() {
        assert_eq!(
            o.team_index, n.team_index,
            "team_scores[{i}].team_index mismatch in {filename}"
        );
        assert_eq!(
            o.score, n.score,
            "team_scores[{i}].score mismatch in {filename}"
        );
    }

    let old_buffs = old.buff_zones();
    let new_buffs = new_world.buff_zones();
    assert_eq!(
        old_buffs.len(),
        new_buffs.len(),
        "buff_zones len mismatch in {filename}: old={} new={}",
        old_buffs.len(),
        new_buffs.len(),
    );
    let mut old_ids: Vec<_> = old_buffs.keys().copied().collect();
    old_ids.sort();
    for id in &old_ids {
        let o = &old_buffs[id];
        let n = new_buffs.get(id).unwrap_or_else(|| {
            panic!("buff_zones missing entity_id={id:?} in {filename}")
        });
        assert_eq!(o.entity_id, n.entity_id, "buff_zones[{id:?}].entity_id mismatch in {filename}");
        assert_eq!(o.position, n.position, "buff_zones[{id:?}].position mismatch in {filename}");
        assert_eq!(o.radius, n.radius, "buff_zones[{id:?}].radius mismatch in {filename}");
        assert_eq!(o.team_id, n.team_id, "buff_zones[{id:?}].team_id mismatch in {filename}");
        assert_eq!(o.is_active, n.is_active, "buff_zones[{id:?}].is_active mismatch in {filename}");
        assert_eq!(
            o.drop_params_id, n.drop_params_id,
            "buff_zones[{id:?}].drop_params_id mismatch in {filename}"
        );
    }

    let old_captured = old.captured_buffs();
    let new_captured = new_world.captured_buffs();
    assert_eq!(
        old_captured.len(),
        new_captured.len(),
        "captured_buffs len mismatch in {filename}: old={} new={}",
        old_captured.len(),
        new_captured.len(),
    );
    for (i, (o, n)) in old_captured.iter().zip(new_captured.iter()).enumerate() {
        assert_eq!(
            o.params_id, n.params_id,
            "captured_buffs[{i}].params_id mismatch in {filename}"
        );
        assert_eq!(
            o.team_id, n.team_id,
            "captured_buffs[{i}].team_id mismatch in {filename}"
        );
        assert_eq!(
            o.clock, n.clock,
            "captured_buffs[{i}].clock mismatch in {filename}"
        );
    }

    let old_weather = old.local_weather_zones();
    let new_weather = new_world.local_weather_zones();
    assert_eq!(
        old_weather.len(),
        new_weather.len(),
        "local_weather_zones len mismatch in {filename}: old={} new={}",
        old_weather.len(),
        new_weather.len(),
    );
    for (i, (o, n)) in old_weather.iter().zip(new_weather.iter()).enumerate() {
        assert_eq!(
            o.name, n.name,
            "local_weather_zones[{i}].name mismatch in {filename}"
        );
        assert_eq!(
            o.params_id, n.params_id,
            "local_weather_zones[{i}].params_id mismatch in {filename}"
        );
        assert_eq!(
            o.radius, n.radius,
            "local_weather_zones[{i}].radius mismatch in {filename}"
        );
        assert_eq!(
            o.position.x, n.position.x,
            "local_weather_zones[{i}].position.x mismatch in {filename}"
        );
        assert_eq!(
            o.position.z, n.position.z,
            "local_weather_zones[{i}].position.z mismatch in {filename}"
        );
    }

    let old_rules = old.scoring_rules().cloned();
    let new_rules = new_world.scoring_rules();
    match (&old_rules, &new_rules) {
        (None, None) => {}
        (Some(o), Some(n)) => {
            assert_eq!(
                o.team_win_score, n.team_win_score,
                "scoring_rules.team_win_score mismatch in {filename}"
            );
            assert_eq!(
                o.hold_reward, n.hold_reward,
                "scoring_rules.hold_reward mismatch in {filename}"
            );
            assert_eq!(
                o.hold_period, n.hold_period,
                "scoring_rules.hold_period mismatch in {filename}"
            );
            assert_eq!(
                o.hold_cp_indices, n.hold_cp_indices,
                "scoring_rules.hold_cp_indices mismatch in {filename}"
            );
        }
        _ => panic!(
            "scoring_rules presence mismatch in {filename}: old={} new={}",
            old_rules.is_some(),
            new_rules.is_some(),
        ),
    }
}

// v15.1, pvp, Domination (3 cap points, scoring rules)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_zones_vermont_pvp() {
    run_parity("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

// v15.1, pve, Operation (Narai - cap points from operation scenario)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_zones_narai_operation() {
    run_parity("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
}

// v9.0.0, pvp, Domination (legacy control points from BattleLogic state, no InteractiveZone)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn parity_zones_shimakaze_legacy() {
    run_parity("20200117_205708_PJSD012-Shimakaze-1943_45_Zigzag.wowsreplay");
}

// v0.11.9, pvp, ArmsRace (buff zones, captured_buffs, no domination cap points)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_6359964)), ignore)]
fn parity_zones_cossack_armsrace() {
    run_parity("20221101_004346_PBSD517-Cossack_37_Ridge.wowsreplay");
}
