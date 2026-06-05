#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use std::collections::HashMap;

use wows_replays::analyzer::battle_controller::EntityKind as OldEntityKind;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_battle_world::components::EntityKind as NewEntityKind;

fn map_old_kind(k: OldEntityKind) -> NewEntityKind {
    match k {
        OldEntityKind::Vehicle => NewEntityKind::Vehicle,
        OldEntityKind::Building => NewEntityKind::Building,
        OldEntityKind::SmokeScreen => NewEntityKind::SmokeScreen,
    }
}

fn run_parity(filename: &str) {
    let (old, mut new_world) = support::both(filename);

    let old_entities = old.entities_by_id();
    let new_kinds: HashMap<_, _> = new_world.entity_kinds().into_iter().collect();

    let old_ids: std::collections::BTreeSet<_> = old_entities.keys().copied().collect();
    let new_ids: std::collections::BTreeSet<_> = new_kinds.keys().copied().collect();
    assert_eq!(
        old_ids, new_ids,
        "entity id sets differ: old_only={:?} new_only={:?}",
        old_ids.difference(&new_ids).collect::<Vec<_>>(),
        new_ids.difference(&old_ids).collect::<Vec<_>>(),
    );

    for (id, old_entity) in old_entities {
        let old_kind = map_old_kind(old_entity.kind());
        let new_kind = *new_kinds.get(id).unwrap();
        assert_eq!(old_kind, new_kind, "entity kind mismatch for id={id:?}");
    }

    // Position parity.
    let old_positions = old.ship_positions();
    let new_positions: HashMap<_, _> = new_world.positions().into_iter().collect();

    let old_pos_ids: std::collections::BTreeSet<_> = old_positions.keys().copied().collect();
    let new_pos_ids: std::collections::BTreeSet<_> = new_positions.keys().copied().collect();
    assert_eq!(
        old_pos_ids, new_pos_ids,
        "position id sets differ: old_only={:?} new_only={:?}",
        old_pos_ids.difference(&new_pos_ids).collect::<Vec<_>>(),
        new_pos_ids.difference(&old_pos_ids).collect::<Vec<_>>(),
    );

    for (id, old_sp) in old_positions {
        let new_t = new_positions.get(id).unwrap();
        assert_eq!(
            old_sp.position.x, new_t.pos.x,
            "x mismatch for id={id:?}"
        );
        assert_eq!(
            old_sp.position.y, new_t.pos.y,
            "y mismatch for id={id:?}"
        );
        assert_eq!(
            old_sp.position.z, new_t.pos.z,
            "z mismatch for id={id:?}"
        );
        assert_eq!(
            old_sp.yaw, new_t.yaw.0,
            "yaw mismatch for id={id:?}"
        );
        assert_eq!(
            old_sp.pitch, new_t.pitch.0,
            "pitch mismatch for id={id:?}"
        );
        assert_eq!(
            old_sp.roll, new_t.roll.0,
            "roll mismatch for id={id:?}"
        );
    }

    // Minimap parity.
    let old_minimap = old.minimap_positions();
    let new_minimap: HashMap<_, _> = new_world.minimap().into_iter().collect();

    let old_mm_ids: std::collections::BTreeSet<_> = old_minimap.keys().copied().collect();
    let new_mm_ids: std::collections::BTreeSet<_> = new_minimap.keys().copied().collect();
    assert_eq!(
        old_mm_ids, new_mm_ids,
        "minimap id sets differ: old_only={:?} new_only={:?}",
        old_mm_ids.difference(&new_mm_ids).collect::<Vec<_>>(),
        new_mm_ids.difference(&old_mm_ids).collect::<Vec<_>>(),
    );

    for (id, old_mm) in old_minimap {
        let new_mm = new_minimap.get(id).unwrap();
        assert_eq!(
            old_mm.position.x, new_mm.pos.x,
            "minimap x mismatch for id={id:?}"
        );
        assert_eq!(
            old_mm.position.y, new_mm.pos.y,
            "minimap y mismatch for id={id:?}"
        );
        assert_eq!(
            old_mm.heading, new_mm.heading.0,
            "heading mismatch for id={id:?}"
        );
        assert_eq!(
            old_mm.visible, new_mm.visible,
            "visible mismatch for id={id:?}"
        );
        assert_eq!(
            old_mm.visibility_flags, new_mm.visibility_flags.0,
            "visibility_flags mismatch for id={id:?}"
        );
        assert_eq!(
            old_mm.is_invisible, new_mm.is_invisible,
            "is_invisible mismatch for id={id:?}"
        );
    }
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_vermont_pvp() {
    run_parity("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_8260685)), ignore)]
fn parity_v170_pvp() {
    run_parity("20240422_161541_PGSD104-V-170_08_NE_passage.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn parity_shimakaze_pvp_v0_9() {
    run_parity("20200117_205708_PJSD012-Shimakaze-1943_45_Zigzag.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_7266701)), ignore)]
fn parity_operation_atoll_pve() {
    run_parity("20230813_200638_PJSC717-Yellow-Dragon_s06_Atoll.wowsreplay");
}
