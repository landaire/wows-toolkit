//! Custom virtualized player table (collapsed rows). Renders the M1
//! presentation model with a fixed sortable header and a `gpui::list`-backed
//! body sharing one horizontal scroll handle so header and rows stay aligned.
//! This layer adds the per-column cell parity pass on top of Task 4's
//! scaffold: colors, NDA text, the multi-segment Name cell (ship-class icon,
//! division, clan tag, player name), Skills tier icons, and hover tooltips
//! for the stat cells that carry a breakdown. Row expansion is a later
//! milestone; this layer still draws collapsed rows only.

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
use super::icons::IconCache;
use super::model::PlayerRow;
use super::model::ReplayReportModel;
use super::sort::SortColumn;
use super::sort::SortOrder;
use super::sort::sort_rows;
use wows_replay_insights::personal_rating::PersonalRatingCategory;

/// Overdraw for the virtualized list: how far past the viewport to render so
/// scrolling reveals already-laid-out rows instead of blank space.
const LIST_OVERDRAW: Pixels = px(200.);

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
fn resolve_color(role: ColorRole) -> Hsla {
    let packed = match role {
        ColorRole::Player(kind) => match kind {
            PlayerColorKind::SelfPlayer => 0xffffff,
            PlayerColorKind::Ally => 0x90ee90,
            PlayerColorKind::Enemy => 0xff8080,
            PlayerColorKind::DivisionMate => 0xffd700,
            PlayerColorKind::Abuser => 0xffc0cb,
        },
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
    /// Decoded ship-class icons for the Name cell. Empty until a later
    /// milestone's replay-loading pipeline feeds it real `GameAsset` bytes;
    /// `name_cell` falls back to a plain species-text label while empty.
    icons: IconCache,
    /// Debug mode lifts NDA hiding and the enemy-only Skills gate, mirroring
    /// the egui app's debug flag. Always `false` until a settings toggle wires
    /// it in a later milestone.
    debug: bool,
}

impl PlayerTable {
    pub fn new(mut model: ReplayReportModel, _cx: &mut Context<Self>) -> Self {
        let sort = SortOrder::default();
        let debug = false;
        sort_rows(&mut model.rows, model.self_team, sort, debug);
        let list_state = ListState::new(model.rows.len(), ListAlignment::Top, LIST_OVERDRAW);
        Self { model, list_state, sort, h_scroll: ScrollHandle::new(), icons: IconCache::new(), debug }
    }

    /// Applies a header click: toggles the sort order for `column`, re-sorts
    /// the rows in place, and resets the list to the new row count.
    fn sort_by(&mut self, column: SortColumn, cx: &mut Context<Self>) {
        self.sort.update_column(column);
        sort_rows(&mut self.model.rows, self.model.self_team, self.sort, self.debug);
        self.list_state.reset(self.model.rows.len());
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
        Tooltip::element(move |_window, _cx| {
            let text = text.clone();
            v_flex()
                .gap_0()
                .text_xs()
                .font_family("Consolas")
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
        Some(text) => {
            base.id(("replay-cell", ix * 32 + col as usize)).tooltip(hover_tooltip(text.into())).into_any_element()
        }
        None => base.into_any_element(),
    }
}

/// The Name column's cell: ship-class icon (or a plain species-text label
/// when no icon is cached yet) + division label + colored clan tag + colored
/// player name. Bypasses `cell_value`'s single joined-string Name arm because
/// each segment needs its own color (the clan tag uses the clan-league color,
/// not the player color; abuser pink applies only to the name, not the
/// icon/division/clan segments), mirroring the egui app's `cell_content_ui`
/// `ReplayColumn::Name` arm, which makes one separate `ui.add`/`ui.label`
/// call per segment.
fn name_cell(ix: usize, row: &PlayerRow, icons: &IconCache, width: f32) -> AnyElement {
    let icon_color = resolve_color(ColorRole::Player(player_color_kind(row)));
    let name_color = resolve_color(ColorRole::Player(name_color_kind(row)));

    let mut cell = h_flex().w(px(width)).flex_none().gap_1().px_1().items_center().overflow_hidden();

    cell = match icons.get(row.ship_class) {
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
        None => cell.child(div().flex_none().text_xs().text_color(icon_color).child(row.ship_species_text.clone())),
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
            .id(("replay-cell", ix * 32 + ReplayColumn::Skills as usize))
            .tooltip(hover_tooltip(text.into()))
            .into_any_element(),
        None => base.into_any_element(),
    }
}

/// One collapsed row: an `h_flex` of column cells with a hover highlight.
fn render_row(
    ix: usize,
    row: &PlayerRow,
    columns: &[ReplayColumn],
    icons: &IconCache,
    debug: bool,
    hover_bg: Hsla,
) -> AnyElement {
    let mut row_el = h_flex().id(ix).w_full().py_0p5().hover(move |style| style.bg(hover_bg));
    for &col in columns {
        let cell_el = match col {
            ReplayColumn::Name => name_cell(ix, row, icons, column_width(col)),
            ReplayColumn::Skills => skills_cell(ix, row, debug, column_width(col)),
            _ => cell_element(ix, col, cell_value(row, col, debug), column_width(col)),
        };
        row_el = row_el.child(cell_el);
    }
    row_el.into_any_element()
}

impl Render for PlayerTable {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let total_width: f32 = self.model.columns.iter().map(|col| column_width(*col)).sum();
        let border = cx.theme().border;
        let hover_bg = cx.theme().muted;

        let header = h_flex()
            .w(px(total_width))
            .flex_none()
            .border_b_1()
            .border_color(border)
            .children(self.model.columns.clone().into_iter().map(|col| self.header_cell(col, cx)));

        let entity = cx.entity();
        let render_item = move |ix: usize, _window: &mut Window, cx: &mut App| -> AnyElement {
            let table = entity.read(cx);
            render_row(ix, &table.model.rows[ix], &table.model.columns, &table.icons, table.debug, hover_bg)
        };

        div()
            .size_full()
            .relative()
            .child(
                div().id("replay-table-h-scroll").size_full().overflow_x_scroll().track_scroll(&self.h_scroll).child(
                    v_flex()
                        .h_full()
                        .w(px(total_width))
                        .child(header)
                        .child(list(self.list_state.clone(), render_item).flex_1()),
                ),
            )
            .child(Scrollbar::vertical(&self.list_state))
    }
}
