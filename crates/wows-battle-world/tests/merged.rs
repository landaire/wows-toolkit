//! Tests for the BattleWorld-backed MergedReplays driver.
//!
//! Multi-perspective merging (the source-team routing filter, the arena-id
//! cross-match guard, and the damage union from multiple perspectives) cannot
//! be exercised without fixtures from multiple simultaneous perspectives of the
//! same battle, which are not available in the test corpus. Those paths are
//! validated by construction (identical logic to the old merged.rs, with the
//! same forwarding filter and dedup key). This file covers the single-replay
//! path, which must produce output identical to a directly-processed BattleWorld.

#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use wows_replays::ReplayFile;
use wows_battle_world::merged::MergedReplays;

fn check_single_replay(filename: &str) {
    let handle = support::load(filename);
    let (old, mut direct) = support::both(filename);

    // Sanity: direct world passes full parity against old controller.
    support::assert_full_parity(&old, &mut direct, filename);

    // Leak the replay so it satisfies 'static required by BattleWorld<'static, 'static, G>.
    let replay: &'static ReplayFile = Box::leak(Box::new(handle.replay));

    // Drive MergedReplays over the same single replay.
    let mut merged = MergedReplays::new(
        handle.specs,
        handle.game_params,
        handle.game_constants,
        handle.version,
        replay,
        &[],
    )
    .expect("MergedReplays::new");

    assert!(!merged.is_done(), "[{filename}] should not be done before first step");

    let mut step_count = 0u32;
    while merged.step().expect("step").is_some() {
        step_count += 1;
    }
    assert!(merged.is_done(), "[{filename}] should be done after stream exhausted");
    assert!(step_count > 0, "[{filename}] must have processed at least one packet");

    // safe_clock is None once every replay is finished.
    assert_eq!(merged.safe_clock(), None, "[{filename}] safe_clock must be None when done");
    assert!(merged.total_duration().0 > 0.0, "[{filename}] total_duration must be > 0");
    assert_eq!(merged.replays().len(), 1, "[{filename}] replays len");
    assert_eq!(merged.self_teams().len(), 1, "[{filename}] self_teams len");

    merged.finish();

    assert_eq!(merged.arena_id(), direct.arena_id(), "[{filename}] arena_id");

    let mut merged_world = merged.into_world();

    // Merged world must match the directly-processed world on every accessor
    // that assert_full_parity covers (entities, positions, minimap, vehicle
    // props, kills, dead ships, damage, ribbons, chat, etc.).
    support::assert_full_parity(&old, &mut merged_world, filename);
}

// v15.1, pvp, Domination (Vermont BB)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn merged_single_replay_vermont_pvp() {
    check_single_replay("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

// v13.3, pvp, Domination (V-170 DD)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_8260685)), ignore)]
fn merged_single_replay_v170_pvp() {
    check_single_replay("20240422_161541_PGSD104-V-170_08_NE_passage.wowsreplay");
}

// v0.9.0, pvp, Domination (Shimakaze) -- older protocol layout
#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn merged_single_replay_shimakaze_v0_9() {
    check_single_replay("20200117_205708_PJSD012-Shimakaze-1943_45_Zigzag.wowsreplay");
}
