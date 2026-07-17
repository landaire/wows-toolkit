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
pub mod expanded;
pub mod icons;
pub mod model;
pub mod sample;
pub mod sort;
pub mod table;

#[cfg(test)]
pub(crate) mod test_support;

pub use columns::CellValue;
pub use columns::ColorRole;
pub use columns::ReplayColumn;
pub use columns::cell_value;
pub use columns::default_columns;
pub use columns::separate_number;
pub use model::PlayerRow;
pub use model::ReplayReportModel;
pub use sample::sample_model;
pub use sort::SortColumn;
pub use sort::SortOrder;
pub use sort::sort_rows;
pub use table::PlayerTable;
