//! What the replays tab does differently under test.

use std::cell::Cell;

thread_local! {
    /// Whether the tab may plan game data downloads and fetch result mappings. A test stays off
    /// the network, and free of the modal download offer, unless it asks.
    static FETCHES_ALLOWED: Cell<bool> = const { Cell::new(false) };
}

/// Lets the tab plan downloads and fetch result mappings, as it does outside a test, until the
/// guard is dropped.
#[must_use = "fetches are held again as soon as the guard is dropped"]
pub struct FetchesAllowed {
    before: bool,
}

impl Drop for FetchesAllowed {
    fn drop(&mut self) {
        FETCHES_ALLOWED.set(self.before);
    }
}

pub fn allow_fetches() -> FetchesAllowed {
    FetchesAllowed { before: FETCHES_ALLOWED.replace(true) }
}

pub(super) fn fetches_allowed() -> bool {
    FETCHES_ALLOWED.get()
}
