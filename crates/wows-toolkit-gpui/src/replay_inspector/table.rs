//! Custom virtualized player table (collapsed rows). Renders the M1
//! presentation model with a fixed sortable header and a `gpui_kit::list`-backed
//! body. The Actions/Name/ShipName columns stay pinned to the left edge
//! (`STICKY_COLUMN_COUNT`); the remaining columns scroll horizontally inside
//! a nested container per row and per header, all sharing one scroll handle
//! so they stay aligned. This layer adds the per-column cell parity pass on
//! top of Task 4's scaffold: colors, NDA text, the multi-segment Name cell
//! (ship-class icon, division, clan tag, player name), Skills tier icons, and
//! hover tooltips for the stat cells that carry a breakdown. A row also
//! carries an expand/collapse state (`expanded`, keyed by `db_id` so it
//! survives re-sorting): the Name cell's caret and a double-click on the row
//! toggle it, growing each column's own cell downward to show that column's
//! `expanded::render_column_detail` under its own header (achievements/
//! ribbons/damage-events under Name, the build under Skills, the damage
//! breakdowns under ActualDamage/ReceivedDamage; see `expanded.rs`).

use std::collections::HashSet;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

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
use super::model::MatchContext;
use super::model::PlayerRow;
use super::model::ReplayReportModel;
use super::model::SkillWarning;
use super::sort::SortColumn;
use super::sort::SortOrder;
use super::sort::sort_rows;
use wows_replay_insights::battle_report::ConnectionNote;
use wows_replay_insights::personal_rating::PersonalRatingResult;
use wows_replays::types::AccountId;
use wows_replays::types::Relation;
use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::twitch::SniperCandidate;
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

/// Column width bounds, matching the egui app's
/// `Column::new(100.0).range(10.0..=500.0)` (`mod.rs:2795`).
const COLUMN_MIN_WIDTH: f32 = 10.0;
const COLUMN_MAX_WIDTH: f32 = 500.0;

/// Fixed pixel width of the Name column's ship-class `img` icon
/// (`name_cell`'s `.w(px(16.))`), the one piece of cell furniture that does
/// not scale with the theme font size.
const SHIP_ICON_WIDTH: f32 = 16.0;

/// Rendered pixel width of `text` at the window's current font and
/// `window.rem_size()` (the theme's body font size; see `theme.rs`), with
/// `bold` reproducing `header_cell`'s `.font_weight(FontWeight::BOLD)`.
/// Backs `measure_column_widths`'s one-shot content pass -- never called on a
/// per-frame path. `pub(super)` so `expanded.rs`'s `expanded_detail_width`
/// (the same one-shot pass, for expanded-row content) can reuse it instead of
/// duplicating text shaping.
pub(super) fn text_width(text: SharedString, window: &mut Window, bold: bool) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let mut font = window.text_style().font();
    if bold {
        font.weight = FontWeight::BOLD;
    }
    let run = TextRun {
        len: text.len(),
        font,
        color: Hsla { h: 0., s: 0., l: 0., a: 1. },
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(text, window.rem_size(), &[run], None).width().as_f32()
}

/// Computes every column's content-fit width: the widest of the column's
/// header label and every row's rendered cell text, plus that column's
/// furniture (icons, marker glyphs, the `px_1()` cell padding both
/// `header_cell` and `cell_element` apply), clamped to
/// `COLUMN_MIN_WIDTH..=COLUMN_MAX_WIDTH`. Indexed by `ReplayColumn as usize`
/// (unconditionally sized to `ReplayColumn::ALL.len()`; a column absent from
/// `model.columns` keeps the placeholder `COLUMN_MIN_WIDTH` entry, since it
/// isn't rendered until `set_columns` brings it back and re-triggers this
/// pass).
///
/// The Name column reserves `max(ship-class icon, its species-text fallback)`
/// for the icon slot regardless of whether `IconCache` has resolved the icon
/// yet (`name_cell` shows the fallback text until the background decode in
/// `PlayerTable::new` completes) -- so this one pass stays correct across
/// that transition without needing a second recompute when the icons land.
///
/// A row in `expanded` also folds `expanded::expanded_detail_width` into that
/// row's per-column width alongside its collapsed content -- `table.rs`'s
/// `render_column_cell` clamps an expanded row's detail (the Skills column's
/// captain-skill grid and consumables table, in particular) to the column's
/// measured width, so without this an expanded row's detail would clip at the
/// collapsed-only width instead of growing the column to fit it.
///
/// Run once per data/column/debug/expanded-set change (`PlayerTable::new`,
/// `set_columns`, `set_debug`, `toggle_expanded`), gated behind
/// `PlayerTable::widths_dirty` in `render` -- never per frame.
fn measure_column_widths(
    model: &ReplayReportModel,
    debug: bool,
    expanded: &HashSet<AccountId>,
    window: &mut Window,
) -> Vec<Pixels> {
    let rem = window.rem_size().as_f32();
    let gap_1 = rem * 0.25;
    let gap_0p5 = rem * 0.125;
    let cell_padding = rem * 0.5;
    let icon_default = rem;
    // The cells clip with `text_ellipsis()`, which truncates (dropping several
    // trailing characters to make room for the "...") the moment the content
    // reaches the available width. A column measured to exactly its content
    // width therefore still shows an ellipsis, so give the text a little
    // breathing room beyond the measured content plus padding.
    let content_slack = rem * 0.5;

    let mut widths = vec![px(COLUMN_MIN_WIDTH); ReplayColumn::ALL.len()];
    for &col in &model.columns {
        let header_w = text_width(column_label(col).into(), window, true);
        let content_w = match col {
            ReplayColumn::Actions => header_w,
            ReplayColumn::Name => {
                let mut max_w = header_w;
                for row in &model.rows {
                    let cell = cell_value(row, col, debug);
                    let text_w = text_width(cell.text.into(), window, false);
                    let species_w = text_width(row.ship_species_text.clone().into(), window, false);
                    let icon_w = species_w.max(SHIP_ICON_WIDTH);
                    let mut row_w = icon_default + gap_1 + icon_w + gap_1 + text_w;
                    if expanded.contains(&row.db_id) {
                        row_w = row_w.max(expanded::expanded_detail_width(col, row, debug, window));
                    }
                    max_w = max_w.max(row_w);
                }
                max_w
            }
            ReplayColumn::Skills => {
                let mut max_w = header_w;
                for row in &model.rows {
                    let cell = cell_value(row, col, debug);
                    let text_w = text_width(cell.text.into(), window, false);
                    let marker_count = row.skill_warning.is_some() as u32 + row.has_dazzle as u32 + row.has_ifa as u32;
                    let mut row_w = if marker_count > 0 {
                        let markers_w =
                            marker_count as f32 * icon_default + marker_count.saturating_sub(1) as f32 * gap_0p5;
                        markers_w + gap_1 + text_w
                    } else {
                        text_w
                    };
                    if expanded.contains(&row.db_id) {
                        row_w = row_w.max(expanded::expanded_detail_width(col, row, debug, window));
                    }
                    max_w = max_w.max(row_w);
                }
                max_w
            }
            _ => {
                let mut max_w = header_w;
                for row in &model.rows {
                    let cell = cell_value(row, col, debug);
                    let mut row_w = text_width(cell.text.into(), window, false);
                    if expanded.contains(&row.db_id) {
                        row_w = row_w.max(expanded::expanded_detail_width(col, row, debug, window));
                    }
                    max_w = max_w.max(row_w);
                }
                max_w
            }
        };
        widths[col as usize] = px((content_w + cell_padding + content_slack).clamp(COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH));
    }
    widths
}

/// Header label for a column, matching the egui app's `ui.replay.column.*`
/// English strings.
const fn column_label_key(col: ReplayColumn) -> &'static str {
    match col {
        ReplayColumn::Actions => "ui.replay.actions",
        ReplayColumn::Name => "ui.replay.column.player_name",
        ReplayColumn::ShipName => "ui.replay.column.ship_name",
        ReplayColumn::Skills => "ui.replay.column.skills",
        ReplayColumn::PersonalRating => "ui.replay.column.personal_rating",
        ReplayColumn::BaseXp => "stat.base_xp",
        ReplayColumn::RawXp => "stat.raw_xp",
        ReplayColumn::Kills => "ui.replay.column.kills",
        ReplayColumn::ObservedDamage => "ui.replay.column.observed_damage",
        ReplayColumn::ActualDamage => "ui.replay.column.actual_damage",
        ReplayColumn::ReceivedDamage => "ui.replay.column.received_damage",
        ReplayColumn::SpottingDamage => "stat.spotting_damage",
        ReplayColumn::PotentialDamage => "ui.replay.column.potential_damage",
        ReplayColumn::Hits => "ui.replay.column.hits",
        ReplayColumn::Heals => "ui.replay.column.heals",
        ReplayColumn::DistanceTraveled => "ui.replay.column.distance_traveled",
        ReplayColumn::TimeLived => "ui.replay.column.time_lived",
    }
}

/// The column's heading in the reader's own language.
fn column_label(col: ReplayColumn) -> String {
    t!(column_label_key(col)).into_owned()
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
        // The band's text color, not its canonical hue: the hue is the chip
        // background, and reading it as text is the low-contrast case the
        // solved table exists to fix. The band has one palette per theme, so
        // it follows the one on screen.
        ColorRole::PrTier(category) => personal_rating::chip_text(category, crate::theme::is_dark_mode()),
        ColorRole::PrTierTint(category) => personal_rating::chip_hue(category),
        ColorRole::CaptainPoints(tier) => {
            let semantic = crate::theme::semantic();
            match tier {
                CaptainPointsTier::Bad => semantic.loss,
                CaptainPointsTier::Warning => semantic.warn,
                CaptainPointsTier::Caution => semantic.notice,
                CaptainPointsTier::Good => semantic.win,
            }
        }
        ColorRole::WinLoss(outcome) => {
            let semantic = crate::theme::semantic();
            match outcome {
                BattleOutcome::Win => semantic.win,
                BattleOutcome::Loss => semantic.loss,
                BattleOutcome::Draw => semantic.draw,
            }
        }
        ColorRole::Fixed(rgb) => rgb,
    };
    rgb(packed).into()
}

/// Events `PlayerTable` emits for its owner (`panel.rs::ReplayPanel`) to act
/// on. Currently just the Actions menu's "View Raw Player Metadata" item,
/// which has no direct handle to the panel's side-panel state from inside
/// the row-menu closure (see `build_actions_menu`), so it emits instead of
/// mutating directly.
pub enum PlayerTableEvent {
    /// A row's raw metadata JSON, pretty-printed, to show in the debug raw-
    /// JSON viewer (`panel.rs`'s `SidePanel::RawPlayerMetadata`).
    ViewRawJson(SharedString),
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
    /// The row a ctrl+click picked out, if any. One at a time, as in the egui
    /// table.
    selected: Option<AccountId>,
    /// Whether alt is down, which turns the damage breakdown's percentages
    /// around: how much of the other player's total this was, rather than how
    /// much of this row's. The egui table reads the modifier per frame
    /// (`mod.rs`'s `alt_held`); here a change notifies and the next render
    /// picks it up.
    alt_held: bool,
    /// Content-fit width per column, indexed by `ReplayColumn as usize`.
    /// Recomputed by `measure_column_widths` whenever `widths_dirty` is set;
    /// `render` is the only reader/writer of that flag, since computing a
    /// width needs `&mut Window` (real text measurement), which only `render`
    /// has.
    column_widths: Vec<Pixels>,
    /// Set on construction and whenever the columns/data/debug flag change
    /// (`set_columns`, `set_debug`); cleared by `render` after it recomputes
    /// `column_widths`. Keeps the (17-column x up-to-24-row) measurement pass
    /// off the per-frame path.
    widths_dirty: bool,
    /// The width every column is drawn at: what was dragged, or what the
    /// content measured. Rebuilt each render, so rows and header agree.
    drawn_widths: Vec<Pixels>,
    /// Widths the reader dragged a column to, by `ReplayColumn as usize`.
    /// `None` leaves that column fitted to its content, which is what every
    /// column does until one is dragged.
    width_overrides: Vec<Option<Pixels>>,
    /// Whether the saved widths have been read back yet. One shot, on the
    /// first frame.
    widths_loaded: bool,
    /// The column being dragged and the width it had when the drag started,
    /// so the new width follows the pointer's total travel rather than
    /// accumulating per-frame deltas.
    resizing: Option<ColumnDrag>,
}

/// A column-width drag in progress.
#[derive(Clone, Copy)]
struct ColumnDrag {
    column: ReplayColumn,
    pointer_start: Pixels,
    width_start: Pixels,
}

/// The narrowest a dragged column may be made. Below this a column's own
/// content is unreadable and its header is gone.
const COLUMN_DRAG_MIN: Pixels = px(28.);

/// The settings row the dragged column widths are kept in.
///
/// The port's own row, not one the egui app reads: its table carries explicit
/// per-column ranges and keeps its widths in egui's own memory, so there is
/// nothing shared to write to.
const COLUMN_WIDTHS_KEY: &str = "replay_column_widths";

/// The width of the strip on a header's trailing edge that starts a drag.
const RESIZE_GRIP: Pixels = px(6.);

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
            selected: None,
            alt_held: false,
            column_widths: vec![px(COLUMN_MIN_WIDTH); ReplayColumn::ALL.len()],
            drawn_widths: vec![px(COLUMN_MIN_WIDTH); ReplayColumn::ALL.len()],
            widths_loaded: false,
            width_overrides: vec![None; ReplayColumn::ALL.len()],
            resizing: None,
            widths_dirty: true,
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
    /// `cell_value`/`expanded::render_column_detail` call picks up the new flag on
    /// its next render. Also re-triggers `measure_column_widths`: NDA hiding
    /// swaps cell text for the shorter "NDA" placeholder, so the fit widths
    /// computed while hidden are too narrow for the real numbers debug mode
    /// reveals.
    pub fn set_debug(&mut self, debug: bool, cx: &mut Context<Self>) {
        if self.debug == debug {
            return;
        }
        self.debug = debug;
        sort_rows(&mut self.model.rows, self.model.self_team, self.sort, self.debug);
        self.list_state.reset(self.model.rows.len());
        self.widths_dirty = true;
        cx.notify();
    }

    /// This replay's own Personal Rating: the rating on the row the recording
    /// player occupies, which is what the outcome row's PR badge shows
    /// (`panel.rs::personal_rating_badge`). `None` until an expected-values
    /// table has been applied, or when the replay carries no self row.
    /// What match this was, for the line under the header.
    pub fn match_context(&self) -> &MatchContext {
        &self.model.context
    }

    /// Damage dealt by each side: the reader's own team first.
    ///
    /// Rows with no server damage figure count as zero, which is what the
    /// egui app's own tally does (`unwrap_or(0)`): a row that reported
    /// nothing did not thereby deal damage.
    pub fn team_damage(&self) -> (u64, u64) {
        let mut friendly = 0u64;
        let mut enemy = 0u64;
        for row in &self.model.rows {
            let damage = row.actual_damage.unwrap_or(0);
            if row.relation.is_enemy() {
                enemy += damage;
            } else {
                friendly += damage;
            }
        }
        (friendly, enemy)
    }

    pub fn self_personal_rating(&self) -> Option<PersonalRatingResult> {
        self.model.rows.iter().find(|row| row.is_self)?.personal_rating.clone()
    }

    /// Fills in the Personal Rating column from an expected-values table that
    /// arrived after the parse (see `panel.rs::ReplayPanel::set_personal_rating`).
    /// Marks `widths_dirty` because the column was measured while every cell
    /// held the "-" placeholder. Idempotent: `populate_personal_ratings`
    /// skips rows that already carry a rating.
    ///
    /// Row order only changes when PR is the sort key, so only that case
    /// resets the list; every other sort keeps the viewport where the reader
    /// left it, since nobody asked for a scroll. Notifies rather than emits:
    /// this runs inside the view's fan-out over its open panels, and an event
    /// from here would re-enter the panel that owns this table.
    /// Flags the rows whose names were plausibly in the monitored channel's
    /// chat around this battle. Row order does not depend on the flag, so
    /// this only remeasures.
    pub fn populate_twitch_candidates(&mut self, observations: &[(String, jiff::Timestamp)], cx: &mut Context<Self>) {
        self.model.populate_twitch_candidates(observations);
        self.list_state.remeasure_items(0..self.model.rows.len());
        cx.notify();
    }

    pub fn populate_personal_ratings(&mut self, table: &PersonalRatingData, cx: &mut Context<Self>) {
        self.model.populate_personal_ratings(table);
        if self.sort.column() == SortColumn::PersonalRating {
            sort_rows(&mut self.model.rows, self.model.self_team, self.sort, self.debug);
            self.list_state.reset(self.model.rows.len());
        } else {
            self.list_state.remeasure_items(0..self.model.rows.len());
        }
        self.widths_dirty = true;
        cx.notify();
    }

    /// Applies a new visible-column set from the header toolbar's
    /// column-filter checkboxes (`view.rs::ReplayInspectorView::set_column_filter`,
    /// via `panel.rs::ReplayPanel::set_columns`). Row order/content is
    /// untouched -- only which columns render -- so this just swaps the field
    /// and notifies; `render`'s `sticky_columns`/`scroll_columns` split is
    /// recomputed from `model.columns` fresh on every render. Marks
    /// `widths_dirty` so a newly-shown column gets a real fit width instead
    /// of its placeholder `COLUMN_MIN_WIDTH` entry.
    pub fn set_columns(&mut self, columns: Vec<ReplayColumn>, cx: &mut Context<Self>) {
        self.model.columns = columns;
        self.widths_dirty = true;
        cx.notify();
    }

    /// Toggles row `ix`'s expanded state and remeasures just that row.
    /// `remeasure_items` (unlike `sort_by`'s `reset`) preserves the list's
    /// current scroll position, so expanding a row on-screen doesn't jump the
    /// viewport. Mirrors the egui app's `is_row_expanded`/`row_heights`
    /// toggle in `cell_content_ui`. Also marks `widths_dirty`: an expanding
    /// row's Skills/Name/damage columns may need to grow to fit that row's
    /// detail (`measure_column_widths`'s `expanded` argument), and a
    /// collapsing row may let them shrink back.
    /// Ctrl+click picks a row out and clicking it again puts it back, which
    /// is how the egui table marks the one it is reading
    /// (`ui/replay_parser/mod.rs:2620`).
    fn toggle_selected(&mut self, ix: usize, cx: &mut Context<Self>) {
        let db_id = self.model.rows.get(ix).map(|row| row.db_id);
        self.selected = if self.selected == db_id { None } else { db_id };
        cx.notify();
    }

    fn toggle_expanded(&mut self, ix: usize, cx: &mut Context<Self>) {
        let db_id = self.model.rows[ix].db_id;
        if !self.expanded.remove(&db_id) {
            self.expanded.insert(db_id);
        }
        self.list_state.remeasure_items(ix..ix + 1);
        self.widths_dirty = true;
        cx.notify();
    }

    /// The width `col` is drawn at: what the reader dragged it to, or what
    /// its content measured.
    fn width_of(&self, col: ReplayColumn) -> Pixels {
        self.width_overrides[col as usize].unwrap_or(self.column_widths[col as usize])
    }

    /// Starts a width drag on `col` from the pointer's current position.
    fn start_column_drag(&mut self, col: ReplayColumn, at: Pixels, cx: &mut Context<Self>) {
        self.resizing = Some(ColumnDrag { column: col, pointer_start: at, width_start: self.width_of(col) });
        cx.notify();
    }

    /// Follows a width drag. A no-op when nothing is being dragged, which is
    /// every pointer move outside one.
    fn drag_column(&mut self, at: Pixels, cx: &mut Context<Self>) {
        let Some(drag) = self.resizing else { return };
        let width = (drag.width_start + (at - drag.pointer_start)).max(COLUMN_DRAG_MIN);
        self.width_overrides[drag.column as usize] = Some(width);
        cx.notify();
    }

    /// Ends a width drag, keeping where it got to.
    fn end_column_drag(&mut self, cx: &mut Context<Self>) {
        if self.resizing.take().is_some() {
            self.save_column_widths(cx);
            cx.notify();
        }
    }

    /// Writes the dragged widths back, so a table opened later opens at them.
    ///
    /// Read once per table, when it is first drawn. Two tables open at the
    /// same time therefore keep their own widths until one of them is
    /// reopened, which is the cost of not making every table listen to every
    /// other one.
    fn save_column_widths(&self, cx: &mut Context<Self>) {
        let widths: Vec<Option<f32>> = self.width_overrides.iter().map(|width| width.map(f32::from)).collect();
        crate::settings_store::save(COLUMN_WIDTHS_KEY, &widths, cx);
    }

    /// Reads the dragged widths back on the first frame, once the config
    /// database is open. `new` runs before it is.
    fn load_column_widths(&mut self, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        cx.spawn(async move |this, cx| {
            let stored = crate::runtime::spawn(cx, async move {
                wows_toolkit_config::queries::get_setting::<Vec<Option<f32>>>(&pool, COLUMN_WIDTHS_KEY).await
            })
            .await;
            let Ok(Some(widths)) = stored else { return };
            let _ = this.update(cx, |this, cx| {
                for (slot, width) in this.width_overrides.iter_mut().zip(widths) {
                    // A width narrower than the grip would leave a column
                    // that cannot be grabbed to widen again.
                    *slot = width.map(|width| px(width).max(COLUMN_DRAG_MIN));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Puts `col` back on its content-fitted width.
    fn reset_column_width(&mut self, col: ReplayColumn, cx: &mut Context<Self>) {
        self.resizing = None;
        if self.width_overrides[col as usize].take().is_some() {
            self.save_column_widths(cx);
            cx.notify();
        }
    }

    fn header_cell(&self, col: ReplayColumn, cx: &mut Context<Self>) -> AnyElement {
        let base = div()
            .w(self.width_of(col))
            .flex_none()
            .px_1()
            .py_1()
            .font_weight(FontWeight::BOLD)
            .whitespace_nowrap()
            .overflow_hidden();

        // The grip sits on the header's trailing edge, over the rule
        // between this column and the next, which is where a reader reaches
        // for it.
        let grip = div()
            .id(("replay-header-grip", col as usize))
            .test_support()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(RESIZE_GRIP)
            .cursor_col_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    // Double-clicking a grip puts that column back on its
                    // content, which is the usual way out of a drag that
                    // went too far.
                    if event.click_count >= 2 {
                        this.reset_column_width(col, cx);
                        return;
                    }
                    this.start_column_drag(col, event.position.x, cx);
                }),
            );

        let base = base.relative().child(grip);

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

impl EventEmitter<PlayerTableEvent> for PlayerTable {}

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
        .child(crate::ui::selectable_text(("replay-cell-text", ix * CELL_ID_STRIDE + col as usize), cell.text));

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
            div()
                .flex_none()
                .text_color(resolve_color(ColorRole::Fixed(row.clan_color_rgb)))
                .child(crate::ui::selectable_text(("replay-clan", ix), clan.clone())),
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
            .child(crate::ui::selectable_text(("replay-name", ix), row.display_name.clone())),
    );

    if row.is_hidden_profile {
        cell = cell.child(
            crate::icons::icon(crate::icons::EYE_SLASH).id(("replay-hidden-profile", ix)).test_support().tooltip(
                |window, cx| Tooltip::new(t!("ui.replay.player.hidden_profile").into_owned()).build(window, cx),
            ),
        );
    }

    if let Some(chip) = twitch_chip(ix, &row.twitch_candidates) {
        cell = cell.child(chip);
    }

    if let Some(note) = row.connection.as_ref() {
        let hover: SharedString = connection_hover_text(note).into();
        cell = cell.child(
            crate::icons::icon(crate::icons::PLUGS)
                .id(("replay-disconnect", ix))
                .test_support()
                .tooltip(move |window, cx| Tooltip::new(hover.clone()).build(window, cx)),
        );
    }

    cell.into_any_element()
}

/// What a connection history says, in the egui app's own wording.
fn connection_hover_text(note: &ConnectionNote) -> String {
    match note {
        ConnectionNote::NeverConnected => t!("ui.replay.player.never_connected").into_owned(),
        ConnectionNote::Interrupted { events } => format!("Player {}", events.join(", ")),
    }
}

/// The Twitch chip: a logo beside the name when a chat login around this
/// battle plausibly names this player.
///
/// One candidate copies its login on click; several open a menu so the
/// reader picks which. `None` when nothing matched, which is a row with no
/// chip.
fn twitch_chip(ix: usize, candidates: &[SniperCandidate]) -> Option<AnyElement> {
    let first = candidates.first()?;

    let hover: SharedString = candidates
        .iter()
        .map(|candidate| {
            let minutes = candidate.minutes.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
            format!(
                "{}
{}",
                t!("ui.twitch.possible_name", name = candidate.login),
                t!("ui.twitch.seen_minutes", minutes = minutes)
            )
        })
        .collect::<Vec<_>>()
        .join(
            "

",
        )
        .into();
    let hover: SharedString = format!(
        "{hover}

{}",
        t!("ui.twitch.click_to_copy")
    )
    .into();

    let glyph = crate::icons::icon(crate::icons::TWITCH_LOGO)
        .id(("replay-twitch", ix))
        .test_support()
        .cursor_pointer()
        .tooltip(move |window, cx| Tooltip::new(hover.clone()).build(window, cx));

    if candidates.len() == 1 {
        let login = first.login.clone();
        return Some(
            glyph
                .on_click(move |_event, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(login.clone()));
                    crate::toast::ok(t!("ui.twitch.copied", name = login).into_owned(), window, cx);
                })
                .into_any_element(),
        );
    }

    let logins: Vec<String> = candidates.iter().map(|candidate| candidate.login.clone()).collect();
    Some(
        Button::new(("replay-twitch-menu", ix))
            .ghost()
            .xsmall()
            .child(glyph)
            .dropdown_menu(move |mut menu, _window, _cx| {
                for login in &logins {
                    let login = login.clone();
                    menu = menu.item(PopupMenuItem::new(login.clone()).icon(IconName::Copy).on_click(
                        move |_event, window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(login.clone()));
                            crate::toast::ok(t!("ui.twitch.copied", name = login).into_owned(), window, cx);
                        },
                    ));
                }
                menu
            })
            .into_any_element(),
    )
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

    // The order the egui label builds: what the captain has, then what is
    // wrong with the build.
    let mut markers = h_flex().flex_none().gap_0p5();
    let mut has_markers = false;
    if row.has_dazzle {
        markers = markers.child(Icon::new(IconName::StarFill).text_color(color));
        has_markers = true;
    }
    if row.has_ifa {
        markers = markers.child(Icon::new(IconName::Bell).text_color(color));
        has_markers = true;
    }
    if let Some(warning) = row.skill_warning {
        // Two separate mistakes, so two separate glyphs: every point in tier
        // 1 is a turret, nothing above tier 2 is a plain warning.
        let glyph = match warning {
            SkillWarning::TowerDefense => crate::icons::CASTLE_TURRET,
            SkillWarning::NoHighTier => crate::icons::WARNING,
        };
        markers = markers.child(crate::icons::icon(glyph).text_color(color));
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

/// Builds the Actions column's `...` menu items, mirroring the egui app's
/// `ReplayColumn::Actions` arm (`ui/replay_parser/mod.rs` ~1681-1799)
/// item-for-item: the ship-config "Open Build in Browser"/"Copy Build Link"/
/// "Copy Short Build Link" trio (shown for non-enemy rows or in debug, and
/// only once the replay observed a vehicle entity, matching the egui gate
/// exactly), a separator, the WoWS-numbers link, and -- debug only -- "View
/// Raw Player Metadata". Each item is omitted (not shown disabled) when its
/// backing URL/JSON is `None`, per this port's "hidden, not a panic" rule for
/// missing config (`PlayerRow::ship_config_url`'s field doc). "Open"/"WoWS
/// numbers" use `PopupMenuItem::link`, which opens via `cx.open_url`
/// (gpui's own OS-opener, no `open`-crate dependency needed) and renders the
/// external-link glyph the egui app's SHARE icon stood in for; the copy
/// items write to the clipboard via `cx.write_to_clipboard`, matching
/// `ui.ctx().copy_text` and the browser_view.rs "Copy Path" precedent. "View
/// Raw Player Metadata" instead emits `PlayerTableEvent::ViewRawJson` on
/// `entity`, matching the egui app's behavior of opening a viewer (not
/// copying) -- `panel.rs` subscribes to that event and shows the payload in
/// the same `RawJsonPanel`/`SidePanel` side-panel slot the debug header's
/// "Raw Metadata"/"Raw Results" buttons use.
/// The handful of `PlayerRow` fields `build_actions_menu` actually reads,
/// cloned out in `actions_cell` instead of the whole row: `PlayerRow` also
/// carries achievements/ribbons/consumables/build/damage-interaction data
/// (and `raw_metadata_json`, a full pretty-printed JSON dump) that the menu
/// never touches, so cloning the whole struct there was a real per-visible-
/// row, every-render cost for data the dropdown discards.
struct ActionsMenuData {
    relation: Relation,
    has_vehicle_entity: bool,
    ship_config_url: Option<String>,
    short_ship_config_url: Option<String>,
    wows_numbers_url: Option<String>,
    raw_metadata_json: Option<String>,
}

impl ActionsMenuData {
    fn from_row(row: &PlayerRow) -> Self {
        Self {
            relation: row.relation,
            has_vehicle_entity: row.has_vehicle_entity,
            ship_config_url: row.ship_config_url.clone(),
            short_ship_config_url: row.short_ship_config_url.clone(),
            wows_numbers_url: row.wows_numbers_url.clone(),
            raw_metadata_json: row.raw_metadata_json.clone(),
        }
    }
}

/// Builds the Actions column's `...` menu items, mirroring the egui app's
/// `ReplayColumn::Actions` arm (`ui/replay_parser/mod.rs` ~1681-1799)
/// item-for-item: the ship-config "Open Build in Browser"/"Copy Build Link"/
/// "Copy Short Build Link" trio (shown for non-enemy rows or in debug, and
/// only once the replay observed a vehicle entity, matching the egui gate
/// exactly), a separator, the WoWS-numbers link, and -- debug only -- "View
/// Raw Player Metadata". Each item is omitted (not shown disabled) when its
/// backing URL/JSON is `None`, per this port's "hidden, not a panic" rule for
/// missing config (`PlayerRow::ship_config_url`'s field doc). "Open"/"WoWS
/// numbers" use `PopupMenuItem::link`, which opens via `cx.open_url`
/// (gpui's own OS-opener, no `open`-crate dependency needed) and renders the
/// external-link glyph the egui app's SHARE icon stood in for; the copy
/// items write to the clipboard via `cx.write_to_clipboard`, matching
/// `ui.ctx().copy_text` and the browser_view.rs "Copy Path" precedent. "View
/// Raw Player Metadata" instead emits `PlayerTableEvent::ViewRawJson` on
/// `entity`, matching the egui app's behavior of opening a viewer (not
/// copying) -- `panel.rs` subscribes to that event and shows the payload in
/// the same `RawJsonPanel`/`SidePanel` side-panel slot the debug header's
/// "Raw Metadata"/"Raw Results" buttons use.
fn build_actions_menu(
    mut menu: PopupMenu,
    row: &ActionsMenuData,
    debug: bool,
    entity: Entity<PlayerTable>,
) -> PopupMenu {
    let show_ship_config = (!row.relation.is_enemy() || debug) && row.has_vehicle_entity;

    if show_ship_config {
        let mut added_any = false;
        if let Some(url) = row.ship_config_url.clone() {
            menu = menu.item(PopupMenuItem::link(t!("ui.replay.build.open_in_browser").into_owned(), url.clone()));
            let copy_url = url;
            menu = menu.item(
                PopupMenuItem::new(t!("ui.replay.build.copy_link").into_owned()).icon(IconName::Copy).on_click(
                    move |_event, _window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                    },
                ),
            );
            added_any = true;
        }
        if let Some(url) = row.short_ship_config_url.clone() {
            menu = menu.item(
                PopupMenuItem::new(t!("ui.replay.build.copy_short_link").into_owned()).icon(IconName::Copy).on_click(
                    move |_event, _window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                    },
                ),
            );
            added_any = true;
        }
        if added_any {
            menu = menu.separator();
        }
    }

    if let Some(url) = row.wows_numbers_url.clone() {
        menu = menu.item(PopupMenuItem::link(t!("ui.replay.build.open_wows_numbers").into_owned(), url));
    }

    if debug && let Some(json) = row.raw_metadata_json.clone() {
        menu = menu.separator();
        menu = menu.item(
            PopupMenuItem::new(t!("ui.replay.debug.view_raw_metadata").into_owned()).icon(IconName::File).on_click(
                move |_event, _window, cx| {
                    let json = json.clone();
                    entity.update(cx, |_this, cx| cx.emit(PlayerTableEvent::ViewRawJson(json.into())));
                },
            ),
        );
    }

    menu
}

/// The Actions column's cell: a ghost icon-only `...` button that opens
/// `build_actions_menu`'s per-row popup menu on click. `ix` keys the
/// trigger's `ElementId` so every row's button/popover state is independent.
/// `entity` is threaded through to `build_actions_menu` so its "View Raw
/// Player Metadata" item can emit `PlayerTableEvent::ViewRawJson` on it. Only
/// the menu's own small `ActionsMenuData` is cloned out of `row` (see its doc
/// comment), not the whole `PlayerRow`.
fn actions_cell(ix: usize, row: &PlayerRow, debug: bool, entity: Entity<PlayerTable>, width: f32) -> AnyElement {
    let row = ActionsMenuData::from_row(row);
    let trigger = Button::new(("replay-row-actions", ix)).ghost().xsmall().icon(IconName::Ellipsis);
    let menu_button =
        trigger.dropdown_menu(move |menu, _window, _cx| build_actions_menu(menu, &row, debug, entity.clone()));

    div().w(px(width)).flex_none().px_1().child(menu_button).into_any_element()
}

/// One column's collapsed cell, dispatching to the Name/Skills/Actions
/// special-cased layouts and falling back to the generic `cell_element` for
/// everything else.
fn render_cell(ix: usize, col: ReplayColumn, row: &PlayerRow, layout: &RowLayout) -> AnyElement {
    let width = layout.column_widths[col as usize].as_f32();
    match col {
        ReplayColumn::Name => name_cell(ix, row, layout, width),
        ReplayColumn::Skills => skills_cell(ix, row, layout.debug, width),
        ReplayColumn::Actions => actions_cell(ix, row, layout.debug, layout.entity.clone(), width),
        _ => cell_element(ix, col, cell_value(row, col, layout.debug), width),
    }
}

/// One column's full cell: the collapsed content and, when the row is
/// expanded and this column has expanded detail, that detail stacked
/// underneath -- so each column's expansion grows downward under its own
/// header (`expanded::render_column_detail`). The cell is a fixed-width
/// `v_flex` clamped to the column's content-fit width so the collapsed table
/// stays aligned and the detail (text wraps to the column width; a dense
/// build/consumable sub-table clips at the column edge) never bleeds into the
/// neighbouring column, matching egui_table's per-cell clip with
/// `AutoSizeMode::Never`. Row height comes from the tallest such cell (flex).
/// Collapsed rows return the bare cell (no wrapping `v_flex`), keeping the
/// non-expanded layout byte-for-byte what it was.
fn render_column_cell(ix: usize, col: ReplayColumn, row: &PlayerRow, layout: &RowLayout, cx: &App) -> AnyElement {
    let collapsed = render_cell(ix, col, row, layout);
    if !layout.is_expanded {
        return collapsed;
    }
    match expanded::render_column_detail(ix, col, row, layout.all_rows, layout.icons, layout.debug, layout.alt_held, cx)
    {
        Some(detail) => v_flex()
            .w(layout.column_widths[col as usize])
            .flex_none()
            .gap_1()
            .overflow_hidden()
            .child(collapsed)
            .child(detail)
            .into_any_element(),
        None => collapsed,
    }
}

/// Per-frame layout shared by every row and the header's scrolling portion:
/// the sticky/scrolling column split, the scrolling section's total width,
/// the content-fit column widths (`PlayerTable::column_widths`, indexed by
/// `ReplayColumn as usize`), the shared horizontal scroll handle, the icon
/// cache, the debug flag, this row's expanded state and entity handle (for
/// the caret/double-click toggle), and the full row list (for the expanded
/// damage-interaction breakdowns, which look up other rows by `db_id`).
/// Bundled into one struct so `render_row` stays under clippy's
/// argument-count limit.
struct RowLayout<'a> {
    sticky_columns: &'a [ReplayColumn],
    scroll_columns: &'a [ReplayColumn],
    scroll_width: f32,
    column_widths: &'a [Pixels],
    icons: &'a IconCache,
    debug: bool,
    h_scroll: &'a ScrollHandle,
    entity: Entity<PlayerTable>,
    is_expanded: bool,
    selected: bool,
    alt_held: bool,
    all_rows: &'a [PlayerRow],
}

/// One row: `layout.sticky_columns` (Actions/Name/ShipName) render as fixed
/// per-column cells outside the horizontal scroll; `layout.scroll_columns`
/// render inside a nested scroll container tracking `layout.h_scroll`, the
/// same handle the header's scrolling portion tracks, so both stay aligned.
/// Mirrors the egui app's `num_sticky_cols(3)`. Each cell is a
/// `render_column_cell`, so when `layout.is_expanded` every column's detail
/// grows downward under its own column (achievements under Name, the build
/// under Skills, the damage breakdowns under ActualDamage/ReceivedDamage);
/// the row's `items_start` top-aligns the columns and its height is the
/// tallest column. A double-click anywhere on the row toggles expansion,
/// mirroring the egui app's whole-row double-click handler in
/// `cell_content_ui`.
fn render_row(ix: usize, row: &PlayerRow, layout: &RowLayout, hover_bg: Hsla, cx: &App) -> AnyElement {
    // A collapsed row is one line of cells and reads across, so its cells are
    // centred on that line; an expanded row grows detail downward under each
    // column, which only lines up if the columns start at the same top edge.
    let align_top = layout.is_expanded;
    let mut sticky = h_flex().flex_none().map(|el| if align_top { el.items_start() } else { el.items_center() });
    for &col in layout.sticky_columns {
        sticky = sticky.child(render_column_cell(ix, col, row, layout, cx));
    }

    let mut scrolling = h_flex()
        .w(px(layout.scroll_width))
        .flex_none()
        .map(|el| if align_top { el.items_start() } else { el.items_center() });
    for &col in layout.scroll_columns {
        scrolling = scrolling.child(render_column_cell(ix, col, row, layout, cx));
    }

    // Selection is what a ctrl+click leaves behind, over the stripe; the
    // egui table paints the same two (`ui/replay_parser/mod.rs:3011`).
    let background = if layout.selected { Some(cx.theme().selection) } else { crate::ui::stripe(ix, cx) };
    let entity = layout.entity.clone();
    let select_entity = layout.entity.clone();
    h_flex()
        .id(ix)
        .w_full()
        .py_0p5()
        .map(|el| if align_top { el.items_start() } else { el.items_center() })
        .when_some(background, |el, color| el.bg(color))
        .hover(move |style| style.bg(hover_bg))
        .on_click(move |event: &ClickEvent, _window, cx: &mut App| {
            if event.modifiers().secondary() {
                select_entity.update(cx, |this, cx| this.toggle_selected(ix, cx));
                return;
            }
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
        )
        .into_any_element()
}

impl Render for PlayerTable {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A window that is not active never sees the alt release, so the
        // modifier is read back here rather than left latched at whatever it
        // was when focus left.
        if !window.is_window_active() && self.alt_held {
            self.alt_held = false;
        }
        if self.widths_dirty {
            self.column_widths = measure_column_widths(&self.model, self.debug, &self.expanded, window);
            self.widths_dirty = false;
        }

        let border = cx.theme().border;
        let hover_bg = cx.theme().muted;

        let sticky_columns: Vec<ReplayColumn> = self.model.columns.iter().copied().take(STICKY_COLUMN_COUNT).collect();
        let scroll_columns: Vec<ReplayColumn> = self.model.columns.iter().copied().skip(STICKY_COLUMN_COUNT).collect();
        if !std::mem::replace(&mut self.widths_loaded, true) {
            self.load_column_widths(cx);
        }

        // Resolved once per frame so the header, the rows and the scrolling
        // section's total all read the same number.
        self.drawn_widths = ReplayColumn::ALL.iter().map(|col| self.width_of(*col)).collect();
        let scroll_width: f32 = scroll_columns.iter().map(|col| self.drawn_widths[*col as usize].as_f32()).sum();

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
            let selected = table.selected == Some(row.db_id);
            let layout = RowLayout {
                sticky_columns: &sticky_columns,
                scroll_columns: &scroll_columns,
                scroll_width,
                column_widths: &table.drawn_widths,
                icons: &table.icons,
                debug: table.debug,
                h_scroll: &h_scroll,
                entity: entity.clone(),
                is_expanded,
                selected,
                alt_held: table.alt_held,
                all_rows: &table.model.rows,
            };
            render_row(ix, row, &layout, hover_bg, cx)
        };

        div()
            .id("replay-table-root")
            .size_full()
            .relative()
            // A width drag keeps following the pointer past the header's own
            // bounds, which is where it goes the moment a column grows.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                this.drag_column(event.position.x, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| this.end_column_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| this.end_column_drag(cx)),
            )
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _window, cx| {
                if this.alt_held == event.modifiers.alt {
                    return;
                }
                this.alt_held = event.modifiers.alt;
                cx.notify();
            }))
            .child(v_flex().size_full().child(header).child(list(self.list_state.clone(), render_item).flex_1()))
            .child(Scrollbar::vertical(&self.list_state))
    }
}
