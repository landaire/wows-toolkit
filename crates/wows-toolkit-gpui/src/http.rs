//! The HTTP client this crate's network calls share, built by
//! `wows_toolkit_replay_data::http` under this front end's own user agent.

use wows_toolkit_replay_data::error::HttpError;

/// Identifies this crate, not the egui app: the two are separate clients of
/// the same services and a log that cannot tell them apart is less useful.
pub const USER_AGENT: &str = concat!("wows-toolkit-gpui/", env!("CARGO_PKG_VERSION"));

/// Builds a client, honouring the proxy this machine is on. `proxy_url` is the
/// reader's own setting; empty means the system's.
pub fn client(proxy_url: &str, redirects: reqwest::redirect::Policy) -> Result<reqwest::Client, HttpError> {
    wows_toolkit_replay_data::http::client(USER_AGENT, proxy_url, redirects)
}
