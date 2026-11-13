//! The Search tab: a query over the replay index, and its results.
//!
//! The query language, its SQL and the sort a header press asks for live in the shared index;
//! the pills, completions and saved state in `wows_toolkit_viewmodel`. This is the bar, the
//! results table and their wiring.

mod bar;
mod model;
#[cfg(test)]
pub(crate) mod tests;

use std::rc::Rc;

use landaire_ui::ColumnId;
use landaire_ui::ColumnLayout;
use landaire_ui::ColumnSizing;
use landaire_ui::ColumnSort;
use landaire_ui::ColumnWidth;
use landaire_ui::GridColumn;
use landaire_ui::GridProps;
use landaire_ui::GridSelectionMode;
use landaire_ui::GridStrings;
use landaire_ui::LoadingProps;
use landaire_ui::MenuItems;
use landaire_ui::RowHeight;
use landaire_ui::RowHover;
use landaire_ui::RowMenu;
use landaire_ui::RowMode;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortDirection;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_icons::Glyph;
use wows_toolkit_services::child_process::reveal_in_file_manager;
use wows_toolkit_services::paths::without_verbatim_prefix;
use wows_toolkit_viewmodel::listing_row::hover_facts_for_match;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use wows_toolkit_viewmodel::search::game_mode_gap_applies;
use wows_toolkit_viewmodel::search::ship_display_name;
use wows_toolkit_viewmodel::search_results::RESULT_LIMIT;
use wows_toolkit_viewmodel::search_results::ResultColumn;
use wows_toolkit_viewmodel::search_results::cell_text;
use wows_toolkit_viewmodel::search_results::game_mode_gap_hint;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuLabelProps;
use zgui_ui::MenuSeparatorProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::ui::ELLIPSIS;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::icons::AppIconProps;
use crate::preview::HoverPreview;
use crate::preview::PreviewTarget;
use crate::preview::card::HoverPreviewCardProps;
use crate::services::SettingsState;
use crate::shell::ReplayRequest;
use crate::shell::Shell;
use crate::shell::palette::TabRequest;
use crate::shell::tabs::AppTab;
use crate::tabs::replays::browser::files::copy_replay_files;
use crate::tabs::search::bar::QueryBarProps;
use crate::tabs::search::model::Search;
use crate::tabs::search::model::Status;
use crate::theme::rating_band_name;

const SHEET: &str = "wt-search";

const CSS: &str = css!(
    ".wt-search {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
    }
    .wt-search-strip {
        flex: none;
        padding: 4px 8px;
        min-height: 14px;
        font-size: var(--zui-type-size-xs);
        color: var(--wt-color-warn);
    }
    .wt-search-strip--error {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
    }
    .wt-search-body {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-height: 0;
    }
    .wt-search-body .lui-grid { flex: 1 1 auto; min-height: 0; }
    .wt-search-status {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        align-items: center;
        justify-content: center;
        gap: 8px;
        font-size: var(--zui-type-size-sm);
        color: var(--lui-color-text-dim);
    }
    .wt-search-footer {
        flex: none;
        min-height: 16px;
        padding: 4px 8px;
        border-top: 1px solid var(--lui-color-border);
        font-size: var(--zui-type-size-xs);
        color: var(--lui-color-text-dim);
    }
    .wt-search-cell {
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--zui-type-size-sm);
    }
    .wt-search-cell[data-outcome='win'] { color: var(--wt-color-win); }
    .wt-search-cell[data-outcome='loss'] { color: var(--wt-color-loss); }
    .wt-search-cell[data-outcome='draw'] { color: var(--wt-color-draw); }
    .wt-search-actions {
        display: flex;
        flex-direction: row;
        align-items: center;
        visibility: hidden;
    }
    .lui-grid__row:hover .wt-search-actions { visibility: visible; }
    .wt-search-actions .zui-button { height: 20px; width: 24px; padding: 0; }
    .wt-search-facts { display: flex; flex-direction: column; max-width: 384px; }
    .wt-search-fact { display: flex; flex-direction: row; gap: 8px; font-size: var(--zui-type-size-xs); }
    .wt-search-fact__label { flex: none; width: 64px; color: var(--lui-color-text-dim); }
    .wt-search-fact__value { flex: 1 1 auto; min-width: 0; }"
);

/// The settings row the dragged column widths are kept in, in [`ResultColumn::all`] order.
const COLUMN_WIDTHS_KEY: &str = "search_column_widths";
/// The pinned columns, in order. Not shared with the GPUI front end, which does not pin.
const PINNED_COLUMNS_KEY: &str = "search_pinned_columns_zgui";

/// The column the row's actions sit in, last.
const ACTIONS_COLUMN: &str = "actions";

/// Each row is this tall, which is what lets the table build only the rows in view.
const ROW_HEIGHT: RowHeight = RowHeight(24.0);

/// One result as the table draws it.
#[derive(Clone)]
struct ResultRow {
    /// Where it sits in its run's results, which keys it.
    position: usize,
    generation: u64,
    hit: Rc<MatchHit>,
    exists: bool,
    ship: String,
}

impl PartialEq for ResultRow {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
            && self.generation == other.generation
            && self.exists == other.exists
            && self.ship == other.ship
    }
}

/// The tab, which is also reached by the palette's seeded searches.
#[component]
pub fn SearchTab() -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let shell = Shell::current();
    let services = shell.services();
    let search = Search::new(services);
    #[cfg(test)]
    tests::publish(search);
    // Held here rather than by the table, which is rebuilt for every run.
    let layout = RwSignal::new_local(ColumnLayout::new(
        ResultColumn::all()
            .into_iter()
            .map(|column| ColumnId::new(column_id(column)))
            .chain([ColumnId::new(ACTIONS_COLUMN)]),
    ));
    persist_layout(services, layout);

    // The saved query is read once the config database is open.
    let opening = RenderEffect::new(move |_| {
        if services.settings.with_state(|state| matches!(state, SettingsState::Loaded(_))) {
            search.open_saved_query();
        }
    });
    let requests = RenderEffect::new(move |_| {
        for request in shell.palette.take_requests(AppTab::Search) {
            if let TabRequest::Search(query) = request {
                search.set_query_text(query);
            }
        }
    });
    on_cleanup_local(move || {
        drop(opening);
        drop(requests);
    });

    let gap_hint = move || {
        let missing = search.game_mode_gap.get()?;
        search.ran.with(|ran| {
            ran.as_ref().filter(|expr| game_mode_gap_applies(missing, expr)).map(|_| game_mode_gap_hint(missing))
        })
    };
    let count = move || match search.status.get() {
        Status::Done if search.results.with(|results| results.truncated) => {
            t!("ui.search.match_count_truncated", count = RESULT_LIMIT).into_owned()
        }
        Status::Done => {
            t!("ui.search.match_count", count = search.results.with(|results| results.hits.len())).into_owned()
        }
        _ => String::new(),
    };

    view! {
        box(class = "wt-search", attr:data-testid = "search-tab") {
            QueryBar(search = search)
            text(class = "wt-search-strip", attr:data-testid = "search-game-mode-gap") {{move || gap_hint().unwrap_or_default()}}
            {move || search.save_error.get().map(|failed| view! {
                row(class = "wt-search-strip wt-search-strip--error", attr:data-testid = "search-settings-save-error") {
                    text {{failed.message}}
                    Button(variant = ButtonVariant::Outline, size = ButtonSize::Xs, on:click = move |_| search.retry_save()) {
                        {t!("ui.buttons.retry").into_owned()}
                    }
                }
            })}
            box(class = "wt-search-body") {
                SearchBody(search = search, layout = layout)
            }
            text(class = "wt-search-footer", attr:data-testid = "search-count") {{count}}
        }
    }
}

/// What the status says in place of the results, or `None` when the results are shown.
fn status_text(status: &Status, has_hits: bool) -> Option<String> {
    match status {
        Status::Idle => Some(t!("ui.search.type_a_query").into_owned()),
        // Said under the bar, not in place of the results.
        Status::Invalid { .. } if has_hits => None,
        Status::Invalid { .. } => Some(t!("ui.search.type_a_query").into_owned()),
        Status::Running => Some(t!("ui.search.searching").into_owned()),
        Status::Failed(reason) => Some(t!("ui.search.failed", reason = reason).into_owned()),
        Status::Done if !has_hits => Some(t!("ui.search.no_matches").into_owned()),
        Status::Done => None,
    }
}

/// The results, or the status in their place.
#[component]
fn SearchBody(search: Search, layout: RwSignal<ColumnLayout, LocalStorage>) -> impl IntoView {
    let status = Memo::new(move |_| {
        let has_hits = search.results.with(|results| !results.hits.is_empty());
        search.status.with(|status| status_text(status, has_hits))
    });
    let showing_results = Memo::new(move |_| status.with(Option::is_none));
    let running = Memo::new(move |_| search.status.with(|status| matches!(status, Status::Running)));
    // A settings failure keeps the plain status, which carries its retry.
    let waiting = Memo::new(move |_| running.get() && search.load_error.with(Option::is_none));

    move || {
        // The status carries the retry in place of the results; beside them, a strip does.
        if showing_results.get() {
            return AnyView::new(view! {
                {move || search.load_error.get().map(|error| view! {
                    row(class = "wt-search-strip wt-search-strip--error", attr:data-testid = "search-settings-load-error") {
                        text {{error}}
                        Button(variant = ButtonVariant::Outline, size = ButtonSize::Xs, on:click = move |_| search.retry_load()) {
                            {t!("ui.buttons.retry").into_owned()}
                        }
                    }
                })}
                ResultsTable(search = search, layout = layout)
            });
        }
        if waiting.get() {
            let label = Signal::derive_local(move || status.get().unwrap_or_default());
            return AnyView::new(view! { Loading(label = label, attr:data-testid = "search-status") });
        }
        AnyView::new(view! {
            box(class = "wt-search-status", attr:data-testid = "search-status") {
                text {{move || status.get().unwrap_or_default()}}
                Show(when = move || search.load_error.with(Option::is_some)) {
                    Button(variant = ButtonVariant::Outline, size = ButtonSize::Xs, on:click = move |_| search.retry_load()) {
                        {t!("ui.buttons.retry").into_owned()}
                    }
                }
            }
        })
    }
}

fn column_id(column: ResultColumn) -> &'static str {
    match column {
        ResultColumn::Sortable(SortColumn::Date) => "date",
        ResultColumn::Sortable(SortColumn::Map) => "map",
        ResultColumn::Sortable(SortColumn::Mode) => "mode",
        ResultColumn::Sortable(SortColumn::Outcome) => "outcome",
        ResultColumn::Sortable(SortColumn::Damage) => "damage",
        ResultColumn::Sortable(SortColumn::Kills) => "kills",
        ResultColumn::Sortable(SortColumn::Pr) => "pr",
        ResultColumn::Ship => "ship",
    }
}

fn sort_column_of(id: &ColumnId) -> Option<SortColumn> {
    ResultColumn::all().into_iter().find_map(|column| match column {
        ResultColumn::Sortable(sortable) if column_id(column) == id.as_str() => Some(sortable),
        _ => None,
    })
}

fn grid_sort(sort: SortSpec) -> ColumnSort {
    ColumnSort {
        column: ColumnId::new(column_id(ResultColumn::Sortable(sort.column))),
        direction: match sort.direction {
            SortDirection::Ascending => landaire_ui::SortDirection::Ascending,
            SortDirection::Descending => landaire_ui::SortDirection::Descending,
        },
    }
}

fn outcome_tone(outcome: MatchOutcome) -> Option<&'static str> {
    match outcome {
        MatchOutcome::Win => Some("win"),
        MatchOutcome::Loss => Some("loss"),
        MatchOutcome::Draw => Some("draw"),
        MatchOutcome::Unknown => None,
    }
}

/// Reads the dragged widths and the pins back once, and writes them whenever they change after
/// that.
fn persist_layout(search_services: crate::services::Services, layout: RwSignal<ColumnLayout, LocalStorage>) {
    let loaded = StoredValue::new(false);
    let loading = RenderEffect::new(move |_| {
        if loaded.get_value() {
            return;
        }
        let Some(pool) = search_services.settings.pool() else {
            // Tracked so the read starts once the database opens.
            search_services.settings.with_state(|_| ());
            return;
        };
        loaded.set_value(true);
        spawn_local(async move {
            let stored = search_services
                .on_tokio(async move {
                    let widths =
                        wows_toolkit_config::queries::get_setting::<Vec<Option<f32>>>(&pool, COLUMN_WIDTHS_KEY).await;
                    let pinned =
                        wows_toolkit_config::queries::get_setting::<Vec<ColumnId>>(&pool, PINNED_COLUMNS_KEY).await;
                    (widths, pinned)
                })
                .await;
            let Ok((widths, pinned)) = stored else { return };
            layout.update(|layout| {
                for (column, width) in ResultColumn::all().into_iter().zip(widths.unwrap_or_default()) {
                    // A width narrower than the grip leaves a column that cannot be grabbed again.
                    let Some(width) = width else { continue };
                    layout
                        .set_width(&ColumnId::new(column_id(column)), Some(ColumnWidth(width.max(ColumnWidth::MIN.0))));
                }
                // The actions column is never pinned, and an id no longer shown is passed over.
                let pinned: Vec<ColumnId> = pinned
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|id| id.as_str() != ACTIONS_COLUMN && layout.ids().any(|known| known == id))
                    .collect();
                for id in &pinned {
                    layout.pin(id);
                }
            });
        });
    });
    let saving = RenderEffect::new(move |previous: Option<Vec<Option<f32>>>| {
        let widths: Vec<Option<f32>> = layout.with(|layout| {
            ResultColumn::all()
                .into_iter()
                .map(|column| layout.width(&ColumnId::new(column_id(column))).map(|width| width.0))
                .collect()
        });
        if previous.as_ref().is_some_and(|previous| *previous != widths) {
            search_services.settings.save(COLUMN_WIDTHS_KEY, &widths);
        }
        widths
    });
    let saving_pins = RenderEffect::new(move |previous: Option<Vec<ColumnId>>| {
        let pinned: Vec<ColumnId> = layout.with(|layout| layout.ids().take(layout.pinned_count()).cloned().collect());
        if previous.as_ref().is_some_and(|previous| *previous != pinned) {
            search_services.settings.save(PINNED_COLUMNS_KEY, &pinned);
        }
        pinned
    });
    on_cleanup_local(move || {
        drop(loading);
        drop(saving);
        drop(saving_pins);
    });
}

/// The results table: one row per match, sorted by the query, with each row's battle played back
/// beside it while the pointer rests on it.
#[component]
fn ResultsTable(search: Search, layout: RwSignal<ColumnLayout, LocalStorage>) -> impl IntoView {
    let shell = Shell::current();
    let preview = HoverPreview::new();

    let rows = Signal::derive_local(move || {
        let results = search.results.get();
        search.resolved_ships.with(|resolved| {
            results
                .hits
                .iter()
                .enumerate()
                .map(|(position, hit)| {
                    let live = hit
                        .version_build
                        .zip(hit.self_ship_id)
                        .and_then(|key| resolved.get(&key))
                        .map(String::as_str)
                        .map(str::to_owned);
                    ResultRow {
                        position,
                        generation: results.generation,
                        hit: Rc::new(hit.clone()),
                        exists: results.on_disk.get(position).copied().unwrap_or(false),
                        ship: ship_display_name(hit, live).unwrap_or_else(|| "-".to_string()),
                    }
                })
                .collect::<Vec<_>>()
        })
    });
    let row_at = move |position: usize| rows.with_untracked(|rows| rows.get(position).cloned());

    // The card goes up for the row the pointer came onto and stays where it is while the pointer
    // travels along that row.
    let hover = move |position: usize, anchor: NodeRef| {
        let Some(row) = row_at(position) else { return };
        if preview.watched_untracked().as_deref() == Some(row.hit.replay_path.as_path()) {
            return;
        }
        search.resolve_hovered_ship(&row.hit);
        preview.enter(PreviewTarget { path: row.hit.replay_path.clone(), map_name: None, anchor });
    };
    let open = move |position: usize| {
        let Some(row) = row_at(position) else { return };
        if row.exists {
            shell.request_replay(ReplayRequest::OpenReplay(row.hit.replay_path.clone()));
        }
    };

    let mut columns: Vec<GridColumn<ResultRow>> = ResultColumn::all()
        .into_iter()
        .map(|column| {
            GridColumn::new(column_id(column), column.label(), move |row: &ResultRow| {
                let (text, outcome, rating) = match column {
                    ResultColumn::Sortable(sortable) => (
                        cell_text(&row.hit, sortable),
                        (sortable == SortColumn::Outcome).then(|| outcome_tone(row.hit.outcome)).flatten(),
                        (sortable == SortColumn::Pr)
                            .then(|| row.hit.self_pr.map(|pr| rating_band_name(PersonalRatingCategory::from_pr(pr))))
                            .flatten(),
                    ),
                    ResultColumn::Ship => (row.ship.clone(), None, None),
                };
                view! {
                    text(
                        class = "wt-search-cell",
                        attr:data-outcome = outcome.map(str::to_owned),
                        style:color = rating.map(|band| format!("var(--wt-pr-{band}-text)"))
                    ) {{text}}
                }
            })
            .text(move |row: &ResultRow| match column {
                ResultColumn::Sortable(sortable) => cell_text(&row.hit, sortable),
                ResultColumn::Ship => row.ship.clone(),
            })
            .sizing(ColumnSizing::Fixed(ColumnWidth(column.default_width())))
            .reorderable(false)
            .sortable(matches!(column, ResultColumn::Sortable(_)))
        })
        .collect();
    columns.push(
        GridColumn::new(ACTIONS_COLUMN, "", move |row: &ResultRow| {
            let position = row.position;
            let path = row.hit.replay_path.clone();
            let clipped = path.clone();
            let exists = row.exists;
            view! {
                row(class = "wt-search-actions") {
                    Tooltip {
                        TooltipTrigger {
                            Button(
                                variant = ButtonVariant::Ghost,
                                size = ButtonSize::IconXs,
                                disabled = !exists,
                                a11y:label = t!("ui.replay.context.render_to_clipboard").into_owned(),
                                attr:data-testid = format!("search-row-render-clipboard-{position}"),
                                on:click = move |_| shell.request_replay(ReplayRequest::RenderToClipboard(clipped.clone()))
                            ) {
                                AppIcon(glyph = Glyph::Copy)
                            }
                        }
                        TooltipContent {{t!("ui.replay.context.render_to_clipboard").into_owned()}}
                    }
                    Tooltip {
                        TooltipTrigger {
                            DropdownMenu {
                                DropdownMenuTrigger(
                                    variant = ButtonVariant::Ghost,
                                    size = ButtonSize::IconXs,
                                    a11y:label = t!("ui.search.row_actions_hint").into_owned(),
                                    attr:data-testid = format!("search-row-actions-{position}")
                                ) {
                                    Icon(icon = ELLIPSIS)
                                }
                                DropdownMenuContent {
                                    {row_menu_items(shell, path.clone(), exists, move || open(position))}
                                }
                            }
                        }
                        TooltipContent {{t!("ui.search.row_actions_hint").into_owned()}}
                    }
                }
            }
        })
        .sizing(ColumnSizing::Fixed(ColumnWidth(52.0)))
        .resizable(false)
        .reorderable(false)
        .pinnable(false),
    );

    let menu: RowMenu<usize> = Rc::new(move |position: usize| {
        let row = row_at(position)?;
        let path = row.hit.replay_path.clone();
        Some(MenuItems::new(move || row_menu_items(shell, path.clone(), row.exists, move || open(position))))
    });
    let facts = move || {
        let path = preview.watched()?;
        let row =
            rows.with(|rows| rows.iter().position(|row| row.hit.replay_path == path).map(|at| rows[at].clone()))?;
        let facts: Vec<AnyView> = hover_facts_for_match(&row.hit, row.ship.clone(), None)
            .into_iter()
            .map(|fact| {
                AnyView::new(view! {
                    row(class = "wt-search-fact") {
                        text(class = "wt-search-fact__label") {{fact.label}}
                        text(class = "wt-search-fact__value") {{fact.value}}
                    }
                })
            })
            .collect();
        Some(AnyView::new(view! {
            box(class = "wt-search-facts") {
                {facts}
            }
        }))
    };
    let strings = GridStrings {
        pin: t!("ui.replay.context.pin_column").into_owned(),
        unpin: t!("ui.replay.context.unpin_column").into_owned(),
        ..GridStrings::new()
    };

    view! {
        box(class = "wt-search-body", on:pointer_leave = move |_| preview.leave(), attr:data-testid = "search-results") {
            Grid(
                rows = rows,
                columns = columns,
                row_key = |row: &ResultRow| row.position,
                mode = RowMode::fixed(ROW_HEIGHT),
                layout = layout,
                strings = strings,
                sort = Signal::derive_local(move || Some(grid_sort(search.sort.get()))),
                on_sort = UnsyncCallback::new(move |asked: Option<ColumnSort>| {
                    // The index sorts by its own rule: a press on the sorted column flips it, a
                    // press elsewhere starts that column at its own first direction.
                    let column = match asked {
                        Some(asked) => sort_column_of(&asked.column),
                        None => Some(search.sort.get_untracked().column),
                    };
                    if let Some(column) = column {
                        search.sort_by(column);
                    }
                }),
                selection_mode = GridSelectionMode::Single,
                on_activate = UnsyncCallback::new(open),
                on_row_hover = UnsyncCallback::new(move |hovered: RowHover<usize>| match hovered {
                    RowHover::Entered { key, anchor } => hover(key, anchor),
                    RowHover::Left { key } => {
                        let Some(row) = row_at(key) else { return };
                        // Moving onto the next row enters it in the same pointer move, so the
                        // leave waits for the move and ends only a preview still on this row.
                        spawn_local(async move {
                            if preview.watched_untracked().as_deref() == Some(row.hit.replay_path.as_path()) {
                                preview.leave();
                            }
                        });
                    }
                }),
                row_menu = menu,
                label = t!("ui.tabs.search").into_owned()
            )
            // The rows fill the tab's width, so the card goes under its row rather than over it.
            HoverPreviewCard(preview = preview, placement = Placement::new(Side::Bottom, Align::End)) {
                {facts}
            }
        }
    }
}

/// What a result row offers, shared by its dots and its right-click menu. A replay that is no
/// longer on disk keeps its path to copy; the rest is greyed out under a line that says why.
fn row_menu_items(shell: Shell, path: std::path::PathBuf, exists: bool, open: impl Fn() + 'static) -> impl IntoView {
    let copied = without_verbatim_prefix(&path).display().to_string();
    let file = path.clone();
    let (rendered, clipped) = (path.clone(), path.clone());
    view! {
        Show(when = move || !exists) {
            MenuLabel {{t!("ui.search.open_missing").into_owned()}}
            MenuSeparator()
        }
        MenuItem(disabled = !exists, on_select = UnsyncCallback::new(move |()| open())) {
            {t!("ui.search.open").into_owned()}
        }
        MenuItem(
            disabled = !exists,
            on_select = UnsyncCallback::new(move |()| shell.request_replay(ReplayRequest::RenderReplay(rendered.clone())))
        ) {
            {t!("ui.replay.context.render_replay").into_owned()}
        }
        MenuItem(
            disabled = !exists,
            on_select = UnsyncCallback::new(move |()| shell.request_replay(ReplayRequest::RenderToClipboard(clipped.clone())))
        ) {
            {t!("ui.replay.context.render_to_clipboard").into_owned()}
        }
        MenuSeparator()
        MenuItem(
            disabled = !exists,
            on_select = UnsyncCallback::new(move |()| copy_replay_files(shell.toasts(), std::slice::from_ref(&file)))
        ) {
            {t!("ui.replay.context.copy_replay").into_owned()}
        }
        MenuItem(on_select = UnsyncCallback::new(move |()| shell.copy_text(copied.clone()))) {
            {t!("ui.replay.context.copy_path").into_owned()}
        }
        MenuItem(disabled = !exists, on_select = UnsyncCallback::new(move |()| reveal_in_file_manager(&path))) {
            {t!("ui.replay.context.show_in_explorer").into_owned()}
        }
    }
}
