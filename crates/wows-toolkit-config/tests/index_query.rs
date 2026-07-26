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

#[tokio::test]
async fn record_paths_in_source_returns_recorded_paths() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(200)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(200, src, "b.wowsreplay")).await.unwrap();
    // Re-recording the same path is idempotent (keyed on source + path).
    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay")).await.unwrap();

    let paths = query::record_paths_in_source(&pool, src).await.unwrap();
    assert_eq!(paths.len(), 2, "two distinct paths recorded, idempotent on re-record");
    assert!(paths.contains("a.wowsreplay"));
    assert!(paths.contains("b.wowsreplay"));
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

async fn seed_rosters(pool: &sqlx::SqlitePool) {
    // arena 100: self Harugumo (id 999), enemy Yamato (id 111, account 501)
    for (arena, acct, ship_id, idx, name, species, rel) in [
        (100i64, 7i64, 999u64, "PJSD018", "Harugumo", "Destroyer", VehicleRelation::SelfPlayer),
        (100, 501, 111, "PJSB018", "Yamato", "Battleship", VehicleRelation::Enemy),
        (200, 7, 999, "PJSD018", "Harugumo", "Destroyer", VehicleRelation::SelfPlayer),
        (200, 777, 222, "PJSD718", "Shimakaze", "Destroyer", VehicleRelation::Enemy),
        (100, 0, 333, "PABOT", "Bot", "Cruiser", VehicleRelation::Enemy),
    ] {
        let v = IndexedVehicleRow {
            arena_id: ArenaId::new(arena),
            account_id: AccountId(acct),
            player_name: format!("p{acct}"),
            clan: String::new(),
            realm: None,
            ship_id: GameParamId::from(ship_id),
            ship_index: idx.into(),
            ship_name: name.into(),
            nation: "japan".into(),
            species: species.into(),
            tier: 10,
            relation: rel,
            division_id: None,
            survived: Some(true),
            damage: Some(1),
            kills: Some(0),
            spotting: Some(0),
            potential: Some(0),
            received: Some(0),
            pr: None,
            is_test_ship: false,
        };
        query::upsert_vehicles(pool, &[v]).await.unwrap();
    }
}

#[tokio::test]
async fn exists_predicates_and_helpers() {
    use wows_toolkit_config::index::rows::MatchFilter;
    let pool = mem_pool().await;
    seed_two_matches(&pool).await;
    seed_rosters(&pool).await;

    // enemy_ship = Yamato (111) -> only arena 100
    let f = MatchFilter { enemy_ship: Some(GameParamId::from(111u64)), ..Default::default() };
    let hits = query::search_matches(&pool, &f).await.unwrap();
    assert_eq!(hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![100]);

    // player_present = account 777 -> only arena 200
    let f = MatchFilter { player_present: Some(AccountId(777)), ..Default::default() };
    let hits = query::search_matches(&pool, &f).await.unwrap();
    assert_eq!(hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![200]);

    // matches_with_player convenience
    let hits = query::matches_with_player(&pool, AccountId(501), &MatchFilter::default()).await.unwrap();
    assert_eq!(hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![100]);

    // matches_with_ship: Yamato as enemy -> arena 100
    let hits = query::matches_with_ship(
        &pool,
        GameParamId::from(111u64),
        Some(VehicleRelation::Enemy),
        &MatchFilter::default(),
    )
    .await
    .unwrap();
    assert_eq!(hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![100]);
}

#[tokio::test]
async fn self_account_ids_returns_distinct_self_accounts() {
    use wows_toolkit_config::index::rows::MatchFilter;
    let pool = mem_pool().await;
    let src = seed_two_matches(&pool).await;

    // Both seeded records use self_account_id 7 (from sample_record).
    let ids = query::self_account_ids(&pool, &MatchFilter::default()).await.unwrap();
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(&AccountId(7)));

    // Source scoping: an unrelated source id excludes everything.
    let other_src = wows_toolkit_config::index::rows::SourceId(src.0 + 1);
    let scoped =
        query::self_account_ids(&pool, &MatchFilter { source_ids: Some(vec![other_src]), ..Default::default() })
            .await
            .unwrap();
    assert!(scoped.is_empty());

    let scoped = query::self_account_ids(&pool, &MatchFilter { source_ids: Some(vec![src]), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(scoped.len(), 1);
    assert!(scoped.contains(&AccountId(7)));
}

#[tokio::test]
async fn facets_list_players_and_self_ships() {
    use wows_toolkit_config::index::rows::MatchFilter;
    let pool = mem_pool().await;
    seed_two_matches(&pool).await;
    seed_rosters(&pool).await;

    let players = query::distinct_players(&pool, &MatchFilter::default()).await.unwrap();
    // account 7 appears in both arenas; 501 and 777 once each. Bot account 0 excluded.
    let by_id: std::collections::HashMap<i64, i64> =
        players.iter().map(|p| (p.account_id.raw(), p.match_count)).collect();
    assert_eq!(by_id.get(&7), Some(&2));
    assert_eq!(by_id.get(&501), Some(&1));
    assert!(!by_id.contains_key(&0));

    let ships = query::distinct_self_ships(&pool, &MatchFilter::default()).await.unwrap();
    // self played Harugumo (999) in both arenas.
    let haru = ships.iter().find(|s| s.ship_id.raw() == 999).unwrap();
    assert_eq!(haru.match_count, 2);
    assert_eq!(haru.ship_name, "Harugumo");
}

use wows_toolkit_config::index::query_model::Chip;
use wows_toolkit_config::index::query_model::Connector;
use wows_toolkit_config::index::query_model::Field;
use wows_toolkit_config::index::query_model::Group;
use wows_toolkit_config::index::query_model::Op;
use wows_toolkit_config::index::query_model::Query;
use wows_toolkit_config::index::query_model::Value;

fn one(field: Field, op: Op, value: Value) -> Query {
    Query { groups: vec![Group { chips: vec![Chip { field, op, value }] }], connector: Connector::And }
}

#[tokio::test]
async fn search_by_query_predicates_and_groups() {
    let pool = mem_pool().await;
    seed_two_matches(&pool).await;
    seed_rosters(&pool).await;

    // Case-insensitive Contains on map: "oce" -> arena 100 (Ocean).
    let q = one(Field::Map, Op::Contains, Value::Text("oce".into()));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    assert_eq!(hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![100]);

    // Numeric: self_damage >= 100k -> arena 100.
    let q = one(Field::SelfDamage, Op::Ge, Value::Int(100_000));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    assert_eq!(hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![100]);

    // Outcome Is Loss -> arena 200.
    let q = one(Field::Outcome, Op::Is, Value::Outcome(MatchOutcome::Loss));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    assert_eq!(hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![200]);

    // Presence: EnemyShip Yamato(111) present -> arena 100.
    let q = one(Field::EnemyShip, Op::Present, Value::Ship(GameParamId::from(111u64)));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    assert_eq!(hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![100]);

    // AND within a group: Loss AND map contains "tr" -> arena 200 only.
    let q = Query {
        groups: vec![Group {
            chips: vec![
                Chip { field: Field::Outcome, op: Op::Is, value: Value::Outcome(MatchOutcome::Loss) },
                Chip { field: Field::Map, op: Op::Contains, value: Value::Text("tr".into()) },
            ],
        }],
        connector: Connector::And,
    };
    assert_eq!(query::search_by_query(&pool, &q, 500).await.unwrap().len(), 1);

    // OR between groups: (Win) OR (map contains "tr") -> both arenas.
    let q = Query {
        groups: vec![
            Group { chips: vec![Chip { field: Field::Outcome, op: Op::Is, value: Value::Outcome(MatchOutcome::Win) }] },
            Group { chips: vec![Chip { field: Field::Map, op: Op::Contains, value: Value::Text("tr".into()) }] },
        ],
        connector: Connector::Or,
    };
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    assert_eq!(hits.len(), 2);

    // Empty query -> all, capped by limit.
    let none = query::search_by_query(&pool, &Query::default(), 1).await.unwrap();
    assert_eq!(none.len(), 1);
}

#[tokio::test]
async fn search_by_query_tier_honors_op() {
    let pool = mem_pool().await;
    seed_two_matches(&pool).await;
    seed_rosters(&pool).await;

    // Both seeded arenas have self tier 10. Tier > 9 matches both.
    let q = one(Field::Tier, Op::Gt, Value::Int(9));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    let mut arenas = hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>();
    arenas.sort();
    assert_eq!(arenas, vec![100, 200], "tier 10 > 9 must match");

    // Tier > 10 matches nothing: proves the op is not silently rewritten to `=`.
    let q = one(Field::Tier, Op::Gt, Value::Int(10));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    assert!(hits.is_empty(), "tier 10 is not > 10; old hardcoded `=` code would have matched both arenas here");

    // Tier = 10 still matches both (Eq still works).
    let q = one(Field::Tier, Op::Eq, Value::Int(10));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    let mut arenas = hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>();
    arenas.sort();
    assert_eq!(arenas, vec![100, 200]);
}

#[tokio::test]
async fn search_by_query_class_honors_op() {
    let pool = mem_pool().await;
    seed_two_matches(&pool).await;
    seed_rosters(&pool).await;

    // Both seeded arenas have self ship Harugumo, a Destroyer.
    let q = one(Field::Class, Op::Is, Value::Class("Destroyer".into()));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    let mut arenas = hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>();
    arenas.sort();
    assert_eq!(arenas, vec![100, 200], "Class Is Destroyer must match both self-Destroyer arenas");

    // IsNot must negate: excludes both arenas since self is always a Destroyer here.
    let q = one(Field::Class, Op::IsNot, Value::Class("Destroyer".into()));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    assert!(
        hits.is_empty(),
        "Class IsNot Destroyer must exclude self-Destroyer arenas; old hardcoded EXISTS code would have matched both"
    );

    // IsNot on a species that is never self: matches everything back.
    let q = one(Field::Class, Op::IsNot, Value::Class("Battleship".into()));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    let mut arenas = hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>();
    arenas.sort();
    assert_eq!(arenas, vec![100, 200]);
}

#[tokio::test]
async fn search_by_query_covers_selfship_allyship_kills_group() {
    use wows_toolkit_config::index::rows::VehicleRelation;

    let pool = mem_pool().await;
    let src = seed_two_matches(&pool).await;
    seed_rosters(&pool).await;

    // Add an ally ship to arena 100 so AllyShip Present has something to find.
    let ally = IndexedVehicleRow {
        arena_id: ArenaId::new(100),
        account_id: AccountId(42),
        player_name: "ally42".into(),
        clan: String::new(),
        realm: None,
        ship_id: GameParamId::from(444u64),
        ship_index: "PJSC018".into(),
        ship_name: "Kuma".into(),
        nation: "japan".into(),
        species: "Cruiser".into(),
        tier: 4,
        relation: VehicleRelation::Ally,
        division_id: None,
        survived: Some(true),
        damage: Some(1),
        kills: Some(0),
        spotting: Some(0),
        potential: Some(0),
        received: Some(0),
        pr: None,
        is_test_ship: false,
    };
    query::upsert_vehicles(&pool, &[ally]).await.unwrap();

    // SelfShip Is Harugumo(999) -> both arenas.
    let q = one(Field::SelfShip, Op::Is, Value::Ship(GameParamId::from(999u64)));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    let mut arenas = hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>();
    arenas.sort();
    assert_eq!(arenas, vec![100, 200]);

    // AllyShip Present (Kuma 444) -> arena 100 only.
    let q = one(Field::AllyShip, Op::Present, Value::Ship(GameParamId::from(444u64)));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    assert_eq!(hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>(), vec![100]);

    // Kills >= 2: seed_two_matches sets self_kills only via sample_record defaults (2),
    // both arenas share the default self_kills of 2.
    let q = one(Field::Kills, Op::Ge, Value::Int(2));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    let mut arenas = hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>();
    arenas.sort();
    assert_eq!(arenas, vec![100, 200]);

    // Group (source) Is the seeded source -> both arenas; a different source id excludes all.
    let q = one(Field::Group, Op::Is, Value::Source(src));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    let mut arenas = hits.iter().map(|h| h.arena_id.raw()).collect::<Vec<_>>();
    arenas.sort();
    assert_eq!(arenas, vec![100, 200]);

    let other_src = wows_toolkit_config::index::rows::SourceId(src.0 + 1);
    let q = one(Field::Group, Op::Is, Value::Source(other_src));
    let hits = query::search_by_query(&pool, &q, 500).await.unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn bounded_search_players_and_ships() {
    let pool = mem_pool().await;
    seed_two_matches(&pool).await;
    seed_rosters(&pool).await;

    // Case-insensitive substring: "P5" matches p501 (name lowercased in DB is "p501").
    let players = query::search_players(&pool, "P5", 50).await.unwrap();
    assert!(players.iter().any(|p| p.account_id.raw() == 501));
    assert!(!players.iter().any(|p| p.account_id.raw() == 0)); // bots excluded

    // limit respected.
    let capped = query::search_players(&pool, "p", 1).await.unwrap();
    assert_eq!(capped.len(), 1);

    // Needle "p" matches every seeded name, including the bot's "p0" (account 0, from
    // seed_rosters). This genuinely exercises the `account_id <> 0` filter: unlike the
    // "P5" search above, the bot's name is NOT already excluded by the LIKE predicate.
    let all = query::search_players(&pool, "p", 50).await.unwrap();
    assert!(all.iter().any(|p| p.account_id.raw() == 7), "real player present");
    assert!(!all.iter().any(|p| p.account_id.raw() == 0), "bot excluded despite matching needle");

    // self-ships: "haru" -> Harugumo.
    let ships = query::search_self_ships(&pool, "haru", 50).await.unwrap();
    assert!(ships.iter().any(|s| s.ship_id.raw() == 999));
}
