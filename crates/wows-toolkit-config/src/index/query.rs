//! Typed query API for the replay index. Populated in later tasks.

use std::path::Path;

use jiff::Timestamp;
use sqlx::SqlitePool;

use super::rows::IndexError;
use super::rows::IndexSource;
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
