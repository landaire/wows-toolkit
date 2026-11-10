//! Game-data loading, replay parsing into the presentation model, the replay
//! index pass, the replay directory registry and game-data cache maintenance,
//! shared by the desktop front ends. Blocking functions and plain data only:
//! each front end runs them on its own executor.

pub mod cache;
pub mod columns;
pub mod constants;
pub mod directories;
pub mod directory_stats;
pub mod error;
pub mod links;
pub mod load;
pub mod live_match;
pub mod minimap;
pub mod model;
pub mod personal_rating;
pub mod playback;
pub mod profiling;
pub mod render_defaults;
pub mod replay_index;
pub mod shareable;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
