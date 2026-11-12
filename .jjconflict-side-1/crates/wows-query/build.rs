//! Emits `has_game_data` and `has_build_NNNNN` for each build present in the
//! game data dump named by `WOWS_GAME_DATA`: a wows-replay-data root holding
//! `builds.toml`. Tests over real data run only where such a dump exists.

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize, Default)]
struct BuildsIndex {
    #[serde(default)]
    builds: Vec<BuildEntry>,
}

#[derive(Deserialize)]
struct BuildEntry {
    build: u32,
    dir: String,
}

/// Builds the tests refer to by number.
const KNOWN_TEST_BUILDS: &[u32] = &[
    12506899, // 15.4.0
    13533378, // 15.10.0
];

fn discover_builds(root: &Path) -> Vec<u32> {
    let Ok(text) = std::fs::read_to_string(root.join("builds.toml")) else {
        return Vec::new();
    };
    let Ok(index) = toml::from_str::<BuildsIndex>(&text) else {
        return Vec::new();
    };

    index
        .builds
        .into_iter()
        .filter(|entry| root.join(&entry.dir).join("metadata.toml").is_file())
        .map(|entry| entry.build)
        .collect()
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_game_data)");
    for build in KNOWN_TEST_BUILDS {
        println!("cargo:rustc-check-cfg=cfg(has_build_{build})");
    }
    println!("cargo:rerun-if-env-changed=WOWS_GAME_DATA");
    println!("cargo:rerun-if-env-changed=WOWS_HERMETIC_BUILD");

    // Hermetic Buck actions cannot read a dump outside their inputs.
    if std::env::var_os("WOWS_HERMETIC_BUILD").is_some() {
        return;
    }
    let Some(root) = std::env::var_os("WOWS_GAME_DATA").map(PathBuf::from) else {
        return;
    };
    println!("cargo:rerun-if-changed={}", root.join("builds.toml").display());

    let builds = discover_builds(&root);
    for build in &builds {
        if !KNOWN_TEST_BUILDS.contains(build) {
            println!("cargo:rustc-check-cfg=cfg(has_build_{build})");
        }
        println!("cargo:rustc-cfg=has_build_{build}");
    }
    if !builds.is_empty() {
        println!("cargo:rustc-cfg=has_game_data");
    }
}
