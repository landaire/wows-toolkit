//! The Unpacker tab: browse the game VFS, search it, and extract from it.
//!
//! What to list, filter, queue, extract and search lives in
//! `wows_toolkit_viewmodel::unpacker`, shared with the egui app. These modules
//! are the GPUI rendering over it.

pub mod browser;
pub mod queue_panel;
pub mod search_panel;
pub mod view;
pub mod viewer_panel;
