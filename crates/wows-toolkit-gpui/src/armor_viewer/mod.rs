//! GPUI Armor Viewer tab. The renderer-independent pieces (viewport engine,
//! ship armor load, mesh upload, visibility state, analysis overlays) live in
//! `wows_toolkit_armor`; this module presents them. `viewport_view` owns the
//! 3D viewport: gpui_kit mouse and key input drives the arcball camera, an
//! on-demand (dirty-gated) offscreen render feeds a gpui_kit `RenderImage`
//! (`render_image`), and a gpui-native overlay reimplements the navigation
//! gizmo on top of it. `assets` loads the shared `ShipAssets` plus the catalog
//! and its nation-flag/class-icon cache on the background executor. `sidebar`
//! presents the catalog as a nation/class/ship tree with a search filter;
//! `load_ship` runs one ship's armor load on the background executor. `pane`
//! ties all of the above into the tab `app.rs` mounts. `picking_ui` adds plate
//! hover highlight, thickness tooltip, and plate visibility toggles.
//! `popover` builds the options rail's tools, the armor-visibility tree and
//! the hull-visibility tree with its camo picker. `dock` lays out one or more
//! panes for comparison.
#![allow(dead_code)]
// Re-exports below are the module's public surface; not everything is
// consumed outside its own file yet.
#![allow(unused_imports)]

pub mod analysis;
pub mod assets;
pub mod dock;
mod export_dialog;
pub mod legend;
mod load_ship;
pub mod options_panel;
pub mod pane;
pub mod picking_ui;
pub mod popover;
mod render_image;
pub mod sidebar;
pub mod viewport_view;

pub use assets::ArmorAssetsBundle;
pub use assets::spawn_load_armor_assets;
pub use pane::ArmorViewerPane;
pub use sidebar::CompareSplit;
pub use sidebar::ShipSelected;
pub use sidebar::Sidebar;
pub use viewport_view::ViewportView;
