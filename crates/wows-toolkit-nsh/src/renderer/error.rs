use rust_i18n::t;
use wows_replays::types::EntityId;
use wows_toolkit_replay_data::minimap::PreviewError;
use wows_toolkit_replay_data::playback::ExportError;

use crate::renderer::ids::RendererId;

/// Why a renderer command could not be carried out. Its `Display` is
/// [`RendererError::user_text`], which is what the view shows.
#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    UnknownRenderer {
        id: RendererId,
    },
    UnknownLayer {
        id: String,
    },
    UnknownCodec {
        value: String,
    },
    UnknownInk {
        index: u32,
    },
    UnknownSpeed {
        index: u32,
    },
    UnknownShip {
        index: u32,
    },
    UnknownSpecies {
        short: String,
    },
    UnknownShipParam {
        param_index: String,
    },
    UnknownKind {
        index: u32,
    },
    Dialog {
        reason: String,
    },
    NotReady,
    /// The single clipboard export found no temporary file to encode into.
    NoTemporaryFile,
    /// The clipboard batch found no temporary directory to render into.
    NoTemporaryDirectory,
    BatchRunning,
    UnknownWindow {
        replay_id: String,
    },
    NothingToRender,
    UnknownEntity {
        entity_id: String,
    },
    NoArmorToShow {
        entity_id: EntityId,
    },
    NoGameData,
}

impl RendererError {
    /// What the reader is told, in the reader's language. The variants that
    /// only a defect in the view can raise stay in English.
    pub fn user_text(&self) -> String {
        match self {
            Self::Dialog { reason } => t!("ui.replay.renderer.error_dialog", reason = reason).into_owned(),
            Self::NotReady => t!("ui.replay.renderer.error_not_ready").into_owned(),
            Self::NoTemporaryFile => t!("ui.replay.renderer.error_no_temporary_file").into_owned(),
            Self::NoTemporaryDirectory => t!("ui.replay.renderer.export_path_unusable").into_owned(),
            Self::BatchRunning => t!("ui.replay.renderer.error_batch_running").into_owned(),
            Self::UnknownWindow { replay_id } => {
                t!("ui.replay.renderer.error_unknown_window", window = replay_id).into_owned()
            }
            Self::NoGameData => t!("ui.replay.renderer.error_no_game_data").into_owned(),
            Self::UnknownRenderer { id } => format!("there is no open renderer {id}"),
            Self::UnknownLayer { id } => format!("there is no layer {id:?}"),
            Self::UnknownCodec { value } => format!("there is no codec {value:?}"),
            Self::UnknownInk { index } => format!("there is no ink {index}"),
            Self::UnknownSpeed { index } => format!("there is no speed {index}"),
            Self::UnknownShip { index } => format!("there is no ship match {index}"),
            Self::UnknownSpecies { short } => format!("there is no ship species {short:?}"),
            Self::UnknownShipParam { param_index } => format!("the build does not know the ship {param_index:?}"),
            Self::UnknownKind { index } => format!("there is no event kind {index}"),
            Self::NothingToRender => "there is no replay to render".to_owned(),
            Self::UnknownEntity { entity_id } => format!("there is no ship with entity {entity_id:?} in this battle"),
            Self::NoArmorToShow { entity_id } => {
                format!("ship {entity_id} took no recorded hits, so there is no armor to show")
            }
        }
    }
}

impl std::fmt::Display for RendererError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.user_text())
    }
}

/// Why a replay's battle could not be read for drawing.
#[derive(Debug, thiserror::Error)]
pub enum ReplayLoadError {
    #[error(transparent)]
    Preview(#[from] PreviewError),
    #[error("{}", RendererError::NoGameData)]
    NoGameData,
}

/// Why an export did not reach where it was going.
#[derive(Debug, thiserror::Error)]
pub enum ExportFailure {
    /// Said in the reader's language: the reader picked the path.
    #[error("{}", t!("ui.replay.renderer.export_path_unusable"))]
    PathUnusable,
    #[error(transparent)]
    Encode(ExportError),
    #[error(transparent)]
    Clipboard(#[from] arboard::Error),
}

impl From<ExportError> for ExportFailure {
    fn from(err: ExportError) -> Self {
        match err {
            ExportError::PathUnusable { .. } => Self::PathUnusable,
            err => Self::Encode(err),
        }
    }
}
