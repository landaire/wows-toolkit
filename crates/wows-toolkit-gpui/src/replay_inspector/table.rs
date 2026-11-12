//! Custom virtualized player table (collapsed rows). Renders the M1
//! presentation model with a fixed sortable header and a `gpui_kit::list`-backed
//! body. The reader's pinned columns (Name/ShipName unless rearranged; see
//! `ColumnArrangement`) stay at the left edge; the remaining columns scroll
//! horizontally inside a nested container per row and per header, all sharing
//! one scroll handle so they stay aligned. A header dragged sideways moves
//! its column, and a right-click on it pins or unpins it. This layer adds the
//! per-column cell parity pass on top of Task 4's scaffold: colors, NDA text,
//! the multi-segment Name cell
//! (ship-class icon, division, clan tag, player name), Skills tier icons, and
//! hover tooltips for the stat cells that carry a breakdown. A row also
//! carries an expand/collapse state (`expanded`, keyed by `db_id` so it
//! survives re-sorting): the Name cell's caret and a double-click on the row
//! toggle it, growing each column's own cell downward to show that column's
//! `expanded::render_column_detail` under its own header (achievements/
//! ribbons/damage-events under Name, the build under Skills, the damage
//! breakdowns under ActualDamage/ReceivedDamage; see `expanded.rs`).

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::base::MotionStatus;
use gpui_kit::base::MotionValue;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_toolkit_replay_data::columns::BattleOutcome;
use wows_toolkit_replay_data::columns::CaptainPointsTier;
use wows_toolkit_replay_data::columns::CellValue;
use wows_toolkit_replay_data::columns::ColorRole;
use wows_toolkit_replay_data::columns::ColumnArrangement;
use wows_toolkit_replay_data::columns::Placement;
use wows_toolkit_replay_data::columns::PlayerColorKind;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::columns::Section;
use wows_toolkit_replay_data::columns::ShownColumns;
use wows_toolkit_replay_data::columns::StoredArrangement;
use wows_toolkit_replay_data::columns::cell_value;
use wows_toolkit_replay_data::columns::name_color_kind;
use wows_toolkit_replay_data::columns::player_color_kind;
use wows_toolkit_replay_data::damage_mix::DamageLine;
use wows_toolkit_replay_data::damage_mix::DamageMix;
use wows_toolkit_replay_data::model::MatchContext;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::ReplayReportModel;
use wows_toolkit_replay_data::model::SkillWarning;
use wows_toolkit_replay_data::sort::SortColumn;
use wows_toolkit_replay_data::sort::SortOrder;
use wows_toolkit_replay_data::sort::sort_rows;

use super::damage_mix;
use super::expanded;
use super::icons::IconCache;
use super::icons::nation_flag_key;
use super::team_bands;
use super::team_bands::RowMark;
use super::team_bands::TeamDamage;
use super::team_bands::TeamSide;
use crate::replay_inspector::player_colors::class_icon_tint;
use crate::replay_inspector::player_colors::player_color_kind_rgb;
use crate::replay_inspector::player_colors::relation_color_rgb;
use wows_replay_insights::battle_report::ConnectionNote;
use wows_replay_insights::battle_report::TimeOffline;
use wows_replay_insights::personal_rating::PersonalRatingResult;
use wows_replays::types::AccountId;
use wows_replays::types::Relation;
use wows_replays::types::TeamId;
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
    let mut font = window.text_style().font();
    if bold {
        font.weight = FontWeight::BOLD;
    }
    font_width(text, font, window)
}

/// `text_width` for a cell drawn with `ui::tabular`, whose digits are wider
/// than the font's own.
fn figures_width(text: SharedString, window: &mut Window) -> f32 {
    let mut font = window.text_style().font();
    font.features = crate::ui::tabular_figures();
    font_width(text, font, window)
}

fn font_width(text: SharedString, font: Font, window: &mut Window) -> f32 {
    if text.is_empty() {
        return 0.0;
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
/// A row in `expanded` or `closing` also folds
/// `expanded::expanded_detail_width` into that row's per-column width
/// alongside its collapsed content -- `table.rs`'s `render_column_detail`
/// clamps a row's detail (the Skills column's captain-skill grid and
/// consumables table, in particular) to the column's measured width, so
/// without this the detail would clip at the collapsed-only width instead of
/// growing the column to fit it. A closing row counts until its detail has
/// closed, so the columns do not narrow under the detail while it is drawn.
///
/// Run once per data/column/debug/detail-set change (`PlayerTable::new`,
/// `set_columns`, `set_debug`, `toggle_expanded`, `finish_closing`), gated
/// behind `PlayerTable::widths_dirty` in `render`, and once per rem size
/// change (`PlayerTable::measured_rem`) -- never per frame.
fn measure_column_widths(
    model: &ReplayReportModel,
    debug: bool,
    expanded: &HashSet<AccountId>,
    closing: &HashMap<AccountId, CloseTicket>,
    window: &mut Window,
) -> MeasuredWidths {
    let shows_detail = |id: &AccountId| expanded.contains(id) || closing.contains_key(id);
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
    let mut details = HashMap::new();
    for &col in &model.columns {
        // A sortable header always holds room for its indicator, so sorting
        // never changes a width.
        let indicator_w = if column_sort(col).is_some() { gap_1 + icon_default } else { 0. };
        let header_w = text_width(column_label(col).into(), window, true) + indicator_w;
        let content_w = match col {
            ReplayColumn::Name => {
                let mut max_w = header_w;
                for row in &model.rows {
                    let cell = cell_value(row, col, debug);
                    let text_w = text_width(cell.text.into(), window, false);
                    let species_w = text_width(row.ship_species_text.clone().into(), window, false);
                    let icon_w = species_w.max(SHIP_ICON_WIDTH);
                    let status_count = usize::from(row.is_hidden_profile)
                        + usize::from(row.connection.is_some())
                        + usize::from(!row.twitch_candidates.is_empty());
                    let mut row_w = icon_default
                        + gap_1
                        + icon_w
                        + gap_1
                        + text_w
                        + ROW_ACTIONS_WIDTH
                        + status_count as f32 * (icon_default + gap_1)
                        + connection_label_width(row, window);
                    if shows_detail(&row.db_id) {
                        let detail_w = framed_detail_width(col, row, &model.rows, debug, window);
                        if detail_w > 0. {
                            details.insert((row.db_id, col), px(detail_w + cell_padding));
                        }
                        row_w = row_w.max(detail_w);
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
                    if shows_detail(&row.db_id) {
                        let detail_w = framed_detail_width(col, row, &model.rows, debug, window);
                        if detail_w > 0. {
                            details.insert((row.db_id, col), px(detail_w + cell_padding));
                        }
                        row_w = row_w.max(detail_w);
                    }
                    max_w = max_w.max(row_w);
                }
                max_w
            }
            _ => {
                let mut max_w = header_w;
                for row in &model.rows {
                    let cell = cell_value(row, col, debug);
                    let mut row_w = if col == ReplayColumn::ShipName {
                        // The nation flag ahead of the name, with its gap. Held
                        // whether or not the flag has loaded yet, as the Name
                        // column holds its class icon.
                        let flag_w = if row.ship_nation.is_some() { rem * SHIP_FLAG_WIDTH_REMS + gap_1 } else { 0. };
                        flag_w + text_width(cell.text.into(), window, false)
                    } else {
                        figures_width(cell.text.into(), window)
                    };
                    if shows_detail(&row.db_id) {
                        let detail_w = framed_detail_width(col, row, &model.rows, debug, window);
                        if detail_w > 0. {
                            details.insert((row.db_id, col), px(detail_w + cell_padding));
                        }
                        row_w = row_w.max(detail_w);
                    }
                    max_w = max_w.max(row_w);
                }
                max_w
            }
        };
        widths[col as usize] = px((content_w + cell_padding + content_slack).clamp(COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH));
    }

    // The same room as a fitted column, so a widened column shows its detail
    // without the cut-off fade. The detail widths already hold the padding.
    let mut detail_fit: Vec<Option<Pixels>> = vec![None; ReplayColumn::ALL.len()];
    for ((_, col), needs) in &details {
        let fit = px((needs.as_f32() + content_slack).clamp(COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH));
        let slot = &mut detail_fit[*col as usize];
        *slot = Some(slot.map_or(fit, |held| held.max(fit)));
    }

    MeasuredWidths { columns: widths, details, detail_fit }
}

/// What a disconnect chip's time adds to the Name column beyond its glyph:
/// the label, its gap and the chip's padding. Measured at the body size, a
/// little over the chip's smaller text.
fn connection_label_width(row: &PlayerRow, window: &mut Window) -> f32 {
    let Some(time) = row.connection.as_ref().and_then(time_offline) else {
        return 0.;
    };
    let rem = window.rem_size().as_f32();

    text_width(offline_label(time).into(), window, false) + rem * 0.125 + rem * 0.25
}

/// What `measure_column_widths` found.
struct MeasuredWidths {
    /// Each column's content-fit width, indexed by `ReplayColumn as usize`.
    columns: Vec<Pixels>,
    /// The width each shown row's detail needs in each column, so a cell
    /// drawn narrower can mark where its detail is cut off.
    details: HashMap<(AccountId, ReplayColumn), Pixels>,
    /// The width each column needs to show every shown detail in full, by
    /// `ReplayColumn as usize`. `None` where no shown row has detail there.
    detail_fit: Vec<Option<Pixels>>,
}

/// Header label for a column, matching the egui app's `ui.replay.column.*`
/// English strings.
pub(super) const fn column_label_key(col: ReplayColumn) -> &'static str {
    match col {
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
        ReplayColumn::Skills | ReplayColumn::TimeLived => None,
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
    /// Rows that have left `expanded` but whose detail is still closing.
    /// They keep their share of the column widths until it has closed.
    closing: HashMap<AccountId, CloseTicket>,
    /// The ticket the next close is given.
    next_close: CloseTicket,
    /// The row a ctrl+click picked out, if any. One at a time, as in the egui
    /// table.
    selected: Option<AccountId>,
    /// Content-fit width per column, indexed by `ReplayColumn as usize`.
    /// Recomputed by `measure_column_widths` whenever `widths_dirty` is set;
    /// `render` is the only reader/writer of that flag, since computing a
    /// width needs `&mut Window` (real text measurement), which only `render`
    /// has.
    column_widths: Vec<Pixels>,
    /// The width each shown row's detail needs per column, from the same
    /// measurement.
    detail_widths: HashMap<(AccountId, ReplayColumn), Pixels>,
    /// The width each column needs for the widest detail shown in it, a
    /// closing one included, from the same measurement. Remeasured as each
    /// row opens or finishes closing.
    detail_fit: Vec<Option<Pixels>>,
    /// Set on construction and whenever the columns/data/debug flag change
    /// (`set_columns`, `set_debug`); cleared by `render` after it recomputes
    /// `column_widths`. Keeps the (17-column x up-to-24-row) measurement pass
    /// off the per-frame path.
    widths_dirty: bool,
    /// The rem size `column_widths` was measured at, so a zoom measures them
    /// again. `None` until the first measurement.
    measured_rem: Option<Pixels>,
    /// The width every column is drawn at: what was dragged or what the
    /// content measured, widened for any detail shown in it. Rebuilt each
    /// render, so rows and header agree.
    drawn_widths: Vec<Pixels>,
    /// Widths the reader dragged a column to, by `ReplayColumn as usize`.
    /// `None` leaves that column fitted to its content, which is what every
    /// column does until one is dragged.
    width_overrides: Vec<Option<Pixels>>,
    /// Columns whose width a drag set while a detail widened them, drawn at
    /// that width rather than the detail's until no detail shows in them.
    fit_waived: HashSet<ReplayColumn>,
    /// Whether the saved widths have been read back yet. One shot, on the
    /// first frame.
    widths_loaded: bool,
    /// The column being dragged and the width it had when the drag started,
    /// so the new width follows the pointer's total travel rather than
    /// accumulating per-frame deltas.
    resizing: Option<ColumnDrag>,
    /// The row and column the right button was last pressed on, which is what
    /// "copy cell" copies. A right-click menu carries no pointer position of
    /// its own, and the cell is what was under it. Taken when a menu reads it,
    /// so a press that lands on a row but not on one of its cells offers
    /// nothing rather than whatever was pressed before.
    right_clicked_cell: Option<(AccountId, ReplayColumn)>,
    /// The table's own bounds, measured as it is laid out, so the frozen
    /// section's cap can be worked out in pixels and a pointer placed among
    /// the columns. `None` before the first frame.
    viewport: Option<Bounds<Pixels>>,

    /// Where the reader has put the columns, and which are pinned.
    arrangement: ColumnArrangement,
    /// Whether this table has changed `arrangement` itself, which a stored
    /// arrangement read afterwards does not undo.
    arrangement_changed: bool,
    /// A column being moved by its header, if any.
    reorder: Option<ColumnReorder>,
    /// Columns sliding to new places, if any.
    slide: Option<ColumnSlide>,
    /// How far each column is drawn from its place while the columns slide,
    /// by `ReplayColumn as usize`.
    column_shift: Vec<Pixels>,
    /// Where each column was last drawn from the table's left edge, scrolled
    /// and slid, by `ReplayColumn as usize`. `None` for a column not drawn.
    drawn_starts: Vec<Option<Pixels>>,
    /// Where the pinned section ends, as last drawn.
    pinned_edge: Pixels,
    /// Ends a drag when the window loses focus, since the release then goes
    /// elsewhere. Made on the first frame, which is the first to have the
    /// window.
    activation: Option<Subscription>,
}

/// Which close of a row's detail a delayed finish belongs to, so the finish
/// of a close since undone does not end a later one early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CloseTicket(u64);

impl CloseTicket {
    fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Columns sliding from where they were drawn to new places.
struct ColumnSlide {
    started: std::time::Instant,
    /// Where each column was drawn when the slide began, as `drawn_starts`.
    from: Vec<Option<Pixels>>,
}

/// The arrangement last read from or written to the settings database, so
/// a table opened later draws it from its first frame.
struct KnownArrangement(ColumnArrangement);

impl Global for KnownArrangement {}

/// A column being moved by its header.
#[derive(Clone)]
enum ColumnReorder {
    /// Pressed, but not yet moved far enough to be told from a click.
    Pressed { column: ReplayColumn, pointer_start: Pixels, grab: Pixels },
    /// Following the pointer. `preview` is what a release here leaves.
    Moving { column: ReplayColumn, grab: Pixels, pointer: Pixels, preview: ColumnArrangement },
}

impl ColumnReorder {
    fn column(&self) -> ReplayColumn {
        match self {
            Self::Pressed { column, .. } | Self::Moving { column, .. } => *column,
        }
    }

    /// The pointer's distance into the header from its left edge, so the
    /// floating header keeps the point it was taken by under the pointer.
    fn grab(&self) -> Pixels {
        match self {
            Self::Pressed { grab, .. } | Self::Moving { grab, .. } => *grab,
        }
    }
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
/// Positional: one entry per column in `ReplayColumn::ALL` order. The key
/// carries a version because of that -- dropping the Actions column shifted
/// every index, and the stored list would otherwise be read onto the wrong
/// columns.
const COLUMN_WIDTHS_KEY: &str = "replay_column_widths_v2";

/// The width of the strip on a header's trailing edge that starts a drag.
const RESIZE_GRIP: Pixels = px(6.);

/// The most of the table's width the two frozen columns may take.
///
/// They are laid out at their own content width, which on a narrow dock panel
/// is wider than the panel itself. Left to it they would paint past the
/// panel's edge over whatever is beside it, and leave the scrolling columns no
/// room to be reached at all. Capped, they clip instead, and the rest of the
/// table stays scrollable.
const STICKY_MAX_FRACTION: f32 = 0.6;

/// Room kept for the horizontal scrollbar, so it is still reachable however
/// wide the frozen columns are drawn.
const H_SCROLLBAR_MIN_WIDTH: Pixels = px(80.);

/// The settings row the column order and the pinned set are kept in, by
/// column name. The port's own row, as with the widths.
const COLUMN_ARRANGEMENT_KEY: &str = "replay_column_arrangement";

/// How faint the dragged column's own cells are drawn in its landing slot.
const DRAGGED_OPACITY: f32 = 0.35;

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
            closing: HashMap::new(),
            next_close: CloseTicket(0),
            selected: None,
            column_widths: vec![px(COLUMN_MIN_WIDTH); ReplayColumn::ALL.len()],
            detail_widths: HashMap::new(),
            detail_fit: vec![None; ReplayColumn::ALL.len()],
            drawn_widths: vec![px(COLUMN_MIN_WIDTH); ReplayColumn::ALL.len()],
            widths_loaded: false,
            width_overrides: vec![None; ReplayColumn::ALL.len()],
            fit_waived: HashSet::new(),
            resizing: None,
            right_clicked_cell: None,
            viewport: None,
            widths_dirty: true,
            measured_rem: None,
            // Before the settings database has been read, nothing is known
            // to replace the default.
            arrangement: cx
                .try_global::<KnownArrangement>()
                .map_or_else(ColumnArrangement::new, |known| known.0.clone()),
            arrangement_changed: false,
            reorder: None,
            slide: None,
            column_shift: vec![px(0.); ReplayColumn::ALL.len()],
            drawn_starts: vec![None; ReplayColumn::ALL.len()],
            pinned_edge: px(0.),
            activation: None,
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
    /// Whether the recording player is in a test ship, which is the only
    /// case where hiding their own figures means anything.
    pub fn self_is_test_ship(&self) -> bool {
        self.model.rows.iter().any(|row| row.is_self && row.is_test_ship)
    }

    /// Whether the recording player's own figures are currently hidden.
    pub fn self_stats_hidden(&self) -> bool {
        self.model.rows.iter().any(|row| row.is_self && row.manual_stat_hide_toggle)
    }

    /// Hides or shows the recording player's own figures.
    ///
    /// The egui app offers this for a test ship, where the NDA applies to the
    /// player's own numbers as much as to anyone else's.
    pub fn set_self_stats_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        let Some(row) = self.model.rows.iter_mut().find(|row| row.is_self) else { return };
        if row.manual_stat_hide_toggle == hidden {
            return;
        }
        row.manual_stat_hide_toggle = hidden;
        self.widths_dirty = true;
        self.list_state.remeasure_items(0..self.model.rows.len());
        cx.notify();
    }

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
    /// and notifies; `render`'s pinned/scrolling split is recomputed from
    /// `model.columns` and the arrangement fresh on every render. Marks
    /// `widths_dirty` so a newly-shown column gets a real fit width instead
    /// of its placeholder `COLUMN_MIN_WIDTH` entry.
    pub fn set_columns(&mut self, columns: Vec<ReplayColumn>, cx: &mut Context<Self>) {
        self.model.columns = columns;
        self.widths_dirty = true;
        cx.notify();
    }

    /// Ctrl+click picks a row out and clicking it again puts it back, which
    /// is how the egui table marks the one it is reading
    /// (`ui/replay_parser/mod.rs:2620`).
    fn toggle_selected(&mut self, ix: usize, cx: &mut Context<Self>) {
        let db_id = self.model.rows.get(ix).map(|row| row.db_id);
        self.selected = if self.selected == db_id { None } else { db_id };
        cx.notify();
    }

    /// Toggles row `ix`'s expanded state and remeasures just that row.
    /// `remeasure_items` (unlike `sort_by`'s `reset`) preserves the list's
    /// current scroll position, so expanding a row on-screen doesn't jump the
    /// viewport. Mirrors the egui app's `is_row_expanded`/`row_heights`
    /// toggle in `cell_content_ui`. An opening row marks `widths_dirty`, since
    /// its Skills/Name/damage columns may need to grow to fit its detail; a
    /// closing row moves to `closing` and keeps the widths it had until
    /// `finish_closing`.
    fn toggle_expanded(&mut self, ix: usize, cx: &mut Context<Self>) {
        let db_id = self.model.rows[ix].db_id;
        if self.expanded.remove(&db_id) {
            self.begin_closing(db_id, cx);
        } else {
            self.expanded.insert(db_id);
            if self.closing.remove(&db_id).is_none() {
                self.widths_dirty = true;
            }
        }
        self.list_state.remeasure_items(ix..ix + 1);
        cx.notify();
    }

    /// Moves a row to `closing` until its detail has closed. The finish also
    /// comes from a timer of the reveal's length, since a row scrolled or
    /// sorted out of view is not drawn to see its detail close.
    fn begin_closing(&mut self, db_id: AccountId, cx: &mut Context<Self>) {
        let ticket = self.next_close;
        self.next_close = ticket.next();
        self.closing.insert(db_id, ticket);

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(crate::motion::reveal_duration()).await;
            let _ = this.update(cx, |this, cx| {
                if this.closing.get(&db_id) == Some(&ticket) {
                    this.finish_closing(db_id, cx);
                }
            });
        })
        .detach();
    }

    /// Lets a row whose detail has closed give up its share of the column
    /// widths.
    fn finish_closing(&mut self, db_id: AccountId, cx: &mut Context<Self>) {
        if self.closing.remove(&db_id).is_some() {
            self.widths_dirty = true;
            cx.notify();
        }
    }

    /// The width the reader set for `col`, no narrower than its header's
    /// furniture, or what its content measured.
    fn width_of(&self, col: ReplayColumn) -> Pixels {
        match self.width_overrides[col as usize] {
            Some(set) => set.max(self.header_min(col)),
            None => self.column_widths[col as usize],
        }
    }

    /// The narrowest `col` is drawn at its reader's width: a sortable header
    /// keeps its padding and indicator, which grow with the zoom.
    fn header_min(&self, col: ReplayColumn) -> Pixels {
        match (column_sort(col), self.measured_rem) {
            // `px_1` either side of a one-rem indicator.
            (Some(_), Some(rem)) => COLUMN_DRAG_MIN.max(rem * 1.5),
            _ => COLUMN_DRAG_MIN,
        }
    }

    /// The width `col` is drawn at before the frozen cap: `width_of`, widened
    /// to the widest detail shown in it unless a drag set its width since.
    /// The reader's width is left as set, so the column returns to it once
    /// the last such detail has closed.
    fn fitted_width(&self, col: ReplayColumn) -> Pixels {
        let set = self.width_of(col);
        if self.fit_waived.contains(&col) {
            return set;
        }

        self.detail_fit[col as usize].map_or(set, |fit| set.max(fit))
    }

    /// Starts a width drag on `col` from the pointer's current position, at
    /// the width the column is drawn at: the grip is under the pointer there,
    /// whether the frozen cap narrowed the column or a detail widened it.
    fn start_column_drag(&mut self, col: ReplayColumn, at: Pixels, cx: &mut Context<Self>) {
        self.resizing =
            Some(ColumnDrag { column: col, pointer_start: at, width_start: self.drawn_widths[col as usize] });
        cx.notify();
    }

    /// Follows a width drag. A no-op when nothing is being dragged, which is
    /// every pointer move outside one.
    fn drag_column(&mut self, at: Pixels, cx: &mut Context<Self>) {
        let Some(drag) = self.resizing else { return };

        let mut width = (drag.width_start + (at - drag.pointer_start)).max(COLUMN_DRAG_MIN);
        // A frozen column may not push the frozen section past its cap: the
        // section is clipped at that edge, and the grips sit on it.
        let frozen = self.frozen_columns();
        if let Some(cap) = self.sticky_cap()
            && frozen.contains(&drag.column)
        {
            width = width.min(frozen_room(&frozen, &self.drawn_widths, cap, drag.column).max(COLUMN_DRAG_MIN));
        }
        // The reader is setting this width with the detail in view, so it is
        // drawn as dragged rather than held at what the detail needs.
        if self.detail_fit[drag.column as usize].is_some() {
            self.fit_waived.insert(drag.column);
        }
        self.width_overrides[drag.column as usize] = Some(width);
        cx.notify();
    }

    /// Drops whatever drag is under way when its release cannot arrive: the
    /// button is found up or the window has lost focus. A width drag keeps
    /// the width it has shown; a column move is undone.
    fn abandon_drags(&mut self, cx: &mut Context<Self>) {
        self.end_column_drag(cx);
        if let Some(ColumnReorder::Moving { .. }) = self.reorder.take() {
            self.begin_slide(cx);
            cx.notify();
        }
    }

    /// The columns that stay put while the rest scroll, as drawn.
    fn frozen_columns(&self) -> Vec<ReplayColumn> {
        self.drawn_columns().pinned
    }

    /// The shown columns in the reader's arrangement.
    fn shown_columns(&self) -> ShownColumns {
        self.arrangement.shown(&self.model.columns)
    }

    /// The shown columns as drawn: while a column is being moved, in the
    /// order a release would leave.
    fn drawn_columns(&self) -> ShownColumns {
        match &self.reorder {
            Some(ColumnReorder::Moving { preview, .. }) => preview.shown(&self.model.columns),
            Some(ColumnReorder::Pressed { .. }) | None => self.shown_columns(),
        }
    }

    /// The column being moved, once it has left its place.
    fn moving_column(&self) -> Option<ReplayColumn> {
        match &self.reorder {
            Some(ColumnReorder::Moving { column, .. }) => Some(*column),
            Some(ColumnReorder::Pressed { .. }) | None => None,
        }
    }

    /// Notes a press on `col`'s header, which becomes a move once the
    /// pointer travels. A press that stays put is a click.
    fn press_header(&mut self, col: ReplayColumn, at: Pixels) {
        let (Some(viewport), Some(start)) = (self.viewport, self.drawn_starts[col as usize]) else { return };

        let left = viewport.origin.x + start;
        self.reorder = Some(ColumnReorder::Pressed { column: col, pointer_start: at, grab: at - left });
    }

    /// Follows a column move. A no-op when no header is pressed.
    fn follow_reorder(&mut self, at: Pixels, cx: &mut Context<Self>) {
        // How far a pressed header must travel before it moves its column
        // rather than sorting by it.
        const THRESHOLD: Pixels = px(4.);

        let Some(reorder) = self.reorder.as_ref() else { return };
        if let ColumnReorder::Pressed { pointer_start, .. } = reorder
            && (at - *pointer_start).abs() < THRESHOLD
        {
            return;
        }
        let Some(viewport) = self.viewport else { return };

        let column = reorder.column();
        let grab = reorder.grab();
        let mut preview = self.arrangement.clone();
        preview.place(column, self.placement_at(column, at - viewport.origin.x), &self.model.columns);
        if self.drawn_columns() != preview.shown(&self.model.columns) {
            self.begin_slide(cx);
        }
        self.reorder = Some(ColumnReorder::Moving { column, grab, pointer: at, preview });
        cx.notify();
    }

    /// Where `column`, dragged to `x` from the table's left edge, lands among
    /// the other shown columns: before the first whose middle lies right of
    /// the pointer, in the section the pointer is over.
    fn placement_at(&self, column: ReplayColumn, x: Pixels) -> Placement {
        // Measured against the zone as drawn, so a column taken into it
        // stays pinned until it is taken back out past the zone's edge.
        let section = if x < self.pinned_edge && self.can_pin(column) { Section::Pinned } else { Section::Scrolling };

        let mut others = self.shown_columns();
        others.pinned.retain(|col| *col != column);
        others.scrolling.retain(|col| *col != column);
        let reserve = pinned_reserve(true, &others.pinned);
        let offsets = column_offsets(&others, &self.drawn_widths, self.sticky_cap(), reserve);
        let (list, scroll) = match section {
            Section::Pinned => (&others.pinned, px(0.)),
            Section::Scrolling => (&others.scrolling, self.h_scroll.offset().x),
        };
        let index = list
            .iter()
            .take_while(|col| {
                offsets.starts[**col as usize]
                    .is_some_and(|start| start + scroll + self.drawn_widths[**col as usize] / 2. < x)
            })
            .count();

        Placement { section, index }
    }

    /// Whether `col` may be pinned, or stay pinned when moved. A shown
    /// column must be left to scroll, and the pinned ones must fit their cap
    /// at their narrowest: otherwise the rest of the table cannot be reached,
    /// nor the clipped pinned headers to unpin them.
    fn can_pin(&self, col: ReplayColumn) -> bool {
        let shown = self.shown_columns();
        let pinned = shown.pinned.iter().filter(|other| **other != col).count() + 1;
        let scrolling_left = shown.scrolling.iter().any(|other| *other != col);
        let fits = self.sticky_cap().is_none_or(|cap| COLUMN_DRAG_MIN * pinned as f32 <= cap);

        scrolling_left && fits
    }

    /// Ends a column move, keeping where it got to.
    fn end_reorder(&mut self, cx: &mut Context<Self>) {
        let Some(ColumnReorder::Moving { preview, .. }) = self.reorder.take() else { return };

        if preview != self.arrangement {
            self.arrangement = preview;
            self.save_arrangement(cx);
        }
        cx.notify();
    }

    /// Pins `col` after the pinned columns, or unpins it beside them.
    fn set_pinned(&mut self, col: ReplayColumn, pinned: bool, cx: &mut Context<Self>) {
        if self.arrangement.is_pinned(col) == pinned || (pinned && !self.can_pin(col)) {
            return;
        }
        if pinned {
            self.arrangement.pin(col);
        } else {
            self.arrangement.unpin(col);
        }
        self.begin_slide(cx);
        self.save_arrangement(cx);
        cx.notify();
    }

    /// Starts the columns sliding from where they are drawn now.
    fn begin_slide(&mut self, cx: &mut Context<Self>) {
        self.slide = Some(ColumnSlide { started: cx.background_executor().now(), from: self.drawn_starts.clone() });
    }

    /// What `save_arrangement` writes and the first frame reads back.
    fn stored_arrangement(&self) -> StoredArrangement {
        self.arrangement.to_stored()
    }

    /// Writes the column order and the pinned set back, as the widths are,
    /// and keeps them for the tables opened after.
    fn save_arrangement(&mut self, cx: &mut Context<Self>) {
        self.arrangement_changed = true;
        cx.set_global(KnownArrangement(self.arrangement.clone()));
        crate::settings_store::save(COLUMN_ARRANGEMENT_KEY, &self.stored_arrangement(), cx);
    }

    /// Takes up the arrangement read from the settings database, which
    /// lands after the first frame. A change this table made meanwhile is
    /// newer and stays, and an arrangement another table already knows is
    /// newer than this read.
    ///
    /// A stored name no column answers to is dropped and a column it does
    /// not name takes its default place (`ColumnArrangement::from_stored`):
    /// the stored list may be from a version with another column set, and the
    /// reader's order for the columns both share is still theirs.
    fn adopt_stored_arrangement(&mut self, stored: Option<&StoredArrangement>, cx: &mut Context<Self>) {
        if self.arrangement_changed {
            return;
        }

        let arrangement = match cx.try_global::<KnownArrangement>() {
            Some(known) => known.0.clone(),
            None => {
                // Nothing stored is the default arrangement, never changed.
                let read = stored.map_or_else(ColumnArrangement::new, ColumnArrangement::from_stored);
                cx.set_global(KnownArrangement(read.clone()));
                read
            }
        };
        if arrangement != self.arrangement {
            // A move in progress was previewed against what this replaces.
            self.reorder = None;
            self.arrangement = arrangement;
            cx.notify();
        }
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

    /// Reads the dragged widths and the column arrangement back on the first
    /// frame, once the config database is open. `new` runs before it is.
    fn load_column_settings(&mut self, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        cx.spawn(async move |this, cx| {
            let stored = crate::runtime::spawn(cx, async move {
                let widths =
                    wows_toolkit_config::queries::get_setting::<Vec<Option<f32>>>(&pool, COLUMN_WIDTHS_KEY).await;
                let arrangement =
                    wows_toolkit_config::queries::get_setting::<StoredArrangement>(&pool, COLUMN_ARRANGEMENT_KEY).await;
                (widths, arrangement)
            })
            .await;
            let Ok((widths, arrangement)) = stored else { return };
            let _ = this.update(cx, |this, cx| {
                for (slot, width) in this.width_overrides.iter_mut().zip(widths.into_iter().flatten()) {
                    // A width narrower than the grip would leave a column
                    // that cannot be grabbed to widen again.
                    *slot = width.map(|width| px(width).max(COLUMN_DRAG_MIN));
                }
                this.adopt_stored_arrangement(arrangement.as_ref(), cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Records which cell the right button went down on, so the menu that
    /// follows knows which one to copy.
    fn note_right_clicked_cell(&mut self, player: AccountId, col: ReplayColumn) {
        self.right_clicked_cell = Some((player, col));
    }

    /// The cell a menu about to be built may copy, which it consumes: the next
    /// press has to record its own.
    fn take_right_clicked_cell(&mut self) -> Option<(AccountId, ReplayColumn)> {
        self.right_clicked_cell.take()
    }

    /// The widest the pinned columns may be drawn, in pixels. `None` until
    /// the table has been laid out once.
    fn sticky_cap(&self) -> Option<Pixels> {
        self.viewport.map(|viewport| viewport.size.width * STICKY_MAX_FRACTION)
    }

    /// One cell's text as the table draws it.
    fn cell_text(&self, player: AccountId, col: ReplayColumn) -> Option<String> {
        let row = self.model.rows.iter().find(|row| row.db_id == player)?;
        Some(cell_value(row, col, self.debug).text.to_string())
    }

    /// One row's visible cells, tab separated, which is what a spreadsheet
    /// reads back as columns.
    fn row_text(&self, player: AccountId) -> Option<String> {
        let row = self.model.rows.iter().find(|row| row.db_id == player)?;
        Some(
            self.shown_columns()
                .iter()
                .map(|col| cell_value(row, col, self.debug).text.to_string())
                .collect::<Vec<_>>()
                .join("\t"),
        )
    }

    /// The whole table: the column headings, then every row in the order the
    /// table is sorted into.
    fn table_text(&self) -> String {
        let mut lines = vec![self.shown_columns().iter().map(column_label).collect::<Vec<_>>().join("\t")];
        lines.extend(self.model.rows.iter().filter_map(|row| self.row_text(row.db_id)));
        lines.join("\n")
    }

    fn row_markdown(&self, player: AccountId) -> Option<String> {
        let row = self.model.rows.iter().find(|row| row.db_id == player)?;
        Some(self.markdown_table(std::iter::once(row)))
    }

    fn table_markdown(&self) -> String {
        self.markdown_table(self.model.rows.iter())
    }

    fn markdown_table<'a>(&self, rows: impl Iterator<Item = &'a PlayerRow>) -> String {
        let columns = self.shown_columns();
        let line = |cells: Vec<String>| format!("| {} |", cells.join(" | "));
        let mut lines = vec![
            line(columns.iter().map(|col| markdown_cell(&column_label(col))).collect()),
            line(columns.iter().map(|_| "---".to_owned()).collect()),
        ];
        lines.extend(rows.map(|row| {
            line(columns.iter().map(|col| markdown_cell(&cell_value(row, col, self.debug).text)).collect())
        }));
        lines.join("\n")
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
                    // The grip sits inside the header, and the header sorts
                    // when it is clicked and moves its column when it is
                    // dragged. Without this, a width drag would do both.
                    cx.stop_propagation();
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

        let sorted = column_sort(col).filter(|sort_column| self.sort.column() == *sort_column).map(|_| self.sort);
        let base = div()
            .id(("replay-header", col as usize))
            .test_support()
            .w(self.drawn_widths[col as usize])
            .flex_none()
            .relative()
            .left(self.column_shift[col as usize])
            .px_1()
            .py_1()
            .flex()
            .items_center()
            .gap_1()
            .font_weight(FontWeight::BOLD)
            .whitespace_nowrap()
            .overflow_hidden()
            .when(self.moving_column() == Some(col), |el| el.opacity(DRAGGED_OPACITY))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, _cx| this.press_header(col, event.position.x)),
            )
            .child(grip)
            // The label gives way to an ellipsis first; the indicator keeps
            // its size and its place at the end.
            .child(
                div()
                    .id(("replay-header-label", col as usize))
                    .test_support()
                    .min_w_0()
                    .flex_shrink_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(column_label(col)),
            )
            .when_some(sorted, |el, sort| {
                el.child(
                    div()
                        .id(("replay-header-sort", col as usize))
                        .test_support()
                        .flex_none()
                        .child(Icon::new(sort_caret_icon(sort))),
                )
            });

        let base = match column_sort(col) {
            None => base,
            Some(sort_column) => {
                base.cursor_pointer().on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                    // A press that moved its column is not a request to sort.
                    if this.moving_column().is_some() {
                        return;
                    }
                    this.sort_by(sort_column, cx);
                }))
            }
        };

        let table = cx.entity();
        base.context_menu(move |menu, _window, cx| {
            let (pinned, refused) = {
                let this = table.read(cx);
                let pinned = this.arrangement.is_pinned(col);
                (pinned, !pinned && !this.can_pin(col))
            };
            let label = if pinned { t!("ui.replay.context.unpin_column") } else { t!("ui.replay.context.pin_column") };
            let table = table.clone();
            menu.item(
                PopupMenuItem::new(label.into_owned()).disabled(refused).on_click(move |_event, _window, cx| {
                    table.update(cx, |this, cx| this.set_pinned(col, !pinned, cx))
                }),
            )
        })
        .into_any_element()
    }

    /// What shows while a column is being moved: the pinned zone tinted to
    /// its edge, the slot the column will land in, and its header following
    /// the pointer. Draws over the table and takes no pointer events.
    fn reorder_overlay(&self, cx: &App) -> Option<AnyElement> {
        let Some(ColumnReorder::Moving { column, grab, pointer, preview }) = &self.reorder else { return None };
        let viewport = self.viewport?;

        let accent = cx.theme().primary;
        let width = self.drawn_widths[*column as usize];

        // A zone that will not take the column is drawn without the accent.
        let (zone_tint, zone_edge) =
            if self.can_pin(*column) { (accent, accent) } else { (cx.theme().muted_foreground, cx.theme().border) };
        let zone = div()
            .id("replay-pinned-zone")
            .test_support()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .w(self.pinned_edge)
            .bg(zone_tint.opacity(0.08))
            .border_r_2()
            .border_color(zone_edge);

        // A scrolling slot is cut at the pinned edge, which its cells pass
        // under as the columns scroll.
        let start = self.drawn_starts[*column as usize]?;
        let left = if preview.is_pinned(*column) { start } else { start.max(self.pinned_edge) };
        let right = start + width;
        let slot = (right > left).then(|| {
            div()
                .id("replay-column-slot")
                .test_support()
                .absolute()
                .top_0()
                .bottom_0()
                .left(left)
                .w(right - left)
                .bg(accent.opacity(0.12))
                .border_2()
                .border_color(accent.opacity(0.6))
        });

        let ghost_left = (*pointer - viewport.origin.x - *grab).min(viewport.size.width - width).max(px(0.));
        let ghost = div()
            .id("replay-column-ghost")
            .test_support()
            .absolute()
            .top_0()
            .left(ghost_left)
            .w(width)
            .px_1()
            .py_1()
            .font_weight(FontWeight::BOLD)
            .whitespace_nowrap()
            .overflow_hidden()
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .border_1()
            .border_color(accent)
            .shadow_md()
            .child(column_label(*column));

        Some(div().absolute().inset_0().child(zone).children(slot).child(ghost).into_any_element())
    }
}

impl EventEmitter<PlayerTableEvent> for PlayerTable {}

/// Where the pinned section ends and each shown column starts, from the
/// table's left edge with the scrolling columns unscrolled.
struct ColumnOffsets {
    pinned_edge: Pixels,
    /// By `ReplayColumn as usize`; `None` for a column not shown.
    starts: Vec<Option<Pixels>>,
}

/// Lays `shown` out at `widths`: the pinned columns from the left edge, the
/// scrolling ones from where the pinned section is cut at `cap`, and no
/// nearer the left edge than `reserve`.
fn column_offsets(shown: &ShownColumns, widths: &[Pixels], cap: Option<Pixels>, reserve: Pixels) -> ColumnOffsets {
    let mut starts = vec![None; ReplayColumn::ALL.len()];

    let mut x = px(0.);
    for col in &shown.pinned {
        starts[*col as usize] = Some(x);
        x += widths[*col as usize];
    }

    let pinned_edge = cap.map_or(x, |cap| x.min(cap)).max(reserve);
    let mut x = pinned_edge;
    for col in &shown.scrolling {
        starts[*col as usize] = Some(x);
        x += widths[*col as usize];
    }

    ColumnOffsets { pinned_edge, starts }
}

/// Room held at the left edge for pinning while a column is `carried` and
/// none of `pinned` is shown. Without it the zone that pins would lie over
/// the first scrolling column, and a column could not be dropped before a
/// narrow one there without pinning it.
fn pinned_reserve(carried: bool, pinned: &[ReplayColumn]) -> Pixels {
    const RESERVE: Pixels = px(24.);

    if carried && pinned.is_empty() { RESERVE } else { px(0.) }
}

/// The order the frozen columns give way in when they overrun their cap:
/// Name first, then the rest from the right.
fn give_way_order(frozen: &[ReplayColumn]) -> Vec<ReplayColumn> {
    let name = frozen.iter().copied().filter(|col| *col == ReplayColumn::Name);
    let rest = frozen.iter().copied().rev().filter(|col| *col != ReplayColumn::Name);

    name.chain(rest).collect()
}

/// Narrows the drawn widths of the frozen columns until they fit `cap`, each
/// no narrower than `COLUMN_DRAG_MIN`, so every frozen grip stays inside the
/// clipped section.
fn fit_frozen_widths(widths: &mut [Pixels], frozen: &[ReplayColumn], cap: Pixels) {
    let total: Pixels = frozen.iter().map(|col| widths[*col as usize]).sum();
    let mut excess = total - cap;

    for col in give_way_order(frozen) {
        if excess <= px(0.) {
            break;
        }
        let width = &mut widths[col as usize];
        let give = (*width - COLUMN_DRAG_MIN).max(px(0.)).min(excess);
        *width -= give;
        excess -= give;
    }
}

/// The widest `col` may be dragged to before the frozen section passes
/// `cap`, with the other frozen columns held at their drawn widths: a drag
/// stops at the cap rather than squeezing the column beside it.
fn frozen_room(frozen: &[ReplayColumn], widths: &[Pixels], cap: Pixels, col: ReplayColumn) -> Pixels {
    let held: Pixels = frozen.iter().filter(|other| **other != col).map(|other| widths[*other as usize]).sum();
    cap - held
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
/// hookup is unique per row/column; `player` is who the cell is about, which
/// is what a right-click records.
/// `leading` is drawn ahead of the text, such as a ship's nation flag.
#[allow(clippy::too_many_arguments)]
fn cell_element(
    ix: usize,
    player: AccountId,
    col: ReplayColumn,
    cell: CellValue,
    leading: Option<AnyElement>,
    width: f32,
    entity: &Entity<PlayerTable>,
) -> AnyElement {
    let id = ix * CELL_ID_STRIDE + col as usize;
    let base = div()
        .id(("replay-cell", id))
        .test_support()
        .on_mouse_down(MouseButton::Right, note_cell(entity, player, col))
        .w(px(width))
        .flex()
        .flex_none()
        .px_1()
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .when_some(cell.color, |el, role| el.text_color(resolve_color(role)))
        // Figures line up down a column; a ship name keeps the font's own.
        .when(col != ReplayColumn::ShipName, crate::ui::tabular);
    // As wide as the text, so the rest of the cell is the row's.
    let text = div()
        .id(("replay-cell-text-box", id))
        .test_support()
        .flex_shrink_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .on_click(keep_click)
        .child(crate::ui::selectable_text(("replay-cell-text", id), cell.text));
    let text = match cell.color {
        // A rating is the band's chip, as on every other surface that shows
        // one: the band's hue behind the band's own text colour. In a narrow
        // column the chip narrows with it and its text ends in an ellipsis.
        Some(ColorRole::PrTier(category)) => text
            .px_1()
            .rounded(px(3.))
            .bg(resolve_color(ColorRole::PrTierTint(category)).opacity(personal_rating::CHIP_TINT_ALPHA as f32 / 255.)),
        _ => text,
    };
    let base = base.children(leading).child(text);

    match cell.hover {
        Some(text) => base.tooltip(hover_tooltip(text.into())).into_any_element(),
        None => base.into_any_element(),
    }
}

/// Keeps a click on a control or a run of text from reaching the row, where
/// a double-click toggles the row's detail. A ctrl+click still reaches it to
/// select the row.
fn keep_click(event: &ClickEvent, _window: &mut Window, cx: &mut App) {
    if !event.modifiers().secondary(cx) {
        cx.stop_propagation();
    }
}

/// The expand/collapse caret shown at the start of the Name cell (the egui
/// app's `col_nr == 1` case in `cell_content_ui`, which prepends the same
/// caret to whichever column happens to sit at index 1 -- always Name, since
/// `default_columns` always puts Actions/Name/ShipName first in that order).
/// A click toggles `PlayerTable::expanded` for this row via the entity handle
/// (this closure runs inside the virtualized list's render callback, which
/// only has `&mut App`, not `Context<PlayerTable>`).
fn expand_caret(ix: usize, entity: Entity<PlayerTable>, turn: f32) -> AnyElement {
    div()
        .id(("replay-row-caret", ix))
        .test_support()
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
        .child(crate::motion::chevron(turn))
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
    // The stripe and class icon say the team; the name is marked only for abuse.
    let name_color = row.is_abuser.then(|| resolve_color(ColorRole::Player(PlayerColorKind::Abuser)));
    let icon_tint = class_icon_tint(player_color_kind(row));

    let container = h_flex()
        .id(("replay-cell", ix * CELL_ID_STRIDE + ReplayColumn::Name as usize))
        .test_support()
        .on_mouse_down(MouseButton::Right, note_cell(&layout.entity, row.db_id, ReplayColumn::Name))
        .w(px(width))
        .flex_none()
        .gap_1()
        .px_1()
        .pr(px(ROW_ACTIONS_WIDTH + 4.))
        .items_center()
        .overflow_hidden();
    let mut cell = h_flex().flex_1().min_w_0().gap_1().items_center().overflow_hidden();
    cell = cell.child(expand_caret(ix, layout.entity.clone(), layout.turn));

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
                        .on_click(keep_click)
                        .tooltip(move |window, cx| Tooltip::new(species_text.clone()).build(window, cx)),
                )
            }
        }
        None => cell.child(
            div()
                .id(("replay-species-box", ix))
                .flex_none()
                .text_xs()
                .on_click(keep_click)
                .child(crate::ui::selectable_text(("replay-species", ix), row.ship_species_text.clone())),
        ),
    };

    if let Some(div_label) = row.division_label.as_ref() {
        cell = cell.child(
            div()
                .id(("replay-division-box", ix))
                .flex_none()
                .on_click(keep_click)
                .child(crate::ui::selectable_text(("replay-division", ix), div_label.clone())),
        );
    }
    if let Some(clan) = row.clan_tag.as_ref() {
        cell = cell.child(
            div()
                .id(("replay-clan-box", ix))
                .flex_none()
                .text_color(clan_tag_color(row))
                .on_click(keep_click)
                .child(crate::ui::selectable_text(("replay-clan", ix), clan.clone())),
        );
    }
    // The name takes only its own width; the space after it is the row's.
    cell = cell
        .child(
            div()
                .id(("replay-name-box", ix))
                .min_w(px(0.))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .when_some(name_color, |el, color| el.text_color(color))
                .on_click(keep_click)
                .child(crate::ui::selectable_text(("replay-name", ix), row.display_name.clone())),
        )
        .child(div().flex_1());

    if row.is_hidden_profile {
        cell = cell.child(
            crate::icons::icon(crate::icons::Glyph::EyeOff)
                .flex_none()
                .id(("replay-hidden-profile", ix))
                .test_support()
                .on_click(keep_click)
                .tooltip(|window, cx| {
                    Tooltip::new(t!("ui.replay.player.hidden_profile").into_owned()).build(window, cx)
                }),
        );
    }

    if let Some(chip) = twitch_chip(ix, &row.twitch_candidates) {
        cell = cell.child(chip);
    }

    if let Some(note) = row.connection.as_ref() {
        cell = cell.child(connection_chip(ix, note, layout));
    }

    // The dots sit at the end of the player's name, between it and the ship:
    // they are that player's actions, and the name is what says which player.
    // Inside the cell, they stay on its line when the row opens below it.
    let actions = row_actions(
        ix,
        Rc::new(ActionsMenuData::from_row(row, layout.icons)),
        layout,
        SharedString::from(format!("replay-row-{ix}")),
    );
    container.relative().child(cell).child(actions).into_any_element()
}

/// What a connection history says, in the egui app's own wording.
fn connection_hover_text(note: &ConnectionNote) -> String {
    match note {
        ConnectionNote::NeverConnected => t!("ui.replay.player.never_connected").into_owned(),
        ConnectionNote::Interrupted { events, .. } => format!("Player {}", events.join(", ")),
    }
}

/// How long the player was offline while alive, when the replay says.
fn time_offline(note: &ConnectionNote) -> Option<TimeOffline> {
    match note {
        ConnectionNote::NeverConnected => None,
        ConnectionNote::Interrupted { time_offline, .. } => *time_offline,
    }
}

/// `time` as minutes and seconds, "1m 12s", or seconds alone under a minute.
fn offline_label(time: TimeOffline) -> String {
    let secs = time.duration().as_secs();
    match (secs / 60, secs % 60) {
        (0, secs) => format!("{secs}s"),
        (mins, secs) => format!("{mins}m {secs}s"),
    }
}

/// How deep a disconnect chip is coloured, from 0 to 1: nothing for a drop
/// under half a minute, rising evenly to full at a minute and a half.
fn offline_depth(time: TimeOffline) -> f32 {
    const NEUTRAL_UNDER_SECS: f32 = 30.;
    const FULL_AT_SECS: f32 = 90.;

    let secs = time.duration().as_secs_f32();
    ((secs - NEUTRAL_UNDER_SECS) / (FULL_AT_SECS - NEUTRAL_UNDER_SECS)).clamp(0., 1.)
}

/// The connection marker: the plug and, when known, how long the player was
/// offline, on a ground that reddens with that time. A click copies what
/// its tooltip says.
fn connection_chip(ix: usize, note: &ConnectionNote, layout: &RowLayout) -> AnyElement {
    let hover: SharedString = connection_hover_text(note).into();
    let copied = hover.clone();
    let time = time_offline(note);

    let chip = h_flex()
        .id(("replay-disconnect", ix))
        .test_support()
        .flex_none()
        .gap_0p5()
        .px_0p5()
        .rounded(px(3.))
        .cursor_pointer()
        .child(crate::icons::icon(crate::icons::Glyph::Plug))
        .on_click(move |event, window, cx| {
            keep_click(event, window, cx);
            cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()));
            crate::toast::ok(t!("ui.replay.context.copied").into_owned(), window, cx);
        })
        .tooltip(move |window, cx| Tooltip::new(hover.clone()).build(window, cx));
    let chip = match time {
        Some(time) => {
            // Opaque over the row, so the text colour can be chosen against it.
            let error: Hsla = rgb(crate::theme::semantic().error).into();
            let ground =
                layout.fade_to.blend(layout.foreground.opacity(0.08)).blend(error.opacity(offline_depth(time)));
            chip.bg(ground).text_color(crate::theme::text_on(ground)).child(div().text_xs().child(offline_label(time)))
        }
        None => chip,
    };

    chip.into_any_element()
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

    let glyph = crate::icons::icon(crate::icons::Glyph::Twitch)
        .id(("replay-twitch", ix))
        .test_support()
        .cursor_pointer()
        .tooltip(move |window, cx| Tooltip::new(hover.clone()).build(window, cx));

    if candidates.len() == 1 {
        let login = first.login.clone();
        return Some(
            glyph
                .on_click(move |event, window, cx| {
                    keep_click(event, window, cx);
                    cx.write_to_clipboard(ClipboardItem::new_string(login.clone()));
                    crate::toast::ok(t!("ui.twitch.copied", name = login).into_owned(), window, cx);
                })
                .into_any_element(),
        );
    }

    let logins: Vec<String> = candidates.iter().map(|candidate| candidate.login.clone()).collect();
    let menu = Button::new(("replay-twitch-menu", ix)).ghost().xsmall().child(glyph).dropdown_menu(
        move |mut menu, _window, _cx| {
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
        },
    );

    Some(div().id(("replay-twitch-menu-box", ix)).flex_none().on_click(keep_click).child(menu).into_any_element())
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
fn skills_cell(ix: usize, row: &PlayerRow, debug: bool, width: f32, entity: &Entity<PlayerTable>) -> AnyElement {
    let cell = cell_value(row, ReplayColumn::Skills, debug);
    let shows_real_label = row.has_vehicle_entity && (!row.relation.is_enemy() || debug);
    if !shows_real_label {
        return cell_element(ix, row.db_id, ReplayColumn::Skills, cell, None, width, entity);
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
            SkillWarning::TowerDefense => crate::icons::Glyph::Castle,
            SkillWarning::NoHighTier => crate::icons::Glyph::TriangleAlert,
        };
        markers = markers.child(crate::icons::icon(glyph).text_color(color));
        has_markers = true;
    }

    let base = div()
        .id(("replay-cell", ix * CELL_ID_STRIDE + ReplayColumn::Skills as usize))
        .test_support()
        .on_mouse_down(MouseButton::Right, note_cell(entity, row.db_id, ReplayColumn::Skills))
        .w(px(width))
        .flex_none()
        .px_1()
        .child(
            h_flex().gap_1().items_center().overflow_hidden().when(has_markers, |el| el.child(markers)).child(
                div()
                    .id(("replay-cell-text-box", ix * CELL_ID_STRIDE + ReplayColumn::Skills as usize))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(color)
                    .on_click(keep_click)
                    .child(crate::ui::selectable_text(
                        ("replay-cell-text", ix * CELL_ID_STRIDE + ReplayColumn::Skills as usize),
                        cell.text.clone(),
                    )),
            ),
        );

    match cell.hover {
        Some(text) => base.tooltip(hover_tooltip(text.into())).into_any_element(),
        None => base.into_any_element(),
    }
}

/// The handful of `PlayerRow` fields `build_actions_menu` reads, cloned out in
/// `render_row` instead of the whole row: `PlayerRow` also carries
/// achievements/ribbons/consumables/build/damage-interaction data (and
/// `raw_metadata_json`, a full pretty-printed JSON dump) that the menu never
/// touches, and a row that is on screen builds this on every render.
///
/// One per row, shared by the dots and the row's right-click menu through an
/// `Rc`, since both offer the same items for the same player.
struct ActionsMenuData {
    /// Whose row this is, so the menu can copy it whatever the table is sorted
    /// into since.
    player: AccountId,
    /// Who the menu is about, drawn as the row reads.
    heading: MenuHeading,
    relation: Relation,
    has_vehicle_entity: bool,
    ship_config_url: Option<String>,
    short_ship_config_url: Option<String>,
    shiptool_url: Option<String>,
    wows_numbers_url: Option<String>,
    raw_metadata_json: Option<String>,
}

/// Who a row's menu is about, drawn as the menu's first item.
///
/// A menu opened by a right-click carries no other sign of which row it came
/// from, and the rows are one line apart. One line, in the row's own order:
/// clan tag, player, class icon, ship.
#[derive(Clone)]
struct MenuHeading {
    /// The class icon as the Name cell draws it, tinted to the player. `None`
    /// until `IconCache` has the asset.
    icon: Option<Arc<RenderImage>>,
    /// Stands in for an icon the cache does not have, as in the Name cell, so
    /// it is read only when `icon` is `None`.
    species: Option<SharedString>,
    clan: Option<SharedString>,
    clan_color: Hsla,
    name: SharedString,
    name_color: Hsla,
    /// `None` for a row whose ship the recording never saw.
    ship: Option<SharedString>,
    /// The heading as one run of text, for a screen reader.
    spoken: SharedString,
}

fn heading_for(row: &PlayerRow, icons: &IconCache) -> MenuHeading {
    let clan: Option<SharedString> = row.clan_tag.as_deref().filter(|clan| !clan.is_empty()).map(SharedString::from);
    let ship: Option<SharedString> = (!row.ship_name.is_empty()).then(|| row.ship_name.clone().into());
    let icon = icons.get(row.ship_class, class_icon_tint(player_color_kind(row)));
    let species: Option<SharedString> = icon
        .is_none()
        .then(|| row.ship_species_text.clone())
        .filter(|species| !species.is_empty())
        .map(SharedString::from);

    let mut spoken = String::new();
    if let Some(clan) = clan.as_ref() {
        spoken.push_str(clan);
        spoken.push(' ');
    }
    spoken.push_str(&row.display_name);
    if let Some(ship) = ship.as_ref() {
        spoken.push_str(", ");
        spoken.push_str(ship);
    }

    MenuHeading {
        icon,
        species,
        clan,
        clan_color: clan_tag_color(row),
        name: row.display_name.clone().into(),
        name_color: resolve_color(ColorRole::Player(name_color_kind(row))),
        ship,
        spoken: spoken.into(),
    }
}

/// The game's clan-league color, else the team relation's color for a replay
/// that carries no clan color, as the egui table does.
fn clan_tag_color(row: &PlayerRow) -> Hsla {
    let rgb = match row.clan_color_rgb {
        // The game's colours are pale tones made for its dark client.
        Some(rgb) => crate::theme::readable_on_rows(rgb),
        None => relation_color_rgb(row.relation),
    };
    resolve_color(ColorRole::Fixed(rgb))
}

/// The heading as the menu's first item: the clan tag and the player in the
/// colours the row gives them, then the ship behind its class icon.
fn menu_heading_element(heading: &MenuHeading) -> AnyElement {
    let mut line = h_flex().gap_1().items_center().whitespace_nowrap();

    if let Some(clan) = heading.clan.clone() {
        line = line.child(div().flex_none().text_color(heading.clan_color).child(clan));
    }
    line = line.child(
        div()
            .id("replay-actions-heading-name")
            .test_support()
            .overflow_hidden()
            .text_ellipsis()
            .text_color(heading.name_color)
            .child(heading.name.clone()),
    );

    if let Some(ship) = heading.ship.clone() {
        if let Some(image) = heading.icon.clone() {
            line = line.child(div().flex_none().child(img(image).w(px(SHIP_ICON_WIDTH)).h(px(SHIP_ICON_WIDTH))));
        } else if let Some(species) = heading.species.clone() {
            line = line.child(div().flex_none().text_xs().child(species));
        }
        line = line.child(
            div()
                .id("replay-actions-heading-ship")
                .test_support()
                .overflow_hidden()
                .text_ellipsis()
                .text_color(crate::theme::text_dim())
                .child(ship),
        );
    }

    line.id("replay-actions-heading").test_support().aria_label(heading.spoken.clone()).into_any_element()
}

impl ActionsMenuData {
    fn from_row(row: &PlayerRow, icons: &IconCache) -> Self {
        Self {
            player: row.db_id,
            heading: heading_for(row, icons),
            relation: row.relation,
            has_vehicle_entity: row.has_vehicle_entity,
            ship_config_url: row.ship_config_url.clone(),
            short_ship_config_url: row.short_ship_config_url.clone(),
            shiptool_url: row.shiptool_url.clone(),
            wows_numbers_url: row.wows_numbers_url.clone(),
            raw_metadata_json: row.raw_metadata_json.clone(),
        }
    }
}

/// Where a menu was opened from, which decides whether it can offer to copy
/// one cell: the dots stand at the end of the row rather than over a cell.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuOrigin {
    RowRightClick,
    RowDots,
}

/// Builds a row's actions menu, mirroring the egui app's
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
/// items write to the clipboard via `cx.write_to_clipboard` and toast
/// `ui.replay.build.link_copied` as the egui app does, matching
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
    origin: MenuOrigin,
    cx: &mut App,
) -> PopupMenu {
    // Whose options these are, first: a right-click menu lands wherever the
    // pointer was, and the rows it could have come from are one line apart.
    // Disabled because it is a heading and not one of the options; the kit
    // draws the item's own text muted then, which the coloured name overrides.
    let heading = row.heading.clone();
    menu = menu
        .item(PopupMenuItem::element(move |_window, _cx| menu_heading_element(&heading)).disabled(true))
        .separator();

    let show_ship_config = (!row.relation.is_enemy() || debug) && row.has_vehicle_entity;
    // Whether anything stands between the heading's separator and the copy
    // block's, so the two never land back to back.
    let mut wrote_links = false;

    if show_ship_config {
        let mut added_any = false;
        if let Some(url) = row.shiptool_url.clone() {
            menu = menu.item(PopupMenuItem::link(t!("ui.replay.build.open_shiptool").into_owned(), url));
            added_any = true;
        }
        if let Some(url) = row.ship_config_url.clone() {
            menu = menu.item(PopupMenuItem::link(t!("ui.replay.build.open_in_browser").into_owned(), url.clone()));
            let copy_url = url;
            menu = menu.item(
                PopupMenuItem::new(t!("ui.replay.build.copy_link").into_owned()).icon(IconName::Copy).on_click(
                    move |_event, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                        crate::toast::ok(t!("ui.replay.build.link_copied").into_owned(), window, cx);
                    },
                ),
            );
            added_any = true;
        }
        if let Some(url) = row.short_ship_config_url.clone() {
            menu = menu.item(
                PopupMenuItem::new(t!("ui.replay.build.copy_short_link").into_owned()).icon(IconName::Copy).on_click(
                    move |_event, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                        crate::toast::ok(t!("ui.replay.build.link_copied").into_owned(), window, cx);
                    },
                ),
            );
            added_any = true;
        }
        if added_any {
            menu = menu.separator();
        }
        wrote_links |= added_any;
    }

    if let Some(url) = row.wows_numbers_url.clone() {
        menu = menu.item(PopupMenuItem::link(t!("ui.replay.build.open_wows_numbers").into_owned(), url));
        wrote_links = true;
    }

    // What the row says, for a reader taking it somewhere else, beside the
    // cells' own selectable text.
    //
    // The pressed cell is consumed whether or not it is used, so a press that
    // landed on the row but not on a cell -- the blank end of the row, the
    // dots -- offers nothing rather than the cell pressed before it.
    let pressed = match origin {
        MenuOrigin::RowRightClick => entity.update(cx, |this, _cx| this.take_right_clicked_cell()),
        MenuOrigin::RowDots => None,
    };
    if wrote_links {
        menu = menu.separator();
    }
    if let Some((player, col)) = pressed.filter(|(player, _)| *player == row.player) {
        let table = entity.clone();
        menu = menu.item(
            PopupMenuItem::new(t!("ui.replay.context.copy_cell").into_owned())
                .icon(IconName::Copy)
                .on_click(move |_event, window, cx| copy_text(table.read(cx).cell_text(player, col), window, cx)),
        );
    }
    let table = entity.clone();
    let player = row.player;
    menu = menu.item(
        PopupMenuItem::new(t!("ui.replay.context.copy_row").into_owned())
            .icon(IconName::Copy)
            .on_click(move |_event, window, cx| copy_text(table.read(cx).row_text(player), window, cx)),
    );
    let table = entity.clone();
    menu = menu.item(
        PopupMenuItem::new(t!("ui.replay.context.copy_table").into_owned())
            .icon(IconName::Copy)
            .on_click(move |_event, window, cx| copy_text(Some(table.read(cx).table_text()), window, cx)),
    );

    let table = entity.clone();
    menu = menu.item(
        PopupMenuItem::new("Copy row as Markdown")
            .icon(IconName::Copy)
            .on_click(move |_event, window, cx| copy_text(table.read(cx).row_markdown(player), window, cx)),
    );
    let table = entity.clone();
    menu = menu.item(
        PopupMenuItem::new("Copy table as Markdown")
            .icon(IconName::Copy)
            .on_click(move |_event, window, cx| copy_text(Some(table.read(cx).table_markdown()), window, cx)),
    );

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

fn markdown_cell(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.replace("\r\n", "\n").chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\n' | '\r' => escaped.push_str("<br>"),
            '\\' | '|' | '`' | '*' | '_' | '[' | ']' | '~' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Writes `text` to the clipboard and says so.
///
/// `None` means the player the menu was opened on is no longer in the table,
/// which happens only if the replay is reloaded while the menu is open; there
/// is nothing to say about it that the emptied table does not already say.
fn copy_text(text: Option<String>, window: &mut Window, cx: &mut App) {
    let Some(text) = text else { return };
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    crate::toast::ok(t!("ui.replay.context.copied").into_owned(), window, cx);
}

/// Records which cell a right-click landed on, so the row's menu knows which
/// one to copy. The press is not consumed: the menu belongs to the row.
fn note_cell(
    entity: &Entity<PlayerTable>,
    player: AccountId,
    col: ReplayColumn,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static + use<> {
    let entity = entity.clone();
    move |_event, _window, cx| entity.update(cx, |this, _cx| this.note_right_clicked_cell(player, col))
}

/// The dots that open one row's actions, revealed while the pointer is on that
/// row.
///
/// The Name cell reserves this space even when the trigger is hidden, so
/// status icons and text do not move or overlap the button on hover.
///
/// `ix` keys the trigger's `ElementId` so every row's popover state is its own,
/// and `group` ties the reveal to that row's hover. `layout.entity` is threaded
/// through to `build_actions_menu` for the raw-metadata item's event.
fn row_actions(ix: usize, row: Rc<ActionsMenuData>, layout: &RowLayout, group: SharedString) -> AnyElement {
    let debug = layout.debug;
    let entity = layout.entity.clone();
    let trigger = Button::new(("replay-row-actions", ix))
        .ghost()
        .xsmall()
        .icon(IconName::Ellipsis)
        .tooltip(t!("ui.replay.row_actions_hint").to_string());
    let menu_button = trigger.dropdown_menu(move |menu, _window, cx| {
        build_actions_menu(menu, &row, debug, entity.clone(), MenuOrigin::RowDots, cx)
    });

    div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(px(ROW_ACTIONS_WIDTH))
        .flex()
        .items_center()
        .justify_center()
        .invisible()
        .group_hover(group, |this| this.visible())
        .child(div().id(("replay-row-actions-box", ix)).on_click(keep_click).child(menu_button))
        .into_any_element()
}

/// Room for the dots at the end of the name.
const ROW_ACTIONS_WIDTH: f32 = 24.0;

/// How wide the fade at a cut-off detail's right edge is.
const CUT_OFF_FADE_WIDTH: f32 = 28.0;

/// One column's collapsed cell, dispatching to the Name/Skills special-cased
/// layouts and falling back to the generic `cell_element` for everything else.
fn render_cell(ix: usize, col: ReplayColumn, row: &PlayerRow, layout: &RowLayout) -> AnyElement {
    let width = layout.column_widths[col as usize].as_f32();
    match col {
        ReplayColumn::Name => name_cell(ix, row, layout, width),
        ReplayColumn::Skills => skills_cell(ix, row, layout.debug, width, &layout.entity),
        ReplayColumn::ActualDamage | ReplayColumn::ReceivedDamage => damage_cell(ix, col, row, layout, width),
        ReplayColumn::ShipName => ship_name_cell(ix, row, layout, width),
        _ => number_cell(ix, col, row, layout, width),
    }
}

/// The ship's name after its nation's flag. No flag, and no room held for
/// one, where the nation or its flag is unknown.
fn ship_name_cell(ix: usize, row: &PlayerRow, layout: &RowLayout, width: f32) -> AnyElement {
    let flag =
        row.ship_nation.as_deref().and_then(|nation| layout.icons.get_keyed(&nation_flag_key(nation))).map(|image| {
            div()
                .id(("replay-ship-flag", ix))
                .test_support()
                .flex_none()
                .mr_1()
                .flex()
                .items_center()
                .child(img(image).w(rems(SHIP_FLAG_WIDTH_REMS)).h(rems(SHIP_FLAG_HEIGHT_REMS)))
                .into_any_element()
        });
    let cell = cell_value(row, ReplayColumn::ShipName, layout.debug);

    cell_element(ix, row.db_id, ReplayColumn::ShipName, cell, flag, width, &layout.entity)
}

/// A nation flag's size beside a ship name: the height of the text, at the
/// flags' own aspect, the same for every row so the names line up.
const SHIP_FLAG_WIDTH_REMS: f32 = 23. / 16.;
const SHIP_FLAG_HEIGHT_REMS: f32 = 1.;

/// The generic `cell_element`, over a faint bar of the row's share of its
/// team's total where the column has one. The hover states the share.
fn number_cell(ix: usize, col: ReplayColumn, row: &PlayerRow, layout: &RowLayout, width: f32) -> AnyElement {
    let share = layout.team_totals.share(row, col, layout.debug);
    let mut cell = cell_value(row, col, layout.debug);
    if let Some(share) = share {
        let line = share.text();
        cell.hover = Some(match cell.hover.take() {
            Some(hover) => format!("{hover}\n{line}"),
            None => line,
        });
    }

    share_bar_cell(ix, col, row, cell, share, None, layout, width)
}

/// `cell_element` for `cell` over a faint bar as long as the row's `share`
/// of its team's total, with `mix`, when given, as a band of damage types
/// under the figure spanning the whole cell: the share says how much of the
/// team's damage, the band what this player's own damage was made of.
#[allow(clippy::too_many_arguments)]
fn share_bar_cell(
    ix: usize,
    col: ReplayColumn,
    row: &PlayerRow,
    cell: CellValue,
    share: Option<TeamShare>,
    mix: Option<&DamageMix>,
    layout: &RowLayout,
    width: f32,
) -> AnyElement {
    let color = cell.color.map_or(layout.foreground, resolve_color);
    let element = cell_element(ix, row.db_id, col, cell, None, width, &layout.entity);
    if share.is_none() && mix.is_none() {
        return element;
    }

    div()
        .relative()
        .w(px(width))
        .flex_none()
        .when_some(share, |cell, share| {
            cell.child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(relative(share.fraction))
                    .bg(color.opacity(MAGNITUDE_BAR_OPACITY)),
            )
        })
        .child(element)
        .when_some(mix, |cell, mix| cell.child(div().px_1().child(damage_mix::stacked_bar(mix))))
        .into_any_element()
}

/// Strength of a magnitude bar against the text colour it is drawn in.
const MAGNITUDE_BAR_OPACITY: f32 = 0.13;

/// The figure a column draws a share bar for, when the cell shows it. A
/// hidden figure gets no bar and is left out of its team's total, as it is
/// from the team heading.
fn bar_figure(row: &PlayerRow, col: ReplayColumn, debug: bool) -> Option<u64> {
    let hidden = row.should_hide_stats() && !debug;
    match col {
        ReplayColumn::ActualDamage if !hidden => row.actual_damage,
        ReplayColumn::ReceivedDamage if !hidden => row.received_damage,
        ReplayColumn::PotentialDamage if !hidden => row.potential_damage,
        // Shown on every row: it is damage allies dealt on this player's spotting.
        ReplayColumn::SpottingDamage => row.spotting_damage,
        _ => None,
    }
}

/// A row's figure as part of its own team's total in that column.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TeamShare {
    side: TeamSide,
    /// From 0 to 1.
    fraction: f32,
}

impl TeamShare {
    fn text(self) -> String {
        let pct = format!("{:.0}%", self.fraction * 100.);
        t!("ui.replay.team_share", pct = pct, team = self.side.label()).into_owned()
    }
}

/// Each barred column's figure summed over each team, indexed by
/// `ReplayColumn as usize`. The team heading shows the Actual Damage entry,
/// so a bar and its heading count the same rows.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TeamTotals {
    self_team: TeamId,
    columns: Vec<TeamDamage>,
}

impl TeamTotals {
    fn of(rows: &[PlayerRow], self_team: TeamId, debug: bool) -> Self {
        let columns = ReplayColumn::ALL
            .into_iter()
            .map(|col| TeamDamage::summing(rows, self_team, |row| bar_figure(row, col, debug)))
            .collect();

        Self { self_team, columns }
    }

    fn column(&self, col: ReplayColumn) -> TeamDamage {
        self.columns[col as usize]
    }

    /// `row`'s figure in `col` against its team's total. `None` where there
    /// is no bar to draw.
    fn share(&self, row: &PlayerRow, col: ReplayColumn, debug: bool) -> Option<TeamShare> {
        let figure = bar_figure(row, col, debug)?;
        let side = TeamSide::of(row, self.self_team);
        let total = self.column(col).side(side)?;

        (figure > 0 && total > 0).then(|| TeamShare { side, fraction: (figure as f64 / total as f64) as f32 })
    }
}

/// A damage cell over its team-share bar, with the player's damage split by
/// type in a band under the figure and listed with its colour key on hover.
/// The plain number cell when there is no split to show.
fn damage_cell(ix: usize, col: ReplayColumn, row: &PlayerRow, layout: &RowLayout, width: f32) -> AnyElement {
    let Some(lines) = wows_toolkit_replay_data::damage_mix::shown_lines(row, col, layout.debug) else {
        return number_cell(ix, col, row, layout, width);
    };
    let Some(mix) = DamageMix::of(lines) else {
        return number_cell(ix, col, row, layout, width);
    };

    // The share is stated under the split, on the one tooltip the cell has.
    let share = layout.team_totals.share(row, col, layout.debug);
    let cell = share_bar_cell(ix, col, row, cell_value(row, col, layout.debug), share, Some(&mix), layout, width);
    let mix = Rc::new(mix);
    let lines: Rc<[DamageLine]> = lines.into();
    let share_text = share.map(|share| SharedString::from(share.text()));
    div()
        .id(("replay-damage-mix", ix * CELL_ID_STRIDE + col as usize))
        .test_support()
        .w(px(width))
        .flex_none()
        .child(cell)
        .tooltip(move |window, cx| {
            let mix = Rc::clone(&mix);
            let lines = Rc::clone(&lines);
            let share_text = share_text.clone();
            Tooltip::element(move |_window, _cx| {
                v_flex()
                    .gap_1()
                    .child(damage_mix::listing(&mix, &lines))
                    .children(share_text.clone().map(|text| div().text_xs().child(text)))
            })
            .build(window, cx)
        })
        .into_any_element()
}

/// One column's part of a shown row detail, under its own header
/// (`expanded::render_column_detail`). Fixed to the column's width so the
/// detail (text wraps to the width; a dense build/consumable sub-table clips
/// at its edge) never bleeds into the next column, matching egui_table's
/// per-cell clip with `AutoSizeMode::Never`. A column without detail keeps
/// its place with an empty cell.
fn render_column_detail(ix: usize, col: ReplayColumn, row: &PlayerRow, layout: &RowLayout, opacity: f32) -> AnyElement {
    let cell = div()
        .id(("replay-expanded-cell", ix * CELL_ID_STRIDE + col as usize))
        .test_support()
        .on_mouse_down(MouseButton::Right, note_cell(&layout.entity, row.db_id, col))
        .w(layout.column_widths[col as usize])
        .flex_none()
        .overflow_hidden();
    let Some(detail) =
        expanded::render_column_detail(ix, col, row, layout.all_rows, layout.icons, layout.debug, layout.alt_held)
    else {
        return cell.into_any_element();
    };

    // A column drawn narrower than the detail needs clips it; the fade says
    // there is more to the right than shows.
    let cut_off =
        layout.detail_widths.get(&(row.db_id, col)).is_some_and(|needs| *needs > layout.column_widths[col as usize]);
    // Only the content fades in: a translucent ground would show the cut-off
    // fade, painted over it, as a darker edge.
    let content = div().opacity(opacity).child(detail).into_any_element();

    cell.child(detail_surface(content, cut_off, ix, layout)).into_any_element()
}

/// How an expanded row's detail is set apart from the row it belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailStyle {
    /// Drawn straight on the row.
    Flat,
    /// Sunk into the row: a shaded ground, a shadow along its top edge and a
    /// highlight along its bottom.
    Well,
}

const DETAIL_STYLE: DetailStyle = DetailStyle::Well;

/// The room between neighbouring wells and their borders' width, either side.
const DETAIL_GAP: f32 = 2.;
const DETAIL_BORDER: f32 = 1.;

/// What a well adds to its content's width: a gap and a border on each side.
const DETAIL_FRAME: f32 = match DETAIL_STYLE {
    DetailStyle::Flat => 0.,
    DetailStyle::Well => 2. * (DETAIL_GAP + DETAIL_BORDER),
};

/// A column's detail width as drawn, frame included; 0 for a column with no
/// detail for `row`.
fn framed_detail_width(
    col: ReplayColumn,
    row: &PlayerRow,
    rows: &[PlayerRow],
    debug: bool,
    window: &mut Window,
) -> f32 {
    let detail_w = expanded::expanded_detail_width(col, row, rows, debug, window);
    if detail_w > 0. { detail_w + DETAIL_FRAME } else { 0. }
}

/// One column's detail on the surface `DETAIL_STYLE` names, with the fade
/// at its right edge when the column is `cut_off`.
fn detail_surface(content: AnyElement, cut_off: bool, ix: usize, layout: &RowLayout) -> Div {
    // Black and white at low strength rather than palette colours: they shade
    // whatever the row is painted, stripe, selection or hover alike.
    let dark = crate::theme::is_dark_mode();
    let shade = hsla(0., 0., 0., if dark { 0.18 } else { 0.05 });
    let shadow = hsla(0., 0., 0., if dark { 0.45 } else { 0.14 });
    let highlight = hsla(0., 0., 1., if dark { 0.07 } else { 0.7 });

    let (ground, hovered_ground) = match DETAIL_STYLE {
        DetailStyle::Flat => (layout.fade_to, layout.fade_to_hovered),
        DetailStyle::Well => (layout.fade_to.blend(shade), layout.fade_to_hovered.blend(shade)),
    };
    let group = SharedString::from(format!("replay-row-{ix}"));
    let fade = |to: Hsla| linear_gradient(90., linear_color_stop(to.opacity(0.), 0.), linear_color_stop(to, 1.));
    let fade = div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(px(CUT_OFF_FADE_WIDTH))
        .bg(fade(ground))
        // The row repaints in its hover colour under the pointer.
        .group_hover(group.clone(), move |style| style.bg(fade(hovered_ground)));
    let line = |color: Hsla| div().absolute().left_0().right_0().h(px(1.)).bg(color);

    // Padded like the cells, which also keeps it clear of the team stripe.
    let surface = div().relative().px_1().child(content).when(cut_off, |surface| surface.child(fade));
    let surface = match DETAIL_STYLE {
        DetailStyle::Flat => surface,
        // Bordered, so a column's detail reads apart from its neighbour's
        // when both sit on the same shade.
        DetailStyle::Well => surface
            .py_1()
            .border(px(DETAIL_BORDER))
            .border_color(crate::theme::border_bright())
            .bg(ground)
            .group_hover(group, move |style| style.bg(hovered_ground))
            .child(line(shadow).top_0())
            .child(line(shadow.opacity(0.5)).top(px(1.)))
            .child(line(highlight).bottom_0()),
    };

    // The gap above the detail is part of what is revealed, so the row grows
    // from its first line in step with the progress.
    v_flex().pt_1().px(px(if DETAIL_STYLE == DetailStyle::Well { DETAIL_GAP } else { 0. })).child(surface)
}

/// How much of a row's detail is drawn.
#[derive(Clone, Copy)]
enum RowDetail {
    Hidden,
    /// Opening or closing, at this progress from 0 closed to 1 open.
    Moving(f32),
    Open,
}

impl RowDetail {
    /// Reads a row's reveal: settled, it is where `expanded` says.
    fn from_reveal(reveal: MotionValue<f32>, expanded: bool) -> Self {
        match reveal.status {
            MotionStatus::Delayed | MotionStatus::Running => Self::Moving(reveal.value),
            MotionStatus::Idle | MotionStatus::Finished if expanded => Self::Open,
            MotionStatus::Idle | MotionStatus::Finished => Self::Hidden,
        }
    }

    fn is_shown(self) -> bool {
        !matches!(self, Self::Hidden)
    }
}

/// Per-frame layout shared by every row and the header's scrolling portion:
/// the sticky/scrolling column split, the scrolling section's total width,
/// the content-fit column widths (`PlayerTable::column_widths`, indexed by
/// `ReplayColumn as usize`), the shared horizontal scroll handle, the icon
/// cache, the debug flag, how much of this row's detail is drawn, the
/// entity handle (for the caret/double-click toggle), and the full row list
/// (for the expanded damage-interaction breakdowns, which look up other rows
/// by `db_id`).
/// Bundled into one struct so `render_row` stays under clippy's
/// argument-count limit.
struct RowLayout<'a> {
    /// The widest the frozen section may be drawn, so the dots can sit at its
    /// visible end rather than at an edge the clip has moved. `None` on the
    /// first frame, before the table has been measured.
    sticky_cap: Option<f32>,
    sticky_columns: &'a [ReplayColumn],
    scroll_columns: &'a [ReplayColumn],
    scroll_width: f32,
    column_widths: &'a [Pixels],
    /// What each shown row's detail needs with its padding, against
    /// `column_widths`.
    detail_widths: &'a HashMap<(AccountId, ReplayColumn), Pixels>,
    /// The colour a cut-off detail fades into at the column's edge.
    fade_to: Hsla,
    /// The same, while the pointer is over the row.
    fade_to_hovered: Hsla,
    /// The text colour of a cell the model gives no colour.
    foreground: Hsla,
    /// Each barred column's team totals, for the bars behind the numbers.
    team_totals: &'a TeamTotals,
    icons: &'a IconCache,
    debug: bool,
    h_scroll: &'a ScrollHandle,
    entity: Entity<PlayerTable>,
    detail: RowDetail,
    /// How far the row's disclosure has turned.
    turn: f32,
    selected: bool,
    /// Which side the row's player is on, for the stripe in its gutter.
    mark: RowMark,
    alt_held: bool,
    /// How far each column is drawn from its place while the columns slide.
    column_shift: &'a [Pixels],
    /// The column being moved by its header, whose cells mark its slot.
    moving: Option<ReplayColumn>,
    /// Room held for the pinned zone while a column is carried and nothing
    /// is pinned.
    pinned_reserve: Pixels,
    all_rows: &'a [PlayerRow],
}

/// A cell at its column's place: offset while the columns slide to new
/// places, and faint while its column is the one being moved.
fn placed_cell(cell: AnyElement, col: ReplayColumn, layout: &RowLayout) -> AnyElement {
    let shift = layout.column_shift[col as usize];
    let moving = layout.moving == Some(col);
    if shift == px(0.) && !moving {
        return cell;
    }

    div()
        .flex_none()
        .relative()
        .left(shift)
        .when(moving, |el| el.opacity(DRAGGED_OPACITY))
        .child(cell)
        .into_any_element()
}

/// One row: `layout.sticky_columns` (Actions/Name/ShipName) render as fixed
/// per-column cells outside the horizontal scroll; `layout.scroll_columns`
/// render inside a nested scroll container tracking `layout.h_scroll`, the
/// same handle the header's scrolling portion tracks, so both stay aligned.
/// Mirrors the egui app's `num_sticky_cols(3)`.
///
/// The first line is laid out the same whether or not the row's detail is
/// shown, so nothing on it moves when the row opens. The detail is a second
/// line of the same columns, each column's detail under its own header
/// (achievements under Name, the build under Skills, the damage breakdowns
/// under ActualDamage/ReceivedDamage). A double-click on the first line
/// toggles the detail, mirroring the egui app's whole-row double-click
/// handler in `cell_content_ui`.
fn render_row(ix: usize, row: &PlayerRow, layout: &RowLayout, cx: &App) -> AnyElement {
    let sticky_section =
        || h_flex().min_w(layout.pinned_reserve).max_w(relative(STICKY_MAX_FRACTION)).overflow_hidden();
    let scrolling_section = || h_flex().w(px(layout.scroll_width)).flex_none();
    let scroller = |id: (&'static str, usize), line: Div| {
        div()
            .id(id)
            .flex_1()
            .min_w(px(0.))
            .overflow_x_scroll()
            // A vertical wheel belongs to the list around the row, not to
            // the columns scrolling sideways inside it.
            .restrict_scroll_to_axis()
            .track_scroll(layout.h_scroll)
            .child(line)
    };

    // A line of cells reads across, so its cells are centred on it.
    let mut sticky = sticky_section().items_center();
    for &col in layout.sticky_columns {
        sticky = sticky.child(placed_cell(render_cell(ix, col, row, layout), col, layout));
    }
    let mut scrolling = scrolling_section().items_center();
    for &col in layout.scroll_columns {
        scrolling = scrolling.child(placed_cell(render_cell(ix, col, row, layout), col, layout));
    }
    let entity = layout.entity.clone();
    let first_line = h_flex()
        .id(("replay-row-line", ix))
        .test_support()
        .w_full()
        .items_center()
        .on_click(move |event: &ClickEvent, _window, cx: &mut App| {
            if event.click_count() >= 2 && !event.modifiers().secondary(cx) {
                entity.update(cx, |this, cx| this.toggle_expanded(ix, cx));
            }
        })
        .child(sticky)
        .child(scroller(("replay-row-h-scroll", ix), scrolling));

    let shown = match layout.detail {
        RowDetail::Hidden => None,
        RowDetail::Moving(progress) => Some((crate::motion::Extent::Moving(progress), progress)),
        RowDetail::Open => Some((crate::motion::Extent::Open, 1.0)),
    };
    let detail_line = shown.map(|(extent, opacity)| {
        // Each column's detail starts at the same top edge.
        let mut sticky = sticky_section().items_start();
        for &col in layout.sticky_columns {
            sticky = sticky.child(placed_cell(render_column_detail(ix, col, row, layout, opacity), col, layout));
        }
        let mut scrolling = scrolling_section().items_start();
        for &col in layout.scroll_columns {
            scrolling = scrolling.child(placed_cell(render_column_detail(ix, col, row, layout, opacity), col, layout));
        }
        let line = h_flex()
            .w_full()
            .items_start()
            .child(sticky)
            .child(scroller(("replay-row-detail-h-scroll", ix), scrolling));
        // Kept in a reveal while open too, so a close starts from the height
        // the detail had rather than from nothing.
        crate::motion::Reveal::new(
            ("replay-expanded-reveal", row.db_id.raw().cast_unsigned()),
            extent,
            line.into_any_element(),
        )
    });

    // Selection is what a ctrl+click leaves behind, over the stripe; the
    // egui table paints the same two (`ui/replay_parser/mod.rs:3011`).
    let background = layout.fade_to;
    let hover_bg = layout.fade_to_hovered;
    let select_entity = layout.entity.clone();
    // A right-click anywhere on the row opens the same menu the dots do: it is
    // where a reader reaches for a row's actions, and the dots are what says so.
    let menu_entity = layout.entity.clone();
    let menu_row = Rc::new(ActionsMenuData::from_row(row, layout.icons));
    let debug = layout.debug;
    let group = SharedString::from(format!("replay-row-{ix}"));
    v_flex()
        .id(ix)
        .test_support()
        .group(group.clone())
        .relative()
        .w_full()
        .py_0p5()
        .bg(background)
        .hover(move |style| style.bg(hover_bg))
        .on_click(move |event: &ClickEvent, _window, cx: &mut App| {
            if event.modifiers().secondary(cx) {
                select_entity.update(cx, |this, cx| this.toggle_selected(ix, cx));
            }
        })
        .child(first_line)
        .children(detail_line)
        .child(team_bands::stripe(layout.mark, cx))
        // Wraps the row, so it goes last: the menu is a container around
        // what it belongs to rather than a style on it.
        .context_menu(move |menu, _window, cx| {
            build_actions_menu(menu, &menu_row, debug, menu_entity.clone(), MenuOrigin::RowRightClick, cx)
        })
        .into_any_element()
}

impl Render for PlayerTable {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rem = window.rem_size();
        if self.widths_dirty || self.measured_rem != Some(rem) {
            let measured = measure_column_widths(&self.model, self.debug, &self.expanded, &self.closing, window);
            self.column_widths = measured.columns;
            self.detail_widths = measured.details;
            self.detail_fit = measured.detail_fit;
            self.widths_dirty = false;
            self.measured_rem = Some(rem);
            // A drag's width holds only while the detail it was set over shows.
            let detail_fit = &self.detail_fit;
            self.fit_waived.retain(|col| detail_fit[*col as usize].is_some());
        }
        if self.activation.is_none() {
            self.activation = Some(cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.abandon_drags(cx);
                }
            }));
        }

        let border = cx.theme().border;
        let hover_bg = cx.theme().muted;

        let shown = self.drawn_columns();
        // Alt turns the damage breakdowns' percentages around: how much of
        // the other player's total a line was, rather than of this row's.
        let alt_held = crate::status_bar::is_held(crate::status_bar::ModifierKey::Alt, window, cx);
        let shares_shown = alt_held
            && expanded::shows_damage_shares(
                self.model
                    .rows
                    .iter()
                    .filter(|row| self.expanded.contains(&row.db_id) || self.closing.contains_key(&row.db_id)),
                shown.iter(),
                self.debug,
            );
        crate::status_bar::set_mode(cx.weak_entity().into(), shares_shown.then_some(expanded::SHARE_MODE), cx);
        if !std::mem::replace(&mut self.widths_loaded, true) {
            self.load_column_settings(cx);
        }

        // Resolved once per frame so the header, the rows and the scrolling
        // section's total all read the same number.
        self.drawn_widths = ReplayColumn::ALL.iter().map(|col| self.fitted_width(*col)).collect();
        let sticky_cap = self.sticky_cap().map(f32::from);
        if let Some(cap) = sticky_cap {
            fit_frozen_widths(&mut self.drawn_widths, &shown.pinned, px(cap));
        }
        let scroll_width: f32 = shown.scrolling.iter().map(|col| self.drawn_widths[*col as usize].as_f32()).sum();
        let reserve = pinned_reserve(self.moving_column().is_some(), &shown.pinned);
        let offsets = column_offsets(&shown, &self.drawn_widths, self.sticky_cap(), reserve);
        self.pinned_edge = offsets.pinned_edge;

        // Each column slides from where it was drawn to its new place, both
        // as seen: scrolled, for the scrolling ones.
        let progress =
            self.slide.as_ref().and_then(|slide| crate::motion::rearrange_progress(slide.started, window, cx));
        if progress.is_none() {
            self.slide = None;
        }
        let scroll = self.h_scroll.offset().x;
        for col in ReplayColumn::ALL {
            let target = offsets.starts[col as usize]
                .map(|start| if shown.pinned.contains(&col) { start } else { start + scroll });
            let from = self.slide.as_ref().and_then(|slide| slide.from[col as usize]);
            let shift = match (target, from, progress) {
                (Some(target), Some(from), Some(progress)) => (from - target) * (1. - progress),
                _ => px(0.),
            };
            self.column_shift[col as usize] = shift;
            self.drawn_starts[col as usize] = target.map(|target| target + shift);
        }

        let ShownColumns { pinned: sticky_columns, scrolling: scroll_columns } = shown;
        let header = h_flex()
            .w_full()
            .flex_none()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .min_w(reserve)
                    .max_w(relative(STICKY_MAX_FRACTION))
                    .overflow_hidden()
                    .children(sticky_columns.iter().map(|col| self.header_cell(*col, cx))),
            )
            .child(
                div()
                    .id("replay-header-h-scroll")
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_x_scroll()
                    .restrict_scroll_to_axis()
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
        // Headings only while each team's rows stay together under the sort.
        let self_team = self.model.self_team;
        let team_runs = team_bands::team_runs(&self.model.rows, self_team).unwrap_or_default();
        let team_totals = TeamTotals::of(&self.model.rows, self_team, self.debug);
        let team_damage = team_totals.column(ReplayColumn::ActualDamage);
        let render_item = move |ix: usize, window: &mut Window, cx: &mut App| -> AnyElement {
            let (db_id, is_expanded, is_closing) = {
                let table = entity.read(cx);
                let db_id = table.model.rows[ix].db_id;
                (db_id, table.expanded.contains(&db_id), table.closing.contains_key(&db_id))
            };
            // Keyed by `db_id`: the row's own ids are list positions.
            let turn = crate::motion::disclosure_turn(
                ("replay-row-chevron", db_id.raw().cast_unsigned()),
                is_expanded,
                window,
                cx,
            );
            let detail = RowDetail::from_reveal(
                crate::motion::reveal_progress(
                    ("replay-row-detail", db_id.raw().cast_unsigned()),
                    is_expanded,
                    window,
                    cx,
                ),
                is_expanded,
            );
            // Deferred because the list is mid-render; the widths it frees
            // apply from the next frame.
            if is_closing && !detail.is_shown() {
                let entity = entity.clone();
                cx.defer(move |cx| entity.update(cx, |table, cx| table.finish_closing(db_id, cx)));
            }
            let table = entity.read(cx);
            let row = &table.model.rows[ix];
            let selected = table.selected == Some(db_id);
            // What the row is painted with, so a cut-off detail and the
            // detail's well match it: the stripe or the selected-row tone.
            let fade_to = if selected {
                cx.theme().table_active
            } else {
                crate::ui::stripe(ix, cx).unwrap_or(cx.theme().background)
            };
            let fade_to_hovered = hover_bg;
            let layout = RowLayout {
                sticky_cap,
                sticky_columns: &sticky_columns,
                scroll_columns: &scroll_columns,
                scroll_width,
                column_widths: &table.drawn_widths,
                detail_widths: &table.detail_widths,
                fade_to,
                fade_to_hovered,
                foreground: cx.theme().foreground,
                team_totals: &team_totals,
                icons: &table.icons,
                debug: table.debug,
                h_scroll: &h_scroll,
                entity: entity.clone(),
                detail,
                turn,
                selected,
                mark: RowMark::of(row, self_team),
                alt_held,
                column_shift: &table.column_shift,
                moving: table.moving_column(),
                pinned_reserve: reserve,
                all_rows: &table.model.rows,
            };
            let row = render_row(ix, row, &layout, cx);
            match team_runs.iter().find(|run| run.start == ix) {
                Some(run) => {
                    v_flex().w_full().child(team_bands::heading(*run, team_damage, cx)).child(row).into_any_element()
                }
                None => row,
            }
        };

        let reorder_overlay = self.reorder_overlay(cx);

        div()
            .id("replay-table-root")
            .size_full()
            .relative()
            // A width drag or a column move keeps following the pointer past
            // the header's own bounds, which is where it goes the moment a
            // column grows or is carried off.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                // The release went somewhere this table did not see.
                if event.pressed_button != Some(MouseButton::Left) {
                    this.abandon_drags(cx);
                    return;
                }
                this.drag_column(event.position.x, cx);
                this.follow_reorder(event.position.x, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    this.end_column_drag(cx);
                    this.end_reorder(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    this.end_column_drag(cx);
                    this.end_reorder(cx);
                }),
            )
            .child(v_flex().size_full().child(header).child(list(self.list_state.clone(), render_item).flex_1()))
            .children(reorder_overlay)
            .child(Scrollbar::vertical(&self.list_state))
            // Along the bottom of the scrolling columns. Without it there is
            // nothing to say the table runs past its right edge, and a
            // reader has no reason to look. The frozen columns are stood off
            // with a spacer that gives way rather than a fixed offset, so on a
            // panel too narrow for them the bar is still there to be used.
            .child(
                h_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(12.))
                    // The same cap the frozen section is drawn at, so the bar
                    // starts where those columns end rather than over them.
                    .child(div().w(offsets.pinned_edge))
                    .child(div().flex_1().min_w(H_SCROLLBAR_MIN_WIDTH).child(Scrollbar::horizontal(&self.h_scroll))),
            )
            // The table's own bounds, for the cap above and for placing a
            // moved column. Painted over everything and hit-testing nothing.
            .child(
                canvas(
                    {
                        let measure = cx.weak_entity();
                        move |bounds: Bounds<Pixels>, _window, cx| {
                            let _ = measure.update(cx, |this: &mut Self, cx| {
                                if this.viewport != Some(bounds) {
                                    this.viewport = Some(bounds);
                                    cx.notify();
                                }
                            });
                        }
                    },
                    |_bounds, _prepaint, _window, _cx| {},
                )
                .absolute()
                .inset_0(),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AppContext as _;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt as _;
    use wows_replays::analyzer::battle_controller::BattleResult;
    use wows_replays::types::TeamId;
    use wows_toolkit_replay_data::columns::ReplayColumn;
    use wows_toolkit_replay_data::damage_mix::DamageKind;
    use wows_toolkit_replay_data::damage_mix::DamageLine;
    use wows_toolkit_replay_data::model::MatchContext;
    use wows_toolkit_replay_data::model::ReplayReportModel;
    use wows_toolkit_replay_data::sort::SortColumn;
    use wows_toolkit_replay_data::test_support::base_row;
    use wowsunpack::vfs::MemoryFS;
    use wowsunpack::vfs::VfsPath;

    use super::ConnectionNote;
    use super::PlayerTable;
    use super::TeamDamage;
    use super::TeamShare;
    use super::TeamSide;
    use super::TeamTotals;
    use super::TimeOffline;
    use wows_replays::types::AccountId;
    use wows_replays::types::Relation;

    fn two_row_model() -> ReplayReportModel {
        ReplayReportModel {
            self_team: TeamId::from(0i64),
            rows: vec![base_row(1, Relation::new(0), true), base_row(2, Relation::new(2), false)],
            battle_result: Some(BattleResult::Win(0)),
            columns: ReplayColumn::ALL.to_vec(),
            map: "Ocean".to_string(),
            chat: Vec::new(),
            timestamp: jiff::Timestamp::UNIX_EPOCH,
            context: MatchContext::default(),
        }
    }

    fn open_table(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<PlayerTable> {
        open_table_at(cx, px(1400.))
    }

    fn open_table_at(cx: &mut TestAppContext, width: gpui_kit::Pixels) -> gpui_kit::WindowHandle<PlayerTable> {
        cx.update(gpui_kit::init);
        cx.open_window(size(width, px(600.)), |_window, cx| {
            let vfs: VfsPath = MemoryFS::new().into();
            PlayerTable::new(two_row_model(), vfs, false, cx)
        })
    }

    /// Widening a column is not a request to re-sort by it: the grip lies
    /// inside the header, which sorts when it is clicked.
    #[gpui_kit::test]
    fn dragging_a_header_grip_leaves_the_sort_alone(cx: &mut TestAppContext) {
        let window = open_table(cx);
        let table = window.entity(cx).expect("the window has a root view");
        let before = cx.update(|cx| table.read(cx).sort);

        let grip = ("replay-header-grip", ReplayColumn::ShipName as usize);
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let from = window.find(grip).bounds().center();
            window.drag(from, gpui_kit::point(from.x + px(40.), from.y), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        assert_eq!(cx.update(|cx| table.read(cx).sort), before, "the drag widened the column and nothing else");
    }

    /// Every column occupies its own strip: each cell begins where the one
    /// before it ended, and the header sits over the cells it names. Checked
    /// while collapsed, while a row's detail is open (which re-measures the
    /// widths) and after a column has been dragged narrower than its content.
    #[gpui_kit::test]
    fn each_column_begins_where_the_one_before_it_ended(cx: &mut TestAppContext) {
        let window = open_table(cx);
        let columns = two_row_model().columns;

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_columns_tile(&columns, "collapsed", window);

            // A row with its detail open, which is what re-measures the widths.
            window.click(("replay-row-caret", 0usize), cx);
            window.render_frame(cx);
            assert_columns_tile(&columns, "expanded", window);

            // And with the leading column dragged well under its content.
            let grip = window.find(("replay-header-grip", ReplayColumn::Name as usize)).bounds().center();
            window.drag(grip, gpui_kit::point(grip.x - px(140.), grip.y), cx);
            window.render_frame(cx);
            assert_columns_tile(&columns, "narrowed", window);
        })
        .expect("the window is open");
    }

    /// A zoom measures the columns again, so each header is as wide as its
    /// label at the new text size rather than at the one it was first drawn
    /// at, where the label would be cut off.
    #[gpui_kit::test]
    fn a_zoom_refits_each_header_to_its_label(cx: &mut TestAppContext) {
        let window = open_table(cx);
        let columns = two_row_model().columns;

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.set_rem_size(window.rem_size() * 2.0);
            window.render_frame(cx);

            let mut checked = 0;
            for col in &columns {
                let Some(header) = window.try_find(("replay-header", *col as usize)) else { continue };
                let header = header.bounds();
                let label = px(super::text_width(super::column_label(*col).into(), window, true));
                assert!(header.size.width >= label, "{col:?}'s header {header:?} holds its label, {label:?} wide");
                checked += 1;
            }
            assert!(checked > 0, "the headers are drawn");
        })
        .expect("the window is open");
    }

    /// Asserts the drawn columns tile left to right with no gap or overlap,
    /// and that each header sits over its own cells.
    fn assert_columns_tile(columns: &[ReplayColumn], state: &str, window: &mut gpui_kit::Window) {
        let mut edge: Option<gpui_kit::Pixels> = None;
        for col in columns {
            // Row 0, where the id encoding `ix * CELL_ID_STRIDE + col` leaves just the column.
            let cell = window.find(("replay-cell", *col as usize)).bounds();
            if let Some(edge) = edge {
                assert_eq!(cell.origin.x, edge, "{state}: {col:?} does not start where the column before it ended");
            }
            if let Some(header) = window.try_find(("replay-header", *col as usize)) {
                let header = header.bounds();
                assert_eq!(header.origin.x, cell.origin.x, "{state}: {col:?}'s header is not over its cells");
                assert_eq!(header.size.width, cell.size.width, "{state}: {col:?}'s header is not its cells' width");
            }
            edge = Some(cell.origin.x + cell.size.width);
        }
    }

    /// A panel narrower than the two frozen columns keeps the table inside
    /// itself: the frozen pair gives way rather than running past the panel's
    /// edge over whatever is beside it, and the scrolling columns keep room to
    /// be reached.
    #[gpui_kit::test]
    fn a_panel_narrower_than_the_frozen_columns_still_holds_the_table(cx: &mut TestAppContext) {
        let panel = px(240.);
        let window = open_table_at(cx, panel);
        let table = window.entity(cx).expect("the window has a root view");
        let shown = super::ColumnArrangement::new().shown(&two_row_model().columns);
        let scrolling = shown.scrolling[0];

        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the window is open");

        // Without this the assertions below would hold on a fixture whose two
        // frozen columns happened to fit, and the clamp would go untested.
        let wanted: gpui_kit::Pixels = cx.update(|cx| {
            let table = table.read(cx);
            shown.pinned.iter().map(|col| table.width_of(*col)).sum()
        });
        assert!(
            wanted > panel * super::STICKY_MAX_FRACTION,
            "the fixture's frozen columns want {wanted:?}, which must exceed the {:?} cap for this to test it",
            panel * super::STICKY_MAX_FRACTION
        );

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            // Row 0, where the id encoding `ix * CELL_ID_STRIDE + col` leaves just the column.
            let first_scrolling = window.find(("replay-cell", scrolling as usize)).bounds();
            assert!(
                first_scrolling.origin.x < panel,
                "the scrolling columns start inside the panel, at {:?} of {panel:?}",
                first_scrolling.origin.x
            );
            assert!(
                first_scrolling.origin.x <= panel * super::STICKY_MAX_FRACTION,
                "the frozen columns take no more than their share, leaving {:?} of {panel:?}",
                first_scrolling.origin.x
            );
        })
        .expect("the window is open");
    }

    /// On a panel too narrow for the frozen pair, the name gives way rather
    /// than the ship column: the frozen section is clipped at its right edge,
    /// and the grip that sizes the ship column against the scrolling ones sits
    /// on that edge, where a drag still reaches it.
    #[gpui_kit::test]
    fn the_last_frozen_column_keeps_its_grip_on_a_narrow_panel(cx: &mut TestAppContext) {
        let panel = px(420.);
        let window = open_table_at(cx, panel);
        let table = window.entity(cx).expect("the window has a root view");
        let ship = ReplayColumn::ShipName;

        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the window is open");
        let (wanted, ship_before) = cx.update(|cx| {
            let table = table.read(cx);
            (table.width_of(ReplayColumn::Name) + table.width_of(ship), table.drawn_widths[ship as usize])
        });
        assert!(wanted > panel * super::STICKY_MAX_FRACTION, "the frozen pair must exceed the cap for this to test it");

        cx.update_window(window.into(), |_, window, cx| {
            let grip = window.find(("replay-header-grip", ship as usize)).bounds().center();
            window.drag(grip, gpui_kit::point(grip.x - px(20.), grip.y), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        assert!(
            cx.update(|cx| table.read(cx).width_of(ship)) < ship_before,
            "the ship column's grip was in reach and the drag narrowed it"
        );
    }

    /// Dragging the ship column wider stops where the frozen section reaches
    /// its cap: the name column beside it keeps its width, so the ship
    /// column's own left edge stays put under the drag.
    #[gpui_kit::test]
    fn a_frozen_drag_stops_at_the_cap_without_moving_its_neighbour(cx: &mut TestAppContext) {
        let window = open_table_at(cx, px(900.));
        let table = window.entity(cx).expect("the window has a root view");
        let ship = ReplayColumn::ShipName;

        let left_edge = |cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.find(("replay-header-grip", ReplayColumn::Name as usize)).bounds().center().x
            })
            .expect("the window is open")
        };
        let before = left_edge(cx);
        cx.update_window(window.into(), |_, window, cx| {
            let grip = window.find(("replay-header-grip", ship as usize)).bounds().center();
            window.drag(grip, gpui_kit::point(grip.x + px(800.), grip.y), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        assert_eq!(left_edge(cx), before, "the name column's edge did not move");
        let frozen: gpui_kit::Pixels = cx.update(|cx| {
            let table = table.read(cx);
            table.drawn_widths[ReplayColumn::Name as usize] + table.drawn_widths[ship as usize]
        });
        assert!(
            frozen <= px(900.) * super::STICKY_MAX_FRACTION + px(1.),
            "the drag stopped at the cap, got {frozen:?}"
        );
    }

    /// A sortable column fitted to its header holds the whole label and the
    /// sort indicator; narrowed below that, even to the drag minimum at
    /// twice the zoom, the label gives way and the indicator stays inside.
    #[gpui_kit::test]
    fn a_headers_sort_indicator_is_never_cut_off(cx: &mut TestAppContext) {
        let window = open_table(cx);
        let table = window.entity(cx).expect("the window has a root view");
        let base_xp = ReplayColumn::BaseXp as usize;
        cx.update(|cx| table.update(cx, |table, cx| table.sort_by(SortColumn::BaseXp, cx)));
        let indicator_inside = |window: &mut gpui_kit::Window| {
            let header = window.find(("replay-header", base_xp)).bounds();
            let indicator = window.find(("replay-header-sort", base_xp)).bounds();
            indicator.left() >= header.left() && indicator.right() <= header.right()
        };

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let label = window.find(("replay-header-label", base_xp)).bounds();
            let text = super::text_width(super::column_label(ReplayColumn::BaseXp).into(), window, true);
            assert!(label.size.width.as_f32() >= text - 0.5, "the fitted label is cut to {:?}", label.size.width);
            assert!(indicator_inside(window), "the fitted header holds the indicator");
        })
        .expect("the window is open");

        cx.update(|cx| {
            table.update(cx, |table, cx| {
                table.width_overrides[base_xp] = Some(super::COLUMN_DRAG_MIN);
                cx.notify();
            })
        });
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(indicator_inside(window), "a narrowed header still holds the indicator");
            window.set_rem_size(window.rem_size() * 2.0);
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(indicator_inside(window), "and does at twice the zoom");
        })
        .expect("the window is open");
    }

    /// The row's dots sit on the row's centre line, as the cells beside them
    /// do, rather than hanging from its top edge.
    #[gpui_kit::test]
    fn the_row_dots_are_centred_on_the_row(cx: &mut TestAppContext) {
        let window = open_table(cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let row = window.find(0usize).bounds();
            let dots = window.find(("replay-row-actions", 0usize)).bounds();
            let offset = (row.center().y - dots.center().y).abs();
            assert!(offset <= px(0.5), "the dots sit {offset:?} off the row's centre line");
        })
        .expect("the window is open");
    }

    /// An expanded row is taller than its first line, and its cells start at
    /// its top: the dots stay on that first line instead of drifting to the
    /// middle of the detail below it.
    #[gpui_kit::test]
    fn an_expanded_rows_dots_stay_on_its_first_line(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        // The layout of an open row is what is asserted, not its opening.
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.open_window(size(px(1400.), px(600.)), |_window, cx| {
            let mut model = two_row_model();
            // Damage events give the Name column detail several lines tall.
            for row in &mut model.rows {
                row.fires = Some(3);
                row.floods = Some(1);
                row.citadels = Some(2);
                row.crits = Some(4);
            }
            let vfs: VfsPath = MemoryFS::new().into();
            PlayerTable::new(model, vfs, false, cx)
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("replay-row-caret", 0usize), cx);
            window.render_frame(cx);
            let row = window.find(0usize).bounds();
            let dots = window.find(("replay-row-actions", 0usize)).bounds();
            assert!(row.size.height > dots.size.height * 2., "the expanded row shows its detail");
            // Centred on a row over twice their height, the dots would sit
            // more than half their height down.
            assert!(
                dots.top() - row.top() < dots.size.height / 2.,
                "the dots sit {:?} below the row's top",
                dots.top() - row.top()
            );
        })
        .expect("the window is open");
    }

    /// A model whose first row has detail under several columns and a
    /// damage split under its Actual Damage figure.
    fn detailed_model() -> ReplayReportModel {
        let mut model = two_row_model();
        for row in &mut model.rows {
            row.fires = Some(3);
            row.floods = Some(1);
            row.citadels = Some(2);
            row.crits = Some(4);
        }
        model.rows[0].actual_damage = Some(50_000);
        model.rows[0].actual_damage_text = Some("50,000".to_string());
        model.rows[0].actual_damage_by_type =
            Some(vec![DamageLine { kind: DamageKind::Ap, label: "AP", amount: 50_000 }]);

        model
    }

    fn open_detailed_table(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<PlayerTable> {
        cx.update(gpui_kit::init);
        // The layout of an open row is what is asserted, not its opening.
        cx.update(|cx| cx.set_reduce_motion(true));
        cx.open_window(size(px(1400.), px(600.)), |_window, cx| {
            let vfs: VfsPath = MemoryFS::new().into();
            PlayerTable::new(detailed_model(), vfs, false, cx)
        })
    }

    fn double_click(window: &mut gpui_kit::Window, at: gpui_kit::Point<gpui_kit::Pixels>, cx: &mut gpui_kit::App) {
        use gpui_kit::InputEvent as _;

        for click_count in [1, 2] {
            let down = gpui_kit::MouseDownEvent {
                button: gpui_kit::MouseButton::Left,
                position: at,
                modifiers: gpui_kit::Modifiers::default(),
                click_count,
                first_mouse: false,
            };
            let up = gpui_kit::MouseUpEvent {
                button: gpui_kit::MouseButton::Left,
                position: at,
                modifiers: gpui_kit::Modifiers::default(),
                click_count,
            };
            window.dispatch_event(down.to_platform_input(), cx);
            window.dispatch_event(up.to_platform_input(), cx);
            window.render_frame(cx);
        }
    }

    /// Opening a row moves nothing on its first line up or down: every cell
    /// keeps its height and its place on the line.
    #[gpui_kit::test]
    fn a_rows_first_line_stays_still_when_it_opens(cx: &mut TestAppContext) {
        let window = open_detailed_table(cx);
        let lines = |window: &mut gpui_kit::Window| {
            let ids = [
                ("replay-cell", ReplayColumn::Name as usize),
                ("replay-cell", ReplayColumn::ShipName as usize),
                ("replay-cell", ReplayColumn::Kills as usize),
                ("replay-damage-mix", ReplayColumn::ActualDamage as usize),
                ("replay-row-actions", 0usize),
            ];
            ids.map(|id| {
                let bounds = window.find(id).bounds();
                (bounds.top(), bounds.size.height)
            })
        };

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let closed = lines(window);
            window.click(("replay-row-caret", 0usize), cx);
            window.render_frame(cx);
            assert!(
                window.try_find(("replay-expanded-cell", ReplayColumn::Name as usize)).is_some(),
                "the row is open"
            );
            assert_eq!(lines(window), closed);
        })
        .expect("the window is open");
    }

    /// A double-click on the empty part of the first line opens and closes
    /// the row; one on the text, or anywhere in the detail, does not.
    #[gpui_kit::test]
    fn a_double_click_on_a_rows_empty_space_toggles_it(cx: &mut TestAppContext) {
        let window = open_detailed_table(cx);
        let table = window.entity(cx).expect("the window has a root view");
        let open = |cx: &mut TestAppContext| cx.update(|cx| table.read(cx).expanded.contains(&AccountId(1)));
        let at = |id: (&'static str, usize), cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                let bounds = window.find(id).bounds();
                gpui_kit::point(bounds.right() - px(3.), bounds.center().y)
            })
            .expect("the window is open")
        };
        let double_click_at = |point, cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| double_click(window, point, cx))
                .expect("the window is open");
        };
        let kills = ReplayColumn::Kills as usize;

        double_click_at(at(("replay-cell-text-box", kills), cx), cx);
        assert!(!open(cx), "a double-click on the figure is the text's");

        double_click_at(at(("replay-cell", kills), cx), cx);
        assert!(open(cx), "a double-click beside the figure opens the row");

        double_click_at(at(("replay-expanded-cell", ReplayColumn::Name as usize), cx), cx);
        assert!(open(cx), "a double-click in the detail leaves it open");

        double_click_at(at(("replay-cell", kills), cx), cx);
        assert!(!open(cx), "and another beside the figure closes it");
    }

    /// A row's detail opens and closes over several frames: partway through
    /// either it is between its closed and open heights, and a close starts
    /// from the open height rather than from nothing.
    #[gpui_kit::test]
    fn a_rows_detail_grows_and_shrinks_rather_than_jumping(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1400.), px(600.)), |_window, cx| {
            let mut model = two_row_model();
            for row in &mut model.rows {
                row.fires = Some(3);
                row.floods = Some(1);
                row.citadels = Some(2);
                row.crits = Some(4);
            }
            let vfs: VfsPath = MemoryFS::new().into();
            PlayerTable::new(model, vfs, false, cx)
        });
        let height = |cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.find(0usize).bounds().size.height
            })
            .expect("the window is open")
        };
        let toggle = |cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| window.click(("replay-row-caret", 0usize), cx))
                .expect("the window is open");
        };
        let step = std::time::Duration::from_millis(80);

        let closed = height(cx);
        toggle(cx);
        height(cx);
        cx.executor().advance_clock(step);
        let opening = height(cx);
        cx.executor().advance_clock(std::time::Duration::from_secs(1));
        let open = height(cx);
        assert!(closed < opening && opening < open, "opening passes between {closed:?} and {open:?}, got {opening:?}");

        toggle(cx);
        let closing_starts = height(cx);
        assert!(closing_starts > (closed + open) / 2., "a close starts near the open height, got {closing_starts:?}");
        cx.executor().advance_clock(step);
        let closing = height(cx);
        assert!(closed < closing && closing < open, "closing passes between them, got {closing:?}");
        cx.executor().advance_clock(std::time::Duration::from_secs(1));
        assert_eq!(height(cx), closed, "and ends closed");
    }

    /// A column follows its content until the reader sets its width, and then
    /// keeps what they set even when the content later measures wider.
    #[gpui_kit::test]
    fn a_width_the_reader_set_outlives_a_remeasure(cx: &mut TestAppContext) {
        let window = open_table(cx);
        let table = window.entity(cx).expect("the window has a root view");
        let set = ReplayColumn::ShipName;
        let untouched = ReplayColumn::PotentialDamage;

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let grip = window.find(("replay-header-grip", set as usize)).bounds().center();
            window.drag(grip, gpui_kit::point(grip.x - px(30.), grip.y), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        let dragged = cx.update(|cx| table.read(cx).width_of(set));
        assert!(cx.update(|cx| table.read(cx).width_overrides[untouched as usize]).is_none(), "only one was set");

        // Opening a row's detail re-measures every column against its content.
        cx.update_window(window.into(), |_, window, cx| {
            window.click(("replay-row-caret", 0usize), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        assert_eq!(cx.update(|cx| table.read(cx).width_of(set)), dragged, "the width the reader set is still theirs");
        assert!(
            cx.update(|cx| table.read(cx).width_overrides[untouched as usize]).is_none(),
            "and a column they never touched still follows its content"
        );

        // Double-clicking the grip hands the column back to its content.
        cx.update_window(window.into(), |_, window, cx| {
            window.double_click(("replay-header-grip", set as usize), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");
        assert!(
            cx.update(|cx| table.read(cx).width_overrides[set as usize]).is_none(),
            "the set width is given up rather than kept at whatever the drag left"
        );
    }

    /// What the copy actions put on the clipboard: one row as its visible
    /// cells, and the whole table under a heading line, both tab separated so
    /// a spreadsheet reads them back as columns.
    #[gpui_kit::test]
    fn a_row_and_the_table_copy_as_tab_separated_text(cx: &mut TestAppContext) {
        let window = open_table(cx);
        let table = window.entity(cx).expect("the window has a root view");

        cx.update(|cx| {
            let table = table.read(cx);
            let columns = table.model.columns.len();

            let first = table.model.rows[0].db_id;
            let row = table.row_text(first).expect("the first row copies");
            assert_eq!(row.split('\t').count(), columns, "one field per visible column");

            let whole = table.table_text();
            let lines: Vec<&str> = whole.lines().collect();
            assert_eq!(lines.len(), table.model.rows.len() + 1, "a heading line and one line per row");
            assert_eq!(lines[0].split('\t').count(), columns, "the heading names every column");
            assert_eq!(lines[1], row, "the first line under the heading is the first row");

            let cell = table.cell_text(first, ReplayColumn::ShipName).expect("the cell copies");
            assert!(row.split('\t').any(|field| field == cell), "the cell's text is one of the row's fields");

            let absent = wows_replays::types::AccountId(-1);
            assert_eq!(table.row_text(absent), None, "a player who is not in the table copies nothing");
        });
    }

    /// And the header still sorts when it is the header that was clicked,
    /// rather than the grip on its edge.
    #[gpui_kit::test]
    fn clicking_a_header_still_sorts_by_it(cx: &mut TestAppContext) {
        let window = open_table(cx);
        let table = window.entity(cx).expect("the window has a root view");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("replay-header", ReplayColumn::ShipName as usize), cx);
        })
        .expect("the window is open");

        assert_eq!(cx.update(|cx| table.read(cx).sort.column()), SortColumn::ShipName);
    }

    /// A frozen column dragged past the cap keeps every frozen grip inside
    /// the clipped section: Name gives way first, and the dragged column
    /// stops where Name has reached its minimum.
    #[test]
    fn a_frozen_column_is_held_inside_the_cap() {
        let frozen = [ReplayColumn::Name, ReplayColumn::ShipName];
        let cap = px(300.);
        let mut widths = vec![px(100.); ReplayColumn::ALL.len()];
        widths[ReplayColumn::ShipName as usize] = px(400.);

        super::fit_frozen_widths(&mut widths, &frozen, cap);

        let total: gpui_kit::Pixels = frozen.iter().map(|col| widths[*col as usize]).sum();
        assert!(total <= cap, "the frozen section is {total:?}, past its cap");
        assert_eq!(widths[ReplayColumn::Name as usize], super::COLUMN_DRAG_MIN, "Name gives way first");
        assert_eq!(
            super::frozen_room(&frozen, &widths, cap, ReplayColumn::ShipName),
            cap - super::COLUMN_DRAG_MIN,
            "a drag of ShipName stops where Name is at its minimum"
        );
        assert_eq!(
            super::frozen_room(&frozen, &widths, cap, ReplayColumn::Name),
            cap - widths[ReplayColumn::ShipName as usize],
            "a drag of Name stops at what ShipName leaves"
        );
    }

    /// A table that draws every move at once, for asserting where things land.
    fn open_still_table(cx: &mut TestAppContext, model: ReplayReportModel) -> gpui_kit::WindowHandle<PlayerTable> {
        cx.update(gpui_kit::init);
        cx.update(|cx| cx.set_reduce_motion(true));
        cx.open_window(size(px(1400.), px(600.)), |_window, cx| {
            let vfs: VfsPath = MemoryFS::new().into();
            PlayerTable::new(model, vfs, false, cx)
        })
    }

    fn header_left(window: &gpui_kit::Window, col: ReplayColumn) -> gpui_kit::Pixels {
        window.find(("replay-header", col as usize)).bounds().origin.x
    }

    /// A ship with a known nation is named after its flag; one without has
    /// no flag.
    #[gpui_kit::test]
    fn a_ship_name_follows_its_nations_flag(cx: &mut TestAppContext) {
        let mut model = two_row_model();
        model.rows[0].ship_nation = Some("usa".to_string());
        let window = open_still_table(cx, model);
        let table = window.entity(cx).expect("the window has a root view");
        // Let the table's own icon load land before the flag is put in place.
        cx.run_until_parked();
        let mut png = Vec::new();
        image::RgbaImage::new(23, 16)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("a flag image encodes");
        cx.update(|cx| {
            table.update(cx, |table, cx| {
                assert!(table.icons.set_keyed(super::nation_flag_key("usa"), &png), "the flag decodes");
                cx.notify();
            })
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(("replay-ship-flag", 0usize)).is_some(), "the known nation's flag is drawn");
            assert!(window.try_find(("replay-ship-flag", 1usize)).is_none(), "an unknown nation draws none");
        })
        .expect("the window is open");
    }

    /// Dragging a header across the others moves its column there, without
    /// sorting by it, and the order it leaves is what is saved.
    #[gpui_kit::test]
    fn dragging_a_header_moves_its_column_and_the_order_is_kept(cx: &mut TestAppContext) {
        let window = open_still_table(cx, two_row_model());
        let table = window.entity(cx).expect("the window has a root view");
        let sort_before = cx.update(|cx| table.read(cx).sort);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let from = window.find(("replay-header", ReplayColumn::Kills as usize)).bounds().center();
            let skills = window.find(("replay-header", ReplayColumn::Skills as usize)).bounds();
            // Inside Skills's left half, so Kills lands before it.
            let to = gpui_kit::point(skills.left() + px(2.), skills.center().y);
            window.drag(from, to, cx);
            window.render_frame(cx);
            assert!(
                header_left(window, ReplayColumn::Kills) < header_left(window, ReplayColumn::Skills),
                "Kills is drawn before Skills"
            );
        })
        .expect("the window is open");

        let (scrolling, stored, arrangement) = cx.update(|cx| {
            let table = table.read(cx);
            (table.shown_columns().scrolling, table.stored_arrangement(), table.arrangement.clone())
        });
        assert_eq!(scrolling[..2], [ReplayColumn::Kills, ReplayColumn::Skills], "Kills landed where it was dropped");
        assert_eq!(cx.update(|cx| table.read(cx).sort), sort_before, "moving a column is not a sort");
        assert_eq!(
            super::ColumnArrangement::from_stored(&stored),
            arrangement,
            "what is saved reads back as the order"
        );
    }

    /// A table opened after a change draws it from its first frame, and the
    /// settings read that lands after that frame does not undo a change made
    /// before it landed.
    #[gpui_kit::test]
    fn a_change_outlives_a_late_settings_read(cx: &mut TestAppContext) {
        let window = open_still_table(cx, two_row_model());
        let table = window.entity(cx).expect("the window has a root view");
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the window is open");

        cx.update(|cx| table.update(cx, |table, cx| table.set_pinned(ReplayColumn::Kills, true, cx)));
        let stale = super::ColumnArrangement::new().to_stored();
        cx.update(|cx| table.update(cx, |table, cx| table.adopt_stored_arrangement(Some(&stale), cx)));
        assert!(
            cx.update(|cx| table.read(cx).arrangement.is_pinned(ReplayColumn::Kills)),
            "the read landing after the pin leaves it"
        );

        let reopened = open_still_table(cx, two_row_model());
        let next = reopened.entity(cx).expect("the window has a root view");
        cx.update_window(reopened.into(), |_, window, cx| {
            window.render_frame(cx);
            let ship_end = window.find(("replay-header", ReplayColumn::ShipName as usize)).bounds().right();
            assert_eq!(header_left(window, ReplayColumn::Kills), ship_end, "its first frame draws Kills pinned");
        })
        .expect("the window is open");
        cx.update(|cx| next.update(cx, |table, cx| table.adopt_stored_arrangement(Some(&stale), cx)));
        assert!(
            cx.update(|cx| next.read(cx).arrangement.is_pinned(ReplayColumn::Kills)),
            "and its own late read does not put back the older arrangement"
        );
    }

    /// While a header is carried, it follows the pointer over a tinted zone
    /// that ends where the pinned columns do, and its slot is marked.
    #[gpui_kit::test]
    fn a_carried_header_follows_the_pointer_over_the_pinned_zone(cx: &mut TestAppContext) {
        use gpui_kit::InputEvent as _;

        let window = open_still_table(cx, two_row_model());
        let table = window.entity(cx).expect("the window has a root view");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let from = window.find(("replay-header", ReplayColumn::Kills as usize)).bounds().center();
            let pinned_end = window.find(("replay-header", ReplayColumn::ShipName as usize)).bounds().right();
            let press = gpui_kit::MouseDownEvent {
                button: gpui_kit::MouseButton::Left,
                position: from,
                modifiers: gpui_kit::Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            };
            let move_to = |window: &mut gpui_kit::Window,
                           to: gpui_kit::Point<gpui_kit::Pixels>,
                           pressed_button: Option<gpui_kit::MouseButton>,
                           cx: &mut gpui_kit::App| {
                let event = gpui_kit::MouseMoveEvent {
                    position: to,
                    pressed_button,
                    modifiers: gpui_kit::Modifiers::default(),
                };
                window.dispatch_event(event.to_platform_input(), cx);
                window.render_frame(cx);
            };
            let carry = |window: &mut gpui_kit::Window, to, cx: &mut gpui_kit::App| {
                move_to(window, to, Some(gpui_kit::MouseButton::Left), cx);
            };
            move_to(window, from, None, cx);
            window.dispatch_event(press.to_platform_input(), cx);
            window.render_frame(cx);

            let to = gpui_kit::point(from.x - px(30.), from.y);
            carry(window, to, cx);
            let ghost = window.find("replay-column-ghost").bounds();
            assert!((ghost.center().x - to.x).abs() < px(1.), "the header is carried with the pointer");
            assert!(window.try_find("replay-column-slot").is_some(), "the slot it will land in is marked");
            let zone = window.find("replay-pinned-zone").bounds();
            assert_eq!(zone.right(), pinned_end, "the zone ends where the pinned columns do");

            let into_zone = gpui_kit::point(
                window.find(("replay-header", ReplayColumn::Name as usize)).bounds().left() + px(4.),
                from.y,
            );
            carry(window, into_zone, cx);
            assert!(
                window.find("replay-pinned-zone").bounds().right() > pinned_end,
                "carried into the zone, the column joins the pinned ones"
            );

            let release = gpui_kit::MouseUpEvent {
                button: gpui_kit::MouseButton::Left,
                position: into_zone,
                modifiers: gpui_kit::Modifiers::default(),
                click_count: 1,
            };
            window.dispatch_event(release.to_platform_input(), cx);
            window.render_frame(cx);
            assert!(window.try_find("replay-column-ghost").is_none(), "nothing is carried once let go");
            assert!(window.try_find("replay-pinned-zone").is_none(), "and the zone is put away");
        })
        .expect("the window is open");

        assert_eq!(
            cx.update(|cx| table.read(cx).frozen_columns()),
            vec![ReplayColumn::Kills, ReplayColumn::Name, ReplayColumn::ShipName],
            "dropped at the start of the zone, Kills is pinned first"
        );
    }

    /// A pinned column dragged out past the zone's edge is unpinned where it
    /// was dropped.
    #[gpui_kit::test]
    fn dragging_a_pinned_column_out_unpins_it(cx: &mut TestAppContext) {
        let window = open_still_table(cx, two_row_model());
        let table = window.entity(cx).expect("the window has a root view");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let from = window.find(("replay-header", ReplayColumn::ShipName as usize)).bounds().center();
            let to = window.find(("replay-header", ReplayColumn::PersonalRating as usize)).bounds().center();
            window.drag(from, to, cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        let shown = cx.update(|cx| table.read(cx).shown_columns());
        assert_eq!(shown.pinned, vec![ReplayColumn::Name], "ShipName left the pinned columns");
        assert!(shown.scrolling.contains(&ReplayColumn::ShipName), "and scrolls with the rest");
    }

    /// A header's right-click menu pins its column after the pinned ones, and
    /// on a pinned column unpins it beside them.
    #[gpui_kit::test]
    fn the_header_menu_pins_and_unpins_a_column(cx: &mut TestAppContext) {
        let window = open_still_table(cx, two_row_model());
        let table = window.entity(cx).expect("the window has a root view");
        let kills = ("replay-header", ReplayColumn::Kills as usize);

        for (label, pinned) in [("ui.replay.context.pin_column", true), ("ui.replay.context.unpin_column", false)] {
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.right_click(kills, cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                let label = rust_i18n::t!(label).into_owned();
                let offered = crate::interaction_tests::menu_items(window);
                let labels: Vec<Option<String>> =
                    offered.iter().map(|(_, item)| item.label().map(str::to_owned)).collect();
                let (row, _) = offered
                    .into_iter()
                    .find(|(_, item)| item.label() == Some(label.as_str()))
                    .unwrap_or_else(|| panic!("the header menu offers {label}, offered {labels:?}"));
                window.within("popup-menu").click(gpui_kit::ElementId::Integer(row), cx);
            })
            .expect("the window is open");
            cx.run_until_parked();

            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                let ship_end = window.find(("replay-header", ReplayColumn::ShipName as usize)).bounds().right();
                assert_eq!(header_left(window, ReplayColumn::Kills), ship_end, "Kills sits beside the pinned edge");
            })
            .expect("the window is open");
            let frozen = cx.update(|cx| table.read(cx).frozen_columns());
            assert_eq!(frozen.contains(&ReplayColumn::Kills), pinned, "after {label}, Kills pinned is {pinned}");
        }
    }

    /// Two players on the reader's side, whose builds show, with captain
    /// details of different widths in Skills: player 2's is the wider.
    fn two_builds_model() -> ReplayReportModel {
        let mut model = two_row_model();
        for row in &mut model.rows {
            row.relation = Relation::new(1);
            let skills = if row.db_id == AccountId(1) {
                "Superintendent, Adrenaline"
            } else {
                "Concealment Expert, Adrenaline Rush, Priority"
            };
            row.skill_hover_text = Some(skills.to_owned());
        }
        model
    }

    /// Opens or closes `player`'s row by its caret.
    fn toggle_player(cx: &mut TestAppContext, window: gpui_kit::WindowHandle<PlayerTable>, player: i64) {
        let table = window.entity(cx).expect("the window has a root view");
        let ix = cx
            .update(|cx| table.read(cx).model.rows.iter().position(|row| row.db_id == AccountId(player)))
            .expect("the player has a row");
        cx.update_window(window.into(), |_, window, cx| {
            window.click(("replay-row-caret", ix), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");
    }

    /// Draws a frame `after` from now.
    fn draw_after(cx: &mut TestAppContext, window: gpui_kit::WindowHandle<PlayerTable>, after: std::time::Duration) {
        cx.executor().advance_clock(after);
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the window is open");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the window is open");
    }

    /// Lets every opening and closing finish.
    fn settle(cx: &mut TestAppContext, window: gpui_kit::WindowHandle<PlayerTable>) {
        draw_after(cx, window, std::time::Duration::from_secs(1));
    }

    /// The Skills column as drawn, and what each shown player's detail needs.
    fn skills_widths(cx: &mut TestAppContext, table: &gpui_kit::Entity<PlayerTable>) -> SkillsWidths {
        cx.update(|cx| {
            let table = table.read(cx);
            let needs = |player| table.detail_widths.get(&(AccountId(player), ReplayColumn::Skills)).copied();
            SkillsWidths {
                drawn: table.drawn_widths[ReplayColumn::Skills as usize],
                narrow: needs(1),
                wide: needs(2),
                set: table.width_overrides[ReplayColumn::Skills as usize],
            }
        })
    }

    struct SkillsWidths {
        drawn: gpui_kit::Pixels,
        narrow: Option<gpui_kit::Pixels>,
        wide: Option<gpui_kit::Pixels>,
        set: Option<gpui_kit::Pixels>,
    }

    /// A column the reader set narrow fits the widest detail shown in it:
    /// closing the widest of two open rows narrows it to the other's, and
    /// closing that one returns it to the width they set, which none of this
    /// changes.
    #[gpui_kit::test]
    fn a_narrowed_column_fits_the_widest_open_detail_until_all_close(cx: &mut TestAppContext) {
        let window = open_still_table(cx, two_builds_model());
        let table = window.entity(cx).expect("the window has a root view");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let grip = window.find(("replay-header-grip", ReplayColumn::Skills as usize)).bounds().center();
            window.drag(grip, gpui_kit::point(grip.x - px(60.), grip.y), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");
        let set = skills_widths(cx, &table).set.expect("the drag set a width");

        toggle_player(cx, window, 1);
        toggle_player(cx, window, 2);
        let both = skills_widths(cx, &table);
        let narrow = both.narrow.expect("player 1's build shows");
        let wide = both.wide.expect("player 2's build shows");
        assert!(set < narrow && narrow < wide, "the fixture needs {set:?} < {narrow:?} < {wide:?} to test this");
        assert!(both.drawn >= wide, "both open, the column fits the wider: drawn {:?}", both.drawn);

        toggle_player(cx, window, 2);
        settle(cx, window);
        let one = skills_widths(cx, &table);
        assert!(
            narrow <= one.drawn && one.drawn < wide,
            "with the wider closed, the column fits the one still open: drawn {:?}",
            one.drawn
        );

        toggle_player(cx, window, 1);
        settle(cx, window);
        let none = skills_widths(cx, &table);
        assert_eq!(none.drawn, set, "with both closed, the column returns to the reader's width");
        assert_eq!(none.set, Some(set), "which was never changed");
    }

    /// A row opened while another is still opening or closing: the column
    /// fits whichever detail shown is widest, a closing one included, and
    /// returns to its content's width once the last has closed.
    #[gpui_kit::test]
    fn a_column_fits_details_that_open_and_close_over_each_other(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1400.), px(600.)), |_window, cx| {
            let vfs: VfsPath = MemoryFS::new().into();
            PlayerTable::new(two_builds_model(), vfs, false, cx)
        });
        let table = window.entity(cx).expect("the window has a root view");
        let midway = std::time::Duration::from_millis(80);
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the window is open");
        let default = skills_widths(cx, &table).drawn;

        toggle_player(cx, window, 2);
        draw_after(cx, window, midway);
        toggle_player(cx, window, 1);
        let opening = skills_widths(cx, &table);
        let wide = opening.wide.expect("player 2's build shows");
        assert!(opening.drawn >= wide, "opened while the wider opens, the column fits the wider");
        settle(cx, window);
        let narrow = skills_widths(cx, &table).narrow.expect("player 1's build shows");
        assert!(default < narrow && narrow < wide, "the fixture needs {default:?} < {narrow:?} < {wide:?}");

        toggle_player(cx, window, 2);
        draw_after(cx, window, midway);
        assert!(skills_widths(cx, &table).drawn >= wide, "a closing detail keeps its width until it has closed");
        settle(cx, window);
        let one = skills_widths(cx, &table).drawn;
        assert!(narrow <= one && one < wide, "then the column fits the one still open: drawn {one:?}");

        toggle_player(cx, window, 1);
        draw_after(cx, window, midway);
        toggle_player(cx, window, 2);
        assert!(skills_widths(cx, &table).drawn >= wide, "opened while the other closes, the column fits it");
        settle(cx, window);
        assert!(skills_widths(cx, &table).drawn >= wide, "and still does once the other has closed");

        toggle_player(cx, window, 2);
        settle(cx, window);
        assert_eq!(skills_widths(cx, &table).drawn, default, "with none open, the column fits its content again");
    }

    /// A grip dragged while a detail widens its column moves from the width
    /// drawn, shows the width dragged to rather than the detail's, and that
    /// width is the reader's once the detail has closed.
    #[gpui_kit::test]
    fn a_drag_over_a_widened_column_sets_what_it_shows(cx: &mut TestAppContext) {
        let window = open_still_table(cx, two_builds_model());
        let table = window.entity(cx).expect("the window has a root view");
        let grip = ("replay-header-grip", ReplayColumn::Skills as usize);
        let drag_by = |cx: &mut TestAppContext, by: gpui_kit::Pixels| {
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                let from = window.find(grip).bounds().center();
                window.drag(from, gpui_kit::point(from.x + by, from.y), cx);
                window.render_frame(cx);
            })
            .expect("the window is open");
        };

        drag_by(cx, px(-60.));
        toggle_player(cx, window, 2);
        let widened = skills_widths(cx, &table).drawn;

        drag_by(cx, px(-30.));
        let dragged = skills_widths(cx, &table);
        assert_eq!(dragged.set, Some(widened - px(30.)), "the drag moved from the width drawn");
        assert_eq!(dragged.drawn, widened - px(30.), "and the column shows it with the detail still open");

        toggle_player(cx, window, 2);
        settle(cx, window);
        let closed = skills_widths(cx, &table);
        assert_eq!(closed.drawn, widened - px(30.), "closed, the column keeps the dragged width");
        assert_eq!(closed.set, Some(widened - px(30.)), "as the reader's own");
    }

    /// The last shown scrolling column cannot be pinned: neither the menu's
    /// pin nor dropping it in the zone pins it.
    #[gpui_kit::test]
    fn the_last_scrolling_column_cannot_be_pinned(cx: &mut TestAppContext) {
        let mut model = two_row_model();
        model.columns = vec![ReplayColumn::Name, ReplayColumn::ShipName, ReplayColumn::Kills];
        let window = open_still_table(cx, model);
        let table = window.entity(cx).expect("the window has a root view");
        let kills = ("replay-header", ReplayColumn::Kills as usize);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.right_click(kills, cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let label = rust_i18n::t!("ui.replay.context.pin_column").into_owned();
            // The kit's menu items do not report their disabled state to
            // the snapshot, so what is checked is that choosing it pins
            // nothing.
            let (row, _) = crate::interaction_tests::menu_items(window)
                .into_iter()
                .find(|(_, item)| item.label() == Some(label.as_str()))
                .expect("the header menu names the pin");
            window.within("popup-menu").click(gpui_kit::ElementId::Integer(row), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        assert!(!cx.update(|cx| table.read(cx).arrangement.is_pinned(ReplayColumn::Kills)), "the menu did not pin it");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let from = window.find(kills).bounds().center();
            let name = window.find(("replay-header", ReplayColumn::Name as usize)).bounds();
            window.drag(from, gpui_kit::point(name.left() + px(4.), from.y), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");
        assert_eq!(
            cx.update(|cx| table.read(cx).shown_columns().scrolling),
            vec![ReplayColumn::Kills],
            "nor did dropping it in the zone"
        );
    }

    /// A pointer that comes back with the button up ends a drag whose release
    /// went elsewhere: a column move is undone, and a width drag stops
    /// following the pointer.
    #[gpui_kit::test]
    fn a_drag_whose_release_was_missed_lets_go(cx: &mut TestAppContext) {
        use gpui_kit::InputEvent as _;

        let window = open_still_table(cx, two_row_model());
        let table = window.entity(cx).expect("the window has a root view");
        let before = cx.update(|cx| table.read(cx).shown_columns());
        let move_to = |window: &mut gpui_kit::Window,
                       to: gpui_kit::Point<gpui_kit::Pixels>,
                       pressed_button: Option<gpui_kit::MouseButton>,
                       cx: &mut gpui_kit::App| {
            let event =
                gpui_kit::MouseMoveEvent { position: to, pressed_button, modifiers: gpui_kit::Modifiers::default() };
            window.dispatch_event(event.to_platform_input(), cx);
            window.render_frame(cx);
        };
        let press = |window: &mut gpui_kit::Window, at: gpui_kit::Point<gpui_kit::Pixels>, cx: &mut gpui_kit::App| {
            move_to(window, at, None, cx);
            let event = gpui_kit::MouseDownEvent {
                button: gpui_kit::MouseButton::Left,
                position: at,
                modifiers: gpui_kit::Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            };
            window.dispatch_event(event.to_platform_input(), cx);
            window.render_frame(cx);
        };

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let from = window.find(("replay-header", ReplayColumn::Kills as usize)).bounds().center();
            let to = window.find(("replay-header", ReplayColumn::Skills as usize)).bounds().left() + px(2.);
            press(window, from, cx);
            move_to(window, gpui_kit::point(to, from.y), Some(gpui_kit::MouseButton::Left), cx);
            assert!(window.try_find("replay-column-ghost").is_some(), "the column is being carried");
            move_to(window, gpui_kit::point(to, from.y), None, cx);
            assert!(window.try_find("replay-column-ghost").is_none(), "with the button up, it is let go");
        })
        .expect("the window is open");
        assert_eq!(cx.update(|cx| table.read(cx).shown_columns()), before, "and the move is undone");

        let ship = ReplayColumn::ShipName as usize;
        let drawn_before = cx.update(|cx| table.read(cx).drawn_widths[ship]);
        cx.update_window(window.into(), |_, window, cx| {
            let grip = window.find(("replay-header-grip", ship)).bounds().center();
            press(window, grip, cx);
            move_to(window, gpui_kit::point(grip.x - px(20.), grip.y), Some(gpui_kit::MouseButton::Left), cx);
            move_to(window, gpui_kit::point(grip.x - px(20.), grip.y), None, cx);
            move_to(window, gpui_kit::point(grip.x - px(60.), grip.y), Some(gpui_kit::MouseButton::Left), cx);
        })
        .expect("the window is open");
        let (resizing, set) = cx.update(|cx| {
            let table = table.read(cx);
            (table.resizing.is_some(), table.width_overrides[ship])
        });
        assert!(!resizing, "the width drag ended with the button up");
        assert_eq!(set, Some(drawn_before - px(20.)), "and kept the width it had shown, not what followed");
    }

    /// A disconnect chip stays neutral under half a minute offline, deepens
    /// evenly to a minute and a half, and stays at full past it.
    #[test]
    fn a_disconnect_chip_deepens_from_thirty_to_ninety_seconds() {
        let depth = |secs| super::offline_depth(TimeOffline::new(std::time::Duration::from_secs(secs)));

        assert_eq!([depth(0), depth(29), depth(30)], [0.; 3]);
        assert!(0. < depth(45) && depth(45) < depth(60) && depth(60) < depth(89) && depth(89) < 1.);
        assert_eq!([depth(90), depth(400)], [1.; 2]);
    }

    /// A click on the disconnect chip puts its tooltip's text on the
    /// clipboard.
    #[gpui_kit::test]
    fn clicking_a_disconnect_chip_copies_its_tooltip(cx: &mut TestAppContext) {
        let note = ConnectionNote::Interrupted {
            events: vec!["disconnected @ 1:40".to_string(), "connected @ 2:52".to_string()],
            time_offline: Some(TimeOffline::new(std::time::Duration::from_secs(72))),
        };
        let mut model = two_row_model();
        model.rows[0].connection = Some(note.clone());
        let window = open_still_table(cx, model);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("replay-disconnect", 0usize), cx);
        })
        .expect("the window is open");

        let copied = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied, Some(super::connection_hover_text(&note)));
    }

    /// A figure reads in the same colour on every side: the team is the
    /// row's stripe and class icon, not its numbers.
    #[test]
    fn figures_read_the_same_on_every_side() {
        let row = |id, relation, is_self| super::PlayerRow {
            base_xp_text: Some("1,200".to_string()),
            actual_damage_text: Some("50,000".to_string()),
            received_damage_text: Some("20,000".to_string()),
            hits_text: Some("40".to_string()),
            ..base_row(id, Relation::new(relation), is_self)
        };
        let (owner, ally, enemy) = (row(1, 0, true), row(2, 1, false), row(3, 2, false));

        for col in [ReplayColumn::BaseXp, ReplayColumn::ActualDamage, ReplayColumn::ReceivedDamage, ReplayColumn::Hits]
        {
            let colors = [&owner, &ally, &enemy].map(|row| super::cell_value(row, col, false).color);
            assert_eq!(colors, [None; 3], "{col:?}");
        }
    }

    /// A bar is the row's figure against its own team's total in that
    /// column, the total the team heading shows. Hidden figures neither get a
    /// bar nor count toward the total; spotting, shown on every row, does.
    #[test]
    fn bars_are_each_rows_share_of_its_team_total() {
        let row = |id, team: i64, actual, spotting| super::PlayerRow {
            team_id: TeamId::from(team),
            actual_damage: Some(actual),
            spotting_damage: Some(spotting),
            ..base_row(id, Relation::new(2), false)
        };
        let hidden = super::PlayerRow { manual_stat_hide_toggle: true, ..row(4, 1, 90_000, 2_000) };
        let rows = [row(1, 0, 30_000, 2_000), row(2, 0, 10_000, 0), row(3, 1, 20_000, 6_000), hidden];
        let share = |side, fraction| Some(TeamShare { side, fraction });

        let totals = TeamTotals::of(&rows, TeamId::from(0i64), false);

        assert_eq!(
            totals.column(ReplayColumn::ActualDamage),
            TeamDamage { allies: Some(40_000), enemies: Some(20_000) }
        );
        assert_eq!(totals.share(&rows[0], ReplayColumn::ActualDamage, false), share(TeamSide::Allies, 0.75));
        assert_eq!(totals.share(&rows[1], ReplayColumn::ActualDamage, false), share(TeamSide::Allies, 0.25));
        assert_eq!(totals.share(&rows[2], ReplayColumn::ActualDamage, false), share(TeamSide::Enemies, 1.));
        assert_eq!(totals.share(&rows[3], ReplayColumn::ActualDamage, false), None);
        assert_eq!(totals.share(&rows[2], ReplayColumn::SpottingDamage, false), share(TeamSide::Enemies, 0.75));
        assert_eq!(totals.share(&rows[3], ReplayColumn::SpottingDamage, false), share(TeamSide::Enemies, 0.25));
        assert_eq!(totals.share(&rows[1], ReplayColumn::SpottingDamage, false), None, "no bar for nothing");
        assert_eq!(totals.share(&rows[0], ReplayColumn::Kills, false), None, "not a barred column");

        let debug = TeamTotals::of(&rows, TeamId::from(0i64), true);
        assert_eq!(debug.column(ReplayColumn::ActualDamage).enemies, Some(110_000));
    }
}
