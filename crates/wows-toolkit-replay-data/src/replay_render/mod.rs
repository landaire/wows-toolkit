//! Playing a replay's battle back on the minimap at whatever speed the reader
//! asks for, drawing over it, and encoding it to video, as plain state and
//! blocking functions a front end drives with its own widgets and executor.
//!
//! The bake keeps the draw commands of a bounded number of frames rather than
//! images, and one frame is rasterised at a time through
//! [`crate::minimap_preview::SharedPreviewRenderer`]. Which commands are
//! drawn is decided as a frame is rasterised, so a toggle costs one frame
//! rather than another walk of the battle.

pub mod annotate;
pub mod armor;
pub mod collab;
pub mod export;
pub mod hover;
pub mod options;
pub mod playback;
pub mod raster;
pub mod ships;
pub mod timeline;
pub mod track;
pub mod view;

#[cfg(test)]
mod tests;
