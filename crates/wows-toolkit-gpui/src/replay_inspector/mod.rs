//! Presentation layer for the replay inspector: the file-browser grouping
//! model and tree view (`browser`/`browser_view`), cell formatting, sort, and
//! the custom player table (`columns`/`sort`/`table`/`expanded`). The row
//! model and the parse are `wows_toolkit_replay_data`. Colors are represented
//! as `ColorRole` and resolved to real colors by the render layer (`table.rs`,
//! `browser_view.rs`). `load` runs the parse on the background executor
//! (`spawn_parse`); `panel`/`view` tie it all together into the dock tab
//! `app.rs` mounts (one `ReplayPanel` per open replay inside a
//! `ReplayInspectorView`).
#![allow(dead_code)]
// Re-exports below are the module's public surface; not every item is
// consumed outside its own file yet (some are test-only or reserved for a
// later milestone).
#![allow(unused_imports)]

pub mod browser;
pub mod browser_view;
pub mod chat;
pub mod columns;
pub mod controls;
mod debug_view;
pub mod expanded;
pub mod icons;
pub mod load;
pub mod panel;
pub mod sample;
pub mod sort;
pub mod table;
pub mod view;

pub use browser::BrowserNode;
pub use browser::ReplayLite;
pub use browser::build_browser_tree;
pub use browser_view::MissingBuild;
pub use browser_view::ReplayBrowser;
pub use browser_view::ReplayBrowserEvent;
pub use chat::ChatPanel;
pub use columns::CellValue;
pub use columns::ColorRole;
pub use columns::cell_value;
pub use debug_view::RawJsonPanel;
pub use icons::IconCache;
pub use load::spawn_parse;
pub use load::spawn_startup_preload;
pub use panel::ReplayPanel;
pub use sample::sample_model;
pub use sort::SortColumn;
pub use sort::SortOrder;
pub use sort::sort_rows;
pub use table::PlayerTable;
pub use view::InspectorSettings;
pub use view::ReplayInspectorView;
