//! Brief messages over the window, for the things that happen without a
//! visible change.
//!
//! The egui app reports these through `egui_notify` toasts: a path copied, a
//! directory opened, a credential refused. A port that only logged them left
//! the reader pressing a button and seeing nothing. `Root` holds the queue and
//! draws the layer, so this is only the wording and the level.

use std::rc::Rc;

use gpui_kit::ElementId;
use gpui_kit::SharedString;
use gpui_kit::Window;
use gpui_kit::component::Root;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::Notification;

/// Something the reader asked for happened.
pub fn ok(message: impl Into<SharedString>, window: &mut Window, cx: &mut gpui_kit::App) {
    let message = message.into();
    push(Notification::success(message.clone()), &message, window, cx);
}

/// Something the reader asked for did not happen.
pub fn failed(message: impl Into<SharedString>, window: &mut Window, cx: &mut gpui_kit::App) {
    let message = message.into();
    push(Notification::error(message.clone()), &message, window, cx);
}

/// Something worth reading that nobody asked for, such as what a jump landed
/// on.
pub fn info(message: impl Into<SharedString>, window: &mut Window, cx: &mut gpui_kit::App) {
    let message = message.into();
    push(Notification::info(message.clone()), &message, window, cx);
}

/// Something is wrong but nothing was lost.
pub fn warn(message: impl Into<SharedString>, window: &mut Window, cx: &mut gpui_kit::App) {
    let message = message.into();
    push(Notification::warning(message.clone()), &message, window, cx);
}

/// What pressing an [`Action`] does.
type Run = dyn Fn(&mut Window, &mut gpui_kit::App);

/// A button on a message that takes the reader to what the message is about.
pub struct Action {
    label: SharedString,
    run: Rc<Run>,
}

impl Action {
    pub fn new(label: impl Into<SharedString>, run: impl Fn(&mut Window, &mut gpui_kit::App) + 'static) -> Self {
        Self { label: label.into(), run: Rc::new(run) }
    }
}

/// As [`warn`], with a button. Pressing it takes the message down: it has
/// done what it was for.
pub fn warn_with_action(message: impl Into<SharedString>, action: Action, window: &mut Window, cx: &mut gpui_kit::App) {
    let message = message.into();
    push(with_action(Notification::warning(message.clone()), action, true), &message, window, cx);
}

/// How a [`replacing`] or [`about`] message reads.
#[derive(Clone, Copy)]
pub enum Level {
    /// As [`ok`].
    Ok,
    /// As [`info`].
    Info,
    /// As [`warn`].
    Warning,
}

impl Level {
    fn notification(self, message: SharedString) -> Notification {
        match self {
            Self::Ok => Notification::success(message),
            Self::Info => Notification::info(message),
            Self::Warning => Notification::warning(message),
        }
    }
}

/// A message that slides away like any other, but that the next one under
/// the same `key` replaces rather than stacks under, so a burst of the same
/// event says only its latest.
pub fn replacing(
    key: &'static str,
    level: Level,
    message: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let message = message.into();
    push(level.notification(message.clone()).id1::<Replacing>(key), &message, window, cx);
}

/// How long an [`about`] message stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stay {
    Briefly,
    UntilDismissed,
}

/// A message about one thing, which the next message about the same `key`
/// replaces and [`withdraw`] takes down.
///
/// For a state that may change again while the reader is still reading about
/// it, such as a worker that stopped and has since been started again. A
/// button on it takes the message down when pressed.
pub fn about(
    key: impl Into<ElementId>,
    level: Level,
    message: impl Into<SharedString>,
    action: Option<Action>,
    stay: Stay,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let message = message.into();
    let notification = level.notification(message.clone()).id1::<About>(key).autohide(stay == Stay::Briefly);
    let notification = match action {
        Some(action) => with_action(notification, action, true),
        None => notification,
    };
    push(notification, &message, window, cx);
}

/// Takes down the [`about`] message under `key`, if one is up.
pub fn withdraw(key: impl Into<ElementId>, window: &mut Window, cx: &mut gpui_kit::App) {
    if window.root::<Root>().flatten().is_none() {
        return;
    }
    window.remove_notification1::<About>(key, cx);
}

/// As [`withdraw`], for an owner that has no window to hand: every window is
/// asked once the current update is done, and only one holds `key`.
pub fn withdraw_everywhere(key: ElementId, cx: &mut gpui_kit::App) {
    cx.defer(move |cx| {
        for window in cx.windows() {
            let _ = window.update(cx, |_root, window, cx| withdraw(key.clone(), window, cx));
        }
    });
}

/// The identity of an [`about`] message.
struct About;

/// The identity of a [`replacing`] message.
struct Replacing;

/// A state the reader has to act on, which stays until they dismiss it.
///
/// For the settings the app cannot work without: the egui app makes these two
/// permanent and closable rather than letting them slide away while the reader is
/// looking elsewhere (`app.rs`'s `duration(None)`/`closable(true)`). `key` is the
/// state being reported, so the same complaint replaces itself rather than
/// stacking one copy per attempt.
pub fn stuck(key: &'static str, message: impl Into<SharedString>, window: &mut Window, cx: &mut gpui_kit::App) {
    let message = message.into();
    push(Notification::warning(message.clone()).autohide(false).id1::<Stuck>(key), &message, window, cx);
}

/// As [`stuck`], with a button. The message stays after it is pressed, until
/// the state is [`resolved`].
pub fn stuck_with_action(
    key: &'static str,
    message: impl Into<SharedString>,
    action: Action,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let message = message.into();
    let notification = Notification::warning(message.clone()).autohide(false).id1::<Stuck>(key);
    push(with_action(notification, action, false), &message, window, cx);
}

fn with_action(notification: Notification, action: Action, dismiss_on_press: bool) -> Notification {
    notification.action(move |_notification, _window, cx| {
        let run = Rc::clone(&action.run);
        let shown = cx.entity();
        Button::new("toast-action").label(action.label.clone()).small().on_click(move |_event, window, cx| {
            run(window, cx);
            if dismiss_on_press {
                shown.update(cx, |notification, cx| notification.dismiss(window, cx));
            }
        })
    })
}

/// The identity of a [`stuck`] message, so one per state is on screen at a time.
struct Stuck;

/// Takes a [`stuck`] message down, for a state the reader has since fixed.
///
/// Called on the way out of the state rather than left to the reader: a
/// permanent complaint about a directory that is now an install is worse than no
/// complaint at all.
pub fn resolved(key: &'static str, window: &mut Window, cx: &mut gpui_kit::App) {
    if window.root::<Root>().flatten().is_none() {
        return;
    }
    window.remove_notification1::<Stuck>(key, cx);
}

/// Queues `notification` on the window's `Root`, which is what holds the queue.
///
/// A window rooted in anything else has none, and the kit panics on the missing
/// root rather than saying so, which would turn a report into a crash on a
/// window opened bare (a test window, or a surface not yet given a `Root`).
fn push(notification: Notification, said: &str, window: &mut Window, cx: &mut gpui_kit::App) {
    #[cfg(test)]
    SAID.with(|all| all.borrow_mut().push(said.to_owned()));
    if window.root::<Root>().flatten().is_none() {
        tracing::warn!("toast: this window has no Root to show a message on, so it is only logged: {said}");
        return;
    }
    window.push_notification(notification, cx);
}

#[cfg(test)]
thread_local! {
    /// Every message put up on this thread: the kit's notification layer
    /// exposes no text to the test harness.
    static SAID: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Every message put up on this thread so far.
#[cfg(test)]
pub(crate) fn said_for_test() -> Vec<String> {
    SAID.with(|all| all.borrow().clone())
}
