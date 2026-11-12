//! Messages driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::Notice;
use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use landaire_ui::ToastKey;
use landaire_ui::Toasts;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use landaire_ui::withdraw_everywhere;
use zgui::component;
use zgui::view;
use zgui::view::IntoView;
use zgui::vocab::Role;

const SIZE: WindowSize = WindowSize { width: 640.0, height: 400.0 };
/// Long enough for a message to finish arriving or leaving.
const MOVE: Duration = Duration::from_millis(1000);

/// Hands the window's messages to the test.
#[component]
fn Grab(handed: Rc<Cell<Option<Toasts>>>) -> impl IntoView {
    handed.set(Toasts::current());

    view! { box() }
}

fn open_host() -> (HeadlessWindow, Toasts) {
    let handed = Rc::new(Cell::new(None));
    let sent = Rc::clone(&handed);
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let sent = Rc::clone(&sent);
        view! {
            ThemeRoot {
                ToastHost {
                    Grab(handed = sent)
                }
            }
        }
    })
    .expect("opens");

    (window, handed.get().expect("the host publishes its messages"))
}

/// What the messages on screen say, newest first.
fn messages(window: &mut HeadlessWindow) -> Vec<String> {
    let alerts = window.by_role(Role::Alert);

    alerts.into_iter().filter_map(|alert| window.a11y(alert).and_then(|state| state.label)).collect()
}

#[test]
fn a_keyed_message_replaces_the_last_one_about_its_key_and_is_withdrawn_by_it() {
    let (mut window, toasts) = open_host();
    let key = ToastKey::new("worker");

    toasts.show(Notice::warn("Worker stopped").keyed(key.clone()));
    window.advance(MOVE);
    toasts.show(Notice::info("Worker restarted").keyed(key.clone()));
    toasts.ok("Saved");
    window.advance(MOVE);
    assert_eq!(
        messages(&mut window),
        ["Saved", "Worker restarted"],
        "the second message about the key replaced the first"
    );

    toasts.withdraw(&key);
    window.advance(MOVE);
    assert_eq!(messages(&mut window), ["Saved"], "withdrawing the key took its message down and only that one");
}

#[test]
fn a_sticky_message_stays_until_it_is_withdrawn() {
    let (mut window, toasts) = open_host();
    let key = ToastKey::new("setup");

    toasts.show(Notice::warn("Choose a directory").keyed(key.clone()).sticky());
    toasts.info("Opened");
    window.advance(Duration::from_secs(8));
    assert_eq!(messages(&mut window), ["Choose a directory"], "the brief message went and the sticky one stayed");

    withdraw_everywhere(&key);
    window.advance(MOVE);
    assert!(messages(&mut window).is_empty(), "withdrawing it from every window took it down");
}
