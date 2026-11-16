//! The Stats tab: session statistics, filtered and aggregated per ship.
//!
//! The aggregation lives in `wows_toolkit_viewmodel::stats`, shared with the
//! egui app; these modules read it out of the database and render it.

pub mod chart_panel;
pub mod load;
pub mod overview;
pub mod plot;
pub mod plot_image;
pub mod ships;
pub mod view;

pub(crate) fn normalize_ship_search(text: &str) -> String {
    unidecode::unidecode(text.trim()).to_lowercase()
}
