use std::io::Read;
use std::path::Path;

use rootcause::prelude::*;
use wowsunpack::data::Version;
use wowsunpack::game_data;

/// Detect the game version for a specific build within the data directory.
/// Expects the build to live at `data_dir/builds/<build>/` with standard layout.
pub fn detect_version_for_build(data_dir: &Path, build: u32) -> Result<String, Report> {
    let build_dir = data_dir.join("builds").join(build.to_string());
    detect_version_at_path(&build_dir, build)
}

/// Detect the game version for a specific build at an arbitrary path.
/// The path should be a valid WoWs game root (containing `bin/<build>/idx/` and `res_packages/`).
pub fn detect_version_at_path(game_dir: &Path, build: u32) -> Result<String, Report> {
    let version = Version {
        major: 0,
        minor: 0,
        patch: 0,
        build,
    };

    let resources = game_data::load_game_resources(game_dir, &version)
        .attach_with(|| format!("Failed to load game resources for build {build} at {}", game_dir.display()))?;

    let account_def_path = "scripts/entity_defs/Account.def";
    let file = resources
        .vfs
        .join(account_def_path)
        .and_then(|p| p.open_file())
        .map_err(|e| rootcause::report!("Failed to open {account_def_path} in VFS for build {build}: {e}"))?;

    let mut content = String::new();
    std::io::BufReader::new(file).read_to_string(&mut content)
        .attach_with(|| "Failed to read Account.def")?;

    let parsed = Version::from_account_def(&content)
        .ok_or_else(|| rootcause::report!("Failed to parse version from Account.def"))?;

    Ok(format!("{}.{}.{}", parsed.major, parsed.minor, parsed.patch))
}

/// Scan a directory for game builds and detect their versions.
/// Returns a list of (build_number, version_string) pairs.
pub fn detect_all_versions(scan_path: &Path) -> Result<Vec<(u32, String)>, Report> {
    if !scan_path.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();

    // Each subdirectory in scan_path could be a build directory
    let entries = std::fs::read_dir(scan_path)
        .attach_with(|| format!("Failed to read {}", scan_path.display()))?;

    for entry in entries {
        let entry: std::fs::DirEntry = entry?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        let dir_name = entry.file_name();
        let Some(name_str) = dir_name.to_str() else {
            continue;
        };
        let Ok(build) = name_str.parse::<u32>() else {
            continue;
        };

        // This directory is a build root — try to detect its version
        let build_path = entry.path();
        match detect_version_at_path(&build_path, build) {
            Ok(version) => results.push((build, version)),
            Err(e) => {
                // Try scanning for builds within this directory
                // (it might be a standard WoWs install with bin/<build>/)
                match game_data::list_available_builds(&build_path) {
                    Ok(sub_builds) => {
                        for sub_build in sub_builds {
                            match detect_version_at_path(&build_path, sub_build) {
                                Ok(version) => results.push((sub_build, version)),
                                Err(e) => {
                                    eprintln!("Warning: could not detect version for build {sub_build} at {}: {e}", build_path.display());
                                }
                            }
                        }
                    }
                    Err(_) => {
                        eprintln!("Warning: could not detect version for build {build}: {e}");
                    }
                }
            }
        }
    }

    results.sort_by_key(|(build, _)| *build);
    Ok(results)
}
