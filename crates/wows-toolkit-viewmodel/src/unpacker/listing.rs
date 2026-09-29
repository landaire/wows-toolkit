//! The Unpacker's VFS listing: the folder tree, the flat file list the filter
//! runs over, what the listing shows for a directory, and how a file's name
//! decides its icon, type label and viewer.
//!
//! Pure functions over a `VfsPath`, shared by both front ends so the listing
//! rules have one implementation, and unit-testable against an in-memory
//! filesystem rather than a game install.

use wowsunpack::data::path_table::PathId;
use wowsunpack::data::path_table::PathTable;
use wowsunpack::vfs::VfsFileType;
use wowsunpack::vfs::VfsPath;

/// One directory in the folder-tree sidebar. Built once when the VFS loads:
/// re-walking `read_dir` per frame is what makes the listing slow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderTreeNode {
    pub name: String,
    /// Absolute, leading-slash path, the key the listing selects by.
    pub path: String,
    pub children: Vec<FolderTreeNode>,
}

/// One file in a [`FileList`], by position.
///
/// A position in the list that issued it. Distinct from the `PathId` the list
/// keeps internally so the two cannot be crossed, but an index and the list it
/// came from still travel together: read against a different list it names a
/// different file. Both front ends hold the pair rather than the index alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileIndex(u32);

/// Every file in a VFS, with the root that opens them.
///
/// The paths live in one buffer rather than a `PathBuf` and a `VfsPath` apiece:
/// a full install is ~390K files, and a handle per file is three copies of
/// every path across more than a million allocations. A `VfsPath` is built for
/// the file actually being opened.
///
/// Not `Clone`: the whole point is that one of these is shared by handle.
pub struct FileList {
    root: VfsPath,
    /// Paths as walked, each relative to `root` and starting with a slash.
    paths: PathTable,
    /// Byte size per path, read from the same metadata that decided the entry
    /// was a file. Kept rather than re-read, so drawing a row costs no VFS
    /// lookup: under a physical-directory VFS that lookup is a `stat`.
    /// Absent for an entry the VFS lists but has no metadata for.
    sizes: Vec<Option<u64>>,
}

/// Prints what the list holds rather than its contents: it is carried by
/// events that derive `Debug`, and the arena is tens of megabytes.
impl std::fmt::Debug for FileList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileList").field("root", &self.root.as_str()).field("files", &self.len()).finish()
    }
}

impl FileList {
    pub fn new(root: VfsPath) -> Self {
        Self { root, paths: PathTable::default(), sizes: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// The path as walked: relative to the list's root, starting with a slash.
    /// Also what a filter matches and what a filter-result row is labelled by.
    ///
    /// Panics past the end of the list. An index from another list of at least
    /// this length names whatever this one holds at that position.
    pub fn path(&self, index: FileIndex) -> &str {
        self.paths.get(self.id(index))
    }

    /// The file's byte size as recorded when the list was walked, absent when
    /// the VFS had no metadata for it.
    pub fn size(&self, index: FileIndex) -> Option<u64> {
        self.sizes[index.0 as usize]
    }

    pub fn indices(&self) -> impl ExactSizeIterator<Item = FileIndex> + use<> {
        (0..self.paths.len() as u32).map(FileIndex)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (FileIndex, &str)> {
        self.indices().map(move |index| (index, self.path(index)))
    }

    fn id(&self, index: FileIndex) -> PathId {
        self.paths.id_at(index.0 as usize).expect("a file index issued by this list")
    }

    /// The handle that opens the file, built on demand.
    ///
    /// `None` when the root will not form the path. [`build_file_list`] only
    /// takes names that `join` reproduces, so for a list it built this means
    /// the VFS has changed under the list.
    pub fn vfs_path(&self, index: FileIndex) -> Option<VfsPath> {
        self.root.join(self.path(index).trim_start_matches('/')).ok()
    }

    /// One listing row, labelled by its whole path the way a filter result is.
    pub fn row(&self, index: FileIndex) -> Option<ListingEntry> {
        Some(ListingEntry {
            label: self.path(index).to_string(),
            is_dir: false,
            size: self.size(index),
            path: self.vfs_path(index)?,
        })
    }

    fn push(&mut self, path: &str, size: Option<u64>) -> FileIndex {
        self.sizes.push(size);
        FileIndex(self.paths.push(path).index() as u32)
    }

    fn shrink_to_fit(&mut self) {
        self.paths.shrink_to_fit();
        self.sizes.shrink_to_fit();
    }
}

/// Whether an entry named `name` can be addressed through the VFS.
///
/// `VfsPath::join` drops an empty component and a ".", and resolves a "..",
/// so a name like that would open something other than the entry it was
/// walked from, or nothing at all. Such an entry cannot be opened, viewed or
/// extracted whatever the listing shows, so it is left out.
fn is_addressable(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".."
}

/// The path of the VFS root, and what an unset selection means.
pub const ROOT_PATH: &str = "/";

/// Filter text shorter than this lists the selected directory instead of
/// searching. One or two characters match most of an install, which costs a
/// full-list pass to produce a result nobody wanted.
pub const MIN_FILTER_LEN: usize = 3;

/// Whether `filter` is long enough to filter by.
pub fn is_filtering(filter: &str) -> bool {
    filter.len() >= MIN_FILTER_LEN
}

/// Which viewer a file opens in, decided by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Image,
    Plaintext,
    /// No in-app viewer; the file can still be queued for extraction.
    Opaque,
}

const IMAGE_FILE_TYPES: [&str; 3] = [".jpg", ".png", ".svg"];
const PLAINTEXT_FILE_TYPES: [&str; 3] = [".xml", ".json", ".txt"];

impl FileKind {
    /// Case-sensitive suffix checks, including the dot.
    pub fn of(file_name: &str) -> Self {
        if IMAGE_FILE_TYPES.iter().any(|ext| file_name.ends_with(ext)) {
            Self::Image
        } else if PLAINTEXT_FILE_TYPES.iter().any(|ext| file_name.ends_with(ext)) {
            Self::Plaintext
        } else {
            Self::Opaque
        }
    }
}

/// Short type name for the listing's Type column.
pub fn file_type_label(name: &str) -> &'static str {
    let Some(dot) = name.rfind('.') else {
        return "File";
    };
    match &name[dot..] {
        ".xml" => "XML",
        ".json" => "JSON",
        ".txt" => "Text",
        ".png" => "PNG",
        ".jpg" | ".jpeg" => "JPEG",
        ".svg" => "SVG",
        ".dds" => "DDS",
        ".pvr" => "PVR",
        ".model" => "Model",
        ".visual" => "Visual",
        ".primitives" | ".primitives_processed" => "Mesh",
        ".wotreplay" | ".wowsreplay" => "Replay",
        ".mp3" | ".ogg" | ".wav" | ".wem" => "Audio",
        ".fev" | ".fsb" => "FMOD",
        ".ttf" | ".otf" => "Font",
        ".py" | ".pyc" => "Python",
        ".fx" | ".hlsl" | ".glsl" => "Shader",
        ".atlas" => "Atlas",
        ".settings" => "Settings",
        ".def" => "Def",
        _ => "File",
    }
}

/// One row of the file listing.
#[derive(Debug, Clone)]
pub struct ListingEntry {
    /// What the row shows: a bare name when listing a directory, the whole
    /// path when showing filter results.
    pub label: String,
    pub is_dir: bool,
    /// Byte size, absent for a directory or an entry whose metadata failed.
    pub size: Option<u64>,
    pub path: VfsPath,
}

impl ListingEntry {
    pub fn kind(&self) -> FileKind {
        FileKind::of(&self.label)
    }

    pub fn type_label(&self) -> &'static str {
        if self.is_dir { "Folder" } else { file_type_label(&self.label) }
    }
}

/// Walks `dir` into the sidebar's directory tree. `path_prefix` is the
/// absolute path of `dir` itself, empty at the root so children come out as
/// "/res" rather than "res".
///
/// An unreadable directory yields no children rather than an error: the tree
/// is navigation, and one bad node should not blank the sidebar.
pub fn build_folder_tree(dir: &VfsPath, path_prefix: &str) -> Vec<FolderTreeNode> {
    let Ok(entries) = dir.read_dir() else {
        return Vec::new();
    };
    let mut dirs: Vec<VfsPath> = entries.filter(|entry| entry.is_dir().unwrap_or(false)).collect();
    dirs.sort_by_key(|dir| dir.filename());

    dirs.iter()
        .map(|child| {
            let name = child.filename();
            let path = if path_prefix.is_empty() { format!("/{name}") } else { format!("{path_prefix}/{name}") };
            let children = build_folder_tree(child, &path);
            FolderTreeNode { name, path, children }
        })
        .collect()
}

/// Walks `root` into the flat list the filter runs over.
///
/// Built lazily on first browser open, and the browser is its only consumer.
/// Paths carry a leading slash so they match `build_folder_tree`'s keys, and
/// only files are listed: a directory is reached through the folder tree.
///
/// Paths are relative to `root`, so a list walked from somewhere other than a
/// VFS root labels its rows from there. Both front ends pass a root.
///
/// The path buffer grows by doubling and is shrunk once at the end: a VFS
/// states no file count, so there is nothing to size it from up front.
pub fn build_file_list(root: &VfsPath) -> FileList {
    // One buffer for the path being walked, extended and truncated per level,
    // rather than a fresh string per entry across the whole install.
    fn collect(dir: &VfsPath, path: &mut String, out: &mut FileList) {
        let Ok(entries) = dir.read_dir() else {
            return;
        };
        for entry in entries {
            let name = entry.filename();
            if !is_addressable(&name) {
                continue;
            }
            // One metadata read decides what the entry is and, for a file, how
            // big it is. `is_dir` would read it too, behind an `exists` that
            // reads it again.
            let meta = entry.metadata();

            let parent_end = path.len();
            path.push('/');
            path.push_str(&name);
            match meta {
                Ok(meta) if meta.file_type == VfsFileType::Directory => collect(&entry, path, out),
                Ok(meta) => {
                    out.push(path, Some(meta.len));
                }
                // An entry the VFS lists but has no metadata for. assets.bin
                // registers names from its path storage that carry no data,
                // and both browsers have always listed those, so they stay in
                // the listing with no size rather than disappearing from it.
                Err(_) => {
                    out.push(path, None);
                }
            }
            path.truncate(parent_end);
        }
    }
    let mut out = FileList::new(root.clone());
    collect(root, &mut String::new(), &mut out);
    out.shrink_to_fit();
    out
}

/// Which files a path filter matches, as positions in `files`.
///
/// A filter containing a star is treated as a glob and matched against the
/// whole path; anything else is a plain substring match. A star filter that
/// does not parse as a glob falls back to the substring match, so a
/// half-typed pattern keeps listing results instead of emptying the pane.
///
/// The glob runs with its default options, so a star spans path separators:
/// "*.png" matches a file at any depth, not only at the root.
pub fn filter_files(files: &FileList, filter: &str) -> Vec<FileIndex> {
    let glob = filter.contains('*').then(|| glob::Pattern::new(filter).ok()).flatten();
    match glob {
        Some(glob) => files.iter().filter(|(_, path)| glob.matches(path)).map(|(index, _)| index).collect(),
        None => files.iter().filter(|(_, path)| path.contains(filter)).map(|(index, _)| index).collect(),
    }
}

/// The entries of one directory, directories first and then files, each group
/// ordered case-insensitively by name.
///
/// An unreadable directory lists nothing rather than failing: the sidebar can
/// offer a path the VFS will not open.
pub fn directory_entries(root: &VfsPath, dir_path: &str) -> Vec<ListingEntry> {
    let target = if dir_path.is_empty() || dir_path == ROOT_PATH {
        root.clone()
    } else {
        match root.join(dir_path.trim_start_matches('/')) {
            Ok(path) => path,
            Err(_) => return Vec::new(),
        }
    };
    let Ok(entries) = target.read_dir() else {
        return Vec::new();
    };

    let mut rows: Vec<ListingEntry> = entries
        .map(|entry| {
            let is_dir = entry.is_dir().unwrap_or(false);
            ListingEntry {
                label: entry.filename(),
                is_dir,
                size: (!is_dir).then(|| entry.metadata().ok().map(|meta| meta.len)).flatten(),
                path: entry,
            }
        })
        .collect();

    rows.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase())));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use wowsunpack::vfs::MemoryFS;

    /// /res/content/a.xml, /res/content/b.png, /res/readme.txt, /res/Zebra.txt
    fn fixture() -> VfsPath {
        let root: VfsPath = MemoryFS::new().into();
        root.join("res/content").unwrap().create_dir_all().unwrap();
        for (path, body) in [
            ("res/content/a.xml", "<a/>"),
            ("res/content/b.png", "png"),
            ("res/readme.txt", "hi"),
            ("res/Zebra.txt", "z"),
        ] {
            root.join(path).unwrap().create_file().unwrap().write_all(body.as_bytes()).unwrap();
        }
        root
    }

    fn sorted_paths(files: &FileList) -> Vec<String> {
        let mut paths: Vec<String> = files.iter().map(|(_, path)| path.to_string()).collect();
        paths.sort();
        paths
    }

    fn sorted_matches(files: &FileList, filter: &str) -> Vec<String> {
        let mut paths: Vec<String> =
            filter_files(files, filter).into_iter().map(|index| files.path(index).to_string()).collect();
        paths.sort();
        paths
    }

    #[test]
    fn folder_tree_is_directories_only_sorted_with_absolute_paths() {
        let tree = build_folder_tree(&fixture(), "");
        assert_eq!(tree.len(), 1, "only /res is a top-level directory");
        assert_eq!(tree[0].path, "/res");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].path, "/res/content");
    }

    #[test]
    fn folder_tree_of_an_unreadable_directory_is_empty_rather_than_an_error() {
        assert!(build_folder_tree(&fixture().join("does/not/exist").unwrap(), "").is_empty());
    }

    #[test]
    fn file_list_is_files_only_with_leading_slash_paths() {
        assert_eq!(
            sorted_paths(&build_file_list(&fixture())),
            vec!["/res/Zebra.txt", "/res/content/a.xml", "/res/content/b.png", "/res/readme.txt"]
        );
    }

    #[test]
    fn a_filter_shorter_than_three_characters_does_not_filter() {
        assert!(!is_filtering(""));
        assert!(!is_filtering("a"));
        assert!(!is_filtering("ab"));
        assert!(is_filtering("abc"));
    }

    #[test]
    fn a_plain_filter_matches_anywhere_in_the_path() {
        let files = build_file_list(&fixture());
        assert_eq!(sorted_matches(&files, "content/"), vec!["/res/content/a.xml", "/res/content/b.png"]);
    }

    #[test]
    fn a_star_filter_is_matched_as_a_glob_against_the_whole_path() {
        let files = build_file_list(&fixture());
        assert_eq!(sorted_matches(&files, "/res/**/*.xml"), vec!["/res/content/a.xml"]);
    }

    #[test]
    fn a_star_filter_that_is_not_a_valid_glob_falls_back_to_a_substring_match() {
        let files = build_file_list(&fixture());
        let pattern = format!("a{}*", '[');
        assert!(glob::Pattern::new(&pattern).is_err(), "an unclosed character class is not a valid pattern");
        assert!(filter_files(&files, &pattern).is_empty(), "no path contains that literal text");
    }

    #[test]
    fn a_glob_star_spans_separators_so_a_bare_extension_pattern_matches_at_any_depth() {
        let files = build_file_list(&fixture());
        assert_eq!(sorted_matches(&files, "*.png"), vec!["/res/content/b.png"]);
    }

    #[test]
    fn every_file_opens_through_the_root_the_list_was_walked_from() {
        use std::io::Read as _;
        let root = fixture();
        let files = build_file_list(&root);

        for (index, path) in files.iter() {
            let opened = files.vfs_path(index).expect("the path resolves");
            assert_eq!(opened.as_str(), path, "the handle names the path the walk recorded");
            let mut body = String::new();
            opened.open_file().expect("the file opens").read_to_string(&mut body).expect("the file reads");
            assert_eq!(files.size(index), Some(body.len() as u64), "the recorded size is the file's own");
        }
    }

    #[test]
    fn a_list_walked_from_a_subdirectory_labels_its_rows_from_there_and_still_opens_them() {
        use std::io::Read as _;
        let root = fixture();
        let content = root.join("res/content").expect("a valid path");
        let files = build_file_list(&content);

        assert_eq!(sorted_paths(&files), vec!["/a.xml", "/b.png"], "labelled from the directory walked");
        let index = files.iter().find(|(_, path)| *path == "/a.xml").map(|(index, _)| index).unwrap();
        let mut body = String::new();
        files.vfs_path(index).unwrap().open_file().unwrap().read_to_string(&mut body).unwrap();
        assert_eq!(body, "<a/>", "and still opening the file under that directory");
    }

    #[test]
    fn a_name_the_vfs_cannot_address_is_not_listed() {
        // `VfsPath::join` drops these or walks out of the directory, so an
        // entry named by one could not be opened from its listed path.
        assert!(!is_addressable(""));
        assert!(!is_addressable("."));
        assert!(!is_addressable(".."));
        assert!(is_addressable("a.xml"));
        assert!(is_addressable("...xml"), "a leading dot is only special on its own");
    }

    #[test]
    fn directory_entries_put_directories_first_then_sort_case_insensitively() {
        let labels: Vec<String> = directory_entries(&fixture(), "/res").into_iter().map(|entry| entry.label).collect();
        assert_eq!(labels, vec!["content", "readme.txt", "Zebra.txt"], "Zebra sorts after readme, not before");
    }

    #[test]
    fn the_root_path_and_an_empty_path_both_list_the_vfs_root() {
        let root = fixture();
        assert_eq!(directory_entries(&root, ROOT_PATH).len(), directory_entries(&root, "").len());
        assert_eq!(directory_entries(&root, ROOT_PATH).len(), 1, "only /res at the root");
    }

    #[test]
    fn listing_an_unreadable_directory_yields_no_rows() {
        assert!(directory_entries(&fixture(), "/nope").is_empty());
    }

    #[test]
    fn directory_rows_carry_a_size_for_files_and_none_for_directories() {
        let rows = directory_entries(&fixture(), "/res");
        let folder = rows.iter().find(|row| row.is_dir).unwrap();
        let file = rows.iter().find(|row| !row.is_dir).unwrap();
        assert_eq!(folder.size, None);
        assert!(file.size.is_some());
        assert_eq!(folder.type_label(), "Folder");
    }

    #[test]
    fn filter_result_rows_are_labelled_by_their_whole_path_and_carry_a_size() {
        let files = build_file_list(&fixture());
        let rows: Vec<ListingEntry> =
            filter_files(&files, "content/").into_iter().filter_map(|index| files.row(index)).collect();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.label.starts_with("/res/content/")));
        assert!(rows.iter().all(|row| !row.is_dir));
        assert!(rows.iter().all(|row| row.size.is_some()));
    }

    #[test]
    fn file_kind_follows_the_extension_lists() {
        assert_eq!(FileKind::of("ship.png"), FileKind::Image);
        assert_eq!(FileKind::of("data.xml"), FileKind::Plaintext);
        assert_eq!(FileKind::of("model.geometry"), FileKind::Opaque);
        assert_eq!(FileKind::of("SHOUT.PNG"), FileKind::Opaque, "the extension lists are case-sensitive");
    }

    #[test]
    fn type_labels_cover_the_known_extensions_and_fall_back_to_file() {
        assert_eq!(file_type_label("a.xml"), "XML");
        assert_eq!(file_type_label("a.jpeg"), "JPEG");
        assert_eq!(file_type_label("a.primitives_processed"), "Mesh");
        assert_eq!(file_type_label("a.wem"), "Audio");
        assert_eq!(file_type_label("a.unknown"), "File");
        assert_eq!(file_type_label("noextension"), "File");
    }
}
