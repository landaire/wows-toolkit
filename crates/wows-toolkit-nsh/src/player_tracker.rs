use jiff::ToSpan as _;
use nsh_core::prelude::*;

use crate::native::database_runtime;

/// Historical players found in the shared replay index.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PlayerTrackerState {
    pub players: Vec<TrackedPlayer>,
    pub total_players: u32,
}

/// One player shown by the historical player tracker.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct TrackedPlayer {
    pub account_id: String,
    pub name: String,
    pub clan: String,
    pub encounters: u32,
}

/// Player tracker services exposed to the TSX interface.
#[module]
pub struct PlayerTracker;

#[module]
impl PlayerTracker {
    /// Reads historical encounters from the shared replay index.
    pub async fn load_player_tracker(period: String, query: String) -> Result<PlayerTrackerState, String> {
        blocking(move || load_player_tracker(&period, &query))
            .await
            .map_err(|error| format!("player tracker reader task failed: {error}"))?
    }
}

fn load_player_tracker(period: &str, query: &str) -> Result<PlayerTrackerState, String> {
    let now = jiff::Timestamp::now();
    let date_from = match period {
        "hour" => Some(now - 1.hour()),
        "six_hours" => Some(now - 6.hours()),
        "day" => Some(now - 24.hours()),
        "week" => Some(now - (24 * 7).hours()),
        "month" => Some(now - (24 * 30).hours()),
        "all" => None,
        _ => return Err(format!("unsupported player tracker period: {period}")),
    };
    database_runtime()?.block_on(async {
        let pool = wows_toolkit_config::open_db()
            .await
            .map_err(|error| format!("could not open the replay index: {error}"))?;
        let filter = wows_toolkit_config::index::rows::MatchFilter { date_from, ..Default::default() };
        let limit = std::num::NonZeroU32::new(500).expect("the player tracker page limit is nonzero");
        let page = wows_toolkit_config::index::query::distinct_players_page(&pool, &filter, query, limit)
            .await
            .map_err(|error| format!("could not read indexed players: {error}"))?;
        let total_players = u32::try_from(page.total_count)
            .map_err(|_| "the filtered player count exceeds the supported range".to_owned())?;
        let players = page
            .players
            .into_iter()
            .map(|player| {
                let encounters = u32::try_from(player.match_count).map_err(|_| {
                    format!("encounter count for account {} exceeds the supported range", player.account_id.raw())
                })?;
                Ok(TrackedPlayer {
                    account_id: player.account_id.raw().to_string(),
                    name: player.latest_name,
                    clan: player.clan,
                    encounters,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(PlayerTrackerState { players, total_players })
    })
}
