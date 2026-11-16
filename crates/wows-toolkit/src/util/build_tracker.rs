//! The per-player build payload `/api/ship_builds` takes.
//!
//! Defined in `wows_toolkit_viewmodel::upload::build_tracker` so both front ends
//! send the same shape. This is the path the rest of this crate uses.

pub(crate) use wows_toolkit_viewmodel::upload::build_tracker::BuildTrackerPayload;
