//! SQLite persistence layer.
//!
//! Replaces eframe's RON-based `app.ron` persistence with a SQLite database.
//! On first launch after the migration, `app.ron` is read and its contents are
//! written into the database. Subsequent launches read directly from SQLite.
//!
//! The egui-free plumbing (connection, migrations, queries, storage paths) lives
//! in `wows-toolkit-config` and is re-exported here so `crate::db::*` call sites
//! keep working.

pub mod load;
pub mod migrate_ron;
pub mod save;

// Kept for `crate::db::*` API parity even though no call site in this crate
// currently needs it directly (the config crate uses its own local db_path).
#[allow(unused_imports)]
pub use wows_toolkit_config::db_path;
// Populated by later index tasks; no call site in this crate yet.
#[allow(unused_imports)]
pub use wows_toolkit_config::index;
pub use wows_toolkit_config::is_migrated;
pub use wows_toolkit_config::load_main_window_settings;
pub use wows_toolkit_config::open_db;
pub use wows_toolkit_config::queries;
pub use wows_toolkit_config::set_migrated;
