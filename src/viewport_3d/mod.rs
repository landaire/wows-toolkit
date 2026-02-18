//! Reusable 3D viewport rendering engine.
//!
//! This module provides a generic 3D viewport that renders colored triangle
//! meshes to an offscreen texture, with an arcball camera, CPU picking, and
//! standard mouse navigation. It has no knowledge of game-specific data.
//!
//! # Usage
//!
//! ```ignore
//! // One-time: create the shared GPU pipeline
//! let pipeline = GpuPipeline::new(&device);
//!
//! // Per viewport: create a Viewport3D
//! let mut viewport = Viewport3D::new();
//! let mesh_id = viewport.add_mesh(&device, &vertices, &indices);
//!
//! // Each frame: render and display
//! if let Some(tex_id) = viewport.render(&render_state, &pipeline, (width, height)) {
//!     ui.image(egui::ImageSource::Texture(SizedTexture::new(tex_id, size)));
//! }
//!
//! // Handle camera input
//! viewport.handle_input(&response, Some((min_bounds, max_bounds)));
//!
//! // Pick on hover
//! if let Some(hit) = viewport.pick(mouse_pos, viewport_rect) {
//!     // hit.mesh_id, hit.triangle_index, hit.world_position
//! }
//! ```

pub mod camera;
pub mod picking;
pub mod renderer;
pub mod types;

pub use camera::ArcballCamera;
pub use renderer::{GpuPipeline, LAYER_DEFAULT, LAYER_HULL, LAYER_OVERLAY, Viewport3D};
pub use types::{HitResult, MeshId, Vertex};
