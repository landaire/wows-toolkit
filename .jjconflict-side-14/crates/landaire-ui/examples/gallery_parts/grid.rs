//! The gallery's grid section: a grid with its first column pinned, a resized column, a sort and
//! two rows selected. A double click opens a row.

use landaire_ui::ColumnAlign;
use landaire_ui::ColumnId;
use landaire_ui::ColumnLayout;
use landaire_ui::ColumnSizing;
use landaire_ui::ColumnSort;
use landaire_ui::ColumnWidth;
use landaire_ui::GridColumn;
use landaire_ui::GridProps;
use landaire_ui::GridSelectionMode;
use landaire_ui::RowMode;
use landaire_ui::Selection;
use landaire_ui::SortDirection;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::test_support::WindowSize;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;

use crate::Section;

pub const SECTION: Section =
    Section { name: "grid", title: "Grid", page: WindowSize { width: 1280.0, height: 340.0 }, view };

const SHEET: &str = css!(
    ".gallery-grid { display: flex; width: 560px; height: 236px; border: 1px solid var(--lui-color-border); }
     .gallery-grid__dim { color: var(--lui-color-text-dim); }"
);

/// One running program, as a task manager lists it.
#[derive(Clone, Debug, PartialEq)]
struct Process {
    pid: u32,
    name: &'static str,
    user: &'static str,
    cpu: f32,
    memory: u32,
    threads: u32,
    state: &'static str,
}

fn view() -> AnyView {
    install_stylesheet("landaire-ui-gallery-grid", SHEET);

    let all = vec![
        Process { pid: 412, name: "compositor", user: "root", cpu: 3.2, memory: 182, threads: 14, state: "Running" },
        Process { pid: 1088, name: "indexer", user: "daemon", cpu: 11.7, memory: 640, threads: 6, state: "Running" },
        Process { pid: 2210, name: "editor", user: "alice", cpu: 0.4, memory: 1210, threads: 31, state: "Sleeping" },
        Process {
            pid: 2231,
            name: "language-server",
            user: "alice",
            cpu: 7.9,
            memory: 905,
            threads: 18,
            state: "Running",
        },
        Process { pid: 3007, name: "backup", user: "root", cpu: 0.0, memory: 54, threads: 2, state: "Stopped" },
        Process { pid: 3150, name: "browser", user: "alice", cpu: 22.5, memory: 2480, threads: 64, state: "Running" },
        Process { pid: 3302, name: "mail", user: "alice", cpu: 0.1, memory: 310, threads: 9, state: "Sleeping" },
    ];
    let initial = Some(ColumnSort { column: ColumnId::from("cpu"), direction: SortDirection::Descending });
    let rows = RwSignal::new_local(sorted(all.clone(), initial.as_ref()));
    let sort = RwSignal::new_local(initial);
    let on_sort = UnsyncCallback::new(move |next: Option<ColumnSort>| {
        rows.set(sorted(all.clone(), next.as_ref()));
        sort.set(next);
    });

    let columns = vec![
        GridColumn::new("name", "Name", |process: &Process| process.name)
            .sortable(true)
            .text(|process| process.name.to_owned()),
        GridColumn::new(
            "pid",
            "PID",
            |process: &Process| view! { text(class = TABULAR_CLASS) {{process.pid.to_string()}} },
        )
        .align(ColumnAlign::End)
        .sortable(true),
        GridColumn::new("user", "User", |process: &Process| process.user),
        GridColumn::new(
            "cpu",
            "CPU %",
            |process: &Process| view! { text(class = TABULAR_CLASS) {{format!("{:.1}", process.cpu)}} },
        )
        .align(ColumnAlign::End)
        .sortable(true),
        GridColumn::new(
            "memory",
            "Memory",
            |process: &Process| view! { text(class = TABULAR_CLASS) {{format!("{} MB", process.memory)}} },
        )
        .align(ColumnAlign::End)
        .sizing(ColumnSizing::Fixed(ColumnWidth(96.0)))
        .sortable(true),
        GridColumn::new("threads", "Threads", |process: &Process| process.threads.to_string()).align(ColumnAlign::End),
        GridColumn::new("state", "State", |process: &Process| process.state),
    ];
    let mut arranged = ColumnLayout::new(columns.iter().map(|column| column.id().clone()));
    arranged.pin(&ColumnId::from("name"));
    arranged.set_width(&ColumnId::from("user"), Some(ColumnWidth(72.0)));
    let layout = RwSignal::new_local(arranged);
    let mut chosen = Selection::new();
    chosen.select_only(3150);
    chosen.toggle(2231);
    let selection = RwSignal::new_local(chosen);
    let detail = RowMode::with_detail(|process: &Process| {
        view! {
            text(class = "gallery-grid__dim") {{format!("{} threads, {} MB resident, owned by {}", process.threads, process.memory, process.user)}}
        }
    });

    AnyView::new(view! {
        box(class = "gallery-grid") {
            Grid(
                rows = rows,
                columns = columns,
                row_key = |process: &Process| process.pid,
                mode = detail,
                layout = layout,
                sort = sort,
                on_sort = on_sort,
                selection_mode = GridSelectionMode::Multiple,
                selection = selection,
                label = "Processes"
            )
        }
    })
}

/// `rows` in the order `sort` asks for.
fn sorted(mut rows: Vec<Process>, sort: Option<&ColumnSort>) -> Vec<Process> {
    let Some(sort) = sort else {
        return rows;
    };

    rows.sort_by(|a, b| {
        let order = match sort.column.as_str() {
            "name" => a.name.cmp(b.name),
            "pid" => a.pid.cmp(&b.pid),
            "cpu" => a.cpu.total_cmp(&b.cpu),
            "memory" => a.memory.cmp(&b.memory),
            _ => std::cmp::Ordering::Equal,
        };
        match sort.direction {
            SortDirection::Ascending => order,
            SortDirection::Descending => order.reverse(),
        }
    });

    rows
}
