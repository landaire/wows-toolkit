//! A read-only [`vfs::FileSystem`] backed by a build manifest and the shared
//! content-addressed store. Resolves a path to a content hash via the manifest
//! (`metadata.files`) and serves the bytes from `common/ab/hash`, so a build is
//! readable with no symlinked `vfs/` tree.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;

use wowsunpack::vfs::FileSystem;
use wowsunpack::vfs::VfsError;
use wowsunpack::vfs::VfsMetadata;
use wowsunpack::vfs::error::VfsErrorKind;

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
}
