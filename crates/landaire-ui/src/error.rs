use crate::dock::layout::GroupId;
use crate::dock::layout::PanelId;
use crate::dock::layout::PanelKind;
use crate::palette::Appearance;
use crate::palette::Tier;

/// A palette whose tiers do not stack the way its appearance requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TierOrderError {
    #[error("{appearance:?} palette: text is not on the far side of the panel")]
    TextNotAgainstPanel { appearance: Appearance },

    #[error("{appearance:?} palette: {lower:?} is not darker than {upper:?}")]
    OutOfOrder { appearance: Appearance, lower: Tier, upper: Tier },

    #[error("{first:?} and {second:?} are the same color")]
    Indistinct { first: Tier, second: Tier },
}

/// Text that is not a color in hex.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HexColorError {
    #[error("character {position} is not a hex digit")]
    NotHex { position: usize },

    #[error("{length} hex digits, not 3 or 6")]
    Length { length: usize },
}

/// A change a dock was asked for that its layout cannot take.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DockError {
    #[error("no panel {id:?} is in the dock")]
    UnknownPanel { id: PanelId },

    #[error("a panel {id:?} is already in the dock")]
    DuplicatePanel { id: PanelId },

    #[error("no panel kind {kind:?} is registered")]
    UnknownKind { kind: PanelKind },

    #[error("panel {id:?} cannot be closed")]
    NotClosable { id: PanelId },

    #[error("no tab group {group:?} is in the dock")]
    UnknownGroup { group: GroupId },

    #[error("the scope that made the dock has ended")]
    Gone,
}

/// A split share outside 0 to 100 percent.
#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
#[error("{percent} is not a share from 0 to 100 percent")]
pub struct ShareError {
    pub percent: f64,
}

/// Why a headless window could not be opened, driven or read back.
#[cfg(feature = "test-support")]
#[derive(Debug, thiserror::Error)]
pub enum HeadlessError {
    #[error("the application did not build")]
    App { source: zgui::runtime::AppError },

    #[error("the application opened no window")]
    NoWindow,

    #[error("element {element:?} produced no box")]
    NoBox { element: zgui::view::NodeId },

    #[error("the window is not drawn on a graphics device, so it has no pixels to read")]
    NotDrawnOnDevice,

    #[error("the readback held no pixels")]
    NoPixels,

    #[error("could not create {path:?}")]
    Write { path: std::path::PathBuf, source: std::io::Error },

    #[error("could not write {path:?} as a PNG")]
    Encode { path: std::path::PathBuf, source: image::ImageError },
}
