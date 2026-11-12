//! What the data-sharing setting needs from one parsed battle.

use wows_toolkit_viewmodel::upload::ResultsScan;
use wows_toolkit_viewmodel::upload::build_tracker::BuildTrackerPayload;

/// What a decision needs to know about the parsed battle.
///
/// Taken as plain data rather than the report itself: the decision is shared and
/// must not depend on either app's own types.
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

/// Whether the recording player's ship is positively not a test ship.
///
/// Any doubt is `false`: an unknown ship keeps the whole file off the service,
/// which is the rule `decide_upload_action` documents.
pub fn self_is_not_a_test_ship(report: &wows_battle_world::report::BattleReport) -> bool {
    report
        .players()
        .iter()
        .find(|player| player.relation().is_self())
        .and_then(|player| player.vehicle().vehicle())
        .map(|vehicle| !vehicle.is_test_ship())
        .unwrap_or(false)
}
