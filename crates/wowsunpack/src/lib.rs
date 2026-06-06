#![allow(dead_code)]

#[cfg(feature = "parsing")]
#[macro_use]
mod variant_accessors;

/// Utilities for resolving raw battle results arrays into named JSON objects.
#[cfg(feature = "json")]
pub mod battle_results;
/// Per-version consumable id -> name tables recovered by static analysis of game scripts.
#[cfg(feature = "parsing")]
pub mod consumable_versions;
/// Utilities for interacting with the game's data files
#[cfg(feature = "parsing")]
pub mod data;
/// Error definitions
#[cfg(feature = "parsing")]
pub mod error;
/// Export functionality (glTF, etc.)
#[cfg(feature = "parsing")]
pub mod export;
/// Version-aware GUI asset resolution: request assets by type, not file path.
/// Resolves and reads through the VFS, so it requires the `vfs` feature.
#[cfg(feature = "vfs")]
pub mod game_assets;
/// Constants parsed from the game's XML files in `res/gui/data/constants/`
#[cfg(feature = "parsing")]
pub mod game_constants;
/// Utilities for loading game resources from a WoWS installation directory.
#[cfg(feature = "vfs-mmap")]
pub mod game_data;
/// Utilities for interacting with the `content/GameParams.data` file
#[cfg(feature = "parsing")]
pub mod game_params;
/// Game concept types (entities, positions, enums) useful across WoWS tools.
/// Live in `wows-core`; re-exported so existing `wowsunpack::game_types` paths
/// keep working.
pub use wows_core::game_types;
/// 3D model formats (geometry, visual, etc.)
#[cfg(feature = "parsing")]
pub mod models;
/// Generic wrapper for values that may or may not match a known variant.
/// Lives in `wows-core`; re-exported here so existing `wowsunpack::recognized`
/// paths keep working.
pub use wows_core::recognized;
/// Utilities involving the game's RPC functions -- useful for parsing entity defs and RPC definitions.
#[cfg(feature = "parsing")]
pub mod rpc;

#[cfg(feature = "vfs")]
pub use vfs;

#[cfg(feature = "arc")]
pub type Rc<T> = std::sync::Arc<T>;

#[cfg(not(feature = "arc"))]
pub type Rc<T> = std::rc::Rc<T>;
