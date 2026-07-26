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
            disconnected: None,
            is_stream_sniper: None,
            sniper_twitch_login: None,
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

/// A results-absent (left-early) replay must index with `results_available =
/// false` and NULL server stats, then be upgraded in place -- not duplicated
/// -- once a later pass indexes the same arena with results present. This is
/// the `ON CONFLICT DO UPDATE` upsert behavior Task 12 depends on.
#[tokio::test]
async fn reindexing_with_results_upgrades_a_results_absent_row() {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("../wows-toolkit-config/migrations").run(&pool).await.unwrap();
    let src = query::ensure_default_source(
        &pool,
        std::path::Path::new("C:/wows/replays"),
        Timestamp::from_second(1).unwrap(),
    )
    .await
    .unwrap();

    let objective = ObjectiveMatch {
        arena_id: ArenaId::new(600),
        timestamp: Timestamp::from_second(9000).unwrap(),
        map: "Ocean".into(),
        game_mode: "Domination".into(),
        game_type: "pvp".into(),
        match_group: "pvp".into(),
        version_build: Some(1),
    };

    let pending_rows = MappedRows {
        objective: objective.clone(),
        vehicles: vec![],
        record: ReplayRecord {
            arena_id: ArenaId::new(600),
            source_id: src,
            replay_path: PathBuf::from("left_early.wowsreplay"),
            file_mtime: Some(1),
            outcome: MatchOutcome::Unknown,
            self_account_id: Some(AccountId(7)),
            self_ship_id: Some(GameParamId::from(999u64)),
            self_survived: None,
            self_damage: None,
            self_kills: None,
            self_pr: None,
            results_available: false,
            indexed_at: Timestamp::from_second(9001).unwrap(),
        },
    };
    write_index(&pool, &pending_rows).await.unwrap();

    let hits = query::search_matches(&pool, &MatchFilter::default()).await.unwrap();
    assert_eq!(hits.len(), 1, "results-absent replay must still produce a match row");
    assert!(!hits[0].results_available, "results_available must be false while results are pending");
    assert_eq!(hits[0].self_damage, None, "server stats must be NULL, not a sentinel, while results are pending");
    assert_eq!(hits[0].outcome, MatchOutcome::Unknown);

    // Results land: a re-index of the same replay (same arena_id, same source +
    // path) upserts rather than inserting a second row.
    let complete_rows = MappedRows {
        objective,
        vehicles: vec![],
        record: ReplayRecord {
            arena_id: ArenaId::new(600),
            source_id: src,
            replay_path: PathBuf::from("left_early.wowsreplay"),
            file_mtime: Some(2),
            outcome: MatchOutcome::Win,
            self_account_id: Some(AccountId(7)),
            self_ship_id: Some(GameParamId::from(999u64)),
            self_survived: Some(true),
            self_damage: Some(50000),
            self_kills: Some(2),
            self_pr: Some(1234.5),
            results_available: true,
            indexed_at: Timestamp::from_second(9500).unwrap(),
        },
    };
    write_index(&pool, &complete_rows).await.unwrap();

    let hits = query::search_matches(&pool, &MatchFilter::default()).await.unwrap();
    assert_eq!(hits.len(), 1, "re-indexing must upgrade the existing row, not duplicate it");
    assert!(hits[0].results_available, "results_available must flip to true once results land");
    assert_eq!(hits[0].self_damage, Some(50000));
    assert_eq!(hits[0].outcome, MatchOutcome::Win);
}
