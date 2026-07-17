//! Presentation layer for the replay inspector: the file-browser grouping
//! model and tree view (`browser`/`browser_view`), the row model, column
//! formatting, sort, and the custom player table (`model`/`columns`/`sort`/
//! `table`/`expanded`). Colors are represented as `ColorRole` and resolved to
//! real colors by the render layer (`table.rs`, `browser_view.rs`).
#![allow(dead_code)]
// Re-exports below are the module's public surface; not every item is
// consumed outside its own file yet (some are test-only or reserved for a
// later milestone, e.g. the dock wiring that will consume
// `ReplayBrowserEvent`).
#![allow(unused_imports)]

pub mod browser;
pub mod browser_view;
pub mod columns;
pub mod expanded;
pub mod icons;
pub mod model;
pub mod sample;
pub mod sort;
pub mod table;

#[cfg(test)]
pub(crate) mod test_support;

pub use browser::BrowserNode;
pub use browser::ReplayLite;
pub use browser::build_browser_tree;
pub use browser_view::ReplayBrowser;
pub use browser_view::ReplayBrowserEvent;
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
