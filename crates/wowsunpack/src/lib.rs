/// Utilities for interacting with the game's data files
pub mod data;
/// Error definitions
pub mod error;
/// Export functionality (glTF, etc.)
pub mod export;
/// Constants parsed from the game's XML files in `res/gui/data/constants/`
pub mod game_constants;
/// Utilities for loading game resources from a WoWS installation directory.
pub mod game_data;
/// Utilities for interacting with the `content/GameParams.data` file
pub mod game_params;
/// Game concept types (entities, positions, enums) useful across WoWS tools.
pub mod game_types;
/// 3D model formats (geometry, visual, etc.)
pub mod models;
/// Generic wrapper for values that may or may not match a known variant.
pub mod recognized;
/// Utilities involving the game's RPC functions -- useful for parsing entity defs and RPC definitions.
pub mod rpc;

#[cfg(feature = "vfs")]
pub use vfs;

#[cfg(feature = "arc")]
pub type Rc<T> = std::sync::Arc<T>;

#[cfg(not(feature = "arc"))]
pub type Rc<T> = std::rc::Rc<T>;
