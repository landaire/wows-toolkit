//! Brief messages over the window, for the things that happen without a
//! visible change.
//!
//! The egui app reports these through `egui_notify` toasts: a path copied, a
//! directory opened, a credential refused. A port that only logged them left
//! the reader pressing a button and seeing nothing. `Root` holds the queue and
//! `App::render` draws the layer, so this is only the wording and the level.

use gpui_kit::Window;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;

/// Something the reader asked for happened.
pub fn ok(message: impl Into<gpui_kit::SharedString>, window: &mut Window, cx: &mut gpui_kit::App) {
    window.push_notification(Notification::success(message), cx);
}

/// Something the reader asked for did not happen.
pub fn failed(message: impl Into<gpui_kit::SharedString>, window: &mut Window, cx: &mut gpui_kit::App) {
    window.push_notification(Notification::error(message), cx);
}

/// Something worth reading that nobody asked for, such as what a jump landed
/// on.
pub fn info(message: impl Into<gpui_kit::SharedString>, window: &mut Window, cx: &mut gpui_kit::App) {
    window.push_notification(Notification::info(message), cx);
}

/// Something is wrong but nothing was lost.
pub fn warn(message: impl Into<gpui_kit::SharedString>, window: &mut Window, cx: &mut gpui_kit::App) {
    window.push_notification(Notification::warning(message), cx);
}
