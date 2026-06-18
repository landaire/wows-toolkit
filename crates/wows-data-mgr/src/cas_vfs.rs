//! A read-only [`vfs::FileSystem`] backed by a build manifest and the shared
//! content-addressed store. Resolves a path to a content hash via the manifest
//! (`metadata.files`) and serves the bytes from `common/ab/hash`, so a build is
//! readable with no symlinked `vfs/` tree.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use wowsunpack::vfs::FileSystem;
use wowsunpack::vfs::VfsError;
use wowsunpack::vfs::VfsMetadata;
use wowsunpack::vfs::VfsPath;
use wowsunpack::vfs::error::VfsErrorKind;
use wowsunpack::vfs::impls::physical::PhysicalFS;

use crate::builds::BuildMetadata;
use crate::cas;

#[derive(Debug, Clone)]
enum CasEntry {
    File { hash: String },
    Directory { children: Vec<String> },
}

/// A read-only filesystem over a manifest (`rel_path -> hash`) and a CAS root.
#[derive(Debug)]
pub struct CasVfs {
    cas_root: PathBuf,
    entries: HashMap<String, CasEntry>,
}

impl CasVfs {
    /// Build the VFS from a CAS root and a `rel_path -> hash` manifest.
    pub fn new(cas_root: PathBuf, files: &BTreeMap<String, String>) -> Self {
        Self { cas_root, entries: build_entries(files) }
    }

    fn entry_at(&self, path: &str) -> wowsunpack::vfs::VfsResult<&CasEntry> {
        let key = if path.is_empty() { "/" } else { path };
        self.entries.get(key).ok_or_else(|| VfsError::from(VfsErrorKind::FileNotFound))
    }
}

/// Resolves how to read one downloaded build: a [`CasVfs`] over the manifest
/// when the build is CAS-format, or a `PhysicalFS` over a legacy `vfs/` tree
/// when the manifest carries no file hashes.
pub struct BuildCas {
    cas_root: PathBuf,
    dump_dir: PathBuf,
    metadata: BuildMetadata,
}

impl BuildCas {
    /// Open the build at `dump_dir`. The CAS store is the sibling `common/` of
    /// the dump base (`dump_dir`'s parent). Returns `None` when there is no
    /// readable `metadata.toml` or no parent directory.
    pub fn open(dump_dir: &Path) -> Option<Self> {
        let metadata = BuildMetadata::load(&dump_dir.join("metadata.toml"))?;
        let dump_base = dump_dir.parent()?;
        Some(Self {
            cas_root: cas::cas_root(dump_base),
            dump_dir: dump_dir.to_path_buf(),
            metadata,
        })
    }

    /// The build's parsed metadata.
    pub fn metadata(&self) -> &BuildMetadata {
        &self.metadata
    }

    /// A VFS for the build. CAS-format builds get a [`CasVfs`] (and a one-time
    /// prune of any stale symlinked tree); legacy builds get a `PhysicalFS`
    /// over the real `vfs/` directory.
    pub fn vfs(&self) -> VfsPath {
        if self.metadata.has_file_hashes() {
            self.prune_materialized_tree();
            VfsPath::new(CasVfs::new(self.cas_root.clone(), &self.metadata.files))
        } else {
            VfsPath::new(PhysicalFS::new(self.dump_dir.join("vfs")))
        }
    }

    /// On-disk path for a derived artifact (e.g. `game_params.rkyv` or a
    /// `translations/.../global.mo`). CAS-format builds resolve it to a CAS
    /// object (a real file usable for mmap); legacy builds resolve it to the
    /// build-dir path when that file exists. `None` when not available.
    pub fn derived_path(&self, rel: &str) -> Option<PathBuf> {
        if let Some(hash) = self.metadata.derived.get(rel) {
            return Some(cas::cas_path(&self.cas_root, hash));
        }
        let path = self.dump_dir.join(rel);
        path.exists().then_some(path)
    }

    /// Remove the redundant materialized tree a symlink-era download left behind:
    /// the `vfs/` directory and any artifact named in `derived`. Only called for
    /// CAS-format builds, where `common/` holds the real bytes.
    ///
    /// Conservative and best-effort: a path is deleted only when it is a symlink
    /// that resolves into this build's CAS store (`common/`). Real files and
    /// symlinks pointing elsewhere are left untouched, so user data is never
    /// destroyed. Every deletion is fault-tolerant: if removing a link fails
    /// (e.g. it requires elevated rights), the failure is logged and the link is
    /// left in place rather than propagating an error. Idempotent.
    fn prune_materialized_tree(&self) {
        let vfs_dir = self.dump_dir.join("vfs");
        if vfs_dir.exists() {
            self.prune_symlink_tree(&vfs_dir);
        }
        for rel in self.metadata.derived.keys() {
            self.remove_if_cas_symlink(&self.dump_dir.join(rel));
        }
    }

    /// Recursively prune CAS symlinks under `dir`, then remove directories left
    /// empty. Descends real subdirectories; never follows symlinks into the CAS.
    fn prune_symlink_tree(&self, dir: &Path) {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                self.remove_if_cas_symlink(&entry.path());
            } else if file_type.is_dir() {
                self.prune_symlink_tree(&entry.path());
            }
            // Real files are left untouched.
        }
        if std::fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(false)
            && let Err(e) = std::fs::remove_dir(dir)
        {
            tracing::debug!("leaving dir {} in place: {e}", dir.display());
        }
    }

    /// Delete `path` only when it is a symlink resolving into this build's CAS
    /// store. Logs and continues on any error (including deletion requiring
    /// elevated rights) so cleanup never fails a load.
    fn remove_if_cas_symlink(&self, path: &Path) {
        let is_symlink = path.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false);
        if !is_symlink {
            return;
        }
        if !symlink_points_into(path, &self.cas_root) {
            tracing::warn!("not pruning symlink that does not point into the CAS: {}", path.display());
            return;
        }
        if let Err(e) = std::fs::remove_file(path) {
            tracing::warn!("failed to prune symlink {} (left in place): {e}", path.display());
        }
    }
}

/// Whether `link` is a symlink whose target resolves into `cas_root`. Resolves
/// the (possibly relative) link target against the link's own directory and
/// compares canonicalized paths. A broken link (target already garbage
/// collected) cannot be canonicalized, so it falls back to a lexical check that
/// the target traverses the CAS directory name, keeping the prune conservative.
fn symlink_points_into(link: &Path, cas_root: &Path) -> bool {
    let Ok(target) = std::fs::read_link(link) else {
        return false;
    };
    let resolved =
        if target.is_absolute() { target.clone() } else { link.parent().unwrap_or(Path::new(".")).join(&target) };
    if let (Ok(resolved), Ok(root)) = (std::fs::canonicalize(&resolved), std::fs::canonicalize(cas_root)) {
        return resolved.starts_with(&root);
    }
    cas_root.file_name().is_some_and(|name| target.components().any(|comp| comp.as_os_str() == name))
}

/// Normalize a manifest-relative path to a VFS key: forward slashes, single
/// leading slash. `content/GameParams.data` becomes `/content/GameParams.data`.
fn normalize(rel: &str) -> String {
    let rel = rel.replace('\\', "/");
    let rel = rel.trim_start_matches('/');
    format!("/{rel}")
}

/// Build the entry map: one `File` per manifest entry plus every ancestor
/// `Directory` with its immediate children, rooted at `/`.
fn build_entries(files: &BTreeMap<String, String>) -> HashMap<String, CasEntry> {
    let mut entries: HashMap<String, CasEntry> = HashMap::with_capacity(files.len() + 1);
    entries.insert("/".to_string(), CasEntry::Directory { children: Vec::new() });

    for (rel, hash) in files {
        let path = normalize(rel);
        entries.insert(path.clone(), CasEntry::File { hash: hash.clone() });

        let mut cur = path.as_str();
        loop {
            let (parent, name) = match cur.rfind('/') {
                Some(0) => ("/", &cur[1..]),
                Some(pos) => (&cur[..pos], &cur[pos + 1..]),
                None => break,
            };
            let parent_entry = entries
                .entry(parent.to_string())
                .or_insert_with(|| CasEntry::Directory { children: Vec::new() });
            if let CasEntry::Directory { children } = parent_entry {
                children.push(name.to_string());
            }
            if parent == "/" {
                break;
            }
            cur = parent;
        }
    }

    for entry in entries.values_mut() {
        if let CasEntry::Directory { children } = entry {
            children.sort();
            children.dedup();
        }
    }

    entries
}

impl FileSystem for CasVfs {
    fn read_dir(&self, path: &str) -> wowsunpack::vfs::VfsResult<Box<dyn Iterator<Item = String> + Send>> {
        match self.entry_at(path)? {
            CasEntry::Directory { children } => Ok(Box::new(children.clone().into_iter())),
            CasEntry::File { .. } => Err(VfsError::from(VfsErrorKind::Other("not a directory".into()))),
        }
    }

    fn create_dir(&self, _path: &str) -> wowsunpack::vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn open_file(&self, path: &str) -> wowsunpack::vfs::VfsResult<Box<dyn wowsunpack::vfs::SeekAndRead + Send>> {
        let CasEntry::File { hash } = self.entry_at(path)? else {
            return Err(VfsError::from(VfsErrorKind::Other("not a file".into())));
        };
        let object = cas::cas_path(&self.cas_root, hash);
        let file = std::fs::File::open(&object).map_err(|e| VfsError::from(VfsErrorKind::IoError(e)))?;
        Ok(Box::new(file))
    }

    fn create_file(&self, _path: &str) -> wowsunpack::vfs::VfsResult<Box<dyn wowsunpack::vfs::SeekAndWrite + Send>> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn append_file(&self, _path: &str) -> wowsunpack::vfs::VfsResult<Box<dyn wowsunpack::vfs::SeekAndWrite + Send>> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn metadata(&self, path: &str) -> wowsunpack::vfs::VfsResult<VfsMetadata> {
        match self.entry_at(path)? {
            CasEntry::Directory { .. } => Ok(VfsMetadata {
                file_type: wowsunpack::vfs::VfsFileType::Directory,
                len: 0,
                created: None,
                modified: None,
                accessed: None,
            }),
            CasEntry::File { hash } => {
                let len = std::fs::metadata(cas::cas_path(&self.cas_root, hash))
                    .map_err(|e| VfsError::from(VfsErrorKind::IoError(e)))?
                    .len();
                Ok(VfsMetadata { file_type: wowsunpack::vfs::VfsFileType::File, len, created: None, modified: None, accessed: None })
            }
        }
    }

    fn exists(&self, path: &str) -> wowsunpack::vfs::VfsResult<bool> {
        Ok(self.entry_at(path).is_ok())
    }

    fn remove_file(&self, _path: &str) -> wowsunpack::vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn remove_dir(&self, _path: &str) -> wowsunpack::vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn set_creation_time(&self, _path: &str, _time: std::time::SystemTime) -> wowsunpack::vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn set_modification_time(&self, _path: &str, _time: std::time::SystemTime) -> wowsunpack::vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn set_access_time(&self, _path: &str, _time: std::time::SystemTime) -> wowsunpack::vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn copy_file(&self, _src: &str, _dest: &str) -> wowsunpack::vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn move_file(&self, _src: &str, _dest: &str) -> wowsunpack::vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn move_dir(&self, _src: &str, _dest: &str) -> wowsunpack::vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use wowsunpack::vfs::VfsPath;

    use crate::builds::BuildMetadata;

    /// Store the given `rel -> bytes` files in a fresh CAS and return
    /// (cas_root, manifest).
    fn fixture(files: &[(&str, &[u8])]) -> (tempfile::TempDir, PathBuf, BTreeMap<String, String>) {
        let dir = tempfile::tempdir().unwrap();
        let cas_root = dir.path().join("common");
        let mut manifest = BTreeMap::new();
        for (rel, bytes) in files {
            let hash = cas::store(&cas_root, bytes).unwrap();
            manifest.insert((*rel).to_string(), hash);
        }
        (dir, cas_root, manifest)
    }

    #[test]
    fn open_file_reads_cas_object() {
        let (_d, cas_root, manifest) = fixture(&[("content/GameParams.data", b"params bytes")]);
        let vfs = VfsPath::new(CasVfs::new(cas_root, &manifest));
        let mut data = Vec::new();
        vfs.join("content/GameParams.data").unwrap().open_file().unwrap().read_to_end(&mut data).unwrap();
        assert_eq!(data, b"params bytes");
    }

    #[test]
    fn read_dir_lists_children() {
        let (_d, cas_root, manifest) =
            fixture(&[("gui/a.png", b"a"), ("gui/b.png", b"b"), ("content/x.dat", b"x")]);
        let vfs = VfsPath::new(CasVfs::new(cas_root, &manifest));
        let mut root: Vec<String> = vfs.read_dir().unwrap().map(|p| p.filename()).collect();
        root.sort();
        assert_eq!(root, vec!["content".to_string(), "gui".to_string()]);
        let mut gui: Vec<String> = vfs.join("gui").unwrap().read_dir().unwrap().map(|p| p.filename()).collect();
        gui.sort();
        assert_eq!(gui, vec!["a.png".to_string(), "b.png".to_string()]);
    }

    #[test]
    fn metadata_reports_len_and_type() {
        let (_d, cas_root, manifest) = fixture(&[("content/x.dat", b"twelve bytes")]);
        let vfs = VfsPath::new(CasVfs::new(cas_root, &manifest));
        assert_eq!(vfs.join("content/x.dat").unwrap().metadata().unwrap().len, 12);
        assert_eq!(
            vfs.join("content").unwrap().metadata().unwrap().file_type,
            wowsunpack::vfs::VfsFileType::Directory
        );
    }

    #[test]
    fn missing_path_errors() {
        let (_d, cas_root, manifest) = fixture(&[("content/x.dat", b"x")]);
        let vfs = VfsPath::new(CasVfs::new(cas_root, &manifest));
        assert!(!vfs.join("content/missing.dat").unwrap().exists().unwrap());
        assert!(vfs.join("content/missing.dat").unwrap().open_file().is_err());
    }

    #[test]
    fn manifest_hash_without_object_errors() {
        let dir = tempfile::tempdir().unwrap();
        let cas_root = dir.path().join("common");
        let mut manifest = BTreeMap::new();
        manifest.insert("content/x.dat".to_string(), "0123456789abcdef0123".to_string());
        let vfs = VfsPath::new(CasVfs::new(cas_root, &manifest));
        assert!(vfs.join("content/x.dat").unwrap().open_file().is_err());
    }

    #[test]
    fn writes_are_unsupported() {
        let (_d, cas_root, manifest) = fixture(&[("content/x.dat", b"x")]);
        let fs = CasVfs::new(cas_root, &manifest);
        assert!(fs.create_file("/content/y.dat").is_err());
        assert!(fs.remove_file("/content/x.dat").is_err());
        assert!(fs.create_dir("/newdir").is_err());
    }

    /// Write a CAS-format build dir under a dump base and return its dump_dir.
    fn cas_build(base: &Path, files: &[(&str, &[u8])], derived: &[(&str, &[u8])]) -> PathBuf {
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

    /// Create a file symlink, returning false when the platform/account forbids
    /// it (the exact situation this whole feature exists to avoid). Tests that
    /// need symlinks skip gracefully when this returns false.
    fn try_symlink(target: &Path, link: &Path) -> bool {
        #[cfg(windows)]
        let result = std::os::windows::fs::symlink_file(target, link);
        #[cfg(not(windows))]
        let result = std::os::unix::fs::symlink(target, link);
        result.is_ok()
    }

    #[test]
    fn buildcas_vfs_reads_cas_format() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = cas_build(base.path(), &[("content/x.dat", b"hi")], &[]);
        let cas = BuildCas::open(&dump_dir).unwrap();
        let mut data = Vec::new();
        cas.vfs().join("content/x.dat").unwrap().open_file().unwrap().read_to_end(&mut data).unwrap();
        assert_eq!(data, b"hi");
    }

    #[test]
    fn buildcas_derived_path_resolves_to_cas_object() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = cas_build(base.path(), &[], &[("game_params.rkyv", b"rkyv bytes")]);
        let cas = BuildCas::open(&dump_dir).unwrap();
        let path = cas.derived_path("game_params.rkyv").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"rkyv bytes");
        assert!(cas.derived_path("translations/en/LC_MESSAGES/global.mo").is_none());
    }

    #[test]
    fn buildcas_legacy_empty_files_uses_physical_vfs() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = base.path().join("0.6.0_50");
        std::fs::create_dir_all(dump_dir.join("vfs/content")).unwrap();
        std::fs::write(dump_dir.join("vfs/content/x.dat"), b"legacy").unwrap();
        let meta = BuildMetadata { version: "0.6.0".into(), build: 50, ..Default::default() };
        meta.save(&dump_dir.join("metadata.toml")).unwrap();

        let cas = BuildCas::open(&dump_dir).unwrap();
        assert!(!cas.metadata().has_file_hashes());
        let mut data = Vec::new();
        cas.vfs().join("content/x.dat").unwrap().open_file().unwrap().read_to_end(&mut data).unwrap();
        assert_eq!(data, b"legacy");
        // Legacy tree is the only copy: it must survive.
        assert!(dump_dir.join("vfs/content/x.dat").exists());
    }

    #[test]
    fn buildcas_prunes_only_cas_symlinks_and_preserves_others() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = cas_build(base.path(), &[("content/x.dat", b"hi")], &[("game_params.rkyv", b"rkyv")]);
        let cas_root = base.path().join("common");

        // A stale symlinked vfs tree pointing into common/.
        let vfs_file = dump_dir.join("vfs/content/x.dat");
        std::fs::create_dir_all(vfs_file.parent().unwrap()).unwrap();
        let x_target = cas::cas_path(&cas_root, &cas::hash_bytes(b"hi"));
        if !try_symlink(&x_target, &vfs_file) {
            eprintln!("skipping: cannot create symlinks on this platform/account");
            return;
        }
        // A derived symlink into common/.
        let derived_link = dump_dir.join("game_params.rkyv");
        let rkyv_target = cas::cas_path(&cas_root, &cas::hash_bytes(b"rkyv"));
        assert!(try_symlink(&rkyv_target, &derived_link));
        // A symlink that does NOT point into common/ must be preserved.
        let outside = base.path().join("outside.bin");
        std::fs::write(&outside, b"keep me").unwrap();
        let foreign = dump_dir.join("vfs/content/foreign.dat");
        assert!(try_symlink(&outside, &foreign));

        let cas = BuildCas::open(&dump_dir).unwrap();
        let _ = cas.vfs();

        // CAS symlinks (vfs tree + derived) are pruned; the CAS objects survive.
        assert!(!vfs_file.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()));
        assert!(!derived_link.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()));
        assert!(x_target.exists());
        assert!(rkyv_target.exists());
        // A symlink pointing outside the CAS is preserved, and so is its target.
        assert!(foreign.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()));
        assert_eq!(std::fs::read(&outside).unwrap(), b"keep me");
        // metadata.toml is never touched, and a second prune is a no-op.
        assert!(dump_dir.join("metadata.toml").exists());
        let _ = cas.vfs();
        assert!(dump_dir.join("metadata.toml").exists());
    }

    #[test]
    fn buildcas_prune_leaves_real_files_untouched() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = cas_build(base.path(), &[("content/x.dat", b"hi")], &[]);
        // A leftover vfs tree of REAL files (not symlinks) must never be deleted.
        let real = dump_dir.join("vfs/content/x.dat");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, b"real bytes").unwrap();

        let cas = BuildCas::open(&dump_dir).unwrap();
        let _ = cas.vfs();
        assert_eq!(std::fs::read(&real).unwrap(), b"real bytes");
    }

    #[test]
    fn buildcas_open_missing_metadata_is_none() {
        let base = tempfile::tempdir().unwrap();
        let dump_dir = base.path().join("nope_1");
        std::fs::create_dir_all(&dump_dir).unwrap();
        assert!(BuildCas::open(&dump_dir).is_none());
    }
}
