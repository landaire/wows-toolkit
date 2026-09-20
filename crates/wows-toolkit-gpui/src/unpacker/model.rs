//! VFS-shaped state behind the Unpacker tab: the folder tree, the flat file
//! list the listing filters over, and how a file's name decides which viewer
//! opens it. Pure functions over a `VfsPath`, so they are unit-testable
//! against an in-memory filesystem rather than a game install.

use std::path::PathBuf;
use std::sync::Arc;
use wowsunpack::vfs::VfsPath;

/// One directory in the folder-tree sidebar. Built once when the VFS loads:
/// re-walking `read_dir` per frame is what made the egui version slow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderTreeNode {
    pub name: String,
    /// Absolute, leading-slash path, the key the listing selects by.
    pub path: String,
    pub children: Vec<FolderTreeNode>,
}

/// A file in the flat list, paired with the VFS handle that opens it.
pub type FileList = Vec<(Arc<PathBuf>, VfsPath)>;

/// Which viewer a file opens in, decided by extension.
///
/// The egui app keys this off two extension lists and falls through to
/// extraction-only for everything else; an enum makes that exhaustive here
/// rather than two overlapping boolean checks.
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
    /// Matches the egui app's `IMAGE_FILE_TYPES`/`PLAINTEXT_FILE_TYPES`
    /// suffix checks, which are case-sensitive and include the dot.
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

/// Walks `root` into the flat list the listing filters over.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use wowsunpack::vfs::MemoryFS;

    /// A small tree: /res/content/a.xml, /res/content/b.png, /res/readme.txt.
    fn fixture() -> VfsPath {
        let root: VfsPath = MemoryFS::new().into();
        root.join("res/content").unwrap().create_dir_all().unwrap();
        for (path, body) in [("res/content/a.xml", "<a/>"), ("res/content/b.png", "png"), ("res/readme.txt", "hi")] {
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
        assert_eq!(tree[0].name, "res");
        assert_eq!(tree[0].path, "/res");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].path, "/res/content");
        assert!(tree[0].children[0].children.is_empty());
    }

    #[test]
    fn folder_tree_of_an_unreadable_directory_is_empty_rather_than_an_error() {
        let missing = fixture().join("does/not/exist").unwrap();
        assert!(build_folder_tree(&missing, "").is_empty());
    }

    #[test]
    fn file_list_is_files_only_with_leading_slash_paths() {
        assert_eq!(
            sorted_paths(&build_file_list(&fixture())),
            vec!["/res/content/a.xml", "/res/content/b.png", "/res/readme.txt"]
        );
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
        assert!(glob::Pattern::new("a[*").is_err(), "an unclosed character class is not a valid pattern");
        assert!(filter_files(&files, "a[*").is_empty(), "no path contains that literal text");
    }

    #[test]
    fn a_glob_star_spans_separators_so_a_bare_extension_pattern_matches_at_any_depth() {
        let files = build_file_list(&fixture());
        assert_eq!(sorted_paths(&filter_files(&files, "*.png")), vec!["/res/content/b.png"]);
    }

    #[test]
    fn an_empty_filter_keeps_every_file() {
        let files = build_file_list(&fixture());
        assert_eq!(filter_files(&files, "").len(), files.len());
    }

    #[test]
    fn file_kind_follows_the_extension_lists() {
        assert_eq!(FileKind::of("ship.png"), FileKind::Image);
        assert_eq!(FileKind::of("icon.svg"), FileKind::Image);
        assert_eq!(FileKind::of("data.xml"), FileKind::Plaintext);
        assert_eq!(FileKind::of("notes.txt"), FileKind::Plaintext);
        assert_eq!(FileKind::of("model.geometry"), FileKind::Opaque);
        assert_eq!(FileKind::of("SHOUT.PNG"), FileKind::Opaque, "the egui lists are case-sensitive");
    }
}
