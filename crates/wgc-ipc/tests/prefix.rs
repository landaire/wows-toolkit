use std::path::Path;

use wgc_ipc::RelayBinaries;
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
