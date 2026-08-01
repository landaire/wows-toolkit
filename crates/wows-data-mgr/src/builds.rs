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
    /// Build-relative path -> CAS hash for derived artifacts (the rkyv game
    /// params blob and the compressed copies fetched by web clients). Kept
    /// separate from `files`, which tracks the extracted `vfs/` tree.
    #[serde(default)]
    pub derived: BTreeMap<String, String>,
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

    /// Collect all unique CAS hashes referenced by this build, across both the
    /// extracted `vfs/` tree and the derived artifacts.
    pub fn referenced_hashes(&self) -> std::collections::HashSet<String> {
        self.files.values().chain(self.derived.values()).cloned().collect()
    }
}

/// Maximum number of file paths named in a [`CorruptObject`] message. One
/// object can back hundreds of files; the full list belongs in the log, not in
/// a string that ends up in a toast.
const NAMED_FILES: usize = 3;

/// A content object whose bytes do not hash to the name it is stored under,
/// attributed to the build that references it and the files it backs.
///
/// A hash is 20 hex characters and says nothing on its own. Carrying the build,
/// the version and the referencing paths is what makes the failure reportable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorruptObject {
    pub build: u32,
    pub version: String,
    /// The name the object is stored under, which is the hash its bytes must
    /// reproduce.
    pub hash: String,
    /// What the bytes actually hashed to.
    pub actual: String,
    /// Paths in the build that read through this object. Goes to the log in
    /// full; [`Display`](std::fmt::Display) names at most [`NAMED_FILES`].
    pub files: Vec<String>,
    /// How many paths reference the object, held separately so a caller that
    /// trims `files` still reports the true count.
    pub total_files: usize,
}

impl CorruptObject {
    /// Attribute a hash mismatch to a build by reverse-mapping the hash through
    /// the build's metadata, which names every path the object backs.
    pub fn attribute(entry: &BuildEntry, metadata: &BuildMetadata, hash: &str, actual: &str) -> Self {
        let files: Vec<String> = metadata
            .files
            .iter()
            .chain(metadata.derived.iter())
            .filter(|(_, referenced)| referenced.as_str() == hash)
            .map(|(path, _)| path.clone())
            .collect();
        Self {
            build: entry.build,
            version: entry.version.clone(),
            hash: hash.to_string(),
            actual: actual.to_string(),
            total_files: files.len(),
            files,
        }
    }

    /// The full list of referencing paths, for the log.
    pub fn all_files(&self) -> String {
        self.files.join(", ")
    }
}

impl std::fmt::Display for CorruptObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "build {} ({}) has a corrupt content object: {} hashed to {}. ",
            self.build, self.version, self.hash, self.actual
        )?;
        if self.total_files == 0 {
            write!(f, "No file in the build's metadata references it. ")?;
        } else {
            let named = self.files.iter().take(NAMED_FILES).cloned().collect::<Vec<_>>().join(", ");
            match self.total_files.saturating_sub(NAMED_FILES) {
                0 => write!(f, "It backs {named}. ")?,
                rest => write!(f, "It backs {named} and {rest} more. ")?,
            }
        }
        write!(f, "Retrying will not help: the data is corrupt at rest, and the build needs re-publishing.")
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

        let mut meta = BuildMetadata {
            version: "15.2.0".into(),
            build: 12100000,
            files: BTreeMap::new(),
            derived: BTreeMap::new(),
        };
        meta.files.insert("gui/test.png".into(), "abcdef1234567890abcd".into());

        meta.save(&path).unwrap();
        let loaded = BuildMetadata::load(&path).unwrap();
        assert_eq!(loaded.files.len(), 1);
        assert!(loaded.has_file_hashes());
    }

    fn corrupt(files: &[&str]) -> CorruptObject {
        CorruptObject {
            build: 12506899,
            version: "15.4.0".into(),
            hash: "a24a46f62dc08fd95fc7".into(),
            actual: "674dcbf6a9204c9fe942".into(),
            files: files.iter().map(|f| f.to_string()).collect(),
            total_files: files.len(),
        }
    }

    #[test]
    fn a_corrupt_object_message_names_the_build_and_caps_the_file_list() {
        let files: Vec<String> = (0..9).map(|i| format!("res/spaces/s{i}/space.settings")).collect();
        let err = corrupt(&files.iter().map(String::as_str).collect::<Vec<_>>());

        let rendered = err.to_string();
        assert_eq!(
            rendered,
            "build 12506899 (15.4.0) has a corrupt content object: a24a46f62dc08fd95fc7 hashed to \
             674dcbf6a9204c9fe942. It backs res/spaces/s0/space.settings, res/spaces/s1/space.settings, \
             res/spaces/s2/space.settings and 6 more. Retrying will not help: the data is corrupt at rest, and \
             the build needs re-publishing."
        );
        assert!(!rendered.contains("res/spaces/s3/space.settings"), "capped at three: {rendered}");
        assert!(rendered.len() < 400, "message must stay toast-sized, got {}", rendered.len());
    }

    /// An implementation that always appends the tail says "and 0 more".
    #[test]
    fn three_or_fewer_files_are_all_named_with_no_more_suffix() {
        let rendered = corrupt(&["content/GameParams.data", "gui/ribbons.png"]).to_string();

        assert_eq!(
            rendered,
            "build 12506899 (15.4.0) has a corrupt content object: a24a46f62dc08fd95fc7 hashed to \
             674dcbf6a9204c9fe942. It backs content/GameParams.data, gui/ribbons.png. Retrying will not help: \
             the data is corrupt at rest, and the build needs re-publishing."
        );
        assert!(!rendered.contains("more"), "no truncation tail belongs here: {rendered}");
    }

    /// The log gets what the message drops.
    #[test]
    fn the_full_file_list_survives_for_the_log() {
        let files: Vec<String> = (0..9).map(|i| format!("res/spaces/s{i}/space.settings")).collect();
        let err = corrupt(&files.iter().map(String::as_str).collect::<Vec<_>>());

        assert_eq!(err.all_files(), files.join(", "));
        assert!(err.all_files().contains("res/spaces/s8/space.settings"));
    }

    /// The reverse lookup spans both the extracted tree and derived artifacts,
    /// and names only the paths that actually read through the bad object.
    #[test]
    fn attribution_names_every_path_backed_by_the_hash() {
        let entry = BuildEntry {
            version: "15.4.0".into(),
            build: 12506899,
            dir: "15.4.0_12506899".into(),
            dumped_at: String::new(),
        };
        let mut metadata = BuildMetadata { version: "15.4.0".into(), build: 12506899, ..Default::default() };
        metadata.files.insert("res/a.xml".into(), "a24a46f62dc08fd95fc7".into());
        metadata.files.insert("res/b.xml".into(), "a24a46f62dc08fd95fc7".into());
        metadata.files.insert("res/other.xml".into(), "11111111111111111111".into());
        metadata.derived.insert("GameParams.rkyv".into(), "a24a46f62dc08fd95fc7".into());

        let err = CorruptObject::attribute(&entry, &metadata, "a24a46f62dc08fd95fc7", "674dcbf6a9204c9fe942");

        assert_eq!(err.files, vec!["res/a.xml", "res/b.xml", "GameParams.rkyv"]);
        assert_eq!(err.total_files, 3);
        assert_eq!(err.build, 12506899);
        assert_eq!(err.version, "15.4.0");
    }

    /// A hash no path claims must not render an empty file list.
    #[test]
    fn an_unreferenced_hash_still_renders_a_sensible_message() {
        let rendered = corrupt(&[]).to_string();

        assert!(rendered.contains("No file in the build's metadata references it"), "{rendered}");
        assert!(!rendered.contains("It backs"), "{rendered}");
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
