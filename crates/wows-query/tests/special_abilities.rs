//! Ship special abilities over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.

use std::path::PathBuf;
use std::sync::LazyLock;

use wows_query::GameData;
use wows_query::QueryError;
use wows_query::ships::ShipArgs;
use wows_query::special_abilities::AbilityKind;
use wows_query::special_abilities::SpecialAbilitySearchArgs;
use wows_query::stats::ShipStatsArgs;

static GAME: LazyLock<GameData> = LazyLock::new(|| {
    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let cache = std::env::temp_dir().join("wows-query-test-cache");
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
        .expect("dump root has a readable builds.toml");
    let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
    GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
});

fn game() -> &'static GameData {
    &GAME
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn special_ability_of_each_kind() {
    for (ship, snapshot) in [
        ("Conde", "special_ability_conde"),
        ("Annapolis", "special_ability_annapolis"),
        ("Libertad", "special_ability_libertad"),
        ("Le Havre", "special_ability_le_havre"),
        ("New Jersey", "special_ability_new_jersey"),
        ("Archerfish", "special_ability_archerfish"),
        ("Vyazma", "special_ability_vyazma"),
        ("Des Moines", "special_ability_des_moines"),
    ] {
        let ability = game().ship_special_ability(&ShipArgs { ship: ship.to_owned() }).expect("ship exists");
        insta::assert_yaml_snapshot!(snapshot, ability);
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ships_with_a_special_ability_by_kind() {
    for (kind, snapshot) in [
        (AbilityKind::CombatInstructions, "ships_with_combat_instructions"),
        (AbilityKind::AlternativeFireMode, "ships_with_alternative_fire_mode"),
    ] {
        let search = game()
            .special_ability_search(&SpecialAbilitySearchArgs { kind: Some(kind), ..Default::default() })
            .expect("search runs");
        insta::assert_yaml_snapshot!(snapshot, search);
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn special_ability_search_by_effect() {
    let search = game()
        .special_ability_search(&SpecialAbilitySearchArgs {
            effect: Some("secondary battery firing range".to_owned()),
            ..Default::default()
        })
        .expect("search runs");

    insta::assert_yaml_snapshot!(search);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ship_stats_with_the_special_ability_active() {
    for (ship, snapshot) in [
        ("Conde", "ship_stats_ability_conde"),
        ("Annapolis", "ship_stats_ability_annapolis"),
        ("Libertad", "ship_stats_ability_libertad"),
    ] {
        let stats = game()
            .ship_stats(&ShipStatsArgs {
                ship: ship.to_owned(),
                special_ability_active: Some(true),
                ..Default::default()
            })
            .expect("ship exists");
        insta::assert_yaml_snapshot!(snapshot, stats);
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ship_stats_refuses_an_ability_the_ship_lacks() {
    let error = game()
        .ship_stats(&ShipStatsArgs {
            ship: "Des Moines".to_owned(),
            special_ability_active: Some(true),
            ..Default::default()
        })
        .expect_err("Des Moines has no special ability");

    assert!(matches!(error, QueryError::NoSpecialAbility { ref ship } if ship == "Des Moines"));
}
