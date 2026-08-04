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
        game_mode_id: None,
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
        disconnected: Some(false),
        is_stream_sniper: None,
        sniper_twitch_login: None,
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
    // arena 100: self Harugumo (id 999, damage 50k), enemy Yamato (id 111, account
    // 501, damage 90k), bot (account 0, damage 1, survived=false).
    // arena 200: self Harugumo (damage 30k), enemy Shimakaze (account 777, damage 20k).
    // Damage is distinct per row so subject-scoped stat predicates (self/any/specific
    // player) are provably non-vacuous and arena-scoped.
    for (arena, acct, ship_id, idx, name, species, rel, damage) in [
        (100i64, 7i64, 999u64, "PJSD018", "Harugumo", "Destroyer", VehicleRelation::SelfPlayer, 50_000u64),
        (100, 501, 111, "PJSB018", "Yamato", "Battleship", VehicleRelation::Enemy, 90_000),
        (200, 7, 999, "PJSD018", "Harugumo", "Destroyer", VehicleRelation::SelfPlayer, 30_000),
        (200, 777, 222, "PJSD718", "Shimakaze", "Destroyer", VehicleRelation::Enemy, 20_000),
        (100, 0, 333, "PABOT", "Bot", "Cruiser", VehicleRelation::Enemy, 1),
    ] {
        // Yamato (arena 100, account 501) gets a distinguishing pr; the bot
        // (arena 100, account 0) gets survived=false, so the two arenas differ
        // on both fields and the any-player predicates are provably non-vacuous.
        let pr = if acct == 501 { Some(1000.0) } else { None };
        let survived = if acct == 0 { Some(false) } else { Some(true) };
        // Self row in arena 100 is explicitly connected; Yamato (account 501, arena
        // 100) explicitly disconnected. Arena 200 rows stay NULL (unknown), so
        // `Is false` on the self row only matches arena 100, never a NULL row.
        let disconnected = if arena == 100 && acct == 7 {
            Some(false)
        } else if arena == 100 && acct == 501 {
            Some(true)
        } else {
            None
        };
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
            survived,
            damage: Some(damage),
            kills: Some(0),
            spotting: Some(0),
            potential: Some(0),
            received: Some(0),
            pr,
            is_test_ship: false,
            disconnected,
            is_stream_sniper: None,
            sniper_twitch_login: None,
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

/// The query bar offers ship values for `enemy.ship:` as readily as for
/// `self.ship:`, so the lookup behind it must span the whole roster. Yamato was
/// only ever an enemy in the seed, which is what separates this from
/// `search_self_ships`.
#[tokio::test]
async fn ship_search_spans_the_whole_roster_not_just_ships_the_user_played() {
    let pool = mem_pool().await;
    seed_two_matches(&pool).await;
    seed_rosters(&pool).await;

    let enemy_only = query::search_ships(&pool, "yamato", 50).await.unwrap();
    assert_eq!(enemy_only.iter().map(|s| s.ship_id.raw()).collect::<Vec<_>>(), vec![111]);
    assert_eq!(enemy_only[0].ship_name, "Yamato");
    assert!(
        query::search_self_ships(&pool, "yamato", 50).await.unwrap().is_empty(),
        "the self-scoped search is what this one has to differ from"
    );

    // Harugumo is one ship played across two arenas, so it must come back once
    // with a count of two rather than once per roster row.
    let played = query::search_ships(&pool, "haru", 50).await.unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].match_count, 2);

    assert_eq!(query::search_ships(&pool, "", 1).await.unwrap().len(), 1, "the limit is respected");
}

/// The `map:` dropdown reads from here, so the names must be the ones the index
/// holds and each must appear once however many matches used it.
#[tokio::test]
async fn distinct_maps_lists_each_map_once_most_played_first() {
    let pool = mem_pool().await;
    let src = seed_two_matches(&pool).await;

    // A third match on Ocean, so Ocean outweighs Trap and the ordering claim is
    // about play counts rather than about insertion order.
    let mut m3 = sample_match(300);
    m3.map = "Ocean".into();
    m3.timestamp = Timestamp::from_second(3000).unwrap();
    query::upsert_match(&pool, &m3).await.unwrap();
    query::upsert_record(&pool, &sample_record(300, src, "c.wowsreplay")).await.unwrap();

    let maps = query::distinct_maps(&pool, 50).await.unwrap();
    assert_eq!(maps, vec!["Ocean".to_string(), "Trap".to_string()]);
    assert_eq!(query::distinct_maps(&pool, 1).await.unwrap(), vec!["Ocean".to_string()]);
}

#[tokio::test]
async fn player_name_resolves_seeded_account_and_none_for_unknown() {
    let pool = mem_pool().await;
    seed_two_matches(&pool).await;
    seed_rosters(&pool).await;

    let name = query::player_name(&pool, AccountId(501)).await.unwrap();
    assert_eq!(name, Some("p501".to_string()));

    let unknown = query::player_name(&pool, AccountId(999_999)).await.unwrap();
    assert_eq!(unknown, None);
}

#[tokio::test]
async fn ship_name_resolves_seeded_ship_and_none_for_unknown() {
    let pool = mem_pool().await;
    seed_two_matches(&pool).await;
    seed_rosters(&pool).await;

    let name = query::ship_name(&pool, GameParamId::from(111u64)).await.unwrap();
    assert_eq!(name, Some("Yamato".to_string()));

    let unknown = query::ship_name(&pool, GameParamId::from(987_654u64)).await.unwrap();
    assert_eq!(unknown, None);
}

#[tokio::test]
async fn twitch_observations_round_trip_window_and_dedup() {
    let pool = mem_pool().await;

    query::record_twitch_observations(
        &pool,
        &[("streamer_a".to_string(), 1000), ("streamer_b".to_string(), 1500), ("streamer_a".to_string(), 2000)],
    )
    .await
    .unwrap();
    // Duplicate (login, seen_at) pair: must dedup to one row, not error.
    query::record_twitch_observations(&pool, &[("streamer_a".to_string(), 1000)]).await.unwrap();

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM twitch_observation").fetch_one(&pool).await.unwrap();
    assert_eq!(count, 3, "duplicate (login, seen_at) must not insert a second row");

    // Window [900, 1600] catches streamer_a@1000 and streamer_b@1500, not streamer_a@2000.
    let mut window = query::observations_in_window(&pool, 900, 1600).await.unwrap();
    window.sort();
    assert_eq!(window, vec![("streamer_a".to_string(), 1000), ("streamer_b".to_string(), 1500)]);

    // Out-of-window bounds exclude everything.
    let none = query::observations_in_window(&pool, 5000, 6000).await.unwrap();
    assert!(none.is_empty());

    // Prune rows older than 1600: removes streamer_a@1000 and streamer_b@1500, keeps streamer_a@2000.
    let pruned = query::prune_twitch_observations(&pool, 1600).await.unwrap();
    assert_eq!(pruned, 2);
    let remaining = query::observations_in_window(&pool, 0, 10_000).await.unwrap();
    assert_eq!(remaining, vec![("streamer_a".to_string(), 2000)]);
}

/// Two matches for account 501: arena 100 as RAIN, then the later arena 101 as
/// WOLF. WOLF is therefore the account's latest clan and arena 100 the only
/// encounter whose clan differs from it.
async fn seeded_pool() -> sqlx::SqlitePool {
    let pool = mem_pool().await;

    let clan_vehicle = |arena: i64, clan: &str| IndexedVehicleRow {
        arena_id: ArenaId::new(arena),
        account_id: AccountId(501),
        player_name: "Wanderer".into(),
        clan: clan.into(),
        realm: Some("na".into()),
        ship_id: GameParamId::from(111u64),
        ship_index: "PJSB018".into(),
        ship_name: "Yamato".into(),
        nation: "japan".into(),
        species: "Battleship".into(),
        tier: 10,
        relation: VehicleRelation::Enemy,
        division_id: None,
        survived: Some(true),
        damage: Some(50_000),
        kills: Some(1),
        spotting: Some(0),
        potential: Some(0),
        received: Some(0),
        pr: Some(1200.0),
        is_test_ship: false,
        disconnected: None,
        is_stream_sniper: None,
        sniper_twitch_login: None,
    };

    for (arena, second, clan) in [(100, 1_700_000_100, "RAIN"), (101, 1_700_000_200, "WOLF")] {
        let objective = ObjectiveMatch {
            arena_id: ArenaId::new(arena),
            timestamp: Timestamp::from_second(second).unwrap(),
            map: "Ocean".into(),
            game_mode: "Domination".into(),
            game_mode_id: None,
            game_type: "pvp".into(),
            match_group: "pvp".into(),
            version_build: Some(1234),
        };
        query::upsert_match(&pool, &objective).await.unwrap();
        query::upsert_vehicles(&pool, &[clan_vehicle(arena, clan)]).await.unwrap();
    }

    pool
}

#[tokio::test]
async fn clan_history_corrections_returns_only_rows_that_differ_from_the_latest_clan() {
    use wows_toolkit_config::index::rows::MatchFilter;

    let pool = seeded_pool().await;
    // Account 501 played arena 100 as RAIN and the later arena 101 as WOLF.
    // Only the RAIN row is a correction, because WOLF is the latest clan.
    let corrections = query::clan_history_corrections(&pool, &MatchFilter::default()).await.unwrap();

    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0].account_id, AccountId(501));
    assert_eq!(corrections[0].arena_id, ArenaId::new(100));
    assert_eq!(corrections[0].clan, "RAIN");
    assert_eq!(corrections[0].timestamp, Timestamp::from_second(1_700_000_100).unwrap());
}

/// One match seen from account 7's perspective: 9 shares the self player's
/// division, 8 is in a division of its own, and 501 is a solo enemy.
async fn division_pool() -> sqlx::SqlitePool {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    let vehicle = |account: i64, division: Option<i64>| IndexedVehicleRow {
        arena_id: ArenaId::new(100),
        account_id: AccountId(account),
        player_name: format!("Player{account}"),
        clan: "CLAN".into(),
        realm: Some("na".into()),
        ship_id: GameParamId::from(111u64),
        ship_index: "PJSB018".into(),
        ship_name: "Yamato".into(),
        nation: "japan".into(),
        species: "Battleship".into(),
        tier: 10,
        relation: VehicleRelation::Enemy,
        division_id: division,
        survived: Some(true),
        damage: Some(50_000),
        kills: Some(1),
        spotting: Some(0),
        potential: Some(0),
        received: Some(0),
        pr: Some(1200.0),
        is_test_ship: false,
        disconnected: None,
        is_stream_sniper: None,
        sniper_twitch_login: None,
    };

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_vehicles(
        &pool,
        &[
            IndexedVehicleRow { relation: VehicleRelation::SelfPlayer, ..vehicle(7, Some(3)) },
            vehicle(9, Some(3)),
            vehicle(8, Some(4)),
            vehicle(501, None),
        ],
    )
    .await
    .unwrap();
    // `sample_record` records account 7 as the self player.
    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay")).await.unwrap();

    pool
}

#[tokio::test]
async fn division_mate_encounters_returns_only_accounts_that_shared_the_self_division() {
    use wows_toolkit_config::index::rows::MatchFilter;

    let pool = division_pool().await;
    let mates = query::division_mate_encounters(&pool, &MatchFilter::default()).await.unwrap();

    let accounts: Vec<AccountId> = mates.iter().map(|encounter| encounter.account_id).collect();
    assert!(accounts.contains(&AccountId(9)), "an account in the self player's division is a mate");
    assert!(!accounts.contains(&AccountId(8)), "another division's id must not pair with the self player's");
    assert!(!accounts.contains(&AccountId(501)), "a player in no division is never a mate");
    assert!(!accounts.contains(&AccountId(7)), "the self player is not their own division mate");
    assert_eq!(mates.len(), 1);

    // Both keys a consumer dedups encounters under have to come back, or it can
    // only mark half of what it counts.
    assert_eq!(mates[0].arena_id, ArenaId::new(100));
    assert_eq!(mates[0].timestamp, sample_match(100).timestamp);
}

/// The same account can be a division mate in one match and an opponent in the
/// next. Only the match you divisioned in comes back, which is what lets a
/// consumer hide that battle and keep the other.
#[tokio::test]
async fn division_mate_encounters_covers_the_division_match_alone() {
    use wows_toolkit_config::index::rows::MatchFilter;

    let pool = division_pool().await;
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), Timestamp::from_second(1).unwrap())
        .await
        .unwrap();

    // A second match, where account 9 is a solo opponent instead.
    let later = ObjectiveMatch { timestamp: Timestamp::from_second(1_700_009_999).unwrap(), ..sample_match(200) };
    query::upsert_match(&pool, &later).await.unwrap();

    let vehicle = |account: i64, relation: VehicleRelation| IndexedVehicleRow {
        arena_id: ArenaId::new(200),
        account_id: AccountId(account),
        player_name: format!("Player{account}"),
        clan: "CLAN".into(),
        realm: Some("na".into()),
        ship_id: GameParamId::from(111u64),
        ship_index: "PJSB018".into(),
        ship_name: "Yamato".into(),
        nation: "japan".into(),
        species: "Battleship".into(),
        tier: 10,
        relation,
        division_id: None,
        survived: Some(true),
        damage: Some(50_000),
        kills: Some(1),
        spotting: Some(0),
        potential: Some(0),
        received: Some(0),
        pr: Some(1200.0),
        is_test_ship: false,
        disconnected: None,
        is_stream_sniper: None,
        sniper_twitch_login: None,
    };
    query::upsert_vehicles(&pool, &[vehicle(7, VehicleRelation::SelfPlayer), vehicle(9, VehicleRelation::Enemy)])
        .await
        .unwrap();
    query::upsert_record(&pool, &sample_record(200, src, "b.wowsreplay")).await.unwrap();

    let mates = query::division_mate_encounters(&pool, &MatchFilter::default()).await.unwrap();

    assert_eq!(mates.len(), 1, "the match they fought you in is not a division encounter");
    assert_eq!(mates[0].account_id, AccountId(9));
    assert_eq!(mates[0].arena_id, ArenaId::new(100), "only the arena they divisioned with you in");
    assert_eq!(mates[0].timestamp, sample_match(100).timestamp);
}

/// A player who was never in a division has a NULL `division_id`, and NULL never
/// equals NULL in SQL. Pinning that: if the self player's own division were
/// NULL, every other soloist would otherwise pair with them.
#[tokio::test]
async fn division_mate_encounters_is_empty_when_the_self_player_had_no_division() {
    use wows_toolkit_config::index::rows::MatchFilter;

    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    let vehicle = |account: i64, relation: VehicleRelation| IndexedVehicleRow {
        arena_id: ArenaId::new(100),
        account_id: AccountId(account),
        player_name: format!("Player{account}"),
        clan: "CLAN".into(),
        realm: Some("na".into()),
        ship_id: GameParamId::from(111u64),
        ship_index: "PJSB018".into(),
        ship_name: "Yamato".into(),
        nation: "japan".into(),
        species: "Battleship".into(),
        tier: 10,
        relation,
        division_id: None,
        survived: Some(true),
        damage: Some(50_000),
        kills: Some(1),
        spotting: Some(0),
        potential: Some(0),
        received: Some(0),
        pr: Some(1200.0),
        is_test_ship: false,
        disconnected: None,
        is_stream_sniper: None,
        sniper_twitch_login: None,
    };

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_vehicles(&pool, &[vehicle(7, VehicleRelation::SelfPlayer), vehicle(501, VehicleRelation::Enemy)])
        .await
        .unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay")).await.unwrap();

    let mates = query::division_mate_encounters(&pool, &MatchFilter::default()).await.unwrap();
    assert!(mates.is_empty(), "a solo self player has no division mates");
}
