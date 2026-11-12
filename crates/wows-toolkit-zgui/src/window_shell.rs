//! Remembering where a window was, in the row every front end shares
//! (`wows_toolkit_config::WindowKind`).

use std::time::Duration;

use wows_toolkit_config::WindowKind;
use wows_toolkit_config::WindowSettings;
use zgui::App;
use zgui::component;
use zgui::prelude::FullscreenMode;
use zgui::prelude::WindowHandle;
use zgui::prelude::try_use_window;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::view::IntoView;
use zgui::view::Timers;

use crate::services::Services;

/// Where the main window opens when nothing was remembered.
const DEFAULT_ORIGIN: (f32, f32) = (200.0, 120.0);
const DEFAULT_SIZE: (f32, f32) = (1200.0, 800.0);

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
///
/// Polled rather than driven by the window's signals: its position has no signal, and a window
/// that is only moved changes nothing else.
#[component]
pub fn RememberGeometry(kind: WindowKind) -> impl IntoView {
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
}
