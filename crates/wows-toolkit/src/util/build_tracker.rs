//! The per-player build payload `/api/ship_builds` takes.
//!
//! Moved to `wows_toolkit_viewmodel::upload::build_tracker` so the GPUI port
//! sends the same shape. This is the path the rest of this crate uses.

pub(crate) use wows_toolkit_viewmodel::upload::build_tracker::BuildTrackerPayload;
