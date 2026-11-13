//! Tooltips in a real document, driven over the headless platform.

#![cfg(feature = "test-support")]

use std::time::Duration;

use landaire_ui::TOOLTIP_DELAY;
use landaire_ui::ThemeRootProps;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::view;
use zgui::view::IntoView;

const SIZE: WindowSize = WindowSize { width: 320.0, height: 300.0 };

/// Past the delay and any exit animation.
const SETTLE: Duration = Duration::from_millis(150);

/// Long enough for a tooltip that was going to show to be showing.
fn past_delay() -> Duration {
    TOOLTIP_DELAY + SETTLE
}

/// One row with a tooltip.
#[component]
fn Row(name: &'static str) -> impl IntoView {
    view! {
        Tooltip {
            TooltipTrigger(style:display = "flex", style:height = "24px") {
                text(attr:data-testid = name, style = "flex: 1 1 auto") {{name}}
            }
            TooltipContent {{format!("about {name}")}}
        }
    }
}

/// Rows in a scroll container, a row holding a control with its own tooltip, a control at the
/// window's right edge, and one row at the foot of the window.
#[component]
fn Rows() -> impl IntoView {
    view! {
        ThemeRoot {
            column(style = "width: 100%; height: 100%") {
                scroll(attr:data-testid = "list", style = "height: 120px; flex: none") {
                    column {
                        Row(name = "first")
                        Row(name = "second")
                        Row(name = "third")
                        Row(name = "fourth")
                        Row(name = "fifth")
                        Row(name = "sixth")
                        Row(name = "seventh")
                    }
                }
                Tooltip {
                    TooltipTrigger(style:display = "flex", style:height = "24px") {
                        text(attr:data-testid = "outer", style = "flex: 1 1 auto") {"outer"}
                        Tooltip {
                            TooltipTrigger {
                                text(attr:data-testid = "inner", style = "width: 40px") {"inner"}
                            }
                            TooltipContent {"about inner"}
                        }
                    }
                    TooltipContent {"about outer"}
                }
                row(style = "height: 24px; flex: none") {
                    box(style = "flex: 1 1 auto")
                    Tooltip {
                        TooltipTrigger {
                            text(attr:data-testid = "edge", style = "width: 20px") {"edge"}
                        }
                        TooltipContent {"about the control at the edge"}
                    }
                }
                box(style = "flex: 1 1 auto")
                Row(name = "foot")
            }
        }
    }
}

fn open() -> HeadlessWindow {
    HeadlessWindow::open(HeadlessOptions::new(SIZE), || view! { Rows() }).expect("the window opens")
}

fn element(window: &HeadlessWindow, id: &str) -> Element {
    window.by_test_id(id).unwrap_or_else(|| panic!("{id} is built"))
}

fn shown(window: &HeadlessWindow) -> Vec<String> {
    window.texts_by_class("lui-tooltip")
}

/// The middle of `element`, on a whole pixel so a placement needs no rounding.
fn centre(window: &HeadlessWindow, element: Element) -> Point<DevicePx, Device> {
    let bounds = window.bounds(element).expect("the element has a box");

    Point::new(
        DevicePx((bounds.origin.x.0 + bounds.size.width.0 / 2.0).round()),
        DevicePx((bounds.origin.y.0 + bounds.size.height.0 / 2.0).round()),
    )
}

#[test]
fn a_tooltip_waits_for_the_delay() {
    let mut window = open();
    let first = element(&window, "first");

    window.hover(first).expect("hovers");
    window.advance(TOOLTIP_DELAY - Duration::from_millis(100));
    assert!(shown(&window).is_empty(), "nothing shows before the delay");

    window.advance(Duration::from_millis(200));
    assert_eq!(shown(&window), ["about first"]);
}

#[test]
fn moving_between_rows_shows_one_tooltip_after_a_fresh_delay() {
    let mut window = open();
    let first = element(&window, "first");
    let second = element(&window, "second");

    window.hover(first).expect("hovers");
    window.advance(past_delay());
    assert_eq!(shown(&window), ["about first"]);

    window.hover(second).expect("hovers");
    window.advance(SETTLE);
    assert!(shown(&window).is_empty(), "the first goes with the pointer and the second waits");

    window.advance(TOOLTIP_DELAY);
    assert_eq!(shown(&window), ["about second"]);
}

#[test]
fn a_press_closes_the_tooltip() {
    let mut window = open();
    let first = element(&window, "first");

    window.hover(first).expect("hovers");
    window.advance(past_delay());
    assert_eq!(shown(&window), ["about first"]);

    window.click(first).expect("clicks");
    window.advance(past_delay());
    assert!(shown(&window).is_empty(), "the press took it away and resting does not bring it back");
}

#[test]
fn the_wheel_closes_the_tooltip() {
    let mut window = open();
    let second = element(&window, "second");

    window.hover(second).expect("hovers");
    window.advance(past_delay());
    assert_eq!(shown(&window), ["about second"]);

    window.wheel(second, 24.0).expect("scrolls");
    window.advance(SETTLE);
    assert!(shown(&window).is_empty(), "scrolling took it away");
}

#[test]
fn a_tooltip_stands_just_past_the_pointer() {
    let mut window = open();
    let first = element(&window, "first");
    let pointer = centre(&window, first);

    window.hover_at(pointer);
    window.advance(past_delay());
    let tooltip = window.by_class("lui-tooltip").expect("the tooltip shows");
    let placed = window.bounds(tooltip).expect("the tooltip has a box");

    assert_eq!(placed.origin.x.0, pointer.x.0 + 1.0);
    assert_eq!(placed.origin.y.0, pointer.y.0 + 1.0);
}

#[test]
fn a_tooltip_at_the_foot_of_the_window_stands_above_the_pointer() {
    let mut window = open();
    let foot = element(&window, "foot");
    let pointer = centre(&window, foot);

    window.hover_at(pointer);
    window.advance(past_delay());
    let tooltip = window.by_class("lui-tooltip").expect("the tooltip shows");
    let placed = window.bounds(tooltip).expect("the tooltip has a box");

    assert_eq!(placed.origin.y.0 + placed.size.height.0, pointer.y.0 - 1.0);
}

#[test]
fn the_pointer_over_the_tooltip_keeps_its_row_hovered() {
    let mut window = open();
    let first = element(&window, "first");
    let pointer = centre(&window, first);

    window.hover_at(pointer);
    window.advance(past_delay());
    assert_eq!(shown(&window), ["about first"]);

    // Onto the label, which still lies over the row.
    window.hover_at(Point::new(DevicePx(pointer.x.0 + 4.0), DevicePx(pointer.y.0 + 4.0)));
    window.advance(past_delay());
    assert_eq!(shown(&window), ["about first"]);
}

#[test]
fn a_tooltip_at_the_right_edge_stands_left_of_the_pointer() {
    let mut window = open();
    let edge = element(&window, "edge");
    let pointer = centre(&window, edge);

    window.hover_at(pointer);
    window.advance(past_delay());
    let tooltip = window.by_class("lui-tooltip").expect("the tooltip shows");
    let placed = window.bounds(tooltip).expect("the tooltip has a box");

    assert_eq!(placed.origin.x.0 + placed.size.width.0, pointer.x.0 - 1.0);
    assert_eq!(placed.origin.y.0, pointer.y.0 + 1.0);
}

#[test]
fn a_control_inside_a_row_takes_over_the_rows_tooltip_and_hands_it_back() {
    let mut window = open();
    let outer = element(&window, "outer");
    let inner = element(&window, "inner");

    window.hover(outer).expect("hovers");
    window.advance(past_delay());
    assert_eq!(shown(&window), ["about outer"]);

    window.hover(inner).expect("hovers");
    window.advance(SETTLE);
    assert!(shown(&window).is_empty(), "the row's goes and the control's waits its own delay");
    window.advance(TOOLTIP_DELAY);
    assert_eq!(shown(&window), ["about inner"]);

    window.hover(outer).expect("hovers");
    window.advance(SETTLE);
    assert!(shown(&window).is_empty(), "the control's goes with the pointer");
    window.advance(TOOLTIP_DELAY);
    assert_eq!(shown(&window), ["about outer"], "the row's comes back after its delay");
}
