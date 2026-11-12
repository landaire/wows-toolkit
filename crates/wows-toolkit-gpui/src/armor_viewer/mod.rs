//! GPUI Armor Viewer tab, hosting the scene logic and viewport engine of
//! `wows_toolkit_armor`. `viewport_view` owns the 3D viewport: gpui_kit mouse
//! and key input drives the arcball camera, an on-demand (dirty-gated) render
//! on GPUI's own device fills a texture GPUI samples directly, and a
//! gpui-native overlay draws the navigation gizmo, labels, tooltips
//! (`picking_ui`) and depth cues (`depth_cues`) on top of it. It also owns the
//! visibility, camouflage and undo state and wires it into the mouse and key
//! handlers.
//! `assets` runs the armor crate's synchronous loaders on the background
//! executor and decodes the catalog's nation flags and class icons. `sidebar`
//! lists the ships, their parts and every catalogue model in one searchable
//! tree (`sidebar_tree` builds it) and emits selection events. `popover`
//! builds the options rail's tools and the armor and hull visibility trees.
//! `pane` ties these into one pane: sidebar on the left, viewport on the
//! right. `dock` lays out one or more panes itself (see its module doc); the
//! sidebar's "Compare" button (`CompareSplit`) adds a pane, and a ship
//! selection routes to the dock's active pane.
#![allow(dead_code)]
// Re-exports below are the module's public surface; not everything is
// consumed outside its own file yet (some are reserved for a later
// milestone, matching `replay_inspector::mod`'s own convention).
#![allow(unused_imports)]

pub mod analysis;
pub mod assets;
mod depth_cues;
pub mod dock;
mod export_dialog;
pub mod legend;
pub mod options_panel;
pub mod pane;
pub mod picking_ui;
pub mod popover;
pub mod sidebar;
mod sidebar_tree;
mod trajectory_panel;
mod tree_filter;
pub mod viewport_view;
pub mod xray;

pub use assets::ArmorAssetsBundle;
pub use assets::spawn_load_armor_assets;
pub use pane::ArmorViewerPane;
pub use sidebar::CompareSplit;
pub use sidebar::ShipSelected;
pub use sidebar::Sidebar;
pub use viewport_view::ViewportView;
