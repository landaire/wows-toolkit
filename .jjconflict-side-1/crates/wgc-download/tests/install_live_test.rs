//! Live network test against the real WG webseed CDN. Ignored by default;
//! run explicitly with:
//!
//!     cargo test -p wgc-download -- --ignored
//!
//! Downloads and 7z-extracts the real ~70MB FULL locale package
//! `wows.na_15.8.0.0.13090112_locale.dspkg` for build
//! `wows_15.8.0.0.13090112_na`, proving `download_and_install` works
//! end-to-end against WG's real CDN and a real, full-size WG `.dspkg`
//! (not just the 181-byte hotfix fixture).

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use wgc_download::install::InstallProgress;
use wgc_download::install::InstallRequest;
use wgc_download::install::download_and_install;
use wgc_download::torrent::TorrentMeta;
use wgc_download::torrent::parse_torrent;
use wgc_download::webseed::DownloadControl;

const BUILD_TORRENT: &[u8] = include_bytes!("fixtures/build.torrent");
const WEB_SEED_BASE: &str = "http://dl-wows-gc.wargaming.net/projects/PT/ru/patches/";

#[tokio::test]
#[ignore = "hits the real WG CDN and downloads ~70MB; run with `cargo test -p wgc-download -- --ignored`"]
async fn downloads_and_installs_the_real_locale_package() {
    let full_torrent = parse_torrent(BUILD_TORRENT).expect("build.torrent fixture should parse");

    let locale = full_torrent
        .files
        .iter()
        .find(|f| f.relative_path().ends_with("_locale.dspkg"))
        .expect("fixture should contain the locale dspkg entry")
        .clone();

    // Only fetch the locale file, not the ~92GB build the fixture
    // otherwise describes.
    let single_file_torrent = TorrentMeta {
        name: full_torrent.name.clone(),
        piece_length: full_torrent.piece_length,
        total_length: locale.length,
        files: vec![locale.clone()],
        unpacked_sizes: Default::default(),
    };

    let run_id = std::process::id();
    let staging_dir = std::env::temp_dir().join(format!("wgc-download-install-live-test-staging-{run_id}"));
    let install_dir = std::env::temp_dir().join(format!("wgc-download-install-live-test-install-{run_id}"));
    std::fs::create_dir_all(&staging_dir).expect("create staging dir");
    std::fs::create_dir_all(&install_dir).expect("create install dir");

    let req = InstallRequest {
        staging_dir: staging_dir.clone(),
        install_dir: install_dir.clone(),
        torrent: single_file_torrent,
        web_seed_base: WEB_SEED_BASE.to_string(),
        is_steam_install: false,
    };
    let client = reqwest::Client::new();
    let control = DownloadControl {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        pause: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    let unpacked_events = AtomicUsize::new(0);
    let outcome = download_and_install(&client, &req, &control, |p| {
        if matches!(p, InstallProgress::Unpacked { .. }) {
            unpacked_events.fetch_add(1, Ordering::SeqCst);
        }
    })
    .await
    .expect("live download+install of the real locale package should succeed");

    assert!(outcome.skipped_deltas.is_empty(), "locale is a FULL package, nothing should be skipped as delta");
    assert!(!outcome.extracted_files.is_empty(), "extracting the real locale package should yield real files");
    assert_eq!(unpacked_events.load(Ordering::SeqCst), 1, "exactly one .dspkg was unpacked");

    for path in &outcome.extracted_files {
        assert!(path.starts_with(&install_dir), "extracted file should live under install_dir: {}", path.display());
        assert!(path.is_file(), "extracted path should exist on disk: {}", path.display());
    }

    let _ = std::fs::remove_dir_all(&staging_dir);
    let _ = std::fs::remove_dir_all(&install_dir);
}
