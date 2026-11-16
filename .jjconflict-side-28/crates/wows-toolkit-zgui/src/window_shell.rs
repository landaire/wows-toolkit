//! What every window of the app shares: the frame drawn where the desktop draws none, and
//! remembering where the window was, in the row every front end shares
//! (`wows_toolkit_config::WindowKind`).

use std::time::Duration;

use landaire_ui::TitleBarProps;
use landaire_ui::TitleBarStrings;
use landaire_ui::WindowFrameProps;
use rust_i18n::t;
use wows_toolkit_config::WindowKind;
use wows_toolkit_config::WindowSettings;
use zgui::App;
use zgui::component;
use zgui::prelude::FullscreenMode;
use zgui::prelude::WindowHandle;
use zgui::prelude::WindowOptions;
use zgui::prelude::try_use_window;
use zgui::reactive::Callable;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::view;
use zgui::view::Children;
use zgui::view::IntoView;
use zgui::view::Timers;

use crate::services::Services;

/// Where the main window opens when nothing was remembered.
const DEFAULT_ORIGIN: (f32, f32) = (200.0, 120.0);
const DEFAULT_SIZE: (f32, f32) = (1200.0, 800.0);

/// The main window's title.
pub fn main_window_title() -> String {
    format!("{} v{}", wows_toolkit_config::APP_NAME, env!("CARGO_PKG_VERSION"))
}

/// A window's content, under the title bar and inside the frame the app draws when the desktop
/// draws neither, as a Wayland compositor without server-side decorations does. Elsewhere the
/// desktop's own frame stays and the content fills the window.
#[component]
pub fn DecoratedWindow(#[prop(into)] title: String, children: Children) -> impl IntoView {
    let title = StoredValue::new_local(title);
    let strings = TitleBarStrings {
        minimize: t!("ui.windows.minimize").into_owned(),
        maximize: t!("ui.windows.maximize").into_owned(),
        restore: t!("ui.windows.restore").into_owned(),
        close: t!("ui.windows.close").into_owned(),
    };

    view! {
        WindowFrame(attr:data-testid = "window-frame") {
            TitleBar(strings = strings, attr:data-testid = "window-title-bar") {
                text {{title.get_value()}}
            }
            {children.into_view_once()}
        }
    }
}

/// How the main window should open, as the shared row remembers it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MainWindowGeometry(Option<WindowSettings>);

impl MainWindowGeometry {
    /// Read straight from the database, before the window opens: a position can only be asked for
    /// when the window is made.
    pub fn remembered() -> Self {
        Self(wows_toolkit_config::load_main_window_settings())
    }

    /// `app`, opening where the window was, falling back to the defaults for whatever is not
    /// remembered.
    pub fn apply(self, app: App) -> App {
        let saved = self.0.unwrap_or_default();
        let [width, height] = saved.inner_size_points.unwrap_or([DEFAULT_SIZE.0, DEFAULT_SIZE.1]);
        let [x, y] = saved.outer_position_pixels.unwrap_or([DEFAULT_ORIGIN.0, DEFAULT_ORIGIN.1]);
        let fullscreen = saved.fullscreen.then_some(FullscreenMode::Borderless);

        app.with_size(width, height).with_position(x, y).with_maximized(saved.maximized).with_fullscreen(fullscreen)
    }
}

/// Options for a secondary window titled `title`, opening as `saved` remembers it, at
/// `default_size` when no size was remembered.
pub fn secondary_window_options(
    title: String,
    saved: Option<WindowSettings>,
    default_size: (f32, f32),
) -> WindowOptions {
    let saved = saved.unwrap_or_default();
    let [width, height] = saved.inner_size_points.unwrap_or([default_size.0, default_size.1]);
    let fullscreen = saved.fullscreen.then_some(FullscreenMode::Borderless);

    let options =
        WindowOptions::new(title).with_size(width, height).with_maximized(saved.maximized).with_fullscreen(fullscreen);
    match saved.outer_position_pixels {
        Some([x, y]) => options.with_position(x, y),
        None => options,
    }
}

/// What `window` would have to be opened with to come back where it is.
///
/// The stored size is the egui app's `inner_size_points` and the position its
/// `outer_position_pixels`. The window reports its position only where the desktop lets an
/// application know it, which no Wayland compositor does; there the position is left out.
fn geometry_of(window: &WindowHandle) -> WindowSettings {
    let size = window.size().get_untracked();
    let position = window.position();

    WindowSettings {
        inner_size_points: Some([size.width.0, size.height.0]),
        outer_position_pixels: position.map(|at| [at.x.0, at.y.0]),
        fullscreen: window.fullscreen().get_untracked().is_some(),
        maximized: window.maximized().get_untracked(),
    }
}

/// Keeps what is remembered for `kind` in step with the window it is in while it is open.
/// `on_change` hears each new geometry, for an opener that keeps its own copy.
///
/// Polled rather than driven by the window's signals: its position has no signal, and a window
/// that is only moved changes nothing else.
#[component]
pub fn RememberGeometry(
    kind: WindowKind,
    #[prop(optional)] on_change: Option<UnsyncCallback<WindowSettings>>,
) -> impl IntoView {
    const POLL: Duration = Duration::from_secs(1);

    let services = Services::current();
    let (Some(window), Some(timers)) = (try_use_window(), Timers::current()) else {
        return;
    };

    let last = StoredValue::new_local(geometry_of(&window));
    let watched = window.clone();
    let polling = timers.set_interval(POLL, move || {
        let geometry = geometry_of(&watched);
        if geometry == last.get_value() {
            return;
        }
        if let Some(on_change) = on_change {
            on_change.run(geometry);
        }
        // Not remembered as written until the database is open to take it.
        if store(services, kind, geometry) {
            last.set_value(geometry);
        }
    });

    // The poll can be a second behind, and resizing a window and then closing it is one gesture.
    let closing = window.clone();
    on_cleanup_local(move || {
        drop(polling);
        let geometry = geometry_of(&closing);
        if geometry != last.get_value() {
            if let Some(on_change) = on_change {
                on_change.run(geometry);
            }
            store_before_exit(services, kind, geometry);
        }
    });
}

/// Writes `geometry` for `kind` in the background. `false` when the database is not open yet.
fn store(services: Services, kind: WindowKind, geometry: WindowSettings) -> bool {
    let Some(pool) = services.settings.pool() else {
        return false;
    };

    let written =
        services.on_tokio(async move { wows_toolkit_config::store_window_settings(&pool, kind, geometry).await });
    zgui::task::spawn_detached(async move {
        match written.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::error!("window geometry: {kind:?} could not be written: {err}"),
            Err(err) => tracing::error!("window geometry: the write of {kind:?} did not complete: {err}"),
        }
    });
    true
}

/// Writes `geometry` for `kind` before returning.
///
/// For a window going away: the process may exit straight after, and a background write would be
/// lost with it. One row, so the UI thread waits only for that.
fn store_before_exit(services: Services, kind: WindowKind, geometry: WindowSettings) {
    // The pool first: `None` once the services are gone, which a secondary window outlives.
    let Some(pool) = services.settings.pool() else { return };
    let Some(runtime) = services.runtime() else { return };

    if let Err(err) = runtime.block_on(wows_toolkit_config::store_window_settings(&pool, kind, geometry)) {
        tracing::error!("window geometry: {kind:?} could not be written as the window closed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use wows_toolkit_config::WindowKind;
    use wows_toolkit_config::WindowSettings;
    use wows_toolkit_services::settings::StartupSettings;
    use zgui::reactive::Mounted;

    use super::store;
    use super::store_before_exit;
    use crate::services::Services;

    /// A secondary window outlives the main window that owns the services when the app exits. Its
    /// geometry poll and its cleanup then find no database rather than a freed value.
    #[test]
    fn geometry_is_not_stored_once_the_services_are_gone() {
        zgui::reactive::install().expect("the reactive runtime installs");
        let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
        let main = Mounted::new();
        let services = main.with(|| Services::provide(Some(Arc::clone(&runtime))));
        let pool = runtime.block_on(wows_toolkit_config::test_pool());
        let settings = runtime.block_on(StartupSettings::load(&pool));
        services.settings.adopt(settings, pool);
        assert!(services.settings.pool().is_some(), "the database is open while the main window is");

        main.unmount();
        let geometry = WindowSettings { inner_size_points: Some([640.0, 480.0]), ..WindowSettings::default() };
        assert!(!store(services, WindowKind::ReplayRenderer, geometry), "nothing is left to store into");
        store_before_exit(services, WindowKind::ReplayRenderer, geometry);
    }

    /// The title bar and frame appear only while the desktop leaves the frame to the app, and go
    /// again when it takes the frame back.
    #[test]
    fn the_app_draws_a_frame_only_where_the_desktop_draws_none() {
        use landaire_ui::ThemeRootProps;
        use landaire_ui::test_support::HeadlessOptions;
        use landaire_ui::test_support::HeadlessWindow;
        use landaire_ui::test_support::WindowSize;
        use zgui::platform::DecorationSource;
        use zgui::view;

        use super::DecoratedWindowProps;

        let options = HeadlessOptions::new(WindowSize { width: 640.0, height: 400.0 });
        let mut window = HeadlessWindow::open(options, || {
            view! {
                ThemeRoot {
                    DecoratedWindow(title = "Tactics Board") {
                        text(attr:data-testid = "window-body") {"body"}
                    }
                }
            }
        })
        .expect("the window opens headlessly");
        window.advance(std::time::Duration::from_millis(100));
        assert!(window.by_test_id("window-body").is_some());
        assert!(window.by_test_id("window-title-bar").is_none(), "the desktop's own title bar is the only one");

        window.set_decoration_source(DecorationSource::Application).expect("the window is open");
        window.advance(std::time::Duration::from_millis(100));
        let bar = window.by_test_id("window-title-bar").expect("the app draws a title bar");
        let frame = window.by_test_id("window-frame").expect("the frame is there");
        assert!(window.bounds(bar).is_some_and(|bounds| bounds.size.height.0 > 0.0));
        assert!(window.by_attribute("data-framed", "true").is_some_and(|framed| framed == frame));
        assert!(window.by_label("Close").is_some(), "the window can be closed from the app's bar");

        window.set_decoration_source(DecorationSource::Platform).expect("the window is open");
        window.advance(std::time::Duration::from_millis(100));
        assert!(window.by_test_id("window-title-bar").is_none());
        assert!(window.by_attribute("data-framed", "true").is_none());
    }
}
