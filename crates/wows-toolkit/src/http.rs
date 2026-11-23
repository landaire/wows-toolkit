//! How the app names itself to the services it calls.

use wows_toolkit_services::http::UserAgent;

/// Identifies the desktop app to the services it calls, apart from their other clients.
pub const USER_AGENT: UserAgent = UserAgent::new(concat!("wows-toolkit/", env!("CARGO_PKG_VERSION")));
