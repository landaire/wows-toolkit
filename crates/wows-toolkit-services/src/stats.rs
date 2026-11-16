//! Reading the session statistics out of the shared config database.
//!
//! The rows and the filter settings are the same ones the egui app writes, so
//! the two apps show the same session without a migration between them.

use sqlx::sqlite::SqlitePool;
use wows_toolkit_config::queries;
use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::GameLimit;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::setting_keys;

/// The saved session's games, oldest first, which is how the recency limit
/// expects them.
pub async fn load_saved_games(pool: &SqlitePool) -> Result<Vec<PerGameStat>, sqlx::Error> {
    let rows = queries::get_all_session_stats(pool).await?;

    Ok(rows.into_iter().map(PerGameStat::from_row).collect())
}

/// Reads the filter bar's saved state. Each miss falls back to that control's
/// own default rather than to a sentinel: an unset limit means no limit, and
/// an unset mode set means every mode.
pub async fn load_filters(pool: &SqlitePool) -> StatsFilters {
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

#[cfg(test)]
mod tests {
    use wows_toolkit_viewmodel::stats::session::SessionScope;

    use super::*;
    use crate::settings::StartupSettings;

    fn game(game_time: &str, ship: u64) -> PerGameStat {
        PerGameStat {
            ship_name: "Ship".to_string(),
            ship_id: ship.into(),
            game_time: game_time.to_string(),
            sort_key: wows_toolkit_viewmodel::stats::sortable_game_time(game_time),
            player_id: 7,
            damage: 1_000,
            spotting_damage: 0,
            frags: 1,
            raw_xp: 0,
            base_xp: 0,
            is_win: true,
            is_loss: false,
            is_draw: false,
            is_div: false,
            match_group: "pvp".to_string(),
            achievements: Vec::new(),
            match_contribution: None,
        }
    }

    #[tokio::test]
    async fn the_session_scope_is_saved_and_defaults_to_manual() {
        let pool = wows_toolkit_config::test_pool().await;
        assert_eq!(StartupSettings::load(&pool).await.session_scope, SessionScope::Manual);

        queries::set_setting(&pool, setting_keys::SESSION_SCOPE, &SessionScope::SinceAppOpened)
            .await
            .expect("the scope is written");

        assert_eq!(StartupSettings::load(&pool).await.session_scope, SessionScope::SinceAppOpened);
    }

    #[tokio::test]
    async fn the_saved_session_reads_back_oldest_first() {
        let pool = wows_toolkit_config::test_pool().await;
        for game in [game("02.11.2026 20:00:00", 2), game("28.10.2026 21:00:00", 1)] {
            queries::add_session_stat(&pool, &game.to_row()).await.expect("a session row is written");
        }

        let games = load_saved_games(&pool).await.expect("the session reads");
        let times: Vec<&str> = games.iter().map(|game| game.game_time.as_str()).collect();
        assert_eq!(times, ["28.10.2026 21:00:00", "02.11.2026 20:00:00"]);
    }
}
