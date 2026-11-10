//! What a sharing decision needs to know about one parsed battle.

use crate::upload::ResultsScan;
use crate::upload::build_tracker::BuildTrackerPayload;

/// What a decision needs to know about the parsed battle.
///
/// Taken as plain data rather than the report itself: the decision is shared and
/// must not depend on any app's own types.
pub struct Shareable {
    /// `ReplayMeta::gameType`, which says whether the service takes this battle.
    pub game_type: String,
    pub version: wowsunpack::data::Version,
    /// Whether the recording player's ship is positively known not to be a test
    /// ship. Uncertainty is `false`, which keeps the file off `/api/replays`.
    pub self_confirmed_non_test: bool,
    /// Whether the packet stream carried an end-of-battle marker.
    pub results: ResultsScan,
    /// The per-player build payloads, for the build-data mode. Empty is nothing
    /// to send.
    pub builds: Vec<BuildTrackerPayload>,
}
