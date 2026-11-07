use std::rc::Rc;
use std::sync::Arc;

use nsh_base::DataTableColumn;
use nsh_base::DataTableEvent;
use nsh_base::DataTablePress;
use nsh_base::DataTableRange;
use nsh_base::DataTableRows;
use nsh_base::DataTableSort;
use nsh_base::DataTableSortDirection;
use nsh_base::data_table;
use nsh_core::prelude::*;

use crate::replays::ReplayColumn;
use crate::replays::ReplayTable;
use crate::replays::RowId;
use crate::replays::SortDirection;
use crate::replays::SortSpec;

pub fn replays_tab() -> impl IntoElement {
    match ReplayTable::load_fixtures() {
        Ok(table) => loaded(table).into_element(),
        Err(error) => text(format!("Replay fixtures did not load: {error}")).into_element(),
    }
}

/// Display order; equality by content keeps redundant sorts from re-rendering.
#[derive(Clone, PartialEq)]
struct Order(Rc<Vec<RowId>>);

fn loaded(table: Arc<ReplayTable>) -> impl IntoElement {
    let sort = signal(None::<SortSpec>);
    let order = signal(Order(Rc::new(table.order(None))));
    let selected = signal(None::<RowId>);

    let summary = format!("Rows: {} ({} replay files)", table.rows.len(), table.source_files);
    if !table.failed_files.is_empty() {
        log::warn!("{} fixture replays failed to parse", table.failed_files.len());
    }

    let rows_table = Arc::clone(&table);
    let first_table = Arc::clone(&table);
    let detail_table = Arc::clone(&table);
    let sort_table = Arc::clone(&table);

    let grid = data_table()
        .flex_1()
        .columns(columns())
        .data(move || rows(&rows_table, &order.get().0))
        .sort(move || sort.get().map(to_table_sort))
        .stripe(true)
        .on_sort_change(move |next: &Option<DataTableSort>| {
            let spec = next.as_ref().and_then(from_table_sort);
            log::info!("sort change -> {spec:?}");
            sort.set(spec);
            order.set(Order(Rc::new(sort_table.order(spec))));
        })
        // Two-field events arrive as the whole enum, not as the two arguments the docs show.
        .on_column_resize(|event: &DataTableEvent| {
            if let DataTableEvent::ColumnResize(key, width) = event {
                log::info!("column {key} resized to {}", width.0);
            }
        })
        .on_visible_rows_change(|range: &DataTableRange| log::info!("visible rows {}..{}", range.start, range.end))
        .on_row_press(move |press: &DataTablePress| {
            let id = order.with_untracked(|order| order.0.get(press.row as usize).copied());
            selected.set(id);
        });

    view()
        .flex_1()
        .gap(px(8.0))
        .p(px(8.0))
        .child(text(summary))
        .child(text(move || {
            let first = order.with(|order| order.0.first().copied());
            match first {
                Some(id) => format!("First row: {}", first_table.row(id).ship),
                None => "First row: none".to_owned(),
            }
        }))
        .child(
            view()
                .flex_1()
                .flex_row()
                .gap(px(8.0))
                .child(view().flex_1().child(grid))
                .child(detail(detail_table, selected)),
        )
}

fn detail(table: Arc<ReplayTable>, selected: Signal<Option<RowId>>) -> impl IntoElement {
    view().w(px(320.0)).p(px(12.0)).gap(px(6.0)).rounded(px(6.0)).border_1().border_color(rgb(0x5a6b7d)).child(dynamic(
        move || {
            let Some(id) = selected.get() else {
                return vec![text("Press a row to see its details").into_element()];
            };

            let row = table.row(id);
            vec![
                text(format!("Row id {}", id.0)).font_weight(FontWeight::BOLD).into_element(),
                text(format!("Ship: {}", row.ship)).into_element(),
                text(format!("Map: {}", row.map)).into_element(),
                text(format!("Mode: {}", row.game_mode)).into_element(),
                text(format!("Player: {}", row.player)).into_element(),
                text(format!("Date: {}", row.date_raw)).into_element(),
                text(format!("Version: {}", row.version)).into_element(),
                text(format!("Duration: {} s", row.duration.as_secs())).into_element(),
                text(format!("Vehicles: {}", row.vehicle_count)).into_element(),
                text(format!("File: {}", row.file_name)).into_element(),
            ]
        },
    ))
}

fn columns() -> Vec<DataTableColumn> {
    ReplayColumn::ALL
        .into_iter()
        .map(|column| {
            let mut def = DataTableColumn::default();
            def.key = column.key().into();
            def.title = Some(column.title().into());
            def.sortable = true;
            def.resizable = Some(true);
            def.width = Some(px(if column == ReplayColumn::Date { 170.0 } else { 140.0 }));
            def
        })
        .collect()
}

fn rows(table: &ReplayTable, order: &[RowId]) -> DataTableRows {
    let mut rows = DataTableRows::new(order.len());
    for column in ReplayColumn::ALL {
        rows = rows.texts(column.key(), order.iter().map(|id| table.row(*id).field(column)));
    }

    rows
}

fn to_table_sort(spec: SortSpec) -> DataTableSort {
    let mut sort = DataTableSort::default();
    sort.key = spec.column.key().into();
    sort.direction = match spec.direction {
        SortDirection::Ascending => DataTableSortDirection::Ascending,
        SortDirection::Descending => DataTableSortDirection::Descending,
    };

    sort
}

fn from_table_sort(sort: &DataTableSort) -> Option<SortSpec> {
    let column = ReplayColumn::from_key(&sort.key)?;
    let direction = match sort.direction {
        DataTableSortDirection::Ascending => SortDirection::Ascending,
        DataTableSortDirection::Descending => SortDirection::Descending,
    };

    Some(SortSpec { column, direction })
}
