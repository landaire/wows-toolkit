//! The Search tab's results: the columns, what each cell reads, and the index lookups behind the
//! query bar's value completions.

use rust_i18n::t;
use wows_core::pseudonym::NameDisplay;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_config::index::rows::MatchOutcome;

use crate::formatting::separate_number;
use crate::name_lookup::PlayerLookup;
use crate::name_lookup::ShipLookup;
use crate::query_bar::label::PillIds;
use crate::query_bar::label::PillNames;
use crate::query_bar::suggest::ValueOption;
use crate::rating_breakdown::MatchContributionWithheld;
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
            Self::Sortable(SortColumn::Mc) => 80.,
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
        SortColumn::Pr if !hit.rating_scope().is_rated() => "-".into(),
        SortColumn::Pr => hit.self_pr.map(|pr| format!("{pr:.0}")).unwrap_or_else(|| "-".into()),
        SortColumn::Mc if !hit.rating_scope().is_rated() => "-".into(),
        SortColumn::Mc => hit.self_mc.map(|mc| format!("{mc:.0}")).unwrap_or_else(|| "-".into()),
    }
}

/// The hover a hit's cell carries, where the cell needs explaining.
pub fn cell_hover(hit: &MatchHit, column: SortColumn) -> Option<String> {
    match column {
        SortColumn::Pr if !hit.rating_scope().is_rated() => Some(t!("ui.personal_rating.unrated_hover").into_owned()),
        SortColumn::Mc if !hit.rating_scope().is_rated() => {
            Some(MatchContributionWithheld::UnratedBattle.explanation())
        }
        _ => None,
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
    /// Players found by their real names are labelled as `display` shows them.
    pub fn with_display(self, display: NameDisplay) -> Self {
        match self {
            Self::Players(players) => Self::Players(players.with_display(display)),
            Self::Ships(ships) => Self::Ships(ships),
        }
    }

    /// The values whose names `needle` matches, best first, ranked over every name before the
    /// dropdown's cap.
    pub fn options(&self, needle: &str) -> Vec<ValueOption> {
        let query = FilterQuery::new(needle);
        match self {
            Self::Players(players) => {
                let display = players.display();
                players
                    .find(&query, VALUE_LIMIT)
                    .into_iter()
                    .map(|facet| {
                        let name = display.player_name(&facet.latest_name);
                        let label = if facet.clan.is_empty() {
                            name.into_owned()
                        } else {
                            format!("[{}] {name}", display.clan_tag(&facet.clan))
                        };
                        ValueOption { label, token: facet.account_id.raw().to_string() }
                    })
                    .collect()
            }
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

/// The names the index holds for `ids`. A failed lookup is logged and its pill keeps reading
/// `#<id>`: the query itself still runs.
pub async fn look_up_pill_names(pool: &sqlx::SqlitePool, ids: &PillIds) -> PillNames {
    let mut found = PillNames::default();
    for &ship in &ids.ships {
        match query::ship_name(pool, ship).await {
            Ok(Some(name)) => found.ships.push((ship, name)),
            Ok(None) => {}
            Err(err) => tracing::warn!("search: the ship name lookup failed for {ship:?}: {err}"),
        }
    }

    for &account in &ids.players {
        match query::player_name(pool, account).await {
            Ok(Some(name)) => found.players.push((account, name)),
            Ok(None) => {}
            Err(err) => tracing::warn!("search: the player name lookup failed for {account:?}: {err}"),
        }
    }

    if !ids.sources.is_empty() {
        match query::list_sources(pool).await {
            Ok(sources) => found.sources = Some(sources),
            Err(err) => tracing::warn!("search: the source lookup failed: {err}"),
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use rust_i18n::t;
    use wows_toolkit_config::index::rows::MatchHit;
    use wows_toolkit_config::index::rows::MatchOutcome;

    use super::ResultColumn;
    use super::cell_hover;
    use super::cell_text;
    use super::game_mode_gap_hint;
    use crate::rating_breakdown::MatchContributionWithheld;
    use wows_toolkit_config::index::query::SortColumn;

    #[test]
    fn ship_sits_after_mode_and_every_sort_column_is_drawn() {
        let columns = ResultColumn::all();
        let mode = columns.iter().position(|column| *column == ResultColumn::Sortable(SortColumn::Mode));
        assert_eq!(columns.get(mode.expect("mode is drawn") + 1), Some(&ResultColumn::Ship));
        assert_eq!(columns.len(), SortColumn::ALL.len() + 1);
    }

    fn hit(game_type: &str, match_group: &str) -> MatchHit {
        MatchHit {
            arena_id: wows_replays::types::ArenaId::new(1),
            timestamp: "2026-07-28T14:23:05Z".parse().expect("a fixed instant"),
            map: "Ocean".into(),
            game_mode: "Domination".into(),
            game_mode_id: Some(7),
            game_type: game_type.into(),
            match_group: match_group.into(),
            version_build: Some(12_830_008),
            source_id: wows_toolkit_config::index::rows::SourceId(1),
            outcome: MatchOutcome::Win,
            self_account_id: None,
            self_ship_id: None,
            self_ship_name: Some("Yamato".into()),
            self_survived: Some(true),
            self_damage: Some(114_230),
            self_kills: Some(3),
            self_pr: Some(1500.0),
            self_mc: Some(1300.0),
            results_available: true,
            replay_path: std::path::PathBuf::from("a.wowsreplay"),
            file_mtime: Some(42),
        }
    }

    /// An unrated battle's rating cells are dashes that say why, whatever the
    /// index still holds; a rated battle's show the figures and need no hover.
    #[test]
    fn unrated_rating_cells_are_explained_dashes() {
        let unrated = hit("CooperativeBattle", "cooperative");
        for column in [SortColumn::Pr, SortColumn::Mc] {
            assert_eq!(cell_text(&unrated, column), "-", "{column:?}");
        }
        assert_eq!(cell_hover(&unrated, SortColumn::Pr), Some(t!("ui.personal_rating.unrated_hover").into_owned()));
        assert_eq!(cell_hover(&unrated, SortColumn::Mc), Some(MatchContributionWithheld::UnratedBattle.explanation()));

        let rated = hit("RandomBattle", "pvp");
        assert_eq!(cell_text(&rated, SortColumn::Pr), "1500");
        assert_eq!(cell_text(&rated, SortColumn::Mc), "1300");
        for column in [SortColumn::Pr, SortColumn::Mc] {
            assert_eq!(cell_hover(&rated, column), None, "{column:?}");
        }
    }

    #[test]
    fn the_gap_hint_is_singular_at_one() {
        assert!(game_mode_gap_hint(1).starts_with("1 indexed match has"));
        assert!(game_mode_gap_hint(2).starts_with("2 indexed matches have"));
    }
}
