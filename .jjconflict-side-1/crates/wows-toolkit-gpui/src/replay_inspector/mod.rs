//! Presentation layer for the replay inspector: the file-browser tree view
//! (`browser_view`) and the custom player table (`table`/`expanded`) over the
//! row model, columns and sort of `wows_toolkit_replay_data`. Colors arrive as
//! `ColorRole` and are resolved to real colors here (`table.rs`,
//! `browser_view.rs`, `player_colors.rs`). `load`
//! runs the background replay parse (`spawn_parse`) that produces a
//! `ReplayReportModel` from a `.wowsreplay` file; `panel`/`view` tie it all
//! together into the dock tab `app.rs` mounts (one `ReplayPanel` per open
//! replay inside a `ReplayInspectorView`).
#![allow(dead_code)]
// Re-exports below are the module's public surface; not every item is
// consumed outside its own file yet (some are test-only or reserved for a
// later milestone).
#![allow(unused_imports)]

pub mod browser_view;
pub mod chat;
pub mod controls;
pub mod damage_mix;
mod debug_view;
mod empty_map;
pub mod expanded;
pub mod icons;
pub mod load;
mod map_thumbnails;
pub mod panel;
pub(crate) mod player_colors;
pub mod table;
mod team_bands;
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

// Removed once armor viewer and replay renderer import from the shared crate.
pub use wows_toolkit_replay_data::load::GameDataCache;
