//! Headless windows themselves, as the tests that use them rely on them.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::ThemeRootProps;
use landaire_ui::count_up;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::geom::CssPx;
use zgui::geom::Size;
use zgui::prelude::CloseResponse;
use zgui::prelude::on_close_request;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::IntoView;
use zgui::vocab::GesturePhase;
use zgui::vocab::ScrollDelta;
use zgui::vocab::ScrollPhase;

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

/// A window whose application refuses to close.
#[component]
fn Refusing(asked: Rc<Cell<u32>>) -> impl IntoView {
    let guard = on_close_request(move || {
        asked.set(asked.get() + 1);
        CloseResponse::Veto
    });
    on_cleanup_local(move || drop(guard));

    view! { ThemeRoot { box(style = "width: 10px; height: 10px") } }
}

#[test]
fn a_close_request_closes_a_window_unless_the_application_refuses() {
    let asked = Rc::new(Cell::new(0));
    let handed = Rc::clone(&asked);
    let mut refusing = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        view! { Refusing(asked = Rc::clone(&handed)) }
    })
    .expect("opens");

    refusing.request_close().expect("the window has a surface");
    assert_eq!(asked.get(), 1, "the application was asked");
    assert!(refusing.is_open(), "the application refused");
    drop(refusing);

    let (mut closing, _) = open_counter();
    closing.request_close().expect("the window has a surface");
    assert!(!closing.is_open());
    assert!(closing.request_close().is_err(), "a closed window has nothing to ask");
}

/// One gesture stage a [`Gestures`] box heard.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Heard {
    Scroll { phase: ScrollPhase, delta: ScrollDelta },
    Pinch { phase: GesturePhase, scale_delta: f32 },
}

/// A box that records the trackpad scrolls and pinches over it.
#[component]
fn Gestures(heard: Rc<RefCell<Vec<Heard>>>) -> impl IntoView {
    let scrolled = Rc::clone(&heard);

    view! {
        ThemeRoot {
            box(
                attr:data-testid = "surface",
                style = "width: 100px; height: 100px",
                // `ev.phase` is the dispatch phase of the event context, so the payload is named.
                on:wheel = move |ev| {
                    let wheel = **ev;
                    scrolled.borrow_mut().push(Heard::Scroll { phase: wheel.phase, delta: wheel.delta });
                },
                on:pinch = move |ev| {
                    let pinch = **ev;
                    heard.borrow_mut().push(Heard::Pinch { phase: pinch.phase, scale_delta: pinch.scale_delta });
                }
            )
        }
    }
}

#[test]
fn a_trackpad_scroll_and_a_pinch_arrive_in_their_stages() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let handed = Rc::clone(&heard);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        view! { Gestures(heard = Rc::clone(&handed)) }
    })
    .expect("opens");
    let surface = window.by_test_id("surface").expect("the box is built");

    let left = Size::new(CssPx(-30.0), CssPx(0.0));
    let stopped = Size::new(CssPx(0.0), CssPx(0.0));
    window.trackpad_scroll(surface, &[left, left]).expect("the box has a box");
    window.scroll_stage(surface, left, ScrollPhase::Momentum).expect("the box has a box");
    window.scroll_stage(surface, stopped, ScrollPhase::Ended).expect("the box has a box");
    window.pinch(surface, &[0.5]).expect("the box has a box");

    let still = ScrollDelta::Pixels(stopped);
    let moved = ScrollDelta::Pixels(left);
    assert_eq!(
        *heard.borrow(),
        [
            Heard::Scroll { phase: ScrollPhase::Started, delta: still },
            Heard::Scroll { phase: ScrollPhase::Moved, delta: moved },
            Heard::Scroll { phase: ScrollPhase::Moved, delta: moved },
            Heard::Scroll { phase: ScrollPhase::Ended, delta: still },
            Heard::Scroll { phase: ScrollPhase::Momentum, delta: moved },
            Heard::Scroll { phase: ScrollPhase::Ended, delta: still },
            Heard::Pinch { phase: GesturePhase::Started, scale_delta: 0.0 },
            Heard::Pinch { phase: GesturePhase::Moved, scale_delta: 0.5 },
            Heard::Pinch { phase: GesturePhase::Ended, scale_delta: 0.0 },
        ]
    );
}
