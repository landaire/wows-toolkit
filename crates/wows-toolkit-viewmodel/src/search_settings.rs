//! The Search tab's saved state: the `search` settings row both desktop front ends and the egui
//! app keep the query bar's query, history, sort and operator choices in.
//!
//! Each field is written on its own, read-modify-write, so the fields another front end keeps in
//! the row survive.

use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortDirection;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::query_ast::OperatorPreferences;

/// The settings row the query bar's state is kept in.
pub const SEARCH_SETTINGS_KEY: &str = "search";

/// Queries the bar remembers. Older ones fall off the end rather than the row growing without
/// bound.
pub const HISTORY_DEPTH: usize = 50;

/// What the `search` row holds that the bar reads back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedSearch {
    pub query: String,
    /// Queries that were run, newest first.
    pub history: Vec<String>,
    pub sort: SortSpec,
    pub operator_preferences: OperatorPreferences,
}

/// Why the `search` row could not be read back.
#[derive(Debug, thiserror::Error)]
pub enum SavedSearchError {
    #[error("Search settings must be a JSON object")]
    NotAnObject,
    #[error("Search settings field 'query' must be a string")]
    Query,
    #[error("Search settings field 'history' is invalid: {source}")]
    History { source: serde_json::Error },
    #[error("Search settings field 'sort' is invalid")]
    Sort,
    #[error("Search settings field 'op_prefs' is invalid: {source}")]
    OperatorPreferences { source: serde_json::Error },
}

/// Reads the row back. A missing row, or a missing field, is the bar's empty state: nothing has
/// been saved yet. A field that is present but malformed is an error, so the bar does not write
/// over what it could not read.
pub fn parse_saved_search(stored: Option<serde_json::Value>) -> Result<SavedSearch, SavedSearchError> {
    let Some(stored) = stored else { return Ok(SavedSearch::default()) };
    let object = stored.as_object().ok_or(SavedSearchError::NotAnObject)?;
    let query = match object.get("query") {
        None => String::new(),
        Some(value) => value.as_str().map(str::to_owned).ok_or(SavedSearchError::Query)?,
    };
    let history = match object.get("history") {
        None => Vec::new(),
        Some(value) => serde_json::from_value(value.clone()).map_err(|source| SavedSearchError::History { source })?,
    };
    let sort = match object.get("sort") {
        None => SortSpec::default(),
        Some(value) => sort_from_value(value).ok_or(SavedSearchError::Sort)?,
    };
    let operator_preferences = match object.get("op_prefs") {
        None => OperatorPreferences::default(),
        Some(value) => {
            serde_json::from_value(value.clone()).map_err(|source| SavedSearchError::OperatorPreferences { source })?
        }
    };
    Ok(SavedSearch { query, history, sort, operator_preferences })
}

/// The sort as the row stores it: `["Column", "Direction"]`.
pub fn sort_from_value(value: &serde_json::Value) -> Option<SortSpec> {
    let pair = value.as_array()?;
    if pair.len() != 2 {
        return None;
    }
    let column = match pair.first()?.as_str()? {
        "Date" => SortColumn::Date,
        "Map" => SortColumn::Map,
        "Mode" => SortColumn::Mode,
        "Outcome" => SortColumn::Outcome,
        "Damage" => SortColumn::Damage,
        "Kills" => SortColumn::Kills,
        "Pr" => SortColumn::Pr,
        _ => return None,
    };
    let direction = match pair.get(1)?.as_str()? {
        "Ascending" => SortDirection::Ascending,
        "Descending" => SortDirection::Descending,
        _ => return None,
    };
    Some(SortSpec { column, direction })
}

pub fn sort_value(sort: SortSpec) -> serde_json::Value {
    let column = match sort.column {
        SortColumn::Date => "Date",
        SortColumn::Map => "Map",
        SortColumn::Mode => "Mode",
        SortColumn::Outcome => "Outcome",
        SortColumn::Damage => "Damage",
        SortColumn::Kills => "Kills",
        SortColumn::Pr => "Pr",
    };
    let direction = match sort.direction {
        SortDirection::Ascending => "Ascending",
        SortDirection::Descending => "Descending",
    };
    serde_json::json!([column, direction])
}

/// `history` with `query` recorded as the newest run. `false` when nothing changed: an empty query,
/// or the newest entry again.
pub fn remember_query(history: &mut Vec<String>, query: &str) -> bool {
    if query.is_empty() || history.first().map(String::as_str) == Some(query) {
        return false;
    }
    history.retain(|entry| entry != query);
    history.insert(0, query.to_owned());
    history.truncate(HISTORY_DEPTH);
    true
}

/// `stored` with `field` set to `value`, keeping every other field.
///
/// `None` when the row is not an object, which the caller reports rather than replacing.
pub fn with_field(
    stored: Option<serde_json::Value>,
    field: &str,
    value: serde_json::Value,
) -> Option<serde_json::Value> {
    let mut stored = stored.unwrap_or_else(|| serde_json::json!({}));
    stored.as_object_mut()?.insert(field.to_owned(), value);
    Some(stored)
}

#[cfg(test)]
mod tests {
    use super::SavedSearchError;
    use super::parse_saved_search;
    use super::remember_query;
    use super::sort_from_value;
    use super::sort_value;
    use super::with_field;
    use wows_toolkit_config::index::query::SortColumn;
    use wows_toolkit_config::index::query::SortDirection;
    use wows_toolkit_config::index::query::SortSpec;

    #[test]
    fn a_missing_row_is_the_empty_bar() {
        let saved = parse_saved_search(None).expect("no row is not an error");
        assert!(saved.query.is_empty() && saved.history.is_empty());
        assert_eq!(saved.sort, SortSpec::default());
    }

    #[test]
    fn the_row_reads_back_what_was_written() {
        let sort = SortSpec { column: SortColumn::Damage, direction: SortDirection::Ascending };
        let row = serde_json::json!({
            "query": "map:ocean",
            "history": ["map:ocean", "outcome=win"],
            "sort": sort_value(sort),
            "columns": ["kept for the egui app"],
        });
        let saved = parse_saved_search(Some(row)).expect("a well-formed row");
        assert_eq!(saved.query, "map:ocean");
        assert_eq!(saved.history, ["map:ocean", "outcome=win"]);
        assert_eq!(saved.sort, sort);
    }

    #[test]
    fn a_malformed_field_is_refused_rather_than_defaulted() {
        let error = parse_saved_search(Some(serde_json::json!({ "query": 3 }))).expect_err("a number is no query");
        assert!(matches!(error, SavedSearchError::Query));
        assert!(sort_from_value(&serde_json::json!(["Date"])).is_none());
        assert!(matches!(parse_saved_search(Some(serde_json::json!([]))), Err(SavedSearchError::NotAnObject)));
    }

    #[test]
    fn the_history_is_newest_first_without_repeats() {
        let mut history = vec!["b".to_owned(), "a".to_owned()];
        assert!(remember_query(&mut history, "a"));
        assert_eq!(history, ["a", "b"]);
        assert!(!remember_query(&mut history, "a"), "the newest entry again changes nothing");
        assert!(!remember_query(&mut history, ""), "an empty query is not remembered");
        for index in 0..super::HISTORY_DEPTH + 5 {
            remember_query(&mut history, &index.to_string());
        }
        assert_eq!(history.len(), super::HISTORY_DEPTH);
    }

    #[test]
    fn a_field_write_keeps_the_other_fields() {
        let row = with_field(Some(serde_json::json!({ "columns": [1] })), "query", serde_json::json!("x"))
            .expect("an object");
        assert_eq!(row, serde_json::json!({ "columns": [1], "query": "x" }));
        assert!(with_field(Some(serde_json::json!(3)), "query", serde_json::json!("x")).is_none());
    }
}
