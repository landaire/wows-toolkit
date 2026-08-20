//! Typed models and parsers for the WGC update-service (WGUS) XML protocol.
//!
//! `game_info` parses the on-disk `game_info.xml` WGC writes per install.
//! `models` types the two catalog XML responses (`metadata`, `patches_chain`).
//! No network access here; see `wgc-catalog`'s design doc for the client
//! that will fetch these documents.

pub mod error;
pub mod game_info;
pub mod models;

pub use error::CatalogError;
