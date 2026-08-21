use wgc_catalog::game_info::parse_game_info;
use wgc_catalog::models::ChainType;
use wgc_catalog::models::PartName;
use wgc_catalog::models::parse_patches_chain;
use wgc_catalog::update::PartStatus;
use wgc_catalog::update::diff_parts;
use wgc_catalog::update::latest_build_from_responses;

const GAME_INFO_XML: &str = include_str!("fixtures/game_info.xml");
const PATCHES_CHAIN_XML: &str = include_str!("fixtures/patches_chain.xml");

/// No live network: this drives the pure update-detection logic against the
/// real fixtures captured from WGUS (see report A). `game_info.xml`'s
/// installed part versions are all `14.9.x`; `patches_chain.xml` was
/// captured separately with `*_current_version=0` (asking for the full
/// newest chain), so every part in this fixture pair legitimately differs
/// -> every part is `UpdateAvailable`, not just `locale`.
#[test]
fn detects_updates_from_real_fixtures() {
    let game_info = parse_game_info(GAME_INFO_XML).expect("game_info.xml should parse");
    let patches = parse_patches_chain(PATCHES_CHAIN_XML).expect("patches_chain.xml should parse");

    let build = latest_build_from_responses(&game_info, &patches).expect("fixture pair has an install chain");

    assert_eq!(build.chain_type, ChainType::Install);
    assert_eq!(build.parts.len(), 4);

    let status_for =
        |name: &PartName| -> &PartStatus { &build.parts.iter().find(|(n, _)| n == name).expect("part present").1 };

    match status_for(&PartName::Locale) {
        PartStatus::UpdateAvailable { from, to } => {
            assert_eq!(from.as_str(), "14.9.0.0.10614298");
            assert_eq!(to.as_str(), "15.8.0.0.13095623");
        }
        other => panic!("expected locale to be UpdateAvailable, got {other:?}"),
    }

    match status_for(&PartName::Client) {
        PartStatus::UpdateAvailable { from, to } => {
            assert_eq!(from.as_str(), "14.9.0.0.10609091");
            assert_eq!(to.as_str(), "15.8.0.0.13090112");
        }
        other => panic!("expected client to be UpdateAvailable, got {other:?}"),
    }

    match status_for(&PartName::Sdcontent) {
        PartStatus::UpdateAvailable { from, to } => {
            assert_eq!(from.as_str(), "14.9.0.0.10609091");
            assert_eq!(to.as_str(), "15.8.0.0.13090112");
        }
        other => panic!("expected sdcontent to be UpdateAvailable, got {other:?}"),
    }

    match status_for(&PartName::Hotfix) {
        PartStatus::UpdateAvailable { from, to } => {
            assert_eq!(from.as_str(), "14.9.0.0.10609091");
            assert_eq!(to.as_str(), "15.8.0.0.13090112");
        }
        other => panic!("expected hotfix to be UpdateAvailable, got {other:?}"),
    }

    assert_eq!(build.downloads.len(), 4);
    for download in &build.downloads {
        assert!(!download.info_hash.as_str().is_empty(), "part {:?} has an empty info_hash", download.part);
        assert!(!download.torrent_urls.is_empty(), "part {:?} has no torrent urls", download.part);
        assert!(!download.files.is_empty(), "part {:?} has no files", download.part);
    }

    let client_download = build.downloads.iter().find(|d| d.part == PartName::Client).expect("client download");
    assert_eq!(
        client_download.web_seed_base.as_deref(),
        Some("http://dl-wows-gc.wargaming.net/projects/PT/ru/patches/")
    );

    // fixtures/patches_chain.xml's web_seeds/url carries threads="3".
    assert_eq!(build.web_seed_threads, Some(3));
}

/// A part with no matching entry in `game_info.installed_parts` is
/// `NotInstalled`, not silently treated as up to date.
#[test]
fn a_part_missing_from_game_info_is_not_installed() {
    let mut game_info = parse_game_info(GAME_INFO_XML).expect("game_info.xml should parse");
    game_info.installed_parts.retain(|p| p.name != PartName::Locale);
    let patches = parse_patches_chain(PATCHES_CHAIN_XML).expect("patches_chain.xml should parse");

    let build = latest_build_from_responses(&game_info, &patches).expect("fixture pair has an install chain");

    let (_, status) = build.parts.iter().find(|(n, _)| *n == PartName::Locale).expect("locale part present");
    match status {
        PartStatus::NotInstalled { to } => assert_eq!(to.as_str(), "15.8.0.0.13095623"),
        other => panic!("expected locale to be NotInstalled, got {other:?}"),
    }
}

/// An installed version equal to the chain's terminal version_to is up to date.
#[test]
fn a_matching_installed_version_is_up_to_date() {
    let mut game_info = parse_game_info(GAME_INFO_XML).expect("game_info.xml should parse");
    let patches = parse_patches_chain(PATCHES_CHAIN_XML).expect("patches_chain.xml should parse");
    for part in &mut game_info.installed_parts {
        if part.name == PartName::Client {
            part.installed = wgc_catalog::models::BuildVersion::new("15.8.0.0.13090112".to_string());
        }
    }

    let build = latest_build_from_responses(&game_info, &patches).expect("fixture pair has an install chain");

    let (_, status) = build.parts.iter().find(|(n, _)| *n == PartName::Client).expect("client part present");
    assert!(matches!(status, PartStatus::UpToDate), "expected UpToDate, got {status:?}");
}

const MINIMAL_GAME_INFO_XML: &str = r#"<protocol name="game_info" version="2.16">
  <id>TEST.GAME</id>
  <localization>EN</localization>
  <update_urls><value>https://example.test</value></update_urls>
  <part_versions></part_versions>
</protocol>"#;

/// A part with two patches: report A's algorithm (sec 3-4) picks by
/// `version_to` VALUE (longest, then lexicographic), not by list position.
/// Here the newer/longer version_to is listed FIRST and the older/shorter
/// one LAST, so a list-position ("last wins") algorithm would pick wrong.
#[test]
fn terminal_patch_is_chosen_by_version_to_value_not_list_position() {
    let xml = r#"<protocol name="patches_chain" version="1.11">
  <parts_info></parts_info>
  <patches_chain type="install">
    <patch>
      <files><file><name>a</name><size>1</size><unpacked_size>1</unpacked_size></file></files>
      <torrent><hash>deadbeef</hash><urls><url>https://example.test/1</url></urls></torrent>
      <part>client</part>
      <version_to>15.8.0.0.13090112</version_to>
    </patch>
    <patch>
      <files><file><name>b</name><size>1</size><unpacked_size>1</unpacked_size></file></files>
      <torrent><hash>deadbeef</hash><urls><url>https://example.test/2</url></urls></torrent>
      <part>client</part>
      <version_to>9.0.0.0.1</version_to>
    </patch>
  </patches_chain>
  <web_seeds><url threads="1" name="g">http://example.test/</url></web_seeds>
  <version_name></version_name>
  <meta_need_update>False</meta_need_update>
</protocol>"#;

    let game_info = parse_game_info(MINIMAL_GAME_INFO_XML).expect("minimal game_info.xml should parse");
    let patches = parse_patches_chain(xml).expect("synthetic patches_chain.xml should parse");
    let install = patches.patches_chain.iter().find(|c| c.chain_type == ChainType::Install).expect("install chain");

    let parts = diff_parts(&game_info, install);
    let (_, status) = parts.iter().find(|(n, _)| *n == PartName::Client).expect("client part present");
    match status {
        PartStatus::NotInstalled { to } => assert_eq!(to.as_str(), "15.8.0.0.13090112"),
        other => panic!("expected NotInstalled{{to: 15.8.0.0.13090112}}, got {other:?}"),
    }
}

/// Equal-length `version_to` values are broken lexicographically (report A
/// sec 3-4: "longest / last-sorted").
#[test]
fn terminal_patch_breaks_equal_length_ties_lexicographically() {
    let xml = r#"<protocol name="patches_chain" version="1.11">
  <parts_info></parts_info>
  <patches_chain type="install">
    <patch>
      <files><file><name>a</name><size>1</size><unpacked_size>1</unpacked_size></file></files>
      <torrent><hash>deadbeef</hash><urls><url>https://example.test/1</url></urls></torrent>
      <part>client</part>
      <version_to>15.8.0.0.100</version_to>
    </patch>
    <patch>
      <files><file><name>b</name><size>1</size><unpacked_size>1</unpacked_size></file></files>
      <torrent><hash>deadbeef</hash><urls><url>https://example.test/2</url></urls></torrent>
      <part>client</part>
      <version_to>15.8.0.0.199</version_to>
    </patch>
  </patches_chain>
  <web_seeds><url threads="1" name="g">http://example.test/</url></web_seeds>
  <version_name></version_name>
  <meta_need_update>False</meta_need_update>
</protocol>"#;

    let game_info = parse_game_info(MINIMAL_GAME_INFO_XML).expect("minimal game_info.xml should parse");
    let patches = parse_patches_chain(xml).expect("synthetic patches_chain.xml should parse");
    let install = patches.patches_chain.iter().find(|c| c.chain_type == ChainType::Install).expect("install chain");

    let parts = diff_parts(&game_info, install);
    let (_, status) = parts.iter().find(|(n, _)| *n == PartName::Client).expect("client part present");
    match status {
        PartStatus::NotInstalled { to } => assert_eq!(to.as_str(), "15.8.0.0.199"),
        other => panic!("expected NotInstalled{{to: 15.8.0.0.199}}, got {other:?}"),
    }
}
