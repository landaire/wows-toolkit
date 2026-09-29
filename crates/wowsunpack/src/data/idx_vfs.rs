//! VFS abstraction for reading files from World of Warships IDX/PKG archives.
//!
//! Follows the sans-IO pattern: the VFS is generic over a data source `T` that
//! implements [`Prime`] (sync) or [`AsyncPrime`] (async). The VFS itself never
//! performs I/O: it delegates to the source for raw byte access.

use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Range;

use std::io::Cursor;

use crate::Rc;

use flate2::read::DeflateDecoder;
use vfs::FileSystem;
use vfs::VfsError;
use vfs::VfsMetadata;
use vfs::error::VfsErrorKind;

use crate::data::idx;
use crate::data::idx::IdxFile;
use crate::data::path_table::PathId;
use crate::data::path_table::PathSet;

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
/// of thousands, so the enum around it is what the entry table pays per slot.
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
        /// The directory's slice of [`IdxVfs::children`], sorted by leaf name.
        /// A range rather than a `Vec` of its own: a build has ~35K
        /// directories, and a `Vec` apiece is 35K allocations and eight more
        /// bytes on every entry, file entries included.
        children: Range<u32>,
    },
}

/// The leaf name of a VFS path, which is what `read_dir` yields.
fn leaf_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(at) => &path[at + 1..],
        None => path,
    }
}

/// The path of the directory holding `path`. A top-level entry belongs to the
/// root, which is named "/" rather than the empty string.
fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(at) => &path[..at],
    }
}

/// A virtual filesystem built from parsed IDX files, backed by PKG volume data.
///
/// Generic over `T`, which provides raw byte access to PKG files via the
/// [`Prime`] trait (or [`AsyncPrime`] for async).
#[derive(Debug)]
pub struct IdxVfs<T> {
    source: T,
    /// Every path in the build, held as one buffer and addressed by [`PathId`].
    paths: PathSet,
    /// Metadata per path, indexed by the path's id.
    entries: Vec<VfsEntryMeta>,
    /// Every directory's children end to end, each directory naming its own
    /// range.
    children: Vec<PathId>,
    /// Distinct volume filenames, indexed by [`VfsFileEntry::volume`].
    volumes: Vec<Rc<str>>,
    /// Distinct `compression_info` values, indexed by
    /// [`VfsFileEntry::compression`].
    compressions: Vec<u64>,
}

impl<T> IdxVfs<T> {
    /// Build a VFS from parsed IDX files and a data source.
    pub fn new(source: T, idx_files: &[IdxFile]) -> Self {
        let Built { paths, entries, children, volumes, compressions } = build_vfs_entries(idx_files);
        Self { source, paths, entries, children, volumes, compressions }
    }

    /// Look up an entry by path.
    pub fn entry_at(&self, path: &str) -> vfs::VfsResult<&VfsEntryMeta> {
        let lookup_key = if path.is_empty() { "/" } else { path };

        match self.paths.id_of(lookup_key) {
            Some(id) => Ok(&self.entries[id.index()]),
            None => Err(VfsError::from(VfsErrorKind::FileNotFound)),
        }
    }

    /// Get the underlying source.
    pub fn source(&self) -> &T {
        &self.source
    }

    /// Iterate over all paths in the VFS.
    pub fn paths(&self) -> impl Iterator<Item = (&str, &VfsEntryMeta)> {
        self.paths.ids().map(|id| (self.paths.get(id), &self.entries[id.index()]))
    }

    /// The leaf names of a directory's children, in listing order.
    ///
    /// Private: a `Directory`'s range only means anything against the VFS that
    /// built it, so an entry is never resolved by a caller that holds one.
    fn children_of<'vfs>(&'vfs self, entry: &VfsEntryMeta) -> impl Iterator<Item = &'vfs str> + use<'vfs, T> {
        let range = match entry {
            VfsEntryMeta::Directory { children } => children.start as usize..children.end as usize,
            VfsEntryMeta::File(_) => 0..0,
        };
        self.children[range].iter().map(|child| leaf_of(self.paths.get(*child)))
    }
}

/// What [`build_vfs_entries`] produces: the paths, the metadata indexed by
/// them, the shared child lists, and the two tables file entries index into.
struct Built {
    paths: PathSet,
    entries: Vec<VfsEntryMeta>,
    children: Vec<PathId>,
    volumes: Vec<Rc<str>>,
    compressions: Vec<u64>,
}

/// Bytes reserved per path up front. Build 13187581 holds 425,482 paths in
/// 23,589,702 bytes, an average of 55.4, so this reaches the final length
/// without repeated reallocation; the slack comes back on the closing
/// `shrink_to_fit`, and a build whose paths run longer costs one doubling
/// rather than a wrong answer.
const RESERVED_BYTES_PER_PATH: usize = 64;

/// Build the VFS entry tables straight from the parsed IDX files.
///
/// Goes directly to `VfsEntryMeta` rather than through `idx::build_file_tree`'s
/// `VfsEntry` map: that intermediate owns a copy of every path and of every
/// file-info record, and a build has hundreds of thousands of each.
fn build_vfs_entries(idx_files: &[IdxFile]) -> Built {
    // One entry per resource plus the root, which is the whole table for every
    // build that has no filename holding a separator.
    let count = idx_files.iter().fold(0, |acc, file| acc + file.resources.len()) + 1;
    let mut paths = PathSet::with_byte_capacity(count, count * RESERVED_BYTES_PER_PATH);
    let mut entries: Vec<VfsEntryMeta> = Vec::with_capacity(count);

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
            None => empty_directory(),
        };

        // A path repeated across IDX files keeps the last record read, which is
        // what the map this replaced did. A path is new exactly when its id is
        // one past the last entry, since every intern here is matched by a push.
        debug_assert_eq!(paths.len(), entries.len(), "one entry per path");
        let id = paths.intern(path);
        if id.index() == entries.len() {
            entries.push(meta);
        } else {
            entries[id.index()] = meta;
        }
    });

    // The root is the one directory with no resource of its own, so
    // `visit_entries` never yields it.
    let _root = add_directory(&mut paths, &mut entries, "/");

    // Each entry paired with the directory that lists it, resolved once here
    // rather than again in `group_children`.
    //
    // A filename holding a separator names a directory that is not itself a
    // resource, so the parent may still be missing; `visit_entries` yields
    // every resource, so every other ancestor is already here. The loop covers
    // the directories it adds, since `id_at` reads the length afresh.
    let mut parented: Vec<(PathId, PathId)> = Vec::with_capacity(count);
    let mut at = 0;
    while let Some(id) = paths.id_at(at) {
        at += 1;
        let path = paths.get(id);
        // The root, and any resource whose filename is empty. Neither names
        // anything a directory listing could show, so neither is listed as a
        // child of one.
        if leaf_of(path).is_empty() {
            continue;
        }
        let parent = parent_of(path);
        if let Some(parent) = paths.id_of(parent) {
            parented.push((parent, id));
            continue;
        }
        let parent = parent.to_string();
        let parent = add_directory(&mut paths, &mut entries, &parent);
        parented.push((parent, id));
    }

    let children = group_children(&paths, &mut entries, parented);

    paths.shrink_to_fit();
    entries.shrink_to_fit();
    volumes.shrink_to_fit();
    compressions.shrink_to_fit();
    Built { paths, entries, children, volumes, compressions }
}

fn empty_directory() -> VfsEntryMeta {
    VfsEntryMeta::Directory { children: 0..0 }
}

/// Adds `path` as a directory if it is not already held, and returns its id.
/// A path that is already an entry keeps the metadata it has.
fn add_directory(paths: &mut PathSet, entries: &mut Vec<VfsEntryMeta>, path: &str) -> PathId {
    debug_assert_eq!(paths.len(), entries.len(), "one entry per path");
    let id = paths.intern(path);
    if id.index() == entries.len() {
        entries.push(empty_directory());
    }
    id
}

/// Lays every directory's children out end to end and points each directory at
/// its own run, sorted by leaf name.
///
/// `parented` is every (directory, entry) pair in the build. A pair whose
/// directory turns out to be a file is dropped: nothing can list it.
fn group_children(paths: &PathSet, entries: &mut [VfsEntryMeta], mut pairs: Vec<(PathId, PathId)>) -> Vec<PathId> {
    pairs.sort_unstable_by_key(|(parent, _)| *parent);

    let mut children: Vec<PathId> = Vec::with_capacity(pairs.len());
    let mut at = 0;
    while at < pairs.len() {
        let parent = pairs[at].0;
        let mut end = at;
        while end < pairs.len() && pairs[end].0 == parent {
            end += 1;
        }
        let run = &mut pairs[at..end];
        at = end;

        let VfsEntryMeta::Directory { children: range } = &mut entries[parent.index()] else {
            continue;
        };

        run.sort_unstable_by(|a, b| leaf_of(paths.get(a.1)).cmp(leaf_of(paths.get(b.1))));
        let first = children.len() as u32;
        children.extend(run.iter().map(|(_, child)| *child));
        *range = first..children.len() as u32;
    }

    children.shrink_to_fit();
    children
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

impl<T> FileSystem for IdxVfs<T>
where
    T: Prime + Debug + Send + Sync + 'static,
{
    fn read_dir(&self, path: &str) -> vfs::VfsResult<Box<dyn Iterator<Item = String> + Send>> {
        let entry = self.entry_at(path)?;
        match entry {
            // The trait yields owned names; the stored children are ids into
            // the shared path buffer, so the copy is made here rather than
            // held per entry.
            VfsEntryMeta::Directory { .. } => {
                let names: Vec<String> = self.children_of(entry).map(str::to_string).collect();
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
        Ok(metadata_of(entry))
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

fn metadata_of(entry: &VfsEntryMeta) -> VfsMetadata {
    match entry {
        VfsEntryMeta::Directory { .. } => VfsMetadata {
            file_type: vfs::VfsFileType::Directory,
            len: 0,
            created: None,
            modified: None,
            accessed: None,
        },
        VfsEntryMeta::File(file) => VfsMetadata {
            file_type: vfs::VfsFileType::File,
            len: file.unpacked_size as u64,
            created: None,
            modified: None,
            accessed: None,
        },
    }
}

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
                VfsEntryMeta::Directory { .. } => {
                    let names: Vec<String> = self.children_of(entry).map(str::to_string).collect();
                    Ok(Box::new(futures::stream::iter(names)))
                }
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

            let volume = self
                .volumes
                .get(file_entry.volume as usize)
                .ok_or_else(|| VfsError::from(VfsErrorKind::Other("unknown volume".into())))?;
            let primed = self.source.prime_volume(volume, data_start..data_end).await?;
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
            Ok(metadata_of(self.entry_at(path)?))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::idx::FileInfo;
    use crate::data::idx::IdxFile;
    use crate::data::idx::PackedFileMetadata;
    use crate::data::idx::ROOT_PARENT_ID;
    use crate::data::idx::Volume;

    /// Serves one volume's bytes, so the `FileSystem` read path can be
    /// exercised without a game install.
    #[derive(Debug)]
    struct TestSource(Vec<u8>);

    impl Prime for TestSource {
        fn prime_volume(&self, _volume: &str, range: Range<usize>) -> Result<impl AsRef<[u8]>, VfsError> {
            Ok(self.0[range].to_vec())
        }
    }

    const VOLUME_ID: u64 = 7;

    fn resource(id: u64, parent_id: u64, filename: &str) -> PackedFileMetadata {
        PackedFileMetadata { resource_ptr: 0, id, parent_id, filename: filename.to_string() }
    }

    fn file_info(resource_id: u64, offset: u64, size: u32) -> FileInfo {
        FileInfo {
            resource_id,
            volume_id: VOLUME_ID,
            offset,
            compression_info: 0,
            size,
            crc32: 0,
            unpacked_size: size,
            padding: 0,
        }
    }

    /// /res (dir), /res/content (dir), /res/content/b.xml, /res/content/a.xml,
    /// /res/readme.txt
    fn fixture() -> IdxFile {
        IdxFile {
            resources: vec![
                resource(1, ROOT_PARENT_ID, "res"),
                resource(2, 1, "content"),
                resource(3, 2, "b.xml"),
                resource(4, 2, "a.xml"),
                resource(5, 1, "readme.txt"),
            ],
            file_infos: vec![file_info(3, 0, 5), file_info(4, 5, 3), file_info(5, 8, 2)],
            volumes: vec![Volume { volume_id: VOLUME_ID, filename: "res.pkg".to_string() }],
        }
    }

    fn vfs(idx_files: &[IdxFile]) -> IdxVfs<TestSource> {
        IdxVfs::new(TestSource(b"bbbbbaaahi".to_vec()), idx_files)
    }

    fn read_dir(vfs: &IdxVfs<TestSource>, path: &str) -> Vec<String> {
        FileSystem::read_dir(vfs, path).expect("the directory lists").collect()
    }

    #[test]
    fn every_resource_and_the_root_become_entries() {
        let vfs = vfs(&[fixture()]);
        let mut paths: Vec<&str> = vfs.paths().map(|(path, _)| path).collect();
        paths.sort_unstable();
        assert_eq!(
            paths,
            vec!["/", "/res", "/res/content", "/res/content/a.xml", "/res/content/b.xml", "/res/readme.txt"]
        );
    }

    #[test]
    fn a_directory_lists_its_own_children_sorted_by_name() {
        let vfs = vfs(&[fixture()]);
        assert_eq!(read_dir(&vfs, "/"), vec!["res"]);
        assert_eq!(read_dir(&vfs, "/res"), vec!["content", "readme.txt"]);
        assert_eq!(read_dir(&vfs, "/res/content"), vec!["a.xml", "b.xml"], "sorted by leaf, not by idx order");
    }

    #[test]
    fn the_empty_path_lists_the_root() {
        let vfs = vfs(&[fixture()]);
        assert_eq!(read_dir(&vfs, ""), read_dir(&vfs, "/"));
    }

    #[test]
    fn a_path_that_is_not_an_entry_is_not_found() {
        let vfs = vfs(&[fixture()]);
        assert!(vfs.entry_at("/res/missing.xml").is_err());
        assert!(!FileSystem::exists(&vfs, "/res/missing.xml").expect("exists answers"));
        assert!(FileSystem::exists(&vfs, "/res/readme.txt").expect("exists answers"));
    }

    #[test]
    fn listing_a_file_is_an_error_and_opening_a_directory_is_too() {
        let vfs = vfs(&[fixture()]);
        assert!(FileSystem::read_dir(&vfs, "/res/readme.txt").is_err());
        assert!(FileSystem::open_file(&vfs, "/res").is_err());
    }

    #[test]
    fn metadata_reports_the_unpacked_size_of_a_file_and_nothing_for_a_directory() {
        let vfs = vfs(&[fixture()]);
        let file = FileSystem::metadata(&vfs, "/res/content/b.xml").expect("the file has metadata");
        assert_eq!(file.file_type, vfs::VfsFileType::File);
        assert_eq!(file.len, 5);
        let dir = FileSystem::metadata(&vfs, "/res/content").expect("the directory has metadata");
        assert_eq!(dir.file_type, vfs::VfsFileType::Directory);
        assert_eq!(dir.len, 0);
    }

    #[test]
    fn a_file_reads_back_the_bytes_its_entry_points_at() {
        use std::io::Read as _;
        let vfs = vfs(&[fixture()]);
        let mut body = String::new();
        FileSystem::open_file(&vfs, "/res/content/a.xml")
            .expect("the file opens")
            .read_to_string(&mut body)
            .expect("the file reads");
        assert_eq!(body, "aaa");
    }

    #[test]
    fn a_filename_holding_a_separator_gets_the_directory_it_names() {
        let idx = IdxFile {
            resources: vec![resource(1, ROOT_PARENT_ID, "res"), resource(2, 1, "spaces/01_map/minimap.png")],
            file_infos: vec![file_info(2, 0, 2)],
            volumes: vec![Volume { volume_id: VOLUME_ID, filename: "res.pkg".to_string() }],
        };
        let vfs = vfs(&[idx]);

        assert_eq!(read_dir(&vfs, "/res"), vec!["spaces"]);
        assert_eq!(read_dir(&vfs, "/res/spaces"), vec!["01_map"]);
        assert_eq!(read_dir(&vfs, "/res/spaces/01_map"), vec!["minimap.png"]);
        assert!(matches!(
            vfs.entry_at("/res/spaces/01_map").expect("the named directory is an entry"),
            VfsEntryMeta::Directory { .. }
        ));
    }

    #[test]
    fn a_path_in_two_idx_files_keeps_the_last_record_and_is_listed_once() {
        let mut second = fixture();
        // The same path, pointing at different bytes in the same volume.
        second.file_infos = vec![file_info(5, 0, 5)];
        let vfs = vfs(&[fixture(), second]);

        assert_eq!(read_dir(&vfs, "/res"), vec!["content", "readme.txt"], "listed once, not twice");
        assert_eq!(
            FileSystem::metadata(&vfs, "/res/readme.txt").expect("the file has metadata").len,
            5,
            "the later record wins"
        );
    }

    #[test]
    fn a_file_at_the_top_level_is_listed_under_the_root() {
        let idx = IdxFile {
            resources: vec![resource(1, ROOT_PARENT_ID, "res"), resource(2, ROOT_PARENT_ID, "readme")],
            file_infos: vec![file_info(2, 0, 2)],
            volumes: vec![Volume { volume_id: VOLUME_ID, filename: "res.pkg".to_string() }],
        };
        let vfs = vfs(&[idx]);

        assert_eq!(read_dir(&vfs, "/"), vec!["readme", "res"]);
        assert_eq!(FileSystem::metadata(&vfs, "/readme").expect("the file has metadata").len, 2);
    }

    #[test]
    fn a_child_of_a_file_is_reachable_by_path_but_listed_by_nothing() {
        // A resource that carries a file_info and is also someone's parent.
        let idx = IdxFile {
            resources: vec![resource(1, ROOT_PARENT_ID, "res"), resource(2, 1, "bundle"), resource(3, 2, "inside.xml")],
            file_infos: vec![file_info(2, 0, 5), file_info(3, 5, 3)],
            volumes: vec![Volume { volume_id: VOLUME_ID, filename: "res.pkg".to_string() }],
        };
        let vfs = vfs(&[idx]);

        assert_eq!(read_dir(&vfs, "/res"), vec!["bundle"]);
        assert!(FileSystem::read_dir(&vfs, "/res/bundle").is_err(), "a file lists nothing");
        assert!(vfs.entry_at("/res/bundle/inside.xml").is_ok(), "the child is still an entry");
    }

    #[test]
    fn an_index_with_no_resources_still_has_a_root_that_lists_nothing() {
        let idx = IdxFile { resources: Vec::new(), file_infos: Vec::new(), volumes: Vec::new() };
        let vfs = vfs(&[idx]);
        assert_eq!(read_dir(&vfs, "/"), Vec::<String>::new());
        assert_eq!(vfs.paths().count(), 1);
    }
}
