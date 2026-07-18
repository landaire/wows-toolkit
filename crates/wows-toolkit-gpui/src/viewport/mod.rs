//! Reusable 3D viewport rendering engine.
//!
//! This module provides a generic 3D viewport that renders colored triangle
//! meshes to an offscreen texture, reads the pixels back on the CPU, and hands
//! them to gpui as a `RenderImage`. It owns its own wgpu device (see `device`)
//! and has no knowledge of game-specific data.
//!
//! Ported from the egui app's `viewport_3d` module: the wgpu pipeline, shader,
//! mesh handling, matrix math, and CPU picking are reused verbatim; the egui
//! presentation path (on-screen texture registration, `egui::Response` input,
//! `egui::Painter` gizmo drawing) is dropped in favor of an offscreen readback
//! and plain-typed camera/picking signatures.
//!
//! The full public surface (textured meshes, overlays, picking variants, the
//! gizmo math) is consumed incrementally by the later armor-viewer tasks, so
//! `dead_code` is allowed at the module level while that build-out proceeds.
#![allow(dead_code)]

pub mod camera;
pub mod device;
pub mod gizmo;
pub mod picking;
pub mod renderer;
pub mod types;
