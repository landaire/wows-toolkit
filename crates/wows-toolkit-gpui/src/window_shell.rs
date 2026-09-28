//! Opening a window, and remembering where it was.
//!
//! The egui app gives its replay renderer, tactics boards and armor viewer each
//! a window of their own, and remembers where each kind was last put
//! (`wows_toolkit_config::WindowKind`). This is the port's equivalent: the
//! options a window opens with, read from the row both apps share, and the
//! writing back of what the reader has since done to it.

use gpui_kit::AnyView;
use gpui_kit::AnyWindowHandle;
use gpui_kit::App;
use gpui_kit::Bounds;
use gpui_kit::Context;
use gpui_kit::Global;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement as _;
use gpui_kit::Render;
use gpui_kit::Styled as _;
use gpui_kit::Subscription;
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
pub fn options(kind: WindowKind, title: String, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(bounds_for(remembered(kind, cx), Bounds { origin: DEFAULT_ORIGIN, size: DEFAULT_SIZE })),
        window_min_size: Some(MIN_SIZE),
        titlebar: Some(gpui_kit::TitlebarOptions { title: Some(title.into()), ..Default::default() }),
        ..Default::default()
    }
}

/// Every kind's remembered geometry, as the shared settings row holds it.
///
/// Held in memory because a window is opened from the UI thread, where the
/// database cannot be read: what the row said at startup is what a window opened
/// later is placed by, kept in step by [`store`].
struct Remembered(std::collections::HashMap<WindowKind, WindowSettings>);

impl Global for Remembered {}

/// Adopts what the database remembers. Called once, when the startup read lands.
pub fn adopt(remembered: std::collections::HashMap<WindowKind, WindowSettings>, cx: &mut App) {
    cx.set_global(Remembered(remembered));
}

/// What is remembered about a window of this kind.
///
/// `None` when nothing is, which is the ordinary state the first time one is
/// opened, and also before the startup read has landed.
fn remembered(kind: WindowKind, cx: &App) -> Option<WindowSettings> {
    cx.try_global::<Remembered>()?.0.get(&kind).copied()
}

/// How long a change can sit unwritten.
///
/// Polled rather than driven by an event: gpui reports a window's bounds but has
/// no notice that they changed, and a window that is only moved never re-renders.
/// The egui app writes geometry from its periodic save task for the same reason.
const POLL: std::time::Duration = std::time::Duration::from_secs(1);

/// Keeps what is remembered for `kind` in step with `window` while it is open.
pub fn remember(kind: WindowKind, window: &Window, cx: &mut App) {
    let mut last = geometry_of(window);
    window
        .spawn(cx, async move |cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                // An error is the window having closed, which ends the watch.
                let Ok(geometry) = cx.update(|window, _cx| geometry_of(window)) else { return };
                if geometry == last {
                    continue;
                }
                last = geometry;
                let _ = cx.update(|_window, cx| store(kind, geometry, cx));
            }
        })
        .detach();
}

/// Writes `window`'s geometry once more as the app quits.
///
/// The poll can be a second behind, and resizing a window and then closing the
/// app is one gesture. The returned subscription has to be held for the callback
/// to stay registered.
pub fn remember_on_quit(kind: WindowKind, window: AnyWindowHandle, cx: &mut App) -> Subscription {
    cx.on_app_quit(move |cx: &mut App| {
        // Read while the window still exists, and hand the write to the runtime
        // directly: gpui awaits this future before it quits, which is what makes
        // the last write land rather than race the process exiting.
        let geometry = window.update(cx, |_view: AnyView, window, _cx| geometry_of(window)).ok();
        let pool = crate::settings_store::pool(cx);
        let runtime = crate::runtime::runtime(cx);

        async move {
            let (Some(geometry), Some(pool), Some(runtime)) = (geometry, pool, runtime) else { return };
            let written = runtime
                .handle()
                .spawn(async move { wows_toolkit_config::store_window_settings(&pool, kind, geometry).await })
                .await;
            match written {
                Ok(Ok(())) => {}
                Ok(Err(err)) => tracing::error!("window geometry: {kind:?} could not be written on quit: {err}"),
                Err(err) => tracing::error!("window geometry: the quit write of {kind:?} did not complete: {err}"),
            }
        }
    })
}

/// What `window` would have to be opened with to come back where it is.
///
/// The stored size is the egui app's `inner_size_points` and the position its
/// `outer_position_pixels`, which is what the shared row means; gpui reports one
/// bounds for both, so the two apps disagree by a title bar's height when they
/// read each other's. Within either app the round trip is exact.
fn geometry_of(window: &Window) -> WindowSettings {
    let (bounds, maximized, fullscreen) = match window.window_bounds() {
        WindowBounds::Windowed(bounds) => (bounds, false, false),
        WindowBounds::Maximized(bounds) => (bounds, true, false),
        WindowBounds::Fullscreen(bounds) => (bounds, false, true),
    };

    WindowSettings {
        inner_size_points: Some([bounds.size.width.into(), bounds.size.height.into()]),
        outer_position_pixels: Some([bounds.origin.x.into(), bounds.origin.y.into()]),
        fullscreen,
        maximized,
    }
}

/// Remembers `geometry` for `kind`, in this process and in the database.
fn store(kind: WindowKind, geometry: WindowSettings, cx: &mut App) {
    if cx.has_global::<Remembered>() {
        cx.global_mut::<Remembered>().0.insert(kind, geometry);
    }

    let Some(pool) = crate::settings_store::pool(cx) else {
        tracing::warn!("window geometry: {kind:?} was not saved because the config database is not open");
        return;
    };

    cx.spawn(async move |cx| {
        let written = crate::runtime::spawn(cx, async move {
            wows_toolkit_config::store_window_settings(&pool, kind, geometry).await
        })
        .await;

        match written {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::error!("window geometry: {kind:?} could not be written: {err}"),
            Err(err) => tracing::error!("window geometry: the write of {kind:?} did not complete: {err}"),
        }
    })
    .detach();
}

/// `saved` as gpui bounds, falling back to `default_bounds` for whatever it does
/// not carry.
pub fn bounds_for(saved: Option<WindowSettings>, default_bounds: Bounds<gpui_kit::Pixels>) -> WindowBounds {
    let Some(saved) = saved else {
        return WindowBounds::Windowed(default_bounds);
    };

    let size = saved.inner_size_points.map(|[w, h]| size(px(w), px(h))).unwrap_or(default_bounds.size);
    let origin = saved.outer_position_pixels.map(|[x, y]| point(px(x), px(y))).unwrap_or(default_bounds.origin);
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

#[cfg(test)]
mod tests {
    use gpui_kit::AppContext as _;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;

    use super::*;

    /// What a window reports is what it is opened with next time: a size and a
    /// position that survive the trip through the shared settings row.
    #[gpui_kit::test]
    fn a_windows_geometry_round_trips(cx: &mut TestAppContext) {
        struct Blank;
        impl gpui_kit::Render for Blank {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut gpui_kit::Context<Self>,
            ) -> impl gpui_kit::IntoElement {
                gpui_kit::div()
            }
        }

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(700.)), |_window, _cx| Blank);

        let geometry =
            cx.update_window(window.into(), |_, window, _cx| geometry_of(window)).expect("the test window is open");

        assert_eq!(geometry.inner_size_points, Some([900., 700.]), "the size it was opened at");
        assert!(!geometry.maximized && !geometry.fullscreen, "an ordinary window is neither");

        let default_bounds = Bounds { origin: gpui_kit::point(px(1.), px(2.)), size: size(px(3.), px(4.)) };
        match bounds_for(Some(geometry), default_bounds) {
            WindowBounds::Windowed(bounds) => {
                assert_eq!(bounds.size, size(px(900.), px(700.)), "and what it would open at again");
            }
            other => panic!("an ordinary window opens windowed, got {other:?}"),
        }
    }

    /// A kind nothing is remembered for opens at the default, which is what a
    /// first launch does.
    #[test]
    fn an_unremembered_window_opens_at_the_default() {
        let default_bounds = Bounds { origin: gpui_kit::point(px(120.), px(120.)), size: size(px(900.), px(700.)) };
        match bounds_for(None, default_bounds) {
            WindowBounds::Windowed(bounds) => assert_eq!(bounds, default_bounds),
            other => panic!("the default is a windowed window, got {other:?}"),
        }
    }
}
