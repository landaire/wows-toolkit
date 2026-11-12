//! A side split driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::PanelWidth;
use landaire_ui::SideEdge;
use landaire_ui::SidePanel;
use landaire_ui::SideSplitProps;
use landaire_ui::ThemeRootProps;
use landaire_ui::WidthRange;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::IntoView;
use zgui::vocab::Role;

const SIZE: WindowSize = WindowSize { width: 600.0, height: 300.0 };
const RANGE: WidthRange = WidthRange { min: PanelWidth(120.0), max: PanelWidth(520.0) };
const SETTLE: Duration = Duration::from_millis(400);

/// A box that counts clicks on it, announced as its count.
#[component]
fn Counter() -> impl IntoView {
    let count = RwSignal::new_local(0_u32);

    view! {
        box(
            attr:data-testid = "count",
            style = "width: 40px; height: 40px",
            a11y:label = move || format!("count {}", count.get()),
            on:click = move |_| count.update(|count| *count += 1)
        )
    }
}

#[component]
fn Split(open: RwSignal<bool, LocalStorage>, edge: SideEdge) -> impl IntoView {
    let width = RwSignal::new_local(PanelWidth(400.0));

    view! {
        ThemeRoot {
            SideSplit(open = open, width = width, range = RANGE, edge = edge, resize_label = "Resize panel") {
                SidePanel(slot) {
                    box(attr:data-testid = "panel", style = "flex: 1 1 auto")
                }
                column(attr:data-testid = "main", style = "flex: 1 1 auto") {
                    Counter()
                }
            }
        }
    }
}

fn open_split(edge: SideEdge, open: bool) -> (HeadlessWindow, RwSignal<bool, LocalStorage>) {
    let signal = Rc::new(Cell::new(None));
    let handed = Rc::clone(&signal);
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let open = RwSignal::new_local(open);
        handed.set(Some(open));
        view! { Split(open = open, edge = edge) }
    })
    .expect("opens");

    (window, signal.get().expect("the view was built"))
}

fn width_of(window: &HeadlessWindow, element: Element) -> f32 {
    window.bounds(element).expect("the element has a box").size.width.0
}

#[test]
fn dragging_the_edge_leaves_the_main_area_its_minimum() {
    for edge in [SideEdge::Left, SideEdge::Right] {
        let (mut window, _open) = open_split(edge, true);
        let splitter = window.by_role_and_label(Role::Splitter, "Resize panel").expect("the edge is announced");
        let panel = window.by_test_id("panel").expect("the panel is open");
        let main = window.by_test_id("main").expect("the main area is built");
        assert_eq!(width_of(&window, main), 200.0, "{edge:?}: the main area gives up the panel's width");

        let far = match edge {
            SideEdge::Left => 595.0,
            SideEdge::Right => 5.0,
        };
        window.drag(splitter, Point::<DevicePx, Device>::new(DevicePx(far), DevicePx(150.0)), 4).expect("drags");
        window.advance(Duration::from_millis(32));

        assert_eq!(width_of(&window, main), 160.0, "{edge:?}: the main area keeps its minimum");
        assert_eq!(width_of(&window, panel), 439.0, "{edge:?}: the panel, less its rule, takes the rest");
    }
}

#[test]
fn the_main_area_keeps_its_state_and_lays_out_once_per_toggle() {
    let (mut window, open) = open_split(SideEdge::Left, false);
    let count = window.by_test_id("count").expect("the counter is built");
    let main = window.by_test_id("main").expect("the main area is built");
    window.click(count).expect("the counter has a box");
    assert_eq!(width_of(&window, main), 600.0);

    open.set(true);
    window.advance(Duration::from_millis(80));
    assert_eq!(width_of(&window, main), 600.0, "the main area keeps its width while the panel moves");
    let panel = window.by_test_id("panel").expect("the panel is built as it opens");
    let left = window.bounds(panel).expect("the panel has a box").origin.x.0;
    assert!(left > -400.0 && left < 0.0, "part way through the slide the panel is part way in, at {left}");

    window.advance(SETTLE);
    assert_eq!(width_of(&window, main), 200.0, "at rest open, the main area gives up the panel's width");

    open.set(false);
    window.advance(Duration::from_millis(16));
    assert_eq!(width_of(&window, main), 600.0, "a close gives the width back as it starts");
    window.advance(SETTLE);
    assert!(window.by_test_id("panel").is_none(), "a shut panel is dropped");

    assert_eq!(window.by_test_id("count"), Some(count), "the main area is the same element throughout");
    window.click(count).expect("the counter has a box");
    let label = window.a11y(count).and_then(|state| state.label);
    assert_eq!(label.as_deref(), Some("count 2"), "what the main area held survived the toggle");
}
