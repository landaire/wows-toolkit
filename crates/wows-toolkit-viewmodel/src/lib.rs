//! Toolkit logic that decides what the UI shows, independent of any UI
//! toolkit.
//!
//! The egui app and the GPUI port both render from these types, so a rule
//! about what to list, filter, sort or queue has one implementation rather
//! than one per front end that can drift apart.

rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

/// Sets the language every `t!` in this crate reads.
///
/// `rust_i18n` keeps its locale per crate, so a front end setting its own
/// does not reach the shared strings; this is how a language change gets
/// here. The languages themselves are `wt_translations::SUPPORTED_LANGUAGES`.
pub fn set_locale(code: &str) {
    rust_i18n::set_locale(code);
}

/// The language this crate is currently translating into.
pub fn locale() -> String {
    rust_i18n::locale().to_string()
}

pub mod formatting;
pub mod glyphs;
pub mod listing_row;
pub mod match_stats;
pub mod personal_rating;
pub mod player_tracker;
pub mod preview_dwell;
pub mod query_bar;
pub mod replay_export;
pub mod search;
pub mod settings;
pub mod stats;
pub mod twitch;
pub mod unpacker;
