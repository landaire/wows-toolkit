use wgc_install::InstallSource;
use wgc_install::wgc::parse_preferences;

#[test]
fn parses_two_wgc_games_with_auto_update_flags() {
    let xml = include_str!("fixtures/preferences.xml");
    let installs = parse_preferences(xml).expect("parse");
    assert_eq!(installs.len(), 2);

    assert!(
        matches!(&installs[0].source, InstallSource::Wgc { working_dir } if working_dir.to_string_lossy().ends_with("World_of_Warships"))
    );
    assert!(installs[0].auto_update);
    assert!(!installs[1].auto_update);
}

use std::fs;
use wgc_install::wgc::discover_wgc_installs;

#[test]
fn discover_reads_preferences_from_override_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("preferences.xml"), include_str!("fixtures/preferences.xml")).unwrap();

    let installs = discover_wgc_installs(Some(dir.path())).expect("discover");
    assert_eq!(installs.len(), 2);
}

#[test]
fn discover_errors_when_override_dir_missing() {
    let missing = std::path::Path::new("/definitely/not/here/gamecenter");
    assert!(matches!(discover_wgc_installs(Some(missing)), Err(wgc_install::InstallError::WgcDataDirMissing)));
}
