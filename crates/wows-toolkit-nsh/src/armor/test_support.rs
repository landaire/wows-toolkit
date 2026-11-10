//! Hooks for tests that start one app after another in a process. The
//! service outlives each app, so a panel one test left open is open in the
//! next.

use crate::armor::state;

/// Shows the panels as a new session opens them: the options rail open and
/// the penetration check closed.
pub fn reset_panels() {
    state::update(|state| {
        state.options_open = true;
        state.pen_open = false;
    });
}
