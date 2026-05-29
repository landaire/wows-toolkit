//! Regression test: GameParams from old builds must parse without panicking.
//!
//! Points at a dumped build's extracted `vfs/` tree (override with
//! `OLD_BUILD_VFS`). Run explicitly:
//!   cargo test -p wowsunpack --test old_build_parse -- --ignored --nocapture

use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::vfs::VfsPath;
use wowsunpack::vfs::impls::physical::PhysicalFS;

/// Decode an old build's GameParams.data to JSON for inspection (jaq/jq).
///   cargo test -p wowsunpack --test old_build_parse dump_gameparams_json -- --ignored --nocapture
#[test]
#[ignore]
fn dump_gameparams_json() {
    use wowsunpack::game_params::convert::game_params_to_pickle;
    let dir = std::env::var("OLD_BUILD_VFS").unwrap_or_else(|_| r"G:\wows_builds\0.6.13_296659\vfs".to_string());
    let out = std::env::var("GP_JSON_OUT").unwrap_or_else(|_| r"G:\temp_gp.json".to_string());
    let data = std::fs::read(format!("{dir}/content/GameParams.data")).expect("read GameParams.data");
    let pickle = game_params_to_pickle(data).expect("decode GameParams");
    let file = std::fs::File::create(&out).expect("create json");
    serde_json::to_writer(std::io::BufWriter::new(file), &pickle).expect("write json");
    eprintln!("wrote {out}");
}

#[test]
#[ignore]
fn parse_old_build_vfs() {
    let dir = std::env::var("OLD_BUILD_VFS").unwrap_or_else(|_| r"G:\wows_builds\0.6.13_296659\vfs".to_string());
    let vfs = VfsPath::new(PhysicalFS::new(dir.as_str()));
    let provider = GameMetadataProvider::from_vfs(&vfs).expect("from_vfs returned Err");
    eprintln!("parsed {} params from {dir}", provider.params().len());
    assert!(!provider.params().is_empty(), "no params parsed");
}
