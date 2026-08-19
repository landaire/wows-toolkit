/// Ballistics and the per-plate penetration chain live in `wowsunpack`;
/// re-exported so armor-viewer paths keep working.
pub use wowsunpack::ballistics;
pub(crate) mod camera_ellipse;
pub(crate) mod camera_perspective;
pub mod common;
pub mod constants;
pub mod export_dialog;
pub mod penetration;
pub mod ship_selector;
pub mod splash;
pub mod state;
pub mod ui;

#[cfg(test)]
mod render_smoke;

pub use state::ArmorViewerState;
