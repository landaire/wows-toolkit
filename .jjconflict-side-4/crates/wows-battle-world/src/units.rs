//! Domain newtypes for angles, flags, durations, and match outcome.

use wows_replays::types::TeamId;

/// Angle newtypes. Defined in `wows-core` alongside the distance units.
pub use wows_core::units::Degrees;
/// Angle newtypes. Defined in `wows-core` alongside the distance units.
pub use wows_core::units::Radians;
/// A duration in seconds, on the same scale as `GameClock`. Defined in
/// `wows-core`.
pub use wows_core::units::Seconds;

/// Seconds left in the match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecondsRemaining(pub i64);

/// Match outcome once decided; `Option::None` means the match is not yet decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchWinner {
    Team(TeamId),
    Draw,
}
