//! Keeping the personal-rating expected values current.
//!
//! The table is cached on disk under the toolkit's storage directory and
//! shared with the egui app, so whichever app refreshes it last is the one
//! both read. The policy -- where it lives, when it is stale, what counts as
//! a usable payload -- is `wows_toolkit_viewmodel::personal_rating`; this
//! module is only the fetch.

use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;

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
async fn fetch(proxy_url: &str) -> Result<Vec<u8>, RefreshError> {
    let client = crate::http::client(proxy_url, reqwest::redirect::Policy::default())?;

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
async fn fetch_and_cache(proxy_url: &str) -> Result<(), RefreshError> {
    let bytes = fetch(proxy_url).await?;
    personal_rating::save_cached(&bytes).map_err(RefreshError::Rejected)
}

/// The expected-values table, refreshed first when the cache is missing or
/// stale.
///
/// A failed refresh is logged rather than surfaced: a cache from last week
/// still rates every replay correctly enough to show, and the alternative is
/// an error banner over a feature the user did not ask to refresh.
pub async fn load_refreshed(proxy_url: &str) -> Option<PersonalRatingData> {
    if personal_rating::needs_update() {
        match fetch_and_cache(proxy_url).await {
            Ok(()) => tracing::info!("personal rating: refreshed the expected values"),
            Err(err) => tracing::warn!("personal rating: could not refresh the expected values: {err}"),
        }
    }

    match personal_rating::load_cached() {
        Ok(table) => Some(table),
        Err(err) => {
            tracing::info!("personal rating: no usable expected values: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fetch;
    use wows_toolkit_viewmodel::personal_rating;

    /// Hits the live wows-numbers endpoint. Ignored because it needs the
    /// network; it writes nothing, so it cannot disturb the real cache. Run
    /// with:
    ///
    /// ```text
    /// cargo test -p wows-toolkit-gpui -- --ignored --nocapture the_published_expected_values_are_a_usable_table
    /// ```
    #[test]
    #[ignore = "needs network access to api.wows-numbers.com"]
    fn the_published_expected_values_are_a_usable_table() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a tokio runtime");
        let bytes = runtime.block_on(fetch("")).expect("the endpoint answered");

        personal_rating::validate_expected_values(&bytes).expect("the published payload is a usable table");
        println!("fetched {} bytes of expected values", bytes.len());
    }
}
