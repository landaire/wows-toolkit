//! The app's windows: what a window opens showing, and remembering where each
//! kind of window was, in the row every front end shares.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use wows_toolkit_config::WindowKind;
use wows_toolkit_config::WindowSettings;

use crate::renderer::RendererId;
use crate::shell::error::WindowError;

/// The launch parameter naming what a window other than the main one shows.
pub const WINDOW_PARAM: &str = "window";

/// A window size in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowSize {
    pub width: f32,
    pub height: f32,
}

/// A window's size, and whether it fills the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    /// The size it has when neither maximized nor full screen.
    pub size: WindowSize,
    pub maximized: bool,
    pub fullscreen: bool,
}

/// A window of its own the shell asks for (`shell.openWindow`), which is also
/// the launch parameter that tells the bundle what to draw in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WindowRequest {
    TacticsBoard,
    /// A docked viewport moved into a window of its own: the window shows
    /// the live viewport `renderer`, so playback carries on where it was.
    #[serde(rename_all = "camelCase")]
    ReplayRenderer {
        /// The replay shown, which titles the window.
        path: PathBuf,
        renderer: RendererId,
    },
}

impl WindowRequest {
    /// The kind whose geometry the window is placed by and remembered under.
    pub fn kind(&self) -> WindowKind {
        match self {
            WindowRequest::TacticsBoard => WindowKind::TacticsBoard,
            WindowRequest::ReplayRenderer { .. } => WindowKind::ReplayRenderer,
        }
    }

    /// The window's title. A renderer's is its replay's name, as the other
    /// front ends title it.
    pub fn title(&self) -> String {
        match self {
            WindowRequest::TacticsBoard => rust_i18n::t!("ui.windows.tactics_board").into_owned(),
            WindowRequest::ReplayRenderer { path, .. } => path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| rust_i18n::t!("ui.replay.context.render_replay").into_owned()),
        }
    }
}

/// What the shared row remembers about windows of `kind`, `None` the first
/// time one opens or when the row cannot be read (the window then opens at
/// its default, which is what a first launch does).
pub fn remembered(kind: WindowKind) -> Option<WindowSettings> {
    wows_toolkit_config::load_window_settings(kind)
}

/// `geometry` as the shared row holds it, keeping the position `previous`
/// recorded: GTK on Wayland neither reports nor sets a window's position, so
/// the one another front end wrote is left for it.
pub fn merged(previous: Option<WindowSettings>, geometry: WindowGeometry) -> WindowSettings {
    WindowSettings {
        inner_size_points: Some([geometry.size.width, geometry.size.height]),
        outer_position_pixels: previous.and_then(|previous| previous.outer_position_pixels),
        fullscreen: geometry.fullscreen,
        maximized: geometry.maximized,
    }
}

/// Writes `geometry` for windows of `kind`. Blocks: called from the GTK
/// thread when a window has settled or is closing, where the write has to land
/// before the process exits.
pub fn store(kind: WindowKind, geometry: WindowGeometry) -> Result<(), WindowError> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(WindowError::Runtime)?;

    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(WindowError::Open)?;
        let previous = wows_toolkit_config::load_all_window_settings(&pool).await.remove(&kind);
        wows_toolkit_config::store_window_settings(&pool, kind, merged(previous, geometry))
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
        let geometry =
            WindowGeometry { size: WindowSize { width: 1300.0, height: 900.0 }, maximized: true, fullscreen: false };

        let stored = merged(Some(previous), geometry);

        assert_eq!(stored.inner_size_points, Some([1300.0, 900.0]));
        assert_eq!(stored.outer_position_pixels, Some([40.0, 50.0]));
        assert!(stored.maximized);
        assert!(!stored.fullscreen);
    }

    #[test]
    fn requests_read_as_the_shell_sends_them() {
        let sent = serde_json::json!({
            "kind": "replayRenderer",
            "path": "/replays/a.wowsreplay",
            "renderer": 7,
        });
        let renderer: WindowRequest = serde_json::from_value(sent.clone()).unwrap();
        let WindowRequest::ReplayRenderer { path, .. } = &renderer else {
            panic!("a renderer request reads as one: {renderer:?}");
        };
        assert_eq!(path, &PathBuf::from("/replays/a.wowsreplay"));
        // The desktop runner hands the request on to the window as its launch parameter.
        assert_eq!(serde_json::to_value(&renderer).unwrap(), sent);
        assert_eq!(renderer.kind(), WindowKind::ReplayRenderer);
        assert_eq!(renderer.title(), "a");

        let board: WindowRequest = serde_json::from_value(serde_json::json!({ "kind": "tacticsBoard" })).unwrap();
        assert_eq!(board.kind(), WindowKind::TacticsBoard);
        assert!(serde_json::from_value::<WindowRequest>(serde_json::json!({ "kind": "armor" })).is_err());
    }
}
