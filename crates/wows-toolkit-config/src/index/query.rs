//! Typed query API for the replay index. Populated in later tasks.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use jiff::Timestamp;
use sqlx::QueryBuilder;
use sqlx::Row;
use sqlx::Sqlite;
use sqlx::SqlitePool;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;

use super::query_ast::MatchExpr;
use super::query_model::Chip;
use super::query_model::Connector;
use super::query_model::Field;
use super::query_model::Op;
use super::query_model::Query;
use super::query_model::Subject;
use super::query_model::Value;
use super::query_sql::CompileCtx;
use super::query_sql::push_match_expr;
use super::rows::ClanCorrection;
use super::rows::DivisionMate;
use super::rows::DivisionMateEncounter;
use super::rows::IndexError;
use super::rows::IndexSource;
use super::rows::IndexedVehicleRow;
use super::rows::MatchFilter;
use super::rows::MatchHit;
use super::rows::MatchOutcome;
use super::rows::ObjectiveMatch;
use super::rows::PlayerFacet;
use super::rows::ReplayRecord;
use super::rows::RowSummary;
use super::rows::ShipFacet;
use super::rows::SourceId;
use super::rows::SourceKind;
use super::rows::VehicleRelation;

/// The live source's id, or `None` when the indexer has not created it yet.
/// Readers must not create the row; that is the indexing path's job. A reader
/// that inserted would race the indexing thread's non-atomic check-then-insert
/// and could leave two `Live` rows behind.
pub async fn live_source_id(pool: &SqlitePool) -> Result<Option<SourceId>, IndexError> {
    let existing: Option<(i64,)> =
        sqlx::query_as("SELECT source_id FROM index_source WHERE kind = ?1 ORDER BY source_id LIMIT 1")
            .bind(SourceKind::Live.as_db_str())
            .fetch_optional(pool)
            .await?;
    Ok(existing.map(|(id,)| SourceId(id)))
}

/// Return the id of the source rooted at `root_path`, creating it if absent.
///
/// `INSERT ... ON CONFLICT DO NOTHING` followed by a re-select, so two threads
/// racing on a fresh database both end up with the same id rather than one
/// failing. The unique indexes from migration 008 are what make the conflict
/// arm reachable. The `ON CONFLICT` is unqualified, so it also absorbs a
/// conflict on `idx_source_single_live` (at most one `Live` source), not only
/// one on `root_path`; for a `Live` insert that lost to that index instead,
/// the row exists at a different `root_path`, so the `root_path` re-select
/// below finds nothing and the `Live`-specific fallback below that resolves it.
///
/// The re-select matches on `root_path` alone, not `(root_path, kind)`, so if
/// a source already owns `root_path` under a different `kind`, this returns
/// that existing source rather than creating a second one for `kind`. This is
/// intentional: a directory has exactly one source, whatever kind it was
/// first registered as. Callers that need a specific kind must check the
/// returned source's kind themselves.
pub async fn ensure_source(
    pool: &SqlitePool,
    name: &str,
    kind: SourceKind,
    root_path: &Path,
    now: Timestamp,
) -> Result<SourceId, IndexError> {
    let root = root_path.to_string_lossy().to_string();

    sqlx::query(
        "INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT DO NOTHING",
    )
    .bind(name)
    .bind(kind.as_db_str())
    .bind(&root)
    .bind(now.as_second())
    .execute(pool)
    .await?;

    let row: Option<(i64,)> = sqlx::query_as("SELECT source_id FROM index_source WHERE root_path = ?1")
        .bind(&root)
        .fetch_optional(pool)
        .await?;

    match row {
        Some((id,)) => Ok(SourceId(id)),
        // Absent here means the insert's conflict was not on root_path. The
        // only other unique index the insert can hit is idx_source_single_live,
        // which only a Live insert can violate, so only Live gets a fallback.
        None if kind == SourceKind::Live => {
            live_source_id(pool).await?.ok_or(IndexError::SourceCreationFailed { root_path: root_path.to_path_buf() })
        }
        None => Err(IndexError::SourceCreationFailed { root_path: root_path.to_path_buf() }),
    }
}

/// Return the id of the source that owns `root_path`, creating a `Live` source
/// there if none exists yet. When `root_path` is already owned by a source of
/// some other kind, [`ensure_source`]'s adoption returns that source's id, so
/// the result may not be a `Live` source. Only the indexing path may call
/// this; readers use [`live_source_id`].
pub async fn ensure_default_source(
    pool: &SqlitePool,
    root_path: &Path,
    now: Timestamp,
) -> Result<SourceId, IndexError> {
    if let Some(id) = live_source_id(pool).await? {
        return Ok(id);
    }
    ensure_source(pool, "Live replays", SourceKind::Live, root_path, now).await
}

/// List every group, newest first.
pub async fn list_sources(pool: &SqlitePool) -> Result<Vec<IndexSource>, IndexError> {
    let rows: Vec<(i64, String, String, Option<String>)> =
        sqlx::query_as("SELECT source_id, name, kind, root_path FROM index_source ORDER BY added_at DESC")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, kind, root)| IndexSource {
            id: SourceId(id),
            name,
            kind: SourceKind::from_db_str(&kind).unwrap_or(SourceKind::AdHoc),
            root_path: root.map(std::path::PathBuf::from),
        })
        .collect())
}

/// Point `source` at a new root, rewriting its records' path prefixes. Returns
/// how many records were repointed.
///
/// A record's path counts as "under `old_root`" only when the character right
/// after the matched prefix is a path separator (or the path ends there), so
/// a sibling directory that merely shares a literal prefix -- `D:/oldarchive`
/// versus `D:/old` -- is left alone. `old_root` and `new_root` are normalised
/// by trimming a trailing `/` or `\` before matching, so a caller passing
/// either form gets the same result. The prefix match is case-sensitive, so
/// on a case-insensitive filesystem a root differing only in case from the
/// stored `replay_path` values (`d:/old` vs `D:/old`) matches no records and
/// this returns zero moved.
///
/// Fails before opening a transaction if `old_root` and `new_root` overlap
/// (they name the same directory, or one is an ancestor of the other):
/// rewriting a prefix while part of it is itself moving cannot be expressed
/// as one substitution, since a record's rewritten path could land on another
/// record's not-yet-rewritten path. `old_root == new_root` falls under "name
/// the same directory" and is rejected the same way, not treated as a no-op.
/// It also fails up front if `new_root` is already another source's `root_path`.
///
/// The remaining writes happen in one transaction, rolled back explicitly on
/// any error so the caller never observes a half-relocated source.
pub async fn relocate_source(
    pool: &SqlitePool,
    source: SourceId,
    old_root: &Path,
    new_root: &Path,
) -> Result<u64, IndexError> {
    let old = normalize_root(old_root);
    let new = normalize_root(new_root);

    if roots_overlap(&old, &new) {
        return Err(IndexError::RelocationNested { old_root: PathBuf::from(old), new_root: PathBuf::from(new) });
    }

    let owner: Option<(i64,)> =
        sqlx::query_as("SELECT source_id FROM index_source WHERE root_path = ?1 AND source_id <> ?2")
            .bind(&new)
            .bind(source.0)
            .fetch_optional(pool)
            .await?;
    if let Some((owner_id,)) = owner {
        return Err(IndexError::RootAlreadyOwned { root_path: PathBuf::from(new), owner: SourceId(owner_id) });
    }

    let mut tx = pool.begin().await?;
    match relocate_source_in_tx(&mut tx, source, &old, &new).await {
        Ok(moved) => {
            tx.commit().await?;
            Ok(moved)
        }
        Err(err) => {
            tx.rollback().await?;
            Err(err)
        }
    }
}

/// Strips a trailing `/` or `\` so `old_root`/`new_root` can be passed with or
/// without one and still line up with the boundary-checked prefix match below.
fn normalize_root(root: &Path) -> String {
    root.to_string_lossy().trim_end_matches(['/', '\\']).to_string()
}

/// True if `outer` names the same directory as `inner`, or is an ancestor
/// directory of it: `inner` equals `outer`, or starts with `outer` followed
/// immediately by a path separator.
fn is_root_or_ancestor(outer: &str, inner: &str) -> bool {
    if inner == outer {
        return true;
    }
    match inner.strip_prefix(outer) {
        Some(rest) => matches!(rest.chars().next(), Some('/') | Some('\\')),
        None => false,
    }
}

fn roots_overlap(old: &str, new: &str) -> bool {
    is_root_or_ancestor(old, new) || is_root_or_ancestor(new, old)
}

async fn relocate_source_in_tx(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    source: SourceId,
    old: &str,
    new: &str,
) -> Result<u64, IndexError> {
    let old_len = old.chars().count() as i64;

    // A record under old_root whose rewritten path already equals another
    // record's path in the same source, where that other record is NOT itself
    // under old_root (so it keeps its path rather than also moving), would
    // collide on (source_id, replay_path) once rewritten. Detect that up front
    // rather than let the UPDATE below fail partway through the source.
    //
    // "Under old_root" requires SUBSTR to match the literal prefix AND the
    // next character to be a path separator (or the path to end exactly at
    // the prefix), so a sibling directory such as D:/oldarchive never counts
    // as being under D:/old.
    let collision: Option<(String,)> = sqlx::query_as(
        "SELECT r2.replay_path FROM replay_record r1 \
         JOIN replay_record r2 ON r2.source_id = r1.source_id \
           AND r2.replay_path = (?1 || SUBSTR(r1.replay_path, ?2)) \
         WHERE r1.source_id = ?3 \
           AND SUBSTR(r1.replay_path, 1, ?4) = ?5 \
           AND (LENGTH(r1.replay_path) = ?4 OR SUBSTR(r1.replay_path, ?4 + 1, 1) IN ('/', '\\')) \
           AND r2.record_id <> r1.record_id \
           AND NOT ( \
             SUBSTR(r2.replay_path, 1, ?4) = ?5 \
             AND (LENGTH(r2.replay_path) = ?4 OR SUBSTR(r2.replay_path, ?4 + 1, 1) IN ('/', '\\')) \
           ) \
         LIMIT 1",
    )
    .bind(new)
    .bind(old_len + 1)
    .bind(source.0)
    .bind(old_len)
    .bind(old)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((path,)) = collision {
        return Err(IndexError::RelocationCollision { path: PathBuf::from(path) });
    }

    // LIKE would treat _ and % in a path as wildcards; comparing an explicit
    // substr keeps the match literal. The boundary clause mirrors the
    // collision precheck above, so a sibling directory is never rewritten.
    let moved = sqlx::query(
        "UPDATE replay_record SET replay_path = ?1 || SUBSTR(replay_path, ?2) \
         WHERE source_id = ?3 \
           AND SUBSTR(replay_path, 1, ?4) = ?5 \
           AND (LENGTH(replay_path) = ?4 OR SUBSTR(replay_path, ?4 + 1, 1) IN ('/', '\\'))",
    )
    .bind(new)
    .bind(old_len + 1)
    .bind(source.0)
    .bind(old_len)
    .bind(old)
    .execute(&mut **tx)
    .await?
    .rows_affected();

    sqlx::query("UPDATE index_source SET root_path = ?1 WHERE source_id = ?2")
        .bind(new)
        .bind(source.0)
        .execute(&mut **tx)
        .await?;

    Ok(moved)
}

/// Delete a source. Its `replay_record` rows go with it via `ON DELETE CASCADE`
/// on `replay_record.source_id`; `indexed_match` and `indexed_vehicle` are keyed
/// by `arena_id`, shared across sources, and are not touched.
pub async fn forget_source(pool: &SqlitePool, source: SourceId) -> Result<(), IndexError> {
    sqlx::query("DELETE FROM index_source WHERE source_id = ?1").bind(source.0).execute(pool).await?;
    Ok(())
}

pub async fn upsert_match(pool: &SqlitePool, m: &ObjectiveMatch) -> Result<(), IndexError> {
    sqlx::query(
        "INSERT INTO indexed_match \
         (arena_id, timestamp, map, game_mode, game_type, match_group, version_build) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(arena_id) DO UPDATE SET \
           timestamp=?2, map=?3, game_mode=?4, game_type=?5, match_group=?6, version_build=?7",
    )
    .bind(m.arena_id.raw())
    .bind(m.timestamp.as_second())
    .bind(&m.map)
    .bind(&m.game_mode)
    .bind(&m.game_type)
    .bind(&m.match_group)
    .bind(m.version_build.map(|v| v as i64))
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn upsert_vehicles(pool: &SqlitePool, rows: &[IndexedVehicleRow]) -> Result<(), IndexError> {
    let mut tx = pool.begin().await?;
    for v in rows {
        sqlx::query(
            "INSERT INTO indexed_vehicle \
             (arena_id, account_id, player_name, clan, realm, ship_id, ship_index, ship_name, nation, species, \
              tier, relation, division_id, survived, damage, kills, spotting, potential, received, pr, is_test_ship, \
              disconnected, is_stream_sniper, sniper_twitch_login) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24) \
             ON CONFLICT(arena_id, account_id, ship_id) DO UPDATE SET \
               player_name=?3, clan=?4, realm=?5, ship_index=?7, ship_name=?8, nation=?9, species=?10, \
               tier=?11, relation=?12, division_id=?13, survived=?14, damage=?15, kills=?16, spotting=?17, \
               potential=?18, received=?19, pr=?20, is_test_ship=?21, disconnected=?22, is_stream_sniper=?23, \
               sniper_twitch_login=?24",
        )
        .bind(v.arena_id.raw())
        .bind(v.account_id.raw())
        .bind(&v.player_name)
        .bind(&v.clan)
        .bind(v.realm.as_deref())
        .bind(v.ship_id.raw() as i64)
        .bind(&v.ship_index)
        .bind(&v.ship_name)
        .bind(&v.nation)
        .bind(&v.species)
        .bind(v.tier as i64)
        .bind(v.relation.as_db_str())
        .bind(v.division_id)
        .bind(v.survived)
        .bind(v.damage.map(|d| d as i64))
        .bind(v.kills)
        .bind(v.spotting.map(|d| d as i64))
        .bind(v.potential.map(|d| d as i64))
        .bind(v.received.map(|d| d as i64))
        .bind(v.pr)
        .bind(v.is_test_ship)
        .bind(v.disconnected)
        .bind(v.is_stream_sniper)
        .bind(&v.sniper_twitch_login)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Record Twitch chat observations (login, seen_at unix seconds). Duplicate
/// (login, seen_at) pairs are silently ignored via the unique index.
pub async fn record_twitch_observations(pool: &SqlitePool, observations: &[(String, i64)]) -> Result<(), IndexError> {
    if observations.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for (login, seen_at) in observations {
        sqlx::query("INSERT OR IGNORE INTO twitch_observation (login, seen_at) VALUES (?1, ?2)")
            .bind(login)
            .bind(seen_at)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Twitch observations with `seen_at` in `[start_unix, end_unix]`.
pub async fn observations_in_window(
    pool: &SqlitePool,
    start_unix: i64,
    end_unix: i64,
) -> Result<Vec<(String, i64)>, IndexError> {
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT login, seen_at FROM twitch_observation WHERE seen_at BETWEEN ?1 AND ?2")
            .bind(start_unix)
            .bind(end_unix)
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

/// Delete Twitch observations older than `older_than_unix`. Returns the number of rows deleted.
pub async fn prune_twitch_observations(pool: &SqlitePool, older_than_unix: i64) -> Result<u64, IndexError> {
    let result =
        sqlx::query("DELETE FROM twitch_observation WHERE seen_at < ?1").bind(older_than_unix).execute(pool).await?;
    Ok(result.rows_affected())
}

pub async fn upsert_record(pool: &SqlitePool, r: &ReplayRecord) -> Result<(), IndexError> {
    sqlx::query(
        "INSERT INTO replay_record \
         (arena_id, source_id, replay_path, file_mtime, outcome, self_account_id, self_ship_id, self_survived, \
          self_damage, self_kills, self_pr, results_available, indexed_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) \
         ON CONFLICT(source_id, replay_path) DO UPDATE SET \
           arena_id=?1, file_mtime=?4, outcome=?5, self_account_id=?6, self_ship_id=?7, self_survived=?8, \
           self_damage=?9, self_kills=?10, self_pr=?11, results_available=?12, indexed_at=?13",
    )
    .bind(r.arena_id.raw())
    .bind(r.source_id.0)
    .bind(r.replay_path.to_string_lossy().to_string())
    .bind(r.file_mtime)
    .bind(r.outcome.as_db_str())
    .bind(r.self_account_id.map(|a| a.raw()))
    .bind(r.self_ship_id.map(|s| s.raw() as i64))
    .bind(r.self_survived)
    .bind(r.self_damage.map(|d| d as i64))
    .bind(r.self_kills)
    .bind(r.self_pr)
    .bind(r.results_available)
    .bind(r.indexed_at.as_second())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn arena_ids_in_source(pool: &SqlitePool, source: SourceId) -> Result<HashSet<ArenaId>, IndexError> {
    let rows: Vec<(i64,)> = sqlx::query_as("SELECT DISTINCT arena_id FROM replay_record WHERE source_id = ?1")
        .bind(source.0)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|(a,)| ArenaId::new(a)).collect())
}

/// Absolute replay paths already recorded for `source`. The startup reconciliation
/// pass uses this as the per-path index-membership ledger so already-indexed files
/// are skipped instead of re-parsed on every launch.
pub async fn record_paths_in_source(pool: &SqlitePool, source: SourceId) -> Result<HashSet<String>, IndexError> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT replay_path FROM replay_record WHERE source_id = ?1")
        .bind(source.0)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|(p,)| p).collect())
}

/// Row summaries for every record in `source`, keyed by replay path.
///
/// `division_id` is read through a correlated subquery on the roster keyed by
/// `self_account_id` rather than by `relation = 'self'`. `indexed_vehicle` rows
/// are shared across perspectives, so `relation` reflects whichever perspective
/// was indexed last, while `self_account_id` on the record is perspective-correct
/// by construction. The subquery form also guarantees one output row per record,
/// which a join would not.
///
/// `division_mates` is filled by one additional source-wide query rather than
/// one query per row: a listing can hold thousands of replays, and a per-row
/// roster lookup would be pathological at that scale.
pub async fn row_summaries_for_source(
    pool: &SqlitePool,
    source: SourceId,
) -> Result<HashMap<PathBuf, RowSummary>, IndexError> {
    let rows = sqlx::query(
        "SELECT r.replay_path, r.outcome, r.self_damage, r.self_kills, r.self_survived, r.self_pr, \
                r.results_available, r.file_mtime, \
                (SELECT v.division_id FROM indexed_vehicle v \
                   WHERE v.arena_id = r.arena_id AND v.account_id = r.self_account_id LIMIT 1) AS division_id \
         FROM replay_record r WHERE r.source_id = ?1",
    )
    .bind(source.0)
    .fetch_all(pool)
    .await?;

    let mut out = HashMap::with_capacity(rows.len());
    for row in &rows {
        let (path, summary) = row_to_row_summary(row)?;
        out.insert(path, summary);
    }

    // `sv.account_id = r.self_account_id` already excludes a NULL self_account_id
    // (a spectator recording): SQL NULL never equals anything, including another
    // NULL, so that record joins to no roster row at all rather than to every
    // NULL-account row. `sv.division_id IS NOT NULL` then drops solo players,
    // whose stored division is NULL, before the mate join runs.
    let mate_rows = sqlx::query(
        "SELECT DISTINCT r.replay_path, mate.player_name, mate.clan \
         FROM replay_record r \
         JOIN indexed_vehicle sv ON sv.arena_id = r.arena_id AND sv.account_id = r.self_account_id \
         JOIN indexed_vehicle mate ON mate.arena_id = r.arena_id AND mate.division_id = sv.division_id \
         WHERE r.source_id = ?1 \
           AND sv.division_id IS NOT NULL \
           AND mate.account_id <> 0 \
           AND mate.account_id <> r.self_account_id \
         ORDER BY r.replay_path, mate.player_name",
    )
    .bind(source.0)
    .fetch_all(pool)
    .await?;

    for row in &mate_rows {
        let path = PathBuf::from(row.try_get::<String, _>("replay_path")?);
        let player_name: String = row.try_get("player_name")?;
        let clan: String = row.try_get("clan")?;
        if let Some(summary) = out.get_mut(&path) {
            summary.division_mates.push(DivisionMate { player_name, clan });
        }
    }

    Ok(out)
}

fn row_to_row_summary(row: &sqlx::sqlite::SqliteRow) -> Result<(PathBuf, RowSummary), IndexError> {
    let outcome_str: String = row.try_get("outcome")?;
    let self_damage: Option<i64> = row.try_get("self_damage")?;
    let path = PathBuf::from(row.try_get::<String, _>("replay_path")?);
    let summary = RowSummary {
        // An unrecognised stored string means the row predates a variant we
        // know; `Unknown` renders untinted, which is the honest fallback.
        outcome: MatchOutcome::from_db_str(&outcome_str).unwrap_or(MatchOutcome::Unknown),
        // A negative stored damage is nonsense; reading it as absent beats
        // wrapping it into ~1.8e19 on the row.
        self_damage: self_damage.and_then(|d| u64::try_from(d).ok()),
        self_kills: row.try_get("self_kills")?,
        self_survived: row.try_get::<Option<bool>, _>("self_survived")?,
        self_pr: row.try_get("self_pr")?,
        division_id: row.try_get("division_id")?,
        division_mates: Vec::new(),
        results_available: row.try_get("results_available")?,
        file_mtime: row.try_get("file_mtime")?,
    };
    Ok((path, summary))
}

/// One `MatchHit` per arena. The chosen record prefers `file_mtime IS NOT NULL`
/// then most-recently indexed, so the open target points at a present file when
/// possible. Applies both match/record-level predicates and roster EXISTS predicates.
pub async fn search_matches(pool: &SqlitePool, filter: &MatchFilter) -> Result<Vec<MatchHit>, IndexError> {
    run_match_query(pool, filter, None).await
}

pub async fn recent_matches(pool: &SqlitePool, filter: &MatchFilter, limit: i64) -> Result<Vec<MatchHit>, IndexError> {
    run_match_query(pool, filter, Some(limit)).await
}

async fn run_match_query(
    pool: &SqlitePool,
    filter: &MatchFilter,
    limit: Option<i64>,
) -> Result<Vec<MatchHit>, IndexError> {
    // Pick one record per arena: prefer a file that still has an mtime, then newest indexed.
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT m.arena_id, m.timestamp, m.map, m.game_mode, m.game_type, m.match_group, m.version_build, \
                r.source_id, r.outcome, r.self_account_id, r.self_ship_id, r.self_survived, r.self_damage, \
                r.self_kills, r.self_pr, r.results_available, r.replay_path, r.file_mtime \
         FROM indexed_match m \
         JOIN replay_record r ON r.record_id = ( \
            SELECT rr.record_id FROM replay_record rr \
            WHERE rr.arena_id = m.arena_id",
    );
    // source scope inside the per-arena record picker
    if let Some(sources) = &filter.source_ids {
        qb.push(" AND rr.source_id IN (");
        let mut sep = qb.separated(", ");
        for s in sources {
            sep.push_bind(s.0);
        }
        qb.push(")");
    }
    qb.push(" ORDER BY (rr.file_mtime IS NOT NULL) DESC, rr.indexed_at DESC LIMIT 1 ) WHERE 1=1");

    // match-level + record-level predicates
    if let Some(o) = filter.outcome {
        qb.push(" AND r.outcome = ").push_bind(o.as_db_str());
    }
    if let Some(ship) = filter.self_ship {
        qb.push(" AND r.self_ship_id = ").push_bind(ship.raw() as i64);
    }
    if let Some(map) = &filter.map {
        qb.push(" AND m.map = ").push_bind(map.clone());
    }
    if let Some(gt) = &filter.game_type {
        qb.push(" AND m.game_type = ").push_bind(gt.clone());
    }
    if let Some(from) = filter.date_from {
        qb.push(" AND m.timestamp >= ").push_bind(from.as_second());
    }
    if let Some(to) = filter.date_to {
        qb.push(" AND m.timestamp <= ").push_bind(to.as_second());
    }
    if let Some(min) = filter.self_damage_min {
        qb.push(" AND r.self_damage >= ").push_bind(min as i64);
    }
    if let Some(max) = filter.self_damage_max {
        qb.push(" AND r.self_damage <= ").push_bind(max as i64);
    }
    if let Some(s) = filter.self_survived {
        qb.push(" AND r.self_survived = ").push_bind(s);
    }
    push_exists_predicates(&mut qb, filter);

    qb.push(" ORDER BY m.timestamp DESC");
    if let Some(limit) = limit {
        qb.push(" LIMIT ").push_bind(limit);
    }

    let rows = qb.build().fetch_all(pool).await?;
    rows.iter().map(row_to_match_hit).collect()
}

fn row_to_match_hit(row: &sqlx::sqlite::SqliteRow) -> Result<MatchHit, IndexError> {
    let outcome_str: String = row.try_get("outcome")?;
    let self_account: Option<i64> = row.try_get("self_account_id")?;
    let self_ship: Option<i64> = row.try_get("self_ship_id")?;
    let version_build: Option<i64> = row.try_get("version_build")?;
    let self_damage: Option<i64> = row.try_get("self_damage")?;
    Ok(MatchHit {
        arena_id: wows_core::game_types::ArenaId::new(row.try_get::<i64, _>("arena_id")?),
        timestamp: jiff::Timestamp::from_second(row.try_get::<i64, _>("timestamp")?)
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
        map: row.try_get("map")?,
        game_mode: row.try_get("game_mode")?,
        game_type: row.try_get("game_type")?,
        match_group: row.try_get("match_group")?,
        version_build: version_build.map(|v| v as u32),
        source_id: SourceId(row.try_get::<i64, _>("source_id")?),
        outcome: MatchOutcome::from_db_str(&outcome_str).unwrap_or(MatchOutcome::Unknown),
        self_account_id: self_account.map(AccountId::from),
        self_ship_id: self_ship.map(|s| GameParamId::from(s as u64)),
        self_survived: row.try_get::<Option<bool>, _>("self_survived")?,
        self_damage: self_damage.map(|d| d as u64),
        self_kills: row.try_get("self_kills")?,
        self_pr: row.try_get("self_pr")?,
        results_available: row.try_get("results_available")?,
        replay_path: std::path::PathBuf::from(row.try_get::<String, _>("replay_path")?),
        file_mtime: row.try_get("file_mtime")?,
    })
}

fn push_exists_predicates(qb: &mut QueryBuilder<'_, Sqlite>, filter: &MatchFilter) {
    if let Some(species) = &filter.species {
        qb.push(" AND EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND v.relation = ")
            .push_bind(VehicleRelation::SelfPlayer.as_db_str())
            .push(" AND v.species = ")
            .push_bind(species.clone())
            .push(")");
    }
    if let Some(tier) = filter.tier {
        qb.push(" AND EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND v.relation = ")
            .push_bind(VehicleRelation::SelfPlayer.as_db_str())
            .push(" AND v.tier = ")
            .push_bind(tier as i64)
            .push(")");
    }
    if let Some(acct) = filter.player_present {
        qb.push(" AND EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND v.account_id = ")
            .push_bind(acct.raw())
            .push(")");
    }
    if let Some(ship) = filter.enemy_ship {
        qb.push(" AND EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND v.relation = ")
            .push_bind(VehicleRelation::Enemy.as_db_str())
            .push(" AND v.ship_id = ")
            .push_bind(ship.raw() as i64)
            .push(")");
    }
}

pub async fn matches_with_player(
    pool: &SqlitePool,
    account: AccountId,
    filter: &MatchFilter,
) -> Result<Vec<MatchHit>, IndexError> {
    let mut f = filter.clone();
    f.player_present = Some(account);
    search_matches(pool, &f).await
}

pub async fn matches_with_ship(
    pool: &SqlitePool,
    ship: GameParamId,
    relation: Option<super::rows::VehicleRelation>,
    filter: &MatchFilter,
) -> Result<Vec<MatchHit>, IndexError> {
    // Reuse search_matches for all shared predicates, then require the ship in the
    // requested relation (or any relation) via arena membership in the roster.
    let arenas: Vec<(i64,)> = {
        let mut qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("SELECT DISTINCT arena_id FROM indexed_vehicle WHERE ship_id = ");
        qb.push_bind(ship.raw() as i64);
        if let Some(rel) = relation {
            qb.push(" AND relation = ").push_bind(rel.as_db_str());
        }
        qb.build_query_as().fetch_all(pool).await?
    };
    let allowed: std::collections::HashSet<i64> = arenas.into_iter().map(|(a,)| a).collect();
    let hits = search_matches(pool, filter).await?;
    Ok(hits.into_iter().filter(|h| allowed.contains(&h.arena_id.raw())).collect())
}

/// Distinct non-bot players across the index, most-encountered first.
/// `filter.source_ids` scopes to groups; other filter fields are ignored here.
pub async fn distinct_players(pool: &SqlitePool, filter: &MatchFilter) -> Result<Vec<PlayerFacet>, IndexError> {
    // latest name = name from the vehicle row in the most recent match for that account.
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT v.account_id, \
                (SELECT v2.player_name FROM indexed_vehicle v2 JOIN indexed_match m2 ON m2.arena_id = v2.arena_id \
                   WHERE v2.account_id = v.account_id ORDER BY m2.timestamp DESC LIMIT 1) AS latest_name, \
                (SELECT v3.clan FROM indexed_vehicle v3 JOIN indexed_match m3 ON m3.arena_id = v3.arena_id \
                   WHERE v3.account_id = v.account_id ORDER BY m3.timestamp DESC LIMIT 1) AS clan, \
                COUNT(DISTINCT v.arena_id) AS match_count \
         FROM indexed_vehicle v WHERE v.account_id <> 0",
    );
    if let Some(sources) = &filter.source_ids {
        qb.push(" AND v.arena_id IN (SELECT arena_id FROM replay_record WHERE source_id IN (");
        let mut sep = qb.separated(", ");
        for s in sources {
            sep.push_bind(s.0);
        }
        qb.push("))");
    }
    qb.push(" GROUP BY v.account_id ORDER BY match_count DESC");

    let rows = qb.build().fetch_all(pool).await?;
    rows.iter()
        .map(|row| {
            Ok(PlayerFacet {
                account_id: AccountId::from(row.try_get::<i64, _>("account_id")?),
                latest_name: row.try_get::<Option<String>, _>("latest_name")?.unwrap_or_default(),
                clan: row.try_get::<Option<String>, _>("clan")?.unwrap_or_default(),
                match_count: row.try_get("match_count")?,
            })
        })
        .collect()
}

/// Distinct self-perspective account ids across the index (`replay_record.self_account_id`).
/// `filter.source_ids` scopes to groups; other filter fields are ignored here.
pub async fn self_account_ids(pool: &SqlitePool, filter: &MatchFilter) -> Result<HashSet<AccountId>, IndexError> {
    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new("SELECT DISTINCT self_account_id FROM replay_record WHERE self_account_id IS NOT NULL");
    if let Some(sources) = &filter.source_ids {
        qb.push(" AND source_id IN (");
        let mut sep = qb.separated(", ");
        for s in sources {
            sep.push_bind(s.0);
        }
        qb.push(")");
    }

    let rows: Vec<(i64,)> = qb.build_query_as().fetch_all(pool).await?;
    Ok(rows.into_iter().map(|(a,)| AccountId::from(a)).collect())
}

/// The encounters in which an account shared a division with the replay's self
/// player, one row per (account, match).
///
/// Per encounter, not per account: an account that divisioned with you in one
/// match and fought you in another is only returned for the first.
/// `indexed_vehicle.division_id` is NULL for a player who is not in a division,
/// so a NULL on either side never pairs, and the writer stores NULL rather than
/// 0 for "no division". The self account of the record is never returned.
///
/// `filter.source_ids` scopes to groups; other filter fields are ignored here.
pub async fn division_mate_encounters(
    pool: &SqlitePool,
    filter: &MatchFilter,
) -> Result<Vec<DivisionMateEncounter>, IndexError> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT DISTINCT mate.account_id, r.arena_id, m.timestamp \
           FROM replay_record r \
           JOIN indexed_match m ON m.arena_id = r.arena_id \
           JOIN indexed_vehicle me ON me.arena_id = r.arena_id AND me.account_id = r.self_account_id \
           JOIN indexed_vehicle mate ON mate.arena_id = r.arena_id AND mate.division_id = me.division_id \
          WHERE r.self_account_id IS NOT NULL AND me.division_id IS NOT NULL \
            AND mate.account_id <> 0 AND mate.account_id <> r.self_account_id",
    );
    if let Some(sources) = &filter.source_ids {
        qb.push(" AND r.source_id IN (");
        let mut sep = qb.separated(", ");
        for s in sources {
            sep.push_bind(s.0);
        }
        qb.push(")");
    }

    let rows = qb.build().fetch_all(pool).await?;
    rows.iter()
        .map(|row| {
            Ok(DivisionMateEncounter {
                account_id: AccountId::from(row.try_get::<i64, _>("account_id")?),
                arena_id: ArenaId::new(row.try_get::<i64, _>("arena_id")?),
                // Matches how `matches_with_player` degrades an unrepresentable
                // stored second; `IndexError` has no timestamp variant.
                timestamp: jiff::Timestamp::from_second(row.try_get::<i64, _>("timestamp")?)
                    .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
            })
        })
        .collect()
}

/// Encounters whose clan at the time differs from the account's latest clan.
/// Only the differences, so the result stays small on a large index: most
/// accounts never changed clan.
pub async fn clan_history_corrections(
    pool: &SqlitePool,
    filter: &MatchFilter,
) -> Result<Vec<ClanCorrection>, IndexError> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "WITH latest AS ( \
           SELECT v.account_id AS account_id, \
                  (SELECT v2.clan FROM indexed_vehicle v2 \
                     JOIN indexed_match m2 ON m2.arena_id = v2.arena_id \
                    WHERE v2.account_id = v.account_id \
                    ORDER BY m2.timestamp DESC LIMIT 1) AS clan \
             FROM indexed_vehicle v WHERE v.account_id <> 0 GROUP BY v.account_id \
         ) \
         SELECT v.account_id, v.arena_id, m.timestamp, v.clan \
           FROM indexed_vehicle v \
           JOIN indexed_match m ON m.arena_id = v.arena_id \
           JOIN latest l ON l.account_id = v.account_id \
          WHERE v.account_id <> 0 AND v.clan <> l.clan",
    );
    if let Some(sources) = &filter.source_ids {
        qb.push(" AND v.arena_id IN (SELECT arena_id FROM replay_record WHERE source_id IN (");
        let mut sep = qb.separated(", ");
        for s in sources {
            sep.push_bind(s.0);
        }
        qb.push("))");
    }

    let rows = qb.build().fetch_all(pool).await?;
    rows.iter()
        .map(|row| {
            Ok(ClanCorrection {
                account_id: AccountId::from(row.try_get::<i64, _>("account_id")?),
                arena_id: ArenaId::new(row.try_get::<i64, _>("arena_id")?),
                // Matches how `matches_with_player` degrades an unrepresentable
                // stored second; `IndexError` has no timestamp variant.
                timestamp: jiff::Timestamp::from_second(row.try_get::<i64, _>("timestamp")?)
                    .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
                // `indexed_vehicle.clan` is NOT NULL and selected directly, so
                // reading it as nullable would only map an impossible NULL onto
                // a clanless player.
                clan: row.try_get::<String, _>("clan")?,
            })
        })
        .collect()
}

/// Ships the user has played (from `replay_record.self_ship_id`), most-played first.
pub async fn distinct_self_ships(pool: &SqlitePool, filter: &MatchFilter) -> Result<Vec<ShipFacet>, IndexError> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT r.self_ship_id AS ship_id, \
                (SELECT v.ship_name FROM indexed_vehicle v WHERE v.ship_id = r.self_ship_id LIMIT 1) AS ship_name, \
                COUNT(DISTINCT r.arena_id) AS match_count \
         FROM replay_record r WHERE r.self_ship_id IS NOT NULL",
    );
    if let Some(sources) = &filter.source_ids {
        qb.push(" AND r.source_id IN (");
        let mut sep = qb.separated(", ");
        for s in sources {
            sep.push_bind(s.0);
        }
        qb.push(")");
    }
    qb.push(" GROUP BY r.self_ship_id ORDER BY match_count DESC");

    let rows = qb.build().fetch_all(pool).await?;
    rows.iter()
        .map(|row| {
            Ok(ShipFacet {
                ship_id: GameParamId::from(row.try_get::<i64, _>("ship_id")? as u64),
                ship_name: row.try_get::<Option<String>, _>("ship_name")?.unwrap_or_default(),
                match_count: row.try_get("match_count")?,
            })
        })
        .collect()
}

/// The account's most-recent `player_name`, or `None` if the account is unknown to the index.
pub async fn player_name(pool: &SqlitePool, account: AccountId) -> Result<Option<String>, IndexError> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT v.player_name FROM indexed_vehicle v JOIN indexed_match m ON m.arena_id = v.arena_id \
         WHERE v.account_id = ?1 ORDER BY m.timestamp DESC LIMIT 1",
    )
    .bind(account.raw())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(name,)| name))
}

/// Any `indexed_vehicle.ship_name` recorded for `ship`, or `None` if the ship is unknown to the index.
pub async fn ship_name(pool: &SqlitePool, ship: GameParamId) -> Result<Option<String>, IndexError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT ship_name FROM indexed_vehicle WHERE ship_id = ?1 LIMIT 1")
        .bind(ship.raw() as i64)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(name,)| name))
}

/// Bounded, case-insensitive player search for the cascading palette: non-bot accounts
/// whose latest name contains `needle`, ranked by match count, capped at `limit`.
/// An empty `needle` matches everything, so this also serves as "top players by count".
pub async fn search_players(pool: &SqlitePool, needle: &str, limit: i64) -> Result<Vec<PlayerFacet>, IndexError> {
    let rows: Vec<(i64, Option<String>, Option<String>, i64)> = sqlx::query_as(
        "SELECT v.account_id, \
                (SELECT v2.player_name FROM indexed_vehicle v2 JOIN indexed_match m2 ON m2.arena_id = v2.arena_id \
                   WHERE v2.account_id = v.account_id ORDER BY m2.timestamp DESC LIMIT 1) AS latest_name, \
                (SELECT v3.clan FROM indexed_vehicle v3 JOIN indexed_match m3 ON m3.arena_id = v3.arena_id \
                   WHERE v3.account_id = v.account_id ORDER BY m3.timestamp DESC LIMIT 1) AS clan, \
                COUNT(DISTINCT v.arena_id) AS match_count \
         FROM indexed_vehicle v \
         WHERE v.account_id <> 0 AND LOWER(v.player_name) LIKE '%' || LOWER(?1) || '%' \
         GROUP BY v.account_id ORDER BY match_count DESC LIMIT ?2",
    )
    .bind(needle)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, clan, count)| PlayerFacet {
            account_id: AccountId::from(id),
            latest_name: name.unwrap_or_default(),
            clan: clan.unwrap_or_default(),
            match_count: count,
        })
        .collect())
}

/// Bounded, case-insensitive ship search over the whole roster: every ship that
/// has appeared in an indexed match, whichever side played it, ranked by how
/// often it appears. Distinct from `search_self_ships`, which is scoped to the
/// ships the user played and so cannot answer an `enemy.ship:` lookup.
pub async fn search_ships(pool: &SqlitePool, needle: &str, limit: i64) -> Result<Vec<ShipFacet>, IndexError> {
    let rows: Vec<(i64, Option<String>, i64)> = sqlx::query_as(
        "SELECT v.ship_id, MAX(v.ship_name) AS ship_name, COUNT(DISTINCT v.arena_id) AS match_count \
         FROM indexed_vehicle v \
         WHERE LOWER(v.ship_name) LIKE '%' || LOWER(?1) || '%' \
         GROUP BY v.ship_id ORDER BY match_count DESC LIMIT ?2",
    )
    .bind(needle)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, count)| ShipFacet {
            ship_id: GameParamId::from(id as u64),
            ship_name: name.unwrap_or_default(),
            match_count: count,
        })
        .collect())
}

/// The map names the index has seen, most-played first, capped at `limit`.
///
/// `indexed_match.map` holds the localized display name the replay reported, so
/// these are already the names the user reads.
pub async fn distinct_maps(pool: &SqlitePool, limit: i64) -> Result<Vec<String>, IndexError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT map, COUNT(*) AS match_count FROM indexed_match WHERE map <> '' \
         GROUP BY map ORDER BY match_count DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(map, _)| map).collect())
}

/// Bounded, case-insensitive self-ship search for the cascading palette: ships the
/// user has played whose name contains `needle`, ranked by match count, capped at `limit`.
pub async fn search_self_ships(pool: &SqlitePool, needle: &str, limit: i64) -> Result<Vec<ShipFacet>, IndexError> {
    let rows: Vec<(i64, Option<String>, i64)> = sqlx::query_as(
        "SELECT r.self_ship_id AS ship_id, \
                (SELECT v.ship_name FROM indexed_vehicle v WHERE v.ship_id = r.self_ship_id LIMIT 1) AS ship_name, \
                COUNT(DISTINCT r.arena_id) AS match_count \
         FROM replay_record r \
         WHERE r.self_ship_id IS NOT NULL AND LOWER( \
                (SELECT v.ship_name FROM indexed_vehicle v WHERE v.ship_id = r.self_ship_id LIMIT 1) \
              ) LIKE '%' || LOWER(?1) || '%' \
         GROUP BY r.self_ship_id ORDER BY match_count DESC LIMIT ?2",
    )
    .bind(needle)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, count)| ShipFacet {
            ship_id: GameParamId::from(id as u64),
            ship_name: name.unwrap_or_default(),
            match_count: count,
        })
        .collect())
}

/// Run a dynamic advanced-search query built from chips grouped by AND, with groups
/// joined by `query.connector`. Reuses the same per-arena record-picker prefix and row
/// mapping as `run_match_query`. An empty query (no chips in any group) matches everything.
pub async fn search_by_query(pool: &SqlitePool, query: &Query, limit: i64) -> Result<Vec<MatchHit>, IndexError> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT m.arena_id, m.timestamp, m.map, m.game_mode, m.game_type, m.match_group, m.version_build, \
                r.source_id, r.outcome, r.self_account_id, r.self_ship_id, r.self_survived, r.self_damage, \
                r.self_kills, r.self_pr, r.results_available, r.replay_path, r.file_mtime \
         FROM indexed_match m \
         JOIN replay_record r ON r.record_id = ( \
            SELECT rr.record_id FROM replay_record rr WHERE rr.arena_id = m.arena_id \
            ORDER BY (rr.file_mtime IS NOT NULL) DESC, rr.indexed_at DESC LIMIT 1 ) WHERE 1=1",
    );
    push_query_where(&mut qb, query);
    qb.push(" ORDER BY m.timestamp DESC LIMIT ").push_bind(limit);
    let rows = qb.build().fetch_all(pool).await?;
    rows.iter().map(row_to_match_hit).collect()
}

/// Run a query built from the AST. Mirrors `search_by_query`: the same per-arena
/// record picker, the same ordering, the same row mapping.
///
/// Fetches `limit + 1` rows so the caller can distinguish "exactly `limit`
/// results" from "at least `limit`" and say so, instead of reporting a
/// truncated count as a total.
pub async fn search_by_ast(
    pool: &SqlitePool,
    expr: &MatchExpr,
    ctx: &CompileCtx<'_>,
    limit: i64,
) -> Result<Vec<MatchHit>, IndexError> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT m.arena_id, m.timestamp, m.map, m.game_mode, m.game_type, m.match_group, m.version_build, \
                r.source_id, r.outcome, r.self_account_id, r.self_ship_id, r.self_survived, r.self_damage, \
                r.self_kills, r.self_pr, r.results_available, r.replay_path, r.file_mtime \
         FROM indexed_match m \
         JOIN replay_record r ON r.record_id = ( \
            SELECT rr.record_id FROM replay_record rr WHERE rr.arena_id = m.arena_id \
            ORDER BY (rr.file_mtime IS NOT NULL) DESC, rr.indexed_at DESC LIMIT 1 ) WHERE ",
    );
    push_match_expr(&mut qb, expr, ctx);
    qb.push(" ORDER BY m.timestamp DESC LIMIT ").push_bind(limit + 1);
    let rows = qb.build().fetch_all(pool).await?;
    rows.iter().map(row_to_match_hit).collect()
}

fn push_query_where(qb: &mut QueryBuilder<'_, Sqlite>, query: &Query) {
    if query.groups.iter().all(|g| g.chips.is_empty()) {
        return; // empty query matches everything
    }
    let join = match query.connector {
        Connector::And => " AND ",
        Connector::Or => " OR ",
    };
    qb.push(" AND (");
    let mut first_group = true;
    for group in &query.groups {
        if group.chips.is_empty() {
            continue;
        }
        if !first_group {
            qb.push(join);
        }
        first_group = false;
        qb.push("(");
        for (i, chip) in group.chips.iter().enumerate() {
            if i > 0 {
                qb.push(" AND ");
            }
            push_chip(qb, chip);
        }
        qb.push(")");
    }
    qb.push(")");
}

fn push_chip(qb: &mut QueryBuilder<'_, Sqlite>, chip: &Chip) {
    // Text columns: case-insensitive. Numeric/record columns direct. Presence: EXISTS.
    match (chip.field, &chip.value) {
        (Field::Map, Value::Text(s)) => push_text(qb, "m.map", chip.op, s),
        (Field::Mode, Value::Text(s)) => push_text(qb, "m.game_type", chip.op, s),
        (Field::Outcome, Value::Outcome(o)) => push_enum(qb, "r.outcome", chip.op, o.as_db_str()),
        (Field::SelfShip, Value::Ship(id)) => push_enum_i64(qb, "r.self_ship_id", chip.op, id.raw() as i64),
        (Field::Date, Value::Timestamp(t)) => push_num(qb, "m.timestamp", chip.op, t.as_second()),
        (Field::Tier, Value::Int(n)) => {
            let sql_op = num_sql_op(chip.op);
            push_exists(qb, &format!("v.relation='self' AND v.tier {sql_op} "), ExistsBind::Int(*n))
        }
        (Field::Class, Value::Class(s)) => {
            push_presence(qb, "v.relation='self' AND v.species = ", ExistsBind::Text(s.clone()), chip.op)
        }
        (Field::PlayerPresent, Value::Account(a)) => {
            push_presence(qb, "v.account_id = ", ExistsBind::Int(a.raw()), chip.op)
        }
        (Field::PlayerNameOrClan, Value::Text(needle)) => push_player_name_or_clan(qb, needle),
        (Field::EnemyShip, Value::Ship(id)) => {
            push_presence(qb, "v.relation='enemy' AND v.ship_id = ", ExistsBind::Int(id.raw() as i64), chip.op)
        }
        (Field::AllyShip, Value::Ship(id)) => {
            push_presence(qb, "v.relation='ally' AND v.ship_id = ", ExistsBind::Int(id.raw() as i64), chip.op)
        }
        (Field::Group, Value::Source(s)) => push_enum_i64(qb, "r.source_id", chip.op, s.0),
        // Match-level flag, no subject: mirrors the bool `Stat` branch below but reads
        // `is_stream_sniper` directly instead of a subject-scoped column.
        (Field::ContainsStreamSniper, Value::Bool(b)) => {
            let sql_op = if matches!(chip.op, Op::IsNot) { "<>" } else { "=" };
            push_exists(qb, &format!("v.is_stream_sniper {sql_op} "), ExistsBind::Bool(*b))
        }
        // Subject-scoped roster stat: EXISTS over the roster, scoped by arena and by
        // the subject (self row / any row / a specific account's row).
        (Field::Stat { kind, subject }, Value::Int(n)) if !kind.is_bool() => {
            let sql_op = num_sql_op(chip.op);
            push_exists_subject(qb, subject, &format!("v.{} {sql_op} ", kind.column()), ExistsBind::Int(*n))
        }
        (Field::Stat { kind, subject }, Value::Bool(b)) if kind.is_bool() => {
            let sql_op = if matches!(chip.op, Op::IsNot) { "<>" } else { "=" };
            push_exists_subject(qb, subject, &format!("v.{} {sql_op} ", kind.column()), ExistsBind::Bool(*b))
        }
        // Field/value mismatch (UI prevents this): no-op predicate + log.
        _ => {
            tracing::warn!("search_by_query: unsupported field/value combination");
            qb.push("1=1");
        }
    }
}

enum ExistsBind {
    Int(i64),
    Text(String),
    Bool(bool),
}

fn push_text(qb: &mut QueryBuilder<'_, Sqlite>, col: &str, op: Op, s: &str) {
    match op {
        Op::Contains => {
            qb.push(format!("LOWER({col}) LIKE '%' || LOWER(")).push_bind(s.to_string()).push(") || '%'");
        }
        Op::NotEquals => {
            qb.push(format!("LOWER({col}) <> LOWER(")).push_bind(s.to_string()).push(")");
        }
        _ => {
            // Equals (and any other): case-insensitive equality.
            qb.push(format!("LOWER({col}) = LOWER(")).push_bind(s.to_string()).push(")");
        }
    }
}

fn num_sql_op(op: Op) -> &'static str {
    match op {
        Op::Eq => "=",
        Op::Ne => "<>",
        Op::Gt => ">",
        Op::Ge => ">=",
        Op::Lt => "<",
        _ => "<=", // Le and any other
    }
}

fn push_num(qb: &mut QueryBuilder<'_, Sqlite>, col: &str, op: Op, n: i64) {
    let sql_op = num_sql_op(op);
    qb.push(format!("{col} {sql_op} ")).push_bind(n);
}

fn push_enum(qb: &mut QueryBuilder<'_, Sqlite>, col: &str, op: Op, val: &str) {
    let sql_op = if matches!(op, Op::IsNot | Op::NotEquals | Op::Ne) { "<>" } else { "=" };
    qb.push(format!("{col} {sql_op} ")).push_bind(val.to_string());
}

fn push_enum_i64(qb: &mut QueryBuilder<'_, Sqlite>, col: &str, op: Op, val: i64) {
    let sql_op = if matches!(op, Op::IsNot | Op::NotEquals | Op::Ne) { "<>" } else { "=" };
    qb.push(format!("{col} {sql_op} ")).push_bind(val);
}

fn push_exists(qb: &mut QueryBuilder<'_, Sqlite>, inner: &str, bind: ExistsBind) {
    qb.push("EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND ").push(inner);
    match bind {
        ExistsBind::Int(n) => qb.push_bind(n),
        ExistsBind::Text(s) => qb.push_bind(s),
        ExistsBind::Bool(b) => qb.push_bind(b),
    };
    qb.push(")");
}

/// `EXISTS` over the roster, scoped by arena and by `subject`: the perspective
/// player's own row, any row, or one specific account's row. `inner` is a trusted
/// (hardcoded) column comparison fragment ending in a placeholder for `bind`; a
/// `Subject::Player` account id is bound before `bind`, matching bind order.
fn push_exists_subject(qb: &mut QueryBuilder<'_, Sqlite>, subject: Subject, inner: &str, bind: ExistsBind) {
    qb.push("EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id");
    match subject {
        Subject::SelfPlayer => {
            qb.push(" AND v.relation = ").push_bind(VehicleRelation::SelfPlayer.as_db_str());
        }
        Subject::AnyPlayer => {}
        Subject::Player(account) => {
            qb.push(" AND v.account_id = ").push_bind(account.raw());
        }
    }
    qb.push(" AND ").push(inner);
    match bind {
        ExistsBind::Int(n) => {
            qb.push_bind(n);
        }
        ExistsBind::Text(s) => {
            qb.push_bind(s);
        }
        ExistsBind::Bool(b) => {
            qb.push_bind(b);
        }
    }
    qb.push(")");
}

fn push_presence(qb: &mut QueryBuilder<'_, Sqlite>, inner: &str, bind: ExistsBind, op: Op) {
    if matches!(op, Op::NotPresent | Op::IsNot) {
        qb.push("NOT ");
    }
    push_exists(qb, inner, bind);
}

/// `EXISTS` over the roster, arena-scoped, matching `needle` (case-insensitive
/// substring) against either `player_name` or `clan`. The needle is bound
/// twice, once per column comparison.
fn push_player_name_or_clan(qb: &mut QueryBuilder<'_, Sqlite>, needle: &str) {
    qb.push("EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND (");
    qb.push("LOWER(v.player_name) LIKE '%' || LOWER(").push_bind(needle.to_string()).push(") || '%'");
    qb.push(" OR LOWER(v.clan) LIKE '%' || LOWER(").push_bind(needle.to_string()).push(") || '%'");
    qb.push("))");
}
