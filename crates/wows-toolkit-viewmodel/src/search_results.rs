//! The Search tab's results: the columns, what each cell reads, and the index lookups behind the
//! query bar's value completions.

use rust_i18n::t;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_config::index::rows::MatchOutcome;

use crate::formatting::separate_number;
use crate::name_lookup::PlayerLookup;
use crate::name_lookup::ShipLookup;
use crate::query_bar::suggest::ValueOption;
use crate::text_filter::FilterQuery;

/// Results asked for per run, rather than pulling an unbounded set into memory.
pub const RESULT_LIMIT: i64 = 500;

/// Rows a value lookup offers.
pub const VALUE_LIMIT: usize = 50;

/// A column the results table draws.
///
/// The sortable ones are the shared `SortColumn`; Ship is drawn beside them and carries no sort,
/// since the index has nothing to order it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultColumn {
    Sortable(SortColumn),
    Ship,
}

impl ResultColumn {
    /// The order the table lists them in: Ship sits after Mode.
    pub fn all() -> Vec<ResultColumn> {
        let mut columns = Vec::with_capacity(SortColumn::ALL.len() + 1);
        for column in SortColumn::ALL {
            columns.push(ResultColumn::Sortable(column));
            if column == SortColumn::Mode {
                columns.push(ResultColumn::Ship);
            }
        }
        columns
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Sortable(column) => column.label(),
            Self::Ship => "Ship",
        }
    }

    /// What the column is drawn at until the reader drags it, in logical pixels.
    pub fn default_width(self) -> f32 {
        match self {
            Self::Sortable(SortColumn::Date) => 160.,
            Self::Sortable(SortColumn::Map) => 150.,
            Self::Sortable(SortColumn::Mode) => 130.,
            Self::Sortable(SortColumn::Outcome) => 90.,
            Self::Sortable(SortColumn::Damage) => 100.,
            Self::Sortable(SortColumn::Kills) => 70.,
            Self::Sortable(SortColumn::Pr) => 80.,
            Self::Ship => 150.,
        }
    }
}

pub fn outcome_label(outcome: MatchOutcome) -> String {
    match outcome {
        MatchOutcome::Win => t!("ui.search.outcome_win").into_owned(),
        MatchOutcome::Loss => t!("ui.search.outcome_loss").into_owned(),
        MatchOutcome::Draw => t!("ui.search.outcome_draw").into_owned(),
        MatchOutcome::Unknown => "-".to_string(),
    }
}

/// What a hit shows in `column`. An absent value reads as a dash rather than a zero, which would
/// sort and scan as a real result.
pub fn cell_text(hit: &MatchHit, column: SortColumn) -> String {
    match column {
        SortColumn::Date => hit.timestamp.strftime("%Y-%m-%d %H:%M").to_string(),
        SortColumn::Map => hit.map.clone(),
        SortColumn::Mode => hit.game_mode.clone(),
        SortColumn::Outcome => outcome_label(hit.outcome),
        // Grouped: a six-figure damage number is unreadable as a bare run of digits.
        SortColumn::Damage => hit.self_damage.map(|d| separate_number(d, None)).unwrap_or_else(|| "-".into()),
        SortColumn::Kills => hit.self_kills.map(|k| k.to_string()).unwrap_or_else(|| "-".into()),
        SortColumn::Pr => hit.self_pr.map(|pr| format!("{pr:.0}")).unwrap_or_else(|| "-".into()),
    }
}

/// The hint a game-mode query shows over an index with `missing` matches that carry no mode.
/// Singular at exactly one.
pub fn game_mode_gap_hint(missing: i64) -> String {
    if missing == 1 {
        "1 indexed match has no recorded game mode and cannot match this query. Re-index to fill it in.".to_string()
    } else {
        format!(
            "{missing} indexed matches have no recorded game mode and cannot match this query. Re-index to fill them in."
        )
    }
}

/// The names a player or ship field ranks as its value is typed.
pub enum ValueNames {
    Players(PlayerLookup),
    Ships(ShipLookup),
}

/// Which names a value field ranks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameList {
    Players,
    Ships,
}

impl NameList {
    pub async fn load(self, pool: &sqlx::SqlitePool) -> Result<ValueNames, IndexError> {
        match self {
            Self::Players => Ok(ValueNames::Players(PlayerLookup::load(pool).await?)),
            Self::Ships => Ok(ValueNames::Ships(ShipLookup::load_roster(pool).await?)),
        }
    }
}

impl ValueNames {
    /// The values whose names `needle` matches, best first, ranked over every name before the
    /// dropdown's cap.
    pub fn options(&self, needle: &str) -> Vec<ValueOption> {
        let query = FilterQuery::new(needle);
        match self {
            Self::Players(players) => players
                .find(&query, VALUE_LIMIT)
                .into_iter()
                .map(|facet| ValueOption {
                    label: if facet.clan.is_empty() {
                        facet.latest_name.clone()
                    } else {
                        format!("[{}] {}", facet.clan, facet.latest_name)
                    },
                    token: facet.account_id.raw().to_string(),
                })
                .collect(),
            Self::Ships(ships) => ships
                .find(&query, VALUE_LIMIT)
                .into_iter()
                .map(|facet| ValueOption { label: facet.ship_name.clone(), token: facet.ship_id.raw().to_string() })
                .collect(),
        }
    }
}

/// The maps the index has seen, offered entire.
pub async fn look_up_maps(pool: &sqlx::SqlitePool) -> Vec<ValueOption> {
    match query::distinct_maps(pool, VALUE_LIMIT as i64).await {
        Ok(names) => names
            .iter()
            .map(|name| ValueOption { label: name.clone(), token: query_text::quote_if_needed(name) })
            .collect(),
        Err(err) => {
            tracing::warn!("search: the map lookup failed: {err}");
            Vec::new()
        }
    }
}

/// The index sources, offered entire.
pub async fn look_up_sources(pool: &sqlx::SqlitePool) -> Vec<ValueOption> {
    match query::list_sources(pool).await {
        Ok(sources) => sources
            .iter()
            .map(|source| ValueOption { label: source.name.clone(), token: source.id.0.to_string() })
            .collect(),
        Err(err) => {
            tracing::warn!("search: the source lookup failed: {err}");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ResultColumn;
    use super::game_mode_gap_hint;
    use wows_toolkit_config::index::query::SortColumn;

    #[test]
    fn ship_sits_after_mode_and_every_sort_column_is_drawn() {
        let columns = ResultColumn::all();
        let mode = columns.iter().position(|column| *column == ResultColumn::Sortable(SortColumn::Mode));
        assert_eq!(columns.get(mode.expect("mode is drawn") + 1), Some(&ResultColumn::Ship));
        assert_eq!(columns.len(), SortColumn::ALL.len() + 1);
    }

    #[test]
    fn the_gap_hint_is_singular_at_one() {
        assert!(game_mode_gap_hint(1).starts_with("1 indexed match has"));
        assert!(game_mode_gap_hint(2).starts_with("2 indexed matches have"));
    }
}
