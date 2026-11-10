use std::num::ParseIntError;

use wows_replays::types::GameParamId;
use wows_toolkit_config::queries;
use wows_toolkit_viewmodel::stats::chart_model::ChartId;

use super::ids::ParamIdText;
use super::ids::ScopeName;
use super::ids::ViewChartId;

/// Why a Stats call failed.
#[derive(Debug, thiserror::Error)]
pub enum StatsError {
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Layout(#[from] LayoutError),
    #[error("{0}")]
    Image(#[from] ChartImageError),
    #[error("no stats view is open as {scope}")]
    UnknownScope { scope: ScopeName },
    #[error("chart {chart} is not open")]
    UnknownChart { chart: ViewChartId },
    #[error("{value} is not a ship id")]
    ShipId { value: ParamIdText, source: ParseIntError },
    /// The recorded games key ships by a signed 64-bit column.
    #[error("{ship} is not a ship id")]
    ShipIdOutOfRange { ship: GameParamId },
    #[error("game data is not loaded; set the World of Warships directory in Settings")]
    GameDataUnavailable,
    #[error("the directory is still being read")]
    StillReading,
    #[error("{color} is not a CSS colour")]
    Color { color: String },
    #[error("the stats task did not finish: {source}")]
    Task { source: tokio::task::JoinError },
}

/// Why the session's rows could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not open the settings database: {source}")]
    Open { source: sqlx::Error },
    #[error("could not read the recorded games: {source}")]
    ReadGames { source: sqlx::Error },
    #[error("could not read setting {key}: {source}")]
    ReadSetting { key: &'static str, source: queries::SettingError },
    #[error("could not save setting {key}: {source}")]
    WriteSetting { key: &'static str, source: sqlx::Error },
    #[error("could not forget the recorded games: {source}")]
    Clear { source: sqlx::Error },
    #[error("could not start the database runtime: {source}")]
    Runtime { source: std::io::Error },
}

/// Why a layout could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("the layout names a panel this tab does not have: {panel}")]
    UnknownPanel { panel: String },
    #[error("the layout repeats chart {}", .id.0)]
    RepeatedChart { id: ChartId },
    #[error("chart {} has an id too large to show", .id.0)]
    ChartIdTooLarge { id: ChartId },
}

/// Why a chart could not be copied as an image.
#[derive(Debug, thiserror::Error)]
pub enum ChartImageError {
    #[error("the chart is not shown, so it has no size to draw at")]
    NotShown,
    #[error("the chart has nothing to plot")]
    Empty,
    #[error("the chart image could not be encoded: {source}")]
    Encode { source: png::EncodingError },
}

/// Why the session opened with the default layout instead of its saved one.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("Saved chart layout could not be restored")]
    Layout { source: LayoutError },
    #[error("Could not read saved chart settings: {source}")]
    ReadCharts { source: StoreError },
    #[error("Could not read saved chart layout: {source}")]
    ReadLayout { source: StoreError },
}
