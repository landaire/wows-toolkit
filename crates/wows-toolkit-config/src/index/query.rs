//! Typed query API for the replay index. Populated in later tasks.

use std::collections::HashSet;
use std::path::Path;

use jiff::Timestamp;
use sqlx::QueryBuilder;
use sqlx::Row;
use sqlx::Sqlite;
use sqlx::SqlitePool;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;

use super::rows::IndexError;
use super::rows::IndexSource;
use super::rows::IndexedVehicleRow;
use super::rows::MatchFilter;
use super::rows::MatchHit;
use super::rows::MatchOutcome;
use super::rows::ObjectiveMatch;
use super::rows::PlayerFacet;
use super::rows::ReplayRecord;
use super::rows::ShipFacet;
use super::rows::SourceId;
use super::rows::SourceKind;
use super::rows::VehicleRelation;

/// Return the id of the single `Live` source, creating it if absent.
pub async fn ensure_default_source(
    pool: &SqlitePool,
    root_path: &Path,
    now: Timestamp,
) -> Result<SourceId, IndexError> {
    let existing: Option<(i64,)> = sqlx::query_as("SELECT source_id FROM index_source WHERE kind = ?1 LIMIT 1")
        .bind(SourceKind::Live.as_db_str())
        .fetch_optional(pool)
        .await?;
    if let Some((id,)) = existing {
        return Ok(SourceId(id));
    }
    let id: (i64,) = sqlx::query_as(
        "INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4) RETURNING source_id",
    )
    .bind("Live replays")
    .bind(SourceKind::Live.as_db_str())
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
