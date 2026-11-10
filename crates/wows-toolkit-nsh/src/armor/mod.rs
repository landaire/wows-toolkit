//! The Armor Viewer service: panes of 3D armor scenes rendered into frame
//! surfaces, the panels around them as one view object, and the commands the
//! TSX interface drives them with.
//!
//! The replay renderer reaches it through [`show_realtime`],
//! [`follow_realtime`] and [`set_seek_handler`].

mod catalog;
mod defaults;
mod error;
mod export;
mod frame;
mod input;
mod module;
mod pen;
mod render;
mod state;
#[cfg(feature = "test-support")]
pub mod test_support;
mod tree;
mod view;

pub use module::Armor;
pub use state::follow_realtime;
pub use state::set_seek_handler;
pub use state::show_realtime;
