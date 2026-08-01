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

/// Maximum number of characters those paths may occupy between them.
///
/// A cap on how many paths are named is not a cap on how long the message gets:
/// measured over `15.6.0_12830008` (4074 paths) a path is 49 characters at the
/// median, 88 at the 99th percentile and 101 at the longest, so "three paths"
/// is anywhere from 150 to 300 characters of message. Paths are named whole or
/// not at all -- half a path identifies nothing -- and the count in the tail
/// accounts for every one left out.
const NAMED_FILES_BUDGET: usize = 160;

/// The length a [`CorruptObject`] message stays within, whatever it is handed.
///
/// This holds for any file list, and for a build number up to [`u32::MAX`], a
/// version string up to 20 characters and the 20-character digests this crate
/// produces. The toast that shows it renders the enclosing report rather than
/// this string alone, so what the user sees also carries the report's tree
/// glyphs and its location attachment on top of this.
pub const MAX_MESSAGE_CHARS: usize = 400;

/// What separates named paths in the message.
const FILE_SEPARATOR: &str = ", ";

/// A content object whose bytes do not hash to the name it is stored under,
/// attributed to the build that references it and the files it backs.
///
/// A hash is 20 hex characters and says nothing on its own. Carrying the build,
/// the version and the referencing paths is what makes the failure reportable.
///
/// Its [`Display`](std::fmt::Display) is what reaches a toast, and renders
/// within [`MAX_MESSAGE_CHARS`] however many paths the object backs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorruptObject {
    pub build: u32,
    pub version: String,
    /// The name the object is stored under, which is the hash its bytes must
    /// reproduce.
    pub hash: String,
    /// What the bytes actually hashed to.
    pub actual: String,
    /// Every path in the build that reads through this object. Goes to the log
    /// in full; [`Display`](std::fmt::Display) names at most [`NAMED_FILES`] of
    /// them and counts the rest.
    pub files: Vec<String>,
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
            files,
        }
    }

    /// The full list of referencing paths, for the log.
    pub fn all_files(&self) -> String {
        self.files.join(FILE_SEPARATOR)
    }

    /// The paths the message names, and how many it leaves out. Whole paths
    /// only, within both the count cap and the character budget.
    fn named_files(&self) -> (Vec<&str>, usize) {
        let mut named: Vec<&str> = Vec::new();
        let mut used = 0;
        for file in self.files.iter().take(NAMED_FILES) {
            let separator = if named.is_empty() { 0 } else { FILE_SEPARATOR.len() };
            if used + separator + file.len() > NAMED_FILES_BUDGET {
                break;
            }
            used += separator + file.len();
            named.push(file.as_str());
        }
        let rest = self.files.len() - named.len();
        (named, rest)
    }
}

impl std::fmt::Display for CorruptObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "build {} ({}) has a corrupt content object: {} hashed to {}. ",
            self.build, self.version, self.hash, self.actual
        )?;
        match self.named_files() {
            (named, 0) if named.is_empty() => write!(f, "No file in the build's metadata references it. ")?,
            (named, rest) if named.is_empty() => {
                write!(f, "It backs {rest} file(s) whose paths are too long to name here. ")?
            }
            (named, 0) => write!(f, "It backs {}. ", named.join(FILE_SEPARATOR))?,
            (named, rest) => write!(f, "It backs {} and {rest} more. ", named.join(FILE_SEPARATOR))?,
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
        }
    }

    /// A path of exactly `len` characters, unique per `index` and shaped like a
    /// real one. Paths in `15.6.0_12830008` measure 49 at the median, 71 at p90,
    /// 88 at p99 and 101 at the longest.
    fn path_of(len: usize, index: usize) -> String {
        let prefix = "res/content/gameplay/common/spaces/";
        let tail = format!("/{index:05}.dds");
        let filler = len.saturating_sub(prefix.len() + tail.len());
        format!("{prefix}{}{tail}", "s".repeat(filler))
    }

    /// The fixture is p99-realistic (88 characters), because a fixture of short
    /// synthetic paths makes a length assertion certify nothing: 28-character
    /// paths render at 304 characters where the real archive's render at 433.
    #[test]
    fn a_corrupt_object_message_names_the_build_and_caps_the_file_list() {
        let files: Vec<String> = (0..9).map(|i| path_of(88, i)).collect();
        let err = corrupt(&files.iter().map(String::as_str).collect::<Vec<_>>());

        let rendered = err.to_string();
        assert_eq!(
            rendered,
            format!(
                "build 12506899 (15.4.0) has a corrupt content object: a24a46f62dc08fd95fc7 hashed to \
                 674dcbf6a9204c9fe942. It backs {} and 8 more. Retrying will not help: the data is corrupt at \
                 rest, and the build needs re-publishing.",
                files[0]
            )
        );
        assert!(!rendered.contains("00001.dds"), "a second path of this length does not fit: {rendered}");
        assert!(rendered.len() <= MAX_MESSAGE_CHARS, "got {}", rendered.len());
    }

    /// The bound is on the rendered string, not on how many paths went into it.
    /// Every combination of path length and count the real archive can produce,
    /// plus lengths well past anything it holds, and the widest build number
    /// and version the fields can carry.
    #[test]
    fn a_corrupt_object_message_stays_within_its_length_bound() {
        let mut worst = String::new();
        for path_len in [28, 49, 71, 88, 101, 250, 4_000] {
            for count in [1, 2, 3, 4, 9, 4_074] {
                let files: Vec<String> = (0..count).map(|i| path_of(path_len, i)).collect();
                let err = CorruptObject {
                    build: u32::MAX,
                    version: "15.6.0-preview-build".into(),
                    hash: "a24a46f62dc08fd95fc7".into(),
                    actual: "674dcbf6a9204c9fe942".into(),
                    files,
                };

                let rendered = err.to_string();
                if rendered.len() > worst.len() {
                    worst = rendered;
                }
            }
        }

        assert!(worst.len() <= MAX_MESSAGE_CHARS, "the longest rendered at {}: {worst}", worst.len());
    }

    /// A truncated path identifies nothing, so a path that does not fit is
    /// dropped rather than cut, and the count still accounts for it.
    #[test]
    fn a_path_too_long_for_the_budget_is_dropped_whole_and_still_counted() {
        let long = path_of(4_000, 0);
        let err = corrupt(&[long.as_str(), "res/b.xml"]);

        let rendered = err.to_string();
        assert!(rendered.contains("It backs 2 file(s) whose paths are too long to name here."), "{rendered}");
        assert!(!rendered.contains("res/content/gameplay"), "no partial path may appear: {rendered}");
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
