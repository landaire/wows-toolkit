//! The Stats tab: session statistics, filtered and aggregated per ship.
//!
//! The aggregation lives in `wows_toolkit_viewmodel::stats`, shared with the
//! egui app; these modules read it out of the database and render it.

pub mod load;
pub mod overview;
pub mod view;
