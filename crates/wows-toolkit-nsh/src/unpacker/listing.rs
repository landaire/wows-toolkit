//! What one browser pane lists, apart from how it is drawn: the rows of the
//! selected directory or of a filter, the breadcrumb trail, and how a row's
//! name decides its glyph and its actions.

use std::rc::Rc;
use std::sync::Arc;

use nsh_base::TreeItem;
use wows_toolkit_viewmodel::unpacker::listing::FileIndex;
use wows_toolkit_viewmodel::unpacker::listing::FileList;
use wows_toolkit_viewmodel::unpacker::listing::FolderTreeNode;
use wows_toolkit_viewmodel::unpacker::listing::ListingEntry;
use wows_toolkit_viewmodel::unpacker::listing::ROOT_PATH;
use wows_toolkit_viewmodel::unpacker::listing::build_file_list;
use wows_toolkit_viewmodel::unpacker::listing::build_folder_tree;
use wows_toolkit_viewmodel::unpacker::listing::directory_entries;
use wows_toolkit_viewmodel::unpacker::listing::filter_files;
use wows_toolkit_viewmodel::unpacker::listing::is_filtering;
use wowsunpack::vfs::VfsPath;

/// What the label of the tree's root reads: the extracted tree starts at
/// `res`, as the egui and GPUI listings label it.
pub const ROOT_LABEL: &str = "res";

/// Which VFS a pane browses. The package VFS arrives with the build;
/// assets.bin is parsed after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserSource {
    Pkg,
    AssetsBin,
}

impl BrowserSource {
    /// The pane's panel key in the dock.
    pub fn panel_id(self) -> &'static str {
        match self {
            Self::Pkg => "packages",
            Self::AssetsBin => "assets-bin",
        }
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Pkg => "ui.unpacker.packages",
            Self::AssetsBin => "ui.unpacker.assets_bin",
        }
    }
}

/// A shared value compared by identity, so a signal holding a large walked
/// tree notices a new one without comparing its contents.
#[derive(Debug)]
pub struct Shared<T>(pub Rc<T>);

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        Self(Rc::new(value))
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> std::ops::Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

/// A VFS walked once when it loads: re-walking per keystroke is what makes
/// a listing slow.
pub struct Walked {
    pub vfs: VfsPath,
    pub files: Arc<FileList>,
    pub folder_tree: Vec<FolderTreeNode>,
}

impl Walked {
    /// Walks `vfs` into its folder tree and flat file list. Blocks for as
    /// long as the walk takes, a few seconds on a full install.
    pub fn walk(vfs: VfsPath) -> Self {
        let folder_tree = build_folder_tree(&vfs, "");
        let files = Arc::new(build_file_list(&vfs));

        Self { vfs, files, folder_tree }
    }

    /// The folder tree as the tree view takes it: one root item labelled
    /// [`ROOT_LABEL`] holding every directory, each keyed by its path.
    pub fn tree_items(&self) -> Vec<TreeItem> {
        let mut root = TreeItem::new(ROOT_PATH, ROOT_LABEL);
        root.children = tree_items(&self.folder_tree);
        vec![root]
    }
}

/// Converts the walked tree with a worklist rather than recursion: a deep
/// install should not be bounded by the stack.
fn tree_items(nodes: &[FolderTreeNode]) -> Vec<TreeItem> {
    struct Pending<'a> {
        node: &'a FolderTreeNode,
        /// Where the finished item goes: its parent's position in `built`.
        parent: Option<usize>,
    }

    let mut built: Vec<(Option<usize>, TreeItem)> = Vec::new();
    let mut pending: Vec<Pending<'_>> = nodes.iter().rev().map(|node| Pending { node, parent: None }).collect();
    while let Some(Pending { node, parent }) = pending.pop() {
        let at = built.len();
        built.push((parent, TreeItem::new(node.path.clone(), node.name.clone())));
        pending.extend(node.children.iter().rev().map(|node| Pending { node, parent: Some(at) }));
    }

    // Children are pushed after their parent, so walking backwards attaches
    // each finished subtree to its parent, in reverse sibling order.
    let mut top = Vec::new();
    while let Some((parent, item)) = built.pop() {
        match parent {
            Some(parent) => built[parent].1.children.insert(0, item),
            None => top.insert(0, item),
        }
    }

    top
}

/// What a pane loaded, or why it has nothing to show.
#[derive(Clone, PartialEq)]
pub enum PaneState {
    /// No game data yet. Not an error: the WoWs directory may be unset.
    Empty,
    Loading,
    Failed(String),
    Ready(Shared<Walked>),
}

/// The rows a listing draws.
///
/// Filter results are positions in the file list rather than rows: a
/// three-character filter matches most of an install, and a materialised
/// row per match would cost several times what the list itself does.
pub enum Rows {
    /// The selected directory's own entries.
    Directory(Vec<ListingEntry>),
    Filtered { files: Arc<FileList>, matches: Vec<FileIndex> },
}

impl Rows {
    /// The listing for `state`: filter results while a long enough filter
    /// is typed, otherwise the selected directory's entries.
    pub fn build(state: &PaneState, selected_dir: &str, filter: &str) -> Self {
        let PaneState::Ready(walked) = state else {
            return Self::Directory(Vec::new());
        };

        if is_filtering(filter) {
            return Self::Filtered { files: Arc::clone(&walked.files), matches: filter_files(&walked.files, filter) };
        }

        Self::Directory(directory_entries(&walked.vfs, selected_dir))
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Directory(rows) => rows.len(),
            Self::Filtered { matches, .. } => matches.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The row at `at`, built on demand for a filter result.
    pub fn row(&self, at: usize) -> Option<ListingEntry> {
        match self {
            Self::Directory(rows) => rows.get(at).cloned(),
            Self::Filtered { files, matches } => files.row(*matches.get(at)?),
        }
    }

    /// Each row's identity, so a listing that changes keeps a cell per row
    /// rather than one per position.
    pub fn keys(&self) -> Vec<RowKey> {
        match self {
            Self::Directory(rows) => rows.iter().map(|row| RowKey::Name(row.label.clone())).collect(),
            Self::Filtered { matches, .. } => matches.iter().copied().map(RowKey::File).collect(),
        }
    }

    /// Every row, for an action that takes the whole listing at once.
    pub fn all(&self) -> Vec<ListingEntry> {
        match self {
            Self::Directory(rows) => rows.clone(),
            Self::Filtered { .. } => (0..self.len()).filter_map(|at| self.row(at)).collect(),
        }
    }
}

/// A listing row's identity: its name within a directory, or its file in
/// a filter result.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowKey {
    Name(String),
    File(FileIndex),
}

/// One breadcrumb: the label it shows and the directory it opens.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Crumb {
    pub label: String,
    pub path: String,
}

/// The path bar above the listing: the root, then one crumb per directory
/// down to `selected_dir`.
pub fn breadcrumbs(selected_dir: &str) -> Vec<Crumb> {
    let mut crumbs = vec![Crumb { label: ROOT_LABEL.to_owned(), path: ROOT_PATH.to_owned() }];

    let mut accumulated = String::new();
    for part in selected_dir.split('/').filter(|part| !part.is_empty()) {
        accumulated.push('/');
        accumulated.push_str(part);
        crumbs.push(Crumb { label: part.to_owned(), path: accumulated.clone() });
    }

    crumbs
}

/// The directory a revealed file is in, in the leading-slash form the tree
/// keys its folders by; a file at the root belongs to the root.
pub fn parent_directory(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    match trimmed.rfind('/') {
        Some(at) => format!("/{}", &trimmed[..at]),
        None => ROOT_PATH.to_owned(),
    }
}

/// Every directory from the root down to `dir`, which the tree expands so
/// `dir` shows.
pub fn ancestors(dir: &str) -> Vec<String> {
    let mut out = vec![ROOT_PATH.to_owned()];
    let mut accumulated = String::new();
    for part in dir.split('/').filter(|part| !part.is_empty()) {
        accumulated.push('/');
        accumulated.push_str(part);
        out.push(accumulated.clone());
    }

    out
}

/// Whether the file viewer has anything to show for this name: the
/// extensions the egui and GPUI listings offer "View Contents" for.
pub fn is_viewable(label: &str) -> bool {
    let extension = label.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    matches!(extension.as_str(), "xml" | "json" | "txt" | "cfg" | "log" | "csv" | "md" | "jpg" | "jpeg" | "png" | "svg")
}

/// The glyph of a listing row, from the four kinds the egui listing tells
/// apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Folder,
    Image,
    Text,
    Audio,
    File,
}

impl Glyph {
    pub fn of(label: &str, is_dir: bool) -> Self {
        if is_dir {
            return Self::Folder;
        }

        let extension = label.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
        match extension.as_str() {
            "png" | "jpg" | "jpeg" | "dds" | "svg" | "tga" | "bmp" => Self::Image,
            "txt" | "xml" | "json" | "cfg" | "log" | "csv" | "md" => Self::Text,
            "wem" | "bnk" | "ogg" | "wav" | "mp3" => Self::Audio,
            _ => Self::File,
        }
    }

    /// A short ASCII mark: the app ships no icon font.
    pub fn mark(self) -> &'static str {
        match self {
            Self::Folder => "[D]",
            Self::Image => "[I]",
            Self::Text => "[T]",
            Self::Audio => "[A]",
            Self::File => "[F]",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use wowsunpack::vfs::MemoryFS;

    use super::*;

    /// /content/a.xml, /content/gameplay/b.xml, /readme.txt
    fn fixture() -> VfsPath {
        let root: VfsPath = MemoryFS::new().into();
        root.join("content/gameplay").unwrap().create_dir_all().unwrap();
        for (path, body) in [("content/a.xml", "<a/>"), ("content/gameplay/b.xml", "<b/>"), ("readme.txt", "hi")] {
            root.join(path).unwrap().create_file().unwrap().write_all(body.as_bytes()).unwrap();
        }
        root
    }

    fn labels(rows: &Rows) -> Vec<String> {
        rows.all().into_iter().map(|row| row.label).collect()
    }

    #[test]
    fn the_tree_has_one_root_holding_every_directory_by_path() {
        let walked = Walked::walk(fixture());
        let items = walked.tree_items();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id.as_ref(), "/");
        assert_eq!(items[0].label.as_ref(), "res");
        assert_eq!(items[0].children[0].id.as_ref(), "/content");
        assert_eq!(items[0].children[0].children[0].id.as_ref(), "/content/gameplay");
        assert!(items[0].children[0].children[0].children.is_empty());
    }

    #[test]
    fn a_short_filter_lists_the_directory_and_a_long_one_searches_every_file() {
        let state = PaneState::Ready(Shared::new(Walked::walk(fixture())));

        assert_eq!(labels(&Rows::build(&state, "/content", "")), vec!["gameplay", "a.xml"]);
        assert_eq!(labels(&Rows::build(&state, "/content", "xm")), vec!["gameplay", "a.xml"], "two characters");

        let mut filtered = labels(&Rows::build(&state, "/content", ".xml"));
        filtered.sort();
        assert_eq!(filtered, vec!["/content/a.xml", "/content/gameplay/b.xml"]);
    }

    #[test]
    fn a_pane_without_a_vfs_lists_nothing() {
        assert!(Rows::build(&PaneState::Loading, "/", "").is_empty());
        assert!(Rows::build(&PaneState::Failed("x".into()), "/", "abc").is_empty());
    }

    #[test]
    fn breadcrumbs_start_at_res_and_follow_the_directory() {
        assert_eq!(breadcrumbs("/"), vec![Crumb { label: "res".into(), path: "/".into() }]);
        assert_eq!(
            breadcrumbs("/content/gameplay"),
            vec![
                Crumb { label: "res".into(), path: "/".into() },
                Crumb { label: "content".into(), path: "/content".into() },
                Crumb { label: "gameplay".into(), path: "/content/gameplay".into() },
            ]
        );
    }

    #[test]
    fn a_revealed_file_selects_its_own_directory() {
        assert_eq!(parent_directory("/content/gameplay/b.xml"), "/content/gameplay");
        assert_eq!(parent_directory("/top.txt"), "/");
        assert_eq!(ancestors("/content/gameplay"), vec!["/", "/content", "/content/gameplay"]);
    }

    #[test]
    fn glyphs_and_viewers_follow_the_extension() {
        assert_eq!(Glyph::of("x", true), Glyph::Folder);
        assert_eq!(Glyph::of("ship.PNG", false), Glyph::Image);
        assert_eq!(Glyph::of("a.wem", false), Glyph::Audio);
        assert!(is_viewable("a.json"));
        assert!(!is_viewable("a.geometry"));
    }
}
