use tracing::warn;
use wows_replay_insights::cap_layout::{CapLayout, CapLayoutDb, CapLayoutKey};

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

pub async fn save_layout_to_db(
    pool: &sqlx::SqlitePool,
    layout: &CapLayout,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let blob = rkyv::to_bytes::<rkyv::rancor::Error>(layout).map_err(|err| format!("{err}"))?;
    wows_toolkit_config::queries::upsert_cap_layout(
        pool,
        layout.key.map_id as i64,
        layout.key.scenario_config_id as i64,
        &blob,
    )
    .await?;
    Ok(())
}
