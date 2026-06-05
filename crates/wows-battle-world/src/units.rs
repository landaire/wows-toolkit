//! Domain newtypes for angles, flags, durations, and match outcome.

use wows_replays::types::TeamId;

/// An angle in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radians(pub f32);

/// An angle in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Degrees(pub f32);

/// The Vehicle.visibilityFlags radar/hydro bitmask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VisibilityFlags(pub u32);

/// Seconds left in the match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecondsRemaining(pub i64);

/// Match outcome once decided; `Option::None` means the match is not yet decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchWinner {
    Team(TeamId),
    Draw,
}
