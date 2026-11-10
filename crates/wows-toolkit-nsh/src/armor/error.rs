//! Why an armor viewer request could not be answered.

use wows_toolkit_armor::load_ship::ExportGlbError;

#[derive(Debug, thiserror::Error)]
pub enum ArmorError {
    #[error("the ship catalog has not loaded")]
    CatalogNotReady,
    #[error("no ship is shown in the active pane")]
    NoShip,
    #[error("ship {param_index} has no comparison data")]
    NoComparisonData { param_index: String },
    #[error("no export dialog is open")]
    NoExportDialog,
    #[error("the ship has not been read yet")]
    ExportNotReady,
    #[error("could not show the save dialog: {reason}")]
    SaveDialog { reason: String },
    #[error(transparent)]
    Export(#[from] ExportGlbError),
    #[error("the settings database failed: {source}")]
    Database { source: sqlx::Error },
    #[error("could not start a runtime: {source}")]
    Runtime { source: std::io::Error },
    #[error("an armor viewer task failed: {source}")]
    Task { source: tokio::task::JoinError },
}
