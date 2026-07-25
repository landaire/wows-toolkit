//! Typed query API for the replay index. Populated in later tasks.

use std::collections::HashSet;
use std::path::Path;

use jiff::Timestamp;
use sqlx::SqlitePool;
use wows_core::game_types::ArenaId;

use super::rows::IndexError;
use super::rows::IndexSource;
use super::rows::IndexedVehicleRow;
use super::rows::ObjectiveMatch;
use super::rows::ReplayRecord;
use super::rows::SourceId;
use super::rows::SourceKind;

/// Return the id of the single `Live` source, creating it if absent.
pub async fn ensure_default_source(
    pool: &SqlitePool,
    root_path: &Path,
    now: Timestamp,
) -> Result<SourceId, IndexError> {
    let existing: Option<(i64,)> =
        sqlx::query_as("SELECT source_id FROM index_source WHERE kind = 'live' LIMIT 1").fetch_optional(pool).await?;
    if let Some((id,)) = existing {
        return Ok(SourceId(id));
    }
    let id: (i64,) = sqlx::query_as(
        "INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, 'live', ?2, ?3) RETURNING source_id",
    )
    .bind("Live replays")
    .bind(root_path.to_string_lossy().to_string())
    .bind(now.as_second())
    .fetch_one(pool)
    .await?;
    Ok(SourceId(id.0))
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
              tier, relation, division_id, survived, damage, kills, spotting, potential, received, pr, is_test_ship) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21) \
             ON CONFLICT(arena_id, account_id, ship_id) DO UPDATE SET \
               player_name=?3, clan=?4, realm=?5, ship_index=?7, ship_name=?8, nation=?9, species=?10, \
               tier=?11, relation=?12, division_id=?13, survived=?14, damage=?15, kills=?16, spotting=?17, \
               potential=?18, received=?19, pr=?20, is_test_ship=?21",
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
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
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
