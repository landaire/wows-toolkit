//! One browser pane's state: what it loaded, the directory it shows, its filter and the rows
//! that follow from them. The rows are rebuilt when one of those changes, never per frame.

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::PanelWidth;
use landaire_ui::TreeHandle;
use landaire_ui::TreeModel;
use rust_i18n::t;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wows_toolkit_viewmodel::unpacker::listing::FileIndex;
use wows_toolkit_viewmodel::unpacker::listing::FileList;
use wows_toolkit_viewmodel::unpacker::listing::FolderTreeNode;
use wows_toolkit_viewmodel::unpacker::listing::ListingEntry;
use wows_toolkit_viewmodel::unpacker::listing::PathFilter;
use wows_toolkit_viewmodel::unpacker::listing::ROOT_PATH;
use wows_toolkit_viewmodel::unpacker::listing::build_file_list;
use wows_toolkit_viewmodel::unpacker::listing::build_folder_tree;
use wows_toolkit_viewmodel::unpacker::listing::containing_dir;
use wows_toolkit_viewmodel::unpacker::listing::directory_entries;
use wows_toolkit_viewmodel::unpacker::listing::filter_files;
use wowsunpack::vfs::VfsPath;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;

/// The folder sidebar's width when the tab opens, and the widths it may be dragged between.
pub const SIDEBAR_WIDTH: PanelWidth = PanelWidth(260.0);
pub const SIDEBAR_MIN_WIDTH: PanelWidth = PanelWidth(160.0);
pub const SIDEBAR_MAX_WIDTH: PanelWidth = PanelWidth(520.0);

/// Which VFS a pane browses. The two load differently: the package VFS arrives with the build,
/// assets.bin is parsed after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserSource {
    Pkg,
    AssetsBin,
}

impl BrowserSource {
    pub const ALL: [Self; 2] = [Self::Pkg, Self::AssetsBin];

    /// Stable fragment for test ids and panel ids, so the two panes never collide.
    pub const fn id_fragment(self) -> &'static str {
        match self {
            Self::Pkg => "pkg",
            Self::AssetsBin => "assetsbin",
        }
    }

    pub const fn title_key(self) -> &'static str {
        match self {
            Self::Pkg => "ui.unpacker.packages",
            Self::AssetsBin => "ui.unpacker.assets_bin",
        }
    }

    pub fn title(self) -> String {
        t!(self.title_key()).into_owned()
    }
}

/// What the pane loaded, or why it has nothing to show.
#[derive(Clone)]
pub enum PaneState {
    /// No game data yet. Not an error: the WoWs directory may be unset.
    Empty,
    Loading,
    Failed(String),
    Ready(Rc<Loaded>),
}

pub struct Loaded {
    pub vfs: VfsPath,
    pub files: Arc<FileList>,
    pub tree: Rc<FolderTree>,
}

/// A directory of the folder tree, by its absolute path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DirKey(pub String);

impl DirKey {
    pub fn root() -> Self {
        Self(ROOT_PATH.to_string())
    }
}

/// The folder sidebar's nodes: "res" at the root, then every directory of the VFS.
pub struct FolderTree {
    roots: Vec<DirKey>,
    children: HashMap<DirKey, Vec<DirKey>>,
    parents: HashMap<DirKey, DirKey>,
    names: HashMap<DirKey, String>,
}

impl FolderTree {
    pub fn new(nodes: &[FolderTreeNode]) -> Self {
        let root = DirKey::root();
        let mut tree = Self {
            roots: vec![root.clone()],
            children: HashMap::new(),
            parents: HashMap::new(),
            names: HashMap::from([(root.clone(), "res".to_string())]),
        };
        tree.add(&root, nodes);
        tree
    }

    fn add(&mut self, parent: &DirKey, nodes: &[FolderTreeNode]) {
        let keys: Vec<DirKey> = nodes.iter().map(|node| DirKey(node.path.clone())).collect();
        for (node, key) in nodes.iter().zip(&keys) {
            self.parents.insert(key.clone(), parent.clone());
            self.names.insert(key.clone(), node.name.clone());
            self.add(key, &node.children);
        }
        self.children.insert(parent.clone(), keys);
    }

    pub fn name(&self, key: &DirKey) -> &str {
        self.names.get(key).map_or("", String::as_str)
    }

    pub fn contains(&self, key: &DirKey) -> bool {
        self.names.contains_key(key)
    }
}

impl TreeModel for FolderTree {
    type Key = DirKey;

    fn roots(&self) -> &[DirKey] {
        &self.roots
    }

    fn children(&self, key: &DirKey) -> &[DirKey] {
        self.children.get(key).map_or(&[], Vec::as_slice)
    }

    fn parent(&self, key: &DirKey) -> Option<DirKey> {
        self.parents.get(key).cloned()
    }

    fn text(&self, key: &DirKey) -> Option<&str> {
        self.names.get(key).map(String::as_str)
    }
}

/// What the listing draws.
///
/// Filter results are positions in the file list rather than rows: a three-character filter
/// matches most of an install, and a row per match would cost several times the list itself.
#[derive(Clone)]
pub enum Rows {
    /// The selected directory's own entries.
    Directory(Rc<Vec<ListingEntry>>),
    Filtered {
        files: Arc<FileList>,
        filter: Rc<PathFilter>,
        matches: Rc<Vec<FileIndex>>,
    },
}

impl Rows {
    pub fn empty() -> Self {
        Self::Directory(Rc::new(Vec::new()))
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
            Self::Filtered { files, matches, .. } => files.row(*matches.get(at)?),
        }
    }

    /// Every row, for an action that takes the whole listing at once.
    pub fn all(&self) -> Vec<ListingEntry> {
        match self {
            Self::Directory(rows) => rows.as_ref().clone(),
            Self::Filtered { .. } => (0..self.len()).filter_map(|at| self.row(at)).collect(),
        }
    }

    /// The typed text a filter result matched, which its label is marked by. A glob marks
    /// nothing.
    pub fn query(&self) -> Option<&FilterQuery> {
        match self {
            Self::Filtered { filter, .. } => match filter.as_ref() {
                PathFilter::Text(query) => Some(query),
                PathFilter::Glob(_) => None,
            },
            Self::Directory(_) => None,
        }
    }
}

/// A listing row, by its position in the rows it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowKey(pub usize);

/// The listing as a flat tree of leaves, so it is drawn by the same virtualized rows as the
/// folder tree.
pub struct ListingModel {
    keys: Vec<RowKey>,
}

impl ListingModel {
    pub fn new(len: usize) -> Self {
        Self { keys: (0..len).map(RowKey).collect() }
    }
}

impl TreeModel for ListingModel {
    type Key = RowKey;

    fn roots(&self) -> &[RowKey] {
        &self.keys
    }

    fn children(&self, _key: &RowKey) -> &[RowKey] {
        &[]
    }

    fn parent(&self, _key: &RowKey) -> Option<RowKey> {
        None
    }
}

/// One browser pane. `Copy`: every field is a handle on reactive state owned by the tab.
#[derive(Clone, Copy)]
pub struct Pane {
    pub source: BrowserSource,
    pub state: RwSignal<PaneState, LocalStorage>,
    /// The directory the listing shows, as an absolute path.
    pub selected_dir: RwSignal<String, LocalStorage>,
    pub filter: RwSignal<String, LocalStorage>,
    pub rows: RwSignal<Rows, LocalStorage>,
    pub listing: RwSignal<Rc<ListingModel>, LocalStorage>,
    pub listing_selection: RwSignal<HashSet<RowKey>, LocalStorage>,
    pub tree: RwSignal<Rc<FolderTree>, LocalStorage>,
    pub expanded: RwSignal<HashSet<DirKey>, LocalStorage>,
    pub tree_selection: RwSignal<HashSet<DirKey>, LocalStorage>,
    pub tree_handle: TreeHandle<DirKey>,
    pub search_query: RwSignal<String, LocalStorage>,
    pub path_filter: RwSignal<String, LocalStorage>,
    pub sidebar_open: RwSignal<bool, LocalStorage>,
    pub sidebar_width: RwSignal<PanelWidth, LocalStorage>,
}

impl Pane {
    /// Made inside the tab's scope; the effects that keep the rows current end with it.
    pub fn new(source: BrowserSource) -> Self {
        let pane = Self {
            source,
            state: RwSignal::new_local(PaneState::Empty),
            selected_dir: RwSignal::new_local(ROOT_PATH.to_string()),
            filter: RwSignal::new_local(String::new()),
            rows: RwSignal::new_local(Rows::empty()),
            listing: RwSignal::new_local(Rc::new(ListingModel::new(0))),
            listing_selection: RwSignal::new_local(HashSet::new()),
            tree: RwSignal::new_local(Rc::new(FolderTree::new(&[]))),
            expanded: RwSignal::new_local(HashSet::from([DirKey::root()])),
            tree_selection: RwSignal::new_local(HashSet::new()),
            tree_handle: TreeHandle::new(),
            search_query: RwSignal::new_local(String::new()),
            path_filter: RwSignal::new_local(String::new()),
            sidebar_open: RwSignal::new_local(true),
            sidebar_width: RwSignal::new_local(SIDEBAR_WIDTH),
        };
        pane.follow();
        pane
    }

    fn follow(self) {
        // The rows follow the load, the directory and the filter.
        let rows = RenderEffect::new(move |_| {
            let state = self.state.get();
            let dir = self.selected_dir.get();
            let filter = self.filter.get();
            let rows = match (&state, PathFilter::new(&filter)) {
                (PaneState::Ready(loaded), Some(filter)) => Rows::Filtered {
                    files: loaded.files.clone(),
                    matches: Rc::new(filter_files(&loaded.files, &filter)),
                    filter: Rc::new(filter),
                },
                (PaneState::Ready(loaded), None) => Rows::Directory(Rc::new(directory_entries(&loaded.vfs, &dir))),
                _ => Rows::empty(),
            };
            self.listing_selection.set(HashSet::new());
            self.listing.set(Rc::new(ListingModel::new(rows.len())));
            self.rows.set(rows);
        });
        // A directory picked in the tree is the one the listing shows.
        let picked = RenderEffect::new(move |_| {
            let picked = self.tree_selection.with(|selection| selection.iter().next().cloned());
            if let Some(DirKey(dir)) = picked
                && self.selected_dir.get_untracked() != dir
            {
                self.selected_dir.set(dir);
            }
        });
        // A directory reached another way (a breadcrumb, a double click, a reveal) is selected in
        // the tree and brought into view.
        let shown = RenderEffect::new(move |_| {
            let key = DirKey(self.selected_dir.get());
            let known = self.tree.with(|tree| tree.contains(&key));
            let already = self.tree_selection.with_untracked(|selection| selection.contains(&key));
            if known && !already {
                self.tree_selection.set(HashSet::from([key.clone()]));
                self.tree_handle.scroll_to_key(key);
            }
        });
        on_cleanup_local(move || {
            drop(rows);
            drop(picked);
            drop(shown);
        });
    }

    pub fn set_loading(self) {
        self.state.set(PaneState::Loading);
    }

    pub fn set_failed(self, reason: String) {
        self.state.set(PaneState::Failed(reason));
    }

    /// Adopts a VFS: walks it into the folder tree and the flat file list, once per load.
    ///
    /// The filter is cleared with the rest: a filter typed against the old build would otherwise
    /// silently re-apply to the new one.
    pub fn set_vfs(self, vfs: VfsPath) {
        let tree = Rc::new(FolderTree::new(&build_folder_tree(&vfs, "")));
        let files = Arc::new(build_file_list(&vfs));
        self.reset_view();
        self.tree.set(tree.clone());
        self.state.set(PaneState::Ready(Rc::new(Loaded { vfs, files, tree })));
    }

    /// Drops the loaded VFS, e.g. when the WoWs directory is cleared.
    pub fn clear(self) {
        self.reset_view();
        self.tree.set(Rc::new(FolderTree::new(&[])));
        self.state.set(PaneState::Empty);
    }

    fn reset_view(self) {
        self.filter.set(String::new());
        self.selected_dir.set(ROOT_PATH.to_string());
        self.tree_selection.set(HashSet::new());
        self.expanded.set(HashSet::from([DirKey::root()]));
    }

    pub fn select_dir(self, dir: String) {
        if self.selected_dir.get_untracked() != dir {
            self.selected_dir.set(dir);
        }
    }

    /// Shows `path`'s own directory with the file among the rows, clearing any filter that would
    /// hide it.
    pub fn reveal(self, path: &VfsPath) {
        if !self.filter.get_untracked().is_empty() {
            self.filter.set(String::new());
        }
        self.select_dir(containing_dir(path));
        self.sidebar_open.set(true);
    }

    pub fn files(self) -> Option<Arc<FileList>> {
        self.state.with_untracked(|state| match state {
            PaneState::Ready(loaded) => Some(loaded.files.clone()),
            _ => None,
        })
    }
}
