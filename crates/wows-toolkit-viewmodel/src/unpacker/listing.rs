//! The Unpacker's VFS listing: the folder tree, the flat file list the filter
//! runs over, what the listing shows for a directory, and how a file's name
//! decides its icon, type label and viewer.
//!
//! Pure functions over a `VfsPath`, shared by both front ends so the listing
//! rules have one implementation, and unit-testable against an in-memory
//! filesystem rather than a game install.

use std::path::PathBuf;
use std::sync::Arc;
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

/// A file in the flat list, paired with the VFS handle that opens it.
pub type FileList = Vec<(Arc<PathBuf>, VfsPath)>;

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
/// Built lazily on first browser open: for a full install this is roughly
/// 85 MiB across ~1.1M allocations, and the browser is its only consumer.
/// Paths carry a leading slash so they match `build_folder_tree`'s keys.
pub fn build_file_list(root: &VfsPath) -> FileList {
    fn collect(dir: &VfsPath, prefix: &str, out: &mut FileList) {
        let Ok(entries) = dir.read_dir() else {
            return;
        };
        for entry in entries {
            let path = format!("{prefix}/{}", entry.filename());
            match entry.is_dir() {
                Ok(true) => collect(&entry, &path, out),
                Ok(false) => out.push((Arc::new(PathBuf::from(&path)), entry)),
                Err(_) => {}
            }
        }
    }
    let mut out = FileList::new();
    collect(root, "", &mut out);
    out
}

/// Applies the listing's path filter.
///
/// A filter containing a star is treated as a glob and matched against the
/// whole path; anything else is a plain substring match. A star filter that
/// does not parse as a glob falls back to the substring match, so a
/// half-typed pattern keeps listing results instead of emptying the pane.
///
/// `matches_path` runs with glob's default options, so a star spans path
/// separators: "*.png" matches a file at any depth, not only at the root.
pub fn filter_files(files: &FileList, filter: &str) -> FileList {
    let glob = filter.contains('*').then(|| glob::Pattern::new(filter).ok()).flatten();
    match glob {
        Some(glob) => files.iter().filter(|(path, _)| glob.matches_path(path)).cloned().collect(),
        None => {
            files.iter().filter(|(path, _)| path.to_str().is_some_and(|path| path.contains(filter))).cloned().collect()
        }
    }
}

/// Filter results as listing rows, each labelled by its whole path.
pub fn filtered_entries(files: &FileList, filter: &str) -> Vec<ListingEntry> {
    filter_files(files, filter)
        .into_iter()
        .map(|(path, vfs_path)| ListingEntry {
            label: path.to_string_lossy().into_owned(),
            is_dir: false,
            size: vfs_path.metadata().ok().map(|meta| meta.len),
            path: vfs_path,
        })
        .collect()
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
        let mut paths: Vec<String> = files.iter().map(|(path, _)| path.to_string_lossy().into_owned()).collect();
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
        assert_eq!(sorted_paths(&filter_files(&files, "content/")), vec!["/res/content/a.xml", "/res/content/b.png"]);
    }

    #[test]
    fn a_star_filter_is_matched_as_a_glob_against_the_whole_path() {
        let files = build_file_list(&fixture());
        assert_eq!(sorted_paths(&filter_files(&files, "/res/**/*.xml")), vec!["/res/content/a.xml"]);
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
        assert_eq!(sorted_paths(&filter_files(&files, "*.png")), vec!["/res/content/b.png"]);
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
    fn filtered_rows_are_labelled_by_their_whole_path() {
        let files = build_file_list(&fixture());
        let rows = filtered_entries(&files, "content/");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.label.starts_with("/res/content/")));
        assert!(rows.iter().all(|row| !row.is_dir));
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
