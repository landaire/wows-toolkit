#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use wows_replays::analyzer::battle_controller::listener::BattleControllerState;

fn run_parity(filename: &str) {
    let (old, new_world) = support::both(filename);

    assert_eq!(
        old.battle_stage(),
        new_world.battle_stage(),
        "battle_stage mismatch in {filename}",
    );
    assert_eq!(
        old.time_left(),
        new_world.time_left(),
        "time_left mismatch in {filename}",
    );
    assert_eq!(
        old.battle_start_clock(),
        new_world.battle_start_clock(),
        "battle_start_clock mismatch in {filename}",
    );
    assert_eq!(
        old.battle_end_clock(),
        new_world.battle_end_clock(),
        "battle_end_clock mismatch in {filename}",
    );
    assert_eq!(
        old.winning_team(),
        new_world.winning_team(),
        "winning_team mismatch in {filename}",
    );
    assert_eq!(
        old.finish_type().map(|r| format!("{r:?}")),
        new_world.finish_type().map(|r| format!("{r:?}")),
        "finish_type mismatch in {filename}",
    );
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_match_state_vermont_pvp() {
    run_parity("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_match_state_marceau_pvp() {
    run_parity("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_match_state_narai_operation() {
    run_parity("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
}
