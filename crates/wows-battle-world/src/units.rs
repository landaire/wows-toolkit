//! Domain newtypes for angles, flags, durations, and match outcome.

use wows_replays::types::TeamId;

/// An angle in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radians(pub f32);

/// An angle in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Degrees(pub f32);

/// Seconds left in the match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecondsRemaining(pub i64);

/// A duration in seconds, on the same scale as `GameClock`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Seconds(pub f32);

/// Match outcome once decided; `Option::None` means the match is not yet decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchWinner {
    Team(TeamId),
    Draw,
}
