//! Live-network smoke test against the real WGUS PT (Public Test) server.
//!
//! Excluded from the default test run so `cargo test -p wgc-catalog` and
//! CI never touch the network. Run explicitly with:
//!
//!     cargo test -p wgc-catalog -- --ignored

use wgc_catalog::CatalogClient;
use wgc_catalog::game_info::parse_game_info;

const GAME_INFO_XML: &str = include_str!("fixtures/game_info.xml");

#[tokio::test]
#[ignore = "hits the real wgus-wowspt.worldofwarships.eu server; run with `cargo test -p wgc-catalog -- --ignored`"]
async fn check_updates_against_the_real_wgus_pt_server() {
    let game_info = parse_game_info(GAME_INFO_XML).expect("game_info.xml should parse");
    assert_eq!(game_info.update_base_url.as_str(), "https://wgus-wowspt.worldofwarships.eu/");
    assert_eq!(game_info.game_id, "WOWS.PT.PRODUCTION");

    let client = CatalogClient::new().expect("client should build");
    let build = client.check_updates(&game_info).await.expect("check_updates should succeed against the live server");

    assert!(!build.parts.is_empty(), "expected at least one part in the live response");
    for (part, status) in &build.parts {
        println!("{part}: {status:?}");
    }
    for download in &build.downloads {
        assert!(!download.info_hash.as_str().is_empty());
        assert!(!download.torrent_urls.is_empty(), "part {:?} has no torrent urls", download.part);
    }
}
