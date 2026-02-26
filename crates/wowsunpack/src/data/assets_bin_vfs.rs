//! VFS abstraction for reading files from an assets.bin PrototypeDatabase.
//!
//! Exposes prototype records as virtual files, keyed by their reconstructed
//! path from the pathsStorage tree. Each file's contents are the raw prototype
//! record data from the record start through the end of the containing blob,
//! preserving relative pointer resolution into out-of-line data.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::io::Cursor;

use vfs::error::VfsErrorKind;
use vfs::{FileSystem, VfsMetadata};

use crate::models::assets_bin::{self, AssetsBinError, PrototypeDatabase};

/// Known item sizes for each blob type (from RE).
/// Index corresponds to blob index in the databases array.
const ITEM_SIZES: [usize; 10] = [
    0x78, // 0: MaterialPrototype
    0x70, // 1: VisualPrototype
    0x20, // 2: SkeletonExtenderPrototype
    0x28, // 3: ModelPrototype
    0x70, // 4: PointLightPrototype
    0x10, // 5: EffectPrototype
    0x18, // 6: VelocityFieldPrototype
    0x10, // 7: EffectPresetPrototype
    0x10, // 8: EffectMetadataPrototype
    0x10, // 9: AtlasContourProto
];

/// Pre-computed file location within the owned assets.bin data.
#[derive(Debug, Clone)]
struct FileLocation {
    /// Byte offset from start of `data` to the record.
    byte_offset: usize,
    /// Byte offset from start of `data` to end of the blob.
    byte_end: usize,
}

/// A virtual filesystem backed by an assets.bin PrototypeDatabase.
///
/// Owns the raw file data and exposes prototype records as virtual files.
/// Paths match the game's resource paths (e.g. `content/gameplay/.../foo.visual`).
#[derive(Debug)]
pub struct AssetsBinVfs {
    data: Vec<u8>,
    files: HashMap<String, FileLocation>,
    dirs: HashMap<String, Vec<String>>,
}

/// Compute the byte offset of a subslice within a parent slice.
fn subslice_offset(parent: &[u8], child: &[u8]) -> usize {
    let parent_start = parent.as_ptr() as usize;
    let child_start = child.as_ptr() as usize;
    debug_assert!(
        child_start >= parent_start && child_start + child.len() <= parent_start + parent.len(),
        "child slice is not within parent"
    );
    child_start - parent_start
}

/// Register a file path's directory ancestors in the directory map.
///
/// Paths use `/`-prefixed format (e.g. `/content/foo.visual`), root = `"/"`.
fn register_path_in_dirs(path: &str, dirs: &mut HashMap<String, BTreeSet<String>>) {
    let mut current = path.to_string();
    while let Some(pos) = current.rfind('/') {
        let child_name = &current[pos + 1..];
        let mut parent = current[..pos].to_string();
        if parent.is_empty() {
            parent = "/".to_string();
        }

        dirs.entry(parent.clone()).or_default().insert(child_name.to_string());

        if parent == "/" {
            break;
        }
        current = parent;
    }
}

impl AssetsBinVfs {
    /// Build a VFS from owned assets.bin file data.
    ///
    /// Parses the PrototypeDatabase, builds a path index mapping every
    /// prototype record to its byte range within `data`, then discards
    /// the parsed database. Only the raw bytes and the index are retained.
    pub fn new(data: Vec<u8>) -> Result<Self, rootcause::Report<AssetsBinError>> {
        let db = assets_bin::parse_assets_bin(&data)?;
        let (files, dirs) = Self::build_index(&db, &data);
        Ok(Self { data, files, dirs })
    }

    fn build_index(
        db: &PrototypeDatabase<'_>,
        data: &[u8],
    ) -> (HashMap<String, FileLocation>, HashMap<String, Vec<String>>) {
        let self_id_index = db.build_self_id_index();
        let mut files = HashMap::new();
        let mut dir_children: HashMap<String, BTreeSet<String>> = HashMap::new();

        // Ensure root directory exists.
        dir_children.entry("/".to_string()).or_default();

        // Register all paths that have prototype data as files.
        for (i, entry) in db.paths_storage.iter().enumerate() {
            let Some(r2p_value) = db.lookup_r2p(entry.self_id) else {
                continue;
            };
            let Ok(location) = db.decode_r2p_value(r2p_value) else {
                continue;
            };
            if location.blob_index >= ITEM_SIZES.len() {
                continue;
            }

            let raw_path = db.reconstruct_path(i, &self_id_index);
            if raw_path.is_empty() {
                continue;
            }

            let item_size = ITEM_SIZES[location.blob_index];
            let blob = &db.databases[location.blob_index];

            let blob_start = subslice_offset(data, blob.data);
            let header_size = 16usize;
            let record_offset = blob_start + header_size + location.record_index * item_size;
            let blob_end = blob_start + blob.data.len();

            if record_offset + item_size > blob_end {
                continue;
            }

            let full_path = format!("/{raw_path}");
            files.insert(full_path.clone(), FileLocation { byte_offset: record_offset, byte_end: blob_end });

            register_path_in_dirs(&full_path, &mut dir_children);
        }

        // Register parent directories for path entries that have no prototype
        // data (e.g. .geometry files that live in PKG archives but appear in
        // pathsStorage). Only register parent dirs — NOT the leaf itself, which
        // would shadow PKG files in the overlay VFS.
        for (i, entry) in db.paths_storage.iter().enumerate() {
            if db.lookup_r2p(entry.self_id).is_some() {
                continue;
            }
            let raw_path = db.reconstruct_path(i, &self_id_index);
            if !raw_path.is_empty() {
                let full_path = format!("/{raw_path}");
                register_path_in_dirs(&full_path, &mut dir_children);
            }
        }

        let dirs = dir_children.into_iter().map(|(k, v)| (k, v.into_iter().collect())).collect();

        (files, dirs)
    }

    /// Number of file entries in this VFS.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Number of directory entries in this VFS.
    pub fn dir_count(&self) -> usize {
        self.dirs.len()
    }

    /// Iterate over all file paths and their sizes (in bytes).
    pub fn files(&self) -> impl Iterator<Item = (&str, usize)> {
        self.files.iter().map(|(path, loc)| (path.as_str(), loc.byte_end - loc.byte_offset))
    }

    /// Iterate over all directory paths.
    pub fn dirs(&self) -> impl Iterator<Item = &str> {
        self.dirs.keys().map(|k| k.as_str())
    }
}

fn lookup_key(path: &str) -> &str {
    if path.is_empty() { "/" } else { path }
}

impl FileSystem for AssetsBinVfs {
    fn read_dir(&self, path: &str) -> vfs::VfsResult<Box<dyn Iterator<Item = String> + Send>> {
        let key = lookup_key(path);
        let children = self.dirs.get(key).ok_or_else(|| vfs::VfsError::from(VfsErrorKind::FileNotFound))?;
        Ok(Box::new(children.clone().into_iter()))
    }

    fn create_dir(&self, _path: &str) -> vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn open_file(&self, path: &str) -> vfs::VfsResult<Box<dyn vfs::SeekAndRead + Send>> {
        let key = lookup_key(path);
        let loc = self.files.get(key).ok_or_else(|| vfs::VfsError::from(VfsErrorKind::FileNotFound))?;
        let data = self.data[loc.byte_offset..loc.byte_end].to_vec();
        Ok(Box::new(Cursor::new(data)))
    }

    fn create_file(&self, _path: &str) -> vfs::VfsResult<Box<dyn vfs::SeekAndWrite + Send>> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn append_file(&self, _path: &str) -> vfs::VfsResult<Box<dyn vfs::SeekAndWrite + Send>> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn metadata(&self, path: &str) -> vfs::VfsResult<VfsMetadata> {
        let key = lookup_key(path);
        if let Some(loc) = self.files.get(key) {
            Ok(VfsMetadata {
                file_type: vfs::VfsFileType::File,
                len: (loc.byte_end - loc.byte_offset) as u64,
                created: None,
                modified: None,
                accessed: None,
            })
        } else if self.dirs.contains_key(key) {
            Ok(VfsMetadata {
                file_type: vfs::VfsFileType::Directory,
                len: 0,
                created: None,
                modified: None,
                accessed: None,
            })
        } else {
            Err(VfsErrorKind::FileNotFound.into())
        }
    }

    fn exists(&self, path: &str) -> vfs::VfsResult<bool> {
        let key = lookup_key(path);
        Ok(self.files.contains_key(key) || self.dirs.contains_key(key))
    }

    fn remove_file(&self, _path: &str) -> vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn remove_dir(&self, _path: &str) -> vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn set_creation_time(&self, _path: &str, _time: std::time::SystemTime) -> vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn set_modification_time(&self, _path: &str, _time: std::time::SystemTime) -> vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn set_access_time(&self, _path: &str, _time: std::time::SystemTime) -> vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn copy_file(&self, _src: &str, _dest: &str) -> vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn move_file(&self, _src: &str, _dest: &str) -> vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn move_dir(&self, _src: &str, _dest: &str) -> vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }
}
