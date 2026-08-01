//! VFS abstraction for reading files from World of Warships IDX/PKG archives.
//!
//! Follows the sans-IO pattern: the VFS is generic over a data source `T` that
//! implements [`Prime`] (sync) or [`AsyncPrime`] (async). The VFS itself never
//! performs I/O — it delegates to the source for raw byte access.

use std::collections::HashMap;
use std::fmt::Debug;

use rustc_hash::FxBuildHasher;
use rustc_hash::FxHashMap;
use std::io::Cursor;
use std::ops::Range;

use crate::Rc;

use flate2::read::DeflateDecoder;
use vfs::FileSystem;
use vfs::VfsError;
use vfs::VfsMetadata;
use vfs::error::VfsErrorKind;

use crate::data::idx;
use crate::data::idx::IdxFile;

/// Trait for providing raw byte access to PKG volume data (sync).
///
/// Implementors must be able to read a byte range from a named volume file.
pub trait Prime {
    fn prime_volume(&self, volume: &str, range: Range<usize>) -> Result<impl AsRef<[u8]>, VfsError>;
}

/// Trait for providing raw byte access to PKG volume data (async).
#[cfg(feature = "async_vfs")]
#[async_trait::async_trait]
pub trait AsyncPrime {
    async fn prime_volume(&self, volume: &str, range: Range<usize>) -> Result<impl AsRef<[u8]>, VfsError>;
}

/// File metadata stored in the VFS for each file entry.
#[derive(Debug, Clone)]
pub struct VfsFileEntry {
    pub volume_filename: Rc<str>,
    pub offset: u64,
    pub size: u32,
    pub unpacked_size: u32,
    pub compression_info: u64,
    pub crc32: u32,
}

/// Entry metadata for any node (file or directory).
#[derive(Debug, Clone)]
pub enum VfsEntryMeta {
    File(VfsFileEntry),
    Directory {
        /// Names of immediate children.
        children: Vec<String>,
    },
}

/// A virtual filesystem built from parsed IDX files, backed by PKG volume data.
///
/// Generic over `T`, which provides raw byte access to PKG files via the
/// [`Prime`] trait (or [`AsyncPrime`] for async).
#[derive(Debug)]
pub struct IdxVfs<T> {
    source: T,
    entries: FxHashMap<Rc<str>, VfsEntryMeta>,
}

impl<T> IdxVfs<T> {
    /// Build a VFS from parsed IDX files and a data source.
    pub fn new(source: T, idx_files: &[IdxFile]) -> Self {
        Self { source, entries: build_vfs_entries(idx_files) }
    }

    /// Look up an entry by path.
    pub fn entry_at(&self, path: &str) -> vfs::VfsResult<&VfsEntryMeta> {
        let lookup_key = if path.is_empty() { "/" } else { path };

        self.entries.get(lookup_key).ok_or_else(|| VfsError::from(VfsErrorKind::FileNotFound))
    }

    /// Get the underlying source.
    pub fn source(&self) -> &T {
        &self.source
    }

    /// Iterate over all paths in the VFS.
    pub fn paths(&self) -> impl Iterator<Item = (&str, &VfsEntryMeta)> {
        self.entries.iter().map(|(k, v)| (&**k, v))
    }
}

/// Build the VFS entry map straight from the parsed IDX files.
///
/// Goes directly to `VfsEntryMeta` rather than through `idx::build_file_tree`'s
/// `VfsEntry` map: that intermediate owns a copy of every path and of every
/// file-info record, and a build has hundreds of thousands of each.
fn build_vfs_entries(idx_files: &[IdxFile]) -> FxHashMap<Rc<str>, VfsEntryMeta> {
    let count = idx_files.iter().fold(0, |acc, file| acc + file.resources.len());
    let mut entries: FxHashMap<Rc<str>, VfsEntryMeta> = FxHashMap::with_capacity_and_hasher(count, FxBuildHasher);

    // Volume filenames repeat across hundreds of thousands of files but only a
    // couple hundred are distinct; intern them so every file entry shares one
    // refcounted handle instead of owning a duplicate String.
    let mut volume_names: HashMap<&str, Rc<str>> = HashMap::default();

    // First pass: add all entries
    idx::visit_entries(idx_files, |path, file| {
        let meta = match file {
            Some(file) => {
                let volume_filename = volume_names
                    .entry(file.volume.filename.as_str())
                    .or_insert_with(|| Rc::from(file.volume.filename.as_str()))
                    .clone();
                VfsEntryMeta::File(VfsFileEntry {
                    volume_filename,
                    offset: file.file_info.offset,
                    size: file.file_info.size,
                    unpacked_size: file.file_info.unpacked_size,
                    compression_info: file.file_info.compression_info,
                    crc32: file.file_info.crc32,
                })
            }
            None => VfsEntryMeta::Directory { children: Vec::new() },
        };

        entries.insert(Rc::clone(path), meta);
    });

    // The root is the one directory with no resource of its own, so
    // `visit_entries` never yields it.
    entries.entry(Rc::from("/")).or_insert_with(|| VfsEntryMeta::Directory { children: Vec::new() });

    // Second pass: populate directory children. Snapshot the keys first, since
    // the loop inserts into `entries`; the handles are refcounted, so the
    // snapshot costs pointers rather than copies of every path.
    let paths: Vec<Rc<str>> = entries.keys().map(Rc::clone).collect();
    for path in &paths {
        let mut parent_path = match path.rfind('/') {
            Some(pos) => &path[..pos],
            None => "/", // top-level entry, parent is root
        };

        if parent_path.is_empty() {
            parent_path = "/";
        }

        let child_name = match path.rfind('/') {
            Some(pos) => &path[pos + 1..],
            None => &**path,
        };

        if child_name.is_empty() {
            continue;
        }

        // Every ancestor of a resource is itself a resource, so the parent is
        // already present and this is a lookup rather than an insert; the
        // fallback covers a parent that somehow is not.
        let parent = match entries.get_mut(parent_path) {
            Some(parent) => parent,
            None => {
                entries.entry(Rc::from(parent_path)).or_insert_with(|| VfsEntryMeta::Directory { children: Vec::new() })
            }
        };

        if let VfsEntryMeta::Directory { children } = parent {
            children.push(child_name.to_string());
        }
    }

    // Deduplicate children (can happen with multiple IDX files). The lists are
    // frozen after this, so release the doubling/dedup slack: ~34K directory
    // Vecs each grown by amortized doubling and then shortened by dedup.
    for entry in entries.values_mut() {
        if let VfsEntryMeta::Directory { children } = entry {
            children.sort();
            children.dedup();
            children.shrink_to_fit();
        }
    }

    entries
}

// --- vfs::FileSystem implementation ---

impl<T> FileSystem for IdxVfs<T>
where
    T: Prime + Debug + Send + Sync + 'static,
{
    fn read_dir(&self, path: &str) -> vfs::VfsResult<Box<dyn Iterator<Item = String> + Send>> {
        let entry = self.entry_at(path)?;
        match entry {
            VfsEntryMeta::Directory { children } => Ok(Box::new(children.clone().into_iter())),
            VfsEntryMeta::File(_) => Err(VfsError::from(VfsErrorKind::Other("not a directory".into()))),
        }
    }

    fn create_dir(&self, _path: &str) -> vfs::VfsResult<()> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn open_file(&self, path: &str) -> vfs::VfsResult<Box<dyn vfs::SeekAndRead + Send>> {
        let entry = self.entry_at(path)?;
        let VfsEntryMeta::File(file_entry) = entry else {
            return Err(VfsError::from(VfsErrorKind::Other("not a file".into())));
        };

        let data_start = file_entry.offset as usize;
        let data_end = data_start + file_entry.size as usize;

        let primed = self.source.prime_volume(&file_entry.volume_filename, data_start..data_end)?;
        let source_bytes: &[u8] = primed.as_ref();

        if file_entry.compression_info != 0 {
            let mut data = Vec::with_capacity(file_entry.unpacked_size as usize);
            let mut decoder = DeflateDecoder::new(source_bytes);
            std::io::copy(&mut decoder, &mut data).map_err(|e| VfsError::from(VfsErrorKind::IoError(e)))?;
            Ok(Box::new(Cursor::new(data)))
        } else {
            let data = source_bytes.to_vec();
            Ok(Box::new(Cursor::new(data)))
        }
    }

    fn create_file(&self, _path: &str) -> vfs::VfsResult<Box<dyn vfs::SeekAndWrite + Send>> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn append_file(&self, _path: &str) -> vfs::VfsResult<Box<dyn vfs::SeekAndWrite + Send>> {
        Err(VfsErrorKind::NotSupported.into())
    }

    fn metadata(&self, path: &str) -> vfs::VfsResult<VfsMetadata> {
        let entry = self.entry_at(path)?;
        let meta = match entry {
            VfsEntryMeta::Directory { .. } => VfsMetadata {
                file_type: vfs::VfsFileType::Directory,
                len: 0,
                created: None,
                modified: None,
                accessed: None,
            },
            VfsEntryMeta::File(f) => VfsMetadata {
                file_type: vfs::VfsFileType::File,
                len: f.unpacked_size as u64,
                created: None,
                modified: None,
                accessed: None,
            },
        };
        Ok(meta)
    }

    fn exists(&self, path: &str) -> vfs::VfsResult<bool> {
        Ok(self.entry_at(path).is_ok())
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

// --- Async VFS implementation ---

#[cfg(feature = "async_vfs")]
mod async_impl {
    use super::*;
    use async_trait::async_trait;
    use vfs::async_vfs::AsyncFileSystem;
    use vfs::async_vfs::SeekAndRead;

    #[async_trait]
    impl<T> AsyncFileSystem for IdxVfs<T>
    where
        T: AsyncPrime + Debug + Send + Sync + 'static,
    {
        async fn read_dir(&self, path: &str) -> vfs::VfsResult<Box<dyn Unpin + futures::Stream<Item = String> + Send>> {
            let entry = self.entry_at(path)?;
            match entry {
                VfsEntryMeta::Directory { children } => Ok(Box::new(futures::stream::iter(children.clone()))),
                VfsEntryMeta::File(_) => Err(VfsError::from(VfsErrorKind::Other("not a directory".into()))),
            }
        }

        async fn create_dir(&self, _path: &str) -> vfs::VfsResult<()> {
            Err(VfsErrorKind::NotSupported.into())
        }

        async fn open_file(&self, path: &str) -> vfs::VfsResult<Box<dyn SeekAndRead + Send + Unpin>> {
            let entry = self.entry_at(path)?;
            let VfsEntryMeta::File(file_entry) = entry else {
                return Err(VfsError::from(VfsErrorKind::Other("not a file".into())));
            };

            let data_start = file_entry.offset as usize;
            let data_end = data_start + file_entry.size as usize;

            let primed = self.source.prime_volume(&file_entry.volume_filename, data_start..data_end).await?;
            let source_bytes: &[u8] = primed.as_ref();

            if file_entry.compression_info != 0 {
                let mut data = Vec::with_capacity(file_entry.unpacked_size as usize);
                let mut decoder = DeflateDecoder::new(source_bytes);
                std::io::copy(&mut decoder, &mut data).map_err(|e| VfsError::from(VfsErrorKind::IoError(e)))?;
                Ok(Box::new(async_std::io::Cursor::new(data)))
            } else {
                let data = source_bytes.to_vec();
                Ok(Box::new(async_std::io::Cursor::new(data)))
            }
        }

        async fn create_file(&self, _path: &str) -> vfs::VfsResult<Box<dyn async_std::io::Write + Send + Unpin>> {
            Err(VfsErrorKind::NotSupported.into())
        }

        async fn append_file(&self, _path: &str) -> vfs::VfsResult<Box<dyn async_std::io::Write + Send + Unpin>> {
            Err(VfsErrorKind::NotSupported.into())
        }

        async fn metadata(&self, path: &str) -> vfs::VfsResult<VfsMetadata> {
            let entry = self.entry_at(path)?;
            let meta = match entry {
                VfsEntryMeta::Directory { .. } => VfsMetadata {
                    file_type: vfs::VfsFileType::Directory,
                    len: 0,
                    created: None,
                    modified: None,
                    accessed: None,
                },
                VfsEntryMeta::File(f) => VfsMetadata {
                    file_type: vfs::VfsFileType::File,
                    len: f.unpacked_size as u64,
                    created: None,
                    modified: None,
                    accessed: None,
                },
            };
            Ok(meta)
        }

        async fn exists(&self, path: &str) -> vfs::VfsResult<bool> {
            Ok(self.entry_at(path).is_ok())
        }

        async fn remove_file(&self, _path: &str) -> vfs::VfsResult<()> {
            Err(VfsErrorKind::NotSupported.into())
        }

        async fn remove_dir(&self, _path: &str) -> vfs::VfsResult<()> {
            Err(VfsErrorKind::NotSupported.into())
        }

        async fn set_creation_time(&self, _path: &str, _time: std::time::SystemTime) -> vfs::VfsResult<()> {
            Err(VfsErrorKind::NotSupported.into())
        }

        async fn set_modification_time(&self, _path: &str, _time: std::time::SystemTime) -> vfs::VfsResult<()> {
            Err(VfsErrorKind::NotSupported.into())
        }

        async fn set_access_time(&self, _path: &str, _time: std::time::SystemTime) -> vfs::VfsResult<()> {
            Err(VfsErrorKind::NotSupported.into())
        }

        async fn copy_file(&self, _src: &str, _dest: &str) -> vfs::VfsResult<()> {
            Err(VfsErrorKind::NotSupported.into())
        }

        async fn move_file(&self, _src: &str, _dest: &str) -> vfs::VfsResult<()> {
            Err(VfsErrorKind::NotSupported.into())
        }

        async fn move_dir(&self, _src: &str, _dest: &str) -> vfs::VfsResult<()> {
            Err(VfsErrorKind::NotSupported.into())
        }
    }
}
