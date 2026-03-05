//! Shared egui types, coordinate transforms, and rendering functions for
//! WoWs Toolkit collaborative sessions.
//!
//! Used by both the desktop app (`wows-toolkit`) and the WASM web client
//! (`wt-web`) to avoid duplicating annotation types and minimap rendering.

pub mod draw_commands;
pub mod interaction;
pub mod rendering;
pub mod toolbar;
pub mod transforms;
pub mod types;
