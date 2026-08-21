//! Live network test against the real WG CDN. Ignored by default; run
//! explicitly with:
//!
//!     cargo test -p wgc-download -- --ignored
//!
//! Fetches the real `.torrent` manifest for build `wows_15.8.0.0.13090112_na`
//! (same build the other fixtures in this crate cover) and checks it parses
//! to the same shape as the local `build.torrent` fixture.

use wgc_download::fetch_torrent;

const TORRENT_URL: &str = "https://dl-wows-gc.wargaming.net/projects/PT/ru/patches/wows_15.8.0.0.13090112_na/wows_15.8.0.0.13090112_na.torrent";

#[tokio::test]
#[ignore = "hits the real WG CDN; run with `cargo test -p wgc-download -- --ignored`"]
async fn fetches_and_parses_the_real_torrent() {
    let client = reqwest::Client::new();
    let meta = fetch_torrent(&client, TORRENT_URL).await.expect("live torrent fetch should succeed");

    assert!(meta.name.starts_with("wows_15.8.0.0.13090112"), "unexpected name: {}", meta.name);
    assert_eq!(meta.total_length, 92_546_639_270);
    assert_eq!(meta.files.len(), 6);
}
