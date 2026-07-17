//! Custom virtualized player table (collapsed rows). Renders the M1
//! presentation model with a fixed sortable header and a `gpui::list`-backed
//! body sharing one horizontal scroll handle so header and rows stay aligned.
//! Row expansion, cell icons, and hover tooltips are later milestones; this
//! layer draws collapsed rows only.

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::h_flex;
use gpui_component::scroll::Scrollbar;
use gpui_component::v_flex;

use super::columns::BattleOutcome;
use super::columns::CaptainPointsTier;
use super::columns::CellValue;
use super::columns::ColorRole;
use super::columns::PlayerColorKind;
use super::columns::ReplayColumn;
use super::columns::cell_value;
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

/// ASCII caret shown after the active sort column's label. The egui app uses
/// an icon-font glyph; this port stays ASCII-only.
fn sort_caret(order: SortOrder) -> &'static str {
    match order {
        SortOrder::Asc(_) => "^",
        SortOrder::Desc(_) => "v",
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
/// the active sort order, and the horizontal scroll handle shared between the
/// header and the body.
pub struct PlayerTable {
    model: ReplayReportModel,
    list_state: ListState,
    sort: SortOrder,
    h_scroll: ScrollHandle,
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
        Self { model, list_state, sort, h_scroll: ScrollHandle::new(), debug }
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
                let mut label = column_label(col).to_string();
                if self.sort.column() == sort_column {
                    label.push(' ');
                    label.push_str(sort_caret(self.sort));
                }
                base.id(("replay-header", col as usize))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.sort_by(sort_column, cx);
                    }))
                    .child(label)
                    .into_any_element()
            }
        }
    }
}

/// One body cell: fixed-width, ellipsis-clipped, colored when the model gives
/// the cell a color role.
fn cell_element(cell: CellValue, width: f32) -> impl IntoElement {
    div()
        .w(px(width))
        .flex_none()
        .px_1()
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .when_some(cell.color, |el, role| el.text_color(resolve_color(role)))
        .child(cell.text)
}

/// One collapsed row: an `h_flex` of column cells with a hover highlight.
fn render_row(ix: usize, row: &PlayerRow, columns: &[ReplayColumn], debug: bool, hover_bg: Hsla) -> AnyElement {
    let mut row_el = h_flex().id(ix).w_full().py_0p5().hover(move |style| style.bg(hover_bg));
    for &col in columns {
        row_el = row_el.child(cell_element(cell_value(row, col, debug), column_width(col)));
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
            render_row(ix, &table.model.rows[ix], &table.model.columns, table.debug, hover_bg)
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
