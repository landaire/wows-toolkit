//! The Search tab's saved state: the settings row both front ends keep the
//! query bar in, and the dragged column widths beside it.

use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortDirection;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::query_ast::OperatorPreferences;

/// The settings row the query bar's state is kept in. The egui tab declares
/// the same row, so every front end reopens on the same query.
pub const SEARCH_SETTINGS_KEY: &str = "search";

/// The settings row the dragged column widths are kept in, one optional
/// width per drawn column.
pub const COLUMN_WIDTHS_KEY: &str = "search_column_widths";

/// Queries the bar remembers. Older ones fall off the end rather than the
/// row growing without bound.
pub const HISTORY_DEPTH: usize = 50;

/// One field of the saved row. Each is written on its own, leaving every
/// other field the egui tab stores beside it untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchField {
    Query,
    History,
    Sort,
    OperatorPreferences,
}

impl SearchField {
    /// The field's key in the saved object.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::History => "history",
            Self::Sort => "sort",
            Self::OperatorPreferences => "op_prefs",
        }
    }
}

/// What the saved row holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SavedSearch {
    pub query: String,
    /// Queries that were run, newest first.
    pub history: Vec<String>,
    pub sort: SortSpec,
    pub operator_preferences: OperatorPreferences,
}

/// Why the saved row could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum SavedSearchError {
    #[error("Search settings must be a JSON object")]
    NotAnObject,
    #[error("Search settings field '{}' must be a string", SearchField::Query.key())]
    QueryNotText,
    #[error("Search settings field '{}' is invalid: {source}", SearchField::History.key())]
    History { source: serde_json::Error },
    #[error("Search settings field '{}' is invalid", SearchField::Sort.key())]
    Sort,
    #[error("Search settings field '{}' is invalid: {source}", SearchField::OperatorPreferences.key())]
    OperatorPreferences { source: serde_json::Error },
}

impl SavedSearch {
    /// Reads the saved row. A missing row is a fresh tab: no query, no
    /// history, newest first.
    pub fn parse(stored: Option<serde_json::Value>) -> Result<Self, SavedSearchError> {
        let Some(stored) = stored else {
            return Ok(Self::default());
        };
        let object = stored.as_object().ok_or(SavedSearchError::NotAnObject)?;

        let query = match object.get(SearchField::Query.key()) {
            None => String::new(),
            Some(value) => value.as_str().map(str::to_owned).ok_or(SavedSearchError::QueryNotText)?,
        };
        let history = match object.get(SearchField::History.key()) {
            None => Vec::new(),
            Some(value) => {
                serde_json::from_value(value.clone()).map_err(|source| SavedSearchError::History { source })?
            }
        };
        let sort = match object.get(SearchField::Sort.key()) {
            None => SortSpec::default(),
            Some(value) => sort_from_value(value).ok_or(SavedSearchError::Sort)?,
        };
        let operator_preferences = match object.get(SearchField::OperatorPreferences.key()) {
            None => OperatorPreferences::default(),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|source| SavedSearchError::OperatorPreferences { source })?,
        };

        Ok(Self { query, history, sort, operator_preferences })
    }
}

/// `stored` with `field` set to `value`, keeping every other field.
pub fn with_field(
    stored: Option<serde_json::Value>,
    field: SearchField,
    value: serde_json::Value,
) -> Result<serde_json::Value, SavedSearchError> {
    let mut stored = stored.unwrap_or_else(|| serde_json::json!({}));
    let object = stored.as_object_mut().ok_or(SavedSearchError::NotAnObject)?;
    object.insert(field.key().to_owned(), value);

    Ok(stored)
}

/// A sort as the saved row spells it: `["Damage", "Descending"]`.
pub fn sort_value(sort: SortSpec) -> serde_json::Value {
    serde_json::json!([sort_column_name(sort.column), sort_direction_name(sort.direction)])
}

/// A sort as the saved row spells it, read back.
pub fn sort_from_value(value: &serde_json::Value) -> Option<SortSpec> {
    let [column, direction] = value.as_array()?.as_slice() else { return None };
    let column = sort_column_from_name(column.as_str()?)?;
    let direction = sort_direction_from_name(direction.as_str()?)?;

    Some(SortSpec { column, direction })
}

/// The saved row's name for `column`.
pub const fn sort_column_name(column: SortColumn) -> &'static str {
    match column {
        SortColumn::Date => "Date",
        SortColumn::Map => "Map",
        SortColumn::Mode => "Mode",
        SortColumn::Outcome => "Outcome",
        SortColumn::Damage => "Damage",
        SortColumn::Kills => "Kills",
        SortColumn::Pr => "Pr",
    }
}

pub fn sort_column_from_name(name: &str) -> Option<SortColumn> {
    SortColumn::ALL.into_iter().find(|column| sort_column_name(*column) == name)
}

pub const fn sort_direction_name(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Ascending => "Ascending",
        SortDirection::Descending => "Descending",
    }
}

pub fn sort_direction_from_name(name: &str) -> Option<SortDirection> {
    match name {
        "Ascending" => Some(SortDirection::Ascending),
        "Descending" => Some(SortDirection::Descending),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_row_opens_a_fresh_tab() {
        assert_eq!(SavedSearch::parse(None).expect("nothing saved is valid"), SavedSearch::default());
    }

    #[test]
    fn the_saved_row_reads_back_what_was_written() {
        let sort = SortSpec { column: SortColumn::Damage, direction: SortDirection::Ascending };
        let stored = with_field(None, SearchField::Query, serde_json::json!("outcome:win")).expect("an object");
        let stored = with_field(Some(stored), SearchField::History, serde_json::json!(["a", "b"])).expect("an object");
        let stored = with_field(Some(stored), SearchField::Sort, sort_value(sort)).expect("an object");
        let stored =
            with_field(Some(stored), SearchField::OperatorPreferences, serde_json::json!({})).expect("an object");

        let saved = SavedSearch::parse(Some(stored)).expect("the row reads back");

        assert_eq!(saved.query, "outcome:win");
        assert_eq!(saved.history, ["a", "b"]);
        assert_eq!(saved.sort, sort);
    }

    #[test]
    fn every_sort_round_trips() {
        for column in SortColumn::ALL {
            for direction in [SortDirection::Ascending, SortDirection::Descending] {
                let sort = SortSpec { column, direction };
                assert_eq!(sort_from_value(&sort_value(sort)), Some(sort));
            }
        }
    }

    #[test]
    fn a_malformed_row_names_what_is_wrong() {
        assert!(matches!(SavedSearch::parse(Some(serde_json::json!([]))), Err(SavedSearchError::NotAnObject)));
        assert!(matches!(
            SavedSearch::parse(Some(serde_json::json!({ "query": 3 }))),
            Err(SavedSearchError::QueryNotText)
        ));
        assert!(matches!(
            SavedSearch::parse(Some(serde_json::json!({ "sort": ["Ship", "Ascending"] }))),
            Err(SavedSearchError::Sort)
        ));
        assert!(matches!(
            SavedSearch::parse(Some(serde_json::json!({ "history": "x" }))),
            Err(SavedSearchError::History { .. })
        ));
    }
}
