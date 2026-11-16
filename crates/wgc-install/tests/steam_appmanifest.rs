use std::path::Path;

use wgc_install::steam::AppManifest;

#[test]
fn appmanifests_report_pending_updates() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/appmanifest");
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    names.sort();

    let report: Vec<String> = names
        .iter()
        .map(|name| {
            let text = std::fs::read_to_string(dir.join(name)).unwrap();
            let parsed = AppManifest::parse(&text).map(|manifest| {
                let pending = manifest.pending_update();
                (manifest, pending)
            });
            format!("{}: {parsed:?}", name.to_string_lossy())
        })
        .collect();

    insta::assert_debug_snapshot!(report);
}
