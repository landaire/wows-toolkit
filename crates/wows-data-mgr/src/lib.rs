//! Test helper API for accessing downloaded World of Warships game data.
//!
//! Use these functions in integration tests to get VFS access to game builds.
//! Tests should skip gracefully when game data is unavailable.
//!
//! # Example
//!
//! ```ignore
//! use wows_data_mgr::{available_builds, vfs_for_build};
//!
//! #[test]
//! fn test_game_params_load() {
//!     let builds = available_builds();
//!     if builds.is_empty() {
//!         eprintln!("Skipping: no game data available");
//!         return;
//!     }
//!     for build in builds {
//!         let vfs = vfs_for_build(build).unwrap();
//!         // test with vfs...
//!     }
//! }
//! ```

/// The `tracing` target every log statement in this crate is emitted under.
///
/// Consumers that filter by target (the toolkit's log file writes through an
/// allowlist) reference this instead of a hand-copied literal, so renaming the
/// crate cannot silently drop its logs on the floor.
pub const LOG_TARGET: &str = module_path!();

pub mod builds;
pub mod cas;
pub mod cas_vfs;
#[cfg(feature = "constants")]
pub mod constants;
#[cfg(feature = "download")]
pub mod download_repo;
pub mod dump;
pub mod manifest;
pub mod overrides;
pub mod registry;

use std::path::Path;
use std::path::PathBuf;

use wowsunpack::game_data;
use wowsunpack::vfs::VfsPath;
use wowsunpack::vfs::impls::physical::PhysicalFS;

/// Returns the path to the game_data/ directory.
///
/// Checks `WOWS_GAME_DATA` env var first, then walks up from the current
/// directory to find the workspace root (identified by `game_versions.toml`).
pub fn game_data_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("WOWS_GAME_DATA") {
        let path = PathBuf::from(dir);
        if path.exists() {
            return Some(path);
        }
    }

    // Walk up from current dir to find repo root
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("game_versions.toml").exists() {
            let data_dir = dir.join("game_data");
            return Some(data_dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Returns sorted list of locally available build numbers.
///
/// Reads the local registry to find both downloaded builds
/// (in `game_data/builds/<build>/`) and registered overrides.
pub fn available_builds() -> Vec<u32> {
    let Some(data_dir) = game_data_dir() else {
        return Vec::new();
    };
    let reg = registry::load_registry(&data_dir.join("versions.toml"));
    // A registry entry is a claim, not proof: a build whose directory was moved
    // or renamed is not available to read, and callers treat this list as data
    // they can open.
    reg.available_builds().into_iter().filter(|build| reg.game_dir_for_build(*build, &data_dir).is_some()).collect()
}

/// Returns the game root path for a specific build.
///
/// For registered overrides, returns the override path.
/// For downloaded builds, returns `game_data/builds/<build>/`.
pub fn game_dir_for_build(build: u32) -> Option<PathBuf> {
    let data_dir = game_data_dir()?;
    let reg = registry::load_registry(&data_dir.join("versions.toml"));
    reg.game_dir_for_build(build, &data_dir)
}

/// A dump directory resolved for reading, whichever on-disk layout it uses.
///
/// A CAS-format dump keeps its bytes in the shared `common/` store and has no
/// `vfs/` tree at all, so reading one through `PhysicalFS` finds nothing. A
/// directory with no readable `metadata.toml` (a hand-extracted dump, or one
/// produced before the content-addressed layout) is served from `vfs/`.
pub struct Dump {
    dump_dir: PathBuf,
    cas: Option<cas_vfs::BuildCas>,
    override_root: Option<PathBuf>,
}

impl Dump {
    /// Resolve `dump_dir`, parsing its manifest once when it has one.
    pub fn open(dump_dir: &Path) -> Self {
        Self { dump_dir: dump_dir.to_path_buf(), cas: cas_vfs::BuildCas::open(dump_dir), override_root: None }
    }

    /// Point this dump at a writable directory whose contents shadow its derived
    /// artifacts, so a cache that no longer loads can be rebuilt once and reused
    /// without editing a dump that may be shared and read-only.
    pub fn set_override_root(&mut self, root: PathBuf) {
        if let Some(cas) = self.cas.as_mut() {
            cas.set_override_root(root.clone());
        }
        self.override_root = Some(root);
    }

    /// A VFS over the dump's game files.
    pub fn vfs(&self) -> VfsPath {
        match &self.cas {
            Some(cas) => cas.vfs(),
            None => VfsPath::new(PhysicalFS::new(self.dump_dir.join("vfs"))),
        }
    }

    /// Path to a derived artifact such as `game_params.rkyv`, or `None` when
    /// this dump does not carry one.
    pub fn derived_path(&self, rel: &str) -> Option<PathBuf> {
        match &self.cas {
            Some(cas) => cas.derived_path(rel),
            None => {
                if let Some(root) = &self.override_root {
                    let path = root.join(rel);
                    if path.exists() {
                        return Some(path);
                    }
                }
                let path = self.dump_dir.join(rel);
                path.exists().then_some(path)
            }
        }
    }

    /// Where to write an override for `rel`, creating its parent directory.
    /// `None` when no override root is set or the directory cannot be created,
    /// in which case the caller simply does not cache.
    pub fn derived_write_path(&self, rel: &str) -> Option<PathBuf> {
        match &self.cas {
            Some(cas) => cas.derived_write_path(rel),
            None => {
                let path = self.override_root.as_ref()?.join(rel);
                std::fs::create_dir_all(path.parent()?).ok()?;
                Some(path)
            }
        }
    }

    /// The build this dump holds, or `None` for a dump with no manifest.
    pub fn build(&self) -> Option<u32> {
        self.cas.as_ref().map(|cas| cas.metadata().build)
    }

    /// Whether this dump has game files to read. Callers that skip when data is
    /// unavailable test this rather than probing for `vfs/`, which a CAS-format
    /// dump legitimately lacks.
    pub fn has_game_files(&self) -> bool {
        match &self.cas {
            Some(cas) => cas.metadata().has_file_hashes() || self.dump_dir.join("vfs").is_dir(),
            None => self.dump_dir.join("vfs").is_dir(),
        }
    }
}

/// Resolves a build number to a readable [`Dump`].
pub fn dump_for_build(build: u32) -> Option<Dump> {
    Some(Dump::open(&game_dir_for_build(build)?))
}

/// Constructs a VFS for a specific build.
///
/// Most registered builds are dumps, which are read through [`Dump`]. A build
/// that resolves to a real game installation has no dump layout to read, and
/// falls back to [`wowsunpack::game_data::build_game_vfs`] over its packages.
pub fn vfs_for_build(build: u32) -> Option<VfsPath> {
    let game_dir = game_dir_for_build(build)?;
    let dump = Dump::open(&game_dir);
    if dump.has_game_files() {
        return Some(dump.vfs());
    }
    game_data::build_game_vfs(&game_dir).ok()
}

/// Returns the latest available build number and its VFS.
pub fn latest_build() -> Option<(u32, VfsPath)> {
    let builds = available_builds();
    let build = *builds.last()?;
    let vfs = vfs_for_build(build)?;
    Some((build, vfs))
}

#[cfg(test)]
mod dump_tests {
    use std::io::Read;

    use super::*;
    use crate::builds::BuildMetadata;

    /// A CAS-format dump: bytes live in the sibling `common/` store and there
    /// is no `vfs/` tree.
    fn cas_dump(base: &Path, files: &[(&str, &[u8])], derived: &[(&str, &[u8])]) -> PathBuf {
        let cas_root = base.join("common");
        let mut meta = BuildMetadata { version: "1.2.3".into(), build: 100, ..Default::default() };
        for (rel, bytes) in files {
            meta.files.insert((*rel).to_string(), cas::store(&cas_root, bytes).unwrap());
        }
        for (rel, bytes) in derived {
            meta.derived.insert((*rel).to_string(), cas::store(&cas_root, bytes).unwrap());
        }
        let dump_dir = base.join("1.2.3_100");
        std::fs::create_dir_all(&dump_dir).unwrap();
        meta.save(&dump_dir.join("metadata.toml")).unwrap();
        dump_dir
    }

    /// A pre-CAS dump: a real `vfs/` tree and no manifest.
    fn legacy_dump(base: &Path, files: &[(&str, &[u8])]) -> PathBuf {
        let dump_dir = base.join("legacy");
        for (rel, bytes) in files {
            let path = dump_dir.join("vfs").join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        dump_dir
    }

    fn read(vfs: &VfsPath, rel: &str) -> Vec<u8> {
        let mut data = Vec::new();
        vfs.join(rel).unwrap().open_file().unwrap().read_to_end(&mut data).unwrap();
        data
    }

    #[test]
    fn cas_dump_reads_game_files_without_a_vfs_tree() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = cas_dump(base.path(), &[("content/GameParams.data", b"params")], &[]);

        let dump = Dump::open(&dump_dir);

        assert!(!dump_dir.join("vfs").exists(), "a CAS dump has no vfs tree to read");
        assert!(dump.has_game_files());
        assert_eq!(read(&dump.vfs(), "content/GameParams.data"), b"params");
    }

    #[test]
    fn cas_dump_resolves_derived_artifacts_into_the_store() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = cas_dump(base.path(), &[], &[("game_params.rkyv", b"rkyv bytes")]);

        let dump = Dump::open(&dump_dir);
        let path = dump.derived_path("game_params.rkyv").expect("derived artifact");

        assert_eq!(std::fs::read(path).unwrap(), b"rkyv bytes");
        assert!(dump.derived_path("absent.rkyv").is_none());
    }

    #[test]
    fn legacy_dump_still_reads_from_its_vfs_tree() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = legacy_dump(base.path(), &[("content/GameParams.data", b"params")]);
        std::fs::write(dump_dir.join("game_params.rkyv"), b"rkyv bytes").unwrap();

        let dump = Dump::open(&dump_dir);

        assert!(dump.has_game_files());
        assert_eq!(read(&dump.vfs(), "content/GameParams.data"), b"params");
        assert_eq!(std::fs::read(dump.derived_path("game_params.rkyv").unwrap()).unwrap(), b"rkyv bytes");
    }

    #[test]
    fn empty_directory_has_no_game_files() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = base.path().join("nothing");
        std::fs::create_dir_all(&dump_dir).unwrap();

        let dump = Dump::open(&dump_dir);

        assert!(!dump.has_game_files());
        assert!(dump.derived_path("game_params.rkyv").is_none());
    }

    #[test]
    fn override_shadows_the_dumps_own_derived_copy() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = cas_dump(base.path(), &[], &[("game_params.rkyv", b"stale bytes")]);
        let overrides = base.path().join("overrides");

        let mut dump = Dump::open(&dump_dir);
        dump.set_override_root(overrides.clone());
        let write_path = dump.derived_write_path("game_params.rkyv").expect("a place to write");
        std::fs::write(&write_path, b"fresh bytes").unwrap();

        assert_eq!(write_path, overrides.join("game_params.rkyv"));
        assert_eq!(std::fs::read(dump.derived_path("game_params.rkyv").unwrap()).unwrap(), b"fresh bytes");
        assert_eq!(dump.build(), Some(100));
    }

    #[test]
    fn legacy_dump_honours_an_override_too() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = legacy_dump(base.path(), &[("content/GameParams.data", b"params")]);
        std::fs::write(dump_dir.join("game_params.rkyv"), b"stale bytes").unwrap();
        let overrides = base.path().join("overrides");

        let mut dump = Dump::open(&dump_dir);
        dump.set_override_root(overrides.clone());
        let write_path = dump.derived_write_path("game_params.rkyv").expect("a place to write");
        std::fs::write(&write_path, b"fresh bytes").unwrap();

        assert_eq!(std::fs::read(dump.derived_path("game_params.rkyv").unwrap()).unwrap(), b"fresh bytes");
        // A legacy dump has no manifest, so it has no build number to report.
        assert_eq!(dump.build(), None);
    }
}

#[cfg(test)]
mod log_target_tests {
    /// `tracing` targets default to the emitting module's path, and a target
    /// filter matches on the leading segments, so every module in this crate is
    /// covered by the crate-level target. Renaming the crate changes both sides
    /// together, which is why consumers must not hard-code the string.
    #[test]
    fn every_module_in_this_crate_logs_under_the_crate_target() {
        assert_eq!(super::LOG_TARGET, "wows_data_mgr");
        assert!(module_path!().starts_with(super::LOG_TARGET), "{}", module_path!());
    }
}
