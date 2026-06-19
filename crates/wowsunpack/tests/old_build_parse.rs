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

/// Cross-version regression for the TTX / normalized-view parsing added in the
/// ship-stats milestones. The component walk, projectile reads, and ability
/// `effect_fields` merge run for *every* param during `from_vfs`, so an old
/// format that violated a structural assumption would surface here as either a
/// failed load or a `ship_stats_stock` panic.
///
/// For each build under `WOWS_BUILDS_ROOT` (default `G:\wows_builds`) whose name
/// appears in `BUILDS`, this:
///   1. loads `<build>\vfs` via `from_vfs` and asserts it succeeds with params,
///   2. picks one ship of each player class (BB / CA / DD / CV) that the era has,
///   3. runs `ship_stats_stock` inside `catch_unwind` and asserts no panic,
///   4. prints a per-build matrix of which TTX sections populated vs came back
///      `None` (gracefully-empty is acceptable on old eras; a panic is not).
///
/// Gated behind `#[ignore]`; needs the dumped `vfs/` trees. Run:
///   cargo test -p wowsunpack --test old_build_parse ttx_across_historical_builds -- --ignored --nocapture
///
/// Override the build root with `WOWS_BUILDS_ROOT` and the current install's
/// `vfs/` with `CURRENT_BUILD_VFS` (default `E:\WoWs\World_of_Warships\bin\<newest>`
/// is *not* an extracted tree, so the current build is covered by `ttx_real_provider`
/// instead; this test focuses on the historical spread).
#[test]
#[ignore]
fn ttx_across_historical_builds() {
    use std::panic::AssertUnwindSafe;
    use std::panic::catch_unwind;

    use wowsunpack::game_params::ttx::ship_stats_stock;
    use wowsunpack::game_params::types::Species;

    // A spread across eras: pre-rework (0.6/0.7), the 10.x renumbering, and the
    // 11.x subs era. Each must have a dumped `<name>\vfs` tree under the root.
    const BUILDS: &[&str] =
        &["0.6.13_296659", "0.7.6_346043", "10.0.0_3343484", "10.6.0_4181350", "0.11.8_6223574", "0.11.11_6623042"];

    let root = std::env::var("WOWS_BUILDS_ROOT").unwrap_or_else(|_| r"G:\wows_builds".to_string());

    // (class label, the Species the orchestration buckets on)
    let classes: [(&str, Species); 4] = [
        ("BB", Species::Battleship),
        ("CA", Species::Cruiser),
        ("DD", Species::Destroyer),
        ("CV", Species::AirCarrier),
    ];

    let mut loaded_any = false;
    let mut failures: Vec<String> = Vec::new();

    for build in BUILDS {
        let dir = format!("{root}\\{build}\\vfs");
        if !std::path::Path::new(&dir).join("content").join("GameParams.data").is_file() {
            eprintln!("SKIP {build}: no {dir}\\content\\GameParams.data");
            continue;
        }
        loaded_any = true;

        let vfs = VfsPath::new(PhysicalFS::new(dir.as_str()));
        let provider = match GameMetadataProvider::from_vfs(&vfs) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("{build}: from_vfs Err: {e:?}"));
                continue;
            }
        };
        let n = provider.params().len();
        if n == 0 {
            failures.push(format!("{build}: from_vfs returned 0 params"));
            continue;
        }
        eprintln!("\n=== {build}: {n} params ===");

        // Bucket the first vehicle of each player class. `species()` is the same
        // accessor `ship_stats_stock` keys on, so this mirrors real dispatch.
        for (label, species) in classes {
            let sample = provider.params().iter().find(|p| {
                p.vehicle().is_some()
                    && p.species().and_then(|s| s.known().copied()) == Some(species)
                    // tier-bearing combat ship (skip the odd auxiliary/event vehicle)
                    && p.vehicle().map(|v| v.level() >= 1 && v.level() <= 11).unwrap_or(false)
            });
            let Some(ship) = sample else {
                eprintln!("  {label}: (no ship of this class in this era)");
                continue;
            };
            let name = ship.name().to_string();

            let result = catch_unwind(AssertUnwindSafe(|| ship_stats_stock(ship, &provider)));
            let stats = match result {
                Ok(s) => s,
                Err(_) => {
                    failures.push(format!("{build} {label} {name}: ship_stats_stock PANICKED"));
                    eprintln!("  {label} {name}: PANIC");
                    continue;
                }
            };

            // Section-population matrix: which cards the era's data supported.
            let mark = |b: bool| if b { "yes" } else { "None" };
            let has_ttx = ship.vehicle().and_then(|v| v.ttx_components()).is_some();
            eprintln!(
                "  {label:<2} {name:<28} ttx={:<4} dur={} mob={} arty={} torp={} sec={} vis={} bat={}",
                mark(has_ttx),
                mark(stats.durability.is_some()),
                mark(stats.mobility.is_some()),
                mark(stats.artillery.is_some()),
                mark(stats.torpedoes.is_some()),
                mark(stats.secondaries.is_some()),
                mark(stats.visibility.is_some()),
                mark(stats.battery.is_some()),
            );
        }
    }

    assert!(loaded_any, "no historical build vfs trees found under {root}; set WOWS_BUILDS_ROOT");
    assert!(failures.is_empty(), "cross-version failures:\n{}", failures.join("\n"));
}
