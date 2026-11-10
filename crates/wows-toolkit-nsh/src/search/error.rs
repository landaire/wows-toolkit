use wows_toolkit_config::index::query_text::ParseErrorKind;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::queries::SettingError;
use wows_toolkit_viewmodel::palette::PaletteError;
use wows_toolkit_viewmodel::search_results::ValueLookupError;
use wows_toolkit_viewmodel::search_settings::SavedSearchError;

use crate::runtime::RuntimeError;

/// Why a search or a saved-state read or write failed.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("{source}")]
    Runtime { source: RuntimeError },
    #[error("could not copy the replay to the clipboard: {source}")]
    Clipboard { source: arboard::Error },
    /// The file manager request failed; nsh reports why only as text.
    #[error("{reason}")]
    Reveal { reason: String },
    #[error("the {task} task did not complete: {source}")]
    Task { task: &'static str, source: tokio::task::JoinError },
    #[error("the replay index is not open: {source}")]
    Open { source: sqlx::Error },
    #[error("the query did not parse: {kind}")]
    Parse { kind: ParseErrorKind },
    #[error("{source}")]
    Search { source: IndexError },
    #[error("{source}")]
    Lookup { source: ValueLookupError },
    #[error("Could not read Search settings: {source}")]
    ReadSettings { source: SettingError },
    #[error("{source}")]
    Saved { source: SavedSearchError },
    #[error("Search {field} could not be saved: {source}")]
    WriteSettings { field: &'static str, source: sqlx::Error },
    #[error("Search {field} could not be written: {source}")]
    Encode { field: &'static str, source: serde_json::Error },
}

/// Why the query bar's operator preferences could not be converted.
#[derive(Debug, thiserror::Error)]
pub enum OperatorPreferencesError {
    #[error("operator preferences could not be written: {source}")]
    Encode { source: serde_json::Error },
    #[error("saved operator preferences are invalid: {source}")]
    Decode { source: serde_json::Error },
}

/// Why an index-backed palette layer could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LayerError {
    #[error("{source}")]
    Runtime { source: RuntimeError },
    #[error("the palette layer task did not complete: {source}")]
    Task { source: tokio::task::JoinError },
    #[error("the replay index is not open: {source}")]
    Open { source: sqlx::Error },
    #[error("{source}")]
    Rows { source: PaletteError },
}

/// The replay file dialog could not be shown; nsh reports why only as text.
#[derive(Debug, thiserror::Error)]
#[error("could not show the replay file dialog: {reason}")]
pub struct ReplayDialogError {
    pub reason: String,
}
