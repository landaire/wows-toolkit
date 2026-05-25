//! Derived projections over a parsed WoWs replay.
//!
//! Sits above `wows_replays` and `wowsunpack` and below any GUI. Consumers include
//! the desktop app, the headless minimap renderer, the CLI, and external tools
//! such as Discord bots.

#[cfg(feature = "build")]
pub mod build;

#[cfg(feature = "build")]
pub use build::ResolvedBuild;
