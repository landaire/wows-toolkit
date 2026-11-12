//! The Stats tab: session statistics, filtered and aggregated per ship.
//!
//! The aggregation lives in `wows_toolkit_viewmodel::stats`, shared with the
//! egui app, and `wows_toolkit_services::stats` reads it out of the database;
//! these modules render it.

pub mod chart_panel;
pub mod directory;
pub mod overview;
pub mod plot;
pub mod plot_image;
pub mod ships;
pub mod view;

pub(crate) fn normalize_ship_search(text: &str) -> String {
    unidecode::unidecode(text.trim()).to_lowercase()
}
