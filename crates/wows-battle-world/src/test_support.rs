//! Doubles shared by the crate's unit tests: a `ResourceLoader` stub and a
//! `ReplayMeta` small enough to construct inline.

use wows_replays::Rc;
use wows_replays::ReplayMeta;
use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::game_params::types::Achievement;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::ParamData;
use wowsunpack::rpc::entitydefs::EntitySpec;

/// Stands in for a real `ResourceLoader`: always resolves to the same fixture
/// `Param` regardless of id, since tests never look up a real ship. Resolution
/// must succeed so `Player::from_arena_player` builds a player.
pub(crate) struct StubResources(pub Rc<Param>);

impl ResourceLoader for StubResources {
    fn localized_name_from_param(&self, _param: &Param) -> Option<String> {
        None
    }

    fn localized_name_from_id(&self, _id: &TranslationKey) -> Option<String> {
        None
    }

    fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
        Some(self.0.clone())
    }

    fn entity_specs(&self) -> &[EntitySpec] {
        &[]
    }
}

pub(crate) fn fixture_param() -> Rc<Param> {
    Rc::new(
        Param::builder()
            .id(GameParamId::from(1u32))
            .index("IDX".to_string())
            .name("Fixture".to_string())
            .nation("USA".to_string())
            .data(ParamData::Achievement(
                Achievement::builder()
                    .is_group(false)
                    .one_per_battle(false)
                    .ui_type("x".to_string())
                    .ui_name("x".to_string())
                    .build(),
            ))
            .build(),
    )
}

pub(crate) fn minimal_meta() -> ReplayMeta {
    ReplayMeta {
        matchGroup: None,
        gameMode: 0,
        gameType: None,
        clientVersionFromExe: "0, 15, 5, 12668706".to_string(),
        scenarioUiCategoryId: None,
        mapDisplayName: String::new(),
        mapId: 0,
        clientVersionFromXml: String::new(),
        weatherParams: None,
        duration: 0,
        gameLogic: None,
        name: String::new(),
        scenario: String::new(),
        playerID: AccountId::from(1u32),
        vehicles: Vec::new(),
        playersPerTeam: 1,
        dateTime: String::new(),
        mapName: String::new(),
        playerName: "self".to_string(),
        scenarioConfigId: 0,
        teamsCount: 2,
        logic: None,
        playerVehicle: String::new(),
        battleDuration: None,
    }
}
