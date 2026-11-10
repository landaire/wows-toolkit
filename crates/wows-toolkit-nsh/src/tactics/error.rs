use wows_toolkit_viewmodel::tactics::PresetError;

use crate::game_data::GameDataError;
use crate::tactics::ids::InkIndex;
use crate::tactics::ids::ShipMatchIndex;

/// Why a tactics board command could not be carried out.
#[derive(Debug, thiserror::Error)]
pub enum TacticsError {
    #[error("{}", rust_i18n::t!("ui.tactics.preset_needs_a_name"))]
    PresetNeedsName,
    #[error("{}", rust_i18n::t!("ui.tactics.preset_needs_a_map"))]
    PresetNeedsMap,
    #[error("{}", rust_i18n::t!("ui.tactics.preset_save_failed", error = source.to_string()))]
    PresetSave {
        name: String,
        #[source]
        source: PresetError,
    },
    #[error("{}", rust_i18n::t!("ui.tactics.preset_load_failed", error = source.to_string()))]
    PresetLoad {
        name: String,
        #[source]
        source: PresetError,
    },
    #[error("{}", rust_i18n::t!("ui.tactics.preset_delete_failed", error = source.to_string()))]
    PresetDelete {
        name: String,
        #[source]
        source: PresetError,
    },
    #[error("{}", rust_i18n::t!("ui.messages.wows_dir_not_set"))]
    NoReplaysDir,
    #[error("{}", rust_i18n::t!("ui.tactics.scan_needs_game_data"))]
    NoGameData,
    #[error("there is no map {value:?} to set the board on")]
    UnknownMap { value: String },
    #[error("the map has no mode {value:?}")]
    UnknownMode { value: String },
    #[error("there is no ship match {index}")]
    UnknownShip { index: ShipMatchIndex },
    #[error("there is no ink {index}")]
    UnknownInk { index: InkIndex },
    #[error("tactics layout task failed: {source}")]
    LayoutTask {
        #[source]
        source: tokio::task::JoinError,
    },
    #[error("tactics game data task failed: {source}")]
    GameDataTask {
        #[source]
        source: tokio::task::JoinError,
    },
    #[error(transparent)]
    GameData(GameDataError),
}
