//! Game-data loading, replay parsing into the presentation model and the
//! replay index pass for the desktop app. Blocking functions and plain data
//! only: the app schedules them on its own executor.

pub mod browser_tree;
pub mod cap_layout_store;
pub mod columns;
pub mod damage_mix;
pub mod directory_stats;
pub mod drawing_style;
pub mod error;
pub mod fire_chance;
pub mod force_reindex;
pub mod frame;
pub mod heap_profile;
pub mod install;
pub mod links;
pub mod live_match;
pub mod load;
pub mod loader;
pub mod minimap_preview;
pub mod model;
#[cfg(any(test, feature = "test-support"))]
pub mod parse_count;
pub mod paths;
pub mod recording;
pub mod render_defaults;
pub mod replay_dir;
pub mod replay_folder;
pub mod replay_index;
pub mod replay_render;
#[cfg(any(test, feature = "test-support"))]
pub mod sample;
pub mod shareable;
pub mod ship_list;
#[cfg(any(test, feature = "test-support"))]
pub mod simulated_panic;
#[cfg(any(test, feature = "test-support"))]
pub mod simulated_watch;
pub mod sort;
pub mod tactics;
pub mod team_totals;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
