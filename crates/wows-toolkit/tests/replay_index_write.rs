use jiff::Timestamp;
use sqlx::sqlite::SqlitePoolOptions;
use std::path::PathBuf;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;
use wows_replays::types::GameParamId;
use wows_toolkit::data::replay_index::MappedRows;
use wows_toolkit::data::replay_index::write_index;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::IndexedVehicleRow;
use wows_toolkit_config::index::rows::MatchFilter;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::VehicleRelation;

#[tokio::test]
async fn write_index_persists_all_three_tables() {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("../wows-toolkit-config/migrations").run(&pool).await.unwrap();
    let src = query::ensure_default_source(
        &pool,
        std::path::Path::new("C:/wows/replays"),
        Timestamp::from_second(1).unwrap(),
    )
    .await
    .unwrap();

    let rows = MappedRows {
        objective: ObjectiveMatch {
            arena_id: ArenaId::new(500),
            timestamp: Timestamp::from_second(9000).unwrap(),
            map: "Ocean".into(),
            game_mode: "Domination".into(),
            game_type: "pvp".into(),
            match_group: "pvp".into(),
            version_build: Some(1),
        },
        vehicles: vec![IndexedVehicleRow {
            arena_id: ArenaId::new(500),
            account_id: AccountId(7),
            player_name: "Me".into(),
            clan: String::new(),
            realm: None,
            ship_id: GameParamId::from(999u64),
            ship_index: "PJSD018".into(),
            ship_name: "Harugumo".into(),
            nation: "japan".into(),
            species: "Destroyer".into(),
            tier: 10,
            relation: VehicleRelation::SelfPlayer,
            division_id: None,
            survived: Some(true),
            damage: Some(1),
            kills: Some(0),
            spotting: Some(0),
            potential: Some(0),
            received: Some(0),
            pr: None,
            is_test_ship: false,
        }],
        record: ReplayRecord {
            arena_id: ArenaId::new(500),
            source_id: src,
            replay_path: PathBuf::from("x.wowsreplay"),
            file_mtime: Some(1),
            outcome: MatchOutcome::Win,
            self_account_id: Some(AccountId(7)),
            self_ship_id: Some(GameParamId::from(999u64)),
            self_survived: Some(true),
            self_damage: Some(1),
            self_kills: Some(0),
            self_pr: None,
            results_available: true,
            indexed_at: Timestamp::from_second(9001).unwrap(),
        },
    };

    write_index(&pool, &rows).await.unwrap();

    let hits = query::search_matches(&pool, &MatchFilter::default()).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].arena_id, ArenaId::new(500));
    assert_eq!(hits[0].self_ship_id, Some(GameParamId::from(999u64)));
}
