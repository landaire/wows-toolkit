//! Ship-config and WoWS-numbers URL builders for the Actions column menu
//! (`table.rs::actions_cell`). Ports `crate::util::formatting`'s
//! `build_ship_config_url`/`build_short_ship_config_url`/
//! `build_wows_numbers_url` from the egui app field-for-field; those
//! functions were already egui-free (they only take `Player` and
//! `GameMetadataProvider`), so this is a direct copy minus the `debug!` log
//! call `build_short_ship_config_url` made on the egui side.

use wows_replay_insights::ResolvedBuild;
use wows_replay_insights::build::wowssb;
use wows_replays::analyzer::battle_controller::Player;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;

/// Matches the egui app's `TOOLKIT_REFERRER` constant (`util/formatting.rs`),
/// passed through to wowssb build URLs unchanged.
const TOOLKIT_REFERRER: &str = "landaire";

pub fn build_wows_numbers_url(player: &Player) -> Option<String> {
    let state = player.initial_state();
    let realm = state.realm()?;
    Some(format!("https://{}.wows-numbers.com/player/{},{}", realm, state.db_id(), state.username()))
}

pub fn build_ship_config_url(player: &Player, metadata_provider: &GameMetadataProvider) -> Option<String> {
    let build = ResolvedBuild::from_player(player, metadata_provider, Version::default())?;
    let build_name = format!("replay_{}", player.initial_state().username());
    Some(wowssb::build_url(&build, &build_name, Some(TOOLKIT_REFERRER)))
}

pub fn build_short_ship_config_url(player: &Player, metadata_provider: &GameMetadataProvider) -> Option<String> {
    let build = ResolvedBuild::from_player(player, metadata_provider, Version::default())?;
    let build_name = format!("replay_{}", player.initial_state().username());
    Some(wowssb::build_short_url(&build, &build_name, Some(TOOLKIT_REFERRER)))
}
