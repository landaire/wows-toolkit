//! GPUI-free presentation layer for the replay inspector: row model, column
//! formatting, and sort. No `gpui`/`egui` types anywhere in this tree; colors
//! are represented as `ColorRole` and resolved to real colors by the render
//! layer (Milestone 2).
//!
//! Not yet wired into `App` (Milestone 2 mounts the player table); the tree
//! compiles and is unit-tested standalone until then.
#![allow(dead_code)]
// Re-exports below are the module's public surface for Milestone 2+; nothing
// in this milestone's crate consumes them yet.
#![allow(unused_imports)]

pub mod columns;
pub mod model;

#[cfg(test)]
pub(crate) mod test_support;

pub use columns::ReplayColumn;
pub use model::PlayerRow;
pub use model::ReplayReportModel;
pub use model::separate_number;
