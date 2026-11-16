//! Messages driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::MotionPreference;
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
const FRAME: Duration = Duration::from_millis(17);

/// Hands the window's messages to the test.
#[component]
fn Grab(handed: Rc<Cell<Option<Toasts>>>) -> impl IntoView {
    handed.set(Toasts::current());

    view! { box() }
}

fn open_host(motion: MotionPreference) -> (HeadlessWindow, Toasts) {
    let handed = Rc::new(Cell::new(None));
    let sent = Rc::clone(&handed);
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let sent = Rc::clone(&sent);
        view! {
            ThemeRoot(motion = motion) {
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
    let (mut window, toasts) = open_host(MotionPreference::Full);
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
    let (mut window, toasts) = open_host(MotionPreference::Full);
    let key = ToastKey::new("setup");

    toasts.show(Notice::warn("Choose a directory").keyed(key.clone()).sticky());
    toasts.info("Opened");
    window.advance(Duration::from_secs(8));
    assert_eq!(messages(&mut window), ["Choose a directory"], "the brief message went and the sticky one stayed");

    withdraw_everywhere(&key);
    window.advance(MOVE);
    assert!(messages(&mut window).is_empty(), "withdrawing it from every window took it down");
}

/// How many animations the message and its slot run a frame after `toasts` puts one up.
fn moving_after_show(motion: MotionPreference) -> usize {
    let (mut window, toasts) = open_host(motion);

    toasts.ok("Saved");
    window.advance(FRAME);
    let toast = window.by_class("zui-toast").expect("the message is built");
    let slot = window.by_class("zui-toast__slot").expect("the message has a slot");

    window.running_animations(toast) + window.running_animations(slot)
}

#[test]
fn reduced_motion_puts_a_message_up_and_takes_it_down_without_moving_it() {
    assert!(moving_after_show(MotionPreference::Full) > 0, "with motion allowed the message slides in");
    assert_eq!(moving_after_show(MotionPreference::Reduced), 0, "with motion reduced it appears in place");

    let (mut window, toasts) = open_host(MotionPreference::Reduced);
    let key = ToastKey::new("saved");
    toasts.show(Notice::ok("Saved").keyed(key.clone()));
    window.advance(FRAME);
    toasts.withdraw(&key);
    // The toaster looks for a running exit a few frames after it starts; a full exit takes 400ms.
    window.advance(Duration::from_millis(100));
    assert!(messages(&mut window).is_empty(), "with motion reduced a message leaves without moving");
}
