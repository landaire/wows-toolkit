//! Queries over World of Warships game data for chat bots and MCP clients.
//!
//! Load a [`GameData`] once from a wows-replay-data dump, then call its query
//! methods. Argument types derive `JsonSchema` for tool calling; outputs
//! serialize to compact JSON. Programs that serve `gameparams_query` call
//! [`jq_worker::init`] first in `main`.

pub mod armor;
pub mod consumables;
pub mod data;
#[cfg(feature = "download")]
pub mod download;
pub mod error;
pub mod gameparams;
pub mod jq_worker;
pub mod loadout;
pub mod modules;
pub mod penetration;
pub mod raw_params;
pub mod replay;
pub mod shells;
pub mod ships;
pub mod skills;
pub mod stats;
pub mod tools;
pub mod upgrades;

mod fuzzy;
mod jq;
mod text;

pub use data::GameData;
pub use error::LoadError;
pub use error::QueryError;
