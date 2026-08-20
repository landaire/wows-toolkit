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
/// installed Proton version, preferring the newest by directory name.
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
    proton_dirs.sort_by(|a, b| b.cmp(a));
    proton_dirs.into_iter().map(|d| d.join("proton")).find(|p| p.exists())
}
