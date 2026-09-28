//! The HTTP client this crate's two network calls share.
//!
//! Both want the same policy: connect and read timeouts rather than a total
//! request timeout, so a large download is not cut off while a stalled
//! connection still fails instead of hanging forever, and an explicit
//! redirect policy rather than reqwest's default.

use std::time::Duration;

use wows_toolkit_viewmodel::proxy;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Identifies this crate, not the egui app: the two are separate clients of
/// the same services and a log that cannot tell them apart is less useful.
const USER_AGENT: &str = concat!("wows-toolkit-gpui/", env!("CARGO_PKG_VERSION"));

/// Builds a client, honouring the proxy this machine is on.
///
/// `proxy_url` is the reader's own setting and wins; failing that the standard
/// environment variables and then the Windows configuration are read, which is
/// what makes the app work on a managed network nobody configured it for
/// (`wows_toolkit_viewmodel::proxy`, shared with the egui app). A proxy URL the
/// client rejects is reported rather than swallowed: sending direct because a
/// proxy setting was malformed is exactly what a reader who set one does not
/// want.
pub fn client(proxy_url: &str, redirects: reqwest::redirect::Policy) -> Result<reqwest::Client, HttpError> {
    let mut builder = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .redirect(redirects);

    let manual = (!proxy_url.trim().is_empty()).then_some(proxy_url);
    if let Some(config) = proxy::resolve_proxy(manual) {
        tracing::debug!(proxy = %config.redacted_url(), source = ?config.source, "routing through a proxy");
        let mut proxy = reqwest::Proxy::all(&config.url)
            .map_err(|source| HttpError::Proxy { url: config.redacted_url(), source })?;
        if !config.bypass.is_empty()
            && let Some(no_proxy) = reqwest::NoProxy::from_string(&config.bypass.join(","))
        {
            proxy = proxy.no_proxy(Some(no_proxy));
        }
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
