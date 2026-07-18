//! GPUI Armor Viewer tab. `viewport_view` owns the 3D viewport: gpui mouse
//! and key input drives the copied `viewport_3d` arcball camera, an
//! on-demand (dirty-gated) offscreen render feeds a gpui `RenderImage`, and a
//! gpui-native overlay reimplements the navigation gizmo on top of it. Ship
//! data loading, the sidebar, picking, and multi-pane comparison are later
//! milestones.

pub mod viewport_view;

pub use viewport_view::ViewportView;
