use std::fs;

use wgc_launch::proton::find_proton_in_library;

#[test]
fn finds_proton_by_scanning_common() {
    let lib = tempfile::tempdir().unwrap();
    let proton_dir = lib.path().join("steamapps/common/Proton 9.0");
    fs::create_dir_all(&proton_dir).unwrap();
    fs::write(proton_dir.join("proton"), b"#!/bin/sh\n").unwrap();

    let compat = tempfile::tempdir().unwrap(); // no config_info -> falls back to scan
    let found = find_proton_in_library(lib.path(), compat.path()).expect("found");
    assert!(found.ends_with("Proton 9.0/proton"));
}

#[test]
fn scan_prefers_numeric_newest_over_lexical() {
    let lib = tempfile::tempdir().unwrap();
    let proton_9 = lib.path().join("steamapps/common/Proton 9.0");
    let proton_10 = lib.path().join("steamapps/common/Proton 10.0");
    fs::create_dir_all(&proton_9).unwrap();
    fs::create_dir_all(&proton_10).unwrap();
    fs::write(proton_9.join("proton"), b"#!/bin/sh\n").unwrap();
    fs::write(proton_10.join("proton"), b"#!/bin/sh\n").unwrap();

    let compat = tempfile::tempdir().unwrap(); // no config_info -> falls back to scan
    let found = find_proton_in_library(lib.path(), compat.path()).expect("found");
    // Lexical sort would pick "Proton 9.0" (since '9' > '1'); numeric sort must pick 10.0.
    assert!(found.ends_with("Proton 10.0/proton"), "expected Proton 10.0, got {}", found.display());
}

#[test]
fn config_info_primary_path_wins_over_scan() {
    let lib = tempfile::tempdir().unwrap();
    // Put a decoy in the scan path that would otherwise be picked.
    let decoy = lib.path().join("steamapps/common/Proton 10.0");
    fs::create_dir_all(&decoy).unwrap();
    fs::write(decoy.join("proton"), b"#!/bin/sh\n").unwrap();

    let configured = tempfile::tempdir().unwrap();
    fs::write(configured.path().join("proton"), b"#!/bin/sh\n").unwrap();

    let compat = tempfile::tempdir().unwrap();
    fs::write(compat.path().join("config_info"), format!("{}\n", configured.path().display())).unwrap();

    let found = find_proton_in_library(lib.path(), compat.path()).expect("found");
    assert_eq!(found, configured.path().join("proton"));
}
