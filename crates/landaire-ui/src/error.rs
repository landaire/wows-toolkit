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
