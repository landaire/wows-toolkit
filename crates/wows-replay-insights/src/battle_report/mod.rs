//! Egui-free normalized battle report and the raw->named results resolution
//! that feeds it.

mod build;
mod resolve;
mod results;

pub use build::*;
pub use resolve::resolve_battle_results;
pub use results::*;
