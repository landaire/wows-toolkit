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
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

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
    path: Option<PathBuf>,
}

struct GameDataSource {
    registry_path: PathBuf,
    data_dir: PathBuf,
    scan_data_directories: bool,
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

fn game_data_source() -> Option<GameDataSource> {
    if let Some(path) = std::env::var_os("WOWS_GAME_DATA") {
        let path = PathBuf::from(path);
        let registry_path = if path.file_name().is_some_and(|name| name == "versions.toml") {
            path.clone()
        } else {
            path.join("versions.toml")
        };
        let data_dir = registry_path.parent()?.to_path_buf();
        return Some(GameDataSource {
            registry_path,
            data_dir,
            scan_data_directories: std::env::var_os("WOWS_HERMETIC_BUILD").is_none(),
        });
    }

    let data_dir = find_workspace_root()?.join("game_data");
    Some(GameDataSource { registry_path: data_dir.join("versions.toml"), data_dir, scan_data_directories: true })
}

#[derive(Deserialize)]
struct BuildMetadata {
    #[allow(dead_code)]
    version: String,
    #[allow(dead_code)]
    build: u32,
    #[serde(default)]
    files: BTreeMap<String, String>,
}

struct Discovery {
    builds: Vec<u32>,
    watched_paths: BTreeSet<PathBuf>,
}

fn scan_bin_dir(path: &Path) -> Vec<u32> {
    let bin_dir = path.join("bin");
    let Ok(entries) = std::fs::read_dir(&bin_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .filter_map(|entry| entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()))
        .collect()
}

fn resolved_build_dir(registry: &Registry, data_dir: &Path, build: u32) -> Option<PathBuf> {
    let downloaded = data_dir.join("builds").join(build.to_string());
    if let Some(entry) = registry.builds.get(&build.to_string()) {
        if let Some(path) = &entry.path
            && path.exists()
        {
            return Some(path.clone());
        }
        if downloaded.exists() {
            return Some(downloaded);
        }
    }
    if let Some(latest) = &registry.latest_path
        && scan_bin_dir(latest).contains(&build)
    {
        return Some(latest.clone());
    }
    downloaded.exists().then_some(downloaded)
}

fn cas_object_path(dump_dir: &Path, hash: &str) -> Option<PathBuf> {
    let Some((prefix, suffix)) = hash.get(..2).zip(hash.get(2..)) else {
        return None;
    };
    dump_dir.parent().map(|base| base.join("common").join(prefix).join(suffix))
}

fn has_vfs_file(dump_dir: &Path, metadata: &BuildMetadata, path: &str) -> bool {
    metadata.files.get(path).and_then(|hash| cas_object_path(dump_dir, hash)).is_some_and(|path| path.exists())
        || dump_dir.join("vfs").join(path).exists()
}

fn is_usable_build(dump_dir: &Path, build: u32) -> bool {
    std::fs::read_to_string(dump_dir.join("metadata.toml"))
        .ok()
        .and_then(|contents| toml::from_str::<BuildMetadata>(&contents).ok())
        .is_some_and(|metadata| {
            metadata.build == build
                && ["content/GameParams.data", "scripts/entity_defs/alias.xml", "scripts/entities.xml"]
                    .into_iter()
                    .all(|path| has_vfs_file(dump_dir, &metadata, path))
        })
}

fn discover_builds(source: &GameDataSource) -> Discovery {
    let registry: Registry =
        std::fs::read_to_string(&source.registry_path).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();

    if !source.scan_data_directories {
        return Discovery { builds: Vec::new(), watched_paths: BTreeSet::new() };
    }

    let mut candidates: BTreeSet<u32> = registry.builds.keys().filter_map(|key| key.parse().ok()).collect();

    if let Some(latest) = &registry.latest_path {
        candidates.extend(scan_bin_dir(latest));
    }

    // Also scan game_data/builds/ for unregistered builds.
    let builds_dir = source.data_dir.join("builds");
    if builds_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&builds_dir)
    {
        for entry in entries.filter_map(|e| e.ok()) {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && let Some(build) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok())
            {
                candidates.insert(build);
            }
        }
    }

    let mut builds = Vec::new();
    let mut watched_paths = BTreeSet::from([builds_dir]);
    if let Some(latest) = &registry.latest_path {
        watched_paths.insert(latest.join("bin"));
    }
    for build in candidates {
        if let Some(dir) = resolved_build_dir(&registry, &source.data_dir, build) {
            watched_paths.insert(dir.join("metadata.toml"));
            watched_paths.insert(dir.join("vfs"));
            if let Some(base) = dir.parent() {
                watched_paths.insert(base.join("common"));
            }
            if is_usable_build(&dir, build) {
                builds.push(build);
            }
        }
    }
    Discovery { builds, watched_paths }
}

/// Build numbers referenced by tests that may not be locally available.
/// Declared here so check-cfg doesn't warn about unknown cfgs.
const KNOWN_TEST_BUILDS: &[u32] = &[
    1427460,  // v0.8.2 (Montana replay)
    1631917,  // v0.8.5 (Bayern, New Orleans replays)
    2171354,  // v0.9.0 (Atlanta, Shimakaze replays)
    3343484,  // v0.10.0 (Jean Bart replay)
    4046169,  // v0.10.5 (Shimakaze clan replay)
    5045210,  // v0.11.0 (Conte di Cavour, Grossdeutschland replays)
    6359964,  // v0.11.9 (Cossack replay)
    6965290,  // v12.3 (S-189 submarine replay)
    7266701,  // v12.6 (Yellow Dragon operation replay)
    8151735,  // v13.2 (Annapolis replay)
    8260685,  // v13.3 (V-170 replay)
    9129736,  // v13.10 (Colbert replay)
    9531281,  // v14.1 (Hull DD replay)
    9643943,  // v14.2 (Oland replay)
    10695045, // v14.9 (Ocean CV event replay)
    11791718, // v15.0 (Forrest Sherman replay)
    11965230, // v15.1 (Vermont, Marceau, Narai replays)
];

fn main() {
    // Declare all possible cfgs to satisfy check-cfg
    println!("cargo:rustc-check-cfg=cfg(has_game_data)");

    // Pre-declare check-cfg for all known test builds
    for &build in KNOWN_TEST_BUILDS {
        println!("cargo:rustc-check-cfg=cfg(has_build_{build})");
    }

    let Some(source) = game_data_source() else {
        return;
    };

    let discovery = discover_builds(&source);

    for &build in &discovery.builds {
        // Declare check-cfg for any discovered build not in the known list
        if !KNOWN_TEST_BUILDS.contains(&build) {
            println!("cargo:rustc-check-cfg=cfg(has_build_{build})");
        }
        println!("cargo:rustc-cfg=has_build_{build}");
    }

    if !discovery.builds.is_empty() {
        println!("cargo:rustc-cfg=has_game_data");
    }

    // Re-run if registry changes
    println!("cargo:rerun-if-changed={}", source.registry_path.display());
    for watched_path in discovery.watched_paths {
        println!("cargo:rerun-if-changed={}", watched_path.display());
    }
    println!("cargo:rerun-if-env-changed=WOWS_GAME_DATA");
    println!("cargo:rerun-if-env-changed=WOWS_HERMETIC_BUILD");
}
