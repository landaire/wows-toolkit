use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rootcause::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LocalRegistry {
    /// Path to a WoWs installation that always provides the latest builds.
    /// Checked dynamically — whatever builds exist there are available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_path: Option<PathBuf>,
    #[serde(default)]
    pub builds: BTreeMap<String, LocalBuildEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalBuildEntry {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloaded_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registered_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl LocalRegistry {
    pub fn has_build(&self, build: u32) -> bool {
        self.builds.contains_key(&build.to_string())
    }

    pub fn get(&self, build: u32) -> Option<&LocalBuildEntry> {
        self.builds.get(&build.to_string())
    }

    pub fn set_downloaded(&mut self, build: u32, version: &str) {
        let now = jiff::Zoned::now();
        let timestamp = now.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string();
        self.builds.insert(
            build.to_string(),
            LocalBuildEntry {
                version: version.to_string(),
                downloaded_at: Some(timestamp),
                registered_at: None,
                path: None,
            },
        );
    }

    pub fn set_registered(&mut self, build: u32, version: &str, path: &Path) {
        let now = jiff::Zoned::now();
        let timestamp = now.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string();
        self.builds.insert(
            build.to_string(),
            LocalBuildEntry {
                version: version.to_string(),
                downloaded_at: None,
                registered_at: Some(timestamp),
                path: Some(path.to_path_buf()),
            },
        );
    }

    /// Returns the sorted list of available build numbers.
    /// Merges explicitly registered/downloaded builds with any builds
    /// found at `latest_path`.
    pub fn available_builds(&self) -> Vec<u32> {
        let mut builds: Vec<u32> = self
            .builds
            .keys()
            .filter_map(|k| k.parse::<u32>().ok())
            .collect();

        if let Some(ref latest) = self.latest_path {
            if let Ok(latest_builds) = wowsunpack::game_data::list_available_builds(latest) {
                for b in latest_builds {
                    if !builds.contains(&b) {
                        builds.push(b);
                    }
                }
            }
        }

        builds.sort();
        builds
    }

    /// Returns the game directory for a build.
    /// Checks in order: explicit registry entry, latest_path, downloaded builds.
    pub fn game_dir_for_build(&self, build: u32, data_dir: &Path) -> Option<PathBuf> {
        // Check explicit registry entry first
        if let Some(entry) = self.get(build) {
            if let Some(ref path) = entry.path {
                return Some(path.clone());
            }
            // Downloaded build
            let dir = data_dir.join("builds").join(build.to_string());
            if dir.exists() {
                return Some(dir);
            }
        }

        // Check latest_path
        if let Some(ref latest) = self.latest_path {
            if let Ok(builds) = wowsunpack::game_data::list_available_builds(latest) {
                if builds.contains(&build) {
                    return Some(latest.clone());
                }
            }
        }

        // Fallback: check if downloaded dir exists even without registry entry
        let dir = data_dir.join("builds").join(build.to_string());
        if dir.exists() {
            Some(dir)
        } else {
            None
        }
    }
}

pub fn load_registry(path: &Path) -> LocalRegistry {
    if !path.exists() {
        return LocalRegistry::default();
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return LocalRegistry::default(),
    };
    toml::from_str(&content).unwrap_or_default()
}

pub fn save_registry(registry: &LocalRegistry, path: &Path) -> Result<(), Report> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .attach_with(|| format!("Failed to create directory {}", parent.display()))?;
    }
    let content = toml::to_string_pretty(registry)
        .map_err(|e| rootcause::report!("Failed to serialize registry: {e}"))?;
    std::fs::write(path, content)
        .attach_with(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}
