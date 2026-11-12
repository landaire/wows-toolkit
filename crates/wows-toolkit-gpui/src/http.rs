//! How this front end names itself to the services it calls.

use wows_toolkit_services::http::UserAgent;

/// Identifies this front end, not the egui app: the two are separate clients
/// of the same services and a log that cannot tell them apart is less useful.
pub const USER_AGENT: UserAgent = UserAgent::new(concat!("wows-toolkit-gpui/", env!("CARGO_PKG_VERSION")));
