//! GPUI Armor Viewer tab. `viewport_view` owns the 3D viewport: gpui mouse
//! and key input drives the copied `viewport_3d` arcball camera, an
//! on-demand (dirty-gated) offscreen render feeds a gpui `RenderImage`, and a
//! gpui-native overlay reimplements the navigation gizmo on top of it.
//! `catalog` builds the ship selector's nation/class/ship listing from the
//! loaded `GameMetadataProvider`; `assets` loads the shared `ShipAssets`
//! (ship-export data) plus the catalog and its nation-flag/class-icon cache
//! on the background executor. The sidebar/tree that presents this data,
//! ship picking, and multi-pane comparison are later milestones.
#![allow(dead_code)]
// Re-exports below are the module's public surface for the sidebar/tree
// milestone that consumes them; not everything is wired up yet.
#![allow(unused_imports)]

pub mod assets;
pub mod catalog;
pub mod viewport_view;

pub use assets::ArmorAssetsBundle;
pub use assets::ArmorAssetsError;
pub use assets::spawn_load_armor_assets;
pub use catalog::ShipCatalog;
pub use viewport_view::ViewportView;
