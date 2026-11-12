//! The capture-point layouts the tactics board places caps from, kept in the
//! shared config database. Both functions are futures for the tokio runtime the
//! store runs on.

use tracing::warn;
use wows_replay_insights::cap_layout::CapLayout;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_replay_insights::cap_layout::CapLayoutKey;

/// Why a layout was not saved.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("the cap layout could not be serialized: {0}")]
    Serialize(#[source] rkyv::rancor::Error),
    #[error("the cap layout could not be written: {0}")]
    Write(#[source] sqlx::Error),
}

/// Every stored layout. A row that cannot be read is logged and left out.
pub async fn load_from_db(pool: &sqlx::SqlitePool) -> CapLayoutDb {
    let mut db = CapLayoutDb::default();
    let Ok(rows) = wows_toolkit_config::queries::get_all_cap_layouts(pool)
        .await
        .inspect_err(|err| warn!("failed to load cap layouts from SQLite: {err}"))
    else {
        return db;
    };

    for (map_id, scenario_config_id, blob) in rows {
        let Ok(layout) = rkyv::from_bytes::<CapLayout, rkyv::rancor::Error>(&blob)
            .inspect_err(|err| warn!("failed to deserialize cap layout ({map_id}, {scenario_config_id}): {err}"))
        else {
            continue;
        };
        db.layouts
            .insert(CapLayoutKey { map_id: map_id as u32, scenario_config_id: scenario_config_id as u32 }, layout);
    }

    tracing::info!("loaded {} cap layouts from SQLite", db.len());
    db
}

pub async fn save_layout_to_db(pool: &sqlx::SqlitePool, layout: &CapLayout) -> Result<(), SaveError> {
    let blob = rkyv::to_bytes::<rkyv::rancor::Error>(layout).map_err(SaveError::Serialize)?;

    wows_toolkit_config::queries::upsert_cap_layout(
        pool,
        layout.key.map_id as i64,
        layout.key.scenario_config_id as i64,
        &blob,
    )
    .await
    .map_err(SaveError::Write)
}
