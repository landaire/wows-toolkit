//! VFS abstraction for reading files from World of Warships IDX/PKG archives.
//!
//! Follows the sans-IO pattern: the VFS is generic over a data source `T` that
//! implements [`Prime`] (sync) or [`AsyncPrime`] (async). The VFS itself never
//! performs I/O — it delegates to the source for raw byte access.

use std::collections::HashMap;
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::Range;

use flate2::read::DeflateDecoder;
use vfs::FileSystem;
use vfs::VfsError;
use vfs::VfsMetadata;
use vfs::error::VfsErrorKind;

use crate::data::idx::IdxFile;
use crate::data::idx::VfsEntry;
use crate::data::idx::{
    self,
};

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
    pub volume_filename: String,
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
    entries: HashMap<String, VfsEntryMeta>,
}

impl<T> IdxVfs<T> {
    /// Build a VFS from parsed IDX files and a data source.
    pub fn new(source: T, idx_files: &[IdxFile]) -> Self {
        let tree = idx::build_file_tree(idx_files);
        let entries = build_vfs_entries(&tree);
        Self { source, entries }
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
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }
}

/// Convert the flat `BTreeMap<String, VfsEntry>` from `build_file_tree` into
/// a `HashMap<String, VfsEntryMeta>` with directory children populated.
fn build_vfs_entries(tree: &HashMap<String, VfsEntry>) -> HashMap<String, VfsEntryMeta> {
    let mut entries = HashMap::with_capacity(tree.len());

    // First pass: add all entries
    for (path, entry) in tree {
        let meta = match entry {
            VfsEntry::File { file_info, volume } => VfsEntryMeta::File(VfsFileEntry {
                volume_filename: volume.filename.clone(),
                offset: file_info.offset,
                size: file_info.size,
                unpacked_size: file_info.unpacked_size,
                compression_info: file_info.compression_info,
                crc32: file_info.crc32,
            }),
            VfsEntry::Directory => VfsEntryMeta::Directory { children: Vec::new() },
        };

        entries.insert(path.clone(), meta);
    }

    // Second pass: populate directory children
    // Collect all paths first to avoid borrow issues
    let all_paths: Vec<String> = entries.keys().cloned().collect();
    for path in &all_paths {
        let mut parent_path = match path.rfind('/') {
            Some(pos) => &path[..pos],
            None => "/", // top-level entry, parent is root
        };

        if parent_path.is_empty() {
            parent_path = "/";
        }

        let child_name = match path.rfind('/') {
            Some(pos) => &path[pos + 1..],
            None => path.as_str(),
        };

        // Ensure parent directory exists and add this child
        let parent =
            entries.entry(parent_path.to_string()).or_insert_with(|| VfsEntryMeta::Directory { children: Vec::new() });

        if child_name.is_empty() {
            continue;
        }

        if let VfsEntryMeta::Directory { children } = parent {
            children.push(child_name.to_string());
        }
    }

    // Deduplicate children (can happen with multiple IDX files)
    for entry in entries.values_mut() {
        if let VfsEntryMeta::Directory { children } = entry {
            children.sort();
            children.dedup();
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
