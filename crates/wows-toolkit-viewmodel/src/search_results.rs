//! What the Search tab's results table and value lookups show.

use rust_i18n::t;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_config::index::rows::MatchOutcome;

use crate::formatting::separate_number;
use crate::query_bar::suggest::ValueOption;
use crate::query_bar::suggest::ValueRequest;

/// Results asked for per run. The egui table pages the same way rather than
/// pulling an unbounded set into memory.
pub const RESULT_LIMIT: i64 = 500;

/// Rows a value lookup offers, matching the egui bar's own limit.
pub const VALUE_LIMIT: i64 = 50;

/// Why a value lookup found nothing to offer.
#[derive(Debug, thiserror::Error)]
pub enum ValueLookupError {
    #[error("the {catalogue} lookup failed: {source}")]
    Index { catalogue: &'static str, source: wows_toolkit_config::index::rows::IndexError },
}

/// A battle result as the table reads it.
pub fn outcome_label(outcome: MatchOutcome) -> String {
    match outcome {
        MatchOutcome::Win => t!("ui.search.outcome_win").into_owned(),
        MatchOutcome::Loss => t!("ui.search.outcome_loss").into_owned(),
        MatchOutcome::Draw => t!("ui.search.outcome_draw").into_owned(),
        MatchOutcome::Unknown => "-".to_owned(),
    }
}

/// What a hit shows in `column`. An absent value reads as a dash rather than
/// a zero, which would sort and scan as a real result.
pub fn cell_text(hit: &MatchHit, column: SortColumn) -> String {
    match column {
        SortColumn::Date => hit.timestamp.strftime("%Y-%m-%d %H:%M").to_string(),
        SortColumn::Map => hit.map.clone(),
        SortColumn::Mode => hit.game_mode.clone(),
        SortColumn::Outcome => outcome_label(hit.outcome),
        // Grouped: a six-figure damage number is unreadable as a bare run of
        // digits, and every other surface groups it.
        SortColumn::Damage => hit.self_damage.map(|damage| separate_number(damage, None)).unwrap_or_else(|| "-".into()),
        SortColumn::Kills => hit.self_kills.map(|kills| kills.to_string()).unwrap_or_else(|| "-".into()),
        SortColumn::Pr => hit.self_pr.map(|pr| format!("{pr:.0}")).unwrap_or_else(|| "-".into()),
    }
}

/// The warning shown when `missing` indexed matches carry no game mode and
/// the query filters on one. Singular at exactly one, so it never reads
/// "1 indexed matches".
pub fn game_mode_gap_hint(missing: i64) -> String {
    if missing == 1 {
        t!("ui.search.game_mode_gap_hint_one").into_owned()
    } else {
        t!("ui.search.game_mode_gap_hint", count = missing).into_owned()
    }
}

/// The footer's count of a result page: `shown` matches, or the limit and a
/// plus when the index held more than the page.
pub fn match_count(shown: usize, truncated: bool) -> String {
    if truncated {
        t!("ui.search.match_count_truncated", count = RESULT_LIMIT).into_owned()
    } else {
        t!("ui.search.match_count", count = shown).into_owned()
    }
}

/// The values `request` asks the index for.
///
/// Ships and players are searched by what has been typed so far; maps and
/// sources are whole catalogues, which are small enough to offer entire.
pub async fn look_up_values(
    pool: &sqlx::SqlitePool,
    request: &ValueRequest,
) -> Result<Vec<ValueOption>, ValueLookupError> {
    let options = match request {
        ValueRequest::Players { needle } => query::search_players(pool, needle, VALUE_LIMIT)
            .await
            .map_err(|source| ValueLookupError::Index { catalogue: "player", source })?
            .iter()
            .map(|facet| ValueOption {
                label: if facet.clan.is_empty() {
                    facet.latest_name.clone()
                } else {
                    format!("[{}] {}", facet.clan, facet.latest_name)
                },
                token: facet.account_id.raw().to_string(),
            })
            .collect(),
        ValueRequest::Ships { needle } => query::search_ships(pool, needle, VALUE_LIMIT)
            .await
            .map_err(|source| ValueLookupError::Index { catalogue: "ship", source })?
            .iter()
            .map(|facet| ValueOption { label: facet.ship_name.clone(), token: facet.ship_id.raw().to_string() })
            .collect(),
        ValueRequest::Maps => query::distinct_maps(pool, VALUE_LIMIT)
            .await
            .map_err(|source| ValueLookupError::Index { catalogue: "map", source })?
            .iter()
            .map(|name| ValueOption { label: name.clone(), token: query_text::quote_if_needed(name) })
            .collect(),
        ValueRequest::Sources => query::list_sources(pool)
            .await
            .map_err(|source| ValueLookupError::Index { catalogue: "source", source })?
            .iter()
            .map(|source| ValueOption { label: source.name.clone(), token: source.id.0.to_string() })
            .collect(),
    };

    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gap_hint_is_singular_at_one() {
        assert_ne!(game_mode_gap_hint(1), game_mode_gap_hint(2));
        assert!(game_mode_gap_hint(12).contains("12"));
    }

    #[tokio::test]
    async fn an_empty_index_offers_no_values() {
        let pool = wows_toolkit_config::test_pool().await;

        let ships = look_up_values(&pool, &ValueRequest::Ships { needle: "yam".into() }).await.expect("ships read");
        let maps = look_up_values(&pool, &ValueRequest::Maps).await.expect("maps read");

        assert!(ships.is_empty());
        assert!(maps.is_empty());
    }
}
