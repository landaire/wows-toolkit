//! One file-browser pane: a resizable folder-tree sidebar beside a virtualized
//! file listing, with a path filter above the tree and the content-search
//! inputs pinned below it. Mirrors the egui app's browser pane
//! (`ui/file_unpacker.rs`), which lays the same four controls out the same way.
//!
//! What to list, filter and search is `wows_toolkit_viewmodel::unpacker`,
//! shared with that pane. This module is the rendering and the caching: the
//! listing rows are rebuilt when something changes them, never per frame.

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::resizable::h_resizable;
use gpui_kit::component::resizable::resizable_panel;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::tree::TreeEntry;
use gpui_kit::component::tree::TreeItem;
use gpui_kit::component::tree::TreeState;
use gpui_kit::component::tree::tree;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use wowsunpack::vfs::VfsPath;

use wows_toolkit_viewmodel::unpacker::listing::FileList;
use wows_toolkit_viewmodel::unpacker::listing::FolderTreeNode;
use wows_toolkit_viewmodel::unpacker::listing::ListingEntry;
use wows_toolkit_viewmodel::unpacker::listing::ROOT_PATH;
use wows_toolkit_viewmodel::unpacker::listing::build_file_list;
use wows_toolkit_viewmodel::unpacker::listing::build_folder_tree;
use wows_toolkit_viewmodel::unpacker::listing::directory_entries;
use wows_toolkit_viewmodel::unpacker::listing::filtered_entries;
use wows_toolkit_viewmodel::unpacker::listing::is_filtering;
use wows_toolkit_viewmodel::unpacker::viewer::decodable_prototype;

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
    /// Queue what the listing currently shows. The rows travel rather than
    /// bare paths so the queue can apply its own files-only rule.
    Extract(Vec<ListingEntry>),
    /// Run a content search over this pane's files.
    Search { source: BrowserSource, query: String, path_filter: String, files: Arc<FileList> },
    /// Open this file in the viewer its name calls for.
    View { path: VfsPath },
    /// Decode this assets.bin prototype and show the JSON.
    ViewAsJson { path: VfsPath },
    /// Decode this assets.bin prototype and write the JSON to a file.
    ExtractAsJson { path: VfsPath },
    /// Add one entry to the extraction queue.
    Queue(VfsPath),
    /// Drop one entry from the extraction queue.
    Unqueue(VfsPath),
}

impl EventEmitter<BrowserEvent> for BrowserPanel {}
impl EventEmitter<PanelEvent> for BrowserPanel {}

const SIDEBAR_WIDTH: Pixels = px(260.);
const SIDEBAR_MIN_WIDTH: Pixels = px(160.);
const SIDEBAR_MAX_WIDTH: Pixels = px(520.);
const ROW_HEIGHT: Pixels = px(22.);
const LIST_OVERDRAW: Pixels = px(200.);
const SIZE_COLUMN_WIDTH: Pixels = px(90.);
const TYPE_COLUMN_WIDTH: Pixels = px(70.);

pub struct BrowserPanel {
    source: BrowserSource,
    state: PaneState,
    /// Directory selected in the tree. `None` means the root.
    selected_dir: Option<String>,
    tree_state: Entity<TreeState>,
    /// Absolute directory path per tree row id, so a click resolves without
    /// re-walking the tree.
    row_paths: Rc<HashMap<SharedString, String>>,
    /// The VFS paths currently queued, so each row can show whether it is in
    /// the queue. Pushed down by the tab that owns the queue; this pane never
    /// holds the queue itself.
    queued: Rc<HashSet<String>>,
    filter_state: Entity<InputState>,
    filter_text: String,
    /// The rows the listing draws, rebuilt only when the VFS, the selected
    /// directory or the filter changes. Rebuilding per frame would re-read the
    /// directory, or re-allocate a path string per file across the whole
    /// install, on every caret blink.
    rows: Rc<Vec<ListingEntry>>,
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

        let subscriptions = vec![
            cx.subscribe(&filter_state, Self::on_filter_event),
            cx.subscribe(&search_state, Self::on_search_input_event),
        ];

        Self {
            source,
            state: PaneState::Empty,
            selected_dir: None,
            tree_state,
            row_paths: Rc::new(HashMap::new()),
            queued: Rc::new(HashSet::new()),
            filter_state,
            filter_text: String::new(),
            rows: Rc::new(Vec::new()),
            search_state,
            path_filter_state,
            list_state: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Marks the pane as waiting on a background load (assets.bin).
    pub fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.state = PaneState::Loading;
        self.rebuild_rows(cx);
    }

    pub fn set_failed(&mut self, reason: String, cx: &mut Context<Self>) {
        self.state = PaneState::Failed(reason);
        self.rebuild_rows(cx);
    }

    /// Adopts a VFS: walks it into the folder tree and the flat file list.
    /// Both walks are O(tree) and happen once per load, never per frame.
    ///
    /// The filter is cleared with the rest: a filter typed against the old
    /// build would otherwise silently re-apply to the new one.
    pub fn set_vfs(&mut self, vfs: VfsPath, window: &mut Window, cx: &mut Context<Self>) {
        let folder_tree = build_folder_tree(&vfs, "");
        let files = Arc::new(build_file_list(&vfs));
        self.state = PaneState::Ready(Loaded { vfs, files, folder_tree });
        self.selected_dir = None;
        self.clear_filter(window, cx);
        self.rebuild_tree(cx);
        self.rebuild_rows(cx);
    }

    /// Drops the loaded VFS, e.g. when the selected build changes.
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state = PaneState::Empty;
        self.selected_dir = None;
        self.clear_filter(window, cx);
        self.rebuild_tree(cx);
        self.rebuild_rows(cx);
    }

    fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_text.clear();
        self.filter_state.update(cx, |state, cx| state.set_value("", window, cx));
    }

    fn on_filter_event(&mut self, _state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        let InputEvent::Change = event else { return };
        let text = self.filter_state.read(cx).value().to_string();
        if text == self.filter_text {
            return;
        }
        self.filter_text = text;
        self.rebuild_rows(cx);
    }

    /// Enter in the query box runs the search, as it does in the egui pane.
    fn on_search_input_event(&mut self, _state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if matches!(event, InputEvent::PressEnter { .. }) {
            self.start_search(cx);
        }
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

    /// Recomputes the listing: filter results while a long enough filter is
    /// typed, otherwise the selected directory's own entries.
    fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let rows = match &self.state {
            PaneState::Ready(loaded) if is_filtering(&self.filter_text) => {
                filtered_entries(&loaded.files, &self.filter_text)
            }
            PaneState::Ready(loaded) => {
                directory_entries(&loaded.vfs, self.selected_dir.as_deref().unwrap_or(ROOT_PATH))
            }
            _ => Vec::new(),
        };

        self.list_state.reset(rows.len());
        self.rows = Rc::new(rows);
        cx.notify();
    }

    /// Adopts the tab's current queue so each listing row can show whether it
    /// is already in it.
    pub fn set_queued(&mut self, queued: Rc<HashSet<String>>, cx: &mut Context<Self>) {
        self.queued = queued;
        cx.notify();
    }

    fn select_dir(&mut self, path: String, cx: &mut Context<Self>) {
        if self.selected_dir.as_deref() == Some(path.as_str()) {
            return;
        }
        self.selected_dir = Some(path);
        self.rebuild_rows(cx);
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

impl Focusable for BrowserPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for BrowserPanel {
    fn panel_name(&self) -> &'static str {
        "UnpackerBrowserPanel"
    }

    /// Not closable: nothing reopens it, so closing one would leave the
    /// Unpacker permanently short a browser. The egui pane refuses for the
    /// same reason.
    fn closable(&self, _cx: &App) -> bool {
        false
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
                .child(div().text_sm().text_color(crate::theme::text_dim()).child(status))
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
                    .flex_1()
                    .min_w(px(0.)),
            );

        let row_paths = self.row_paths.clone();
        let entity = cx.entity();
        let folder_tree = tree(&self.tree_state, move |ix, entry, selected, _window, cx| {
            render_folder_row(ix, entry, selected, &row_paths, entity.clone(), cx)
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
                        .flex_1()
                        .min_w(px(0.)),
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

        let rows = self.rows.clone();
        let listing_entity = cx.entity();
        let hover_bg = cx.theme().accent;
        let queued = self.queued.clone();
        let render_row = {
            let rows = rows.clone();
            move |ix: usize, _window: &mut Window, cx: &mut App| {
                let dim = crate::theme::text_dim();
                let Some(row) = rows.get(ix) else {
                    return div().into_any_element();
                };
                let entity = listing_entity.clone();
                let path = row.path.clone();
                let is_dir = row.is_dir;
                let size = row.size.map(format_size).unwrap_or_default();
                // Only an assets.bin prototype the decoder understands offers
                // the JSON view.
                let decodable = !is_dir && decodable_prototype(&row.label).is_some();
                let viewable = !is_dir && is_viewable(&row.label);
                let view_path = path.clone();
                let view_entity = entity.clone();
                let json_path = path.clone();
                let json_entity = entity.clone();
                let is_queued = queued.contains(path.as_str());
                h_flex()
                    .id(ix)
                    .w_full()
                    .h(ROW_HEIGHT)
                    .gap_1()
                    .items_center()
                    .px_2()
                    .when_some(crate::ui::stripe(ix, cx), |el, color| el.bg(color))
                    .hover(|this| this.bg(hover_bg))
                    .child(queue_toggle(ix, path.clone(), is_dir, is_queued, entity.clone()))
                    .child(file_glyph(&row.label, is_dir))
                    .child(div().flex_1().text_sm().truncate().child(row.label.clone()))
                    .child(div().w(SIZE_COLUMN_WIDTH).text_xs().text_color(dim).child(size))
                    .child(div().w(TYPE_COLUMN_WIDTH).text_xs().text_color(dim).child(row.type_label()))
                    .on_click(move |event, _window, cx| {
                        if event.click_count() < 2 {
                            return;
                        }
                        entity.update(cx, |this, cx| {
                            if is_dir {
                                // The entry's own VFS path is already the
                                // absolute path the tree selects by.
                                this.select_dir(path.as_str().to_string(), cx);
                            } else {
                                cx.emit(BrowserEvent::View { path: path.clone() });
                            }
                        });
                    })
                    .context_menu(move |mut menu, _window, _cx| {
                        // A file the viewer understands opens in it, whatever
                        // kind it is; a prototype the decoder understands
                        // additionally reads and writes as JSON, which is the
                        // set the egui listing offers (`ui/file_unpacker.rs`).
                        if viewable {
                            let path = view_path.clone();
                            let entity = view_entity.clone();
                            menu =
                                menu.item(PopupMenuItem::new("View contents").on_click(move |_event, _window, cx| {
                                    let path = path.clone();
                                    entity.update(cx, |_this, cx| cx.emit(BrowserEvent::View { path }));
                                }));
                        }
                        if !decodable {
                            return menu;
                        }
                        let path = json_path.clone();
                        let entity = json_entity.clone();
                        let extract_path = json_path.clone();
                        let extract_entity = json_entity.clone();
                        menu.item(PopupMenuItem::new("View as JSON").on_click(move |_event, _window, cx| {
                            let path = path.clone();
                            entity.update(cx, |_this, cx| cx.emit(BrowserEvent::ViewAsJson { path }));
                        }))
                        .item(PopupMenuItem::new("Extract as JSON").on_click(
                            move |_event, _window, cx| {
                                let path = extract_path.clone();
                                extract_entity.update(cx, |_this, cx| cx.emit(BrowserEvent::ExtractAsJson { path }));
                            },
                        ))
                    })
                    .into_any_element()
            }
        };

        let queue_rows = rows.clone();
        // The columns are named, as they are in the egui listing: a bare
        // number and a bare word at the end of a row say nothing about which
        // is the size and which the type.
        let column_header = h_flex()
            .flex_none()
            .h(ROW_HEIGHT)
            .gap_1()
            .items_center()
            .px_2()
            .bg(crate::theme::surface())
            .border_b_1()
            .border_color(border)
            .child(div().w(QUEUE_COLUMN_WIDTH))
            .child(div().w(GLYPH_COLUMN_WIDTH))
            .child(div().flex_1().text_xs().font_weight(FontWeight::BOLD).child("Name"))
            .child(div().w(SIZE_COLUMN_WIDTH).text_xs().font_weight(FontWeight::BOLD).child("Size"))
            .child(div().w(TYPE_COLUMN_WIDTH).text_xs().font_weight(FontWeight::BOLD).child("Type"));

        let dim = crate::theme::text_dim();
        let listing_header = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(div().flex_1().text_xs().text_color(dim).child(format!("{} items", rows.len())))
            .child(
                Button::new(SharedString::from(format!("unpacker-{fragment}-extract-listed")))
                    .icon(IconName::HardDrive)
                    .label("Queue listed files")
                    .compact()
                    .disabled(rows.is_empty())
                    .on_click(cx.listener(move |_this, _event, _window, cx| {
                        cx.emit(BrowserEvent::Extract(queue_rows.as_ref().clone()));
                    })),
            );

        let listing = v_flex()
            .size_full()
            .child(breadcrumbs(self.selected_dir.as_deref().unwrap_or(ROOT_PATH), cx.entity()))
            .child(listing_header)
            .child(column_header)
            .child(
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

/// One listing row's queue control. A file gets a checkbox, a directory a
/// plus/remove button, matching the egui listing: a directory is queued whole
/// rather than ticked like a single file.
fn queue_toggle(ix: usize, path: VfsPath, is_dir: bool, is_queued: bool, entity: Entity<BrowserPanel>) -> AnyElement {
    let emit = move |cx: &mut App| {
        let path = path.clone();
        entity.update(cx, |_this, cx| {
            if is_queued {
                cx.emit(BrowserEvent::Unqueue(path));
            } else {
                cx.emit(BrowserEvent::Queue(path));
            }
        });
    };

    if is_dir {
        let (icon, tooltip) = if is_queued {
            (IconName::Close, "Remove this folder from the queue")
        } else {
            (IconName::Plus, "Queue this folder")
        };
        return Button::new(("queue-toggle", ix))
            .icon(icon)
            .compact()
            .tooltip(tooltip)
            .on_click(move |_event, _window, cx: &mut App| emit(cx))
            .into_any_element();
    }

    Checkbox::new(("queue-toggle", ix))
        .checked(is_queued)
        .on_click(move |_checked: &bool, _window, cx: &mut App| emit(cx))
        .into_any_element()
}

/// The path bar above the listing: "res" and then one segment per directory,
/// each navigating to that level. Mirrors the egui app's breadcrumb row.
fn breadcrumbs(selected_dir: &str, entity: Entity<BrowserPanel>) -> impl IntoElement {
    let mut crumbs: Vec<AnyElement> = vec![crumb("res", ROOT_PATH.to_string(), 0, entity.clone())];

    let mut accumulated = String::new();
    for (depth, part) in selected_dir.trim_matches('/').split('/').filter(|part| !part.is_empty()).enumerate() {
        accumulated.push('/');
        accumulated.push_str(part);
        crumbs.push(div().flex_none().text_xs().text_color(crate::theme::text_faint()).child("/").into_any_element());
        crumbs.push(crumb(part, accumulated.clone(), depth + 1, entity.clone()));
    }

    h_flex().flex_none().gap_1().items_center().px_2().py_1().children(crumbs)
}

/// One breadcrumb segment. `depth` only distinguishes the element ids, since
/// two levels can share a directory name.
fn crumb(label: &str, path: String, depth: usize, entity: Entity<BrowserPanel>) -> AnyElement {
    Button::new(("breadcrumb", depth))
        .label(SharedString::from(label.to_string()))
        .compact()
        .ghost()
        .on_click(move |_event, _window, cx: &mut App| {
            let path = path.clone();
            entity.update(cx, |this, cx| this.select_dir(path, cx));
        })
        .into_any_element()
}

/// Byte count in the largest unit that keeps it under four digits.
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{size:.1} {}", UNITS[unit]) }
}

/// What one level of the tree indents by, and the width its guide is drawn in.
const INDENT: Pixels = px(16.);

/// The two columns the listing header has to line its labels up with.
const QUEUE_COLUMN_WIDTH: Pixels = px(20.);
const GLYPH_COLUMN_WIDTH: Pixels = px(16.);

/// Whether the file viewer has anything to show for this name.
///
/// The same extensions the egui listing offers "View Contents" for; the
/// viewer itself decides what it makes of the bytes.
fn is_viewable(label: &str) -> bool {
    let extension = label.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    matches!(extension.as_str(), "xml" | "json" | "txt" | "cfg" | "log" | "csv" | "md" | "jpg" | "jpeg" | "png" | "svg")
}

/// The glyph for a listing row: a directory, or the kind of file its name
/// says it is, from the same four the egui listing tells apart
/// (`ui/file_unpacker.rs`'s `file_icon_rich_text`).
fn file_glyph(label: &str, is_dir: bool) -> impl IntoElement {
    if is_dir {
        return crate::icons::icon(crate::icons::FOLDER).text_color(crate::theme::icon_accent());
    }
    let extension = label.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    let glyph = match extension.as_str() {
        "png" | "jpg" | "jpeg" | "dds" | "svg" | "tga" | "bmp" => crate::icons::IMAGE,
        "txt" | "xml" | "json" | "cfg" | "log" | "csv" | "md" => crate::icons::FILE_TEXT,
        "wem" | "bnk" | "ogg" | "wav" | "mp3" => crate::icons::MUSIC_NOTE,
        _ => crate::icons::FILE,
    };
    crate::icons::icon(glyph)
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
    cx: &App,
) -> ListItem {
    let item = entry.item();
    // The chevron says whether a directory is open; the folder glyph follows
    // it rather than following whether the directory has children, which is
    // what it used to do -- a closed parent drew as open.
    let mut row = h_flex().gap_1().items_center().child(crate::ui::indent_guides(entry.depth(), INDENT, cx));
    if entry.is_folder() {
        let chevron = if entry.is_expanded() { IconName::ChevronDown } else { IconName::ChevronRight };
        row = row.child(Icon::new(chevron));
    } else {
        row = row.child(div().w(INDENT));
    }
    let folder = if entry.is_expanded() { IconName::FolderOpen } else { IconName::Folder };
    let row = row
        .child(Icon::new(folder).text_color(crate::theme::icon_accent()))
        .child(div().text_sm().child(item.label.clone()));

    let list_item = crate::ui::tree_row(ListItem::new(ix).selected(selected).child(row), cx);
    match row_paths.get(&item.id).cloned() {
        Some(path) => list_item.on_click(move |_event: &ClickEvent, _window, cx: &mut App| {
            panel.update(cx, |this, cx| this.select_dir(path.clone(), cx));
        }),
        None => list_item,
    }
}

#[cfg(test)]
mod format_tests {
    use super::format_size;

    #[test]
    fn sizes_are_shown_in_the_largest_unit_that_keeps_them_under_four_digits() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(1536 * 1024), "1.5 MiB");
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AppContext;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt;
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::io::Write as _;
    use std::rc::Rc;
    use wowsunpack::vfs::MemoryFS;
    use wowsunpack::vfs::VfsPath;

    use super::BrowserEvent;
    use super::BrowserPanel;
    use super::BrowserSource;

    /// A package tree with one nested directory and two files, enough to
    /// exercise the breadcrumb path and both kinds of queue control.
    fn fixture() -> VfsPath {
        let root: VfsPath = MemoryFS::new().into();
        root.join("content/gameplay").unwrap().create_dir_all().unwrap();
        root.join("content/a.xml").unwrap().create_file().unwrap().write_all(b"<a/>").unwrap();
        root.join("content/gameplay/b.xml").unwrap().create_file().unwrap().write_all(b"<b/>").unwrap();
        root
    }

    fn open_pane(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<BrowserPanel> {
        cx.update(gpui_kit::init);
        cx.open_window(size(px(900.), px(600.)), |window, cx| {
            let mut pane = BrowserPanel::new(BrowserSource::Pkg, window, cx);
            pane.set_vfs(fixture(), window, cx);
            pane
        })
    }

    #[gpui_kit::test]
    fn the_breadcrumbs_follow_the_selected_directory(cx: &mut TestAppContext) {
        let window = open_pane(cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(("breadcrumb", 0usize)).label(), Some("res"), "the root crumb is always there");
            assert!(window.try_find(("breadcrumb", 1usize)).is_none(), "the root has no deeper crumb");
        })
        .expect("the window is open");

        window
            .update(cx, |pane, _window, cx| pane.select_dir("/content/gameplay".to_string(), cx))
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(("breadcrumb", 1usize)).label(), Some("content"));
            assert_eq!(window.find(("breadcrumb", 2usize)).label(), Some("gameplay"));
        })
        .expect("the window is open");
    }

    #[gpui_kit::test]
    fn clicking_a_breadcrumb_navigates_to_that_level(cx: &mut TestAppContext) {
        let window = open_pane(cx);
        window
            .update(cx, |pane, _window, cx| pane.select_dir("/content/gameplay".to_string(), cx))
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("breadcrumb", 1usize), cx);
            assert!(window.try_find(("breadcrumb", 2usize)).is_none(), "clicking 'content' drops the level below it");
            assert_eq!(window.find(("breadcrumb", 1usize)).label(), Some("content"));
        })
        .expect("the window is open");
    }

    #[gpui_kit::test]
    fn a_listing_rows_queue_control_reports_what_the_tab_has_queued(cx: &mut TestAppContext) {
        let window = open_pane(cx);
        window.update(cx, |pane, _window, cx| pane.select_dir("/content".to_string(), cx)).expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            // Directories sort first, so row 0 is "gameplay" and row 1 "a.xml".
            assert_eq!(window.find(("queue-toggle", 1usize)).checked(), Some(false), "nothing is queued yet");
        })
        .expect("the window is open");

        let queued: Rc<HashSet<String>> = Rc::new(["/content/a.xml".to_string()].into_iter().collect());
        window.update(cx, |pane, _window, cx| pane.set_queued(queued, cx)).expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(("queue-toggle", 1usize)).checked(), Some(true), "the queued file reads as queued");
        })
        .expect("the window is open");
    }

    #[gpui_kit::test]
    fn clicking_a_rows_queue_control_asks_the_tab_to_queue_that_entry(cx: &mut TestAppContext) {
        let window = open_pane(cx);
        window.update(cx, |pane, _window, cx| pane.select_dir("/content".to_string(), cx)).expect("the window is open");

        let seen: Rc<RefCell<Vec<BrowserEvent>>> = Rc::new(RefCell::new(Vec::new()));
        let pane = window.entity(cx).expect("the window has a root view");
        let recorder = seen.clone();
        let subscription = cx.update(|cx| {
            cx.subscribe(&pane, move |_pane, event: &BrowserEvent, _cx| recorder.borrow_mut().push(event.clone()))
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            // Row 0 is the "gameplay" directory, row 1 the file beside it.
            window.click(("queue-toggle", 0usize), cx);
            window.click(("queue-toggle", 1usize), cx);
        })
        .expect("the window is open");

        let seen = seen.borrow();
        assert!(
            matches!(seen.first(), Some(BrowserEvent::Queue(path)) if path.as_str() == "/content/gameplay"),
            "the folder button queues the folder, got {seen:?}"
        );
        assert!(
            matches!(seen.get(1), Some(BrowserEvent::Queue(path)) if path.as_str() == "/content/a.xml"),
            "the file checkbox queues the file, got {seen:?}"
        );
        drop(subscription);
    }
}
