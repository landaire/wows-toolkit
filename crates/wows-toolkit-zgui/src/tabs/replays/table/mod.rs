//! The replay's player table: one row per player under a header that sorts, resizes, reorders
//! and pins the columns, with the reader's arrangement kept between sessions.

mod arrangement;
mod art;
mod cells;
mod damage_mix;
mod menu;
mod style;
#[cfg(test)]
pub(crate) mod tests;

use std::borrow::Borrow;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::ColumnId;
use landaire_ui::ColumnSort;
use landaire_ui::GridColumn;
use landaire_ui::GridProps;
use landaire_ui::GridSelectionMode;
use landaire_ui::GridStrings;
use landaire_ui::RowGroups;
use landaire_ui::RowMark;
use landaire_ui::RowMenu;
use landaire_ui::RowMenuHandle;
use landaire_ui::RowMode;
use landaire_ui::Selection;
use landaire_ui::SortDirection;
use landaire_ui::Toasts;
use rust_i18n::t;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::columns::StoredArrangement;
use wows_toolkit_replay_data::columns::cell_value;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::ReplayReportModel;
use wows_toolkit_replay_data::sort::sort_rows;
use wows_toolkit_replay_data::team_totals::TeamDamage;
use wows_toolkit_replay_data::team_totals::TeamShare;
use wows_toolkit_replay_data::team_totals::TeamSide;
use wows_toolkit_replay_data::team_totals::TeamTotals;
use wows_toolkit_replay_data::team_totals::team_runs;
use wows_toolkit_viewmodel::rating_breakdown::MatchContributionWithheld;
use wows_toolkit_viewmodel::replay_sort::SortOrder;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::keys;
use zgui::component;
use zgui::prelude::AnyView;
use zgui::prelude::ClipboardKind;
use zgui::prelude::Clipboards;
use zgui::prelude::try_use_clipboard;
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
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::reactive::use_local_context;
use zgui::task::Task;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::PointerButton;

use crate::game_art::GameArt;
use crate::services::GameDataState;
use crate::services::Services;
use crate::shell::Appearance;
use crate::tabs::replays::table::arrangement::COLUMN_ARRANGEMENT_KEY;
use crate::tabs::replays::table::arrangement::COLUMN_WIDTHS_KEY;
use crate::tabs::replays::table::arrangement::Columns;
use crate::tabs::replays::table::arrangement::column_id;
use crate::theme::palette_choice;

/// Builds one column's part of an expanded row; `None` for a column with nothing to show.
pub type PlayerDetail = Rc<dyn Fn(PlayerDetailCx) -> Option<AnyView>>;

/// A row's place in the order the replay lists its players. Rows are told apart by it rather than
/// by account, since every bot has account id 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowId(usize);

impl RowId {
    #[cfg(test)]
    pub fn from_position(position: usize) -> Self {
        Self(position)
    }
}

/// What one column's part of an expanded row is built from.
#[derive(Clone)]
pub struct PlayerDetailCx {
    pub id: RowId,
    pub row: Arc<PlayerRow>,
    /// Every player in the replay, for a detail that refers to other rows.
    pub rows: Arc<[PlayerRow]>,
    pub column: ReplayColumn,
    /// The game art the table has read, which the detail's icons draw from.
    pub art: GameArt,
}

/// One row as the grid holds it: the player, and what its cells draw from the rows around it.
#[derive(Clone)]
struct TableRow {
    id: RowId,
    row: Arc<PlayerRow>,
    /// Indexed by `ReplayColumn as usize`.
    shares: Arc<[Option<TeamShare>]>,
    side: TeamSide,
    /// The heading over the row's team, while the order keeps each team's rows together.
    team: Option<TeamHeading>,
    debug: bool,
    /// Why the battle has no match contribution for anyone.
    match_contribution_withheld: Option<MatchContributionWithheld>,
}

impl PartialEq for TableRow {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && Arc::ptr_eq(&self.row, &other.row)
            && self.shares == other.shares
            && self.side == other.side
            && self.team == other.team
            && self.debug == other.debug
            && self.match_contribution_withheld == other.match_contribution_withheld
    }
}

impl Borrow<PlayerRow> for TableRow {
    fn borrow(&self) -> &PlayerRow {
        &self.row
    }
}

impl TableRow {
    fn share(&self, col: ReplayColumn) -> Option<TeamShare> {
        self.shares[col as usize]
    }
}

/// What the heading over a team's rows shows: the team, and both teams' Actual Damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TeamHeading {
    side: TeamSide,
    damage: TeamDamage,
}

/// A model's rows, each shared once so a re-sort keeps every row the same value.
#[derive(Clone)]
struct ModelRows {
    all: Arc<[PlayerRow]>,
    /// Each of `all`, shared, at its `RowId`.
    shared: Arc<[Arc<PlayerRow>]>,
    match_contribution_withheld: Option<MatchContributionWithheld>,
}

impl PartialEq for ModelRows {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.all, &other.all) && self.match_contribution_withheld == other.match_contribution_withheld
    }
}

impl ModelRows {
    /// `model`'s rows. A row equal to the one `previous` shares at its place keeps that shared
    /// value, so a change to other rows leaves its cells and open tooltips as they are.
    fn of(model: &ReplayReportModel, previous: Option<&Self>) -> Self {
        let all: Arc<[PlayerRow]> = model.rows.clone().into();
        let shared = all
            .iter()
            .enumerate()
            .map(|(position, row)| {
                let kept = previous.and_then(|previous| previous.shared.get(position)).filter(|kept| ***kept == *row);
                kept.map_or_else(|| Arc::new(row.clone()), Arc::clone)
            })
            .collect();

        Self { all, shared, match_contribution_withheld: model.match_contribution_withheld }
    }
}

/// The cell the latest right press landed on, which the row menu that follows may copy. A press
/// that lands on the row but on no cell leaves nothing noted, rather than the cell pressed before.
///
/// The grid builds the menu while the press is still on its way out to the table, so the item
/// follows `cell` and a press that reached no cell clears it before the menu is drawn.
#[derive(Clone, Copy)]
struct NotedCell {
    cell: RwSignal<Option<(RowId, ReplayColumn)>, LocalStorage>,
    /// Whether a cell noted the press under way.
    fresh: StoredValue<bool, LocalStorage>,
}

impl NotedCell {
    fn new() -> Self {
        Self { cell: RwSignal::new_local(None), fresh: StoredValue::new_local(false) }
    }

    fn note(self, row: RowId, col: ReplayColumn) {
        self.fresh.set_value(true);
        self.cell.set(Some((row, col)));
    }

    /// Ends a right press that has reached the table.
    fn press(self) {
        if !self.fresh.get_value() {
            self.cell.set(None);
        }
        self.fresh.set_value(false);
    }

    /// Forgets the noted cell, for a menu opened from somewhere other than a cell.
    fn clear(self) {
        self.fresh.set_value(false);
        self.cell.set(None);
    }

    /// The cell the latest right press landed on, if it landed on one.
    fn current(self) -> Option<(RowId, ReplayColumn)> {
        self.cell.get()
    }
}

/// What the cells and the row menus act through.
#[derive(Clone, Copy)]
struct TableCx {
    expanded: RwSignal<HashSet<RowId>, LocalStorage>,
    noted: NotedCell,
    rows: Memo<Vec<TableRow>>,
    shown: Memo<Vec<ReplayColumn>>,
    columns: RwSignal<Columns, LocalStorage>,
    debug: Signal<bool, LocalStorage>,
    clipboard: StoredValue<Option<Clipboards>, LocalStorage>,
    toasts: Option<Toasts>,
    on_view_raw_json: Option<UnsyncCallback<String>>,
    menus: RowMenuHandle<RowId>,
    art: GameArt,
    /// The palettes on screen, which a clan tag's colour is fitted to.
    theme: Signal<landaire_ui::ThemeChoice, LocalStorage>,
    /// Whether a row opens a detail region.
    expandable: bool,
}

impl TableCx {
    fn toggle_expanded(self, row: RowId) {
        self.expanded.update(|expanded| {
            if !expanded.remove(&row) {
                expanded.insert(row);
            }
        });
    }

    /// The shown columns in the order drawn.
    fn drawn_columns(self) -> Vec<ReplayColumn> {
        let shown = self.shown.get_untracked();
        self.columns.with_untracked(|columns| columns.arrangement.shown(&shown).iter().collect())
    }

    /// Puts `text` on the clipboard and says so with `said`.
    fn copy(self, text: String, said: String) {
        self.clipboard.with_value(|clipboard| {
            if let Some(clipboard) = clipboard {
                clipboard.set_text(ClipboardKind::Standard, text);
            }
        });
        if let Some(toasts) = self.toasts {
            toasts.ok(said);
        }
    }
}

/// The player table of one replay.
///
/// Sorted by header: a press on a header sorts by it, descending first, and a second
/// press turns the order. A double click or Enter opens the row's detail, built by `detail`.
#[component]
pub fn PlayerTable(
    #[prop(into)] model: Signal<Rc<ReplayReportModel>, LocalStorage>,
    /// Lifts NDA hiding and the enemy-only gates, as the app's debug mode does.
    #[prop(into)]
    debug: Signal<bool, LocalStorage>,
    /// The region an expanded row opens.
    #[prop(optional)]
    detail: Option<PlayerDetail>,
    /// Told a row's raw metadata JSON when the reader asks to view it. Offered only in debug mode.
    #[prop(optional)]
    on_view_raw_json: Option<UnsyncCallback<String>>,
) -> impl IntoView {
    install_stylesheet(style::SHEET, style::CSS);
    let services = Services::current();
    let locale = services.locale;
    let expanded = RwSignal::new_local(HashSet::new());
    let selection = RwSignal::new_local(Selection::new());
    // A test reads and sets the open rows and reads the selection of the table it built.
    #[cfg(test)]
    {
        tests::EXPANDED.with(|slot| slot.set(Some(expanded)));
        tests::SELECTION.with(|slot| slot.set(Some(selection)));
    }
    // The sort every table shares, kept in the settings the egui app reads. A press before the
    // settings are read sorts this table alone.
    let unsaved_sort = RwSignal::new_local(SortOrder::default());
    let sort = Memo::new(move |_| {
        services.settings.read(|settings| settings.replay_sort).unwrap_or_else(|| unsaved_sort.get())
    });

    let model_rows = Memo::new(move |previous: Option<&ModelRows>| model.with(|model| ModelRows::of(model, previous)));
    let rows = Memo::new(move |_| {
        let order = sort.get();
        let debug = debug.get();
        let self_team = model.with(|model| model.self_team);
        model_rows.with(|rows| table_rows(rows, self_team, order, debug))
    });
    let shown = Memo::new(move |_| model.with(|model| model.columns.clone()));

    let columns = RwSignal::new_local(arrangement::known().unwrap_or_default());
    let changed = StoredValue::new_local(false);
    let reading: StoredValue<Option<Task>, LocalStorage> = StoredValue::new_local(None);
    let read_columns = RenderEffect::new(move |_| {
        if services.settings.read(|_| ()).is_none() || reading.with_value(Option::is_some) {
            return;
        }
        if let Some(known) = arrangement::known() {
            if !changed.get_value() {
                columns.set(known);
            }
            return;
        }
        let Some(pool) = services.settings.pool() else { return };

        let read = services.on_tokio(async move {
            let widths = wows_toolkit_config::queries::get_setting::<Vec<Option<f32>>>(&pool, COLUMN_WIDTHS_KEY).await;
            let arrangement =
                wows_toolkit_config::queries::get_setting::<StoredArrangement>(&pool, COLUMN_ARRANGEMENT_KEY).await;
            (widths, arrangement)
        });
        let task = spawn_local(async move {
            let Ok((widths, stored)) = read.await else { return };

            // Another table may have read or changed them meanwhile, which is newer than this read.
            let read = arrangement::known().unwrap_or_else(|| {
                let read = Columns::from_stored(stored.as_ref(), widths.as_deref());
                arrangement::remember(&read);
                read
            });
            // A change this table made meanwhile is newer and stays.
            if !changed.get_value() {
                columns.set(read);
            }
        });
        reading.set_value(Some(task));
    });
    on_cleanup_local(move || drop(read_columns));

    let grid_sort = Signal::derive_local(move || {
        let order = sort.get();
        let col = ReplayColumn::ALL.into_iter().find(|col| col.sort_column() == Some(order.column()))?;
        let direction = match order {
            SortOrder::Asc(_) => SortDirection::Ascending,
            SortOrder::Desc(_) => SortDirection::Descending,
        };

        Some(ColumnSort { column: column_id(col), direction })
    });
    // The grid proposes its own next sort; a press on the sorted column turns its order.
    let on_sort = UnsyncCallback::new(move |asked: Option<ColumnSort>| {
        let pressed = asked.map(|asked| asked.column).or_else(|| grid_sort.get_untracked().map(|sort| sort.column));
        let Some(col) = pressed.and_then(|id| ReplayColumn::from_storage_key(id.as_str())) else { return };
        let Some(sort_column) = col.sort_column() else { return };

        let mut next = sort.get_untracked();
        next.update_column(sort_column);
        if services.settings.read_untracked(|_| ()).is_some() {
            services.settings.edit(keys::REPLAY_SORT, |settings| {
                settings.replay_sort = next;
                next
            });
        } else {
            unsaved_sort.set(next);
        }
    });

    let art = GameArt::new();
    let wanting = RenderEffect::new(move |_| {
        let data = match services.game_data.state() {
            GameDataState::Ready(data) => Some(data),
            _ => None,
        };
        model.with(|model| art.want(data.as_deref(), model.rows.iter().flat_map(art::row_keys)));
    });
    on_cleanup_local(move || drop(wanting));

    // Outside the main window's shell the palettes follow the desktop, as `ThemeRoot`'s do.
    let shell = use_local_context::<Appearance>();
    let theme = Signal::derive_local(move || {
        palette_choice(shell.map_or(ThemeChoice::System, |appearance| appearance.shown_theme()))
    });
    let cx = TableCx {
        expanded,
        noted: NotedCell::new(),
        rows,
        shown,
        columns,
        debug,
        clipboard: StoredValue::new_local(try_use_clipboard()),
        toasts: Toasts::current(),
        on_view_raw_json,
        menus: RowMenuHandle::new(),
        art,
        theme,
        expandable: detail.is_some(),
    };

    let layout =
        RwSignal::new_local(shown.with_untracked(|shown| columns.with_untracked(|columns| columns.layout(shown))));
    let grid_columns: RwSignal<Vec<GridColumn<TableRow>>, LocalStorage> = RwSignal::new_local(Vec::new());
    // A filter or language change hands the grid its new columns and the layout over them at once,
    // so neither is reconciled against the other's old value. The grid stays, and with it the
    // scroll position, the selection and the keyboard's row.
    let offering = RenderEffect::new(move |_| {
        locale.track();
        shown.with(|shown| {
            let wanted = columns.with_untracked(|columns| columns.layout(shown));
            if layout.with_untracked(|layout| *layout != wanted) {
                layout.set(wanted);
            }
            grid_columns.set(shown.iter().map(|col| grid_column(*col, cx)).collect());
        });
    });
    let following = RenderEffect::new(move |_| {
        let wanted = columns.with(|columns| shown.with_untracked(|shown| columns.layout(shown)));
        if layout.with_untracked(|layout| *layout != wanted) {
            layout.set(wanted);
        }
    });
    let keeping = RenderEffect::new(move |_| {
        let next = layout.with(|layout| {
            shown.with_untracked(|shown| columns.with_untracked(|columns| columns.with_layout(layout, shown)))
        });
        let previous = columns.get_untracked();
        if next == previous {
            return;
        }

        changed.set_value(true);
        arrangement::remember(&next);
        if next.arrangement != previous.arrangement {
            services.settings.save(COLUMN_ARRANGEMENT_KEY, &next.arrangement.to_stored());
        }
        if next.widths != previous.widths {
            services.settings.save(COLUMN_WIDTHS_KEY, &next.stored_widths());
        }
        columns.set(next);
    });
    on_cleanup_local(move || {
        drop(offering);
        drop(following);
        drop(keeping);
    });

    let mode = match detail {
        Some(detail) => RowMode::with_column_detail(move |row: &TableRow, id: &ColumnId| {
            let column = ReplayColumn::from_storage_key(id.as_str())?;
            detail(PlayerDetailCx {
                id: row.id,
                row: Arc::clone(&row.row),
                rows: model_rows.with_untracked(|rows| Arc::clone(&rows.all)),
                column,
                art,
            })
        }),
        None => RowMode::natural(),
    };
    let mode = mode.grouped(RowGroups::new(
        |row: &TableRow| row.team.map(|team| team.side),
        |row: &TableRow| row.team.map(cells::team_heading),
    ));
    let strings = Signal::derive_local(move || {
        locale.track();
        GridStrings {
            pin: t!("ui.replay.context.pin_column").into_owned(),
            unpin: t!("ui.replay.context.unpin_column").into_owned(),
            ..GridStrings::new()
        }
    });
    let label = Signal::derive_local(move || {
        locale.track();
        t!("ui.tabs.replay_parser").into_owned()
    });
    let row_menu: RowMenu<RowId> = Rc::new(move |row: RowId| menu::row_menu(row, cx));
    let row_mark: RowMark<TableRow> = Rc::new(|row: &TableRow| Some(cells::row_mark(&row.row, row.side)));

    let noted = cx.noted;
    let on_press = move |ev: &mut EventCx<'_, events::PointerDown>| {
        if ev.button == Some(PointerButton::Secondary) {
            noted.press();
        }
    };

    view! {
        box(class = "wt-pt", on:pointer_down = on_press) {
            Grid(
                rows = Signal::derive_local(move || rows.get()),
                columns = grid_columns,
                row_key = |row: &TableRow| row.id,
                mode = mode,
                layout = layout,
                sort = grid_sort,
                on_sort = on_sort,
                selection_mode = GridSelectionMode::Multiple,
                selection = selection,
                expanded = expanded,
                row_menu = row_menu,
                row_mark = row_mark,
                strings = strings,
                row_menu_handle = cx.menus,
                label = label,
                class = "wt-pt-grid",
                attr:data-testid = "player-table"
            )
        }
    }
}

/// The table's rows for `order`, each with its share of its team's totals.
fn table_rows(
    rows: &ModelRows,
    self_team: wows_replays::types::TeamId,
    order: SortOrder,
    debug: bool,
) -> Vec<TableRow> {
    let totals = TeamTotals::of(&rows.all, self_team, debug);
    let damage = totals.column(ReplayColumn::ActualDamage);

    let mut table: Vec<TableRow> = rows
        .shared
        .iter()
        .enumerate()
        .map(|(position, row)| TableRow {
            id: RowId(position),
            shares: ReplayColumn::ALL.iter().map(|col| totals.share(row, *col, debug)).collect(),
            side: TeamSide::of(row, self_team),
            team: None,
            debug,
            match_contribution_withheld: rows.match_contribution_withheld,
            row: Arc::clone(row),
        })
        .collect();
    sort_rows(&mut table, self_team, order, debug);

    // Headings only while each team's rows stay together under the sort.
    if team_runs(&table, self_team).is_some() {
        for row in &mut table {
            row.team = Some(TeamHeading { side: row.side, damage });
        }
    }

    table
}

/// The grid's column for `col`.
fn grid_column(col: ReplayColumn, cx: TableCx) -> GridColumn<TableRow> {
    GridColumn::new(column_id(col), col.label(), move |row: &TableRow| cells::cell(col, row, cx))
        .text(move |row: &TableRow| cell_value(&row.row, col, row.debug).text)
        .sortable(col.sort_column().is_some())
}
