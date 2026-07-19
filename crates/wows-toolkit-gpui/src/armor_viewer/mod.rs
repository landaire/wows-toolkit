//! GPUI Armor Viewer tab. `viewport_view` owns the 3D viewport: gpui mouse
//! and key input drives the copied `viewport_3d` arcball camera, an
//! on-demand (dirty-gated) offscreen render feeds a gpui `RenderImage`, and a
//! gpui-native overlay reimplements the navigation gizmo on top of it.
//! `catalog` builds the ship selector's nation/class/ship listing from the
//! loaded `GameMetadataProvider`; `assets` loads the shared `ShipAssets`
//! (ship-export data) plus the catalog and its nation-flag/class-icon cache
//! on the background executor. `sidebar` presents the catalog as a
//! nation/class/ship tree with a search filter and emits a selection event;
//! `load_ship` loads one ship's armor model on the background executor;
//! `upload` uploads a loaded ship's armor meshes into a `Viewport3D`. `pane`
//! ties all of the above into the single-pane tab `app.rs` mounts: sidebar on
//! the left, the 3D viewport on the right, wired so picking a ship loads and
//! displays its armor. `picking_ui` adds CPU plate picking on top: hover
//! highlight + thickness tooltip, and click/context-menu plate visibility
//! toggles (`viewport_view.rs` owns the actual hover/`plate_visibility`
//! state and wires it into the mouse handlers). Multi-pane comparison (M5),
//! hull/camo display (M4), and visibility undo/redo (M3 Task 7) are later
//! milestones.
#![allow(dead_code)]
// Re-exports below are the module's public surface; not everything is
// consumed outside its own file yet (some are reserved for a later
// milestone, matching `replay_inspector::mod`'s own convention).
#![allow(unused_imports)]

pub mod assets;
pub mod catalog;
pub mod legend;
pub mod load_ship;
pub mod pane;
pub mod picking_ui;
pub mod sidebar;
pub mod upload;
pub mod viewport_view;

pub use assets::ArmorAssetsBundle;
pub use assets::ArmorAssetsError;
pub use assets::spawn_load_armor_assets;
pub use catalog::ShipCatalog;
pub use load_ship::LoadedShipArmor;
pub use load_ship::ShipLoadError;
pub use pane::ArmorViewerPane;
pub use sidebar::ShipSelected;
pub use sidebar::Sidebar;
pub use viewport_view::ViewportView;
