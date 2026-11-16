//! One ship's minimum, maximum, total and average of each statistic.

use std::sync::Arc;

use landaire_ui::TABULAR_CLASS;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_viewmodel::stats::table::COLUMNS;
use wows_toolkit_viewmodel::stats::table::Cell;
use wows_toolkit_viewmodel::stats::table::Column;
use wows_toolkit_viewmodel::stats::table::ShipSummary;
use zgui::component;
use zgui::css;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

use crate::tabs::stats::rating::RatingChipProps;

const SHEET: &str = "wt-stats-ship-detail";

const CSS: &str = css!(
    ".wt-ship-detail {
        display: block;
        width: 100%;
        overflow-x: auto;
        overflow-y: hidden;
    }
    .wt-ship-detail__table {
        display: flex;
        flex-direction: column;
        min-width: 628px;
        padding: 4px 0;
    }
    .wt-ship-detail__row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        padding: 2px 8px;
    }
    .wt-ship-detail__head {
        font-size: 0.85em;
        font-weight: 700;
        padding: 4px 8px;
    }
    .wt-ship-detail__label {
        flex: none;
        width: 140px;
    }
    .wt-ship-detail__cell {
        display: flex;
        flex: none;
        width: 110px;
    }
    .wt-ship-detail__cell[data-average] { font-weight: 700; }
    .wt-ship-detail__row[data-stripe] { background-color: var(--lui-color-faint); }"
);

/// The table under a ship's row, from the same rows a copy writes.
#[component]
pub(super) fn ShipDetail(summary: Arc<ShipSummary>) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let ship = summary.ship_id;
    let headings: Vec<AnyView> = COLUMNS
        .iter()
        .map(|column| {
            AnyView::new(view! {
                text(class = "wt-ship-detail__cell") {{t!(column.translation_key()).into_owned()}}
            })
        })
        .collect();
    let rows: Vec<AnyView> = summary
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let cells: Vec<AnyView> = COLUMNS.iter().map(|column| cell(row.cell(*column), *column)).collect();
            AnyView::new(view! {
                row(
                    class = "wt-ship-detail__row",
                    attr:data-stripe = index.is_multiple_of(2).then(String::new),
                    attr:data-testid = format!("ship-row-{ship}-{:?}", row.label)
                ) {
                    text(class = "wt-ship-detail__label") {{t!(row.label.translation_key()).into_owned()}}
                    {cells}
                }
            })
        })
        .collect();

    view! {
        scroll(class = "wt-ship-detail", attr:data-testid = format!("ship-detail-{ship}")) {
            column(class = "wt-ship-detail__table", class = TABULAR_CLASS) {
                row(class = "wt-ship-detail__row", class = "wt-ship-detail__head") {
                    box(class = "wt-ship-detail__label") {}
                    {headings}
                }
                {rows}
            }
        }
    }
}

/// One figure. A rated figure sits on its band's chip, which names the band on hover.
fn cell(cell: &Cell, column: Column) -> AnyView {
    let average = (column == Column::Average).then(String::new);
    let text = cell.text.clone();

    match &cell.rating {
        Some(rating) => {
            let category = rating.category;
            let band = category.name().to_owned();
            AnyView::new(view! {
                box(class = "wt-ship-detail__cell", attr:data-average = average) {
                    Tooltip {
                        TooltipTrigger {
                            RatingChip(text = text, category = category)
                        }
                        TooltipContent {{band.clone()}}
                    }
                }
            })
        }
        None => AnyView::new(view! {
            text(class = "wt-ship-detail__cell", attr:data-average = average) {{text}}
        }),
    }
}
