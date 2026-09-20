//! One file-browser pane: a resizable folder-tree sidebar beside a virtualized
//! file listing, with a path filter above the tree and the content-search
//! inputs pinned below it. Mirrors the egui app's browser pane
//! (`ui/file_unpacker.rs`), which lays the same four controls out the same way.
//!
//! The pane owns no extraction or search machinery of its own; it emits
//! [`BrowserEvent`] and the Unpacker tab acts on it.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::resizable::h_resizable;
use gpui_kit::component::resizable::resizable_panel;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::tree::TreeEntry;
use gpui_kit::component::tree::TreeItem;
use gpui_kit::component::tree::TreeState;
use gpui_kit::component::tree::tree;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use wowsunpack::vfs::VfsPath;

use super::model::FileKind;
use super::model::FileList;
use super::model::FolderTreeNode;
use super::model::build_file_list;
use super::model::build_folder_tree;
use super::model::filter_files;

/// Which VFS a pane browses. The two sources load differently -- the package
/// VFS arrives with the build, assets.bin is parsed in the background -- so
/// the pane keeps them apart rather than treating one as a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserSource {
    Pkg,
    AssetsBin,
}

impl BrowserSource {
    /// Stable fragment for element ids, so the two panes never collide.
    pub fn id_fragment(self) -> &'static str {
        match self {
            Self::Pkg => "pkg",
            Self::AssetsBin => "assetsbin",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Pkg => "Packages",
            Self::AssetsBin => "Assets.bin",
        }
    }
}

/// What the pane loaded, or why it has nothing to show.
///
/// An enum rather than a `loading` flag beside an `Option<VfsPath>` beside an
/// `Option<String>` error: those three admit states that cannot happen.
enum PaneState {
    /// No game data yet. Not an error: the WoWs directory may be unset.
    Empty,
    Loading,
    Failed(String),
    Ready(Loaded),
}

struct Loaded {
    vfs: VfsPath,
    files: Arc<FileList>,
    folder_tree: Vec<FolderTreeNode>,
}

/// Raised for the Unpacker tab to act on.
#[derive(Clone, Debug)]
pub enum BrowserEvent {
    /// Queue these VFS entries for extraction.
    Extract(Vec<VfsPath>),
    /// Run a content search over this pane's files.
    Search { source: BrowserSource, query: String, path_filter: String, files: Arc<FileList> },
    /// Open this file in the in-app viewer that suits its kind.
    View { path: VfsPath, kind: FileKind },
}

impl EventEmitter<BrowserEvent> for BrowserPanel {}
impl EventEmitter<PanelEvent> for BrowserPanel {}

const SIDEBAR_WIDTH: Pixels = px(260.);
const SIDEBAR_MIN_WIDTH: Pixels = px(160.);
const SIDEBAR_MAX_WIDTH: Pixels = px(520.);
const ROW_HEIGHT: Pixels = px(22.);
const LIST_OVERDRAW: Pixels = px(200.);

/// The root row's id. The egui tree shows the VFS root as a node labelled
/// "res" that is open by default; this mirrors it.
const ROOT_PATH: &str = "/";

pub struct BrowserPanel {
    source: BrowserSource,
    state: PaneState,
    /// Directory selected in the tree. `None` means the root.
    selected_dir: Option<String>,
    tree_state: Entity<TreeState>,
    /// Absolute directory path per tree row id, so a click resolves without
    /// re-walking the tree.
    row_paths: Rc<HashMap<SharedString, String>>,
    filter_state: Entity<InputState>,
    filter_text: String,
    /// `filter_text` applied to the file list. Recomputed only when the text
    /// changes, since a full install filters ~1.1M paths.
    filtered: Option<Arc<FileList>>,
    search_state: Entity<InputState>,
    path_filter_state: Entity<InputState>,
    list_state: ListState,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl BrowserPanel {
    pub fn new(source: BrowserSource, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let filter_state = cx.new(|cx| InputState::new(window, cx).placeholder("Filter files..."));
        let search_state = cx.new(|cx| InputState::new(window, cx).placeholder("Search in files..."));
        let path_filter_state = cx.new(|cx| InputState::new(window, cx).placeholder("Path filter (glob)"));

        let filter_subscription = cx.subscribe(&filter_state, Self::on_filter_event);

        Self {
            source,
            state: PaneState::Empty,
            selected_dir: None,
            tree_state,
            row_paths: Rc::new(HashMap::new()),
            filter_state,
            filter_text: String::new(),
            filtered: None,
            search_state,
            path_filter_state,
            list_state: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![filter_subscription],
        }
    }

    /// Marks the pane as waiting on a background load (assets.bin).
    pub fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.state = PaneState::Loading;
        self.sync_rows(cx);
    }

    pub fn set_failed(&mut self, reason: String, cx: &mut Context<Self>) {
        self.state = PaneState::Failed(reason);
        self.sync_rows(cx);
    }

    /// Adopts a VFS: walks it into the folder tree and the flat file list.
    /// Both walks are O(tree) and happen once per load, never per frame.
    pub fn set_vfs(&mut self, vfs: VfsPath, cx: &mut Context<Self>) {
        let folder_tree = build_folder_tree(&vfs, "");
        let files = Arc::new(build_file_list(&vfs));
        self.state = PaneState::Ready(Loaded { vfs, files, folder_tree });
        self.selected_dir = None;
        self.filtered = None;
        self.rebuild_tree(cx);
        self.sync_rows(cx);
    }

    /// Drops the loaded VFS, e.g. when the selected build changes.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.state = PaneState::Empty;
        self.selected_dir = None;
        self.filtered = None;
        self.rebuild_tree(cx);
        self.sync_rows(cx);
    }

    fn on_filter_event(&mut self, _state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        let InputEvent::Change = event else { return };
        let text = self.filter_state.read(cx).value().to_string();
        if text == self.filter_text {
            return;
        }
        self.filter_text = text;
        self.filtered = None;
        self.sync_rows(cx);
    }

    fn rebuild_tree(&mut self, cx: &mut Context<Self>) {
        let PaneState::Ready(loaded) = &self.state else {
            self.row_paths = Rc::new(HashMap::new());
            self.tree_state.update(cx, |state, cx| state.set_items(Vec::new(), cx));
            return;
        };

        let mut row_paths: HashMap<SharedString, String> = HashMap::new();
        let source = self.source;

        fn walk(
            nodes: &[FolderTreeNode],
            source: BrowserSource,
            row_paths: &mut HashMap<SharedString, String>,
        ) -> Vec<TreeItem> {
            nodes
                .iter()
                .map(|node| {
                    let id: SharedString = format!("unpacker-{}-dir-{}", source.id_fragment(), node.path).into();
                    row_paths.insert(id.clone(), node.path.clone());
                    TreeItem::new(id, node.name.clone()).children(walk(&node.children, source, row_paths))
                })
                .collect()
        }

        let children = walk(&loaded.folder_tree, source, &mut row_paths);
        let root_id: SharedString = format!("unpacker-{}-dir-root", source.id_fragment()).into();
        row_paths.insert(root_id.clone(), ROOT_PATH.to_string());
        let root = TreeItem::new(root_id, "res").children(children).expanded(true);

        self.row_paths = Rc::new(row_paths);
        self.tree_state.update(cx, |state, cx| state.set_items(vec![root], cx));
    }

    /// The rows the listing shows: the filtered flat list while a filter is
    /// typed, otherwise the selected directory's own entries.
    fn rows(&mut self) -> Vec<ListingRow> {
        let PaneState::Ready(loaded) = &self.state else {
            return Vec::new();
        };

        if !self.filter_text.is_empty() {
            let filtered =
                self.filtered.get_or_insert_with(|| Arc::new(filter_files(&loaded.files, &self.filter_text))).clone();
            return filtered
                .iter()
                .map(|(path, vfs_path)| ListingRow {
                    label: path.to_string_lossy().into_owned(),
                    path: vfs_path.clone(),
                    is_dir: false,
                })
                .collect();
        }

        let dir = match &self.selected_dir {
            Some(dir) if dir != ROOT_PATH => loaded.vfs.join(dir.trim_start_matches('/')).ok(),
            _ => Some(loaded.vfs.clone()),
        };
        let Some(dir) = dir else { return Vec::new() };
        let Ok(entries) = dir.read_dir() else { return Vec::new() };

        let mut rows: Vec<ListingRow> = entries
            .map(|entry| ListingRow { label: entry.filename(), is_dir: entry.is_dir().unwrap_or(false), path: entry })
            .collect();
        // Directories first, then files, each alphabetical -- the order the
        // egui listing shows.
        rows.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.label.cmp(&b.label)));
        rows
    }

    fn sync_rows(&mut self, cx: &mut Context<Self>) {
        let len = self.rows().len();
        self.list_state.reset(len);
        cx.notify();
    }

    fn select_dir(&mut self, path: String, cx: &mut Context<Self>) {
        if self.selected_dir.as_deref() == Some(path.as_str()) {
            return;
        }
        self.selected_dir = Some(path);
        self.sync_rows(cx);
    }

    fn start_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search_state.read(cx).value().trim().to_string();
        if query.is_empty() {
            return;
        }
        let PaneState::Ready(loaded) = &self.state else {
            return;
        };
        let files = loaded.files.clone();
        let path_filter = self.path_filter_state.read(cx).value().trim().to_string();
        cx.emit(BrowserEvent::Search { source: self.source, query, path_filter, files });
    }
}

/// One row in the file listing.
#[derive(Clone)]
struct ListingRow {
    label: String,
    is_dir: bool,
    path: VfsPath,
}

impl Focusable for BrowserPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for BrowserPanel {
    fn panel_name(&self) -> &'static str {
        "UnpackerBrowserPanel"
    }
}

impl Panel for BrowserPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(self.source.title())
    }
}

impl Render for BrowserPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = match &self.state {
            PaneState::Empty => Some("No game data loaded".to_string()),
            PaneState::Loading => Some("Loading...".to_string()),
            PaneState::Failed(reason) => Some(format!("Failed to load: {reason}")),
            PaneState::Ready(_) => None,
        };

        if let Some(status) = status {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().opacity(0.6).child(status))
                .into_any_element();
        }

        let border = cx.theme().border;
        let fragment = self.source.id_fragment();

        let filter_row = h_flex()
            .flex_none()
            .gap_1()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(Icon::new(IconName::Search))
            .child(
                Input::new(&self.filter_state)
                    .id(SharedString::from(format!("unpacker-{fragment}-filter")))
                    .small()
                    .w_full(),
            );

        let row_paths = self.row_paths.clone();
        let entity = cx.entity();
        let folder_tree = tree(&self.tree_state, move |ix, entry, selected, _window, _cx| {
            render_folder_row(ix, entry, selected, &row_paths, entity.clone())
        })
        .flex_1();

        let search_box = v_flex()
            .flex_none()
            .gap_1()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(border)
            .child(div().text_xs().font_weight(FontWeight::BOLD).child("Search in files"))
            .child(
                h_flex().gap_1().items_center().child(Icon::new(IconName::Search)).child(
                    Input::new(&self.search_state)
                        .id(SharedString::from(format!("unpacker-{fragment}-search")))
                        .small()
                        .w_full(),
                ),
            )
            .child(
                Input::new(&self.path_filter_state)
                    .id(SharedString::from(format!("unpacker-{fragment}-path-filter")))
                    .small()
                    .w_full(),
            )
            .child(
                Button::new(SharedString::from(format!("unpacker-{fragment}-search-run")))
                    .label("Search")
                    .compact()
                    .on_click(cx.listener(|this, _event, _window, cx| this.start_search(cx))),
            );

        let sidebar = v_flex().size_full().child(filter_row).child(folder_tree).child(search_box);

        let rows = Rc::new(self.rows());
        let listing_entity = cx.entity();
        let hover_bg = cx.theme().accent;
        let render_row = {
            let rows = rows.clone();
            move |ix: usize, _window: &mut Window, _cx: &mut App| {
                let Some(row) = rows.get(ix) else {
                    return div().into_any_element();
                };
                let entity = listing_entity.clone();
                let path = row.path.clone();
                let is_dir = row.is_dir;
                let label = row.label.clone();
                let kind = FileKind::of(&label);
                h_flex()
                    .id(ix)
                    .w_full()
                    .h(ROW_HEIGHT)
                    .gap_1()
                    .items_center()
                    .px_2()
                    .hover(|this| this.bg(hover_bg))
                    .child(Icon::new(if is_dir { IconName::Folder } else { IconName::FileText }))
                    .child(div().flex_1().text_sm().child(label.clone()))
                    .on_click(move |event, _window, cx| {
                        if event.click_count() < 2 {
                            return;
                        }
                        entity.update(cx, |this, cx| {
                            if is_dir {
                                // Double-clicking a directory navigates into it.
                                let Some(name) = path.filename().into() else { return };
                                let base = this.selected_dir.clone().unwrap_or_else(|| ROOT_PATH.to_string());
                                let next =
                                    if base == ROOT_PATH { format!("/{name}") } else { format!("{base}/{name}") };
                                this.select_dir(next, cx);
                            } else {
                                cx.emit(BrowserEvent::View { path: path.clone(), kind });
                            }
                        });
                    })
                    .into_any_element()
            }
        };

        let extract_rows = rows.clone();
        let listing_header = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(div().flex_1().text_xs().opacity(0.6).child(format!("{} items", rows.len())))
            .child(
                Button::new(SharedString::from(format!("unpacker-{fragment}-extract-listed")))
                    .icon(IconName::HardDrive)
                    .label("Extract listed")
                    .compact()
                    .disabled(rows.is_empty())
                    .on_click(cx.listener(move |_this, _event, _window, cx| {
                        let paths: Vec<VfsPath> = extract_rows.iter().map(|row| row.path.clone()).collect();
                        cx.emit(BrowserEvent::Extract(paths));
                    })),
            );

        let listing = v_flex().size_full().child(listing_header).child(
            div()
                .relative()
                .flex_1()
                .child(list(self.list_state.clone(), render_row).size_full())
                .child(Scrollbar::vertical(&self.list_state)),
        );

        h_resizable(SharedString::from(format!("unpacker-{fragment}-split")))
            .child(
                resizable_panel()
                    .size(SIDEBAR_WIDTH)
                    .size_range(SIDEBAR_MIN_WIDTH..SIDEBAR_MAX_WIDTH)
                    .flex_none()
                    .child(sidebar),
            )
            .child(resizable_panel().child(listing))
            .into_any_element()
    }
}

/// One folder row: indentation by depth, a folder glyph, the directory name.
/// Clicking selects the directory, which is what drives the listing; the
/// tree's own chevron still handles expand and collapse.
fn render_folder_row(
    ix: usize,
    entry: &TreeEntry,
    selected: bool,
    row_paths: &HashMap<SharedString, String>,
    panel: Entity<BrowserPanel>,
) -> ListItem {
    let item = entry.item();
    let row = h_flex()
        .gap_1()
        .items_center()
        .pl(px(16.) * entry.depth())
        .child(Icon::new(if entry.is_folder() { IconName::FolderOpen } else { IconName::Folder }))
        .child(div().text_sm().child(item.label.clone()));

    let list_item = ListItem::new(ix).selected(selected).child(row);
    match row_paths.get(&item.id).cloned() {
        Some(path) => list_item.on_click(move |_event: &ClickEvent, _window, cx: &mut App| {
            panel.update(cx, |this, cx| this.select_dir(path.clone(), cx));
        }),
        None => list_item,
    }
}
