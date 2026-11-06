//! Queries over World of Warships game data for chat bots and MCP clients.
//!
//! Load a [`GameData`] once from a wows-replay-data dump, then call its query
//! methods. Argument types derive `JsonSchema` for tool calling; outputs
//! serialize to compact JSON.

pub mod armor;
pub mod consumables;
pub mod data;
#[cfg(feature = "download")]
pub mod download;
pub mod error;
pub mod loadout;
pub mod penetration;
pub mod replay;
pub mod shells;
pub mod ships;
pub mod skills;
pub mod stats;
pub mod tools;
pub mod upgrades;

mod text;

pub use data::GameData;
pub use error::LoadError;
pub use error::QueryError;
