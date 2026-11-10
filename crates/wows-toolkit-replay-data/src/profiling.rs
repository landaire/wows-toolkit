//! Named sampling points for a front end's heap profiler.
//!
//! A build load is most of a session's heap, so the steps inside it are where
//! a profile needs marks. The front end that profiles installs its sampler once
//! at startup; without one a mark does nothing.

use std::sync::OnceLock;

static MARKER: OnceLock<fn(&str)> = OnceLock::new();

/// Routes every later [`mark`] to `marker`. Only the first install takes.
pub fn install(marker: fn(&str)) {
    if MARKER.set(marker).is_err() {
        tracing::warn!("a heap marker is already installed; the second one is ignored");
    }
}

/// Samples the heap at a named point, when a marker is installed. `label` is a
/// short step name with no spaces.
pub fn mark(label: &str) {
    if let Some(marker) = MARKER.get() {
        marker(label);
    }
}
