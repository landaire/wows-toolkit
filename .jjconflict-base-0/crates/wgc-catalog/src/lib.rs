//! Typed models, parsers, and an async client for the WGC update-service
//! (WGUS) XML protocol.
//!
//! `game_info` parses the on-disk `game_info.xml` WGC writes per install, and
//! also builds a fresh (nothing-installed) `GameInfo` for games not yet on
//! disk. `known_games` is a built-in registry of such fresh descriptors.
//! `models` types the two catalog XML responses (`metadata`, `patches_chain`).
//! `client` fetches those documents over HTTP. `update` compares an
//! install's part versions against a fetched chain to report what is out
//! of date.

pub mod client;
pub mod error;
pub mod game_info;
pub mod known_games;
pub mod models;
pub mod update;

pub use client::CatalogClient;
pub use error::CatalogError;
