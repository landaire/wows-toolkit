#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wowsunpack::game_types::PlaneId;

fn run_parity(filename: &str) {
    let (old, mut new_world) = support::both(filename);

    let old_planes = old.active_planes();
    let new_planes = new_world.active_planes();

    let old_wards = old.active_wards();
    let new_wards = new_world.active_wards();

    assert_eq!(
        old_planes.len(),
        new_planes.len(),
        "active_planes count mismatch in {filename}: old={} new={}",
        old_planes.len(),
        new_planes.len()
    );

    let mut old_ids: Vec<PlaneId> = old_planes.keys().copied().collect();
    old_ids.sort();
    for id in &old_ids {
        let old_p = &old_planes[id];
        let new_p = new_planes.get(id).unwrap_or_else(|| {
            panic!("plane {id:?} in old but missing from new (file={filename})")
        });
        assert_eq!(
            old_p, new_p,
            "ActivePlane mismatch for plane_id={id:?} in {filename}"
        );
    }

    assert_eq!(
        old_wards.len(),
        new_wards.len(),
        "active_wards count mismatch in {filename}: old={} new={}",
        old_wards.len(),
        new_wards.len()
    );

    let mut ward_ids: Vec<PlaneId> = old_wards.keys().copied().collect();
    ward_ids.sort();
    for id in &ward_ids {
        let old_w = &old_wards[id];
        let new_w = new_wards.get(id).unwrap_or_else(|| {
            panic!("ward {id:?} in old but missing from new (file={filename})")
        });
        assert_eq!(
            old_w, new_w,
            "ActiveWard mismatch for plane_id={id:?} in {filename}"
        );
    }
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_10695045)), ignore)]
fn parity_aviation_ocean_cv() {
    run_parity("20251001_145225_PBSA710-Ocean_28_naval_mission.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_aviation_vermont_pvp() {
    run_parity("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}
