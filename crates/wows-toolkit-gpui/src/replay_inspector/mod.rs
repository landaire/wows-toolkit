//! Presentation layer for the replay inspector: the file-browser grouping
//! model and tree view (`browser`/`browser_view`), the row model, column
//! formatting, sort, and the custom player table (`model`/`columns`/`sort`/
//! `table`/`expanded`). Colors are represented as `ColorRole` and resolved to
//! real colors by the render layer (`table.rs`, `browser_view.rs`). `load`
//! runs the background replay parse (`spawn_parse`) that produces a
//! `ReplayReportModel` from a `.wowsreplay` file; `panel`/`view` tie it all
//! together into the dock tab `app.rs` mounts (one `ReplayPanel` per open
//! replay inside a `ReplayInspectorView`).
#![allow(dead_code)]
// Re-exports below are the module's public surface; not every item is
// consumed outside its own file yet (some are test-only or reserved for a
// later milestone).
#![allow(unused_imports)]

pub mod browser;
pub mod browser_view;
pub mod chat;
pub mod columns;
pub mod expanded;
pub mod icons;
pub mod load;
pub mod model;
pub mod panel;
pub mod sample;
pub mod sort;
pub mod table;
pub mod view;

#[cfg(test)]
pub(crate) mod test_support;

pub use browser::BrowserNode;
pub use browser::ReplayLite;
pub use browser::build_browser_tree;
pub use browser_view::ReplayBrowser;
pub use browser_view::ReplayBrowserEvent;
pub use chat::ChatPanel;
pub use columns::CellValue;
pub use columns::ColorRole;
pub use columns::ReplayColumn;
pub use columns::cell_value;
pub use columns::default_columns;
pub use columns::separate_number;
pub use load::GameDataCache;
pub use load::GameDataStatus;
pub use load::ReplayLoadError;
pub use load::spawn_parse;
pub use load::spawn_startup_preload;
pub use model::ChatMessage;
pub use model::PlayerRow;
pub use model::ReplayReportModel;
pub use panel::ReplayPanel;
pub use sample::sample_model;
pub use sort::SortColumn;
pub use sort::SortOrder;
pub use sort::sort_rows;
pub use table::PlayerTable;
pub use view::ReplayInspectorView;
