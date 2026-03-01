use std::collections::BTreeMap;
use std::path::Path;

use rootcause::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct GameVersionManifest {
    #[serde(default)]
    pub versions: BTreeMap<String, GameVersionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameVersionEntry {
    pub version: String,
    pub depot_id: u32,
    pub manifest_id: String,
}

impl GameVersionManifest {
    /// Returns the highest build number in the manifest.
    pub fn latest_build(&self) -> Option<u32> {
        self.versions
            .keys()
            .filter_map(|k| k.parse::<u32>().ok())
            .max()
    }

    /// Look up a build number by version string (supports shorthand like "15.1").
    /// When multiple builds match, returns the highest.
    pub fn find_by_version(&self, query: &str) -> Option<u32> {
        let mut matched: Vec<u32> = self
            .versions
            .iter()
            .filter(|(_, entry)| version_matches(&entry.version, query))
            .filter_map(|(k, _)| k.parse::<u32>().ok())
            .collect();
        matched.sort();
        matched.last().copied()
    }

    /// Get a manifest entry by build number.
    pub fn get(&self, build: u32) -> Option<&GameVersionEntry> {
        self.versions.get(&build.to_string())
    }
}

/// Check if a full version string matches a possibly-shorthand query.
/// "15.1.0" matches "15.1", "15.1.0", and "15".
pub fn version_matches(full: &str, query: &str) -> bool {
    let full_parts: Vec<&str> = full.split('.').collect();
    let query_parts: Vec<&str> = query.split('.').collect();

    if query_parts.len() > full_parts.len() {
        return false;
    }

    full_parts
        .iter()
        .zip(query_parts.iter())
        .all(|(f, q)| f == q)
}

pub fn load_manifest(path: &Path) -> Result<GameVersionManifest, Report> {
    if !path.exists() {
        return Ok(GameVersionManifest {
            versions: BTreeMap::new(),
        });
    }
    let content = std::fs::read_to_string(path)
        .attach_with(|| format!("Failed to read {}", path.display()))?;
    let manifest: GameVersionManifest = toml::from_str(&content)
        .map_err(|e| rootcause::report!("Failed to parse {}: {e}", path.display()))?;
    Ok(manifest)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn version_matches_exact() {
        assert!(version_matches("15.1.0", "15.1.0"));
    }

    #[test]
    fn version_matches_shorthand_two() {
        assert!(version_matches("15.1.0", "15.1"));
    }

    #[test]
    fn version_matches_shorthand_one() {
        assert!(version_matches("15.1.0", "15"));
    }

    #[test]
    fn version_no_match() {
        assert!(!version_matches("15.1.0", "14.1"));
    }

    #[test]
    fn version_query_longer() {
        assert!(!version_matches("15.1", "15.1.0"));
    }

    #[test]
    fn find_by_version_picks_highest() {
        let mut versions = BTreeMap::new();
        versions.insert(
            "11791718".to_string(),
            GameVersionEntry {
                version: "15.0.0".to_string(),
                depot_id: 552991,
                manifest_id: "aaa".to_string(),
            },
        );
        versions.insert(
            "11965230".to_string(),
            GameVersionEntry {
                version: "15.1.0".to_string(),
                depot_id: 552991,
                manifest_id: "bbb".to_string(),
            },
        );
        let manifest = GameVersionManifest { versions };

        assert_eq!(manifest.find_by_version("15"), Some(11965230));
        assert_eq!(manifest.find_by_version("15.1"), Some(11965230));
        assert_eq!(manifest.find_by_version("15.0"), Some(11791718));
        assert_eq!(manifest.find_by_version("14"), None);
    }
}
