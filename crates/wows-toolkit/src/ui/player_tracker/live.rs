//! The roster of the match currently in progress.
//!
//! The model is `wows_toolkit_viewmodel::player_tracker::live`, which the
//! GPUI port renders too; this adapts it to the tracker's own storage.

use std::collections::HashMap;

use wows_replays::types::AccountId;

pub use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
pub use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
pub use wows_toolkit_viewmodel::player_tracker::live::LiveRosterRow;
pub use wows_toolkit_viewmodel::player_tracker::live::ResolvedRoster;
pub use wows_toolkit_viewmodel::player_tracker::live::TrackedNames;

use super::TrackedPlayer;
use crate::data::wows_data::BuildData;

/// Tracked players keyed by lower-cased name, covering the current name and
/// every recorded alias.
pub(crate) fn build_name_index(players: &HashMap<AccountId, TrackedPlayer>) -> HashMap<String, AccountId> {
    wows_toolkit_viewmodel::player_tracker::live::build_name_index(players.iter().map(|(id, player)| TrackedNames {
        id: *id,
        current: &player.last_name,
        aliases: &player.names,
    }))
}

/// Joins the live roster to game data, tracked history and the identity scan.
pub(crate) fn resolve_roster(
    live: &LiveMatch,
    tracked: &HashMap<AccountId, TrackedPlayer>,
    identities: Option<&LiveIdentities>,
    wows_data: Option<&BuildData>,
) -> ResolvedRoster {
    let metadata = wows_data.and_then(|data| data.game_metadata.as_deref());
    wows_toolkit_viewmodel::player_tracker::live::resolve_roster(live, &build_name_index(tracked), identities, metadata)
}
