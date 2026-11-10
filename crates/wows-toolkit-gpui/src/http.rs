//! This crate's HTTP client, identified as this app.

use wows_toolkit_viewmodel::http::HttpError;

/// Identifies this crate, not the egui app: the two are separate clients of
/// the same services and a log that cannot tell them apart is less useful.
pub const USER_AGENT: &str = concat!("wows-toolkit-gpui/", env!("CARGO_PKG_VERSION"));

pub fn client(proxy_url: &str, redirects: reqwest::redirect::Policy) -> Result<reqwest::Client, HttpError> {
    wows_toolkit_viewmodel::http::client(USER_AGENT, proxy_url, redirects)
}
