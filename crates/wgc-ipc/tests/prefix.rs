use std::path::Path;

use wgc_ipc::PrefixError;
use wgc_ipc::RelayBinaries;
use wgc_ipc::RelayBinary;
use wgc_ipc::prepare_prefix;

fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                files.push((relative, std::fs::read(&path).unwrap()));
            }
        }
    }
    files.sort();

    files
}

#[test]
fn writes_the_fake_install_and_is_idempotent() {
    let shipped = tempfile::tempdir().unwrap();
    let relay = shipped.path().join("wgc-relay.exe");
    let placeholder = shipped.path().join("wgc-placeholder.exe");
    std::fs::write(&relay, b"relay").unwrap();
    std::fs::write(&placeholder, b"placeholder").unwrap();
    let binaries = RelayBinaries::new(relay, placeholder);
    let drive_c = tempfile::tempdir().unwrap();

    let prepared = prepare_prefix(drive_c.path(), &binaries).unwrap();
    let first = tree(drive_c.path());
    let modified = std::fs::metadata(&prepared.relay_exe).unwrap().modified().unwrap();
    let again = prepare_prefix(drive_c.path(), &binaries).unwrap();

    assert_eq!(again, prepared);
    assert_eq!(tree(drive_c.path()), first);
    assert_eq!(std::fs::metadata(&prepared.relay_exe).unwrap().modified().unwrap(), modified);
    assert_eq!(prepared.relay_exe, drive_c.path().join("ProgramData/Wargaming.net/GameCenter/wgc-relay.exe"));
    let listing: Vec<_> =
        first.iter().map(|(path, bytes)| format!("{path}: {}", String::from_utf8_lossy(bytes))).collect();
    insta::assert_debug_snapshot!(listing, @r#"
    [
        "ProgramData/Wargaming.net/GameCenter/data/wgc_path.dat: C:\\ProgramData\\Wargaming.net\\GameCenter",
        "ProgramData/Wargaming.net/GameCenter/wgc-relay.exe: relay",
        "ProgramData/Wargaming.net/GameCenter/wgc.exe: placeholder",
    ]
    "#);
}

#[test]
fn replaces_a_changed_relay() {
    let shipped = tempfile::tempdir().unwrap();
    let relay = shipped.path().join("wgc-relay.exe");
    let placeholder = shipped.path().join("wgc-placeholder.exe");
    std::fs::write(&relay, b"old").unwrap();
    std::fs::write(&placeholder, b"placeholder").unwrap();
    let binaries = RelayBinaries::new(relay.clone(), placeholder);
    let drive_c = tempfile::tempdir().unwrap();
    prepare_prefix(drive_c.path(), &binaries).unwrap();

    std::fs::write(&relay, b"new").unwrap();
    let prepared = prepare_prefix(drive_c.path(), &binaries).unwrap();

    assert_eq!(std::fs::read(prepared.relay_exe).unwrap(), b"new");
}

#[test]
fn locates_binaries_beside_the_executable() {
    let shipped = tempfile::tempdir().unwrap();
    for name in ["wows_toolkit", "wgc-relay.exe", "wgc-placeholder.exe"] {
        std::fs::write(shipped.path().join(name), name).unwrap();
    }

    let binaries = RelayBinaries::beside(&shipped.path().join("wows_toolkit")).unwrap();

    assert_eq!(
        binaries,
        RelayBinaries::new(shipped.path().join("wgc-relay.exe"), shipped.path().join("wgc-placeholder.exe"))
    );
}

/// The layout `buck2 run` leaves: each resource is another target's output,
/// listed in `<exe>.resources.json` relative to the executable's directory.
#[test]
fn locates_binaries_through_the_buck_resource_database() {
    let out = tempfile::tempdir().unwrap();
    let exe_dir = out.path().join("crates/wows-toolkit/__zgui_bin__");
    let relay_dir = out.path().join("crates/wgc-relay/__wgc_relay_bin__");
    let placeholder_dir = out.path().join("crates/wgc-placeholder/__wgc_placeholder_bin__");
    for dir in [&exe_dir, &relay_dir, &placeholder_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let exe = exe_dir.join("zgui_bin");
    std::fs::write(&exe, b"zgui").unwrap();
    std::fs::write(relay_dir.join("wgc_relay.exe"), b"relay").unwrap();
    std::fs::write(placeholder_dir.join("wgc_placeholder.exe"), b"placeholder").unwrap();
    std::fs::write(
        exe_dir.join("zgui_bin.resources.json"),
        r#"{
            "crates/wows-toolkit/wgc-placeholder.exe": "../../wgc-placeholder/__wgc_placeholder_bin__/wgc_placeholder.exe",
            "crates/wows-toolkit/wgc-relay.exe": "../../wgc-relay/__wgc_relay_bin__/wgc_relay.exe"
        }"#,
    )
    .unwrap();

    let binaries = RelayBinaries::beside(&exe).unwrap();

    assert_eq!(std::fs::read(binaries.relay).unwrap(), b"relay");
    assert_eq!(std::fs::read(binaries.placeholder).unwrap(), b"placeholder");
}

#[test]
fn names_the_missing_binary() {
    let shipped = tempfile::tempdir().unwrap();
    std::fs::write(shipped.path().join("wows_toolkit"), b"toolkit").unwrap();
    std::fs::write(shipped.path().join("wgc-relay.exe"), b"relay").unwrap();

    let err = RelayBinaries::beside(&shipped.path().join("wows_toolkit")).unwrap_err();

    assert!(
        matches!(&err, PrefixError::BinaryMissing { binary: RelayBinary::Placeholder, dir } if dir == shipped.path()),
        "{err:?}"
    );
    assert!(err.to_string().starts_with("wgc-placeholder.exe is missing from "), "{err}");
}
