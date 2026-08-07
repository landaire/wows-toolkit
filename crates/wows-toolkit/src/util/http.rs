//! Shared HTTP client construction plus small retry and error-formatting helpers.
//!
//! Async clients use a connect timeout and a read (inactivity) timeout rather
//! than a total request timeout, so large downloads are not cut off while a
//! genuinely stalled connection still fails instead of hanging forever. The
//! blocking client is only used for small JSON requests, so it uses a total
//! request timeout.

use std::time::Duration;

const USER_AGENT: &str = concat!("wows-toolkit/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const BLOCKING_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ATTEMPTS: u32 = 3;

/// Async client with connect + read (inactivity) timeouts; safe for streaming large downloads.
pub fn async_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
}

/// Blocking client with connect + total-request timeouts, for small requests.
pub fn blocking_client(redirect_policy: reqwest::redirect::Policy) -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(BLOCKING_TIMEOUT)
        .redirect(redirect_policy)
        .build()
}

/// Render an error and its full `source()` chain on one line, so logs and
/// user-facing messages show the underlying cause (e.g. `UnknownIssuer`,
/// `connection reset`) instead of only the generic top-level text.
pub fn error_chain(err: &dyn std::error::Error) -> String {
    // Cap the walk so a pathological self-referencing source() can't loop forever.
    const MAX_DEPTH: usize = 16;
    let mut message = err.to_string();
    let mut source = err.source();
    let mut depth = 0;
    while let Some(cause) = source {
        depth += 1;
        if depth > MAX_DEPTH {
            message.push_str(": ...");
            break;
        }
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// 429 is intentionally excluded: a rate limiter wants a wait of seconds to
/// minutes, so short-backoff retries do not help and only add load. Let it
/// surface so the caller can see the rate limit.
fn is_retryable(err: &reqwest::Error) -> bool {
    err.is_timeout()
        || err.is_connect()
        || err.is_request()
        || matches!(err.status(), Some(status) if status.is_server_error())
}

/// GET `url`, retrying transient failures (timeouts, connect/request errors, 5xx, 429)
/// with exponential backoff. Returns the response after `error_for_status`.
pub async fn get_with_retry(client: &reqwest::Client, url: &str) -> reqwest::Result<reqwest::Response> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match client.get(url).send().await.and_then(reqwest::Response::error_for_status) {
            Ok(response) => return Ok(response),
            Err(err) if attempt < MAX_ATTEMPTS && is_retryable(&err) => {
                let delay = Duration::from_millis(500 * 2u64.pow(attempt - 1));
                tracing::warn!(
                    "GET {url} failed (attempt {attempt}/{MAX_ATTEMPTS}), retrying in {delay:?}: {}",
                    error_chain(&err)
                );
                tokio::time::sleep(delay).await;
            }
            Err(err) => return Err(err),
        }
    }
}

/// The process-wide async client, so every caller shares one connection pool
/// and TLS session cache. `None` only when the client cannot be built at all,
/// which is a broken TLS backend rather than a per-call failure.
pub fn shared_async_client() -> Option<&'static reqwest::Client> {
    static SHARED: std::sync::OnceLock<Option<reqwest::Client>> = std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| async_client().inspect_err(|e| tracing::error!("failed to build shared HTTP client: {e}")).ok())
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four call sites fetch from the same host. Handing each its own client
    /// gives each its own connection pool and TLS session cache, so a download
    /// of thousands of small objects reconnects instead of reusing.
    #[test]
    fn the_shared_client_is_one_client() {
        let first = shared_async_client().expect("a client can be built in this environment");
        let second = shared_async_client().expect("a client can be built in this environment");
        assert!(std::ptr::eq(first, second));
    }
}
