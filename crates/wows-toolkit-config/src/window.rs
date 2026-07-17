//! Persisted window geometry types shared between the egui app and GPUI port.
//!
//! The egui-specific constructors/appliers live in the egui app as an
//! extension trait; this crate holds only the serializable data.

use serde::Deserialize;
use serde::Serialize;

/// Identifies which type of window settings to persist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WindowKind {
    Main,
    ReplayRenderer,
    TacticsBoard,
    ArmorViewer,
}

/// Persisted window geometry, modeled after `egui_winit::WindowSettings`.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    pub inner_size_points: Option<[f32; 2]>,
    pub outer_position_pixels: Option<[f32; 2]>,
    pub fullscreen: bool,
    pub maximized: bool,
}
