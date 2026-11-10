//! One file-browser pane: a collapsible folder-tree sidebar beside a
//! virtualized file listing, with a path filter above the tree and the
//! content-search inputs below it, as the GPUI pane lays them out.

use std::sync::Arc;

use nsh_base::SelectValue;
use nsh_base::TreeEntry;
use nsh_base::TreeItem;
use nsh_base::checkbox;
use nsh_base::context_menu;
use nsh_base::h_resizable;
use nsh_base::menu_item;
use nsh_base::resizable_panel;
use nsh_base::tree;
use nsh_core::prelude::*;
use wows_toolkit_viewmodel::formatting::byte_size;
use wows_toolkit_viewmodel::unpacker::listing::FileList;
use wows_toolkit_viewmodel::unpacker::listing::ROOT_PATH;
use wows_toolkit_viewmodel::unpacker::listing::file_type_label;
use wows_toolkit_viewmodel::unpacker::viewer::decodable_prototype;
use wowsunpack::vfs::VfsPath;

use crate::icons::Icon;
use crate::icons::icon;

use super::Tab;
use super::error::UnpackerError;
use super::listing::BrowserSource;
use super::listing::Glyph;
use super::listing::PaneState;
use super::listing::Rows;
use super::listing::Shared;
use super::listing::Walked;
use super::listing::ancestors;
use super::listing::breadcrumbs;
use super::listing::is_viewable;
use super::listing::parent_directory;
use super::ui::ButtonStyle;
use super::ui::button;
use super::ui::caption;
use super::ui::class;
use super::ui::icon_button;
use super::ui::status;
use super::ui::text_field;
use super::ui::tip;

/// The folder sidebar's width at first and the range a drag keeps it in, as GPUI's.
const SIDEBAR_WIDTH: Points = px(260.0);
const SIDEBAR_MIN_WIDTH: Points = px(160.0);
const SIDEBAR_MAX_WIDTH: Points = px(520.0);

/// The state of one pane. `Copy`: a bundle of signal handles.
#[derive(Clone, Copy)]
pub struct BrowserPane {
    pub source: BrowserSource,
    pub state: Signal<PaneState>,
    /// Directory selected in the tree, in the leading-slash form the tree
    /// keys folders by.
    pub selected_dir: Signal<String>,
    pub filter: Signal<String>,
    pub expanded: Signal<Vec<SharedString>>,
    pub tree_items: Signal<Shared<Vec<TreeItem>>>,
    /// Whether the folder tree is put away behind its rail. Session state:
    /// the Unpacker persists no layout.
    pub sidebar_collapsed: Signal<bool>,
    /// The folder tree's dragged width, so a remade view keeps it.
    pub sidebar_width: Stored<Points>,
    pub search_query: Signal<String>,
    pub path_filter: Signal<String>,
    /// The rows the listing draws, rebuilt only when the VFS, the selected
    /// directory or the filter changes.
    pub rows: Memo<Shared<Rows>>,
}

/// One listing row as the cell draws it.
#[derive(Clone, PartialEq)]
struct RowData {
    label: String,
    is_dir: bool,
    size: Option<u64>,
    path: VfsPath,
}

impl BrowserPane {
    pub fn new(source: BrowserSource) -> Self {
        let state = signal(PaneState::Empty);
        let selected_dir = signal(ROOT_PATH.to_owned());
        let filter = signal(String::new());
        let rows = computed(move || {
            let dir = selected_dir.get();
            let filter = filter.get();
            state.with(|state| Shared::new(Rows::build(state, &dir, &filter)))
        });

        Self {
            source,
            state,
            selected_dir,
            filter,
            expanded: signal(vec![SharedString::from(ROOT_PATH)]),
            tree_items: signal(Shared::new(Vec::new())),
            sidebar_collapsed: signal(false),
            sidebar_width: stored(SIDEBAR_WIDTH),
            search_query: signal(String::new()),
            path_filter: signal(String::new()),
            rows,
        }
    }

    /// Marks the pane as waiting on a background load.
    pub fn set_loading(&self) {
        self.state.set(PaneState::Loading);
    }

    pub fn set_failed(&self, error: Shared<UnpackerError>) {
        self.state.set(PaneState::Failed(error));
    }

    /// Adopts a walked VFS. The selection and the filter are cleared with
    /// the rest: a filter typed against the old build would otherwise
    /// silently apply to the new one.
    pub fn set_ready(&self, walked: Walked) {
        self.tree_items.set(Shared::new(walked.tree_items()));
        self.expanded.set(vec![SharedString::from(ROOT_PATH)]);
        self.selected_dir.set(ROOT_PATH.to_owned());
        self.filter.set(String::new());
        self.state.set(PaneState::Ready(Shared::new(walked)));
    }

    /// Drops the loaded VFS, as when the WoWs directory is cleared.
    pub fn clear(&self) {
        self.tree_items.set(Shared::new(Vec::new()));
        self.selected_dir.set(ROOT_PATH.to_owned());
        self.filter.set(String::new());
        self.state.set(PaneState::Empty);
    }

    /// The walked file list, for a content search over this pane.
    pub fn files(&self) -> Option<Arc<FileList>> {
        self.state.with_untracked(|state| match state {
            PaneState::Ready(walked) => Some(Arc::clone(&walked.files)),
            _ => None,
        })
    }

    pub fn select_dir(&self, path: String) {
        self.selected_dir.set(path);
    }

    /// Shows `path`'s own directory, with the file among the rows, and
    /// clears any filter that would hide it: where a content-search hit
    /// came from, which the hit row cannot show.
    pub fn reveal(&self, path: &VfsPath) {
        let dir = parent_directory(path.as_str());
        self.filter.set(String::new());
        self.expanded.update(|expanded| {
            for ancestor in ancestors(&dir) {
                if !expanded.iter().any(|open| open.as_ref() == ancestor) {
                    expanded.push(SharedString::from(ancestor));
                }
            }
        });
        self.selected_dir.set(dir);
    }
}

/// The pane: a status line until a VFS is loaded, then the sidebar and the
/// listing.
pub fn browser_view(pane: BrowserPane, tab: Tab) -> AnyElement {
    let ready = move || pane.state.with(|state| matches!(state, PaneState::Ready(_)));
    let status_text = move || {
        pane.state.with(|state| match state {
            PaneState::Empty => tab.text("ui.unpacker.no_game_data"),
            PaneState::Loading => match pane.source {
                BrowserSource::Pkg => tab.text("ui.replay.loading"),
                BrowserSource::AssetsBin => tab.text("ui.unpacker.loading_assets"),
            },
            PaneState::Failed(error) => tab.text_with("ui.unpacker.load_failed", &[("error", &**error)]),
            // The status line is hidden while the listing shows.
            PaneState::Ready(_) => String::new(),
        })
    };

    view()
        .app_class(class("flex-1 min-h-0 min-w-0"))
        .child(when(move || !ready(), move || status(status_text)))
        .child(when(ready, move || {
            view()
                .app_class(class("flex-1 min-h-0 flex-row"))
                .child(when(move || pane.sidebar_collapsed.get(), move || sidebar_rail(pane, tab)))
                .child(
                    h_resizable()
                        .app_class(class("flex-1 min-w-0 min-h-0"))
                        .handle_class("bg-border")
                        .child(
                            resizable_panel()
                                .size(pane.sidebar_width.get())
                                .on_resize(move |size: &Points| {
                                    pane.sidebar_width.replace(*size);
                                })
                                .size_range(SIDEBAR_MIN_WIDTH..SIDEBAR_MAX_WIDTH)
                                .visible(move || !pane.sidebar_collapsed.get())
                                .child(when(move || !pane.sidebar_collapsed.get(), move || sidebar(pane, tab))),
                        )
                        .child(resizable_panel().child(listing(pane, tab))),
                )
        }))
        .into_element()
}

fn sidebar_rail(pane: BrowserPane, tab: Tab) -> AnyElement {
    view()
        .app_class(class("w-7 border-r border-border items-center pt-1"))
        .child(
            icon_button(Icon::PanelLeftOpen, tab.text("ui.unpacker.expand_folders"))
                .on_press(move |_: &PressEvent| pane.sidebar_collapsed.set(false)),
        )
        .into_element()
}

fn sidebar(pane: BrowserPane, tab: Tab) -> AnyElement {
    let filter_row = view()
        .app_class(class("flex-row items-center gap-1 px-2 py-1 border-b border-border"))
        .child(
            icon_button(Icon::PanelLeftClose, tab.text("ui.unpacker.collapse_folders"))
                .on_press(move |_: &PressEvent| pane.sidebar_collapsed.set(true)),
        )
        .child(
            text_field(pane.filter, tab.text("ui.unpacker.filter_files"), tab.text("ui.unpacker.filter_files"))
                .app_class(class(
                    "flex-1 min-w-0 min-h-7 px-2 rounded-(--radius) border border-input bg-background text-xs \
                     text-foreground",
                )),
        );

    let folders = tree()
        .app_class(class("flex-1 min-h-0"))
        .accessibility_label(format!("{} folders", tab.text(pane.source.title_key())))
        .items(move || pane.tree_items.with(|items: &Shared<Vec<TreeItem>>| items.0.as_ref().clone()))
        .expanded(move || Some(pane.expanded.get()))
        .selected(move || SelectValue::Controlled(Some(SharedString::from(pane.selected_dir.get()))))
        .item_class("group min-h-6 px-1 rounded-(--radius) hover:bg-accent selected:bg-accent")
        .render_item(|entry: &TreeEntry| folder_row(entry))
        .on_expanded_change(move |expanded: &Vec<SharedString>| pane.expanded.set(expanded.clone()))
        .on_selected_change(move |selected: &SharedString| pane.select_dir(selected.to_string()));

    let search_box = view()
        .app_class(class("gap-1 px-2 py-2 border-t border-border"))
        .child(text(tab.text("ui.unpacker.search_in_files")).app_class(class("text-xs font-semibold text-foreground")))
        .child(
            text_field(pane.search_query, tab.text("ui.unpacker.search_hint"), tab.text("ui.unpacker.search_hint"))
                .on_submit(move |_: &str| tab.start_search(pane)),
        )
        .child(text_field(
            pane.path_filter,
            tab.text("ui.unpacker.path_filter_hint"),
            tab.text("ui.unpacker.path_filter_hint"),
        ))
        .child(
            button(tab.text("ui.tabs.search"), ButtonStyle::Secondary)
                .label(format!("{} {}", tab.text("ui.tabs.search"), tab.text(pane.source.title_key())))
                .on_press(move |_: &PressEvent| tab.start_search(pane)),
        );

    view()
        .app_class(class("flex-1 min-h-0 border-r border-border"))
        .child(filter_row)
        .child(folders)
        .child(search_box)
        .into_element()
}

/// One folder row: indented by depth, a chevron for a folder with
/// children, the directory name.
fn folder_row(entry: &TreeEntry) -> AnyElement {
    let indent = px(entry.depth as f32 * 14.0);
    let folder = entry.folder;

    view()
        .app_class(class("flex-row items-center gap-1"))
        .child(view().w(indent))
        .child(
            view()
                .app_class(class("w-3 text-muted-foreground group-aria-expanded:rotate-90"))
                .children(folder.then(|| icon(Icon::ChevronRight, px(12.0)))),
        )
        // TreeEntry carries no expansion state, so the row's aria-expanded picks the icon.
        .child(
            icon(Icon::Folder, px(14.0))
                .test_id("unpacker-folder")
                .app_class(class("text-primary group-aria-expanded:hidden")),
        )
        .child(
            icon(Icon::FolderOpen, px(14.0))
                .test_id("unpacker-folder-open")
                .app_class(class("hidden text-primary group-aria-expanded:flex")),
        )
        .child(text(entry.label.clone()).app_class(class("text-xs text-foreground")))
        .into_element()
}

fn listing(pane: BrowserPane, tab: Tab) -> AnyElement {
    let crumbs = dynamic(move || {
        breadcrumbs(&pane.selected_dir.get())
            .into_iter()
            .enumerate()
            .flat_map(|(depth, crumb)| {
                let separator = (depth > 0).then(|| caption("/").into_element());
                let path = crumb.path.clone();
                let link = button(crumb.label.clone(), ButtonStyle::Ghost)
                    .label(format!("{}: {}", tab.text("ui.unpacker.go_to_directory"), crumb.label))
                    .on_press(move |_: &PressEvent| {
                        pane.filter.set(String::new());
                        pane.select_dir(path.clone());
                    })
                    .into_element();
                separator.into_iter().chain(std::iter::once(link))
            })
            .collect::<Vec<_>>()
    });

    let header = view()
        .app_class(class("flex-row items-center gap-2 px-2 py-1 border-b border-border"))
        .child(
            caption(move || tab.text_with("ui.unpacker.item_count", &[("count", &pane.rows.with(|rows| rows.len()))]))
                .app_class(class("flex-1 text-xs text-muted-foreground")),
        )
        .child(
            button(tab.text("ui.unpacker.queue_all_files"), ButtonStyle::Secondary)
                .label(format!("{} ({})", tab.text("ui.unpacker.queue_all_files"), tab.text(pane.source.title_key())))
                .disabled(move || pane.rows.with(|rows| rows.is_empty()))
                .on_press(move |_: &PressEvent| tab.queue_listed(pane.rows.with_untracked(|rows| rows.all()))),
        );

    let columns = view()
        .app_class(class("flex-row items-center gap-1 h-6 px-2 bg-muted border-b border-border"))
        .child(view().w(px(20.0)))
        .child(view().w(px(24.0)))
        .child(text(tab.text("ui.unpacker.column.name")).app_class(class("flex-1 text-xs font-semibold")))
        .child(text(tab.text("ui.unpacker.column.size")).app_class(class("w-24 text-xs font-semibold")))
        .child(text(tab.text("ui.unpacker.column.type_col")).app_class(class("w-20 text-xs font-semibold")));

    let rows = scroll().app_class(class("flex-1 min-h-0")).child(
        virtual_list(move || pane.rows.with(|rows| rows.len()), move |at: Position| listing_row(pane, tab, at))
            .keys(move || pane.rows.with(|rows| rows.keys()))
            .estimated_item_size(px(24.0))
            .viewport(ListViewport::Parent),
    );

    view()
        .app_class(class("flex-1 min-w-0 min-h-0"))
        .child(view().app_class(class("flex-row flex-wrap items-center gap-1 px-2 py-1")).child(crumbs))
        .child(header)
        .child(columns)
        .child(rows)
        .into_element()
}

fn listing_row(pane: BrowserPane, tab: Tab, at: Position) -> AnyElement {
    let data = computed(move || {
        pane.rows.with(|rows| {
            rows.row(at.index()).map(|entry| RowData {
                label: entry.label,
                is_dir: entry.is_dir,
                size: entry.size,
                path: entry.path,
            })
        })
    });
    // A row past the end, while the list catches up with a shorter listing,
    // shows blank.
    let field = move |read: fn(&RowData) -> String| move || data.with(|row| row.as_ref().map(read).unwrap_or_default());
    let is_dir = move || data.with(|row| row.as_ref().is_some_and(|row| row.is_dir));
    let path = move || data.with_untracked(|row| row.as_ref().map(|row| row.path.clone()));
    let viewable = move || data.with(|row| row.as_ref().is_some_and(|row| !row.is_dir && is_viewable(&row.label)));
    let decodable = move || {
        data.with(|row| row.as_ref().is_some_and(|row| !row.is_dir && decodable_prototype(&row.label).is_some()))
    };

    let queue_toggle = checkbox()
        .accessibility_label(move || {
            let key = if is_dir() { "ui.unpacker.queue_folder" } else { "ui.unpacker.queue_file" };
            let label = data.with(|row| row.as_ref().map(|row| row.label.clone()).unwrap_or_default());
            Some(SharedString::from(format!("{}: {label}", tab.text(key))))
        })
        .checked(move || data.with(|row| row.as_ref().is_some_and(|row| tab.is_queued(&row.path))))
        .on_change(move |_: &bool| {
            if let Some(path) = path() {
                tab.toggle_queued(path);
            }
        })
        .app_class(class("w-4 h-4 rounded-sm border border-border checked:bg-primary"));

    let row = view()
        .accessible(true)
        .label(move || data.with(|row| row.as_ref().map(|row| row.label.clone()).unwrap_or_default()))
        .app_class(move || {
            Some(SharedString::from(if at.index() % 2 == 1 {
                "flex-row items-center gap-1 h-6 px-2 bg-muted/40 hover:bg-accent"
            } else {
                "flex-row items-center gap-1 h-6 px-2 hover:bg-accent"
            }))
        })
        .on_press(move |press: &PressEvent| {
            if press.clicks < 2 {
                return;
            }
            let Some(path) = path() else {
                return;
            };
            if is_dir() {
                pane.filter.set(String::new());
                pane.select_dir(path.as_str().to_owned());
            } else {
                tab.open_viewer(path);
            }
        })
        .child(
            view()
                .w(px(20.0))
                .tooltip(tip(move || {
                    tab.text(if is_dir() { "ui.unpacker.queue_folder" } else { "ui.unpacker.queue_file" })
                }))
                .child(queue_toggle),
        )
        .child(view().app_class(class("w-6 text-primary")).child(dynamic(move || {
            data.with(|row| {
                row.as_ref()
                    .map(|row| icon(Glyph::of(&row.label, row.is_dir).icon(), px(14.0)))
                    .into_iter()
                    .collect::<Vec<_>>()
            })
        })))
        .child(text(field(|row| row.label.clone())).app_class(class("flex-1 min-w-0 text-xs text-foreground truncate")))
        .child(
            // A directory has no size to show.
            text(field(|row| row.size.map(byte_size).unwrap_or_default()))
                .app_class(class("w-24 text-xs text-muted-foreground")),
        )
        .child(
            text(field(|row| if row.is_dir { "Folder" } else { file_type_label(&row.label) }.to_owned()))
                .app_class(class("w-20 text-xs text-muted-foreground")),
        );

    let menu = context_menu()
        .trigger(row)
        .popup_class("min-w-48 p-1 rounded-(--radius) border border-border bg-popover")
        .child(when(viewable, move || {
            menu_item()
                .accessibility_label(tab.text("ui.unpacker.view_contents"))
                .app_class(class("min-h-7 px-2 rounded-(--radius) justify-center hover:bg-accent"))
                .on_select(move || {
                    if let Some(path) = path() {
                        tab.open_viewer(path);
                    }
                })
                .child(text(tab.text("ui.unpacker.view_contents")).app_class(class("text-xs text-popover-foreground")))
        }))
        .child(when(decodable, move || {
            vec![
                menu_item()
                    .accessibility_label(tab.text("ui.unpacker.view_as_json"))
                    .app_class(class("min-h-7 px-2 rounded-(--radius) justify-center hover:bg-accent"))
                    .on_select(move || {
                        if let Some(path) = path() {
                            tab.open_json_viewer(path);
                        }
                    })
                    .child(
                        text(tab.text("ui.unpacker.view_as_json")).app_class(class("text-xs text-popover-foreground")),
                    )
                    .into_element(),
                menu_item()
                    .accessibility_label(tab.text("ui.unpacker.extract_as_json"))
                    .app_class(class("min-h-7 px-2 rounded-(--radius) justify-center hover:bg-accent"))
                    .on_select(move || {
                        if let Some(path) = path() {
                            tab.extract_as_json(path);
                        }
                    })
                    .child(
                        text(tab.text("ui.unpacker.extract_as_json"))
                            .app_class(class("text-xs text-popover-foreground")),
                    )
                    .into_element(),
            ]
        }));

    // A fixed height: the list places rows by their measured size, and the
    // menu's closed popup must not add to it.
    view().app_class(class("h-6 overflow-hidden")).child(menu).into_element()
}
