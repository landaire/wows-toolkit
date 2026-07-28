//! Doubles shared by the crate's unit tests: a `ResourceLoader` stub and a
//! `ReplayMeta` small enough to construct inline.

use wows_replays::Rc;
use wows_replays::ReplayMeta;
use wows_replays::analyzer::battle_controller::MetadataPlayer;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::decoder::PlayerStateData;
use wows_replays::types::AccountId;
use wows_replays::types::EntityId;
use wows_replays::types::GameParamId;
use wows_replays::types::Relation;
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

// Minimal PlayerStateData built via its derived Deserialize impl: its fields are
// private to wows_replays, so this is the only way to construct one from
// outside that crate. `raw`/`raw_with_names` are `#[serde(skip_deserializing)]`
// and default to empty.
const SELF_PLAYER_JSON: &str = r#"{
    "username": "self",
    "clan": "",
    "clan_id": 0,
    "clan_color": 0,
    "db_id": 1,
    "realm": null,
    "meta_ship_id": 1,
    "entity_id": 3,
    "team_id": 0,
    "max_health": 50000,
    "is_abuser": false,
    "is_hidden": false,
    "is_bot": false,
    "human_properties": null
}"#;

/// The recording player, resolved through the same `Player::from_arena_player`
/// path production ingest uses, alongside their vehicle entity id.
pub(crate) fn self_player(resources: &StubResources) -> (EntityId, Player) {
    let state: PlayerStateData =
        serde_json::from_str(SELF_PLAYER_JSON).expect("fixture matches PlayerStateData's shape");
    let metadata =
        MetadataPlayer::new(AccountId::from(1u32), "self".to_string(), Relation::new(0), Rc::clone(&resources.0));
    let player =
        Player::from_arena_player(&state, &metadata, resources).expect("stub resources always resolve a vehicle");
    (state.entity_id(), player)
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
