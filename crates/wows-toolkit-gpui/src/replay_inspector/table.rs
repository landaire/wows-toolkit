//! Custom virtualized player table (collapsed rows). Renders the M1
//! presentation model with a fixed sortable header and a `gpui::list`-backed
//! body. The Actions/Name/ShipName columns stay pinned to the left edge
//! (`STICKY_COLUMN_COUNT`); the remaining columns scroll horizontally inside
//! a nested container per row and per header, all sharing one scroll handle
//! so they stay aligned. This layer adds the per-column cell parity pass on
//! top of Task 4's scaffold: colors, NDA text, the multi-segment Name cell
//! (ship-class icon, division, clan tag, player name), Skills tier icons, and
//! hover tooltips for the stat cells that carry a breakdown. A row also
//! carries an expand/collapse state (`expanded`, keyed by `db_id` so it
//! survives re-sorting): the Name cell's caret and a double-click on the
//! collapsed row toggle it, growing the row to show `expanded::render_detail`
//! underneath (achievements, ribbons, build, consumables, damage
//! breakdowns; see `expanded.rs`).

use std::collections::HashSet;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::Icon;
use gpui_component::IconName;
use gpui_component::h_flex;
use gpui_component::scroll::Scrollbar;
use gpui_component::tooltip::Tooltip;
use gpui_component::v_flex;

use super::columns::BattleOutcome;
use super::columns::CaptainPointsTier;
use super::columns::CellValue;
use super::columns::ColorRole;
use super::columns::PlayerColorKind;
use super::columns::ReplayColumn;
use super::columns::cell_value;
use super::columns::name_color_kind;
use super::columns::player_color_kind;
use super::columns::player_color_kind_rgb;
use super::expanded;
use super::icons::IconCache;
use super::model::PlayerRow;
use super::model::ReplayReportModel;
use super::sort::SortColumn;
use super::sort::SortOrder;
use super::sort::sort_rows;
use wows_replay_insights::personal_rating::PersonalRatingCategory;
use wows_replays::types::AccountId;
use wowsunpack::vfs::VfsPath;

/// Overdraw for the virtualized list: how far past the viewport to render so
/// scrolling reveals already-laid-out rows instead of blank space.
const LIST_OVERDRAW: Pixels = px(200.);

/// Multiplier for a cell's `ElementId` (`ix * CELL_ID_STRIDE + col as usize`),
/// spacing row indices apart enough that no two `(row, column)` pairs collide.
/// Must exceed `ReplayColumn::ALL.len()`.
const CELL_ID_STRIDE: usize = 32;
const _: () = assert!(CELL_ID_STRIDE > ReplayColumn::ALL.len(), "CELL_ID_STRIDE must exceed the column count");

/// Columns pinned to the left edge of the table while the rest scroll
/// horizontally: Actions, Name, ShipName, in that order. Mirrors the egui
/// app's `num_sticky_cols(3)` (`mod.rs:2808`). `default_columns` always
/// includes these three first and unconditionally, so this always freezes
/// exactly them.
const STICKY_COLUMN_COUNT: usize = 3;

/// Fixed pixel width for a column's header and body cells. Both use the same
/// value so the shared horizontal scroll keeps them aligned.
fn column_width(col: ReplayColumn) -> f32 {
    match col {
        ReplayColumn::Actions => 60.0,
        ReplayColumn::Name => 220.0,
        ReplayColumn::ShipName => 150.0,
        ReplayColumn::Skills => 130.0,
        ReplayColumn::PersonalRating => 70.0,
        ReplayColumn::BaseXp => 80.0,
        ReplayColumn::RawXp => 80.0,
        ReplayColumn::Kills => 55.0,
        ReplayColumn::ObservedDamage => 130.0,
        ReplayColumn::ActualDamage => 120.0,
        ReplayColumn::ReceivedDamage => 130.0,
        ReplayColumn::SpottingDamage => 130.0,
        ReplayColumn::PotentialDamage => 130.0,
        ReplayColumn::Hits => 70.0,
        ReplayColumn::Heals => 70.0,
        ReplayColumn::DistanceTraveled => 120.0,
        ReplayColumn::TimeLived => 90.0,
    }
}

/// Header label for a column, matching the egui app's `ui.replay.column.*`
/// English strings.
fn column_label(col: ReplayColumn) -> &'static str {
    match col {
        ReplayColumn::Actions => "Actions",
        ReplayColumn::Name => "Player Name",
        ReplayColumn::ShipName => "Ship Name",
        ReplayColumn::Skills => "Skills",
        ReplayColumn::PersonalRating => "PR",
        ReplayColumn::BaseXp => "Base XP",
        ReplayColumn::RawXp => "Raw XP",
        ReplayColumn::Kills => "Kills",
        ReplayColumn::ObservedDamage => "Observed Damage",
        ReplayColumn::ActualDamage => "Actual Damage",
        ReplayColumn::ReceivedDamage => "Received Damage",
        ReplayColumn::SpottingDamage => "Spotting Damage",
        ReplayColumn::PotentialDamage => "Potential Damage",
        ReplayColumn::Hits => "Hits",
        ReplayColumn::Heals => "Heals",
        ReplayColumn::DistanceTraveled => "Distance Traveled",
        ReplayColumn::TimeLived => "Time Lived",
    }
}

/// The `SortColumn` a header click sorts by, or `None` for the columns the
/// egui app renders as plain (non-clickable) headers: Actions, Skills, and
/// Time Lived.
fn column_sort(col: ReplayColumn) -> Option<SortColumn> {
    match col {
        ReplayColumn::Actions | ReplayColumn::Skills | ReplayColumn::TimeLived => None,
        ReplayColumn::Name => Some(SortColumn::Name),
        ReplayColumn::ShipName => Some(SortColumn::ShipName),
        ReplayColumn::PersonalRating => Some(SortColumn::PersonalRating),
        ReplayColumn::BaseXp => Some(SortColumn::BaseXp),
        ReplayColumn::RawXp => Some(SortColumn::RawXp),
        ReplayColumn::Kills => Some(SortColumn::Kills),
        ReplayColumn::ObservedDamage => Some(SortColumn::ObservedDamage),
        ReplayColumn::ActualDamage => Some(SortColumn::ActualDamage),
        ReplayColumn::ReceivedDamage => Some(SortColumn::ReceivedDamage),
        ReplayColumn::SpottingDamage => Some(SortColumn::SpottingDamage),
        ReplayColumn::PotentialDamage => Some(SortColumn::PotentialDamage),
        ReplayColumn::Hits => Some(SortColumn::Hits),
        ReplayColumn::Heals => Some(SortColumn::Heals),
        ReplayColumn::DistanceTraveled => Some(SortColumn::DistanceTraveled),
    }
}

/// Sort-direction icon shown after the active sort column's label. The egui
/// app uses an icon-font glyph; gpui-component ships the same shape as a
/// bundled SVG (`sort-ascending`/`sort-descending`), so this port uses that
/// instead of an ASCII arrow.
fn sort_caret_icon(order: SortOrder) -> IconName {
    match order {
        SortOrder::Asc(_) => IconName::SortAscending,
        SortOrder::Desc(_) => IconName::SortDescending,
    }
}

/// Resolves a color role to a concrete `Hsla`, reproducing the egui app's
/// palette exactly (values verified against `util::formatting`,
/// `util::personal_rating`, and the win/loss header colors).
pub(crate) fn resolve_color(role: ColorRole) -> Hsla {
    let packed = match role {
        ColorRole::Player(kind) => player_color_kind_rgb(kind),
        ColorRole::PrTier(category) => match category {
            PersonalRatingCategory::Bad => 0xff0000,
            PersonalRatingCategory::BelowAverage => 0xfe7903,
            PersonalRatingCategory::Average => 0xffc71f,
            PersonalRatingCategory::Good => 0x44b300,
            PersonalRatingCategory::VeryGood => 0x318000,
            PersonalRatingCategory::Great => 0x02c9b3,
            PersonalRatingCategory::Unicum => 0xd042f3,
            PersonalRatingCategory::SuperUnicum => 0xa00dc5,
        },
        ColorRole::CaptainPoints(tier) => match tier {
            CaptainPointsTier::Bad => 0xff8080,
            CaptainPointsTier::Warning => 0xfcae1e,
            CaptainPointsTier::Caution => 0xffff00,
            CaptainPointsTier::Good => 0x90ee90,
        },
        ColorRole::WinLoss(outcome) => match outcome {
            BattleOutcome::Win => 0x90ee90,
            BattleOutcome::Loss => 0xff8080,
            BattleOutcome::Draw => 0xffffe0,
        },
        ColorRole::Fixed(rgb) => rgb,
    };
    rgb(packed).into()
}

/// The player table view: the presentation model, the virtualized list state,
/// the active sort order, the ship-class icon cache, and the horizontal
/// scroll handle shared between the header and the body.
pub struct PlayerTable {
    model: ReplayReportModel,
    list_state: ListState,
    sort: SortOrder,
    h_scroll: ScrollHandle,
    /// Decoded icons (ship-class for the Name cell; achievement/ribbon/
    /// consumable/modernization/signal/captain-skill for `expanded.rs`),
    /// resolved from the parsed replay's own build VFS by
    /// `IconCache::populate_from_rows`, decoded on the background executor and
    /// applied back once `new`'s spawned task completes (see `new`). Empty
    /// (every lookup a miss, so `name_cell`/`expanded.rs` fall back to a plain
    /// text label) until then; never blocks entity construction.
    icons: IconCache,
    /// Debug mode lifts NDA hiding and the enemy-only Skills gate, mirroring
    /// the egui app's `AppPreferences.debug_mode`. Seeded from `new`'s `debug`
    /// argument and kept live afterward by `set_debug` (see `panel.rs`'s
    /// runtime toggle).
    debug: bool,
    /// Rows currently showing their expanded detail, keyed by `db_id` rather
    /// than list index so a row's expanded state survives a re-sort (which
    /// changes indices but not identities). Mirrors the egui app's
    /// `is_row_expanded: BTreeMap<u64, bool>`, minus the closed/`false`
    /// entries it also keeps around (a `HashSet` has no use for them).
    expanded: HashSet<AccountId>,
}

impl PlayerTable {
    /// Builds the table for `model` and kicks off resolving every icon its
    /// rows reference from `vfs` (the exact VFS `model` was parsed against;
    /// see `load::ParsedReplay`) on the background executor -- the VFS reads
    /// and PNG/SVG decodes underneath `IconCache::populate_from_rows` are too
    /// slow (dozens of icons per replay) to run on the UI thread without
    /// stalling it. The table renders immediately with `icons` empty (every
    /// cell falls back to its text label, per `name_cell`/`expanded.rs`), then
    /// re-renders with real icons once the spawned task's decode completes and
    /// applies its result back via `cx.notify()`.
    pub fn new(mut model: ReplayReportModel, vfs: VfsPath, debug: bool, cx: &mut Context<Self>) -> Self {
        let sort = SortOrder::default();
        sort_rows(&mut model.rows, model.self_team, sort, debug);
        let list_state = ListState::new(model.rows.len(), ListAlignment::Top, LIST_OVERDRAW);

        let svg_renderer = cx.svg_renderer();
        let rows_for_icons = model.rows.clone();
        cx.spawn(async move |this, cx| {
            let icons = cx
                .background_spawn(async move {
                    let mut icons = IconCache::new();
                    icons.populate_from_rows(&rows_for_icons, &vfs, &svg_renderer);
                    icons
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                tracing::info!(
                    ship_class_icons = icons.ship_class_count(),
                    keyed_icons = icons.keyed_count(),
                    "replay inspector: resolved player-table icons"
                );
                this.icons = icons;
                cx.notify();
            });
        })
        .detach();

        Self {
            model,
            list_state,
            sort,
            h_scroll: ScrollHandle::new(),
            icons: IconCache::new(),
            debug,
            expanded: HashSet::new(),
        }
    }

    /// Applies a header click: toggles the sort order for `column`, re-sorts
    /// the rows in place, and resets the list to the new row count.
    fn sort_by(&mut self, column: SortColumn, cx: &mut Context<Self>) {
        self.sort.update_column(column);
        sort_rows(&mut self.model.rows, self.model.self_team, self.sort, self.debug);
        self.list_state.reset(self.model.rows.len());
        cx.notify();
    }

    /// Applies a runtime debug-mode toggle (see `panel.rs::ReplayPanel::set_debug`):
    /// re-sorts, since a column's NDA-hidden sort key changes between debug
    /// on/off (`sort_rows`'s `debug` gate), then notifies so every cell's
    /// `cell_value`/`expanded::render_detail` call picks up the new flag on
    /// its next render.
    pub fn set_debug(&mut self, debug: bool, cx: &mut Context<Self>) {
        if self.debug == debug {
            return;
        }
        self.debug = debug;
        sort_rows(&mut self.model.rows, self.model.self_team, self.sort, self.debug);
        self.list_state.reset(self.model.rows.len());
        cx.notify();
    }

    /// Applies a new visible-column set from the header toolbar's
    /// column-filter checkboxes (`view.rs::ReplayInspectorView::set_column_filter`,
    /// via `panel.rs::ReplayPanel::set_columns`). Row order/content is
    /// untouched -- only which columns render -- so this just swaps the field
    /// and notifies; `render`'s `sticky_columns`/`scroll_columns` split is
    /// recomputed from `model.columns` fresh on every render.
    pub fn set_columns(&mut self, columns: Vec<ReplayColumn>, cx: &mut Context<Self>) {
        self.model.columns = columns;
        cx.notify();
    }

    /// Toggles row `ix`'s expanded state and remeasures just that row.
    /// `remeasure_items` (unlike `sort_by`'s `reset`) preserves the list's
    /// current scroll position, so expanding a row on-screen doesn't jump the
    /// viewport. Mirrors the egui app's `is_row_expanded`/`row_heights`
    /// toggle in `cell_content_ui`.
    fn toggle_expanded(&mut self, ix: usize, cx: &mut Context<Self>) {
        let db_id = self.model.rows[ix].db_id;
        if !self.expanded.remove(&db_id) {
            self.expanded.insert(db_id);
        }
        self.list_state.remeasure_items(ix..ix + 1);
        cx.notify();
    }

    fn header_cell(&self, col: ReplayColumn, cx: &mut Context<Self>) -> AnyElement {
        let base = div()
            .w(px(column_width(col)))
            .flex_none()
            .px_1()
            .py_1()
            .font_weight(FontWeight::BOLD)
            .whitespace_nowrap()
            .overflow_hidden();

        match column_sort(col) {
            None => base.child(column_label(col)).into_any_element(),
            Some(sort_column) => {
                let sorted = self.sort.column() == sort_column;
                let sort = self.sort;
                base.id(("replay-header", col as usize))
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.sort_by(sort_column, cx);
                    }))
                    .child(column_label(col))
                    .when(sorted, |el| el.child(Icon::new(sort_caret_icon(sort))))
                    .into_any_element()
            }
        }
    }
}

/// Builds the `.tooltip()` callback for a cell's hover text: one line per
/// `\n`-separated entry, monospace, matching the egui app's
/// `RichText::monospace` breakdown tooltips (`breakdown_hover_string`) and
/// plain single-line tooltips (Heals, Skills, PersonalRating) alike.
fn hover_tooltip(text: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |window, cx| {
        let text = text.clone();
        let mono_font_family = cx.theme().mono_font_family.clone();
        Tooltip::element(move |_window, _cx| {
            let text = text.clone();
            v_flex()
                .gap_0()
                .text_xs()
                .font_family(mono_font_family.clone())
                .children(text.split('\n').map(|line| div().child(line.to_string())))
        })
        .build(window, cx)
    }
}

/// One body cell: fixed-width, ellipsis-clipped, colored when the model gives
/// the cell a color role, with a hover tooltip when it carries breakdown or
/// explanatory text. `ix`/`col` key the cell's `ElementId` so the tooltip
/// hookup is unique per row/column.
fn cell_element(ix: usize, col: ReplayColumn, cell: CellValue, width: f32) -> AnyElement {
    let base = div()
        .w(px(width))
        .flex_none()
        .px_1()
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .when_some(cell.color, |el, role| el.text_color(resolve_color(role)))
        .child(cell.text);

    match cell.hover {
        Some(text) => base
            .id(("replay-cell", ix * CELL_ID_STRIDE + col as usize))
            .tooltip(hover_tooltip(text.into()))
            .into_any_element(),
        None => base.into_any_element(),
    }
}

/// The expand/collapse caret shown at the start of the Name cell (the egui
/// app's `col_nr == 1` case in `cell_content_ui`, which prepends the same
/// caret to whichever column happens to sit at index 1 -- always Name, since
/// `default_columns` always puts Actions/Name/ShipName first in that order).
/// A click toggles `PlayerTable::expanded` for this row via the entity handle
/// (this closure runs inside the virtualized list's render callback, which
/// only has `&mut App`, not `Context<PlayerTable>`).
fn expand_caret(ix: usize, entity: Entity<PlayerTable>, is_expanded: bool) -> AnyElement {
    let icon = if is_expanded { IconName::ChevronDown } else { IconName::ChevronRight };
    div()
        .id(("replay-row-caret", ix))
        .flex_none()
        .cursor_pointer()
        .on_click(move |_event: &ClickEvent, _window, cx: &mut App| {
            entity.update(cx, |this, cx| this.toggle_expanded(ix, cx));
            // Without this, a double-click landing on the caret fires this
            // handler twice (once per click) plus the row's own
            // double-click handler once, netting an odd (3) toggle count
            // instead of the even (2) egui nets for the same gesture.
            cx.stop_propagation();
        })
        .child(Icon::new(icon))
        .into_any_element()
}

/// The Name column's cell: the expand caret + ship-class icon (or a plain
/// species-text label when no icon is cached yet) + division label + colored
/// clan tag + colored player name. Bypasses `cell_value`'s single
/// joined-string Name arm because each segment needs its own color (the clan
/// tag uses the clan-league color, not the player color; abuser pink applies
/// only to the name, not the icon/division/clan segments), mirroring the
/// egui app's `cell_content_ui` `ReplayColumn::Name` arm, which makes one
/// separate `ui.add`/`ui.label` call per segment.
fn name_cell(ix: usize, row: &PlayerRow, layout: &RowLayout, width: f32) -> AnyElement {
    let name_color = resolve_color(ColorRole::Player(name_color_kind(row)));
    let icon_tint = player_color_kind_rgb(player_color_kind(row));

    let mut cell = h_flex().w(px(width)).flex_none().gap_1().px_1().items_center().overflow_hidden();
    cell = cell.child(expand_caret(ix, layout.entity.clone(), layout.is_expanded));

    cell = match layout.icons.get(row.ship_class, icon_tint) {
        Some(image) => {
            let icon_el = div().flex_none().child(img(image).w(px(16.)).h(px(16.)));
            if row.ship_species_text.is_empty() {
                cell.child(icon_el)
            } else {
                let species_text: SharedString = row.ship_species_text.clone().into();
                cell.child(
                    icon_el
                        .id(("replay-name-icon", ix))
                        .tooltip(move |window, cx| Tooltip::new(species_text.clone()).build(window, cx)),
                )
            }
        }
        None => cell.child(div().flex_none().text_xs().child(row.ship_species_text.clone())),
    };

    if let Some(div_label) = row.division_label.as_ref() {
        cell = cell.child(div().flex_none().child(div_label.clone()));
    }
    if let Some(clan) = row.clan_tag.as_ref() {
        cell = cell.child(
            div().flex_none().text_color(resolve_color(ColorRole::Fixed(row.clan_color_rgb))).child(clan.clone()),
        );
    }
    cell = cell.child(
        div()
            .flex_1()
            .min_w(px(0.))
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap()
            .text_color(name_color)
            .child(row.display_name.clone()),
    );

    cell.into_any_element()
}

/// The Skills column's cell: `cell_value`'s text/color/hover, prefixed with
/// the Dazzle/Incoming-Fire-Alert/tier-warning glyphs the egui app prepends
/// in `util::colorize_captain_points`. `cell_value` intentionally leaves
/// these glyphs out of its plain text (see `PlayerRow::skill_label_text`'s
/// doc comment in `model.rs`); this rebuilds them from
/// `has_dazzle`/`has_ifa`/`skill_warning`, matching the egui source, which
/// colors every glyph the same tier color as the label text. Only applies
/// when the underlying cell shows the real skill label, not the enemy/
/// no-vehicle-entity dash cases (which fall through to the generic
/// `cell_element`).
fn skills_cell(ix: usize, row: &PlayerRow, debug: bool, width: f32) -> AnyElement {
    let cell = cell_value(row, ReplayColumn::Skills, debug);
    let shows_real_label = row.has_vehicle_entity && (!row.relation.is_enemy() || debug);
    if !shows_real_label {
        return cell_element(ix, ReplayColumn::Skills, cell, width);
    }

    let color = cell
        .color
        .map(resolve_color)
        .unwrap_or_else(|| resolve_color(ColorRole::CaptainPoints(CaptainPointsTier::Good)));

    let mut markers = h_flex().flex_none().gap_0p5();
    let mut has_markers = false;
    if row.skill_warning {
        markers = markers.child(Icon::new(IconName::TriangleAlert).text_color(color));
        has_markers = true;
    }
    if row.has_dazzle {
        markers = markers.child(Icon::new(IconName::StarFill).text_color(color));
        has_markers = true;
    }
    if row.has_ifa {
        markers = markers.child(Icon::new(IconName::Bell).text_color(color));
        has_markers = true;
    }

    let base = div().w(px(width)).flex_none().px_1().child(
        h_flex().gap_1().items_center().overflow_hidden().when(has_markers, |el| el.child(markers)).child(
            div().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(color).child(cell.text.clone()),
        ),
    );

    match cell.hover {
        Some(text) => base
            .id(("replay-cell", ix * CELL_ID_STRIDE + ReplayColumn::Skills as usize))
            .tooltip(hover_tooltip(text.into()))
            .into_any_element(),
        None => base.into_any_element(),
    }
}

/// One column cell, dispatching to the Name/Skills special-cased layouts and
/// falling back to the generic `cell_element` for everything else.
fn render_cell(ix: usize, col: ReplayColumn, row: &PlayerRow, layout: &RowLayout) -> AnyElement {
    match col {
        ReplayColumn::Name => name_cell(ix, row, layout, column_width(col)),
        ReplayColumn::Skills => skills_cell(ix, row, layout.debug, column_width(col)),
        _ => cell_element(ix, col, cell_value(row, col, layout.debug), column_width(col)),
    }
}

/// Per-frame layout shared by every row and the header's scrolling portion:
/// the sticky/scrolling column split, the scrolling section's total width,
/// the shared horizontal scroll handle, the icon cache, the debug flag, this
/// row's expanded state and entity handle (for the caret/double-click
/// toggle), and the full row list (for the expanded damage-interaction
/// breakdowns, which look up other rows by `db_id`). Bundled into one struct
/// so `render_row` stays under clippy's argument-count limit.
struct RowLayout<'a> {
    sticky_columns: &'a [ReplayColumn],
    scroll_columns: &'a [ReplayColumn],
    scroll_width: f32,
    icons: &'a IconCache,
    debug: bool,
    h_scroll: &'a ScrollHandle,
    entity: Entity<PlayerTable>,
    is_expanded: bool,
    all_rows: &'a [PlayerRow],
}

/// One collapsed row: `layout.sticky_columns` (Actions/Name/ShipName) render
/// as fixed cells outside the horizontal scroll; `layout.scroll_columns`
/// render inside a nested scroll container tracking `layout.h_scroll`, the
/// same handle the header's scrolling portion tracks, so both stay aligned.
/// Mirrors the egui app's `num_sticky_cols(3)`. When `layout.is_expanded`,
/// stacks `expanded::render_detail`'s content underneath (see `expanded.rs`);
/// a double-click anywhere on the collapsed row also toggles expansion,
/// mirroring the egui app's whole-row double-click handler in
/// `cell_content_ui`.
fn render_row(ix: usize, row: &PlayerRow, layout: &RowLayout, hover_bg: Hsla, cx: &App) -> AnyElement {
    let mut sticky = h_flex().flex_none();
    for &col in layout.sticky_columns {
        sticky = sticky.child(render_cell(ix, col, row, layout));
    }

    let mut scrolling = h_flex().w(px(layout.scroll_width)).flex_none();
    for &col in layout.scroll_columns {
        scrolling = scrolling.child(render_cell(ix, col, row, layout));
    }

    let entity = layout.entity.clone();
    let collapsed = h_flex()
        .id(ix)
        .w_full()
        .py_0p5()
        .hover(move |style| style.bg(hover_bg))
        .on_click(move |event: &ClickEvent, _window, cx: &mut App| {
            if event.click_count() >= 2 {
                entity.update(cx, |this, cx| this.toggle_expanded(ix, cx));
            }
        })
        .child(sticky)
        .child(
            div()
                .id(("replay-row-h-scroll", ix))
                .flex_1()
                .min_w(px(0.))
                .overflow_x_scroll()
                .track_scroll(layout.h_scroll)
                .child(scrolling),
        );

    if !layout.is_expanded {
        return collapsed.into_any_element();
    }

    match expanded::render_detail(ix, row, layout.all_rows, layout.icons, layout.debug, cx) {
        Some(detail) => v_flex().w_full().child(collapsed).child(detail).into_any_element(),
        None => collapsed.into_any_element(),
    }
}

impl Render for PlayerTable {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let hover_bg = cx.theme().muted;

        let sticky_columns: Vec<ReplayColumn> = self.model.columns.iter().copied().take(STICKY_COLUMN_COUNT).collect();
        let scroll_columns: Vec<ReplayColumn> = self.model.columns.iter().copied().skip(STICKY_COLUMN_COUNT).collect();
        let scroll_width: f32 = scroll_columns.iter().map(|col| column_width(*col)).sum();

        let header = h_flex()
            .w_full()
            .flex_none()
            .border_b_1()
            .border_color(border)
            .child(h_flex().flex_none().children(sticky_columns.iter().map(|col| self.header_cell(*col, cx))))
            .child(
                div()
                    .id("replay-header-h-scroll")
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_x_scroll()
                    .track_scroll(&self.h_scroll)
                    .child(
                        h_flex()
                            .w(px(scroll_width))
                            .flex_none()
                            .children(scroll_columns.iter().map(|col| self.header_cell(*col, cx))),
                    ),
            );

        let entity = cx.entity();
        let h_scroll = self.h_scroll.clone();
        let render_item = move |ix: usize, _window: &mut Window, cx: &mut App| -> AnyElement {
            let table = entity.read(cx);
            let row = &table.model.rows[ix];
            let is_expanded = table.expanded.contains(&row.db_id);
            let layout = RowLayout {
                sticky_columns: &sticky_columns,
                scroll_columns: &scroll_columns,
                scroll_width,
                icons: &table.icons,
                debug: table.debug,
                h_scroll: &h_scroll,
                entity: entity.clone(),
                is_expanded,
                all_rows: &table.model.rows,
            };
            render_row(ix, row, &layout, hover_bg, cx)
        };

        div()
            .size_full()
            .relative()
            .child(v_flex().size_full().child(header).child(list(self.list_state.clone(), render_item).flex_1()))
            .child(Scrollbar::vertical(&self.list_state))
    }
}
