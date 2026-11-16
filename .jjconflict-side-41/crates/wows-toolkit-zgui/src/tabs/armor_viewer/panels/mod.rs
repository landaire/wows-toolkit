//! The armor viewer's panels beside and over the hull, ported from the GPUI viewer:
//!
//! - `pen`: the penetration checker, which reads the ships being compared against the plate under
//!   the pointer, and the incoming fire log of a ship opened from a replay.
//! - `trajectory`: what the next cast is fired with, a marker per cast arc and the editor of the
//!   arc a marker selects.
//! - `export`: the GLB export dialog, and the export it confirms, run off the UI thread.
//! - `legend`: the armor thickness legend.
//!
//! The tab owns one `PenetrationState` per pane and hands the panels the viewport's state.

pub mod export;
pub mod incoming;
pub mod legend;
pub mod pen;
#[cfg(test)]
mod tests;
pub mod trajectory;
