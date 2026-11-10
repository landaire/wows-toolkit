//! Numbers the board's commands and its view name things by.

use std::fmt;

use crate::js_newtype::js_newtype;

/// Counts the board's changes, so the view can wait for the next one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewRevision(u32);

js_newtype!(ViewRevision, u32);

impl ViewRevision {
    pub fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// Names one notice, so the same text said twice is shown twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoticeId(u32);

js_newtype!(NoticeId, u32);

impl NoticeId {
    /// The view holds zero before any notice, so counting starts above it.
    pub const FIRST: Self = Self(1);

    pub fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// A place in the list of offered inks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InkIndex(u32);

js_newtype!(InkIndex, u32);

impl InkIndex {
    pub fn from_position(position: usize) -> Self {
        Self(position as u32)
    }

    pub fn position(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for InkIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A place in the list of ships the search found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShipMatchIndex(u32);

js_newtype!(ShipMatchIndex, u32);

impl ShipMatchIndex {
    pub fn from_position(position: usize) -> Self {
        Self(position as u32)
    }

    pub fn position(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for ShipMatchIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
