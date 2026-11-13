//! One browser pane drawn: a resizable folder-tree sidebar beside a virtualized file listing,
//! with a path filter above the tree and the content-search inputs pinned below it.

use std::rc::Rc;

use landaire_ui::BranchClick;
use landaire_ui::CollapseButtonProps;
use landaire_ui::ExpandRailProps;
use landaire_ui::LeafLead;
use landaire_ui::LoadingProps;
use landaire_ui::MarkedTextProps;
use landaire_ui::NodeKind;
use landaire_ui::PanelSide;
use landaire_ui::SidePanel;
use landaire_ui::SideSplitProps;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use landaire_ui::TreeMenu;
use landaire_ui::TreeProps;
use landaire_ui::TreeRow;
use landaire_ui::TreeStripes;
use landaire_ui::WidthRange;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::formatting::byte_size;
use wows_toolkit_viewmodel::unpacker::listing::ListingEntry;
use wows_toolkit_viewmodel::unpacker::listing::breadcrumbs;
use wows_toolkit_viewmodel::unpacker::viewer::FileGlyph;
use wows_toolkit_viewmodel::unpacker::viewer::decodable_prototype;
use wows_toolkit_viewmodel::unpacker::viewer::is_viewable;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::InputProps;
use zgui_ui::MenuItemProps;
use zgui_ui::prelude::Binding;

use crate::icons::AppIconProps;
use crate::tabs::unpacker::model::UnpackerModel;
use crate::tabs::unpacker::pane::DirKey;
use crate::tabs::unpacker::pane::Pane;
use crate::tabs::unpacker::pane::PaneState;
use crate::tabs::unpacker::pane::RowKey;
use crate::tabs::unpacker::pane::SIDEBAR_MAX_WIDTH;
use crate::tabs::unpacker::pane::SIDEBAR_MIN_WIDTH;
use crate::text_marks::marks;

const SHEET: &str = "wt-unpacker-browser";

const CSS: &str = css!(
    ".wt-pane { display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-pane__state {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        align-items: center;
        justify-content: center;
        gap: 8px;
        font-size: 13px;
        color: var(--wt-color-text-dim);
    }
    .wt-pane__sidebar { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-pane__bar {
        display: flex;
        flex: none;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-pane__field { flex: 1 1 auto; min-width: 0; }
    .wt-pane__tree { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-pane__tree .lui-tree-area, .wt-pane__tree .lui-tree { flex: 1; min-height: 0; }
    .wt-pane__search {
        display: flex;
        flex-direction: column;
        flex: none;
        gap: 4px;
        padding: 4px 8px;
        border-top: 1px solid var(--lui-color-border);
    }
    .wt-pane__strong { font-size: 12px; font-weight: 700; }
    .wt-pane__row { display: flex; align-items: center; gap: 4px; }
    .wt-pane__main { display: flex; flex-direction: row; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-listing-area { display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-crumbs { display: flex; flex: none; align-items: center; gap: 4px; padding: 4px 8px; }
    .wt-crumbs__slash { flex: none; font-size: 12px; color: var(--wt-color-text-faint); }
    .wt-listing-head {
        display: flex;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-listing-head__count { flex: 1 1 auto; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-columns {
        display: flex;
        flex: none;
        align-items: center;
        gap: 4px;
        height: 22px;
        padding: 0 8px;
        background-color: var(--lui-color-surface);
        border-bottom: 1px solid var(--lui-color-border);
        font-size: 12px;
        font-weight: 700;
    }
    .wt-files { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-files .lui-tree-area, .wt-files .lui-tree { flex: 1; min-height: 0; }
    .wt-file { display: flex; flex: 1; align-items: center; gap: 4px; height: 100%; min-width: 0; padding-right: 8px; }
    .wt-file__queue { display: flex; flex: none; width: 20px; justify-content: center; }
    .wt-file__glyph { display: flex; flex: none; width: 16px; }
    .wt-file__dir { color: var(--lui-color-accent); }
    .wt-file__name {
        flex: 1 1 auto;
        min-width: 0;
        font-size: 13px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-file__size, .wt-file__type { flex: none; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-file__size { width: 90px; }
    .wt-file__type { width: 70px; }
    .wt-folder { display: flex; flex: 1; align-items: center; gap: 4px; min-width: 0; font-size: 13px; }
    .wt-folder__glyph { color: var(--lui-color-accent); }"
);

/// One browser pane.
#[component]
pub fn BrowserPane(model: UnpackerModel, pane: Pane) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let fragment = pane.source.id_fragment();
    let body = move || match pane.state.get() {
        PaneState::Empty => AnyView::new(view! {
            box(class = "wt-pane__state", attr:data-testid = format!("unpacker-{fragment}-empty")) {
                text {{t!("ui.unpacker.no_game_data").into_owned()}}
            }
        }),
        PaneState::Loading => AnyView::new(view! {
            Loading(label = t!("ui.replay.loading").into_owned(), attr:data-testid = format!("unpacker-{fragment}-loading"))
        }),
        PaneState::Failed(reason) => AnyView::new(view! {
            box(class = "wt-pane__state", attr:data-testid = format!("unpacker-{fragment}-failed")) {
                text {{t!("ui.unpacker.load_failed", error = reason).into_owned()}}
            }
        }),
        PaneState::Ready(_) => AnyView::new(view! { ReadyPane(model = model, pane = pane) }),
    };

    view! {
        column(class = "wt-pane", attr:data-testid = format!("unpacker-{fragment}-pane")) {{body}}
    }
}

/// The sidebar and the listing, once the pane has a VFS.
#[component]
fn ReadyPane(model: UnpackerModel, pane: Pane) -> impl IntoView {
    let fragment = pane.source.id_fragment();
    let range = WidthRange { min: SIDEBAR_MIN_WIDTH, max: SIDEBAR_MAX_WIDTH };

    view! {
        SideSplit(
            open = pane.sidebar_open,
            width = pane.sidebar_width,
            range = range,
            resize_label = t!("ui.unpacker.collapse_folders").into_owned(),
            attr:data-testid = format!("unpacker-{fragment}-split")
        ) {
            SidePanel(slot) {
                Sidebar(model = model, pane = pane)
            }
            row(class = "wt-pane__main") {
                Show(when = move || !pane.sidebar_open.get()) {
                    ExpandRail(
                        side = PanelSide::Left,
                        label = t!("ui.unpacker.expand_folders").into_owned(),
                        attr:data-testid = format!("unpacker-{fragment}-tree-rail"),
                        on:click = move |_| pane.sidebar_open.set(true)
                    )
                }
                Listing(model = model, pane = pane)
            }
        }
    }
}

/// The filter, the folder tree and the content search.
#[component]
fn Sidebar(model: UnpackerModel, pane: Pane) -> impl IntoView {
    let fragment = pane.source.id_fragment();
    let model_tree: Signal<Rc<crate::tabs::unpacker::pane::FolderTree>, LocalStorage> = pane.tree.into();
    let start_search = move || {
        let query = pane.search_query.get_untracked().trim().to_string();
        let Some(files) = pane.files().filter(|_| !query.is_empty()) else { return };
        let path_filter = pane.path_filter.get_untracked().trim().to_string();
        model.open_search(pane.source, query, path_filter, files);
    };
    let on_search_key = move |ev: &mut EventCx<'_, events::KeyDown>| {
        if ev.key == Key::Named(NamedKey::Enter) {
            start_search();
        }
    };

    view! {
        column(class = "wt-pane__sidebar") {
            row(class = "wt-pane__bar") {
                CollapseButton(
                    side = PanelSide::Left,
                    label = t!("ui.unpacker.collapse_folders").into_owned(),
                    attr:data-testid = format!("unpacker-{fragment}-tree-collapse"),
                    on:click = move |_| pane.sidebar_open.set(false)
                )
                AppIcon(glyph = Glyph::Search)
                Input(
                    value = pane.filter,
                    placeholder = t!("ui.unpacker.filter_files").into_owned(),
                    label = t!("ui.unpacker.filter_files").into_owned(),
                    class = "wt-pane__field",
                    attr:data-testid = format!("unpacker-{fragment}-filter")
                )
            }
            box(class = "wt-pane__tree") {
                Tree(
                    model = model_tree,
                    row = move |row: TreeRow<DirKey>| folder_row(pane, row),
                    expanded = pane.expanded,
                    selection = pane.tree_selection,
                    handle = pane.tree_handle,
                    label = pane.source.title(),
                    attr:data-testid = format!("unpacker-{fragment}-tree")
                )
            }
            column(class = "wt-pane__search") {
                text(class = "wt-pane__strong") {{t!("ui.unpacker.search_in_files").into_owned()}}
                row(class = "wt-pane__row") {
                    AppIcon(glyph = Glyph::Search)
                    Input(
                        value = pane.search_query,
                        placeholder = t!("ui.unpacker.search_hint").into_owned(),
                        label = t!("ui.unpacker.search_hint").into_owned(),
                        class = "wt-pane__field",
                        attr:data-testid = format!("unpacker-{fragment}-search"),
                        on:key_down = on_search_key
                    )
                }
                Input(
                    value = pane.path_filter,
                    placeholder = t!("ui.unpacker.path_filter_hint").into_owned(),
                    label = t!("ui.unpacker.path_filter_hint").into_owned(),
                    attr:data-testid = format!("unpacker-{fragment}-path-filter"),
                    on:key_down = on_search_key
                )
                Button(
                    size = ButtonSize::Sm,
                    variant = ButtonVariant::Outline,
                    attr:data-testid = format!("unpacker-{fragment}-search-run"),
                    on:click = move |_| start_search()
                ) {
                    {t!("ui.tabs.search").into_owned()}
                }
            }
        }
    }
}

/// One folder row: a folder glyph that opens with the directory, and the directory's name.
fn folder_row(pane: Pane, row: TreeRow<DirKey>) -> impl IntoView {
    let expanded = row.expanded;
    let glyph = Signal::derive_local(move || if expanded.get() { Glyph::FolderOpen } else { Glyph::Folder });
    let key = row.key;
    let name = move || pane.tree.with(|tree| tree.name(&key).to_string());

    view! {
        row(class = "wt-folder") {
            AppIcon(glyph = glyph, class = "wt-folder__glyph")
            text {{name}}
        }
    }
}

/// The path bar, the listing's own actions, its column names and its rows.
#[component]
fn Listing(model: UnpackerModel, pane: Pane) -> impl IntoView {
    let fragment = pane.source.id_fragment();
    let listing: Signal<Rc<crate::tabs::unpacker::pane::ListingModel>, LocalStorage> = pane.listing.into();
    let count = Signal::derive_local(move || pane.rows.with(|rows| rows.len()));
    let crumbs = move || {
        breadcrumbs(&pane.selected_dir.get())
            .into_iter()
            .enumerate()
            .map(|(depth, crumb)| {
                let path = crumb.path;
                let slash = (depth > 0).then(|| view! { text(class = "wt-crumbs__slash") {"/"} });
                AnyView::new(view! {
                    {slash.map(IntoView::into_view)}
                    Button(
                        size = ButtonSize::Xs,
                        variant = ButtonVariant::Ghost,
                        attr:data-testid = format!("unpacker-{fragment}-crumb-{depth}"),
                        on:click = move |_| pane.select_dir(path.clone())
                    ) {
                        {crumb.label}
                    }
                })
            })
            .collect::<Vec<AnyView>>()
    };
    let on_activate = UnsyncCallback::new(move |key: RowKey| {
        let Some(entry) = pane.rows.with_untracked(|rows| rows.row(key.0)) else { return };
        if entry.is_dir {
            pane.select_dir(entry.path.as_str().to_string());
        } else {
            model.open_viewer(entry.path);
        }
    });
    let menu = TreeMenu::new(move |key: &RowKey| row_menu(model, pane, *key));

    view! {
        column(class = "wt-listing-area") {
            row(class = "wt-crumbs", attr:data-testid = format!("unpacker-{fragment}-crumbs")) {{crumbs}}
            row(class = "wt-listing-head") {
                text(class = format!("wt-listing-head__count {TABULAR_CLASS}")) {
                    {move || t!("ui.unpacker.item_count", count = count.get()).into_owned()}
                }
                Button(
                    size = ButtonSize::Sm,
                    variant = ButtonVariant::Outline,
                    disabled = Signal::derive_local(move || count.get() == 0),
                    attr:data-testid = format!("unpacker-{fragment}-extract-listed"),
                    on:click = move |_| model.queue_listed(&pane.rows.with_untracked(|rows| rows.all()))
                ) {
                    AppIcon(glyph = Glyph::Download)
                    text {{t!("ui.unpacker.queue_all_files").into_owned()}}
                }
            }
            // Named, since a bare number and a bare word at the end of a row say nothing about
            // which is the size and which the type.
            row(class = "wt-columns") {
                box(class = "wt-file__queue")
                box(class = "wt-file__glyph")
                text(class = "wt-file__name") {{t!("ui.unpacker.column.name").into_owned()}}
                text(class = "wt-file__size") {{t!("ui.unpacker.column.size").into_owned()}}
                text(class = "wt-file__type") {{t!("ui.unpacker.column.type_col").into_owned()}}
            }
            box(class = "wt-files") {
                Tree(
                    model = listing,
                    row = move |row: TreeRow<RowKey>| file_row(model, pane, row),
                    selection = pane.listing_selection,
                    branch_click = BranchClick::Select,
                    stripes = TreeStripes::Rows,
                    leaf_lead = LeafLead::Nothing,
                    guides = false,
                    on_activate = on_activate,
                    context_menu = menu,
                    label = pane.source.title(),
                    attr:data-testid = format!("unpacker-{fragment}-listing")
                )
            }
        }
    }
}

/// One listing row. Read through the rows, since a row is kept by its position while the
/// directory it lists changes.
fn file_row(model: UnpackerModel, pane: Pane, row: TreeRow<RowKey>) -> impl IntoView {
    debug_assert_eq!(row.kind, NodeKind::Leaf);
    let ix = row.key.0;
    let fragment = pane.source.id_fragment();
    let content = move || {
        let entry = pane.rows.with(|rows| rows.row(ix))?;
        Some(file_row_content(model, pane, ix, entry))
    };

    view! {
        row(class = "wt-file", attr:data-testid = format!("unpacker-{fragment}-row-{ix}")) {{content}}
    }
}

fn file_row_content(model: UnpackerModel, pane: Pane, ix: usize, entry: ListingEntry) -> AnyView {
    let fragment = pane.source.id_fragment();
    let path = entry.path.clone();
    let queued = {
        let path = path.clone();
        Signal::derive_local(move || Checked::from(model.queue.with(|queue| queue.entries().contains(&path))))
    };
    let toggle = {
        let path = path.clone();
        move |checked: Checked| {
            if checked == Checked::Yes {
                model.queue_path(path.clone());
            } else {
                model.unqueue(&path);
            }
        }
    };
    let hover = if entry.is_dir { t!("ui.unpacker.queue_folder") } else { t!("ui.unpacker.queue_file") }.into_owned();
    let label = hover.clone();
    let glyph = if entry.is_dir {
        AnyView::new(view! { AppIcon(glyph = Glyph::Folder, class = "wt-file__dir") })
    } else {
        let glyph = match FileGlyph::of(&entry.label) {
            FileGlyph::Image => Glyph::Image,
            FileGlyph::Text => Glyph::FileText,
            FileGlyph::Audio => Glyph::Music,
            FileGlyph::Other => Glyph::File,
        };
        AnyView::new(view! { AppIcon(glyph = glyph) })
    };
    let size = entry.size.map(byte_size).unwrap_or_default();
    // A directory listing and a glob mark nothing.
    let label_marks =
        pane.rows.with_untracked(|rows| rows.query().map(|query| marks(query, &entry.label)).unwrap_or_default());

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = "wt-file__queue") {
                Checkbox(
                    checked = Binding::controlled(queued, toggle),
                    a11y:label = label,
                    attr:data-testid = format!("unpacker-{fragment}-queue-{ix}")
                )
            }
            TooltipContent {{hover.clone()}}
        }
        box(class = "wt-file__glyph") {{glyph}}
        MarkedText(class = "wt-file__name", text = entry.label.clone(), ranges = label_marks)
        text(class = format!("wt-file__size {TABULAR_CLASS}")) {{size}}
        text(class = "wt-file__type") {{entry.type_label()}}
    })
}

/// A file the viewer understands opens in it; a prototype the decoder understands also reads
/// and writes as JSON.
fn row_menu(model: UnpackerModel, pane: Pane, key: RowKey) -> AnyView {
    let Some(entry) = pane.rows.with_untracked(|rows| rows.row(key.0)) else {
        return AnyView::new(());
    };
    if entry.is_dir {
        return AnyView::new(());
    }
    let viewable = is_viewable(&entry.label).then(|| {
        let path = entry.path.clone();
        view! {
            MenuItem(on_select = UnsyncCallback::new(move |()| model.open_viewer(path.clone()))) {
                {t!("ui.unpacker.view_contents").into_owned()}
            }
        }
    });
    let decodable = decodable_prototype(&entry.label).is_some().then(|| {
        let (shown, written) = (entry.path.clone(), entry.path.clone());
        view! {
            MenuItem(on_select = UnsyncCallback::new(move |()| model.open_json_viewer(shown.clone()))) {
                {t!("ui.unpacker.view_as_json").into_owned()}
            }
            MenuItem(on_select = UnsyncCallback::new(move |()| model.extract_as_json(written.clone()))) {
                {t!("ui.unpacker.extract_as_json").into_owned()}
            }
        }
    });

    AnyView::new(view! {
        {viewable.map(IntoView::into_view)}
        {decodable.map(IntoView::into_view)}
    })
}
