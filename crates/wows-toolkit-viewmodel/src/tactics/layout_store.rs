//! The capture layouts a walk of the replays recorded, kept in the settings
//! database so the walk is paid for once.

use wows_replay_insights::cap_layout::CapLayout;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_replay_insights::cap_layout::CapLayoutKey;

/// Why a layout could not be saved.
#[derive(Debug, thiserror::Error)]
pub enum LayoutStoreError {
    #[error("the capture layout for map {map_id} could not be encoded: {source}")]
    Encode {
        map_id: u32,
        #[source]
        source: rkyv::rancor::Error,
    },
    #[error("the capture layout for map {map_id} could not be written: {source}")]
    Write {
        map_id: u32,
        #[source]
        source: sqlx::Error,
    },
    #[error("the capture layouts could not be read: {source}")]
    Read {
        #[source]
        source: sqlx::Error,
    },
}

/// Every recorded layout. A row that will not read is skipped and logged: the
/// rest still offer their modes.
pub async fn load_from_db(pool: &sqlx::SqlitePool) -> Result<CapLayoutDb, LayoutStoreError> {
    let rows = wows_toolkit_config::queries::get_all_cap_layouts(pool)
        .await
        .map_err(|source| LayoutStoreError::Read { source })?;

    let mut db = CapLayoutDb::default();
    for (map_id, scenario_config_id, blob) in rows {
        let (Ok(map_id), Ok(scenario_config_id)) = (u32::try_from(map_id), u32::try_from(scenario_config_id)) else {
            tracing::warn!("skipping cap layout with out of range key ({map_id}, {scenario_config_id})");
            continue;
        };
        let Ok(layout) = rkyv::from_bytes::<CapLayout, rkyv::rancor::Error>(&blob).inspect_err(|err| {
            tracing::warn!("failed to deserialize cap layout ({map_id}, {scenario_config_id}): {err}")
        }) else {
            continue;
        };
        db.layouts.insert(CapLayoutKey { map_id, scenario_config_id }, layout);
    }
    tracing::info!("loaded {} cap layouts from SQLite", db.len());

    Ok(db)
}

/// Writes one layout, leaving every other row as it is.
pub async fn save_layout_to_db(pool: &sqlx::SqlitePool, layout: &CapLayout) -> Result<(), LayoutStoreError> {
    let map_id = layout.key.map_id;
    let blob =
        rkyv::to_bytes::<rkyv::rancor::Error>(layout).map_err(|source| LayoutStoreError::Encode { map_id, source })?;

    wows_toolkit_config::queries::upsert_cap_layout(pool, map_id as i64, layout.key.scenario_config_id as i64, &blob)
        .await
        .map_err(|source| LayoutStoreError::Write { map_id, source })
}
