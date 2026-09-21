//! Toolkit logic that decides what the UI shows, independent of any UI
//! toolkit.
//!
//! The egui app and the GPUI port both render from these types, so a rule
//! about what to list, filter, sort or queue has one implementation rather
//! than one per front end that can drift apart.

rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

pub mod formatting;
pub mod match_stats;
pub mod personal_rating;
pub mod player_tracker;
pub mod query_bar;
pub mod replay_export;
pub mod search;
pub mod settings;
pub mod stats;
pub mod twitch;
pub mod unpacker;
