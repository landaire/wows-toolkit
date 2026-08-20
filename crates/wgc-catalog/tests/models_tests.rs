use wgc_catalog::models::PartName;
use wgc_catalog::models::parse_metadata;
use wgc_catalog::models::parse_patches_chain;

const METADATA_XML: &str = include_str!("fixtures/metadata.xml");
const PATCHES_CHAIN_XML: &str = include_str!("fixtures/patches_chain.xml");

#[test]
fn parses_real_metadata_fixture() {
    let metadata = parse_metadata(METADATA_XML).expect("metadata.xml should parse");

    assert_eq!(metadata.app_id(), "WOWS.PT.PRODUCTION");
    assert_eq!(metadata.chain_id(), "f21");
    assert!(!metadata.metadata_version().is_empty());
    assert_eq!(metadata.safe_mode_args(), vec!["--safe"]);
    assert_eq!(metadata.predefined_section.client_types.default, "high");

    let launch_options = metadata.launch_options();
    assert_eq!(launch_options.get("RESET_ALL"), Some(&"--ra"));
    assert_eq!(launch_options.get("WGCHECK"), Some(&"--wgcheck"));
}

#[test]
fn parses_real_patches_chain_fixture() {
    let chain = parse_patches_chain(PATCHES_CHAIN_XML).expect("patches_chain.xml should parse");

    let install = chain.patches_chain.iter().find(|c| c.chain_type == "install").expect("install chain present");
    assert_eq!(install.patch.len(), 4);

    let parts: Vec<&PartName> = install.patch.iter().map(|p| &p.part).collect();
    assert!(parts.contains(&&PartName::Client));
    assert!(parts.contains(&&PartName::Locale));
    assert!(parts.contains(&&PartName::Sdcontent));
    assert!(parts.contains(&&PartName::Hotfix));

    for patch in &install.patch {
        assert!(!patch.torrent.hash.as_str().is_empty());
        assert!(!patch.torrent.urls.url.is_empty());
        assert!(!patch.version_to.as_str().is_empty());
    }

    assert!(!chain.web_seeds.url.is_empty());
    let base = url::Url::parse(&chain.web_seeds.url[0].base).expect("web seed base url should parse");
    assert_eq!(base.host_str(), Some("dl-wows-gc.wargaming.net"));
}
