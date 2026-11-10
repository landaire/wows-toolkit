//! Presentation layer for the replay inspector: the file-browser tree view
//! (`browser_view`) and the custom player table (`table`/`expanded`). The row
//! model, the parse, the browser grouping, cell formatting and sort are
//! `wows_toolkit_replay_data`. Colors are represented as `ColorRole` and
//! resolved to real colors by the render layer (`columns`, `table.rs`,
//! `browser_view.rs`). `load` runs the parse on the background executor
//! (`spawn_parse`); `panel`/`view` tie it all together into the dock tab
//! `app.rs` mounts (one `ReplayPanel` per open replay inside a
//! `ReplayInspectorView`).
#![allow(dead_code)]
// Re-exports below are the module's public surface; not every item is
// consumed outside its own file yet (some are test-only or reserved for a
// later milestone).
#![allow(unused_imports)]

pub mod browser_view;
pub mod chat;
pub mod columns;
pub mod controls;
mod debug_view;
pub mod expanded;
pub mod icons;
pub mod load;
pub mod panel;
pub mod table;
pub mod view;

pub use browser_view::ReplayBrowser;
pub use browser_view::ReplayBrowserEvent;
pub use chat::ChatPanel;
pub use debug_view::RawJsonPanel;
pub use icons::IconCache;
pub use load::spawn_parse;
pub use load::spawn_startup_preload;
pub use panel::ReplayPanel;
pub use table::PlayerTable;
pub use view::InspectorSettings;
pub use view::ReplayInspectorView;
