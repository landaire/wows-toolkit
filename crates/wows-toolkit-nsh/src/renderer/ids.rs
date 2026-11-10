//! Identifiers and indices the renderer passes between its parts and the view.

use std::fmt;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use serde::Deserialize;
use serde::Serialize;
use wows_toolkit_replay_data::playback::SPEEDS;

use crate::js_newtype::js_newtype;

/// One open viewport, hover preview or watched window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RendererId(u32);

js_newtype!(RendererId, u32);

impl RendererId {
    /// An id no other viewport, preview or watched window holds, so their
    /// surfaces never share a name.
    pub fn fresh() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(1);

        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for RendererId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Counts the changes to something the view waits on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Revision(u32);

js_newtype!(Revision, u32);

impl Revision {
    pub fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// One message the viewport said. Counts up, so the same text said twice is
/// shown twice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoticeId(u32);

js_newtype!(NoticeId, u32);

impl NoticeId {
    pub fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// A frame of a played-back battle, counted from its first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameIndex(usize);

impl FrameIndex {
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn get(self) -> usize {
        self.0
    }

    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// A rung of the playback speed ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeedIndex(usize);

impl SpeedIndex {
    /// Rung `index`, or `None` past the top of the ladder.
    pub fn new(index: usize) -> Option<Self> {
        (index < SPEEDS.len()).then_some(Self(index))
    }

    pub fn speed(self) -> f32 {
        SPEEDS[self.0]
    }
}

/// One ship the ship search found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchIndex(usize);

impl MatchIndex {
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn get(self) -> usize {
        self.0
    }
}
