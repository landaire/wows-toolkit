//! Integration check that MergedReplays builds a non-empty, sorted, dual-source
//! position timeline from a real replay.
#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use wows_battle_world::SampledPos;
use wows_battle_world::merged::MergedReplays;
use wows_replays::ReplayFile;

// v15.1 PVP. Verifies the position timeline is built and well-formed.
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn position_timeline_is_built_and_sorted() {
    let h = support::load("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
    let session =
        MergedReplays::new(h.specs, h.game_params, h.game_constants, h.version, &h.replay, &[] as &[ReplayFile])
            .expect("build session");
    let timeline = session.position_timeline();

    assert!(!timeline.is_empty(), "expected position samples");

    for samples in timeline.values() {
        assert!(
            samples.windows(2).all(|w| w[0].clock.0 <= w[1].clock.0),
            "each entity timeline must be sorted ascending by clock"
        );
    }

    let world_samples = timeline.values().flatten().filter(|s| matches!(s.pos, SampledPos::World(_))).count();
    let minimap_samples = timeline.values().flatten().filter(|s| matches!(s.pos, SampledPos::Minimap(_))).count();
    assert!(world_samples > 0, "expected world-space samples (Position / PlayerOrientation)");
    assert!(minimap_samples > 0, "expected minimap samples (radar/hydro spotted ships)");
}
