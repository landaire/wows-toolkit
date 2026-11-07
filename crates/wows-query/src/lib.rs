//! Queries over World of Warships game data for chat bots and MCP clients.
//!
//! Load a [`GameData`] once from a wows-replay-data dump, then call its query
//! methods. Argument types derive `JsonSchema` for tool calling; outputs
//! serialize to compact JSON. Programs that serve `gameparams_query` call
//! [`jq_worker::init`] first in `main`; programs that read replays call
//! [`replay_worker::init`] there too.

pub mod aircraft;
pub mod armament;
pub mod armor;
pub mod armor_assessment;
mod armor_models;
pub mod armor_trace;
pub mod builds;
pub mod citadel;
pub mod commanders;
pub mod concealment;
pub mod consumables;
pub mod data;
#[cfg(feature = "download")]
pub mod download;
pub mod error;
pub mod fire_target;
pub mod fires;
pub mod firing_arcs;
pub mod gameparams;
pub mod jq_worker;
pub mod loadout;
pub mod modules;
pub mod names;
pub mod penetration;
pub mod rates;
pub mod raw_params;
pub mod replay;
pub mod replay_worker;
pub mod shells;
pub mod ship_variants;
pub mod ships;
pub mod skills;
pub mod special_abilities;
pub mod stat_search;
pub mod stats;
pub mod tech_tree;
pub mod tools;
pub mod turtleback;
pub mod upgrades;

mod fuzzy;
mod jq;
mod sandbox;
mod text;

pub use data::GameData;
pub use error::LoadError;
pub use error::QueryError;
pub use names::Alias;
pub use names::AliasError;
pub use names::AliasTarget;
pub use names::Interpretation;
pub use names::Mention;
pub use names::NameKind;
pub use names::ResolutionMethod;
