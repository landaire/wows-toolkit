//! Proton resolution for Linux launches.

use std::path::Path;
use std::path::PathBuf;

/// Per-WGC-install Proton settings the user configures once.
#[derive(Debug, Clone)]
pub struct ProtonConfig {
    pub proton_binary: PathBuf,
    pub compat_prefix: PathBuf,
}

/// Locate the Proton binary used to run `compat_data`'s prefix.
///
/// Reads `config_info` from the compatdata prefix (first line is the Proton
/// install path), falling back to scanning `steamapps/common` for any
/// installed Proton version, preferring the newest by parsed version number.
pub fn find_proton_in_library(library_root: &Path, compat_data: &Path) -> Option<PathBuf> {
    if let Ok(contents) = std::fs::read_to_string(compat_data.join("config_info"))
        && let Some(first_line) = contents.lines().next()
    {
        let raw = Path::new(first_line.trim_end_matches('/'));
        let base = std::fs::canonicalize(raw).unwrap_or_else(|_| raw.to_path_buf());
        for candidate in [base.join("proton"), base.parent().map(|p| p.join("proton")).unwrap_or_default()] {
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    let common = library_root.join("steamapps/common");
    let mut proton_dirs: Vec<PathBuf> = std::fs::read_dir(&common)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("Proton"))
        .map(|e| e.path())
        .collect();
    // Sort newest-first by numeric version so "Proton 10.0" beats "Proton 9.0";
    // dirs whose name doesn't carry a parseable version sort last.
    proton_dirs.sort_by(|a, b| {
        let va = a.file_name().and_then(|n| n.to_str()).and_then(parse_proton_version);
        let vb = b.file_name().and_then(|n| n.to_str()).and_then(parse_proton_version);
        match (va, vb) {
            (Some(va), Some(vb)) => vb.cmp(&va),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    proton_dirs.into_iter().map(|d| d.join("proton")).find(|p| p.exists())
}

/// Parse the leading `major.minor` version out of a Proton directory name,
/// e.g. "Proton 10.0" -> (10, 0), "Proton 9.0-GE" -> (9, 0).
fn parse_proton_version(dir_name: &str) -> Option<(u32, u32)> {
    let rest = dir_name.strip_prefix("Proton")?.trim_start();
    let version_str: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    if version_str.is_empty() {
        return None;
    }
    let mut parts = version_str.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u32>().unwrap_or(0);
    Some((major, minor))
}

/// Locate the Steam client root directory, distinct from any secondary
/// Steam library that may hold the game's compatdata prefix. Checks
/// `$XDG_DATA_HOME/Steam`, `$HOME/.local/share/Steam`, and
/// `$HOME/.steam/steam`, returning the first with a `steamapps` dir.
pub fn find_steam_root() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let mut candidates = Vec::new();
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        candidates.push(PathBuf::from(&xdg).join("Steam"));
    }
    candidates.push(PathBuf::from(&home).join(".local/share/Steam"));
    candidates.push(PathBuf::from(&home).join(".steam/steam"));
    candidates.into_iter().find(|p| p.join("steamapps").is_dir())
}
