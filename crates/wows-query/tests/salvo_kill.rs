//! `salvo_kill` over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.

use std::path::PathBuf;
use std::sync::LazyLock;

use wows_core::units::Km;
use wows_query::GameData;
use wows_query::salvo_kill::AmmoSalvo;
use wows_query::salvo_kill::SalvoKillArgs;
use wows_query::shells::Ammo;

static GAME: LazyLock<GameData> = LazyLock::new(|| {
    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let cache = std::env::temp_dir().join("wows-query-test-cache");
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
        .expect("dump root has a readable builds.toml");
    let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
    GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
});

/// The conclusion states the verdict the best case computed.
fn assert_conclusion_matches(salvo: &AmmoSalvo) {
    let case = &salvo.best_case;
    match case.shells_to_sink {
        Some(needed) => {
            assert!(case.max_damage.value() >= case.ship_hp.value());
            let claim = format!("can sink it with {needed} of {} shells", salvo.shells_per_salvo);
            assert!(salvo.conclusion.contains(&claim), "{}", salvo.conclusion);
        }
        None => {
            assert!(case.max_damage.value() < case.ship_hp.value());
            assert!(salvo.conclusion.contains("cannot sink it"), "{}", salvo.conclusion);
        }
    }
    assert!(salvo.conclusion.contains(&format!("at most {} damage", case.max_damage)), "{}", salvo.conclusion);
}

/// Piemonte's SAP against a broadside G. Hoffmann at the default 10 km.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn piemonte_sap_against_a_broadside_hoffmann() {
    let output = GAME
        .salvo_kill(&SalvoKillArgs {
            shooter: "Piemonte".to_owned(),
            target: "G. Hoffmann".to_owned(),
            ammo: Some(Ammo::Sap),
            ..Default::default()
        })
        .expect("Piemonte's SAP traces against G. Hoffmann");

    let [salvo] = output.salvos.as_slice() else {
        panic!("one ammo type was asked for");
    };
    assert_eq!(salvo.shells_per_salvo, 12);
    assert_conclusion_matches(salvo);
    insta::assert_yaml_snapshot!(output);
}

/// Yamato's AP against a broadside Des Moines at 12 km.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn yamato_ap_against_a_broadside_des_moines() {
    let output = GAME
        .salvo_kill(&SalvoKillArgs {
            shooter: "Yamato".to_owned(),
            target: "Des Moines".to_owned(),
            ammo: Some(Ammo::Ap),
            range_km: Some(Km::new(12.0)),
            ..Default::default()
        })
        .expect("Yamato's AP traces against Des Moines");

    let [salvo] = output.salvos.as_slice() else {
        panic!("one ammo type was asked for");
    };
    assert_conclusion_matches(salvo);
    insta::assert_yaml_snapshot!(output);
}

/// Ammo the shooter's main battery does not fire is an error naming what it
/// fires.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ammo_the_main_battery_does_not_fire() {
    let error = GAME
        .salvo_kill(&SalvoKillArgs {
            shooter: "Yamato".to_owned(),
            target: "Des Moines".to_owned(),
            ammo: Some(Ammo::Sap),
            ..Default::default()
        })
        .expect_err("Yamato fires no SAP");

    insta::assert_snapshot!(error.to_string());
}
