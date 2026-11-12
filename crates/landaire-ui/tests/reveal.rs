//! A reveal and a count driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::CountFormat;
use landaire_ui::CountUpProps;
use landaire_ui::MotionPreference;
use landaire_ui::RevealProps;
use landaire_ui::ThemeRootProps;
use landaire_ui::count_up;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::IntoView;

const SIZE: WindowSize = WindowSize { width: 320.0, height: 400.0 };
const SETTLE: Duration = Duration::from_millis(400);

#[derive(Clone, Copy)]
struct Controls {
    open: RwSignal<bool, LocalStorage>,
    height: RwSignal<u32, LocalStorage>,
    count: RwSignal<u64, LocalStorage>,
    motion: RwSignal<MotionPreference, LocalStorage>,
}

#[component]
fn Revealed(controls: Controls) -> impl IntoView {
    let Controls { open, height, count, motion } = controls;

    view! {
        ThemeRoot(motion = motion) {
            column {
                Reveal(open = open) {
                    box(attr:data-testid = "content", style:height = move || Some(format!("{}px", height.get())))
                }
                Counted(count = count)
            }
        }
    }
}

/// A box announced as the figure a count shows for `count`.
#[component]
fn Counted(count: RwSignal<u64, LocalStorage>) -> impl IntoView {
    let shown = count_up::<u64>(count.into());

    view! {
        box(
            attr:data-testid = "count",
            style = "width: 10px; height: 10px",
            a11y:label = move || format!("count {}", shown.get())
        )
    }
}

fn open_revealed() -> (HeadlessWindow, Controls) {
    let handed = Rc::new(Cell::new(None));
    let sent = Rc::clone(&handed);
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let controls = Controls {
            open: RwSignal::new_local(false),
            height: RwSignal::new_local(100),
            count: RwSignal::new_local(0),
            motion: RwSignal::new_local(MotionPreference::Full),
        };
        sent.set(Some(controls));
        view! { Revealed(controls = controls) }
    })
    .expect("opens");

    (window, handed.get().expect("the view was built"))
}

fn shown_count(window: &mut HeadlessWindow) -> u64 {
    let count = window.by_test_id("count").expect("the count is built");
    let label = window.a11y(count).and_then(|state| state.label).expect("the count is announced");

    label.trim_start_matches("count ").parse().expect("the label ends in a figure")
}

fn revealed_height(window: &HeadlessWindow) -> f32 {
    let reveal = window.by_class("lui-reveal").expect("the reveal is built");

    window.bounds(reveal).expect("the reveal has a box").size.height.0
}

#[test]
fn a_reveal_opens_to_its_content_follows_it_and_closes_to_nothing() {
    let (mut window, controls) = open_revealed();
    assert_eq!(revealed_height(&window), 0.0);
    assert!(window.by_test_id("content").is_none(), "shut content is not built");

    controls.open.set(true);
    window.advance(Duration::from_millis(100));
    let opening = revealed_height(&window);
    assert!(opening > 0.0 && opening < 100.0, "part way open at {opening}");

    window.advance(SETTLE);
    assert_eq!(revealed_height(&window), 100.0);
    controls.height.set(150);
    window.advance(Duration::from_millis(17));
    assert_eq!(revealed_height(&window), 150.0, "open, it takes whatever height its content takes at once");

    controls.open.set(false);
    window.advance(Duration::from_millis(100));
    let closing = revealed_height(&window);
    assert!(closing > 0.0 && closing < 150.0, "part way shut at {closing}");

    window.advance(SETTLE);
    assert_eq!(revealed_height(&window), 0.0);
    assert!(window.by_test_id("content").is_none(), "shut content is dropped");
}

#[test]
fn a_count_rises_to_each_new_figure_and_drops_at_once() {
    let (mut window, controls) = open_revealed();

    controls.count.set(40);
    window.advance(Duration::from_millis(200));
    let rising = shown_count(&mut window);
    assert!(rising > 0 && rising < 40, "part way through, the count is between its figures at {rising}");

    window.advance(SETTLE);
    assert_eq!(shown_count(&mut window), 40);

    controls.count.set(3);
    window.advance(Duration::from_millis(17));
    assert_eq!(shown_count(&mut window), 3, "a smaller figure is a new count, shown at once");
}

#[test]
fn reducing_motion_ends_a_count_and_a_close_under_way() {
    let (mut window, controls) = open_revealed();
    controls.open.set(true);
    window.advance(SETTLE);

    controls.count.set(40);
    controls.open.set(false);
    window.advance(Duration::from_millis(100));
    assert!(shown_count(&mut window) < 40, "the count is still rising");
    assert!(revealed_height(&window) > 0.0, "the reveal is still closing");

    controls.motion.set(MotionPreference::Reduced);
    window.advance(Duration::from_millis(17));
    assert_eq!(shown_count(&mut window), 40, "the count is at its figure");
    assert_eq!(revealed_height(&window), 0.0, "the reveal is shut");
    assert!(window.by_test_id("content").is_none(), "its content is dropped");
}

/// A percentage counted from nothing when it is first shown.
#[component]
fn Share(share: RwSignal<f64, LocalStorage>) -> impl IntoView {
    view! {
        ThemeRoot {
            CountUp(value = share, from = 0.0, format = CountFormat::percent(1), attr:data-testid = "share")
        }
    }
}

fn shown_share(window: &mut HeadlessWindow) -> String {
    let share = window.by_test_id("share").expect("the share is built");

    window.a11y(share).and_then(|state| state.label).expect("the share is announced")
}

#[test]
fn a_decimal_count_runs_from_its_start_and_is_written_by_its_format() {
    let handed = Rc::new(Cell::new(None));
    let sent = Rc::clone(&handed);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let share = RwSignal::new_local(0.5);
        sent.set(Some(share));
        view! { Share(share = share) }
    })
    .expect("opens");
    let share = handed.get().expect("the view was built");

    let started: f64 = shown_share(&mut window).trim_end_matches('%').parse().expect("a percentage");
    assert!(started < 50.0, "the count starts below its figure at {started}");

    window.advance(SETTLE);
    assert_eq!(shown_share(&mut window), "50.0%");

    share.set(0.625);
    window.advance(Duration::from_millis(100));
    let rising: f64 = shown_share(&mut window).trim_end_matches('%').parse().expect("a percentage");
    assert!(rising > 50.0 && rising < 62.5, "part way through, the count is between its figures at {rising}");

    window.advance(SETTLE);
    assert_eq!(shown_share(&mut window), "62.5%");
}
