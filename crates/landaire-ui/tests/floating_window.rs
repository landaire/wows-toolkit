//! A floating window driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::FloatingSize;
use landaire_ui::FloatingWindowProps;
use landaire_ui::Share;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::geom::Rect;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::IntoView;
use zgui::vocab::Role;

const SIZE: WindowSize = WindowSize { width: 800.0, height: 600.0 };

/// The container a test window is held in, and the share of its height the window may take.
#[derive(Clone, Copy)]
struct Room {
    size: FloatingSize,
    max_height_percent: f64,
}

const ROOM: Room = Room { size: FloatingSize { width: 800.0, height: 600.0 }, max_height_percent: 80.0 };

#[component]
fn Host(open: RwSignal<bool, LocalStorage>, room: Room) -> impl IntoView {
    let close = UnsyncCallback::new(move |()| open.set(false));
    let max_height = Share::new(room.max_height_percent).expect("a percentage");

    view! {
        ThemeRoot {
            box(
                style = "position: relative",
                style:width = format!("{}px", room.size.width),
                style:height = format!("{}px", room.size.height)
            ) {
                FloatingWindow(
                    open = open,
                    on_close = close,
                    title = "Details",
                    default_size = {FloatingSize { width: 400.0, height: 300.0 }},
                    max_height = max_height,
                    attr:data-testid = "window"
                ) {
                    box(attr:data-testid = "content", style = "height: 40px")
                }
            }
        }
    }
}

fn open_host() -> (HeadlessWindow, RwSignal<bool, LocalStorage>) {
    open_room(ROOM)
}

fn open_room(room: Room) -> (HeadlessWindow, RwSignal<bool, LocalStorage>) {
    let handed = Rc::new(Cell::new(None));
    let sent = Rc::clone(&handed);
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let open = RwSignal::new_local(true);
        sent.set(Some(open));
        view! { Host(open = open, room = room) }
    })
    .expect("opens");

    (window, handed.get().expect("the view was built"))
}

fn bounds(window: &HeadlessWindow, element: Element) -> Rect<DevicePx, Device> {
    window.bounds(element).expect("the element has a box")
}

fn point(x: f32, y: f32) -> Point<DevicePx, Device> {
    Point::new(DevicePx(x), DevicePx(y))
}

fn title_bar_point(window: &HeadlessWindow) -> Point<DevicePx, Device> {
    let frame = bounds(window, window.by_test_id("window").expect("the window is open"));

    point(frame.origin.x.0 + frame.size.width.0 / 2.0, frame.origin.y.0 + 15.0)
}

/// Drags the grip named `name` by `dx`, `dy` from its middle.
fn drag_grip(window: &mut HeadlessWindow, name: &str, dx: f32, dy: f32) {
    let grip = window.by_attribute("data-grip", name).expect("the grip is drawn");
    let at = bounds(window, grip);
    let from = point(at.origin.x.0 + at.size.width.0 / 2.0, at.origin.y.0 + at.size.height.0 / 2.0);

    window.drag_from(from, point(from.x.0 + dx, from.y.0 + dy), 4);
}

#[test]
fn it_opens_at_its_default_size_in_the_middle_of_its_container() {
    let (window, _open) = open_host();
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));

    assert_eq!((frame.size.width.0, frame.size.height.0), (400.0, 300.0));
    assert_eq!((frame.origin.x.0, frame.origin.y.0), (200.0, 150.0));
}

#[test]
fn dragging_the_title_bar_moves_it_but_never_out_of_its_container() {
    let (mut window, _open) = open_host();
    let from = title_bar_point(&window);

    window.drag_from(from, point(from.x.0 - 50.0, from.y.0 + 30.0), 4);
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert_eq!((frame.origin.x.0, frame.origin.y.0), (150.0, 180.0), "the window follows the pointer");

    let from = title_bar_point(&window);
    window.drag_from(from, point(790.0, 590.0), 4);
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert_eq!((frame.origin.x.0, frame.origin.y.0), (400.0, 300.0), "the window stops at the container's corner");
    assert_eq!(frame.size.width.0, 400.0, "moving does not resize it");
}

#[test]
fn a_corner_resizes_it_within_its_limits() {
    let (mut window, _open) = open_host();

    drag_grip(&mut window, "south-east", 100.0, 150.0);
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert_eq!(frame.size.width.0, 500.0, "the width follows the pointer");
    assert_eq!(frame.size.height.0, 450.0, "the height stops at the container's edge");

    drag_grip(&mut window, "north", 0.0, -145.0);
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert_eq!(frame.size.height.0, 480.0, "the height is held to its share of the container");
    assert_eq!(frame.origin.y.0 + frame.size.height.0, 600.0, "the bottom edge stays where it was");

    drag_grip(&mut window, "north-west", 596.0, 446.0);
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert_eq!((frame.size.width.0, frame.size.height.0), (160.0, 80.0), "it keeps its minimum size");
    assert_eq!(
        (frame.origin.x.0 + frame.size.width.0, frame.origin.y.0 + frame.size.height.0),
        (700.0, 600.0),
        "the opposite corner stays where it was"
    );
}

#[test]
fn collapsing_folds_it_to_its_title_bar_and_closing_asks_the_caller() {
    let (mut window, open) = open_host();
    let collapse = window.by_role_and_label(Role::Button, "Collapse").expect("the collapse button is announced");

    window.click(collapse).expect("the button has a box");
    assert!(window.by_test_id("content").is_none(), "a folded window hides its content");
    let frame = bounds(&window, window.by_test_id("window").expect("the window is still open"));
    assert!(frame.size.height.0 < 40.0, "a folded window is its title bar, {} high", frame.size.height.0);

    let expand = window.by_role_and_label(Role::Button, "Expand").expect("the button now offers to expand");
    window.click(expand).expect("the button has a box");
    assert!(window.by_test_id("content").is_some(), "the content is back");

    let close = window.by_role_and_label(Role::Button, "Close").expect("the close button is announced");
    window.click(close).expect("the button has a box");
    assert!(!open.get_untracked(), "the close button ran the caller's callback");
    assert!(window.by_test_id("window").is_none(), "a shut window is dropped");

    open.set(true);
    window.advance(Duration::from_millis(16));
    assert!(window.by_test_id("content").is_some(), "it opens again with its content");
}

#[test]
fn a_drag_the_window_closed_during_does_not_carry_on_once_it_reopens() {
    let (mut window, open) = open_host();
    let from = title_bar_point(&window);
    window.press_at(from);
    window.hover_at(point(from.x.0 - 20.0, from.y.0));

    open.set(false);
    window.advance(Duration::from_millis(16));
    window.release();
    open.set(true);
    window.advance(Duration::from_millis(16));
    let before = bounds(&window, window.by_test_id("window").expect("the window is open again"));

    let over_bar = title_bar_point(&window);
    window.hover_at(over_bar);
    window.hover_at(point(over_bar.x.0 + 150.0, over_bar.y.0 + 100.0));
    window.advance(Duration::from_millis(16));
    let after = bounds(&window, window.by_test_id("window").expect("the window is open"));

    assert_eq!(after.origin, before.origin, "a pointer moving with no button held does not move the window");
}

#[test]
fn a_folded_window_moves_down_to_the_container_edge_and_unfolds_inside_it() {
    let (mut window, _open) = open_host();
    let collapse = window.by_role_and_label(Role::Button, "Collapse").expect("the collapse button is announced");
    window.click(collapse).expect("the button has a box");
    window.advance(Duration::from_millis(16));

    let from = title_bar_point(&window);
    window.drag_from(from, point(from.x.0, 599.0), 4);
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert_eq!(frame.origin.y.0 + frame.size.height.0, 600.0, "the folded bar reaches the bottom edge");

    let expand = window.by_role_and_label(Role::Button, "Expand").expect("the button offers to expand");
    window.click(expand).expect("the button has a box");
    window.advance(Duration::from_millis(16));
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert_eq!(frame.size.height.0, 300.0, "it unfolds to the size it had");
    assert_eq!(frame.origin.y.0 + frame.size.height.0, 600.0, "and stays inside its container");
}

#[test]
fn a_container_smaller_than_the_minimum_holds_the_window_inside_it() {
    let (window, _open) =
        open_room(Room { size: FloatingSize { width: 120.0, height: 60.0 }, max_height_percent: 100.0 });
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));

    assert_eq!((frame.origin.x.0, frame.origin.y.0), (0.0, 0.0));
    assert!(frame.size.width.0 <= 120.0, "the minimum width gives way, at {}", frame.size.width.0);
    assert!(frame.size.height.0 <= 60.0, "the minimum height gives way, at {}", frame.size.height.0);
}

#[test]
fn a_max_height_below_the_minimum_limits_the_height() {
    let (mut window, _open) = open_room(Room { max_height_percent: 10.0, ..ROOM });
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert_eq!(frame.size.height.0, 60.0, "a tenth of the container, below the 80 pixel minimum");

    drag_grip(&mut window, "south", 0.0, 200.0);
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert_eq!(frame.size.height.0, 60.0, "resizing does not pass it either");
}

#[component]
fn FollowingHost(content: RwSignal<f32, LocalStorage>) -> impl IntoView {
    let max_height = Share::new(50.0).expect("a percentage");

    view! {
        ThemeRoot {
            box(style = "position: relative; width: 800px; height: 600px") {
                FloatingWindow(
                    open = true,
                    on_close = UnsyncCallback::new(|()| {}),
                    title = "Details",
                    default_size = {FloatingSize { width: 400.0, height: 250.0 }},
                    max_height = max_height,
                    height_follows_content = true,
                    attr:data-testid = "window"
                ) {
                    box(attr:data-testid = "content", style:height = move || Some(format!("{}px", content.get())))
                }
            }
        }
    }
}

#[test]
fn a_window_that_follows_its_content_is_as_high_as_it_until_its_height_is_resized() {
    let handed = Rc::new(Cell::new(None));
    let sent = Rc::clone(&handed);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let content = RwSignal::new_local(40.0_f32);
        sent.set(Some(content));
        view! { FollowingHost(content = content) }
    })
    .expect("opens");
    let content = handed.get().expect("the view was built");
    window.advance(Duration::from_millis(50));
    let height = |window: &HeadlessWindow| bounds(window, window.by_test_id("window").expect("the window is open")).size.height.0;
    let chrome = height(&window) - 40.0;
    assert!(chrome > 0.0 && chrome < 60.0, "the window is its content and its title bar high, {chrome}px more");
    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert_eq!(frame.origin.y.0, (600.0 - frame.size.height.0) / 2.0, "it is centred at the height it takes");

    content.set(120.0);
    window.advance(Duration::from_millis(50));
    assert_eq!(height(&window), 120.0 + chrome, "it grows with its content");

    content.set(900.0);
    window.advance(Duration::from_millis(50));
    assert_eq!(height(&window), 300.0, "it stops at its share of the container");

    content.set(40.0);
    window.advance(Duration::from_millis(50));
    assert_eq!(height(&window), 40.0 + chrome, "it shrinks with its content");
    drag_grip(&mut window, "south", 0.0, 100.0);
    window.advance(Duration::from_millis(50));
    assert_eq!(height(&window), 140.0 + chrome, "the grip resizes it from the height it had");
    content.set(20.0);
    window.advance(Duration::from_millis(50));
    assert_eq!(height(&window), 140.0 + chrome, "once resized, it keeps the height given");
}

#[test]
fn a_window_that_follows_its_content_folds_to_its_title_bar() {
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let content = RwSignal::new_local(200.0_f32);
        view! { FollowingHost(content = content) }
    })
    .expect("opens");
    window.advance(Duration::from_millis(50));
    let collapse = window.by_role_and_label(Role::Button, "Collapse").expect("the collapse button is announced");

    window.click(collapse).expect("the button has a box");
    window.advance(Duration::from_millis(50));

    let frame = bounds(&window, window.by_test_id("window").expect("the window is open"));
    assert!(frame.size.height.0 < 40.0, "only the title bar shows: {}px", frame.size.height.0);
}
