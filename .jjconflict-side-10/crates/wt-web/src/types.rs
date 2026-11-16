//! Local annotation types and annotation-layer state for the web client.
//!
//! Shared egui types (`Annotation`, `PaintTool`, `AnnotationState`, constants,
//! and wire<->local conversion functions) live in `wt_collab_egui::types` and are
//! re-exported here for convenience.

// Re-export shared types so existing `crate::types::*` imports keep working.
pub use wt_collab_egui::types::Annotation;
pub use wt_collab_egui::types::AnnotationState;
pub use wt_collab_egui::types::PaintTool;
pub use wt_collab_egui::types::local_to_wire;
pub use wt_collab_egui::types::wire_to_local;
