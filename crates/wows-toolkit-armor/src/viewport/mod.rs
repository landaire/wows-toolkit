//! Reusable 3D viewport rendering engine.
//!
//! Renders colored triangle meshes to an offscreen texture and reads the
//! pixels back on the CPU for the UI toolkit to show. It owns its own wgpu
//! device (see `device`) and has no knowledge of game-specific data.

pub mod camera;
pub mod device;
pub mod gizmo;
pub mod picking;
pub mod renderer;
pub mod types;
