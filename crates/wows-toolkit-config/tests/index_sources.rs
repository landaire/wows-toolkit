use std::path::Path;
use std::path::PathBuf;

use jiff::Timestamp;
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_config::index::rows::SourceKind;

async fn mem_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
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

fn sample_record(arena: i64, source: SourceId, path: &str) -> ReplayRecord {
    ReplayRecord {
        arena_id: ArenaId::new(arena),
        source_id: source,
        replay_path: PathBuf::from(path),
        file_mtime: Some(42),
        outcome: MatchOutcome::Win,
        self_account_id: Some(AccountId::from(7)),
        self_ship_id: Some(GameParamId::from(999u64)),
        self_survived: Some(true),
        self_damage: Some(123_456),
        self_kills: Some(2),
        self_pr: Some(1500.0),
        results_available: true,
        indexed_at: Timestamp::from_second(1_700_000_100).unwrap(),
    }
}

/// Like [`sample_record`], but with the fields the migration's collision
/// preference actually compares (`results_available`, `indexed_at`) and a
/// distinguishing `self_damage` so a test can tell which of two colliding
/// rows won.
fn record_with_results(
    arena: i64,
    source: SourceId,
    path: &str,
    results_available: bool,
    self_damage: u64,
    indexed_at: i64,
) -> ReplayRecord {
    ReplayRecord {
        arena_id: ArenaId::new(arena),
        source_id: source,
        replay_path: PathBuf::from(path),
        file_mtime: Some(42),
        outcome: MatchOutcome::Win,
        self_account_id: Some(AccountId::from(7)),
        self_ship_id: Some(GameParamId::from(999u64)),
        self_survived: Some(true),
        self_damage: Some(self_damage),
        self_kills: Some(2),
        self_pr: Some(1500.0),
        results_available,
        indexed_at: Timestamp::from_second(indexed_at).unwrap(),
    }
}

/// Rebuilds the pre-008 shape inside an already-migrated `mem_pool()` (its
/// migration 008 indexes are dropped) and creates a second `live` source at a
/// higher id than the default one, reproducing the duplicate-live-source
/// condition a racing first launch could leave behind. Returns
/// `(survivor, doomed)`.
async fn two_live_sources(pool: &SqlitePool, now: Timestamp) -> (SourceId, SourceId) {
    sqlx::query("DROP INDEX idx_source_single_live").execute(pool).await.unwrap();
    sqlx::query("DROP INDEX idx_source_root_path").execute(pool).await.unwrap();

    let survivor = query::ensure_default_source(pool, Path::new("C:/wows/replays"), now).await.unwrap();
    let doomed_row: (i64,) = sqlx::query_as(
        "INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4) RETURNING source_id",
    )
    .bind("Live replays (dup)")
    .bind(SourceKind::Live.as_db_str())
    .bind("D:/other/replays")
    .bind(now.as_second())
    .fetch_one(pool)
    .await
    .unwrap();
    let doomed = SourceId(doomed_row.0);
    assert!(doomed.0 > survivor.0, "the doomed source must have the higher id for this scenario to be meaningful");
    (survivor, doomed)
}

#[tokio::test]
async fn a_second_live_source_cannot_be_inserted() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    // The uniqueness constraint is what makes the two-writer race harmless.
    let second: Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> =
        sqlx::query("INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4)")
            .bind("Live replays")
            .bind(SourceKind::Live.as_db_str())
            .bind("D:/other/replays")
            .bind(now.as_second())
            .execute(&pool)
            .await;

    assert!(second.is_err(), "a second live source must violate the unique index");
}

#[tokio::test]
async fn two_sources_cannot_share_a_root_path() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    let dup: Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> =
        sqlx::query("INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4)")
            .bind("Imported")
            .bind(SourceKind::ImportedDir.as_db_str())
            .bind("C:/wows/replays")
            .bind(now.as_second())
            .execute(&pool)
            .await;

    assert!(dup.is_err(), "root_path must be unique across sources");
}

#[tokio::test]
async fn several_sources_may_have_a_null_root_path() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    for name in ["a", "b"] {
        sqlx::query("INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, NULL, ?3)")
            .bind(name)
            .bind(SourceKind::AdHoc.as_db_str())
            .bind(now.as_second())
            .execute(&pool)
            .await
            .expect("NULL root_path must not participate in the unique index");
    }
}

#[tokio::test]
async fn ensure_default_source_id_round_trips_through_live_source_id() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let created = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    let read = query::live_source_id(&pool).await.unwrap();
    assert_eq!(read, Some(created));
}

#[tokio::test]
async fn single_live_source_read_write_round_trips_after_migration_008() {
    // With only one live source the dedupe statements have nothing to do;
    // this only proves migration 008 does not break the ordinary insert/read
    // path. It does not exercise the dedupe logic itself -- see
    // migration_dedupes_two_live_sources_repointing_non_colliding_records and
    // its siblings for that.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay")).await.unwrap();

    let paths = query::record_paths_in_source(&pool, src).await.unwrap();
    assert!(paths.contains("a.wowsreplay"));
}

#[tokio::test]
async fn migration_dedupes_two_live_sources_repointing_non_colliding_records() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (survivor, doomed) = two_live_sources(&pool, now).await;

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(200)).await.unwrap();
    query::upsert_match(&pool, &sample_match(300)).await.unwrap();

    // Already on the survivor; the migration must leave it alone.
    query::upsert_record(&pool, &sample_record(100, survivor, "a.wowsreplay")).await.unwrap();
    // Only on the doomed source; the migration must repoint it onto the survivor.
    query::upsert_record(&pool, &sample_record(200, doomed, "b.wowsreplay")).await.unwrap();
    // Both sources independently indexed the same physical file (the two-writer
    // race the migration is repairing): same arena and path, one row per source.
    // The doomed copy must be dropped rather than repointed, because repointing
    // it would collide with the survivor's own row at (source_id, replay_path).
    query::upsert_record(&pool, &sample_record(300, survivor, "shared.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(300, doomed, "shared.wowsreplay")).await.unwrap();

    // Run the real migration body, not a paraphrase, so this test cannot drift
    // from what actually ships.
    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let live_sources: Vec<(i64,)> = sqlx::query_as("SELECT source_id FROM index_source WHERE kind = ?1")
        .bind(SourceKind::Live.as_db_str())
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(live_sources, vec![(survivor.0,)], "exactly one live source must remain, at the lowest id");

    let paths = query::record_paths_in_source(&pool, survivor).await.unwrap();
    assert!(paths.contains("a.wowsreplay"), "the survivor's own record must be untouched");
    assert!(paths.contains("b.wowsreplay"), "the non-colliding doomed record must be repointed onto the survivor");
    assert!(paths.contains("shared.wowsreplay"), "the survivor's copy of the colliding record must remain");

    let shared_rows: Vec<(i64, i64)> =
        sqlx::query_as("SELECT source_id, arena_id FROM replay_record WHERE replay_path = 'shared.wowsreplay'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        shared_rows,
        vec![(survivor.0, 300)],
        "the doomed source's colliding copy must be dropped, not repointed"
    );

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM replay_record").fetch_one(&pool).await.unwrap();
    assert_eq!(total.0, 3, "one duplicate record must be dropped; the other three must remain");
}

/// One row's `self_damage` after the migration, for the single-path fixtures
/// the collision-preference tests use.
async fn self_damage_at(pool: &SqlitePool, path: &str) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT self_damage FROM replay_record WHERE replay_path = ?1")
        .bind(path)
        .fetch_one(pool)
        .await
        .unwrap();
    row.0
}

#[tokio::test]
async fn migration_dedupe_prefers_the_doomed_row_when_it_has_results_and_the_survivor_does_not() {
    // The two live sources are populated by separate threads at different
    // times, so a record present under both can disagree: one thread indexed
    // it before WG post-battle results were written, the other after.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (survivor, doomed) = two_live_sources(&pool, now).await;

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &record_with_results(100, survivor, "x.wowsreplay", false, 111, 1_700_000_000))
        .await
        .unwrap();
    query::upsert_record(&pool, &record_with_results(100, doomed, "x.wowsreplay", true, 999, 1_700_000_050))
        .await
        .unwrap();

    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let remaining: Vec<(i64,)> =
        sqlx::query_as("SELECT source_id FROM replay_record WHERE replay_path = 'x.wowsreplay'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, vec![(survivor.0,)], "exactly one row must remain, repointed onto the survivor");
    assert_eq!(
        self_damage_at(&pool, "x.wowsreplay").await,
        999,
        "the results-bearing doomed row must win over the results-absent survivor row"
    );
}

#[tokio::test]
async fn migration_dedupe_keeps_the_survivor_row_when_it_already_has_results() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (survivor, doomed) = two_live_sources(&pool, now).await;

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &record_with_results(100, survivor, "x.wowsreplay", true, 999, 1_700_000_050))
        .await
        .unwrap();
    query::upsert_record(&pool, &record_with_results(100, doomed, "x.wowsreplay", false, 111, 1_700_000_000))
        .await
        .unwrap();

    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let remaining: Vec<(i64,)> =
        sqlx::query_as("SELECT source_id FROM replay_record WHERE replay_path = 'x.wowsreplay'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, vec![(survivor.0,)], "exactly one row must remain, already on the survivor");
    assert_eq!(
        self_damage_at(&pool, "x.wowsreplay").await,
        999,
        "the survivor's already-results-bearing row must be kept over the results-absent doomed row"
    );
}

#[tokio::test]
async fn migration_dedupe_prefers_results_over_recency() {
    // The other two preference tests covary results_available and indexed_at,
    // so neither pins the ordering between them: a predicate that compared
    // only indexed_at would pass both. Here the fields disagree -- the
    // survivor is more recent but results-absent, the doomed row is older but
    // results-bearing -- so only "results beats recency" picks the doomed row.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (survivor, doomed) = two_live_sources(&pool, now).await;

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &record_with_results(100, survivor, "x.wowsreplay", false, 111, 1_700_000_100))
        .await
        .unwrap();
    query::upsert_record(&pool, &record_with_results(100, doomed, "x.wowsreplay", true, 999, 1_700_000_000))
        .await
        .unwrap();

    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let remaining: Vec<(i64,)> =
        sqlx::query_as("SELECT source_id FROM replay_record WHERE replay_path = 'x.wowsreplay'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, vec![(survivor.0,)], "exactly one row must remain, repointed onto the survivor");
    assert_eq!(
        self_damage_at(&pool, "x.wowsreplay").await,
        999,
        "the older, results-bearing doomed row must win over the newer, results-absent survivor row"
    );
}

#[tokio::test]
async fn migration_nulls_root_path_for_all_but_the_lowest_id_among_a_group_sharing_one() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    sqlx::query("DROP INDEX idx_source_single_live").execute(&pool).await.unwrap();
    sqlx::query("DROP INDEX idx_source_root_path").execute(&pool).await.unwrap();

    // Three ad-hoc sources sharing one root_path: the non-obvious case is
    // whether the correlated subquery picking MIN(source_id) per root_path
    // still holds across a group larger than two. A fourth, unrelated source
    // with its own root_path proves the migration does not touch rows outside
    // any duplicate group.
    let mut shared_ids = Vec::new();
    for name in ["shared-a", "shared-b", "shared-c"] {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4) RETURNING source_id",
        )
        .bind(name)
        .bind(SourceKind::AdHoc.as_db_str())
        .bind("E:/shared")
        .bind(now.as_second())
        .fetch_one(&pool)
        .await
        .unwrap();
        shared_ids.push(row.0);
    }
    let lowest_shared = *shared_ids.iter().min().unwrap();

    sqlx::query("INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4)")
        .bind("unrelated")
        .bind(SourceKind::AdHoc.as_db_str())
        .bind("E:/other")
        .bind(now.as_second())
        .execute(&pool)
        .await
        .unwrap();

    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let rows: Vec<(i64, Option<String>)> =
        sqlx::query_as("SELECT source_id, root_path FROM index_source ORDER BY source_id")
            .fetch_all(&pool)
            .await
            .unwrap();

    for (id, root_path) in &rows {
        if *id == lowest_shared {
            assert_eq!(root_path.as_deref(), Some("E:/shared"), "the lowest id in the group must keep the root_path");
        } else if shared_ids.contains(id) {
            assert_eq!(root_path.as_deref(), None, "every non-lowest id in the group must have root_path nulled out");
        } else {
            assert_eq!(root_path.as_deref(), Some("E:/other"), "a source outside the group must be untouched");
        }
    }
}

async fn row_count_for_root(pool: &SqlitePool, root: &str) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM index_source WHERE root_path = ?1")
        .bind(root)
        .fetch_one(pool)
        .await
        .unwrap();
    row.0
}

async fn name_for_root(pool: &SqlitePool, root: &str) -> String {
    let row: (String,) =
        sqlx::query_as("SELECT name FROM index_source WHERE root_path = ?1").bind(root).fetch_one(pool).await.unwrap();
    row.0
}

#[tokio::test]
async fn ensure_source_is_idempotent_for_a_repeated_root() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let root = Path::new("D:/dump/replays");
    let first = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, root, now).await.unwrap();
    let second = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, root, now).await.unwrap();
    assert_eq!(first, second, "the same root must resolve to the same source");
}

#[tokio::test]
async fn ensure_source_distinguishes_roots() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let a = query::ensure_source(&pool, "A", SourceKind::ImportedDir, Path::new("D:/a"), now).await.unwrap();
    let b = query::ensure_source(&pool, "B", SourceKind::ImportedDir, Path::new("D:/b"), now).await.unwrap();
    assert_ne!(a, b);
}

#[tokio::test]
async fn ensure_source_returns_the_existing_row_rather_than_erroring_on_the_unique_index() {
    // The insert races another thread; recovering by re-selecting is what makes
    // the two-writer path safe rather than merely constrained.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let root = Path::new("D:/dump/replays");
    let existing = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, root, now).await.unwrap();

    // A differently-named call for the same root must still resolve, not fail.
    let again = query::ensure_source(&pool, "Renamed", SourceKind::ImportedDir, root, now).await.unwrap();
    assert_eq!(existing, again);

    // The differing name proves the second insert really did collide on
    // root_path (a call with identical arguments could resolve to the same id
    // via a check-then-insert that never touches the conflict arm at all).
    // A single surviving row, still named "Dump", proves the conflict was
    // absorbed by ON CONFLICT DO NOTHING and recovered via the re-select,
    // rather than by inserting a second row or upserting over the first.
    assert_eq!(row_count_for_root(&pool, "D:/dump/replays").await, 1, "only one row may exist for the root");
    assert_eq!(
        name_for_root(&pool, "D:/dump/replays").await,
        "Dump",
        "the original row's name must survive untouched; DO NOTHING must not upsert"
    );
}

#[tokio::test]
async fn ensure_default_source_still_yields_the_live_source() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let via_default = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    assert_eq!(query::live_source_id(&pool).await.unwrap(), Some(via_default));
}

#[tokio::test]
async fn ensure_default_source_adopts_an_existing_source_for_the_same_root() {
    // If a directory is already registered as an imported_dir source before any
    // live source exists, ensure_default_source must adopt that existing source
    // rather than fail or create a second row for the same root_path (which the
    // unique index would reject anyway).
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let root = Path::new("C:/wows/replays");
    let imported = query::ensure_source(&pool, "Imported", SourceKind::ImportedDir, root, now).await.unwrap();

    let via_default = query::ensure_default_source(&pool, root, now).await.unwrap();
    assert_eq!(via_default, imported, "ensure_default_source must adopt the pre-existing source for this root");

    // No live source was created: the conflict on root_path prevented the
    // live-kind insert from ever landing a row.
    assert_eq!(
        query::live_source_id(&pool).await.unwrap(),
        None,
        "adopting an existing source must not create a live-kind row"
    );
    assert_eq!(row_count_for_root(&pool, "C:/wows/replays").await, 1, "only one row may exist for the root");
}
