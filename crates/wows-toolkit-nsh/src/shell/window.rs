//! Remembering where the main window was, in the row every front end shares.

use wows_toolkit_config::WindowKind;
use wows_toolkit_config::WindowSettings;

use crate::shell::error::WindowError;

/// A window size in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowSize {
    pub width: f32,
    pub height: f32,
}

/// The main window's size, and whether it fills the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MainWindowGeometry {
    /// The size it has when neither maximized nor full screen.
    pub size: WindowSize,
    pub maximized: bool,
    pub fullscreen: bool,
}

/// What the shared row remembers about the main window, `None` on a first
/// launch or when the row cannot be read (the window then opens at its
/// default, which is what a first launch does).
pub fn remembered() -> Option<WindowSettings> {
    wows_toolkit_config::load_main_window_settings()
}

/// `geometry` as the shared row holds it, keeping the position `previous`
/// recorded: GTK on Wayland neither reports nor sets a window's position, so
/// the one another front end wrote is left for it.
pub fn merged(previous: Option<WindowSettings>, geometry: MainWindowGeometry) -> WindowSettings {
    WindowSettings {
        inner_size_points: Some([geometry.size.width, geometry.size.height]),
        outer_position_pixels: previous.and_then(|previous| previous.outer_position_pixels),
        fullscreen: geometry.fullscreen,
        maximized: geometry.maximized,
    }
}

/// Writes `geometry` for the main window. Blocks: called from the GTK thread
/// when the window has settled or is closing, where the write has to land
/// before the process exits.
pub fn store(geometry: MainWindowGeometry) -> Result<(), WindowError> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(WindowError::Runtime)?;

    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(WindowError::Open)?;
        let previous = wows_toolkit_config::load_all_window_settings(&pool).await.remove(&WindowKind::Main);
        wows_toolkit_config::store_window_settings(&pool, WindowKind::Main, merged(previous, geometry))
            .await
            .map_err(WindowError::Write)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_another_front_end_recorded_is_kept() {
        let previous = WindowSettings {
            inner_size_points: Some([800.0, 600.0]),
            outer_position_pixels: Some([40.0, 50.0]),
            fullscreen: false,
            maximized: false,
        };
        let geometry = MainWindowGeometry {
            size: WindowSize { width: 1300.0, height: 900.0 },
            maximized: true,
            fullscreen: false,
        };

        let stored = merged(Some(previous), geometry);

        assert_eq!(stored.inner_size_points, Some([1300.0, 900.0]));
        assert_eq!(stored.outer_position_pixels, Some([40.0, 50.0]));
        assert!(stored.maximized);
        assert!(!stored.fullscreen);
    }
}
