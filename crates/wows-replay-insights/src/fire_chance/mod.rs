//! Effective fire chance: derived stats over a replay's burn history.

#[cfg(feature = "build")]
pub mod analysis;
pub mod geometry;
#[cfg(feature = "build")]
pub mod resolve;
#[cfg(feature = "fire-sections")]
pub mod sections;
pub mod victim;
