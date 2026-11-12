//! A content-search results panel: the query and its progress above a virtualized list of hits.
//! One panel per search, so an earlier result set survives the next run.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use landaire_ui::TABULAR_CLASS;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::unpacker::search::ContentSearchHit;
use wows_toolkit_viewmodel::unpacker::search::SearchProgress;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::ProgressProps;
use zgui_ui::VirtualListProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::ui::ELLIPSIS;

use crate::icons::AppIconProps;
use crate::tabs::unpacker::model::UnpackerModel;
use crate::tabs::unpacker::pane::BrowserSource;

const SHEET: &str = "wt-unpacker-search";

const CSS: &str = css!(
    ".wt-hits { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-hits__header {
        display: flex;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-hits__query { font-size: 13px; }
    .wt-hits__dim { font-size: 12px; color: var(--wt-color-text-dim); white-space: nowrap; }
    .wt-hits__count { flex: 1 1 auto; min-width: 0; }
    .wt-hits__progress { flex: none; width: 120px; }
    .wt-hits__list { flex: 1 1 auto; min-height: 0; }
    .wt-hits__empty {
        display: flex;
        flex: 1 1 auto;
        align-items: center;
        justify-content: center;
        font-size: 13px;
        color: var(--wt-color-text-dim);
    }
    .wt-hit {
        display: flex;
        align-items: center;
        gap: 4px;
        height: 34px;
        padding: 0 8px;
    }
    .wt-hit:hover { background-color: var(--lui-color-widget-hot); }
    .wt-hit__body {
        display: flex;
        flex-direction: column;
        justify-content: center;
        flex: 1 1 auto;
        min-width: 0;
    }
    .wt-hit__path, .wt-hit__context {
        font-size: 12px;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-hit__line { display: flex; gap: 4px; min-width: 0; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-hit__offset { flex: none; }"
);

/// The row height of a hit: its path and the context line under it.
const ROW_HEIGHT: f32 = 34.0;

/// One search's results. `Copy`: every field is a handle on state owned by the tab.
#[derive(Clone, Copy)]
pub struct SearchResults {
    pub query: StoredValue<String, LocalStorage>,
    pub source: BrowserSource,
    pub hits: RwSignal<Vec<ContentSearchHit>, LocalStorage>,
    pub progress: RwSignal<SearchProgress, LocalStorage>,
    pub running: RwSignal<bool, LocalStorage>,
    /// Set to end the scan early; the worker polls it per file.
    pub stop_flag: StoredValue<Arc<AtomicBool>, LocalStorage>,
}

impl SearchResults {
    pub fn new(query: String, source: BrowserSource) -> Self {
        Self {
            query: StoredValue::new_local(query),
            source,
            hits: RwSignal::new_local(Vec::new()),
            progress: RwSignal::new_local(SearchProgress { scanned: 0, total: 0 }),
            running: RwSignal::new_local(true),
            stop_flag: StoredValue::new_local(Arc::new(AtomicBool::new(false))),
        }
    }

    pub fn title(self) -> String {
        t!("ui.unpacker.search_title", query = self.query.get_value()).into_owned()
    }

    pub fn extend(self, hits: Vec<ContentSearchHit>) {
        if !hits.is_empty() {
            self.hits.update(|held| held.extend(hits));
        }
    }

    pub fn stop(self) {
        self.stop_flag.get_value().store(true, Ordering::Relaxed);
        self.running.set(false);
    }
}

#[component]
pub fn SearchPanel(model: UnpackerModel, results: SearchResults) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let fraction = Signal::derive_local(move || {
        let progress = results.progress.get();
        (progress.total > 0).then(|| progress.scanned as f64 / progress.total as f64 * 100.0)
    });
    let count = move || {
        let progress = results.progress.get();
        format!("{} hits, {} of {} files", results.hits.with(Vec::len), progress.scanned, progress.total)
    };
    let body = move || {
        if results.hits.with(Vec::is_empty) && !results.running.get() {
            return AnyView::new(view! {
                box(class = "wt-hits__empty", attr:data-testid = "unpacker-search-empty") {
                    text {{t!("ui.search.no_matches").into_owned()}}
                }
            });
        }
        AnyView::new(view! {
            VirtualList(
                count = Signal::derive_local(move || results.hits.with(Vec::len)),
                row_size = ROW_HEIGHT,
                label = results.title(),
                class = "wt-hits__list",
                attr:data-testid = "unpacker-search-hits",
                row = move |ix: usize| hit_row(model, results, ix)
            )
        })
    };

    view! {
        column(class = "wt-hits", attr:data-testid = "unpacker-search-panel") {
            row(class = "wt-hits__header") {
                AppIcon(glyph = Glyph::Search)
                text(class = "wt-hits__query") {{results.query.get_value()}}
                text(class = "wt-hits__dim") {{t!("ui.unpacker.search_in", source = results.source.title()).into_owned()}}
                text(class = format!("wt-hits__dim wt-hits__count {TABULAR_CLASS}")) {{count}}
                Show(when = move || results.running.get() && fraction.get().is_some()) {
                    box(class = "wt-hits__progress") {
                        Progress(value = fraction, attr:data-testid = "unpacker-search-progress")
                    }
                }
                Button(
                    size = ButtonSize::Xs,
                    variant = ButtonVariant::Outline,
                    disabled = Signal::derive_local(move || !results.running.get()),
                    attr:data-testid = "unpacker-search-stop",
                    on:click = move |_| results.stop()
                ) {
                    {t!("ui.buttons.stop").into_owned()}
                }
            }
            {body}
        }
    }
}

/// One hit: its path, where in the file the match is and the text around it, and its menu.
fn hit_row(model: UnpackerModel, results: SearchResults, ix: usize) -> AnyView {
    let Some(hit) = results.hits.with_untracked(|hits| hits.get(ix).cloned()) else {
        return AnyView::new(());
    };
    let path = StoredValue::new_local(hit.vfs_path.clone());
    let source = results.source;

    AnyView::new(view! {
        row(class = "wt-hit", attr:data-testid = format!("unpacker-hit-{ix}")) {
            column(
                class = "wt-hit__body",
                on:double_click = move |_| model.open_viewer(path.get_value())
            ) {
                text(class = "wt-hit__path") {{hit.path.clone()}}
                row(class = "wt-hit__line") {
                    text(class = format!("wt-hit__offset {TABULAR_CLASS}")) {
                        {t!("ui.unpacker.hit_offset", offset = hit.offset).into_owned()}
                    }
                    text(class = "wt-hit__context") {{hit.context.clone()}}
                }
            }
            DropdownMenu {
                DropdownMenuTrigger(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconXs,
                    attr:data-testid = format!("unpacker-hit-actions-{ix}"),
                    a11y:label = hit.path.clone()
                ) {
                    Icon(icon = ELLIPSIS, size = IconSize::Sm)
                }
                DropdownMenuContent {
                    MenuItem(on_select = UnsyncCallback::new(move |()| model.open_viewer(path.get_value()))) {
                        {t!("ui.unpacker.view_contents").into_owned()}
                    }
                    MenuItem(on_select = UnsyncCallback::new(move |()| model.queue_path(path.get_value()))) {
                        {t!("ui.unpacker.queue").into_owned()}
                    }
                    MenuItem(on_select = UnsyncCallback::new(move |()| model.reveal(source, path.get_value()))) {
                        {t!("ui.unpacker.reveal_in_tree").into_owned()}
                    }
                }
            }
        }
    })
}
