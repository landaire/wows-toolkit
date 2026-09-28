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
