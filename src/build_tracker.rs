use serde::Serialize;
use wows_replays::analyzer::battle_controller::VehicleEntity;
use wowsunpack::{
    data::Version,
    game_params::{
        provider::GameMetadataProvider,
        types::{GameParamProvider, Species},
    },
};

#[derive(Serialize)]
pub(crate) struct BuildTrackerPayload {
    game_version: Version,
    // Player's WG DB ID
    player_id: i64,
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
    ids.iter()
        .filter_map(|id| Some(metadata_provider.game_param_by_id(*id)?.index().to_owned()))
        .collect()
}

impl BuildTrackerPayload {
    pub fn build_from(entity: &VehicleEntity, realm: String, version: Version, game_type: String, metadata_provider: &GameMetadataProvider) -> Self {
        let config = entity.props().ship_config();
        let player = entity.player().expect("entity has no player?");
        let ship = player.vehicle();

        Self {
            game_version: version,
            realm,
            player_id: player.db_id(),
            ship_id: ship.index().to_string(),
            ship_kind: ship.species(),
            modules: indicies_to_index(config.units(), metadata_provider),
            upgrades: indicies_to_index(config.modernization(), metadata_provider),
            captain: entity.captain().map(|capt| capt.index()).unwrap_or("PCW001").to_string(),
            skills: entity.commander_skills_raw().to_vec(),
            consumables: indicies_to_index(config.abilities(), metadata_provider),
            signals: indicies_to_index(config.signals(), metadata_provider),
            game_type,
        }
    }
}
