//! Keeping each build's game constants (`constants.json`) current: cached under the shared storage
//! directory, fetched from GitHub, or generated from the installed client's scripts with
//! `wowsdeob` when GitHub has none for the build. Battle results are decoded through them.

mod acquire;
mod worker;

use std::collections::BTreeSet;

use zgui::reactive::LocalStorage;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::UpdateValue;

pub use crate::services::constants::acquire::Fallback;
pub use crate::services::constants::acquire::Missed;
pub use crate::services::constants::acquire::Outcome;
pub use crate::services::constants::acquire::Provenance;
pub use crate::services::constants::acquire::Step;
pub use crate::services::constants::acquire::Unpublished;
pub use crate::services::constants::acquire::Wanted;
pub use crate::services::constants::worker::ConstantsRequest;
pub use crate::services::constants::worker::ConstantsWorker;
pub use crate::services::constants::worker::cached_current;

/// The builds this session is updating, as a `Copy` handle.
#[derive(Clone, Copy)]
pub struct ConstantsUpdates {
    /// Builds with a request on the worker, so a second ask for one starts nothing.
    in_flight: StoredValue<InFlight, LocalStorage>,
}

impl ConstantsUpdates {
    pub(super) fn new() -> Self {
        Self { in_flight: StoredValue::new_local(InFlight::default()) }
    }

    /// Claims the builds of `wanted` that are not already being updated, released when each
    /// returned claim is dropped.
    pub fn claim(self, wanted: Vec<Wanted>) -> Vec<Claim> {
        // Nothing is claimed once the window's services are gone.
        let claimed = self.in_flight.try_update_value(|in_flight| in_flight.claim(wanted)).unwrap_or_default();

        claimed.into_iter().map(|wanted| Claim { updates: self, wanted }).collect()
    }
}

/// One build being updated. Released when dropped, so work cancelled with its view does not leave
/// the build claimed for the rest of the session.
pub struct Claim {
    updates: ConstantsUpdates,
    wanted: Wanted,
}

impl Claim {
    pub fn wanted(&self) -> &Wanted {
        &self.wanted
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let build = self.wanted.build;
        // `try_`, as the claim can outlive the window whose services hold the set.
        self.updates.in_flight.try_update_value(|in_flight| in_flight.release(build));
    }
}

/// The builds with a request on the worker.
#[derive(Default)]
struct InFlight {
    builds: BTreeSet<u32>,
}

impl InFlight {
    /// Takes the builds of `wanted` that are not held, once each.
    fn claim(&mut self, wanted: Vec<Wanted>) -> Vec<Wanted> {
        wanted.into_iter().filter(|wanted| self.builds.insert(wanted.build)).collect()
    }

    fn release(&mut self, build: u32) {
        self.builds.remove(&build);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(build: u32) -> Wanted {
        Wanted { build, release: None }
    }

    #[test]
    fn a_build_being_updated_is_not_claimed_again() {
        let mut in_flight = InFlight::default();

        let first = in_flight.claim(vec![wanted(1), wanted(2), wanted(1)]);
        let second = in_flight.claim(vec![wanted(2), wanted(3)]);

        assert_eq!(first, vec![wanted(1), wanted(2)]);
        assert_eq!(second, vec![wanted(3)]);
    }

    #[test]
    fn a_released_build_is_claimed_again() {
        let mut in_flight = InFlight::default();
        in_flight.claim(vec![wanted(1)]);

        in_flight.release(1);

        assert_eq!(in_flight.claim(vec![wanted(1)]), vec![wanted(1)]);
    }
}
