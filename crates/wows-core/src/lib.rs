//! Lightweight domain core for the WoWS toolkit.
//!
//! Holds shared types and small derived data with minimal dependencies, so lean
//! consumers (e.g. a wasm viewer) can depend on just the types without pulling
//! in the heavy unpacking/parsing stack that lives in `wowsunpack`.

pub mod recognized;
pub mod version;

pub use version::Version;

/// Reference-counted pointer, unified across the workspace by the `arc` feature.
/// With `arc` it is `Arc` (thread-safe); without it, `Rc`.
#[cfg(feature = "arc")]
pub type Rc<T> = std::sync::Arc<T>;

#[cfg(not(feature = "arc"))]
pub type Rc<T> = std::rc::Rc<T>;
