//! Local annotation types and annotation-layer state for the web client.
//!
//! Shared egui types (`Annotation`, `PaintTool`, `AnnotationState`, constants,
//! and wire<->local conversion functions) live in `wt_collab_egui::types` and are
//! re-exported here for convenience.

// Re-export shared types so existing `crate::types::*` imports keep working.
#[allow(unused_imports)] // used by app.rs; dead on non-WASM native check
pub use wt_collab_egui::types::Annotation;
#[allow(unused_imports)] // used by app.rs; dead on non-WASM native check
pub use wt_collab_egui::types::AnnotationSnapshot;
#[allow(unused_imports)] // used by app.rs; dead on non-WASM native check
pub use wt_collab_egui::types::AnnotationState;
#[allow(unused_imports)] // used by app.rs; dead on non-WASM native check
pub use wt_collab_egui::types::ENEMY_COLOR;
#[allow(unused_imports)] // used by app.rs; dead on non-WASM native check
pub use wt_collab_egui::types::FRIENDLY_COLOR;
#[allow(unused_imports)] // used by app.rs; dead on non-WASM native check
pub use wt_collab_egui::types::PaintTool;
#[allow(unused_imports)] // used by app.rs; dead on non-WASM native check
pub use wt_collab_egui::types::SHIP_SPECIES;
#[allow(unused_imports)] // used by app.rs; dead on non-WASM native check
pub use wt_collab_egui::types::local_to_wire;
#[allow(unused_imports)] // used by app.rs; dead on non-WASM native check
pub use wt_collab_egui::types::wire_to_local;
