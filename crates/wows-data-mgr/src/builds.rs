//! Master builds index (`builds.toml`) and per-build metadata.
//!
//! The builds index lives at `{dump_base}/builds.toml` and tracks all dumped
//! game versions. Per-build metadata lives in `{build_dir}/metadata.toml` and
//! includes file hashes for content-addressed storage management.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

// -- Master builds index (builds.toml) --

/// Top-level index of all dumped builds.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BuildsIndex {
    #[serde(default)]
    pub builds: Vec<BuildEntry>,
}

/// A single dumped build entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildEntry {
    pub version: String,
    pub build: u32,
    pub dir: String,
    pub dumped_at: String,
}

impl BuildsIndex {
    /// Load from disk. Returns an empty index if the file doesn't exist.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
    }

    /// Save to disk. Uses write-to-temp-then-rename for atomicity.
    pub fn save(&self, path: &Path) -> Result<(), rootcause::Report> {
        use rootcause::prelude::*;
        let contents = toml::to_string_pretty(self).attach_with(|| "Failed to serialize builds.toml")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .attach_with(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, &contents).attach_with(|| format!("Failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .attach_with(|| format!("Failed to rename {} to {}", tmp.display(), path.display()))?;
        Ok(())
    }

    /// Add or update an entry. If a build with the same number exists, it's replaced.
    pub fn upsert(&mut self, entry: BuildEntry) {
        if let Some(existing) = self.builds.iter_mut().find(|e| e.build == entry.build) {
            *existing = entry;
        } else {
            self.builds.push(entry);
        }
        self.builds.sort_by_key(|e| e.build);
    }

    /// Remove a build entry. Returns the removed entry if found.
    pub fn remove_build(&mut self, build: u32) -> Option<BuildEntry> {
        let idx = self.builds.iter().position(|e| e.build == build)?;
        Some(self.builds.remove(idx))
    }

    /// Find an entry by exact build number.
    pub fn find_by_build(&self, build: u32) -> Option<&BuildEntry> {
        self.builds.iter().find(|e| e.build == build)
    }

    /// Find all entries matching a version prefix.
    /// e.g. "15.2.0" matches all builds with that version, regardless of build number.
    pub fn find_by_version(&self, version_query: &str) -> Vec<&BuildEntry> {
        self.builds.iter().filter(|e| crate::manifest::version_matches(&e.version, version_query)).collect()
    }

    /// Resolve a build number to a dump entry.
    ///
    /// 1. Try exact build match
    /// 2. If no exact match and `target_version` is provided, find builds with
    ///    the same `major.minor.patch` and pick the closest build number
    ///
    /// Returns `(entry, is_exact_match)`.
    pub fn resolve_build(&self, target_build: u32, target_version: Option<&str>) -> Option<(&BuildEntry, bool)> {
        // Exact match
        if let Some(entry) = self.find_by_build(target_build) {
            return Some((entry, true));
        }

        // Version-based fallback
        if let Some(version) = target_version {
            let candidates = self.find_by_version(version);
            if !candidates.is_empty() {
                let closest =
                    candidates.iter().min_by_key(|e| (e.build as i64 - target_build as i64).unsigned_abs()).unwrap();
                return Some((closest, false));
            }
        }

        None
    }
}

// -- Per-build metadata (metadata.toml) --

/// Enhanced per-build metadata with file hashes for CAS management.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BuildMetadata {
    pub version: String,
    pub build: u32,
    /// VFS file path -> CAS hash. Only present in new-format dumps.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

impl BuildMetadata {
    /// Load from disk. Returns None if the file doesn't exist or can't be parsed.
    pub fn load(path: &Path) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;
        toml::from_str(&contents).ok()
    }

    /// Save to disk.
    pub fn save(&self, path: &Path) -> Result<(), rootcause::Report> {
        use rootcause::prelude::*;
        let contents = toml::to_string_pretty(self).attach_with(|| "Failed to serialize metadata.toml")?;
        std::fs::write(path, &contents).attach_with(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    /// Whether this metadata has CAS file hashes (new format).
    pub fn has_file_hashes(&self) -> bool {
        !self.files.is_empty()
    }

    /// Collect all unique hashes referenced by this build.
    pub fn referenced_hashes(&self) -> std::collections::HashSet<String> {
        self.files.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_index_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("builds.toml");

        let mut index = BuildsIndex::default();
        index.upsert(BuildEntry {
            version: "15.1.0".into(),
            build: 11965230,
            dir: "15.1.0_11965230".into(),
            dumped_at: "2025-06-15T10:00:00Z".into(),
        });
        index.upsert(BuildEntry {
            version: "15.2.0".into(),
            build: 12100000,
            dir: "15.2.0_12100000".into(),
            dumped_at: "2025-07-01T14:00:00Z".into(),
        });

        index.save(&path).unwrap();
        let loaded = BuildsIndex::load(&path);
        assert_eq!(loaded.builds.len(), 2);
        assert_eq!(loaded.builds[0].build, 11965230);
    }

    #[test]
    fn resolve_exact_match() {
        let mut index = BuildsIndex::default();
        index.upsert(BuildEntry {
            version: "15.2.0".into(),
            build: 12100000,
            dir: "15.2.0_12100000".into(),
            dumped_at: String::new(),
        });

        let (entry, exact) = index.resolve_build(12100000, None).unwrap();
        assert!(exact);
        assert_eq!(entry.build, 12100000);
    }

    #[test]
    fn resolve_version_fallback() {
        let mut index = BuildsIndex::default();
        index.upsert(BuildEntry {
            version: "15.2.0".into(),
            build: 12100000,
            dir: "15.2.0_12100000".into(),
            dumped_at: String::new(),
        });

        // Different build but same version (e.g. CN server)
        let (entry, exact) = index.resolve_build(12100500, Some("15.2.0")).unwrap();
        assert!(!exact);
        assert_eq!(entry.build, 12100000);
    }

    #[test]
    fn resolve_no_match() {
        let index = BuildsIndex::default();
        assert!(index.resolve_build(99999, Some("99.0.0")).is_none());
    }

    #[test]
    fn metadata_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metadata.toml");

        let mut meta = BuildMetadata { version: "15.2.0".into(), build: 12100000, files: BTreeMap::new() };
        meta.files.insert("gui/test.png".into(), "abcdef1234567890abcd".into());

        meta.save(&path).unwrap();
        let loaded = BuildMetadata::load(&path).unwrap();
        assert_eq!(loaded.files.len(), 1);
        assert!(loaded.has_file_hashes());
    }

    #[test]
    fn old_format_metadata_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metadata.toml");
        std::fs::write(&path, "version = \"15.1.0\"\nbuild = 11965230\n").unwrap();

        let loaded = BuildMetadata::load(&path).unwrap();
        assert_eq!(loaded.version, "15.1.0");
        assert!(!loaded.has_file_hashes());
    }
}
