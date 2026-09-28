//! Brief messages over the window, for the things that happen without a
//! visible change.
//!
//! The egui app reports these through `egui_notify` toasts: a path copied, a
//! directory opened, a credential refused. A port that only logged them left
//! the reader pressing a button and seeing nothing. `Root` holds the queue and
//! the window's own view draws the layer (`App::render`, `window_shell::Shell`),
//! so this is only the wording and the level.

use gpui_kit::SharedString;
use gpui_kit::Window;
use gpui_kit::component::Root;
use gpui_kit::component::WindowExt as _;
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
    if window.root::<Root>().flatten().is_none() {
        tracing::warn!("toast: this window has no Root to show a message on, so it is only logged: {said}");
        return;
    }
    window.push_notification(notification, cx);
}
