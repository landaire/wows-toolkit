//! Opening a window other than the main one.
//!
//! The egui app gives its replay renderer, tactics boards and armor viewer
//! each a window of their own, and remembers where each kind was last put
//! (`wows_toolkit_config::WindowKind`). This is the port's equivalent: the
//! options a second window opens with, read from the same rows.

use gpui_kit::AnyView;
use gpui_kit::Bounds;
use gpui_kit::Context;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement as _;
use gpui_kit::Render;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::WindowBounds;
use gpui_kit::WindowOptions;
use gpui_kit::component::Root;
use gpui_kit::div;
use gpui_kit::point;
use gpui_kit::px;
use gpui_kit::size;
use wows_toolkit_config::WindowKind;
use wows_toolkit_config::WindowSettings;

/// Where a window of this kind opens when nothing was remembered.
const DEFAULT_SIZE: gpui_kit::Size<gpui_kit::Pixels> = gpui_kit::Size { width: px(900.), height: px(700.) };
const DEFAULT_ORIGIN: gpui_kit::Point<gpui_kit::Pixels> = gpui_kit::Point { x: px(120.), y: px(120.) };

/// The smallest a secondary window may be dragged to. The renderer's
/// transport is the widest thing in one, and below this its controls wrap
/// into each other.
const MIN_SIZE: gpui_kit::Size<gpui_kit::Pixels> = gpui_kit::Size { width: px(480.), height: px(420.) };

/// How a window of `kind` should open, titled `title`.
pub fn options(kind: WindowKind, title: String) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(bounds_for(load(kind))),
        window_min_size: Some(MIN_SIZE),
        titlebar: Some(gpui_kit::TitlebarOptions { title: Some(title.into()), ..Default::default() }),
        ..Default::default()
    }
}

/// What was remembered about a window of this kind.
///
/// `None` when nothing was, which is the ordinary state the first time one is
/// opened.
fn load(kind: WindowKind) -> Option<WindowSettings> {
    let _ = kind;
    // The shared row is read through the config crate's own loader, which
    // currently exposes the main window only; a secondary window opens at the
    // default until that is widened.
    None
}

fn bounds_for(saved: Option<WindowSettings>) -> WindowBounds {
    let default_bounds = Bounds { origin: DEFAULT_ORIGIN, size: DEFAULT_SIZE };
    let Some(saved) = saved else {
        return WindowBounds::Windowed(default_bounds);
    };

    let size = saved.inner_size_points.map(|[w, h]| size(px(w), px(h))).unwrap_or(DEFAULT_SIZE);
    let origin = saved.outer_position_pixels.map(|[x, y]| point(px(x), px(y))).unwrap_or(DEFAULT_ORIGIN);
    let bounds = Bounds { origin, size };

    if saved.fullscreen {
        WindowBounds::Fullscreen(bounds)
    } else if saved.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}

/// What a secondary window's own view draws: its content, and the layers `Root`
/// holds but does not draw itself.
///
/// `Root` keeps the dialog, sheet and notification queues; whoever renders the
/// window's view has to put them on screen, which the main window does in
/// `App::render`. A window opened straight around a panel has no such view, so
/// everything queued there -- every toast the panel reports -- is queued and
/// never seen.
pub struct Shell {
    content: AnyView,
}

impl Shell {
    pub fn new(content: impl Into<AnyView>) -> Self {
        Self { content: content.into() }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sheets = Root::render_sheet_layer(window, cx);
        let dialogs = Root::render_dialog_layer(window, cx);
        let notifications = Root::render_notification_layer(window, cx);

        div().size_full().child(self.content.clone()).children(sheets).children(dialogs).children(notifications)
    }
}
