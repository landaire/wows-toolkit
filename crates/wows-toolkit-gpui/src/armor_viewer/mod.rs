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
//! toggles. `visibility` holds the zone/part/plate override maps' shared
//! types (`VisibilityFilter`, undo/redo snapshot stack, sidebar-highlight
//! key) plus the tri-state tree's pure derivation logic; `popover` builds the
//! toolbar button and the armor-visibility popover's gpui tree on top of it,
//! plus (Milestone 4 Task 8a) the hull-visibility popover's tree, backed by
//! `upload_hull`'s hull-only mesh upload (kept separate from `upload`'s armor
//! upload so a hull-only change never rebuilds the armor mesh set).
//! `viewport_view.rs` owns the actual `part_visibility`/`plate_visibility`/
//! `hull_visibility`/`selected_camo`/undo-stack state and wires all of the
//! above into the mouse and key handlers. `camo` (Milestone 4 Task 8b) is the
//! pure pixel-math camo compositor (`build_active_camo`) that bakes a decoded
//! camo scheme against the hull's base albedo; `popover`'s hull popover adds
//! the camo picker on top of it. Multi-pane comparison (M5) and hull
//! upgrade-LOD reload (M4 Task 8c) are later milestones. `dock` (M5 Task 9a)
//! is the chrome-less scaffold `pane` wraps the viewport in, ready for Task
//! 9b to turn into a real multi-pane split.
#![allow(dead_code)]
// Re-exports below are the module's public surface; not everything is
// consumed outside its own file yet (some are reserved for a later
// milestone, matching `replay_inspector::mod`'s own convention).
#![allow(unused_imports)]

pub mod assets;
pub(crate) mod camo;
pub mod catalog;
pub mod dock;
pub mod legend;
pub mod load_ship;
pub mod pane;
pub mod picking_ui;
pub mod popover;
pub mod sidebar;
pub mod upload;
pub mod upload_hull;
pub mod viewport_view;
pub mod visibility;

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
