//! Reading the session's saved charts, including the one-time import of the charts kept in the
//! legacy `chart_configs` table.

use serde::Deserialize;
use sqlx::SqlitePool;
use wows_toolkit_config::queries;

use crate::stats::chart::ChartMode;
use crate::stats::chart::ChartSettings;
use crate::stats::chart::ChartableStat;

/// The settings row the session's charts are kept in.
pub const CHARTS_SETTINGS_KEY: &str = "stats_charts";

#[derive(Debug, thiserror::Error)]
pub enum LoadChartsError {
    #[error("the saved charts could not be read")]
    Read(#[source] queries::SettingError),
    #[error("the egui charts could not be read")]
    ReadEgui(#[source] sqlx::Error),
    #[error("the imported charts could not be stored")]
    Store(#[source] sqlx::Error),
}

/// The serialized shape of one legacy `chart_configs` row.
#[derive(Debug, Deserialize)]
struct EguiChartConfig {
    selected_stat: ChartableStat,
    mode: ChartMode,
    /// Ship names, which name no ship id and are dropped.
    #[serde(default)]
    selected_ships: Vec<serde_json::Value>,
    selected_ships_manually_changed: bool,
    rolling_average: bool,
    #[serde(default)]
    combined: bool,
    show_labels: bool,
}

impl From<EguiChartConfig> for ChartSettings {
    fn from(config: EguiChartConfig) -> Self {
        Self {
            id: None,
            stat: config.selected_stat,
            mode: config.mode,
            running: config.rolling_average,
            combined: config.combined,
            show_values: config.show_labels,
            own_filters: None,
            selected_ships: config.selected_ships.iter().filter_map(serde_json::Value::as_u64).collect(),
            selection_touched: config.selected_ships_manually_changed,
        }
    }
}

/// The session's saved charts, or `None` when none were saved.
///
/// Without a saved list, the legacy charts are read in the order they were made and stored as
/// the list, so later launches read the list and never the legacy table again.
pub async fn load(pool: &SqlitePool) -> Result<Option<Vec<ChartSettings>>, LoadChartsError> {
    let saved = queries::try_get_setting::<Vec<ChartSettings>>(pool, CHARTS_SETTINGS_KEY)
        .await
        .map_err(LoadChartsError::Read)?;
    if saved.is_some() {
        return Ok(saved);
    }

    let mut rows = queries::get_all_chart_configs(pool).await.map_err(LoadChartsError::ReadEgui)?;
    if rows.is_empty() {
        return Ok(None);
    }

    rows.sort_by_key(|(chart_id, _)| *chart_id);
    let charts: Vec<ChartSettings> = rows
        .into_iter()
        .map(|(chart_id, json)| match serde_json::from_str::<EguiChartConfig>(&json) {
            Ok(config) => config.into(),
            // An unreadable row becomes a default chart.
            Err(err) => {
                tracing::warn!("egui chart {chart_id} is imported as a default chart: {err}");
                ChartSettings::default()
            }
        })
        .collect();

    // A list saved while this ran wins over the import.
    let inserted =
        queries::insert_setting_if_absent(pool, CHARTS_SETTINGS_KEY, &charts).await.map_err(LoadChartsError::Store)?;
    if inserted {
        return Ok(Some(charts));
    }

    queries::try_get_setting::<Vec<ChartSettings>>(pool, CHARTS_SETTINGS_KEY).await.map_err(LoadChartsError::Read)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"selected_stat":"Frags","mode":"Line","selected_ships":[4181604048,"Yamato"],
        "selected_ships_manually_changed":true,"rolling_average":true,"combined":true,"show_labels":false}"#;
    const BAR: &str = r#"{"selected_stat":"WinRate","mode":"Bar","selected_ships_manually_changed":false,
        "rolling_average":false,"show_labels":true}"#;

    #[tokio::test]
    async fn egui_charts_are_imported_once_in_order() {
        let pool = wows_toolkit_config::test_pool().await;
        queries::upsert_chart_config(&pool, 9, BAR).await.unwrap();
        queries::upsert_chart_config(&pool, 2, LINE).await.unwrap();
        queries::upsert_chart_config(&pool, 5, "not json").await.unwrap();

        let imported = load(&pool).await.unwrap().expect("the egui charts are imported");
        let line = ChartSettings {
            id: None,
            stat: ChartableStat::Frags,
            mode: ChartMode::Line,
            running: true,
            combined: true,
            show_values: false,
            own_filters: None,
            selected_ships: vec![4_181_604_048],
            selection_touched: true,
        };
        let bar = ChartSettings {
            id: None,
            stat: ChartableStat::WinRate,
            mode: ChartMode::Bar,
            running: false,
            combined: false,
            show_values: true,
            own_filters: None,
            selected_ships: Vec::new(),
            selection_touched: false,
        };
        let expected = vec![line, ChartSettings::default(), bar];
        assert_eq!(imported, expected, "in egui's order, the unreadable row as a default");

        queries::upsert_chart_config(&pool, 11, LINE).await.unwrap();
        assert_eq!(load(&pool).await.unwrap(), Some(imported), "a later egui chart is not imported again");
    }

    #[tokio::test]
    async fn saved_charts_win_over_egui_charts() {
        let pool = wows_toolkit_config::test_pool().await;
        let saved = vec![ChartSettings { stat: ChartableStat::BaseXp, ..ChartSettings::default() }];
        queries::set_setting(&pool, CHARTS_SETTINGS_KEY, &saved).await.unwrap();
        queries::upsert_chart_config(&pool, 1, LINE).await.unwrap();

        assert_eq!(load(&pool).await.unwrap(), Some(saved));
    }

    #[tokio::test]
    async fn no_charts_anywhere_is_none() {
        let pool = wows_toolkit_config::test_pool().await;

        assert_eq!(load(&pool).await.unwrap(), None);
    }
}
