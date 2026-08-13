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

fn discover_builds(source: &GameDataSource) -> Vec<u32> {
    let registry: Registry =
        std::fs::read_to_string(&source.registry_path).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();

    let mut builds: Vec<u32> = Vec::new();

    if source.scan_data_directories {
        for key in registry.builds.keys() {
            if let Ok(build) = key.parse::<u32>() {
                let build_dir = source.data_dir.join("builds").join(key);
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

        let builds_dir = source.data_dir.join("builds");
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

        // The vendor hybrid-graphics shims look these up in the export table by
        // name. Defining the statics is not enough; an executable exports
        // nothing unless the linker is told to.
        //
        // Scoped to the GUI binary, which is where the statics are defined and
        // the only target that opens a surface. `-bins` would put the directive
        // on every binary in the crate, and the headless helpers in src/bin
        // define no such symbol, so the linker would fail them on an export it
        // cannot resolve.
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            println!("cargo:rustc-link-arg-bin=wows_toolkit=/EXPORT:NvOptimusEnablement,DATA");
            println!("cargo:rustc-link-arg-bin=wows_toolkit=/EXPORT:AmdPowerXpressRequestHighPerformance,DATA");
        }
    }

    println!("cargo:rustc-check-cfg=cfg(has_game_data)");
    for &build in KNOWN_TEST_BUILDS {
        println!("cargo:rustc-check-cfg=cfg(has_build_{build})");
    }

    let Some(source) = game_data_source() else {
        return;
    };

    let builds = discover_builds(&source);

    for &build in &builds {
        if !KNOWN_TEST_BUILDS.contains(&build) {
            println!("cargo:rustc-check-cfg=cfg(has_build_{build})");
        }
        println!("cargo:rustc-cfg=has_build_{build}");
    }

    if !builds.is_empty() {
        println!("cargo:rustc-cfg=has_game_data");
    }

    println!("cargo:rerun-if-changed={}", source.registry_path.display());
    println!("cargo:rerun-if-env-changed=WOWS_GAME_DATA");
    println!("cargo:rerun-if-env-changed=WOWS_HERMETIC_BUILD");
}
