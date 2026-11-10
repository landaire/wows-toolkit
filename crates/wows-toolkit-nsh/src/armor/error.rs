//! Why an armor viewer request could not be answered.

use rust_i18n::t;
use wows_toolkit_armor::error::ArmorAssetsError;
use wows_toolkit_armor::error::ExportGlbError;

use crate::game_data::GameDataError;

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
    #[error("not a #rrggbb colour: {color}")]
    InvalidColor { color: String },
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

/// Why the ship data every pane loads from could not be read.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error(transparent)]
    GameData(#[from] GameDataError),
    #[error(transparent)]
    Assets(#[from] ArmorAssetsError),
}

/// Why the export dialog could not read its ship. `detail` is the formatted
/// report of a reader whose report type this crate does not depend on.
#[derive(Debug, thiserror::Error)]
pub enum ExportReadError {
    #[error("{}", t!("ui.armor.export.vehicle_not_found"))]
    VehicleNotFound { param_index: String },
    #[error("{detail}")]
    LoadShip { detail: String },
    #[error("{detail}")]
    CamoSource { detail: String },
}
