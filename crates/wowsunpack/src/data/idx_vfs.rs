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
///
/// Sized deliberately: one of these is held per entry and a build has hundreds
/// of thousands, so the enum around it is what the entry map pays per slot.
/// `volume` indexes [`IdxVfs::volumes`] rather than holding a refcounted name,
/// since a build has a couple of hundred distinct volumes and 16 bytes of
/// pointer per entry is 6.5 MiB of them.
#[derive(Debug, Clone)]
pub struct VfsFileEntry {
    pub offset: u64,
    pub size: u32,
    pub unpacked_size: u32,
    pub crc32: u32,
    pub volume: u16,
    /// Indexes [`IdxVfs::compressions`]. The idx records a 64-bit
    /// `compression_info` whose distinct values number in the single digits per
    /// build (build 13187581 uses two: 0 and 0x1_0000_0005), so it is interned
    /// rather than narrowed: the field is structured, not a flag, and a build
    /// old enough to use another method must not read as one this understands.
    pub compression: u8,
}

/// Entry metadata for any node (file or directory).
#[derive(Debug, Clone)]
pub enum VfsEntryMeta {
    File(VfsFileEntry),
    Directory {
        /// The entry map's own keys for the immediate children, shared with
        /// it rather than a second copy of every leaf name.
        children: Vec<Rc<str>>,
    },
}

/// The leaf name of a VFS path, which is what `read_dir` yields.
fn leaf_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(at) => &path[at + 1..],
        None => path,
    }
}

/// A virtual filesystem built from parsed IDX files, backed by PKG volume data.
///
/// Generic over `T`, which provides raw byte access to PKG files via the
/// [`Prime`] trait (or [`AsyncPrime`] for async).
#[derive(Debug)]
pub struct IdxVfs<T> {
    source: T,
    entries: FxHashMap<Rc<str>, VfsEntryMeta>,
    /// Distinct volume filenames, indexed by [`VfsFileEntry::volume`].
    volumes: Vec<Rc<str>>,
    /// Distinct `compression_info` values, indexed by
    /// [`VfsFileEntry::compression`].
    compressions: Vec<u64>,
}

impl<T> IdxVfs<T> {
    /// Build a VFS from parsed IDX files and a data source.
    pub fn new(source: T, idx_files: &[IdxFile]) -> Self {
        let Built { entries, volumes, compressions } = build_vfs_entries(idx_files);
        Self { source, entries, volumes, compressions }
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
/// What [`build_vfs_entries`] produces: the entry map and the two tables its
/// file entries index into.
struct Built {
    entries: FxHashMap<Rc<str>, VfsEntryMeta>,
    volumes: Vec<Rc<str>>,
    compressions: Vec<u64>,
}

fn build_vfs_entries(idx_files: &[IdxFile]) -> Built {
    let count = idx_files.iter().fold(0, |acc, file| acc + file.resources.len());
    let mut entries: FxHashMap<Rc<str>, VfsEntryMeta> = FxHashMap::with_capacity_and_hasher(count, FxBuildHasher);

    // Volume filenames repeat across hundreds of thousands of files but only a
    // couple hundred are distinct; every file entry indexes this table rather
    // than holding a handle of its own.
    let mut volumes: Vec<Rc<str>> = Vec::new();
    let mut volume_index: HashMap<&str, u16> = HashMap::default();
    let mut compressions: Vec<u64> = Vec::new();

    idx::visit_entries(idx_files, |path, file| {
        let meta = match file {
            Some(file) => {
                let name = file.volume.filename.as_str();
                let volume = match volume_index.get(name) {
                    Some(at) => *at,
                    None => {
                        let at = u16::try_from(volumes.len()).expect("a build has far fewer than 65536 volumes");
                        volumes.push(Rc::from(name));
                        volume_index.insert(name, at);
                        at
                    }
                };
                VfsEntryMeta::File(VfsFileEntry {
                    offset: file.file_info.offset,
                    size: file.file_info.size,
                    unpacked_size: file.file_info.unpacked_size,
                    crc32: file.file_info.crc32,
                    volume,
                    compression: intern_compression(&mut compressions, file.file_info.compression_info),
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

        if leaf_of(path).is_empty() {
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
            children.push(Rc::clone(path));
        }
    }

    // Deduplicate children (can happen with multiple IDX files). The lists are
    // frozen after this, so release the doubling/dedup slack: ~34K directory
    // Vecs each grown by amortized doubling and then shortened by dedup.
    for entry in entries.values_mut() {
        if let VfsEntryMeta::Directory { children } = entry {
            children.sort_by(|a, b| leaf_of(a).cmp(leaf_of(b)));
            children.dedup_by(|a, b| leaf_of(a) == leaf_of(b));
            children.shrink_to_fit();
        }
    }

    volumes.shrink_to_fit();
    compressions.shrink_to_fit();
    Built { entries, volumes, compressions }
}

/// The index of `info` in `table`, appending it if it is new.
///
/// Linear because the table holds a handful of values; a build that somehow
/// carried more than 256 distinct ones would be a format change, and is
/// rejected rather than silently folded onto another value.
fn intern_compression(table: &mut Vec<u64>, info: u64) -> u8 {
    if let Some(at) = table.iter().position(|held| *held == info) {
        return at as u8;
    }
    let at = u8::try_from(table.len()).expect("a build uses a handful of distinct compression methods");
    table.push(info);
    at
}

// --- vfs::FileSystem implementation ---

impl<T> FileSystem for IdxVfs<T>
where
    T: Prime + Debug + Send + Sync + 'static,
{
    fn read_dir(&self, path: &str) -> vfs::VfsResult<Box<dyn Iterator<Item = String> + Send>> {
        let entry = self.entry_at(path)?;
        match entry {
            // The trait yields owned names; the stored children are the map's
            // own keys, so the copy is made here rather than held per entry.
            VfsEntryMeta::Directory { children } => {
                let names: Vec<String> = children.iter().map(|child| leaf_of(child).to_string()).collect();
                Ok(Box::new(names.into_iter()))
            }
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

        let volume = self
            .volumes
            .get(file_entry.volume as usize)
            .ok_or_else(|| VfsError::from(VfsErrorKind::Other("unknown volume".into())))?;
        let primed = self.source.prime_volume(volume, data_start..data_end)?;
        let source_bytes: &[u8] = primed.as_ref();

        let compression = self
            .compressions
            .get(file_entry.compression as usize)
            .copied()
            .ok_or_else(|| VfsError::from(VfsErrorKind::Other("unknown compression".into())))?;
        if compression != 0 {
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
