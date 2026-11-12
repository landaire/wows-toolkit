//! What a frame drawn by the application asks of the window it is in.

use std::rc::Rc;

use zgui::platform::CursorStyle;
use zgui::platform::DecorationSource;
use zgui::platform::FullscreenMode;
use zgui::platform::ResizeEdge;
use zgui::reactive::Signal;
use zgui::runtime::windows::WindowHandle;
use zgui::runtime::windows::use_window;

/// The window a [`TitleBar`](crate::TitleBar) or [`WindowFrame`](crate::WindowFrame) reads and
/// acts on.
///
/// [`WindowHandle`] is the one an application uses. Another stands in for it where the platform
/// does not carry out these requests, such as a headless test, or to show a frame on a desktop
/// that draws its own.
pub trait FrameWindow {
    /// Who draws the window's frame.
    fn decoration_source(&self) -> Signal<DecorationSource>;

    fn maximized(&self) -> Signal<bool>;

    fn fullscreen(&self) -> Signal<Option<FullscreenMode>>;

    fn minimize(&self);

    fn toggle_maximized(&self);

    /// Does what a double press on a title bar does on this desktop.
    fn title_bar_double_click(&self);

    fn close(&self);

    /// Hands the pointer to the desktop to move the window.
    fn begin_move_drag(&self);

    /// Hands the pointer to the desktop to resize the window from `edge`.
    fn begin_resize_drag(&self, edge: ResizeEdge);

    fn set_cursor(&self, cursor: CursorStyle);
}

impl FrameWindow for WindowHandle {
    fn decoration_source(&self) -> Signal<DecorationSource> {
        WindowHandle::decoration_source(self)
    }

    fn maximized(&self) -> Signal<bool> {
        WindowHandle::maximized(self)
    }

    fn fullscreen(&self) -> Signal<Option<FullscreenMode>> {
        WindowHandle::fullscreen(self)
    }

    fn minimize(&self) {
        WindowHandle::minimize(self);
    }

    fn toggle_maximized(&self) {
        WindowHandle::toggle_maximized(self);
    }

    fn title_bar_double_click(&self) {
        WindowHandle::title_bar_double_click(self);
    }

    fn close(&self) {
        WindowHandle::close(self);
    }

    fn begin_move_drag(&self) {
        WindowHandle::begin_move_drag(self);
    }

    fn begin_resize_drag(&self, edge: ResizeEdge) {
        WindowHandle::begin_resize_drag(self, edge);
    }

    fn set_cursor(&self, cursor: CursorStyle) {
        WindowHandle::set_cursor(self, cursor);
    }
}

/// `window`, or the window the calling component is in.
pub(crate) fn or_current(window: Option<Rc<dyn FrameWindow>>) -> Rc<dyn FrameWindow> {
    window.unwrap_or_else(|| Rc::new(use_window()))
}
