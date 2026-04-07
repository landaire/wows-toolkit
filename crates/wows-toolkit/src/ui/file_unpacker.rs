use rust_i18n::t;
use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use crate::icons;
use egui::Label;
use egui::RichText;
use egui::Sense;
use egui::Ui;
use egui::UiKind;
use egui::WidgetText;
use egui_dock::DockArea;
use egui_dock::DockState;
use egui_dock::NodeIndex;
use egui_dock::TabViewer;
use egui_dock::tab_viewer::OnCloseResponse;
use parking_lot::Mutex;
use pickled::HashableValue;
use serde::Serialize;
use serde_cbor::ser::IoWrite;
use wowsunpack::data::assets_bin_vfs::AssetsBinVfs;
use wowsunpack::data::assets_bin_vfs::PrototypeType;
use wowsunpack::game_params::convert::game_params_to_pickle;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::vfs::VfsPath;

use crate::app::ToolkitTabViewer;
use crate::ui::plaintext_viewer;
use crate::ui::plaintext_viewer::FileType;
type FilteredFileList = Arc<Vec<(Arc<PathBuf>, VfsPath)>>;

pub static UNPACKER_STOP: AtomicBool = AtomicBool::new(false);

pub struct UnpackerProgress {
    pub file_name: String,
    pub progress: f32,
}

const IMAGE_FILE_TYPES: [&str; 3] = [".jpg", ".png", ".svg"];
const PLAINTEXT_FILE_TYPES: [&str; 3] = [".xml", ".json", ".txt"];

#[derive(Eq, PartialEq)]
enum GameParamsFormat {
    Json,
    Cbor,
    MinimalJson,
    MinimalCbor,
}
/// A single match found during content search.
#[derive(Clone)]
pub struct ContentSearchHit {
    pub vfs_path_str: String,
    pub vfs_path: VfsPath,
    pub context: String,
    pub offset: usize,
}

/// Progress update from the content search background thread.
pub(crate) enum ContentSearchMessage {
    Hit(ContentSearchHit),
    Progress(usize, usize),
    Done,
}
/// Identifies which VFS a browser pane is for.
#[derive(Clone, PartialEq, Eq)]
pub enum BrowserSource {
    Pkg,
    AssetsBin,
}

/// Pre-computed directory tree node for the folder tree sidebar.
/// Built once when the VFS loads, reused every frame to avoid per-frame `read_dir()` calls.
struct FolderTreeNode {
    name: String,
    path: String,
    children: Vec<FolderTreeNode>,
}

/// State for a single file browser pane (directory tree + file listing).
pub struct BrowserPane {
    pub source: BrowserSource,
    /// The VFS root for this browser. None while loading (assets.bin background parse).
    pub vfs: Option<VfsPath>,
    /// Flat file list for filtering/searching. None while loading.
    pub files: Option<Vec<(Arc<PathBuf>, VfsPath)>>,
    /// Currently selected directory in the folder tree.
    pub selected_dir: Option<String>,
    /// Whether this pane is still loading its VFS (assets.bin only).
    pub loading: bool,
    /// Error message if VFS failed to load.
    pub error: Option<String>,
    /// Content search query text.
    pub content_search_query: String,
    /// Path filter for content search (glob pattern).
    pub content_search_path_filter: String,
    /// Per-pane path filter text.
    pub filter: String,
    /// Last-applied filter (to detect changes).
    pub used_filter: Option<String>,
    /// Cached filtered file list.
    pub filtered_file_list: Option<FilteredFileList>,
    /// Cached directory entries: (dir_path, entries). Invalidated when selected_dir changes.
    cached_dir_entries: Option<(String, Vec<FileEntry>)>,
    /// Cached folder tree structure. Built once when VFS loads.
    cached_folder_tree: Option<Vec<FolderTreeNode>>,
}

impl BrowserPane {
    fn new_pkg() -> Self {
        Self {
            source: BrowserSource::Pkg,
            vfs: None,
            files: None,
            selected_dir: None,
            loading: false,
            error: None,
            content_search_query: String::new(),
            content_search_path_filter: String::new(),
            filter: String::new(),
            used_filter: None,
            filtered_file_list: None,
            cached_dir_entries: None,
            cached_folder_tree: None,
        }
    }

    fn new_assets_bin() -> Self {
        Self {
            source: BrowserSource::AssetsBin,
            vfs: None,
            files: None,
            selected_dir: None,
            loading: true,
            error: None,
            content_search_query: String::new(),
            content_search_path_filter: String::new(),
            filter: String::new(),
            used_filter: None,
            filtered_file_list: None,
            cached_dir_entries: None,
            cached_folder_tree: None,
        }
    }

    /// Reset filter state (called when game data reloads).
    pub fn reset_filter(&mut self) {
        self.filter.clear();
        self.used_filter = None;
        self.filtered_file_list = None;
        self.cached_dir_entries = None;
    }
}

/// A single pane in the unpacker's inner dock.
pub enum UnpackerPane {
    /// A file browser pane. Each VFS source gets its own tab.
    Browser(BrowserPane),
    /// A content search results tab.
    Search(ContentSearchTab),
}

/// State for one content search tab.
pub struct ContentSearchTab {
    pub id: u64,
    pub query: String,
    /// Which VFS source this search was run against.
    pub source: BrowserSource,
    pub stop_flag: Arc<AtomicBool>,
    pub rx: Option<mpsc::Receiver<ContentSearchMessage>>,
    pub results: Vec<ContentSearchHit>,
    pub progress: (usize, usize),
    pub running: bool,
}

impl Drop for ContentSearchTab {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

/// Result sent from the background assets.bin loading thread.
pub(crate) struct AssetsBinLoadResult {
    pub vfs: VfsPath,
    pub files: Vec<(Arc<PathBuf>, VfsPath)>,
}

/// State for the explorer-style resource browser.
pub struct ResourceBrowserState {
    /// Whether to show the extraction queue popover.
    pub show_queue_popover: bool,
    /// Inner dock state for browser + search result tabs.
    pub dock_state: DockState<UnpackerPane>,
    /// Next unique ID for search tabs.
    pub next_search_id: u64,
    /// Receiver for background assets.bin parse result.
    pub assets_bin_rx: Option<mpsc::Receiver<Result<AssetsBinLoadResult, String>>>,
    /// Build number the assets.bin loading was initiated for.
    pub assets_bin_loading_build: Option<u32>,
    /// When true, batch extraction decodes supported Assets.bin prototypes to JSON.
    pub decode_prototypes_as_json: bool,
}

impl Default for ResourceBrowserState {
    fn default() -> Self {
        Self {
            show_queue_popover: false,
            dock_state: DockState::new(vec![
                UnpackerPane::Browser(BrowserPane::new_pkg()),
                UnpackerPane::Browser(BrowserPane::new_assets_bin()),
            ]),
            next_search_id: 0,
            assets_bin_rx: None,
            assets_bin_loading_build: None,
            decode_prototypes_as_json: false,
        }
    }
}

impl ResourceBrowserState {
    /// Reset filter state on all browser panes (called when game data reloads).
    pub fn reset_filters(&mut self) {
        for (_, pane) in self.dock_state.iter_all_tabs_mut() {
            if let UnpackerPane::Browser(browser) = pane {
                browser.reset_filter();
            }
        }
    }
}

/// A file entry displayed in the right-side file listing.
struct FileEntry {
    name: String,
    is_dir: bool,
    size: u64,
    vfs_path: VfsPath,
}
/// Per-frame viewer implementing `egui_dock::TabViewer` for unpacker panes.
struct UnpackerPaneViewer<'a> {
    items_to_extract: &'a Mutex<Vec<VfsPath>>,
    file_viewer: &'a Mutex<Vec<plaintext_viewer::PlaintextFileViewer>>,
    /// Deferred navigation signal: (source, path) set by search results "Go to Directory".
    navigate_to: &'a std::cell::Cell<Option<(BrowserSource, String)>>,
    /// Deferred filter clear signal: which browser source to clear.
    clear_filter: &'a std::cell::Cell<Option<BrowserSource>>,
    /// Deferred signal to start a content search from a browser pane.
    start_search: &'a std::cell::Cell<Option<BrowserSource>>,
}

impl TabViewer for UnpackerPaneViewer<'_> {
    type Tab = UnpackerPane;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        match tab {
            UnpackerPane::Browser(b) => match b.source {
                BrowserSource::Pkg => egui::Id::new("unpacker_pkg_browser"),
                BrowserSource::AssetsBin => egui::Id::new("unpacker_assetsbin_browser"),
            },
            UnpackerPane::Search(s) => egui::Id::new(("unpacker_search_pane", s.id)),
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        match tab {
            UnpackerPane::Browser(b) => {
                let label: String = match b.source {
                    BrowserSource::Pkg => wt_translations::icon_t(icons::ARCHIVE, "PKG"),
                    BrowserSource::AssetsBin => wt_translations::icon_t(icons::DATABASE, &t!("ui.unpacker.assets_bin")),
                };
                label.into()
            }
            UnpackerPane::Search(s) => {
                let status = if s.running { " ..." } else { "" };
                format!("{} {}{}", icons::MAGNIFYING_GLASS, s.query, status).into()
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            UnpackerPane::Browser(browser) => {
                self.render_browser_pane(ui, browser);
            }
            UnpackerPane::Search(search) => {
                self.render_search_pane(ui, search);
            }
        }
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        matches!(tab, UnpackerPane::Search(_))
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        if let UnpackerPane::Search(s) = tab {
            s.stop_flag.store(true, Ordering::Relaxed);
        }
        OnCloseResponse::Close
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }
}

impl UnpackerPaneViewer<'_> {
    /// Render the file browser pane (folder tree + directory listing or filter results).
    fn render_browser_pane(&self, ui: &mut Ui, browser: &mut BrowserPane) {
        // Loading state (assets.bin)
        if browser.loading {
            ui.vertical_centered(|ui| {
                let avail = ui.available_height();
                ui.add_space(avail / 2.0 - 20.0);
                ui.spinner();
                ui.label(RichText::new(t!("ui.unpacker.loading_assets").as_ref()).weak());
            });
            return;
        }

        // Error state
        if let Some(err) = &browser.error {
            ui.vertical_centered(|ui| {
                let avail = ui.available_height();
                ui.add_space(avail / 2.0 - 10.0);
                ui.label(
                    RichText::new(t!("ui.unpacker.load_failed", error = err).as_ref())
                        .color(egui::Color32::from_rgb(220, 80, 80)),
                );
            });
            return;
        }

        // No VFS available
        if browser.vfs.is_none() {
            ui.vertical_centered(|ui| {
                let avail = ui.available_height();
                ui.add_space(avail / 2.0 - 10.0);
                ui.label(RichText::new(t!("ui.unpacker.no_game_data").as_ref()).weak());
            });
            return;
        }

        let source_id = match browser.source {
            BrowserSource::Pkg => "pkg",
            BrowserSource::AssetsBin => "assetsbin",
        };

        // Recompute filter if needed
        recompute_filter(browser);

        // Per-pane folder tree in a left side panel
        egui::Panel::left(format!("browser_folder_tree_{}", source_id))
            .default_size(260.0)
            .resizable(true)
            .show_inside(ui, |ui| {
                // Filter input
                ui.horizontal(|ui| {
                    ui.label(icons::FUNNEL);
                    let response = ui
                        .add(egui::TextEdit::singleline(&mut browser.filter).hint_text(t!("ui.unpacker.filter_files")));
                    if response.changed() {
                        browser.used_filter = None;
                    }
                });
                ui.separator();

                // Content search input (at bottom of sidebar)
                egui::Panel::bottom(format!("browser_content_search_{}", source_id)).show_inside(ui, |ui| {
                    ui.add_space(2.0);
                    ui.label(RichText::new(t!("ui.unpacker.search_in_files").as_ref()).strong());
                    ui.horizontal(|ui| {
                        ui.label(icons::MAGNIFYING_GLASS);
                        let search_response = ui.add(
                            egui::TextEdit::singleline(&mut browser.content_search_query)
                                .hint_text(t!("ui.unpacker.search_hint")),
                        );
                        if search_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.start_search.set(Some(browser.source.clone()));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut browser.content_search_path_filter)
                                .hint_text(t!("ui.unpacker.path_filter_hint")),
                        );
                    });
                });

                // Folder tree
                egui::ScrollArea::both().id_salt(format!("folder_tree_scroll_{}", source_id)).show(ui, |ui| {
                    if let Some(cached_tree) = &browser.cached_folder_tree {
                        let tree_id = ui.make_persistent_id(format!("resource_folder_tree_{}", source_id));

                        let mut id_to_path: std::collections::HashMap<egui::Id, String> =
                            std::collections::HashMap::new();

                        let root_id = egui::Id::new(("browser_dir", source_id, "/"));
                        id_to_path.insert(root_id, "/".to_string());

                        let tree = egui_ltreeview::TreeView::new(tree_id);

                        let (_response, actions) = tree.show(ui, |builder| {
                            let root_node = egui_ltreeview::NodeBuilder::dir(root_id)
                                .default_open(true)
                                .icon(|ui| {
                                    ui.label(
                                        RichText::new(icons::FOLDER_OPEN).color(egui::Color32::from_rgb(200, 180, 120)),
                                    );
                                })
                                .label("res");

                            let is_open = builder.node(root_node);
                            if is_open {
                                render_folder_tree(builder, cached_tree, &mut id_to_path, source_id);
                            }
                            builder.close_dir();
                        });

                        for action in actions {
                            match action {
                                egui_ltreeview::Action::SetSelected(selected_ids) => {
                                    if let Some(path) = selected_ids.first().and_then(|id| id_to_path.get(id)) {
                                        browser.selected_dir = Some(path.clone());
                                    }
                                }
                                egui_ltreeview::Action::Activate(activate) => {
                                    if let Some(path) = activate.selected.first().and_then(|id| id_to_path.get(id)) {
                                        browser.selected_dir = Some(path.clone());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                });
            });

        // Main content area
        let selected_dir = browser.selected_dir.clone().unwrap_or_else(|| "/".to_string());
        let is_filtering = browser.filter.len() >= 3;

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if is_filtering {
                // ── Filter results mode ──
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} Filter: \"{}\"", icons::FUNNEL, browser.filter)).strong());
                });
                ui.separator();

                if let Some(filtered_files) = &browser.filtered_file_list {
                    let items_snapshot = self.items_to_extract.lock().clone();
                    let queued_paths: HashSet<String> = items_snapshot.iter().map(|v| v.as_str().to_string()).collect();

                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} results", filtered_files.len())).weak());
                        if !filtered_files.is_empty()
                            && ui
                                .small_button(wt_translations::icon_t(icons::PLUS_CIRCLE, &t!("ui.unpacker.queue_all")))
                                .clicked()
                        {
                            for file in filtered_files.iter() {
                                queue_extract(self.items_to_extract, file.1.clone());
                            }
                        }
                    });
                    ui.separator();

                    egui::ScrollArea::both().id_salt(format!("filter_results_scroll_{}", source_id)).show(ui, |ui| {
                        render_filter_results_table(
                            ui,
                            filtered_files,
                            &queued_paths,
                            self.items_to_extract,
                            self.file_viewer,
                            self.navigate_to,
                            self.clear_filter,
                            &browser.source,
                        );
                    });
                }
            } else {
                // ── Directory browsing mode ──
                ui.horizontal(|ui| {
                    let parts: Vec<&str> = selected_dir.split('/').filter(|s| !s.is_empty()).collect();

                    let source = browser.source.clone();
                    if ui
                        .add(
                            Label::new(RichText::new(format!("{} res", icons::FOLDER_OPEN)).strong())
                                .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        self.navigate_to.set(Some((source.clone(), "/".to_string())));
                    }

                    let mut accumulated_path = String::new();
                    for part in &parts {
                        ui.label(RichText::new(icons::CARET_RIGHT).weak());
                        accumulated_path = format!("{}/{}", accumulated_path, part);
                        let path_clone = accumulated_path.clone();
                        if ui.add(Label::new(RichText::new(*part).strong()).sense(Sense::click())).clicked() {
                            self.navigate_to.set(Some((source.clone(), path_clone)));
                        }
                    }
                });
                ui.separator();

                if let Some(vfs) = &browser.vfs {
                    // Use cached dir entries to avoid per-frame read_dir/metadata calls
                    let needs_refresh =
                        browser.cached_dir_entries.as_ref().is_none_or(|(cached_dir, _)| cached_dir != &selected_dir);
                    if needs_refresh {
                        let entries = get_dir_entries(vfs, &selected_dir);
                        browser.cached_dir_entries = Some((selected_dir.clone(), entries));
                    }
                    let entries = &browser.cached_dir_entries.as_ref().unwrap().1;

                    let items_snapshot = self.items_to_extract.lock().clone();
                    let queued_paths: HashSet<String> = items_snapshot.iter().map(|v| v.as_str().to_string()).collect();

                    if entries.is_empty() {
                        ui.centered_and_justified(|ui| {
                            ui.label(RichText::new(t!("ui.unpacker.empty_directory").as_ref()).weak());
                        });
                    } else {
                        let file_entries: Vec<&FileEntry> = entries.iter().filter(|e| !e.is_dir).collect();
                        if !file_entries.is_empty() {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{} items", entries.len())).weak());
                                if ui
                                    .small_button(wt_translations::icon_t(
                                        icons::PLUS_CIRCLE,
                                        &t!("ui.unpacker.queue_all_files"),
                                    ))
                                    .clicked()
                                {
                                    for entry in &file_entries {
                                        queue_extract(self.items_to_extract, entry.vfs_path.clone());
                                    }
                                }
                            });
                            ui.separator();
                        }

                        egui::ScrollArea::both().id_salt(format!("file_listing_scroll_{}", source_id)).show(ui, |ui| {
                            render_file_listing_table(
                                ui,
                                entries,
                                &queued_paths,
                                self.items_to_extract,
                                self.file_viewer,
                                self.navigate_to,
                                &browser.source,
                            );
                        });
                    }
                }
            }
        });
    }

    /// Render a content search results pane.
    fn render_search_pane(&self, ui: &mut Ui, search: &mut ContentSearchTab) {
        // Header bar
        ui.horizontal(|ui| {
            let results_count = search.results.len();
            let (searched, total) = search.progress;

            if search.running {
                ui.spinner();
                ui.label(RichText::new(format!("{}/{} files, {} hits", searched, total, results_count)).weak());

                if ui
                    .button(RichText::new(icons::X_CIRCLE).color(egui::Color32::from_rgb(220, 80, 80)))
                    .on_hover_text(t!("ui.unpacker.stop_search_tooltip"))
                    .clicked()
                {
                    search.stop_flag.store(true, Ordering::Relaxed);
                }
            } else {
                ui.label(RichText::new(format!("{} hits in {} files searched", results_count, total)).weak());
            }
        });

        // Progress bar (while running)
        if search.running {
            let (searched, total) = search.progress;
            let fraction = if total > 0 { searched as f32 / total as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(fraction).text(format!("{searched}/{total} files")));
        }

        ui.separator();

        // Results table
        if search.results.is_empty() {
            if !search.running {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new(t!("ui.unpacker.no_matches").as_ref()).weak());
                });
            }
        } else {
            let results = search.results.clone();
            render_search_results_table(
                ui,
                &results,
                self.items_to_extract,
                self.file_viewer,
                self.navigate_to,
                self.clear_filter,
                &search.source,
            );
        }
    }
}
/// Push a VfsPath into the extraction queue if it's not already queued.
fn queue_extract(items: &Mutex<Vec<VfsPath>>, path: VfsPath) {
    let mut items = items.lock();
    if !items.iter().any(|v| v.as_str() == path.as_str()) {
        items.push(path);
    }
}

/// Get a RichText icon for a file based on its name and type.
fn file_icon_rich_text(name: &str, is_dir: bool) -> RichText {
    if is_dir {
        return RichText::new(icons::FOLDER).color(egui::Color32::from_rgb(200, 180, 120));
    }
    let icon =
        if IMAGE_FILE_TYPES.iter().any(|ext| name.ends_with(ext)) || name.ends_with(".dds") || name.ends_with(".pvr") {
            icons::IMAGE
        } else if PLAINTEXT_FILE_TYPES.iter().any(|ext| name.ends_with(ext)) {
            icons::FILE_TEXT
        } else if name.ends_with(".mp3")
            || name.ends_with(".ogg")
            || name.ends_with(".wav")
            || name.ends_with(".wem")
            || name.ends_with(".fev")
            || name.ends_with(".fsb")
        {
            icons::MUSIC_NOTE
        } else {
            icons::FILE
        };
    RichText::new(icon)
}

/// Open a file viewer window for a VFS node (plaintext or image).
fn open_file_viewer(file_viewer: &Mutex<Vec<plaintext_viewer::PlaintextFileViewer>>, node: &VfsPath) {
    let filename = node.filename();
    let is_plaintext_file = PLAINTEXT_FILE_TYPES.iter().find(|extension| filename.ends_with(**extension));
    let is_image_file = IMAGE_FILE_TYPES.iter().find(|extension| filename.ends_with(**extension));

    if is_plaintext_file.is_some() || is_image_file.is_some() {
        let mut file_contents = Vec::new();
        if let Ok(mut reader) = node.open_file() {
            let _ = reader.read_to_end(&mut file_contents);
        }

        let file_type = match (is_plaintext_file, is_image_file) {
            (Some(ext), None) => String::from_utf8(file_contents)
                .ok()
                .map(|contents| FileType::PlainTextFile { ext: ext.to_string(), contents }),
            (None, Some(_ext)) => Some(FileType::Image { contents: file_contents }),
            _ => None,
        };

        if let Some(file_type) = file_type {
            let path_str = node.as_str().trim_start_matches('/');
            let viewer = plaintext_viewer::PlaintextFileViewer {
                title: Arc::new(format!("res/{path_str}")),
                file_info: Arc::new(Mutex::new(file_type)),
                open: Arc::new(AtomicBool::new(true)),
            };
            file_viewer.lock().push(viewer);
        }
    }
}

/// Infer a decodable `PrototypeType` from a filename's extension.
/// Returns `Some` only if the extension maps to a type we can decode to JSON.
fn decodable_prototype_type(filename: &str) -> Option<PrototypeType> {
    let dot = filename.rfind('.')?;
    let ext = &filename[dot..];
    let pt = PrototypeType::from_extension(ext)?;
    if wowsunpack::models::can_decode_prototype(pt) { Some(pt) } else { None }
}

/// Add a "View Contents" context menu to a response for viewable file types.
/// For Assets.bin files, shows "View as JSON" / "Extract as JSON" for decodable prototype types.
fn add_view_file_context_menu(
    file_viewer: &Mutex<Vec<plaintext_viewer::PlaintextFileViewer>>,
    response: &egui::Response,
    node: &VfsPath,
    source: &BrowserSource,
) {
    if *source == BrowserSource::AssetsBin {
        if let Some(pt) = decodable_prototype_type(&node.filename()) {
            let node_view = node.clone();
            let node_extract = node.clone();
            response.context_menu(|ui| {
                if ui.button(wt_translations::icon_t(icons::EYE, &t!("ui.unpacker.view_as_json"))).clicked() {
                    open_decoded_json_viewer(file_viewer, &node_view, pt);
                    ui.close_kind(UiKind::Menu);
                }
                if ui
                    .button(wt_translations::icon_t(icons::DOWNLOAD_SIMPLE, &t!("ui.unpacker.extract_as_json")))
                    .clicked()
                {
                    extract_single_as_json(&node_extract, pt);
                    ui.close_kind(UiKind::Menu);
                }
            });
        }
        return;
    }

    let filename = node.filename();
    let is_viewable = PLAINTEXT_FILE_TYPES.iter().any(|ext| filename.ends_with(ext))
        || IMAGE_FILE_TYPES.iter().any(|ext| filename.ends_with(ext));

    if is_viewable {
        response.context_menu(|ui| {
            if ui.button(wt_translations::icon_t(icons::EYE, &t!("ui.unpacker.view_contents"))).clicked() {
                open_file_viewer(file_viewer, node);
                ui.close_kind(UiKind::Menu);
            }
        });
    }
}

/// Open a decoded JSON viewer for an Assets.bin prototype record.
fn open_decoded_json_viewer(
    file_viewer: &Mutex<Vec<plaintext_viewer::PlaintextFileViewer>>,
    node: &VfsPath,
    proto_type: PrototypeType,
) {
    let mut data = Vec::new();
    if let Ok(mut reader) = node.open_file() {
        let _ = reader.read_to_end(&mut data);
    }
    match wowsunpack::models::decode_prototype_to_json(&data, proto_type) {
        Ok(json) => {
            let path_str = node.as_str().trim_start_matches('/');
            let viewer = plaintext_viewer::PlaintextFileViewer {
                title: Arc::new(format!("res/{path_str} (JSON)")),
                file_info: Arc::new(Mutex::new(FileType::PlainTextFile { ext: ".json".to_string(), contents: json })),
                open: Arc::new(AtomicBool::new(true)),
            };
            file_viewer.lock().push(viewer);
        }
        Err(e) => {
            eprintln!("Failed to decode {}: {}", node.as_str(), e);
        }
    }
}

/// Extract a single Assets.bin file as decoded JSON via file dialog.
fn extract_single_as_json(node: &VfsPath, proto_type: PrototypeType) {
    let filename = node.filename();
    let json_filename = format!("{filename}.json");

    if let Some(path) = rfd::FileDialog::new().set_file_name(&json_filename).save_file() {
        let mut data = Vec::new();
        if let Ok(mut reader) = node.open_file() {
            let _ = reader.read_to_end(&mut data);
        }
        match wowsunpack::models::decode_prototype_to_json(&data, proto_type) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json.as_bytes()) {
                    eprintln!("Failed to write {}: {}", path.display(), e);
                }
            }
            Err(e) => {
                eprintln!("Failed to decode {}: {}", node.as_str(), e);
            }
        }
    }
}

/// Extract a UTF-8 context snippet around a byte offset in file data.
fn extract_context_snippet(data: &[u8], match_start: usize, match_end: usize, radius: usize) -> String {
    let text = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => {
            let window_start = match_start.saturating_sub(radius * 4);
            let window_end = (match_end + radius * 4).min(data.len());
            let lossy = String::from_utf8_lossy(&data[window_start..window_end]);
            let matched_text = String::from_utf8_lossy(&data[match_start..match_end]);

            if let Some(pos) = lossy.find(matched_text.as_ref()) {
                let char_start = lossy[..pos].chars().count().saturating_sub(radius);
                let chars: Vec<char> = lossy.chars().collect();
                let char_end = (char_start + radius + matched_text.chars().count() + radius).min(chars.len());
                let actual_start = chars.iter().take(char_start).count();
                let snippet: String = chars[actual_start..char_end].iter().collect();
                return snippet.replace('\n', " ").replace('\r', "");
            }
            return matched_text.replace('\n', " ").replace('\r', "");
        }
    };

    let prefix = &text[..match_start];
    let match_char_start = prefix.chars().count();
    let match_text = &text[match_start..match_end];
    let match_char_len = match_text.chars().count();

    let chars: Vec<char> = text.chars().collect();
    let snippet_char_start = match_char_start.saturating_sub(radius);
    let snippet_char_end = (match_char_start + match_char_len + radius).min(chars.len());

    let snippet: String = chars[snippet_char_start..snippet_char_end].iter().collect();
    snippet.replace('\n', " ").replace('\r', "")
}

/// Get file entries for a given directory VFS path.
fn get_dir_entries(vfs: &VfsPath, dir_path: &str) -> Vec<FileEntry> {
    let target = if dir_path.is_empty() || dir_path == "/" {
        vfs.clone()
    } else {
        match vfs.join(dir_path.trim_start_matches('/')) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        }
    };

    let entries = match target.read_dir() {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut result: Vec<FileEntry> = entries
        .map(|entry| {
            let is_dir = entry.is_dir().unwrap_or(false);
            let size = if is_dir { 0 } else { entry.metadata().map(|m| m.len).unwrap_or(0) };
            FileEntry { name: entry.filename(), is_dir, size, vfs_path: entry }
        })
        .collect();

    result.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));

    result
}

/// Get a file type string from the filename extension.
fn file_type_label(name: &str) -> &'static str {
    if let Some(dot_pos) = name.rfind('.') {
        match &name[dot_pos..] {
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
    } else {
        "File"
    }
}

/// Poll a single search tab for new results from its background thread.
fn poll_search_tab(search: &mut ContentSearchTab) {
    let Some(rx) = &search.rx else { return };

    loop {
        match rx.try_recv() {
            Ok(ContentSearchMessage::Hit(hit)) => {
                search.results.push(hit);
            }
            Ok(ContentSearchMessage::Progress(searched, total)) => {
                search.progress = (searched, total);
            }
            Ok(ContentSearchMessage::Done) => {
                search.running = false;
                search.rx = None;
                return;
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                search.running = false;
                search.rx = None;
                return;
            }
        }
    }
}

/// Render the file listing table for directory browsing mode.
fn render_file_listing_table(
    ui: &mut Ui,
    entries: &[FileEntry],
    queued_paths: &HashSet<String>,
    items_to_extract: &Mutex<Vec<VfsPath>>,
    file_viewer: &Mutex<Vec<plaintext_viewer::PlaintextFileViewer>>,
    navigate_to: &std::cell::Cell<Option<(BrowserSource, String)>>,
    source: &BrowserSource,
) {
    use egui_extras::Column;
    use egui_extras::TableBuilder;

    let table = TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(28.0))
        .column(Column::exact(20.0))
        .column(Column::remainder().at_least(150.0))
        .column(Column::exact(80.0))
        .column(Column::exact(60.0))
        .sense(Sense::click());

    table
        .header(20.0, |mut header| {
            header.col(|_ui| {});
            header.col(|_ui| {});
            header.col(|ui| {
                ui.strong(t!("ui.unpacker.column.name"));
            });
            header.col(|ui| {
                ui.strong(t!("ui.unpacker.column.size"));
            });
            header.col(|ui| {
                ui.strong(t!("ui.unpacker.column.type_col"));
            });
        })
        .body(|body| {
            body.rows(22.0, entries.len(), |mut row| {
                let entry = &entries[row.index()];
                let is_queued = queued_paths.contains(entry.vfs_path.as_str());

                row.col(|ui| {
                    if entry.is_dir {
                        if is_queued {
                            if ui
                                .small_button(icons::X_CIRCLE)
                                .on_hover_text(t!("ui.unpacker.remove_from_queue"))
                                .clicked()
                            {
                                items_to_extract.lock().retain(|v| v.as_str() != entry.vfs_path.as_str());
                            }
                        } else if ui
                            .small_button(icons::PLUS_CIRCLE)
                            .on_hover_text(t!("ui.unpacker.queue_folder"))
                            .clicked()
                        {
                            queue_extract(items_to_extract, entry.vfs_path.clone());
                        }
                    } else {
                        let mut checked = is_queued;
                        if ui.checkbox(&mut checked, "").changed() {
                            if checked {
                                queue_extract(items_to_extract, entry.vfs_path.clone());
                            } else {
                                items_to_extract.lock().retain(|v| v.as_str() != entry.vfs_path.as_str());
                            }
                        }
                    }
                });

                row.col(|ui| {
                    ui.label(file_icon_rich_text(&entry.name, entry.is_dir));
                });

                let (_, name_response, name_label_response) = {
                    let mut label_resp = None;
                    let (rect, cell_resp) = row.col(|ui| {
                        label_resp = Some(ui.label(&entry.name));
                    });
                    (rect, cell_resp, label_resp.unwrap())
                };

                row.col(|ui| {
                    if !entry.is_dir {
                        ui.label(RichText::new(humansize::format_size(entry.size, humansize::BINARY)).weak());
                    }
                });

                row.col(|ui| {
                    let type_str = if entry.is_dir { "Folder" } else { file_type_label(&entry.name) };
                    ui.label(RichText::new(type_str).weak());
                });

                let row_response = row.response();
                if entry.is_dir {
                    if row_response.double_clicked() {
                        navigate_to.set(Some((source.clone(), entry.vfs_path.as_str().to_string())));
                    }
                    row_response.on_hover_text(t!("ui.unpacker.double_click_open"));
                } else {
                    add_view_file_context_menu(file_viewer, &name_label_response, &entry.vfs_path, source);
                    add_view_file_context_menu(file_viewer, &name_response, &entry.vfs_path, source);
                    add_view_file_context_menu(file_viewer, &row_response, &entry.vfs_path, source);
                    row_response.on_hover_text(format!("res/{}", entry.vfs_path.as_str().trim_start_matches('/')));
                }
            });
        });
}

/// Render path filter results in a table format.
#[allow(clippy::too_many_arguments)]
fn render_filter_results_table(
    ui: &mut Ui,
    filtered_files: &[(Arc<PathBuf>, VfsPath)],
    queued_paths: &HashSet<String>,
    items_to_extract: &Mutex<Vec<VfsPath>>,
    file_viewer: &Mutex<Vec<plaintext_viewer::PlaintextFileViewer>>,
    navigate_to: &std::cell::Cell<Option<(BrowserSource, String)>>,
    clear_filter: &std::cell::Cell<Option<BrowserSource>>,
    source: &BrowserSource,
) {
    use egui_extras::Column;
    use egui_extras::TableBuilder;

    let table = TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(28.0))
        .column(Column::exact(20.0))
        .column(Column::remainder().at_least(200.0))
        .column(Column::exact(80.0))
        .column(Column::exact(60.0))
        .sense(Sense::click());

    table
        .header(20.0, |mut header| {
            header.col(|_ui| {});
            header.col(|_ui| {});
            header.col(|ui| {
                ui.strong(t!("ui.unpacker.column.path"));
            });
            header.col(|ui| {
                ui.strong(t!("ui.unpacker.column.size"));
            });
            header.col(|ui| {
                ui.strong(t!("ui.unpacker.column.type_col"));
            });
        })
        .body(|body| {
            body.rows(22.0, filtered_files.len(), |mut row| {
                let (path, vfs_path) = &filtered_files[row.index()];
                let is_queued = queued_paths.contains(vfs_path.as_str());
                let filename = vfs_path.filename();

                row.col(|ui| {
                    let mut checked = is_queued;
                    if ui.checkbox(&mut checked, "").changed() {
                        if checked {
                            queue_extract(items_to_extract, vfs_path.clone());
                        } else {
                            items_to_extract.lock().retain(|v| v.as_str() != vfs_path.as_str());
                        }
                    }
                });

                row.col(|ui| {
                    ui.label(file_icon_rich_text(&filename, false));
                });

                let (_, path_response, path_label_response) = {
                    let mut label_resp = None;
                    let (rect, cell_resp) = row.col(|ui| {
                        let path_str = path.to_string_lossy();
                        let display = format!("res/{}", path_str.trim_start_matches('/'));
                        label_resp = Some(ui.label(&display));
                    });
                    (rect, cell_resp, label_resp.unwrap())
                };

                row.col(|ui| {
                    let size = vfs_path
                        .metadata()
                        .map(|m| humansize::format_size(m.len, humansize::BINARY))
                        .unwrap_or_default();
                    ui.label(RichText::new(size).weak());
                });

                row.col(|ui| {
                    ui.label(RichText::new(file_type_label(&filename)).weak());
                });

                let row_response = row.response();
                add_view_file_context_menu(file_viewer, &path_label_response, vfs_path, source);
                add_view_file_context_menu(file_viewer, &path_response, vfs_path, source);
                add_view_file_context_menu(file_viewer, &row_response, vfs_path, source);
                if row_response.clicked() {
                    let vfs_str = vfs_path.as_str();
                    if let Some(parent_end) = vfs_str.rfind('/') {
                        let parent = &vfs_str[..parent_end];
                        navigate_to.set(Some((
                            source.clone(),
                            if parent.is_empty() { "/".to_string() } else { parent.to_string() },
                        )));
                        clear_filter.set(Some(source.clone()));
                    }
                }
            });
        });
}

/// Render content search results table.
fn render_search_results_table(
    ui: &mut Ui,
    results: &[ContentSearchHit],
    items_to_extract: &Mutex<Vec<VfsPath>>,
    file_viewer: &Mutex<Vec<plaintext_viewer::PlaintextFileViewer>>,
    navigate_to: &std::cell::Cell<Option<(BrowserSource, String)>>,
    clear_filter: &std::cell::Cell<Option<BrowserSource>>,
    source: &BrowserSource,
) {
    use egui_extras::Column;
    use egui_extras::TableBuilder;

    let items_snapshot = items_to_extract.lock().clone();
    let queued_paths: HashSet<String> = items_snapshot.iter().map(|v| v.as_str().to_string()).collect();

    let table = TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(28.0))
        .column(Column::exact(20.0))
        .column(Column::initial(300.0).at_least(150.0).resizable(true))
        .column(Column::exact(70.0))
        .column(Column::remainder().at_least(200.0))
        .sense(Sense::click());

    table
        .header(20.0, |mut header| {
            header.col(|_ui| {});
            header.col(|_ui| {});
            header.col(|ui| {
                ui.strong(t!("ui.unpacker.column.file"));
            });
            header.col(|ui| {
                ui.strong(t!("ui.unpacker.column.offset"));
            });
            header.col(|ui| {
                ui.strong(t!("ui.unpacker.column.context"));
            });
        })
        .body(|body| {
            body.rows(22.0, results.len(), |mut row| {
                let hit = &results[row.index()];
                let is_queued = queued_paths.contains(hit.vfs_path.as_str());
                let filename = hit.vfs_path.filename();

                row.col(|ui| {
                    let mut checked = is_queued;
                    if ui.checkbox(&mut checked, "").changed() {
                        if checked {
                            queue_extract(items_to_extract, hit.vfs_path.clone());
                        } else {
                            items_to_extract.lock().retain(|v| v.as_str() != hit.vfs_path.as_str());
                        }
                    }
                });

                row.col(|ui| {
                    ui.label(file_icon_rich_text(&filename, false));
                });

                let (_, file_response) = row.col(|ui| {
                    let display = format!("res/{}", hit.vfs_path_str.trim_start_matches('/'));
                    ui.add(Label::new(&display).truncate());
                });

                row.col(|ui| {
                    ui.label(RichText::new(format!("0x{:X}", hit.offset)).weak());
                });

                row.col(|ui| {
                    ui.label(RichText::new(&hit.context).monospace().weak());
                });

                let vfs_str = hit.vfs_path_str.clone();
                let vfs_path = hit.vfs_path.clone();
                let vfs_str2 = vfs_str.clone();
                let vfs_path2 = vfs_path.clone();
                file_response.context_menu(|ui| {
                    if ui
                        .button(wt_translations::icon_t(icons::FOLDER_OPEN, &t!("ui.unpacker.go_to_directory")))
                        .clicked()
                    {
                        if let Some(parent_end) = vfs_str.rfind('/') {
                            let parent = &vfs_str[..parent_end];
                            navigate_to.set(Some((
                                source.clone(),
                                if parent.is_empty() { "/".to_string() } else { parent.to_string() },
                            )));
                            clear_filter.set(Some(source.clone()));
                        }
                        ui.close_kind(UiKind::Menu);
                    }
                    if ui.button(wt_translations::icon_t(icons::EYE, &t!("ui.unpacker.view_contents"))).clicked() {
                        open_file_viewer(file_viewer, &vfs_path);
                        ui.close_kind(UiKind::Menu);
                    }
                });
                row.response().context_menu(|ui| {
                    if ui
                        .button(wt_translations::icon_t(icons::FOLDER_OPEN, &t!("ui.unpacker.go_to_directory")))
                        .clicked()
                    {
                        if let Some(parent_end) = vfs_str2.rfind('/') {
                            let parent = &vfs_str2[..parent_end];
                            navigate_to.set(Some((
                                source.clone(),
                                if parent.is_empty() { "/".to_string() } else { parent.to_string() },
                            )));
                            clear_filter.set(Some(source.clone()));
                        }
                        ui.close_kind(UiKind::Menu);
                    }
                    if ui.button(wt_translations::icon_t(icons::EYE, &t!("ui.unpacker.view_contents"))).clicked() {
                        open_file_viewer(file_viewer, &vfs_path2);
                        ui.close_kind(UiKind::Menu);
                    }
                });
            });
        });
}

/// Build a cached folder tree structure from a VFS root (called once at load time).
fn build_folder_tree(vfs: &VfsPath, path_prefix: &str) -> Vec<FolderTreeNode> {
    let Ok(entries) = vfs.read_dir() else {
        return Vec::new();
    };
    let mut dirs: Vec<VfsPath> = entries.filter(|e| e.is_dir().unwrap_or(false)).collect();
    dirs.sort_by_key(|d| d.filename());

    dirs.iter()
        .map(|child_dir| {
            let name = child_dir.filename();
            let path = if path_prefix.is_empty() { format!("/{name}") } else { format!("{path_prefix}/{name}") };
            let children = build_folder_tree(child_dir, &path);
            FolderTreeNode { name, path, children }
        })
        .collect()
}

/// Render the cached folder tree into the egui tree view builder.
fn render_folder_tree(
    builder: &mut egui_ltreeview::TreeViewBuilder<'_, egui::Id>,
    nodes: &[FolderTreeNode],
    id_to_path: &mut std::collections::HashMap<egui::Id, String>,
    source_id: &str,
) {
    for node in nodes {
        let dir_id = egui::Id::new(("browser_dir", source_id, &node.path));
        id_to_path.insert(dir_id, node.path.clone());

        if node.children.is_empty() {
            let leaf = egui_ltreeview::NodeBuilder::leaf(dir_id)
                .icon(|ui| {
                    ui.label(RichText::new(icons::FOLDER).color(egui::Color32::from_rgb(200, 180, 120)));
                })
                .label(&node.name);
            builder.node(leaf);
        } else {
            let dir = egui_ltreeview::NodeBuilder::dir(dir_id)
                .default_open(false)
                .icon(|ui| {
                    ui.label(RichText::new(icons::FOLDER).color(egui::Color32::from_rgb(200, 180, 120)));
                })
                .label(&node.name);

            let is_open = builder.node(dir);
            if is_open {
                render_folder_tree(builder, &node.children, id_to_path, source_id);
            }
            builder.close_dir();
        }
    }
}

/// Recompute the filtered file list for a browser pane if the filter text changed.
fn recompute_filter(browser: &mut BrowserPane) {
    let is_filtering = browser.filter.len() >= 3;
    if !is_filtering {
        // Clear stale filtered results when filter is too short
        if browser.filtered_file_list.is_some() {
            browser.filtered_file_list = None;
            browser.used_filter = None;
        }
        return;
    }

    if browser.used_filter.as_deref() == Some(&browser.filter) {
        return; // already up to date
    }

    let Some(files) = &browser.files else { return };

    let filter_list = {
        let glob = glob::Pattern::new(&browser.filter);
        if browser.filter.contains('*')
            && let Ok(glob) = glob
        {
            files.iter().filter(|(path, _node)| glob.matches_path(path)).cloned().collect()
        } else {
            files
                .iter()
                .filter(|(path, _node)| {
                    path.to_str().map(|path| path.contains(browser.filter.as_str())).unwrap_or(false)
                })
                .cloned()
                .collect()
        }
    };

    browser.filtered_file_list = Some(Arc::new(filter_list));
    browser.used_filter = Some(browser.filter.clone());
}

/// Recursively collect all files from a VFS into a flat list.
fn collect_vfs_files(vfs: &VfsPath, prefix: &str) -> Vec<(Arc<PathBuf>, VfsPath)> {
    let mut result = Vec::new();
    let target = if prefix.is_empty() {
        vfs.clone()
    } else {
        match vfs.join(prefix.trim_start_matches('/')) {
            Ok(p) => p,
            Err(_) => return result,
        }
    };

    if let Ok(entries) = target.read_dir() {
        for entry in entries {
            let name = entry.filename();
            let child_path = if prefix.is_empty() { format!("/{name}") } else { format!("{prefix}/{name}") };

            if entry.is_dir().unwrap_or(false) {
                result.extend(collect_vfs_files(vfs, &child_path));
            } else {
                let vfs_path = entry;
                result.push((Arc::new(PathBuf::from(&child_path)), vfs_path));
            }
        }
    }
    result
}
impl ToolkitTabViewer<'_> {
    /// Start a content search on a background thread, creating a new search tab.
    /// Searches the specified browser pane's VFS.
    fn start_content_search(&mut self, target_source: BrowserSource) {
        // Find the target browser pane and extract query, path_filter, VFS, and files
        let (query, path_filter, source, active_vfs, active_files) = {
            let mut found = None;
            for (_, pane) in self.tab_state.browser_state.dock_state.iter_all_tabs() {
                if let UnpackerPane::Browser(browser) = pane
                    && browser.source == target_source
                {
                    let q = browser.content_search_query.trim().to_string();
                    let pf = browser.content_search_path_filter.trim().to_string();
                    if let (Some(vfs), Some(files)) = (&browser.vfs, &browser.files) {
                        found = Some((q, pf, browser.source.clone(), vfs.clone(), files.clone()));
                    }
                    break;
                }
            }
            match found {
                Some(f) => f,
                None => return,
            }
        };

        if query.is_empty() {
            return;
        }

        let id = self.tab_state.browser_state.next_search_id;
        self.tab_state.browser_state.next_search_id += 1;

        let stop_flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let search_tab = ContentSearchTab {
            id,
            query: query.clone(),
            source: source.clone(),
            stop_flag: stop_flag.clone(),
            rx: Some(rx),
            results: Vec::new(),
            progress: (0, 0),
            running: true,
        };

        // Add search tab to dock
        let pane = UnpackerPane::Search(search_tab);
        let tree = self.tab_state.browser_state.dock_state.main_surface_mut();

        // Find if there's already a search tab node to add to
        let existing_search_node = tree.iter().enumerate().find_map(|(idx, node)| {
            if let egui_dock::Node::Leaf(leaf) = node
                && leaf.tabs().iter().any(|t| matches!(t, UnpackerPane::Search(_)))
            {
                return Some(NodeIndex(idx));
            }
            None
        });

        if let Some(node_idx) = existing_search_node {
            // Add as a new tab alongside existing search tabs
            let tab_count = if let egui_dock::Node::Leaf(leaf) = &tree[node_idx] { leaf.tabs().len() } else { 0 };
            tree.set_focused_node(node_idx);
            tree.push_to_focused_leaf(pane);
            // Focus the new tab
            let _ = tree.set_active_tab(node_idx, egui_dock::TabIndex(tab_count));
        } else {
            // First search: split below the browser
            tree.split_below(NodeIndex::root(), 0.6, vec![pane]);
        }

        // Spawn background thread
        let stop = stop_flag;
        let filtered_files = active_files;
        let vfs = active_vfs;

        crate::util::thread::spawn_logged("vfs-search", move || {
            let regex = match regex::bytes::Regex::new(&query) {
                Ok(r) => r,
                Err(_) => match regex::bytes::Regex::new(&regex::escape(&query)) {
                    Ok(r) => r,
                    Err(_) => {
                        let _ = tx.send(ContentSearchMessage::Done);
                        return;
                    }
                },
            };

            let glob_filter = if path_filter.is_empty() { None } else { glob::Pattern::new(&path_filter).ok() };

            let files_to_search: Vec<_> = filtered_files
                .iter()
                .filter(|(path, node)| {
                    if !node.is_file().unwrap_or(false) {
                        return false;
                    }
                    if let Some(ref glob) = glob_filter {
                        return glob.matches_path(path);
                    }
                    true
                })
                .collect();

            let total = files_to_search.len();
            let _ = tx.send(ContentSearchMessage::Progress(0, total));

            let mut buffer = Vec::new();
            const MAX_RETAINED: usize = 4 * 1024 * 1024;

            for (i, (_path, vfs_path)) in files_to_search.iter().enumerate() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                if i % 100 == 0 {
                    let _ = tx.send(ContentSearchMessage::Progress(i, total));
                }

                buffer.clear();
                let Ok(joined) = vfs.join(vfs_path.as_str().trim_start_matches('/')) else {
                    continue;
                };
                let Ok(mut file) = joined.open_file() else {
                    continue;
                };
                if file.read_to_end(&mut buffer).is_err() {
                    continue;
                }

                for mat in regex.find_iter(&buffer) {
                    let context = extract_context_snippet(&buffer, mat.start(), mat.end(), 30);
                    let _ = tx.send(ContentSearchMessage::Hit(ContentSearchHit {
                        vfs_path_str: vfs_path.as_str().to_string(),
                        vfs_path: vfs_path.clone(),
                        context,
                        offset: mat.start(),
                    }));
                }

                if buffer.capacity() > MAX_RETAINED {
                    buffer = Vec::new();
                }
            }

            let _ = tx.send(ContentSearchMessage::Progress(total, total));
            let _ = tx.send(ContentSearchMessage::Done);
        });
    }

    fn extract_files(&mut self, output_dir: &Path, items_to_unpack: &[VfsPath], decode_json: bool) {
        let (tx, rx) = mpsc::channel();

        self.tab_state.unpacker_progress = Some(rx);
        UNPACKER_STOP.store(false, Ordering::Relaxed);

        if !items_to_unpack.is_empty() {
            let output_dir = output_dir.to_owned();
            let mut file_queue = items_to_unpack.to_vec();
            let _unpacker_thread = Some(crate::util::thread::spawn_logged("vfs-extract", move || {
                let mut files_to_extract: Vec<VfsPath> = Vec::new();
                let mut folders_created: HashSet<PathBuf> = HashSet::default();
                while let Some(file) = file_queue.pop() {
                    if file.is_file().unwrap_or(false) {
                        files_to_extract.push(file);
                    } else if let Ok(entries) = file.read_dir() {
                        for child in entries {
                            file_queue.push(child);
                        }
                    }
                }
                let file_count = files_to_extract.len();

                for (files_written, file) in files_to_extract.iter().enumerate() {
                    if UNPACKER_STOP.load(Ordering::Relaxed) {
                        break;
                    }

                    let vfs_path_str = file.as_str().trim_start_matches('/');
                    let parent_path = Path::new(vfs_path_str).parent().unwrap_or(Path::new(""));
                    let path = output_dir.join(parent_path);
                    let filename = file.filename();
                    let file_path = path.join(&filename);
                    tx.send(UnpackerProgress {
                        file_name: file_path.to_string_lossy().into(),
                        progress: (files_written as f32) / (file_count as f32),
                    })
                    .unwrap();
                    if !folders_created.contains(&path) {
                        fs::create_dir_all(&path).expect("failed to create folder");
                        folders_created.insert(path.clone());
                    }

                    // Try JSON decode for Assets.bin prototypes when toggle is on
                    if decode_json && let Some(pt) = decodable_prototype_type(&filename) {
                        let mut data = Vec::new();
                        if let Ok(mut reader) = file.open_file() {
                            let _ = reader.read_to_end(&mut data);
                        }
                        if let Ok(json) = wowsunpack::models::decode_prototype_to_json(&data, pt) {
                            let json_path = path.join(format!("{filename}.json"));
                            let mut out_file = File::create(json_path).expect("failed to create output file");
                            out_file.write_all(json.as_bytes()).expect("Failed to write JSON");
                            continue;
                        }
                        // Decode failed — fall through to raw extraction
                    }

                    let mut out_file = File::create(file_path).expect("failed to create output file");

                    if let Ok(mut reader) = file.open_file() {
                        std::io::copy(&mut reader, &mut out_file).expect("Failed to extract file");
                    }
                }
            }));
        }
    }

    fn extract_files_clicked(&mut self) {
        let items_to_unpack = self.tab_state.items_to_extract.lock().clone();
        let output_dir = Path::new(self.tab_state.persisted.read().output_dir.as_str()).join("res");
        let decode_json = self.tab_state.browser_state.decode_prototypes_as_json;

        self.extract_files(output_dir.as_ref(), items_to_unpack.as_slice(), decode_json);
    }

    fn dump_game_params(&mut self, file_path: PathBuf, format: GameParamsFormat, base_params: bool) {
        let (tx, rx) = mpsc::channel();

        self.tab_state.unpacker_progress = Some(rx);
        UNPACKER_STOP.store(false, Ordering::Relaxed);

        if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
            let wows_data = wows_data.read();
            let metadata_provider = { wows_data.game_metadata.clone() };
            let vfs = wows_data.vfs.clone();

            let game_params_path = vfs.join("content/GameParams.data");
            if let Ok(game_params_vfs) = game_params_path {
                let _unpacker_thread = Some(crate::util::thread::spawn_logged("load-game-params", move || {
                    let mut game_params_data = Vec::new();

                    game_params_vfs
                        .open_file()
                        .and_then(|mut f| Ok(f.read_to_end(&mut game_params_data)?))
                        .expect("failed to read GameParams");

                    tx.send(UnpackerProgress { file_name: file_path.to_string_lossy().into(), progress: 0.0 }).unwrap();

                    let pickle = game_params_to_pickle(game_params_data).expect("failed to deserialize GameParams");
                    let params_dict = if base_params {
                        match pickle {
                            pickled::Value::Dict(params_dict) => params_dict
                                .inner()
                                .get(&HashableValue::String("".to_string().into()))
                                .expect("Could not find base game params with empty key")
                                .clone(),
                            pickled::Value::List(params_list) => params_list.inner_mut().remove(0),
                            _other => {
                                panic!("Unexpected GameParams root element type");
                            }
                        }
                    } else {
                        pickle
                    };

                    let file = BufWriter::new(File::create(&file_path).expect("failed to create GameParams.json file"));
                    match format {
                        GameParamsFormat::Json => {
                            let mut serializer = serde_json::Serializer::pretty(file);
                            params_dict.serialize(&mut serializer).expect("failed to write GameParams data");
                        }
                        GameParamsFormat::Cbor => {
                            let file = IoWrite::new(file);
                            let mut serializer = serde_cbor::Serializer::new(file);
                            params_dict.serialize(&mut serializer).expect("failed to write GameParams data");
                        }
                        GameParamsFormat::MinimalJson => {
                            if let Some(metadata_provider) = metadata_provider {
                                serde_json::to_writer(file, &metadata_provider.params())
                                    .expect("failed to write CBOR data");
                            }
                        }
                        GameParamsFormat::MinimalCbor => {
                            if let Some(metadata_provider) = metadata_provider {
                                serde_cbor::to_writer(file, &metadata_provider.params())
                                    .expect("failed to write CBOR data");
                            }
                        }
                    }

                    tx.send(UnpackerProgress { file_name: file_path.to_string_lossy().into(), progress: 1.0 }).unwrap();
                }));
            }
        }
    }

    /// Returns the SharedWoWsData for the currently selected browser build.
    fn selected_browser_data(&self) -> Option<crate::data::wows_data::SharedWoWsData> {
        let map = self.tab_state.wows_data_map.as_ref()?;
        map.get(self.tab_state.selected_browser_build)
    }

    /// Provision the PKG browser pane with the current build's VFS + file list if not set.
    fn provision_pkg_browser(&mut self) {
        let wows_data = self.selected_browser_data().or_else(|| self.tab_state.world_of_warships_data.clone());
        let Some(wows_data) = wows_data else { return };
        let wows_data = wows_data.read();

        for (_, pane) in self.tab_state.browser_state.dock_state.iter_all_tabs_mut() {
            if let UnpackerPane::Browser(browser) = pane
                && browser.source == BrowserSource::Pkg
                && browser.vfs.is_none()
            {
                browser.cached_folder_tree = Some(build_folder_tree(&wows_data.vfs, ""));
                browser.vfs = Some(wows_data.vfs.clone());
                browser.files = Some(wows_data.filtered_files.clone());
            }
        }
    }

    /// Kick off background assets.bin loading if needed.
    fn maybe_start_assets_bin_loading(&mut self) {
        // Check if assets.bin pane is in loading state and no receiver is active
        let needs_loading = self.tab_state.browser_state.dock_state.iter_all_tabs().any(
            |(_, pane)| matches!(pane, UnpackerPane::Browser(b) if b.source == BrowserSource::AssetsBin && b.loading),
        );

        if !needs_loading || self.tab_state.browser_state.assets_bin_rx.is_some() {
            return;
        }

        let wows_data = self.selected_browser_data().or_else(|| self.tab_state.world_of_warships_data.clone());
        let Some(wows_data) = wows_data else { return };
        let wows_data_read = wows_data.read();
        let current_build = wows_data_read.build_number;

        // Don't re-load if we already loaded for this build
        if self.tab_state.browser_state.assets_bin_loading_build == Some(current_build) {
            return;
        }

        let vfs = wows_data_read.vfs.clone();
        drop(wows_data_read);

        self.tab_state.browser_state.assets_bin_loading_build = Some(current_build);

        let (tx, rx) = mpsc::channel();
        self.tab_state.browser_state.assets_bin_rx = Some(rx);

        crate::util::thread::spawn_logged("load-assets-bin", move || {
            let result = (|| -> Result<AssetsBinLoadResult, String> {
                let assets_bin_path = vfs.join("content/assets.bin").map_err(|e| format!("{}", e))?;
                let mut assets_data = Vec::new();
                assets_bin_path
                    .open_file()
                    .and_then(|mut f| Ok(f.read_to_end(&mut assets_data)?))
                    .map_err(|e| format!("Failed to read assets.bin: {}", e))?;

                let assets_vfs =
                    AssetsBinVfs::new(assets_data).map_err(|e| format!("Failed to parse assets.bin: {}", e))?;

                let vfs = VfsPath::new(assets_vfs);
                let files = collect_vfs_files(&vfs, "");
                Ok(AssetsBinLoadResult { vfs, files })
            })();
            let _ = tx.send(result);
        });
    }

    /// Poll the assets.bin background loading receiver.
    fn poll_assets_bin_loading(&mut self) {
        let Some(rx) = &self.tab_state.browser_state.assets_bin_rx else { return };

        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.tab_state.browser_state.assets_bin_rx = None;
                // Find the assets.bin browser pane and populate it
                for (_, pane) in self.tab_state.browser_state.dock_state.iter_all_tabs_mut() {
                    if let UnpackerPane::Browser(browser) = pane
                        && browser.source == BrowserSource::AssetsBin
                    {
                        browser.cached_folder_tree = Some(build_folder_tree(&result.vfs, ""));
                        browser.vfs = Some(result.vfs.clone());
                        browser.files = Some(result.files.clone());
                        browser.loading = false;
                        break;
                    }
                }
            }
            Ok(Err(err)) => {
                self.tab_state.browser_state.assets_bin_rx = None;
                for (_, pane) in self.tab_state.browser_state.dock_state.iter_all_tabs_mut() {
                    if let UnpackerPane::Browser(browser) = pane
                        && browser.source == BrowserSource::AssetsBin
                    {
                        browser.loading = false;
                        browser.error = Some(err.clone());
                        break;
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {} // still loading
            Err(mpsc::TryRecvError::Disconnected) => {
                self.tab_state.browser_state.assets_bin_rx = None;
                for (_, pane) in self.tab_state.browser_state.dock_state.iter_all_tabs_mut() {
                    if let UnpackerPane::Browser(browser) = pane
                        && browser.source == BrowserSource::AssetsBin
                    {
                        browser.loading = false;
                        browser.error = Some("Background thread disconnected".to_string());
                        break;
                    }
                }
            }
        }
    }

    /// Builds the file unpacker tab with an explorer-style layout.
    pub fn build_unpacker_tab(&mut self, ui: &mut egui::Ui) {
        // Provision PKG browser with VFS data
        self.provision_pkg_browser();

        // Poll / start assets.bin background loading
        self.poll_assets_bin_loading();
        self.maybe_start_assets_bin_loading();

        // Poll all search tabs for new results
        for (_, pane) in self.tab_state.browser_state.dock_state.iter_all_tabs_mut() {
            if let UnpackerPane::Search(search) = pane {
                poll_search_tab(search);
            }
        }

        // ── Top panel: version selector (only when multiple builds) ──────
        if self.tab_state.available_builds.len() > 1 {
            egui::Panel::top("browser_version_bar").show_inside(ui, |ui| {
                if let Some(map) = &self.tab_state.wows_data_map {
                    let mut builds = self.tab_state.available_builds.clone();
                    builds.sort();
                    builds.reverse();

                    let selected_label = format!("{}", self.tab_state.selected_browser_build);

                    ui.horizontal(|ui| {
                        ui.label(t!("ui.unpacker.version"));
                        egui::ComboBox::from_id_salt("browser_version_select").selected_text(&selected_label).show_ui(
                            ui,
                            |ui| {
                                for &build in &builds {
                                    let is_loaded = map.get(build).is_some();
                                    let label = format!("{}", build);
                                    if ui
                                        .selectable_value(&mut self.tab_state.selected_browser_build, build, &label)
                                        .changed()
                                    {
                                        self.tab_state.browser_state.reset_filters();
                                        // Reset PKG browser VFS so it re-provisions
                                        for (_, pane) in self.tab_state.browser_state.dock_state.iter_all_tabs_mut() {
                                            if let UnpackerPane::Browser(browser) = pane {
                                                if browser.source == BrowserSource::Pkg {
                                                    browser.vfs = None;
                                                    browser.files = None;
                                                    browser.selected_dir = None;
                                                    browser.cached_folder_tree = None;
                                                } else if browser.source == BrowserSource::AssetsBin {
                                                    browser.vfs = None;
                                                    browser.files = None;
                                                    browser.selected_dir = None;
                                                    browser.cached_folder_tree = None;
                                                    browser.loading = true;
                                                    browser.error = None;
                                                }
                                            }
                                        }
                                        // Reset assets.bin loading state so it re-loads for new build
                                        self.tab_state.browser_state.assets_bin_rx = None;
                                        self.tab_state.browser_state.assets_bin_loading_build = None;

                                        if !is_loaded {
                                            let map = map.clone();
                                            let (tx, rx) = std::sync::mpsc::channel();
                                            crate::util::thread::spawn_logged("resolve-build", move || {
                                                match map.resolve_build(build) {
                                                    Some(_) => {
                                                        let _ = tx.send(Ok(
                                                            crate::task::BackgroundTaskCompletion::BuildDataLoaded {
                                                                build,
                                                            },
                                                        ));
                                                    }
                                                    None => {
                                                        let report: rootcause::Report =
                                                            crate::util::error::ToolkitError::ReplayBuildUnavailable {
                                                                build,
                                                                version: format!("{}", build),
                                                            }
                                                            .into();
                                                        let _ = tx.send(Err(report
                                                            .attach("game data could not be loaded for this build")));
                                                    }
                                                }
                                            });
                                            let _ = self.tab_state.background_task_sender.send(
                                                crate::task::BackgroundTask {
                                                    receiver: Some(rx),
                                                    kind: crate::task::BackgroundTaskKind::LoadingBuildData(build),
                                                },
                                            );
                                        }
                                    }
                                }
                            },
                        );
                    });
                }
            });
        }

        // ── Bottom panel: extract controls ───────────────────────────────
        egui::Panel::bottom("browser_extract_bar").exact_size(36.0).show_inside(ui, |ui| {
            let queue_count = self.tab_state.items_to_extract.lock().len();

            ui.horizontal_centered(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // GameParams dump menu (rightmost)
                    ui.menu_button(wt_translations::icon_t(icons::FLOPPY_DISK, &t!("ui.unpacker.game_params")), |ui| {
                        if ui.small_button(t!("ui.unpacker.base_as_json")).clicked() {
                            if let Some(path) = rfd::FileDialog::new().set_file_name("GameParams.json").save_file() {
                                self.dump_game_params(path, GameParamsFormat::Json, true);
                            }
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui.small_button(t!("ui.unpacker.as_json")).clicked() {
                            if let Some(path) = rfd::FileDialog::new().set_file_name("GameParams.json").save_file() {
                                self.dump_game_params(path, GameParamsFormat::Json, false);
                            }
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui.small_button(t!("ui.unpacker.as_cbor")).clicked() {
                            if let Some(path) = rfd::FileDialog::new().set_file_name("GameParams.cbor").save_file() {
                                self.dump_game_params(path, GameParamsFormat::Cbor, false);
                            }
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui.small_button(t!("ui.unpacker.as_json_minimal")).clicked() {
                            if let Some(path) = rfd::FileDialog::new().set_file_name("MinGameParams.json").save_file() {
                                self.dump_game_params(path, GameParamsFormat::MinimalJson, false);
                            }
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui.small_button(t!("ui.unpacker.as_cbor_minimal")).clicked() {
                            if let Some(path) = rfd::FileDialog::new().set_file_name("MinGameParams.cbor").save_file() {
                                self.dump_game_params(path, GameParamsFormat::MinimalCbor, false);
                            }
                            ui.close_kind(UiKind::Menu);
                        }
                    });

                    // Extract button
                    let extract_label = if queue_count > 0 {
                        format!(
                            "{} Extract {} Item{}",
                            icons::DOWNLOAD_SIMPLE,
                            queue_count,
                            if queue_count == 1 { "" } else { "s" }
                        )
                    } else {
                        format!("{} Extract", icons::DOWNLOAD_SIMPLE)
                    };
                    let extract_enabled = queue_count > 0 && !self.tab_state.persisted.read().output_dir.is_empty();
                    if ui.add_enabled(extract_enabled, egui::Button::new(extract_label)).clicked() {
                        self.extract_files_clicked();
                    }

                    ui.checkbox(
                        &mut self.tab_state.browser_state.decode_prototypes_as_json,
                        t!("ui.unpacker.decode_prototypes"),
                    )
                    .on_hover_text(t!("ui.unpacker.decode_prototypes_tooltip"));

                    // Queue popover button
                    let queue_label = if queue_count > 0 {
                        format!("{} {} queued", icons::LIST_CHECKS, queue_count)
                    } else {
                        format!("{} Queue", icons::LIST_CHECKS)
                    };
                    let queue_btn = ui.button(queue_label);
                    if queue_btn.clicked() {
                        self.tab_state.browser_state.show_queue_popover =
                            !self.tab_state.browser_state.show_queue_popover;
                    }

                    // Queue popover
                    if self.tab_state.browser_state.show_queue_popover && queue_count > 0 {
                        egui::Area::new(egui::Id::new("extract_queue_popover"))
                            .order(egui::Order::Foreground)
                            .fixed_pos(queue_btn.rect.left_top() - egui::vec2(0.0, 8.0))
                            .pivot(egui::Align2::LEFT_BOTTOM)
                            .show(ui.ctx(), |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    ui.set_max_width(400.0);
                                    ui.set_max_height(300.0);
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(t!("ui.unpacker.extraction_queue"));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui
                                                    .small_button(wt_translations::icon_t(
                                                        icons::TRASH,
                                                        &t!("ui.unpacker.clear_all"),
                                                    ))
                                                    .clicked()
                                                {
                                                    self.tab_state.items_to_extract.lock().clear();
                                                    self.tab_state.browser_state.show_queue_popover = false;
                                                }
                                            });
                                        });
                                        ui.separator();
                                        egui::ScrollArea::vertical().max_height(250.0).show(ui, |ui| {
                                            let mut items = self.tab_state.items_to_extract.lock();
                                            let mut remove_idx = None;
                                            for (i, item) in items.iter().enumerate() {
                                                ui.horizontal(|ui| {
                                                    let path_str = item.as_str().trim_start_matches('/');
                                                    let is_dir = item.is_dir().unwrap_or(false);
                                                    let icon = if is_dir { icons::FOLDER } else { icons::FILE };
                                                    ui.label(RichText::new(icon).color(if is_dir {
                                                        egui::Color32::from_rgb(200, 180, 120)
                                                    } else {
                                                        ui.style().visuals.text_color()
                                                    }));
                                                    ui.label(format!("res/{path_str}"));
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(egui::Align::Center),
                                                        |ui| {
                                                            if ui.small_button(icons::X_CIRCLE).clicked() {
                                                                remove_idx = Some(i);
                                                            }
                                                        },
                                                    );
                                                });
                                            }
                                            if let Some(idx) = remove_idx {
                                                items.remove(idx);
                                            }
                                        });
                                    });
                                });
                            });
                    }

                    ui.separator();

                    // Browse button
                    if ui.button(wt_translations::icon_t(icons::FOLDER, &t!("ui.unpacker.browse"))).clicked()
                        && let Some(folder) = rfd::FileDialog::new().pick_folder()
                    {
                        self.tab_state.persisted.write().output_dir = folder.to_string_lossy().into_owned();
                    }

                    // Output path text box fills remaining space
                    let mut output_dir = self.tab_state.persisted.read().output_dir.clone();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut output_dir)
                            .hint_text(t!("ui.unpacker.output_dir_hint"))
                            .desired_width(ui.available_width()),
                    );
                    if response.changed() {
                        self.tab_state.persisted.write().output_dir = output_dir;
                    }
                });
            });
        });

        // ── Central panel: nested DockArea ───────────────────────────────
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let navigate_to: std::cell::Cell<Option<(BrowserSource, String)>> = std::cell::Cell::new(None);
            let clear_filter: std::cell::Cell<Option<BrowserSource>> = std::cell::Cell::new(None);
            let start_search: std::cell::Cell<Option<BrowserSource>> = std::cell::Cell::new(None);

            let mut viewer = UnpackerPaneViewer {
                items_to_extract: &self.tab_state.items_to_extract,
                file_viewer: &self.tab_state.file_viewer,
                navigate_to: &navigate_to,
                clear_filter: &clear_filter,
                start_search: &start_search,
            };

            DockArea::new(&mut self.tab_state.browser_state.dock_state)
                .id(egui::Id::new("unpacker_dock"))
                .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                .allowed_splits(egui_dock::AllowedSplits::All)
                .show_close_buttons(true)
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_inside(ui, &mut viewer);

            // Apply deferred navigation
            if let Some((source, dir)) = navigate_to.take() {
                for (_, pane) in self.tab_state.browser_state.dock_state.iter_all_tabs_mut() {
                    if let UnpackerPane::Browser(browser) = pane
                        && browser.source == source
                    {
                        browser.selected_dir = Some(dir);
                        break;
                    }
                }
            }
            if let Some(source) = clear_filter.take() {
                for (_, pane) in self.tab_state.browser_state.dock_state.iter_all_tabs_mut() {
                    if let UnpackerPane::Browser(browser) = pane
                        && browser.source == source
                    {
                        browser.reset_filter();
                        break;
                    }
                }
            }

            // Apply deferred content search
            if let Some(source) = start_search.take() {
                self.start_content_search(source);
            }
        });
    }
}
