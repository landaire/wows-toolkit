//! Detects available game data builds and emits cfg flags for conditional test compilation.
//!
//! Emits:
//! - `has_game_data` — at least one build is available
//! - `has_build_NNNNN` — specific build number is available
//!
//! Tests can use:
//! ```ignore
//! #[test]
//! #[cfg_attr(not(has_game_data), ignore)]
//! fn test_needs_game_data() { ... }
//! ```

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default)]
struct Registry {
    latest_path: Option<PathBuf>,
    #[serde(default)]
    builds: BTreeMap<String, RegistryEntry>,
}

#[derive(Deserialize)]
struct RegistryEntry {
    #[allow(dead_code)]
    version: String,
}

fn find_workspace_root() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?);
    let mut dir = manifest_dir.as_path();
    loop {
        if dir.join("game_versions.toml").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

fn scan_bin_dir(path: &Path) -> Vec<u32> {
    let bin_dir = path.join("bin");
    let Ok(entries) = std::fs::read_dir(&bin_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()))
        .collect()
}

fn discover_builds(workspace_root: &Path) -> Vec<u32> {
    let data_dir = match std::env::var("WOWS_GAME_DATA") {
        Ok(d) => PathBuf::from(d),
        Err(_) => workspace_root.join("game_data"),
    };

    let registry_path = data_dir.join("versions.toml");
    let registry: Registry = std::fs::read_to_string(&registry_path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();

    let mut builds: Vec<u32> = Vec::new();

    // Builds from registry
    for key in registry.builds.keys() {
        if let Ok(build) = key.parse::<u32>() {
            // For downloaded builds, verify the directory exists
            let build_dir = data_dir.join("builds").join(key);
            if build_dir.exists() {
                builds.push(build);
            }
        }
    }

    // Builds from latest_path
    if let Some(ref latest) = registry.latest_path {
        for build in scan_bin_dir(latest) {
            if !builds.contains(&build) {
                builds.push(build);
            }
        }
    }

    // Also scan game_data/builds/ for any unregistered builds
    let builds_dir = data_dir.join("builds");
    if builds_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&builds_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(build) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) {
                        if !builds.contains(&build) {
                            builds.push(build);
                        }
                    }
                }
            }
        }
    }

    builds.sort();
    builds
}

/// Build numbers referenced by tests that may not be locally available.
/// Declared here so check-cfg doesn't warn about unknown cfgs.
const KNOWN_TEST_BUILDS: &[u32] = &[
    6965290,  // v12.3.1 (S-189 submarine replay)
    9531281,  // v14.1.0 (Hull DD replay)
    11965230, // v15.1.0 (Vermont, Marceau, Narai replays)
];

fn main() {
    // Declare all possible cfgs to satisfy check-cfg
    println!("cargo:rustc-check-cfg=cfg(has_game_data)");

    // Pre-declare check-cfg for all known test builds
    for &build in KNOWN_TEST_BUILDS {
        println!("cargo:rustc-check-cfg=cfg(has_build_{build})");
    }

    let Some(workspace_root) = find_workspace_root() else {
        return;
    };

    let builds = discover_builds(&workspace_root);

    for &build in &builds {
        // Declare check-cfg for any discovered build not in the known list
        if !KNOWN_TEST_BUILDS.contains(&build) {
            println!("cargo:rustc-check-cfg=cfg(has_build_{build})");
        }
        println!("cargo:rustc-cfg=has_build_{build}");
    }

    if !builds.is_empty() {
        println!("cargo:rustc-cfg=has_game_data");
    }

    // Re-run if registry changes
    let data_dir = match std::env::var("WOWS_GAME_DATA") {
        Ok(d) => PathBuf::from(d),
        Err(_) => workspace_root.join("game_data"),
    };
    println!(
        "cargo:rerun-if-changed={}",
        data_dir.join("versions.toml").display()
    );
    println!("cargo:rerun-if-env-changed=WOWS_GAME_DATA");
}
