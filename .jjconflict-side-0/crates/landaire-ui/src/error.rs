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
