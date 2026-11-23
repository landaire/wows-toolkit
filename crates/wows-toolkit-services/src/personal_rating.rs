//! Keeping the personal-rating expected values current.
//!
//! The table is cached on disk under the toolkit's storage directory. The
//! policy -- where it lives, when it is stale, what counts as
//! a usable payload -- is `wows_toolkit_viewmodel::personal_rating`; this
//! module is only the fetch, a future for the tokio runtime.

use std::time::Duration;
use std::time::Instant;

use tokio::sync::Mutex;
use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;

use crate::http::UserAgent;

/// When the last refresh failed, or `None` when it has not failed since the
/// process started or since the last refresh that succeeded.
///
/// Held across a refresh. A caller arriving meanwhile reads the cache as it
/// stands rather than waiting: while the endpoint stalls, waiting would hold
/// the caller's thread for the client's timeouts.
static LAST_FAILURE: Mutex<Option<Instant>> = Mutex::const_new(None);

#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error(transparent)]
    Client(#[from] crate::http::HttpError),
    #[error("the expected values could not be fetched")]
    Request(#[source] reqwest::Error),
    #[error("the expected values were rejected")]
    Rejected(#[source] personal_rating::ExpectedValuesError),
}

/// Fetches the published expected values.
async fn fetch(agent: UserAgent, proxy_url: &str) -> Result<Vec<u8>, RefreshError> {
    let client = crate::http::client(agent, proxy_url, reqwest::redirect::Policy::default())?;

    let response = client
        .get(personal_rating::EXPECTED_VALUES_URL)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(RefreshError::Request)?;

    response.bytes().await.map(|bytes| bytes.to_vec()).map_err(RefreshError::Request)
}

/// Fetches the expected values and replaces the cache with them.
///
/// The payload is validated before it is written: wows-numbers downtime has
/// served HTML error pages under a 200 status, and one of those must not
/// overwrite a good cache.
async fn fetch_and_cache(agent: UserAgent, proxy_url: &str) -> Result<(), RefreshError> {
    let bytes = fetch(agent, proxy_url).await?;
    personal_rating::save_cached(&bytes).map_err(RefreshError::Rejected)
}

/// The expected-values table, refreshed first when the cache is missing or
/// stale, no other refresh is under way, and none failed in the last ten
/// minutes.
///
/// A failed refresh is logged rather than surfaced: a cache from last week
/// still rates every replay correctly enough to show, and the alternative is
/// an error banner over a feature the user did not ask to refresh.
///
/// With the `test-support` feature nothing is downloaded: the table comes from
/// a third party, and a stalled answer holds a test for the client's whole
/// read timeout.
pub async fn load_refreshed(agent: UserAgent, proxy_url: &str) -> Option<PersonalRatingData> {
    if cfg!(not(feature = "test-support")) {
        refresh_if_stale(agent, proxy_url).await;
    }

    match personal_rating::load_cached() {
        Ok(table) => Some(table),
        Err(err) => {
            tracing::info!("personal rating: no usable expected values: {err}");
            None
        }
    }
}

/// Refreshes the cache when it is missing or stale, unless a refresh is under
/// way or one failed within the last `RETRY_AFTER`.
///
/// While wows-numbers stalls, each attempt waits out the client's timeouts,
/// and every indexed replay asks for the table.
async fn refresh_if_stale(agent: UserAgent, proxy_url: &str) {
    const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);

    let Ok(mut last_failure) = LAST_FAILURE.try_lock() else { return };
    if !personal_rating::needs_update() {
        return;
    }
    if last_failure.is_some_and(|failed| failed.elapsed() < RETRY_AFTER) {
        return;
    }

    match fetch_and_cache(agent, proxy_url).await {
        Ok(()) => {
            *last_failure = None;
            tracing::info!("personal rating: refreshed the expected values");
        }
        Err(err) => {
            *last_failure = Some(Instant::now());
            tracing::warn!("personal rating: could not refresh the expected values: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use wows_toolkit_viewmodel::personal_rating;

    use super::fetch;
    use crate::http::UserAgent;

    /// Hits the live wows-numbers endpoint. Ignored because it needs the
    /// network; it writes nothing, so it cannot disturb the real cache. Run
    /// with:
    ///
    /// ```text
    /// cargo test -p wows-toolkit-services -- --ignored --nocapture the_published_expected_values_are_a_usable_table
    /// ```
    #[test]
    #[ignore = "needs network access to api.wows-numbers.com"]
    fn the_published_expected_values_are_a_usable_table() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a tokio runtime");
        let agent = UserAgent::new(concat!("wows-toolkit-services/", env!("CARGO_PKG_VERSION")));
        let bytes = runtime.block_on(fetch(agent, "")).expect("the endpoint answered");

        personal_rating::validate_expected_values(&bytes).expect("the published payload is a usable table");
        println!("fetched {} bytes of expected values", bytes.len());
    }
}
