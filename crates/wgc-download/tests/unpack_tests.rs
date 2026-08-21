//! `unpack_dspkg` / `is_delta_package` tests against the real 181-byte
//! `hotfix.dspkg` fixture (a genuine WG 7z archive containing a single
//! `placeholder.txt`) and a synthesized malicious 7z built in-process with
//! `sevenz-rust2`'s writer.

use std::io::Cursor;
use std::path::PathBuf;

use sevenz_rust2::ArchiveEntry;
use sevenz_rust2::ArchiveWriter;
use wgc_download::error::DownloadError;
use wgc_download::is_delta_package;
use wgc_download::unpack_dspkg;

const HOTFIX_DSPKG: &[u8] = include_bytes!("fixtures/hotfix.dspkg");

fn tempdir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("wgc-download-unpack-test-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

fn cleanup(dir: &std::path::Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn unpacks_the_real_hotfix_fixture() {
    let staging_dir = tempdir("hotfix-src");
    let dspkg_path = staging_dir.join("wows.na_15.8.0.0.13090112_hotfix.dspkg");
    std::fs::write(&dspkg_path, HOTFIX_DSPKG).expect("write fixture to staging");

    let dest_dir = tempdir("hotfix-dest");
    let extracted = unpack_dspkg(&dspkg_path, &dest_dir).expect("hotfix fixture should unpack");

    assert_eq!(extracted, vec![dest_dir.join("placeholder.txt")]);

    let content = std::fs::read_to_string(dest_dir.join("placeholder.txt")).expect("placeholder.txt should exist");
    assert_eq!(content, "This is release hotfix patch directory.");

    cleanup(&staging_dir);
    cleanup(&dest_dir);
}

#[test]
fn is_delta_package_distinguishes_full_from_delta_names() {
    assert!(!is_delta_package("wows.na_15.8.0.0.13090112_client.dspkg"), "full package should not be a delta");
    assert!(
        is_delta_package("wows.na_15.8.0.0.13090112_15.7.0.0.13016296_client.dspkg"),
        "delta package (two version tokens) should be detected"
    );
}

#[test]
fn unpack_dspkg_rejects_a_delta_named_package() {
    let staging_dir = tempdir("delta-src");
    // Content does not matter: the delta check runs on the file name before
    // the archive is even opened.
    let dspkg_path = staging_dir.join("wows.na_15.8.0.0.13090112_15.7.0.0.13016296_client.dspkg");
    std::fs::write(&dspkg_path, HOTFIX_DSPKG).expect("write fixture to staging");

    let dest_dir = tempdir("delta-dest");
    let err = unpack_dspkg(&dspkg_path, &dest_dir).expect_err("delta-named package should be rejected");
    match err {
        DownloadError::DeltaUnsupported { file } => {
            assert_eq!(file, "wows.na_15.8.0.0.13090112_15.7.0.0.13016296_client.dspkg");
        }
        other => panic!("expected DeltaUnsupported, got {other:?}"),
    }

    cleanup(&staging_dir);
    cleanup(&dest_dir);
}

/// Builds a 7z archive in-process (via `sevenz-rust2`'s writer, a
/// dev-only feature) with one entry named `../evil.txt` - a Zip-Slip-style
/// path-traversal attempt - and proves `unpack_dspkg` refuses it and
/// writes nothing outside `dest_dir`.
#[test]
fn unpack_dspkg_rejects_path_traversal_entry() {
    let mut archive_bytes = Vec::new();
    {
        let mut writer = ArchiveWriter::new(Cursor::new(&mut archive_bytes)).expect("create archive writer");
        let entry = ArchiveEntry::new_file("../evil.txt");
        writer.push_archive_entry(entry, Some(Cursor::new(b"pwned".to_vec()))).expect("push malicious entry");
        writer.finish().expect("finish archive");
    }

    let staging_dir = tempdir("traversal-src");
    // Full-package-shaped name so the delta check does not short-circuit
    // before the archive itself is opened.
    let dspkg_path = staging_dir.join("wows.na_1.2.3.0.4_client.dspkg");
    std::fs::write(&dspkg_path, &archive_bytes).expect("write malicious archive");

    let dest_dir = tempdir("traversal-dest");
    let result = unpack_dspkg(&dspkg_path, &dest_dir);
    assert!(matches!(result, Err(DownloadError::UnsafePath { .. })), "expected UnsafePath, got {result:?}");

    let traversal_target = dest_dir.parent().unwrap().join("evil.txt");
    assert!(!traversal_target.exists(), "traversal target must not be written: {}", traversal_target.display());

    cleanup(&staging_dir);
    cleanup(&dest_dir);
    let _ = std::fs::remove_file(&traversal_target);
}
