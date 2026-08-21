//! Live-network smoke tests: enumerate a full install from a fresh (nothing
//! installed) `GameInfo`, built only from a `known_games` descriptor - no
//! on-disk `game_info.xml`, no login/token.
//!
//! Excluded from the default test run so `cargo test -p wgc-catalog` and CI
//! never touch the network. Run explicitly with:
//!
//!     cargo test -p wgc-catalog --test fresh_install_live_test -- --ignored --nocapture

use wgc_catalog::CatalogClient;
use wgc_catalog::known_games::known_games;
use wgc_catalog::update::PartStatus;

async fn assert_fresh_install_enumerates(game_id: &str) {
    let game = known_games().iter().find(|g| g.game_id == game_id).expect("descriptor present in known_games");
    let game_info = game.to_fresh_game_info().expect("known descriptor's URL should parse");
    assert!(game_info.installed_parts.is_empty());

    let client = CatalogClient::new().expect("client should build");
    let build = client
        .check_updates(&game_info)
        .await
        .expect("check_updates should succeed for a fresh install against the live server");

    assert!(!build.parts.is_empty(), "expected at least one part in the live response");
    for (part, status) in &build.parts {
        println!("{part}: {status:?}");
        match status {
            PartStatus::NotInstalled { to } => println!("  -> not installed, latest available: {to}"),
            other => panic!("expected every part to be NotInstalled for a fresh install, got {part}: {other:?}"),
        }
    }

    assert!(!build.downloads.is_empty(), "expected downloads for a full fresh install");
    for download in &build.downloads {
        println!(
            "download {}: version_to={} torrent_urls={} web_seed_base={:?}",
            download.part,
            download.version_to,
            download.torrent_urls.len(),
            download.web_seed_base
        );
        assert!(
            !download.torrent_urls.is_empty() || download.web_seed_base.is_some(),
            "part {:?} has neither torrent urls nor a web seed base",
            download.part
        );
    }
}

#[tokio::test]
#[ignore = "hits the real wgus-wowspt.worldofwarships.eu server; run with `cargo test -p wgc-catalog -- --ignored`"]
async fn fresh_install_enumerates_a_full_pt_build() {
    assert_fresh_install_enumerates("WOWS.PT.PRODUCTION").await;
}

#[tokio::test]
#[ignore = "hits the real wgus-eu.wargaming.net server; run with `cargo test -p wgc-catalog -- --ignored`"]
async fn fresh_install_enumerates_a_full_ww_build() {
    assert_fresh_install_enumerates("WOWS.WW.PRODUCTION").await;
}
