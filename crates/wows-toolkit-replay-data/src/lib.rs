//! Game-data loading, replay parsing into the presentation model, the replay
//! index pass, the replay directory registry and game-data cache maintenance,
//! shared by the desktop front ends. Blocking functions and plain data only:
//! each front end runs them on its own executor.

rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

pub mod browser_tree;
pub mod cache;
pub mod cells;
pub mod chat;
pub mod columns;
pub mod constants;
pub mod directories;
pub mod directory_stats;
pub mod error;
pub mod export;
pub mod links;
pub mod live_match;
pub mod load;
pub mod minimap;
pub mod model;
pub mod palette;
pub mod personal_rating;
pub mod playback;
pub mod player_table;
pub mod profiling;
pub mod render_defaults;
pub mod replay_index;
pub mod replay_scan;
pub mod row_detail;
pub mod row_icons;
#[cfg(any(test, feature = "test-support"))]
pub mod sample;
pub mod shareable;
pub mod sort;
pub mod summary;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
