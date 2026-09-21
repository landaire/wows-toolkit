//! Toolkit logic that decides what the UI shows, independent of any UI
//! toolkit.
//!
//! The egui app and the GPUI port both render from these types, so a rule
//! about what to list, filter, sort or queue has one implementation rather
//! than one per front end that can drift apart.

pub mod match_stats;
pub mod personal_rating;
pub mod player_tracker;
pub mod settings;
pub mod stats;
pub mod unpacker;
