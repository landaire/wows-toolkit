use wgc_catalog::game_info::parse_game_info;
use wgc_catalog::models::PartName;

const GAME_INFO_XML: &str = include_str!("fixtures/game_info.xml");

#[test]
fn parses_real_game_info_fixture() {
    let info = parse_game_info(GAME_INFO_XML).expect("game_info.xml should parse");

    assert_eq!(info.game_id, "WOWS.PT.PRODUCTION");
    assert_eq!(info.update_base_url.host_str(), Some("wgus-wowspt.worldofwarships.eu"));
    assert_eq!(info.installed_parts.len(), 4);

    let locale = info.installed_parts.iter().find(|p| p.name == PartName::Locale).expect("locale part present");
    assert_ne!(locale.installed, locale.available);
}
