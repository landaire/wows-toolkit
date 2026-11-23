//! What the replays tab does differently under test.

use std::cell::Cell;
use std::cell::RefCell;

use wows_toolkit_replay_data::replay_dir::MissingBuild;

thread_local! {
    /// Whether the tab may plan game data downloads and fetch result mappings. A test stays off
    /// the network, and free of the modal download offer, unless it asks.
    static FETCHES_ALLOWED: Cell<bool> = const { Cell::new(false) };
    /// The builds the tab would have offered to fetch while fetches were held.
    static HELD_OFFERS: RefCell<Vec<MissingBuild>> = const { RefCell::new(Vec::new()) };
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

pub(crate) fn fetches_allowed() -> bool {
    FETCHES_ALLOWED.get()
}

/// Notes `missing` as offered while fetches are held.
pub(crate) fn hold_offer(missing: &[MissingBuild]) {
    HELD_OFFERS.with(|held| held.borrow_mut().extend_from_slice(missing));
}

/// The builds offered while fetches were held, since this was last asked.
pub(crate) fn take_held_offers() -> Vec<MissingBuild> {
    HELD_OFFERS.with(|held| std::mem::take(&mut *held.borrow_mut()))
}
