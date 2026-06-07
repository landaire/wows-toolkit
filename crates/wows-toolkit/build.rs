use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize, Default)]
struct Registry {
    latest_path: Option<PathBuf>,
    #[serde(default)]
    builds: BTreeMap<String, toml::Value>,
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
    let registry: Registry =
        std::fs::read_to_string(&registry_path).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();

    let mut builds: Vec<u32> = Vec::new();

    for key in registry.builds.keys() {
        if let Ok(build) = key.parse::<u32>() {
            let build_dir = data_dir.join("builds").join(key);
            if build_dir.exists() {
                builds.push(build);
            }
        }
    }

    if let Some(ref latest) = registry.latest_path {
        for build in scan_bin_dir(latest) {
            if !builds.contains(&build) {
                builds.push(build);
            }
        }
    }

    let builds_dir = data_dir.join("builds");
    if builds_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&builds_dir)
    {
        for entry in entries.filter_map(|e| e.ok()) {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && let Some(build) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok())
                && !builds.contains(&build)
            {
                builds.push(build);
            }
        }
    }

    builds.sort();
    builds
}

const KNOWN_TEST_BUILDS: &[u32] = &[
    6965290,  // v12.3
    8151735,  // v13.2
    9129736,  // v13.10
    9531281,  // v14.1
    9643943,  // v14.2
    10695045, // v14.9
    11791718, // v15.0
    11965230, // v15.1 (Vermont)
];

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/wows_toolkit.ico");
        res.compile().unwrap();
    }

    println!("cargo:rustc-check-cfg=cfg(has_game_data)");
    for &build in KNOWN_TEST_BUILDS {
        println!("cargo:rustc-check-cfg=cfg(has_build_{build})");
    }

    let Some(workspace_root) = find_workspace_root() else {
        return;
    };

    let builds = discover_builds(&workspace_root);

    for &build in &builds {
        if !KNOWN_TEST_BUILDS.contains(&build) {
            println!("cargo:rustc-check-cfg=cfg(has_build_{build})");
        }
        println!("cargo:rustc-cfg=has_build_{build}");
    }

    if !builds.is_empty() {
        println!("cargo:rustc-cfg=has_game_data");
    }

    let data_dir = match std::env::var("WOWS_GAME_DATA") {
        Ok(d) => PathBuf::from(d),
        Err(_) => workspace_root.join("game_data"),
    };
    println!("cargo:rerun-if-changed={}", data_dir.join("versions.toml").display());
    println!("cargo:rerun-if-env-changed=WOWS_GAME_DATA");
}
