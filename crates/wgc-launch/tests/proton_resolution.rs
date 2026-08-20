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
