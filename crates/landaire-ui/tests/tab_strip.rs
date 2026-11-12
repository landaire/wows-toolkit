//! A tab strip driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::TabStripProps;
use landaire_ui::TabStripTabProps;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::IntoView;
use zgui::vocab::Role;

const SIZE: WindowSize = WindowSize { width: 600.0, height: 120.0 };
const SETTLE: Duration = Duration::from_millis(400);

/// What a test drives the strip through.
#[derive(Clone, Copy)]
struct Controls {
    value: RwSignal<String, LocalStorage>,
    first_label: RwSignal<String, LocalStorage>,
    closed: RwSignal<u32, LocalStorage>,
}

#[component]
fn Strip(controls: Controls) -> impl IntoView {
    let Controls { value, first_label, closed } = controls;
    let close = UnsyncCallback::new(move |()| closed.update(|closed| *closed += 1));

    view! {
        ThemeRoot {
            TabStrip(value = value, label = "Sections") {
                TabStripTab(value = "alpha") {{move || first_label.get()}}
                TabStripTab(value = "bravo") {"Bravo"}
                TabStripTab(value = "charlie", on_close = close, close_label = "Close Charlie") {"Charlie"}
            }
        }
    }
}

fn open_strip() -> (HeadlessWindow, Controls) {
    let handed = Rc::new(Cell::new(None));
    let sent = Rc::clone(&handed);
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let controls = Controls {
            value: RwSignal::new_local("alpha".to_owned()),
            first_label: RwSignal::new_local("Alpha".to_owned()),
            closed: RwSignal::new_local(0),
        };
        sent.set(Some(controls));
        view! { Strip(controls = controls) }
    })
    .expect("opens");

    (window, handed.get().expect("the view was built"))
}

/// Where the underline is, as its left edge and width in device pixels.
fn underline(window: &HeadlessWindow) -> (f32, f32) {
    let line = window.by_class("lui-tab-strip__underline").expect("the strip draws an underline");
    let bounds = window.bounds(line).expect("the underline has a box");

    (bounds.origin.x.0, bounds.size.width.0)
}

/// Where the underline rests under `tab`: inset 8px from either end.
fn under(window: &HeadlessWindow, tab: Element) -> (f32, f32) {
    let bounds = window.bounds(tab).expect("the tab has a box");

    (bounds.origin.x.0 + 8.0, bounds.size.width.0 - 16.0)
}

fn tabs(window: &mut HeadlessWindow) -> Vec<Element> {
    window.by_role(Role::Tab)
}

#[test]
fn the_underline_moves_from_where_it_is_to_the_tab_chosen() {
    let (mut window, controls) = open_strip();
    let [alpha, bravo, _charlie] = tabs(&mut window)[..] else {
        panic!("three tabs are announced");
    };
    assert_eq!(underline(&window), under(&window, alpha), "the first draw rests under the selected tab");

    window.click(bravo).expect("the tab has a box");
    assert_eq!(controls.value.get_untracked(), "bravo");
    window.advance(Duration::from_millis(100));
    let (moving, _) = underline(&window);
    let (from, _) = under(&window, alpha);
    let (to, _) = under(&window, bravo);
    assert!(from < moving && moving < to, "part way through, the line is between the tabs: {from} < {moving} < {to}");

    window.advance(SETTLE);
    assert_eq!(underline(&window), under(&window, bravo), "the move ends under the tab chosen");
}

#[test]
fn the_underline_follows_its_tab_at_once_when_the_tab_is_laid_out_elsewhere() {
    let (mut window, controls) = open_strip();
    let bravo = tabs(&mut window)[1];
    controls.value.set("bravo".to_owned());
    window.advance(SETTLE);
    let line = window.by_class("lui-tab-strip__underline").expect("the strip draws an underline");

    controls.first_label.set("Alpha, renamed to something much longer".to_owned());
    window.advance(Duration::from_millis(34));

    assert_eq!(window.running_animations(line), 0, "a relayout does not move the line by animation");
    assert_eq!(underline(&window), under(&window, bravo), "the line is under the tab where it now is");
}

#[test]
fn closing_a_tab_does_not_choose_it() {
    let (mut window, controls) = open_strip();
    let close = window.by_role_and_label(Role::Button, "Close Charlie").expect("the close button is announced");

    window.click(close).expect("the close button has a box");

    assert_eq!(controls.closed.get_untracked(), 1, "the tab was asked to close");
    assert_eq!(controls.value.get_untracked(), "alpha", "the selection stayed where it was");
}
