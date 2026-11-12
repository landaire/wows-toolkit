//! A grid in a real document, driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::ColumnId;
use landaire_ui::ColumnLayout;
use landaire_ui::ColumnSizing;
use landaire_ui::ColumnSort;
use landaire_ui::ColumnWidth;
use landaire_ui::GridColumn;
use landaire_ui::GridProps;
use landaire_ui::GridSelectionMode;
use landaire_ui::MotionPreference;
use landaire_ui::RowGroups;
use landaire_ui::RowHeight;
use landaire_ui::RowHover;
use landaire_ui::RowMenu;
use landaire_ui::RowMenuHandle;
use landaire_ui::RowMode;
use landaire_ui::SortDirection;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollTarget;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui_ui::MenuItemProps;

const SIZE: WindowSize = WindowSize { width: 420.0, height: 300.0 };

#[derive(Clone, Debug, PartialEq)]
struct Item {
    id: u32,
    name: String,
    size: u32,
}

fn items(count: u32) -> Vec<Item> {
    (0..count).map(|id| Item { id, name: format!("item {id}"), size: id * 10 }).collect()
}

/// A name column, a sortable size column and three wide columns that make the grid scroll
/// sideways. Each cell carries a test id naming its column and row.
fn columns() -> Vec<GridColumn<Item>> {
    let mut columns = vec![
        GridColumn::new("name", "Name", |item: &Item| {
            view! { text(attr:data-testid = format!("name-{}", item.id)) {{item.name.clone()}} }
        })
        .sizing(ColumnSizing::Fixed(ColumnWidth(100.0)))
        .min_width(ColumnWidth(40.0))
        .text(|item| item.name.clone()),
        GridColumn::new("size", "Size", |item: &Item| {
            view! { text(attr:data-testid = format!("size-{}", item.id)) {{item.size.to_string()}} }
        })
        .sizing(ColumnSizing::Fixed(ColumnWidth(80.0)))
        .min_width(ColumnWidth(40.0))
        .sortable(true)
        .text(|item| item.size.to_string()),
    ];
    for extra in ["first", "second", "third"] {
        columns.push(
            GridColumn::new(extra, extra, |item: &Item| item.id.to_string())
                .sizing(ColumnSizing::Fixed(ColumnWidth(160.0))),
        );
    }

    columns
}

/// What a test reads back from a grid it opened.
#[derive(Default)]
struct Handles {
    port: Cell<Option<NodeRef>>,
    layout: Cell<Option<RwSignal<ColumnLayout, LocalStorage>>>,
}

fn open(rows: Vec<Item>, mode: impl Fn() -> RowMode<Item> + 'static, handles: Rc<Handles>) -> HeadlessWindow {
    open_with(rows, mode, handles, MotionPreference::Full, |layout| layout)
}

fn open_with(
    rows: Vec<Item>,
    mode: impl Fn() -> RowMode<Item> + 'static,
    handles: Rc<Handles>,
    motion: MotionPreference,
    arrange: impl Fn(ColumnLayout) -> ColumnLayout + 'static,
) -> HeadlessWindow {
    HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let port = NodeRef::new();
        let columns = columns();
        let layout = RwSignal::new_local(arrange(ColumnLayout::new(columns.iter().map(|column| column.id().clone()))));
        handles.port.set(Some(port));
        handles.layout.set(Some(layout));
        let rows = RwSignal::new_local(rows.clone());
        let mode = mode();

        view! {
            ThemeRoot(motion = motion) {
                box(style = "display: flex; width: 420px; height: 300px") {
                    Grid(
                        rows = rows,
                        columns = columns,
                        row_key = |item: &Item| item.id,
                        mode = mode,
                        layout = layout,
                        node_ref = port,
                        label = "Items"
                    )
                }
            }
        }
    })
    .expect("opens")
}

fn header(window: &mut HeadlessWindow, label: &str) -> Element {
    window.by_role_and_label(Role::ColumnHeader, label).expect("the header is announced")
}

fn left(window: &HeadlessWindow, element: Element) -> f32 {
    window.bounds(element).expect("the element has a box").origin.x.0
}

fn top(window: &HeadlessWindow, element: Element) -> f32 {
    window.bounds(element).expect("the element has a box").origin.y.0
}

fn width(window: &HeadlessWindow, element: Element) -> f32 {
    window.bounds(element).expect("the element has a box").size.width.0
}

fn cell(window: &HeadlessWindow, id: &str) -> Element {
    window.by_test_id(id).unwrap_or_else(|| panic!("{id} is built"))
}

fn ids(layout: &ColumnLayout) -> Vec<&str> {
    layout.ids().map(ColumnId::as_str).collect()
}

#[test]
fn a_column_dragged_narrower_than_its_minimum_stops_at_it() {
    let handles = Rc::new(Handles::default());
    let mut window = open(items(5), RowMode::natural, Rc::clone(&handles));
    let grip = window.by_role_and_label(Role::Splitter, "Resize Name").expect("the grip is announced");
    let from = window.bounds(grip).expect("the grip has a box");

    let to = Point::<DevicePx, Device>::new(DevicePx(from.origin.x.0 - 300.0), from.origin.y);
    window.drag(grip, to, 8).expect("the grip has a box");
    window.advance(Duration::from_millis(50));
    let name = header(&mut window, "Name");
    assert_eq!(width(&window, name), 40.0);
    assert_eq!(width(&window, cell(&window, "name-0")).min(40.0), width(&window, cell(&window, "name-0")));
    let layout = handles.layout.get().expect("the layout").get_untracked();
    assert_eq!(layout.width(&ColumnId::from("name")), Some(ColumnWidth(40.0)));
}

#[test]
fn a_pinned_column_dragged_wider_stops_at_the_pinned_share_of_the_view() {
    let handles = Rc::new(Handles::default());
    let mut window =
        open_with(items(5), RowMode::natural, Rc::clone(&handles), MotionPreference::Full, |mut layout| {
            layout.pin(&ColumnId::from("name"));
            layout
        });
    let grip = window.by_role_and_label(Role::Splitter, "Resize Name").expect("the grip is announced");
    let from = window.bounds(grip).expect("the grip has a box");

    let to = Point::<DevicePx, Device>::new(DevicePx(from.origin.x.0 + 400.0), from.origin.y);
    window.drag(grip, to, 8).expect("the grip has a box");
    window.advance(Duration::from_millis(50));

    // The pinned columns may take 60% of the 420px view, 252px.
    let name = header(&mut window, "Name");
    assert!(width(&window, name) < 252.5, "the pinned column is {} wide", width(&window, name));
    let layout = handles.layout.get().expect("the layout").get_untracked();
    let stored = layout.width(&ColumnId::from("name")).expect("the drag stored a width");
    assert!(stored.0 > 240.0 && stored.0 < 252.5, "the stored width is {}", stored.0);
}

#[test]
fn the_menu_key_opens_the_row_menu_at_the_start_of_the_row() {
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), || {
        let row_menu: RowMenu<u32> = Rc::new(|key: u32| AnyView::new(view! { MenuItem {{format!("Open {key}")}} }));
        view! {
            ThemeRoot {
                box(style = "display: flex; width: 420px; height: 300px") {
                    Grid(
                        rows = RwSignal::new_local(items(4)),
                        columns = columns(),
                        row_key = |item: &Item| item.id,
                        row_menu = row_menu,
                        label = "Items"
                    )
                }
            }
        }
    })
    .expect("opens");
    let row = cell(&window, "name-2");
    let row_box = window.bounds(row).expect("the cell has a box");

    window.click(row).expect("the cell has a box");
    window.press_key(NamedKey::F10, Modifiers::SHIFT);
    window.advance(Duration::from_millis(300));

    let item = window.by_role_and_label(Role::MenuItem, "Open 2").expect("the row menu is open");
    let item_box = window.bounds(item).expect("the item has a box");
    assert!(
        item_box.origin.x.0 < row_box.origin.x.0 + 40.0,
        "the menu opens at the row start: {}",
        item_box.origin.x.0
    );
    assert!(item_box.origin.y.0 >= row_box.origin.y.0 + row_box.size.height.0, "the menu opens under the row");
}

#[test]
fn a_row_menu_handle_opens_a_row_menu_from_code_and_tells_it_is_open() {
    type Given = Option<(RowMenuHandle<u32>, NodeRef)>;
    let handles: Rc<Cell<Given>> = Rc::new(Cell::new(None));
    let given = Rc::clone(&handles);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let row_menu: RowMenu<u32> = Rc::new(|key: u32| AnyView::new(view! { MenuItem {{format!("Open {key}")}} }));
        let handle = RowMenuHandle::new();
        let anchor = NodeRef::new();
        given.set(Some((handle, anchor)));
        view! {
            ThemeRoot {
                box(style = "display: flex; flex-direction: column; width: 420px; height: 300px") {
                    box(node_ref = anchor, style = "height: 20px") {}
                    Grid(
                        rows = RwSignal::new_local(items(4)),
                        columns = columns(),
                        row_key = |item: &Item| item.id,
                        row_menu = row_menu,
                        row_menu_handle = handle,
                        selection_mode = GridSelectionMode::Multiple,
                        label = "Items"
                    )
                }
            }
        }
    })
    .expect("opens");
    let (handle, anchor) = handles.get().expect("the grid was built");
    assert_eq!(handle.open_row(), None);

    handle.open(2, anchor);
    window.advance(Duration::from_millis(300));

    window.by_role_and_label(Role::MenuItem, "Open 2").expect("the row menu is open");
    assert_eq!(handle.open_row(), Some(2));
    let row = window.by_role(Role::Row)[3];
    assert_eq!(window.a11y(row).and_then(|state| state.selected), Some(true), "the row is selected");

    window.press_key(NamedKey::Escape, Modifiers::NONE);
    window.advance(Duration::from_millis(300));
    assert_eq!(handle.open_row(), None, "a closed menu leaves no row open");
}

#[test]
fn a_row_menu_handle_ignores_a_missing_row_and_closes_when_its_row_goes() {
    type Given = Option<(RowMenuHandle<u32>, NodeRef, RwSignal<Vec<Item>, LocalStorage>)>;
    let handles: Rc<Cell<Given>> = Rc::new(Cell::new(None));
    let given = Rc::clone(&handles);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let row_menu: RowMenu<u32> = Rc::new(|key: u32| AnyView::new(view! { MenuItem {{format!("Open {key}")}} }));
        let handle = RowMenuHandle::new();
        let anchor = NodeRef::new();
        let rows = RwSignal::new_local(items(4));
        given.set(Some((handle, anchor, rows)));
        view! {
            ThemeRoot {
                box(style = "display: flex; flex-direction: column; width: 420px; height: 300px") {
                    box(node_ref = anchor, style = "height: 20px") {}
                    Grid(
                        rows = rows,
                        columns = columns(),
                        row_key = |item: &Item| item.id,
                        row_menu = row_menu,
                        row_menu_handle = handle,
                        selection_mode = GridSelectionMode::Multiple,
                        label = "Items"
                    )
                }
            }
        }
    })
    .expect("opens");
    let (handle, anchor, rows) = handles.get().expect("the grid was built");

    handle.open(9, anchor);
    window.advance(Duration::from_millis(300));
    assert!(window.by_role_and_label(Role::MenuItem, "Open 9").is_none(), "no menu for a missing row");
    assert_eq!(handle.open_row(), None);
    let selected = window
        .by_role(Role::Row)
        .into_iter()
        .filter(|&row| window.a11y(row).and_then(|state| state.selected) == Some(true))
        .count();
    assert_eq!(selected, 0, "a missing row selects nothing");

    handle.open(2, anchor);
    window.advance(Duration::from_millis(300));
    assert_eq!(handle.open_row(), Some(2));

    rows.set(items(4).into_iter().filter(|item| item.id != 2).collect());
    window.advance(Duration::from_millis(300));
    assert_eq!(handle.open_row(), None, "the menu closes with its row");
}

#[test]
fn the_pointer_entering_and_leaving_a_row_is_told_once_per_row_with_an_anchor_beside_it() {
    #[derive(Default)]
    struct Told {
        events: RefCell<Vec<String>>,
        anchor: Cell<Option<NodeRef>>,
    }

    let told = Rc::new(Told::default());
    let given = Rc::clone(&told);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let given = Rc::clone(&given);
        let on_row_hover = UnsyncCallback::new(move |hover: RowHover<u32>| match hover {
            RowHover::Entered { key, anchor } => {
                given.events.borrow_mut().push(format!("entered {key}"));
                given.anchor.set(Some(anchor));
            }
            RowHover::Left { key } => given.events.borrow_mut().push(format!("left {key}")),
        });
        view! {
            ThemeRoot {
                box(style = "display: flex; width: 420px; height: 300px") {
                    Grid(
                        rows = RwSignal::new_local(items(4)),
                        columns = columns(),
                        row_key = |item: &Item| item.id,
                        on_row_hover = on_row_hover,
                        label = "Items"
                    )
                }
            }
        }
    })
    .expect("opens");

    window.hover(cell(&window, "name-1")).expect("the cell has a box");
    window.hover(cell(&window, "size-1")).expect("the cell has a box");
    assert_eq!(*told.events.borrow(), ["entered 1"], "moving along a row stays on it");

    let text = window.bounds(cell(&window, "name-1")).expect("the cell has a box");
    let next = window.bounds(cell(&window, "name-2")).expect("the cell has a box");
    let anchor = told.anchor.get().and_then(|anchor| anchor.window_bounds()).expect("the anchor has a box");
    let bottom = anchor.origin.y.0 + anchor.size.height.0;
    let spans = anchor.origin.y.0 <= text.origin.y.0 && text.origin.y.0 + text.size.height.0 <= bottom;
    assert!(spans, "the anchor spans the row");
    assert!(bottom <= next.origin.y.0, "and not the next");

    window.hover(cell(&window, "name-2")).expect("the cell has a box");
    assert_eq!(*told.events.borrow(), ["entered 1", "left 1", "entered 2"]);
}

#[test]
fn a_header_dropped_past_a_neighbour_moves_its_column_and_its_cells() {
    let handles = Rc::new(Handles::default());
    let mut window = open(items(5), RowMode::natural, Rc::clone(&handles));
    let name = header(&mut window, "Name");
    let size = header(&mut window, "Size");
    let size_box = window.bounds(size).expect("the header has a box");

    // The pointer lands a column before the first column whose middle lies beyond it. With the name
    // column taken out, the size column's middle is half its width from the leading edge and the
    // next column's is past the size column's end.
    let to =
        Point::<DevicePx, Device>::new(DevicePx(size_box.origin.x.0 + size_box.size.width.0 / 2.0), size_box.origin.y);
    window.drag(name, to, 10).expect("the header has a box");
    window.advance(Duration::from_millis(400));

    let layout = handles.layout.get().expect("the layout").get_untracked();
    assert_eq!(ids(&layout), ["size", "name", "first", "second", "third"]);
    assert!(left(&window, size) < left(&window, name), "the headers swapped");
    assert!(left(&window, cell(&window, "size-0")) < left(&window, cell(&window, "name-0")), "the cells followed");
}

#[test]
fn a_column_moved_from_the_keyboard_slides_unless_motion_is_reduced() {
    for (motion, slides) in [(MotionPreference::Full, true), (MotionPreference::Reduced, false)] {
        let handles = Rc::new(Handles::default());
        let mut window = open_with(items(3), RowMode::natural, Rc::clone(&handles), motion, |layout| layout);
        let name = header(&mut window, "Name");
        let size = header(&mut window, "Size");
        let size_before = left(&window, size);

        window.click(name).expect("the header has a box");
        window.press_key(NamedKey::ArrowRight, Modifiers::ALT);
        window.advance(Duration::from_millis(16));

        assert_eq!(window.running_animations(size) > 0, slides, "{motion:?}");
        window.advance(Duration::from_millis(300));
        assert!(left(&window, size) < size_before, "{motion:?}: the size column moved ahead");
        assert_eq!(window.running_animations(size), 0);
    }
}

#[test]
fn pinned_columns_hold_still_while_the_rest_scroll() {
    let handles = Rc::new(Handles::default());
    let mut window =
        open_with(items(5), RowMode::natural, Rc::clone(&handles), MotionPreference::Full, |mut layout| {
            layout.pin(&ColumnId::from("size"));
            layout
        });
    let name = header(&mut window, "Name");
    let size = header(&mut window, "Size");
    let before = [
        left(&window, size),
        left(&window, cell(&window, "size-2")),
        left(&window, name),
        left(&window, cell(&window, "name-2")),
    ];
    assert!(before[0] < before[2], "the pinned column leads");

    let port = handles.port.get().expect("the port");
    port.scroll_to(ScrollTarget::Offset(Point::new(DevicePx(150.0), DevicePx(0.0))), ScrollBehavior::Instant);
    window.advance(Duration::from_millis(50));

    assert_eq!(left(&window, size), before[0]);
    assert_eq!(left(&window, cell(&window, "size-2")), before[1]);
    assert_eq!(left(&window, name), before[2] - 150.0);
    assert_eq!(left(&window, cell(&window, "name-2")), before[3] - 150.0);
}

#[test]
fn pressing_a_sortable_header_asks_for_ascending_then_descending_then_none() {
    let asked: Rc<RefCell<Vec<Option<ColumnSort>>>> = Rc::default();
    let told = Rc::clone(&asked);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let sort: RwSignal<Option<ColumnSort>, LocalStorage> = RwSignal::new_local(None);
        let told = Rc::clone(&told);
        let on_sort = UnsyncCallback::new(move |next: Option<ColumnSort>| {
            told.borrow_mut().push(next.clone());
            sort.set(next);
        });
        view! {
            ThemeRoot {
                Grid(
                    rows = RwSignal::new_local(items(3)),
                    columns = columns(),
                    row_key = |item: &Item| item.id,
                    sort = sort,
                    on_sort = on_sort,
                    label = "Items"
                )
            }
        }
    })
    .expect("opens");
    let size = header(&mut window, "Size");
    let name = header(&mut window, "Name");

    window.click(name).expect("the header has a box");
    for _ in 0..3 {
        window.click(size).expect("the header has a box");
        window.advance(Duration::from_millis(16));
    }

    let column = ColumnId::from("size");
    let ascending = ColumnSort { column: column.clone(), direction: SortDirection::Ascending };
    let descending = ColumnSort { column, direction: SortDirection::Descending };
    assert_eq!(*asked.borrow(), [Some(ascending), Some(descending), None], "the name column does not sort");
}

#[test]
fn an_expanded_row_grows_into_its_detail_and_pushes_the_rows_below_down() {
    let handles = Rc::new(Handles::default());
    let detail = || {
        RowMode::with_detail(|item: &Item| {
            view! { box(attr:data-testid = format!("detail-{}", item.id), style = "height: 60px") {"detail"} }
        })
    };
    let mut window = open(items(4), detail, Rc::clone(&handles));
    let opened = cell(&window, "name-1");
    let below = top(&window, cell(&window, "name-2"));
    let above = top(&window, cell(&window, "name-0"));

    window.double_click(opened).expect("the cell has a box");
    window.advance(Duration::from_millis(50));
    let partway = top(&window, cell(&window, "name-2")) - below;
    window.advance(Duration::from_millis(400));
    let open = top(&window, cell(&window, "name-2")) - below;

    assert!(open >= 60.0, "the row below moved down by the detail ({open})");
    assert!(partway > 0.0 && partway < open, "the detail was revealed over time ({partway} of {open})");
    assert_eq!(top(&window, cell(&window, "name-0")), above, "the row above stayed");
    let row = window.by_role(Role::Row)[2];
    assert_eq!(window.a11y(row).and_then(|state| state.expanded), Some(true));

    window.double_click(opened).expect("the cell has a box");
    window.advance(Duration::from_millis(400));
    assert_eq!(top(&window, cell(&window, "name-2")), below, "the row below moved back");
    assert!(window.by_test_id("detail-1").is_none(), "a closed detail is taken down");
}

#[test]
fn a_group_heading_opens_each_run_and_follows_a_reorder() {
    let rows = RwSignal::new_local(items(5));
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let mode = RowMode::natural().grouped(RowGroups::new(
            |item: &Item| Some(item.id / 2),
            |item: &Item| view! { text(attr:data-testid = format!("group-{}", item.id / 2)) {"group"} },
        ));
        view! {
            ThemeRoot {
                box(style = "display: flex; width: 420px; height: 300px") {
                    Grid(
                        rows = rows,
                        columns = columns(),
                        row_key = |item: &Item| item.id,
                        mode = mode,
                        label = "Items"
                    )
                }
            }
        }
    })
    .expect("opens");

    assert_eq!(window.by_role(Role::RowHeader).len(), 3, "one heading per run");
    let heading = top(&window, cell(&window, "group-1"));
    assert!(heading < top(&window, cell(&window, "name-2")), "the heading sits over its run");
    assert!(heading > top(&window, cell(&window, "name-1")), "the heading sits under the run before");

    rows.set(vec![items(5)[0].clone(), items(5)[2].clone(), items(5)[1].clone()]);
    window.advance(Duration::from_millis(50));
    assert_eq!(window.by_role(Role::RowHeader).len(), 3, "a split run opens again under a heading");
    let headed = top(&window, cell(&window, "group-0"));
    assert!(headed < top(&window, cell(&window, "name-0")));
}

#[test]
fn a_fixed_height_grid_builds_only_the_rows_in_view() {
    let handles = Rc::new(Handles::default());
    let mut window = open(items(10_000), || RowMode::fixed(RowHeight(24.0)), Rc::clone(&handles));

    // The header row and the body rows.
    let built = window.by_role(Role::Row).len() - 1;
    assert!((10..=24).contains(&built), "{built} rows built for a view of about eleven");
    assert!(window.by_test_id("name-0").is_some());

    let port = handles.port.get().expect("the port");
    port.scroll_to(ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(24.0 * 5_000.0))), ScrollBehavior::Instant);
    window.advance(Duration::from_millis(50));

    assert!(window.by_test_id("name-5003").is_some(), "the rows scrolled to are built");
    assert!(window.by_test_id("name-0").is_none(), "the rows scrolled away from are not");
    let built = window.by_role(Role::Row).len() - 1;
    assert!(built <= 24, "{built} rows built after the scroll");
}

/// How tall row `id`'s tall cell is: 20, 45 or 70px.
fn tall(id: u32) -> f32 {
    20.0 + (id % 3) as f32 * 25.0
}

/// A name column and a column whose cells differ in height from row to row.
fn tall_columns() -> Vec<GridColumn<Item>> {
    vec![
        GridColumn::new("name", "Name", |item: &Item| {
            view! { text(attr:data-testid = format!("name-{}", item.id)) {{item.name.clone()}} }
        })
        .sizing(ColumnSizing::Fixed(ColumnWidth(100.0))),
        GridColumn::new("tall", "Tall", |item: &Item| {
            let style = format!("width: 10px; height: {}px", tall(item.id));
            view! { box(attr:data-testid = format!("tall-{}", item.id), style = style) {} }
        })
        .sizing(ColumnSizing::Fixed(ColumnWidth(80.0))),
    ]
}

/// What a test drives a measured grid through.
#[derive(Clone, Copy)]
struct Measured {
    rows: RwSignal<Vec<Item>, LocalStorage>,
    expanded: RwSignal<HashSet<u32>, LocalStorage>,
    port: NodeRef,
}

fn open_measured(
    count: u32,
    columns: fn() -> Vec<GridColumn<Item>>,
    mode: fn() -> RowMode<Item>,
) -> (HeadlessWindow, Measured) {
    let handed = Rc::new(Cell::new(None));
    let sent = Rc::clone(&handed);
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let measured = Measured {
            rows: RwSignal::new_local(items(count)),
            expanded: RwSignal::new_local(HashSet::new()),
            port: NodeRef::new(),
        };
        sent.set(Some(measured));
        view! {
            ThemeRoot(motion = MotionPreference::Reduced) {
                box(style = "display: flex; width: 420px; height: 300px") {
                    Grid(
                        rows = measured.rows,
                        columns = columns(),
                        row_key = |item: &Item| item.id,
                        mode = mode(),
                        expanded = measured.expanded,
                        node_ref = measured.port,
                        label = "Items"
                    )
                }
            }
        }
    })
    .expect("opens");

    (window, handed.get().expect("the grid was built"))
}

fn bottom(window: &HeadlessWindow, element: Element) -> f32 {
    let bounds = window.bounds(element).expect("the element has a box");

    bounds.origin.y.0 + bounds.size.height.0
}

/// The ids of the rows built, in row order.
fn built_ids(window: &HeadlessWindow, rows: &[Item]) -> Vec<u32> {
    rows.iter().map(|item| item.id).filter(|id| window.by_test_id(&format!("name-{id}")).is_some()).collect()
}

/// Each built row starts below the tall cell of the row before it, and not far below.
fn assert_rows_apart(window: &HeadlessWindow, built: &[u32]) {
    for pair in built.windows(2) {
        let gap = top(window, cell(window, &format!("tall-{}", pair[1])))
            - top(window, cell(window, &format!("tall-{}", pair[0])));
        let height = tall(pair[0]);
        assert!(gap >= height && gap <= height + 16.0, "row {} is {gap}px tall for a {height}px cell", pair[0]);
    }
}

#[test]
fn a_measured_grid_builds_the_rows_in_view_at_their_own_heights() {
    // An estimate well over every row's height, so rows built at it would not fill the view.
    let (mut window, measured) = open_measured(2_000, tall_columns, || RowMode::measured(RowHeight(120.0)));
    window.advance(Duration::from_millis(100));

    let rows = measured.rows.get_untracked();
    let built = built_ids(&window, &rows);
    assert!(built.len() < 40, "{} rows built for a short view", built.len());
    assert_eq!(built.first(), Some(&0));
    assert_rows_apart(&window, &built);
    let port = window.by_class("lui-grid__port").expect("the port is built");
    let view_bottom = bottom(&window, port);
    let last = *built.last().expect("rows are built");
    assert!(bottom(&window, cell(&window, &format!("tall-{last}"))) >= view_bottom, "the built rows fill the view");

    // The rows reached are measured on arrival and the end moves, so the end is asked for again.
    let end = Point::new(DevicePx(0.0), DevicePx(1.0e7));
    measured.port.scroll_to(ScrollTarget::Offset(end), ScrollBehavior::Instant);
    window.advance(Duration::from_millis(200));
    measured.port.scroll_to(ScrollTarget::Offset(end), ScrollBehavior::Instant);
    window.advance(Duration::from_millis(200));
    let end = cell(&window, "tall-1999");
    let end_bottom = bottom(&window, end);
    assert!(
        end_bottom <= view_bottom && end_bottom >= view_bottom - 40.0,
        "the last row ends the view at {end_bottom}"
    );
    assert!(window.by_test_id("name-0").is_none(), "the rows scrolled away from are not built");
}

#[test]
fn a_measured_grid_shows_a_reorder_from_its_first_row_at_each_row_s_height() {
    let (mut window, measured) = open_measured(300, tall_columns, || RowMode::measured(RowHeight(30.0)));
    window.advance(Duration::from_millis(100));

    let mut reversed = measured.rows.get_untracked();
    reversed.reverse();
    measured.rows.set(reversed.clone());
    window.advance(Duration::from_millis(100));

    let built = built_ids(&window, &reversed);
    assert_eq!(built.first(), Some(&299), "the reorder is shown from its first row");
    assert_rows_apart(&window, &built);
}

#[test]
fn a_measured_row_counts_its_open_detail_as_part_of_its_height() {
    let mode = || RowMode::Measured {
        estimate: RowHeight(24.0),
        overscan: 4,
        detail: Some(Rc::new(|item: &Item| {
            AnyView::new(view! { box(attr:data-testid = format!("detail-{}", item.id), style = "height: 120px") {} })
        })),
    };
    let (mut window, measured) = open_measured(200, columns, mode);
    window.advance(Duration::from_millis(100));
    let line = top(&window, cell(&window, "name-1")) - top(&window, cell(&window, "name-0"));

    // Row 0 above the view, and still built.
    let scrolled = Point::new(DevicePx(0.0), DevicePx(line * 2.5));
    measured.port.scroll_to(ScrollTarget::Offset(scrolled), ScrollBehavior::Instant);
    window.advance(Duration::from_millis(100));
    let held = top(&window, cell(&window, "name-5"));

    measured.expanded.set(HashSet::from([0]));
    window.advance(Duration::from_millis(100));
    assert!(window.by_test_id("detail-0").is_some(), "the detail is open");
    assert_eq!(top(&window, cell(&window, "name-5")), held, "a row growing above the view moves nothing in it");
}

#[test]
fn a_copy_holds_the_selected_rows_in_row_order() {
    let copied: Rc<RefCell<Option<String>>> = Rc::default();
    let told = Rc::clone(&copied);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let told = Rc::clone(&told);
        view! {
            ThemeRoot {
                Grid(
                    rows = RwSignal::new_local(items(4)),
                    columns = columns(),
                    row_key = |item: &Item| item.id,
                    selection_mode = GridSelectionMode::Multiple,
                    on_copy = UnsyncCallback::new(move |text: String| *told.borrow_mut() = Some(text)),
                    label = "Items"
                )
            }
        }
    })
    .expect("opens");

    window.click(cell(&window, "name-3")).expect("the cell has a box");
    window.click_with(cell(&window, "name-1"), Modifiers::PRIMARY).expect("the cell has a box");
    window.press_character('c', Modifiers::PRIMARY);

    assert_eq!(copied.borrow().as_deref(), Some("item 1\t10\nitem 3\t30"));
}
