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
