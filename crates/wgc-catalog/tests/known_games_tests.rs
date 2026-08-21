//! No live network: `GameInfo::for_fresh_install` and the built-in
//! `known_games` registry, checked structurally.

use wgc_catalog::game_info::GameInfo;
use wgc_catalog::known_games::AccountRealmHint;
use wgc_catalog::known_games::known_games;

#[test]
fn for_fresh_install_has_no_installed_parts() {
    let base = url::Url::parse("https://example.test").expect("static URL parses");
    let info = GameInfo::for_fresh_install("TEST.GAME".to_string(), base.clone(), "EN".to_string());

    assert_eq!(info.game_id, "TEST.GAME");
    assert_eq!(info.update_base_url, base);
    assert_eq!(info.localization, "EN");
    assert!(info.installed_parts.is_empty());
}

#[test]
fn known_games_has_the_two_seeded_wows_descriptors() {
    let games = known_games();
    assert_eq!(games.len(), 2);

    let ww = games.iter().find(|g| g.game_id == "WOWS.WW.PRODUCTION").expect("WW descriptor present");
    assert_eq!(ww.update_base_url, "https://wgus-eu.wargaming.net");
    assert_eq!(ww.realm, AccountRealmHint::Eu);

    let pt = games.iter().find(|g| g.game_id == "WOWS.PT.PRODUCTION").expect("PT descriptor present");
    assert_eq!(pt.update_base_url, "https://wgus-wowspt.worldofwarships.eu");
    assert_eq!(pt.realm, AccountRealmHint::Eu);

    for game in games {
        let info = game.to_fresh_game_info().expect("known game's update_base_url should parse as a URL");
        assert_eq!(info.game_id, game.game_id);
        assert!(info.installed_parts.is_empty());
    }
}
