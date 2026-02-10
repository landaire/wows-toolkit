use serde::Serialize;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::types::AccountId;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;

#[derive(Serialize)]
pub(crate) struct BuildTrackerPayload {
    game_version: Version,
    // Player's WG DB ID
    player_id: AccountId,
    // Which realm this game was played on
    realm: String,
    /// Ship's GameParams index
    ship_id: String,
    ship_kind: Option<Species>,
    /// Module GameParams indices
    modules: Vec<String>,
    /// Ship upgrades GameParams IDs
    upgrades: Vec<String>,
    /// Captain GameParams ID
    captain: String,
    skills: Vec<u8>,
    // Consumables GameParams ID
    consumables: Vec<String>,
    // Signals GameParams ID
    signals: Vec<String>,
    // Which game type this build was seen in
    game_type: String,
}

fn indicies_to_index(ids: &[u32], metadata_provider: &GameMetadataProvider) -> Vec<String> {
    ids.iter().filter_map(|id| Some(metadata_provider.game_param_by_id(*id)?.index().to_owned())).collect()
}

impl BuildTrackerPayload {
    pub fn build_from(
        player: &Player,
        realm: String,
        version: Version,
        game_type: String,
        metadata_provider: &GameMetadataProvider,
    ) -> Option<Self> {
        let entity = player.vehicle_entity()?;
        let config = entity.props().ship_config();
        let ship = player.vehicle();

        Some(Self {
            game_version: version,
            realm,
            player_id: player.initial_state().db_id(),
            ship_id: ship.index().to_string(),
            ship_kind: ship.species(),
            modules: indicies_to_index(config.units(), metadata_provider),
            upgrades: indicies_to_index(config.modernization(), metadata_provider),
            captain: entity.captain().map(|capt| capt.index()).unwrap_or("PCW001").to_string(),
            skills: entity.commander_skills_raw(ship.species()?).to_vec(),
            consumables: indicies_to_index(config.abilities(), metadata_provider),
            signals: indicies_to_index(config.signals(), metadata_provider),
            game_type,
        })
    }
}
