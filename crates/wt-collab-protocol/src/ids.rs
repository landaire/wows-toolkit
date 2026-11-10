//! What a session calls the things it shares. Each is a `u64` on the wire;
//! the newtypes keep a board's id from being passed where a cap point's is
//! expected.

/// Who someone is in a session. Assigned by the host, so a reader alone has
/// none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UserId(u64);

impl UserId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

/// What a tactics board is called in a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoardId(u64);

impl BoardId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    /// A name nothing else in the session holds.
    pub fn fresh() -> Self {
        Self(fresh_raw())
    }
}

/// What a replay window is called in a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReplayId(u64);

impl ReplayId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    /// A name nothing else in the session holds.
    pub fn fresh() -> Self {
        Self(fresh_raw())
    }
}

/// What a capture point is called in a session. Its place in a list is not
/// that: every peer's list is its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CapPointId(u64);

impl CapPointId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    /// A name nothing else in the session holds.
    pub fn fresh() -> Self {
        Self(fresh_raw())
    }
}

/// An id nothing else in the session holds.
///
/// Drawn at random rather than counted up: every peer mints its own, and two
/// of them naming something at the same moment must not land on the same
/// number.
pub fn fresh_raw() -> u64 {
    rand::random()
}
