//! Reading the Stats tab's data out of the shared config database.
//!
//! The rows and the filter settings are the same ones the egui app writes, so
//! the two apps show the same session without a migration between them.

use std::sync::Arc;

use sqlx::sqlite::SqlitePool;
use wows_toolkit_config::queries;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::GameLimit;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::setting_keys;

/// Everything the Stats tab needs at startup.
#[derive(Default)]
pub struct SessionData {
    /// In `sort_key` order, oldest first, which is how the recency limit
    /// expects them.
    pub games: Vec<PerGameStat>,
    pub filters: StatsFilters,
    /// The expected-values table personal rating is computed against.
    /// Refreshed on the shared disk cache when it is missing or stale (see
    /// `crate::personal_rating`), so the two apps keep one copy between them.
    /// Absent when the fetch fails and there is no cache to fall back on.
    pub personal_rating: Option<Arc<PersonalRatingData>>,
}

impl SessionData {
    /// `proxy_url` is the saved proxy setting, empty for a direct
    /// connection; the expected-values refresh is the one network call here.
    pub async fn load(pool: &SqlitePool, proxy_url: &str) -> Self {
        let games = match queries::get_all_session_stats(pool).await {
            Ok(rows) => rows.into_iter().map(PerGameStat::from_row).collect(),
            Err(err) => {
                tracing::warn!("stats: could not read the session rows: {err}");
                Vec::new()
            }
        };

        let personal_rating = crate::personal_rating::load_refreshed(proxy_url).await.map(Arc::new);

        Self { games, filters: load_filters(pool).await, personal_rating }
    }
}

/// Reads the filter bar's saved state. Each miss falls back to that control's
/// own default rather than to a sentinel: an unset limit means no limit, and
/// an unset mode set means every mode.
async fn load_filters(pool: &SqlitePool) -> StatsFilters {
    let limit_enabled = queries::get_setting::<bool>(pool, setting_keys::LIMIT_ENABLED).await.unwrap_or(false);
    let game_count = queries::get_setting::<usize>(pool, setting_keys::GAME_COUNT).await;
    let limit = match (limit_enabled, game_count) {
        (true, Some(count)) if count > 0 => GameLimit::Recent(count),
        // A saved count of zero would show nothing at all; the egui slider
        // cannot produce one, so treat it as the unlimited it means.
        _ => GameLimit::All,
    };

    StatsFilters {
        limit,
        division: queries::get_setting::<DivisionFilter>(pool, setting_keys::DIVISION_FILTER).await.unwrap_or_default(),
        game_modes: queries::get_setting(pool, setting_keys::GAME_MODE_FILTER).await.unwrap_or_default(),
    }
}
