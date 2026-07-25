use std::path::Path;
use std::path::PathBuf;

use jiff::Timestamp;
use sqlx::sqlite::SqlitePoolOptions;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::IndexedVehicleRow;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::SourceKind;
use wows_toolkit_config::index::rows::VehicleRelation;

async fn mem_pool() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn ensure_default_source_is_idempotent() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let a = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    let b = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    assert_eq!(a, b, "second call must return the same source id");

    let sources = query::list_sources(&pool).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].kind, SourceKind::Live);
    assert_eq!(sources[0].id, a);
}

fn sample_match(arena: i64) -> ObjectiveMatch {
    ObjectiveMatch {
        arena_id: ArenaId::new(arena),
        timestamp: Timestamp::from_second(1_700_000_000).unwrap(),
        map: "Ocean".into(),
        game_mode: "Domination".into(),
        game_type: "pvp".into(),
        match_group: "pvp".into(),
        version_build: Some(1234),
    }
}

fn sample_record(arena: i64, source: wows_toolkit_config::index::rows::SourceId, path: &str) -> ReplayRecord {
    ReplayRecord {
        arena_id: ArenaId::new(arena),
        source_id: source,
        replay_path: PathBuf::from(path),
        file_mtime: Some(42),
        outcome: MatchOutcome::Win,
        self_account_id: Some(AccountId(7)),
        self_ship_id: Some(GameParamId::from(999u64)),
        self_survived: Some(true),
        self_damage: Some(123_456),
        self_kills: Some(2),
        self_pr: Some(1500.0),
        results_available: true,
        indexed_at: Timestamp::from_second(1_700_000_100).unwrap(),
    }
}

#[tokio::test]
#[allow(clippy::cloned_ref_to_slice_refs)]
async fn upserts_are_idempotent_and_ledger_tracks_arena() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap(); // idempotent

    let veh = IndexedVehicleRow {
        arena_id: ArenaId::new(100),
        account_id: AccountId(7),
        player_name: "Me".into(),
        clan: "CLAN".into(),
        realm: Some("na".into()),
        ship_id: GameParamId::from(999u64),
        ship_index: "PJSD018".into(),
        ship_name: "Harugumo".into(),
        nation: "japan".into(),
        species: "Destroyer".into(),
        tier: 10,
        relation: VehicleRelation::SelfPlayer,
        division_id: None,
        survived: Some(true),
        damage: Some(123_456),
        kills: Some(2),
        spotting: Some(50_000),
        potential: Some(1_000_000),
        received: Some(10_000),
        pr: Some(1500.0),
        is_test_ship: false,
    };
    query::upsert_vehicles(&pool, &[veh.clone()]).await.unwrap();
    query::upsert_vehicles(&pool, &[veh]).await.unwrap(); // idempotent

    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay")).await.unwrap(); // idempotent

    let ledger = query::arena_ids_in_source(&pool, src).await.unwrap();
    assert_eq!(ledger.len(), 1);
    assert!(ledger.contains(&ArenaId::new(100)));

    // Exactly one row survived each idempotent upsert.
    let (matches,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM indexed_match").fetch_one(&pool).await.unwrap();
    let (vehicles,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM indexed_vehicle").fetch_one(&pool).await.unwrap();
    let (records,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM replay_record").fetch_one(&pool).await.unwrap();
    assert_eq!((matches, vehicles, records), (1, 1, 1));
}

async fn seed_two_matches(pool: &sqlx::SqlitePool) -> wows_toolkit_config::index::rows::SourceId {
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(pool, Path::new("C:/wows/replays"), now).await.unwrap();

    // arena 100: win, Ocean, ts 1000, self_damage 120k
    let mut m1 = sample_match(100);
    m1.map = "Ocean".into();
    m1.timestamp = Timestamp::from_second(1000).unwrap();
    query::upsert_match(pool, &m1).await.unwrap();
    let mut r1 = sample_record(100, src, "a.wowsreplay");
    r1.outcome = MatchOutcome::Win;
    r1.self_damage = Some(120_000);
    r1.self_survived = Some(true);
    query::upsert_record(pool, &r1).await.unwrap();

    // arena 200: loss, Trap, ts 2000, self_damage 40k
    let mut m2 = sample_match(200);
    m2.map = "Trap".into();
    m2.timestamp = Timestamp::from_second(2000).unwrap();
    query::upsert_match(pool, &m2).await.unwrap();
    let mut r2 = sample_record(200, src, "b.wowsreplay");
    r2.outcome = MatchOutcome::Loss;
    r2.self_damage = Some(40_000);
    r2.self_survived = Some(false);
    query::upsert_record(pool, &r2).await.unwrap();

    src
}

#[tokio::test]
async fn search_matches_applies_match_level_predicates() {
    use wows_toolkit_config::index::rows::MatchFilter;
    let pool = mem_pool().await;
    seed_two_matches(&pool).await;

    // No filter: both, newest first.
    let all = query::search_matches(&pool, &MatchFilter::default()).await.unwrap();
    assert_eq!(all.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![200, 100]);

    // Outcome = loss.
    let losses = query::search_matches(&pool, &MatchFilter { outcome: Some(MatchOutcome::Loss), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(losses.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![200]);

    // Map = Ocean.
    let ocean =
        query::search_matches(&pool, &MatchFilter { map: Some("Ocean".into()), ..Default::default() }).await.unwrap();
    assert_eq!(ocean.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![100]);

    // self_damage >= 100k.
    let big = query::search_matches(&pool, &MatchFilter { self_damage_min: Some(100_000), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(big.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![100]);

    // survived = false.
    let died =
        query::search_matches(&pool, &MatchFilter { self_survived: Some(false), ..Default::default() }).await.unwrap();
    assert_eq!(died.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![200]);

    // recent capped to 1 returns newest.
    let recent = query::recent_matches(&pool, &MatchFilter::default(), 1).await.unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].arena_id.raw(), 200);
    assert_eq!(recent[0].outcome, MatchOutcome::Loss);
    assert_eq!(recent[0].self_damage, Some(40_000));
}
