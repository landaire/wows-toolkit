//! The HTTP client this crate's two network calls share.
//!
//! Both want the same policy: connect and read timeouts rather than a total
//! request timeout, so a large download is not cut off while a stalled
//! connection still fails instead of hanging forever, and an explicit
//! redirect policy rather than reqwest's default.

use std::time::Duration;

use wows_toolkit_viewmodel::settings;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Identifies this crate, not the egui app: the two are separate clients of
/// the same services and a log that cannot tell them apart is less useful.
const USER_AGENT: &str = concat!("wows-toolkit-gpui/", env!("CARGO_PKG_VERSION"));

/// Builds a client, honouring `proxy_url` when it is set.
///
/// A proxy URL the client rejects is reported rather than swallowed: sending
/// direct because a proxy setting was malformed is exactly what a user who
/// set one does not want.
pub fn client(proxy_url: &str, redirects: reqwest::redirect::Policy) -> Result<reqwest::Client, HttpError> {
    let mut builder = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .redirect(redirects);

    if let Some(url) = settings::normalize_proxy_url(proxy_url) {
        let proxy = reqwest::Proxy::all(&url).map_err(|source| HttpError::Proxy { url, source })?;
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(HttpError::Build)
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("the proxy setting {url} is not a usable proxy URL")]
    Proxy {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("the HTTP client could not be built")]
    Build(#[source] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_proxy_builds_a_direct_client() {
        assert!(client("", reqwest::redirect::Policy::none()).is_ok());
    }

    #[test]
    fn a_scheme_less_proxy_is_accepted_the_way_the_windows_dialog_stores_it() {
        assert!(client("proxy.corp:8080", reqwest::redirect::Policy::none()).is_ok());
    }

    /// A proxy the client cannot use is an error rather than a silent direct
    /// connection.
    #[test]
    fn a_proxy_url_the_client_rejects_is_reported() {
        for raw in ["http://[::1", "http://user@:@host"] {
            if let Err(error) = client(raw, reqwest::redirect::Policy::none()) {
                assert!(matches!(error, HttpError::Proxy { .. }), "{raw} is refused as a proxy, not as a build");
                return;
            }
        }
        panic!("no candidate proxy URL was rejected; the refusal path is untested");
    }
}
