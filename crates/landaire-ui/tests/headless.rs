//! Headless windows themselves, as the tests that use them rely on them.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

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
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::IntoView;

const SIZE: WindowSize = WindowSize { width: 240.0, height: 160.0 };

/// A button that counts its clicks and a count that runs up to a figure set from outside.
#[component]
fn Counter(clicks: RwSignal<u64, LocalStorage>, target: RwSignal<u64, LocalStorage>) -> impl IntoView {
    let shown = count_up::<u64>(target.into());

    view! {
        ThemeRoot {
            column {
                box(
                    attr:data-testid = "count",
                    style = "width: 40px; height: 20px",
                    on:click = move |_| clicks.update(|clicks| *clicks += 1)
                )
                box(
                    attr:data-testid = "clicks",
                    style = "width: 10px; height: 10px",
                    a11y:label = move || format!("clicks {}", clicks.get())
                )
                box(
                    attr:data-testid = "shown",
                    style = "width: 10px; height: 10px",
                    a11y:label = move || format!("shown {}", shown.get())
                )
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Controls {
    clicks: RwSignal<u64, LocalStorage>,
    target: RwSignal<u64, LocalStorage>,
}

fn open_counter() -> (HeadlessWindow, Controls) {
    let handed = Rc::new(Cell::new(None));
    let sent = Rc::clone(&handed);
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let controls = Controls { clicks: RwSignal::new_local(0), target: RwSignal::new_local(0) };
        sent.set(Some(controls));
        view! { Counter(clicks = controls.clicks, target = controls.target) }
    })
    .expect("opens");

    (window, handed.get().expect("the view was built"))
}

fn label(window: &mut HeadlessWindow, id: &str) -> String {
    let element = window.by_test_id(id).expect("the box is built");

    window.a11y(element).and_then(|state| state.label).expect("the box is announced")
}

fn click_count(window: &mut HeadlessWindow) {
    let button = window.by_test_id("count").expect("the button is built");
    window.click(button).expect("the button has a box");
}

#[test]
fn two_windows_on_one_thread_are_open_and_driven_at_once() {
    let (mut first, first_controls) = open_counter();
    let (mut second, second_controls) = open_counter();

    click_count(&mut first);
    click_count(&mut second);
    click_count(&mut second);
    assert_eq!(label(&mut first, "clicks"), "clicks 1");
    assert_eq!(label(&mut second, "clicks"), "clicks 2");

    first_controls.target.set(10);
    second_controls.target.set(20);
    first.advance(Duration::from_secs(2));
    second.advance(Duration::from_secs(2));
    assert_eq!(label(&mut first, "shown"), "shown 10");
    assert_eq!(label(&mut second, "shown"), "shown 20");

    drop(second);
    first_controls.target.set(30);
    first.advance(Duration::from_secs(2));
    assert_eq!(label(&mut first, "shown"), "shown 30", "the first window is driven after the second closes");
}

#[test]
fn a_window_opens_after_every_window_on_its_thread_has_closed() {
    let (first, _) = open_counter();
    let (second, _) = open_counter();
    drop(first);
    drop(second);

    let (mut third, _) = open_counter();
    click_count(&mut third);
    assert_eq!(label(&mut third, "clicks"), "clicks 1");
}
