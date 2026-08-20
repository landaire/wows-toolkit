//! Live network test against the real WG webseed CDN. Ignored by default;
//! run explicitly with:
//!
//!     cargo test -p wgc-download -- --ignored
//!
//! Downloads the real 181-byte hotfix `.dspkg` for build
//! `wows_15.8.0.0.13090112_na` and verifies it against the sha1 recorded
//! in the real `build.torrent` fixture (`tests/fixtures/build.torrent`,
//! same fixture `torrent_tests.rs` parses).

use sha1::Digest;
use sha1::Sha1;
use wgc_download::torrent::TorrentMeta;
use wgc_download::torrent::parse_torrent;
use wgc_download::webseed::DownloadRequest;
use wgc_download::webseed::download;

const BUILD_TORRENT: &[u8] = include_bytes!("fixtures/build.torrent");
const WEB_SEED_BASE: &str = "http://dl-wows-gc.wargaming.net/projects/PT/ru/patches/";

#[tokio::test]
#[ignore = "hits the real WG CDN; run with `cargo test -p wgc-download -- --ignored`"]
async fn downloads_and_verifies_the_real_hotfix_file() {
    let full_torrent = parse_torrent(BUILD_TORRENT).expect("build.torrent fixture should parse");

    let hotfix = full_torrent
        .files
        .iter()
        .find(|f| f.relative_path().ends_with("_hotfix.dspkg"))
        .expect("fixture should contain the hotfix dspkg entry")
        .clone();
    let expected_sha1 = hotfix.sha1.clone().expect("hotfix entry should carry a sha1");

    // Only fetch the tiny hotfix file, not the ~67GB client entry also
    // listed in the fixture: reuse the fixture's torrent `name` (needed
    // for URL construction) but scope `files` to just this one entry.
    let single_file_torrent = TorrentMeta {
        name: full_torrent.name.clone(),
        piece_length: full_torrent.piece_length,
        total_length: hotfix.length,
        files: vec![hotfix.clone()],
        unpacked_sizes: Default::default(),
    };

    let staging_dir = std::env::temp_dir().join(format!("wgc-download-live-test-{}", std::process::id()));
    std::fs::create_dir_all(&staging_dir).expect("create staging dir");

    let req = DownloadRequest {
        staging_dir: staging_dir.clone(),
        torrent: single_file_torrent,
        web_seed_base: WEB_SEED_BASE.to_string(),
        is_steam_install: false,
    };
    let client = reqwest::Client::new();

    download(&client, &req, |_| {}).await.expect("live download of the hotfix file should succeed and verify");

    let dest = hotfix.path.iter().fold(staging_dir.clone(), |acc, segment| acc.join(segment));
    let bytes = std::fs::read(&dest).expect("downloaded hotfix file should exist");
    assert_eq!(bytes.len() as u64, hotfix.length);

    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    let got_sha1 = hex::encode(hasher.finalize());
    assert_eq!(got_sha1, expected_sha1, "downloaded file's sha1 should match the fixture's torrent manifest");

    let _ = std::fs::remove_dir_all(&staging_dir);
}
