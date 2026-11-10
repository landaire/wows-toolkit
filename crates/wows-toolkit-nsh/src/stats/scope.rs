//! The games one Stats view looks at, the filters over them and the charts
//! drawn from them, and the settings rows the session's are kept in.

use std::collections::BTreeMap;
use std::sync::Arc;

use nsh_base::dock::DockAreaLayout;
use nsh_base::dock::DockAreaState;
use sqlx::SqlitePool;
use wows_toolkit_config::queries;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::GameLimit;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::chart_model::ChartId;
use wows_toolkit_viewmodel::stats::chart_model::ChartModel;
use wows_toolkit_viewmodel::stats::chart_model::ChartSettings;
use wows_toolkit_viewmodel::stats::setting_keys;

use super::chart::Chart;
use super::chart::ChartSnapshot;
use super::chart::surface_id;
use super::layout;
use super::layout::CHARTS_DOCK_LAYOUT_KEY;
use super::layout::CHARTS_SETTINGS_KEY;
use super::layout::LayoutError;
use super::snapshot::FilterState;
use super::snapshot::StatsSnapshot;
use super::snapshot::snapshot;

/// Games shown when the limit is switched on without a saved count. The egui
/// drag value opens at this and clamps to 1..=999.
pub(crate) const DEFAULT_GAME_LIMIT: usize = 25;
const MIN_GAME_LIMIT: usize = 1;
const MAX_GAME_LIMIT: usize = 999;

/// Why the session's rows could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not open the settings database: {source}")]
    Open { source: sqlx::Error },
    #[error("could not read the recorded games: {source}")]
    ReadGames { source: sqlx::Error },
    #[error("could not read setting {key}: {source}")]
    ReadSetting { key: &'static str, source: queries::SettingError },
    #[error("could not save setting {key}: {source}")]
    WriteSetting { key: &'static str, source: sqlx::Error },
    #[error("could not forget the recorded games: {source}")]
    Clear { source: sqlx::Error },
    #[error("could not start the database runtime: {source}")]
    Runtime { source: std::io::Error },
}

/// The games of one view and everything drawn from them.
pub(crate) struct Scope {
    /// Names the scope's chart surfaces.
    name: String,
    /// Whether the filters, charts and layout are saved and the games can be
    /// cleared. Only the session's are: a directory view is not kept.
    persists: bool,
    /// In `sort_key` order, oldest first.
    games: Vec<PerGameStat>,
    filters: StatsFilters,
    /// The count the limit returns to when it is switched back on.
    last_count: usize,
    personal_rating: Option<Arc<PersonalRatingData>>,
    charts: BTreeMap<ChartId, Chart>,
    next_chart: ChartId,
}

impl Scope {
    pub(crate) fn new(name: String, persists: bool) -> Self {
        Self {
            name,
            persists,
            games: Vec::new(),
            filters: StatsFilters::default(),
            last_count: DEFAULT_GAME_LIMIT,
            personal_rating: None,
            charts: BTreeMap::new(),
            next_chart: ChartId(0),
        }
    }

    pub(crate) fn persists(&self) -> bool {
        self.persists
    }

    pub(crate) fn games(&self) -> &[PerGameStat] {
        &self.games
    }

    pub(crate) fn filters(&self) -> &StatsFilters {
        &self.filters
    }

    /// Adopts `games`, keeping the filters and charts.
    pub(crate) fn set_games(&mut self, games: Vec<PerGameStat>) {
        self.games = games;
        self.prune_modes();
        self.push_games();
    }

    pub(crate) fn set_personal_rating(&mut self, table: Option<Arc<PersonalRatingData>>) {
        self.personal_rating = table;
        for chart in self.charts.values() {
            chart.update(|model| model.set_personal_rating(self.personal_rating.clone()));
        }
    }

    /// Adopts the filters a saved row or the filter bar gives.
    pub(crate) fn set_filters(&mut self, filters: StatsFilters) {
        self.filters = filters;
        if let GameLimit::Recent(count) = self.filters.limit {
            self.last_count = count.clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT);
            self.filters.limit = GameLimit::Recent(self.last_count);
        }
        self.prune_modes();
        self.push_games();
    }

    /// The filters the bar's state describes. A count outside the egui
    /// slider's range is clamped into it rather than refused, which is what
    /// the GPUI stepper does.
    pub(crate) fn filters_from(state: &FilterState) -> StatsFilters {
        let count = usize::try_from(state.limit_count).unwrap_or(MAX_GAME_LIMIT).clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT);
        StatsFilters {
            limit: if state.limit_enabled { GameLimit::Recent(count) } else { GameLimit::All },
            division: state.division.filter(),
            game_modes: state.game_modes.iter().cloned().collect(),
        }
    }

    /// The bar's state keeps the count while the limit is off, which the
    /// filters themselves cannot.
    pub(crate) fn remember_count(&mut self, state: &FilterState) {
        if let Ok(count) = usize::try_from(state.limit_count) {
            self.last_count = count.clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT);
        }
    }

    /// A mode the games no longer hold cannot stay chosen: it would hide
    /// everything with no button left to undo it.
    fn prune_modes(&mut self) {
        let present = wows_toolkit_viewmodel::stats::all_match_groups(&self.games);
        self.filters.game_modes.retain(|mode| present.contains(mode));
    }

    /// Hands every chart the games and the bar's filters.
    fn push_games(&self) {
        for chart in self.charts.values() {
            chart.update(|model| model.set_games(&self.games, &self.filters));
        }
    }

    pub(crate) fn snapshot(&self) -> StatsSnapshot {
        snapshot(&self.games, &self.filters, self.last_count, self.personal_rating.as_deref())
    }

    pub(crate) fn chart_snapshots(&self) -> Vec<ChartSnapshot> {
        self.charts.values().map(Chart::snapshot).collect()
    }

    pub(crate) fn chart(&self, id: ChartId) -> Option<&Chart> {
        self.charts.get(&id)
    }

    /// Opens a chart set up the way `settings` says, under the id it names or
    /// the next free one.
    pub(crate) fn add_chart(&mut self, settings: ChartSettings) -> ChartId {
        let id = settings.id.unwrap_or(self.next_chart);
        self.next_chart = self.next_chart.max(id.next());

        let mut model = ChartModel::new(id);
        // Before the games, so the narrowing is in place the first time they
        // are filtered.
        model.apply_settings(ChartSettings { id: Some(id), ..settings });
        model.set_personal_rating(self.personal_rating.clone());
        model.set_games(&self.games, &self.filters);
        self.charts.insert(id, Chart::start(model, surface_id(&self.name, id)));
        id
    }

    /// Opens a chart plotting `stat`, as the Add-chart menu asks.
    pub(crate) fn add_chart_for(&mut self, stat: ChartableStat) -> ChartId {
        self.add_chart(ChartSettings { stat, ..ChartSettings::default() })
    }

    /// Lets go of a chart whose panel was closed.
    pub(crate) fn remove_chart(&mut self, id: ChartId) -> bool {
        self.charts.remove(&id).is_some()
    }

    /// Draws every chart again, as after a theme change.
    pub(crate) fn redraw_charts(&self) {
        for chart in self.charts.values() {
            chart.redraw();
        }
    }

    /// The open charts as the settings row stores them.
    pub(crate) fn chart_settings(&self) -> Vec<ChartSettings> {
        self.charts.values().map(|chart| chart.read(ChartModel::settings)).collect()
    }
}

/// What the session's settings rows hold.
pub(crate) struct Stored {
    pub(crate) games: Vec<PerGameStat>,
    pub(crate) filters: StatsFilters,
    pub(crate) charts: Result<Option<Vec<ChartSettings>>, StoreError>,
    pub(crate) layout: Result<Option<DockAreaState>, StoreError>,
}

/// The charts and the layout a session view opens with.
pub(crate) struct Restored {
    pub(crate) charts: Vec<ChartSettings>,
    pub(crate) layout: DockAreaLayout,
}

/// The charts and layout the saved rows describe, as the GPUI tab restores
/// them (`StatsView::load_charts`).
///
/// A saved layout decides which charts open; a saved chart the layout does
/// not place is added beside the others. Without a layout the charts open in
/// the right dock. A saved empty list means every chart was closed. Nothing
/// saved opens one damage chart.
pub(crate) fn restore(
    charts: Option<Vec<ChartSettings>>,
    saved_layout: Option<&DockAreaState>,
) -> Result<Restored, LayoutError> {
    let Some(state) = saved_layout else {
        let charts = match charts {
            None => vec![ChartSettings { id: Some(ChartId(0)), ..ChartSettings::default() }],
            Some(saved) => with_ids(saved),
        };
        let ids: Vec<ChartId> = charts.iter().filter_map(|chart| chart.id).collect();
        return Ok(Restored { layout: layout::default_layout(&ids), charts });
    };

    let mut restored_layout = layout::from_state(state)?;
    let placed = layout::charts(&restored_layout)?;
    let saved = charts.map(with_ids).unwrap_or_default();

    let mut restored: Vec<ChartSettings> = placed
        .iter()
        .map(|id| {
            saved
                .iter()
                .find(|settings| settings.id == Some(*id))
                .cloned()
                .unwrap_or(ChartSettings { id: Some(*id), ..ChartSettings::default() })
        })
        .collect();
    for settings in saved {
        let Some(id) = settings.id else { continue };
        if placed.contains(&id) {
            continue;
        }
        restored_layout = layout::with_chart(restored_layout, id);
        restored.push(settings);
    }

    Ok(Restored { charts: restored, layout: restored_layout })
}

/// Saved charts with an id each: a row written before ids existed is
/// numbered by position, as GPUI numbers it.
fn with_ids(saved: Vec<ChartSettings>) -> Vec<ChartSettings> {
    let taken: Vec<ChartId> = saved.iter().filter_map(|settings| settings.id).collect();
    let mut next = taken.iter().copied().max().map_or(ChartId(0), ChartId::next);
    saved
        .into_iter()
        .enumerate()
        .map(|(index, settings)| {
            let id = match settings.id {
                Some(id) => id,
                // The first chart is the one the tab always opens with.
                None if index == 0 && !taken.contains(&ChartId(0)) => ChartId(0),
                None => {
                    let id = next;
                    next = next.next();
                    id
                }
            };
            ChartSettings { id: Some(id), ..settings }
        })
        .collect()
}

/// Runs `work` against the shared settings database on a runtime of its own.
pub(crate) fn with_database<T>(
    work: impl AsyncFnOnce(&SqlitePool) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| StoreError::Runtime { source })?;
    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(|source| StoreError::Open { source })?;
        work(&pool).await
    })
}

/// Reads the session's games and saved rows.
pub(crate) async fn load(pool: &SqlitePool) -> Result<Stored, StoreError> {
    let rows = queries::get_all_session_stats(pool).await.map_err(|source| StoreError::ReadGames { source })?;
    let games = rows.into_iter().map(PerGameStat::from_row).collect();

    Ok(Stored {
        games,
        filters: load_filters(pool).await?,
        charts: read::<Vec<ChartSettings>>(pool, CHARTS_SETTINGS_KEY).await,
        layout: read::<DockAreaState>(pool, CHARTS_DOCK_LAYOUT_KEY).await,
    })
}

/// Reads the games again, for a refresh.
pub(crate) async fn load_games(pool: &SqlitePool) -> Result<Vec<PerGameStat>, StoreError> {
    let rows = queries::get_all_session_stats(pool).await.map_err(|source| StoreError::ReadGames { source })?;
    Ok(rows.into_iter().map(PerGameStat::from_row).collect())
}

/// Reads the filter bar's saved state, from the rows the egui and GPUI apps
/// write. Each missing row is that control's own default: no limit, every
/// division and every mode.
async fn load_filters(pool: &SqlitePool) -> Result<StatsFilters, StoreError> {
    let limit_enabled = read::<bool>(pool, setting_keys::LIMIT_ENABLED).await?.unwrap_or(false);
    let game_count = read::<usize>(pool, setting_keys::GAME_COUNT).await?;
    let limit = match (limit_enabled, game_count) {
        (true, Some(count)) if count > 0 => GameLimit::Recent(count),
        // A saved count of zero would show nothing at all; the egui slider
        // cannot produce one, so it means the unlimited it reads as.
        _ => GameLimit::All,
    };

    Ok(StatsFilters {
        limit,
        division: read::<DivisionFilter>(pool, setting_keys::DIVISION_FILTER).await?.unwrap_or_default(),
        game_modes: read(pool, setting_keys::GAME_MODE_FILTER).await?.unwrap_or_default(),
    })
}

/// Writes the filter bar's state to the rows `load_filters` reads.
pub(crate) async fn save_filters(pool: &SqlitePool, filters: &StatsFilters) -> Result<(), StoreError> {
    let (limit_enabled, count) = match filters.limit {
        GameLimit::All => (false, None),
        GameLimit::Recent(count) => (true, Some(count)),
    };
    write(pool, setting_keys::LIMIT_ENABLED, &limit_enabled).await?;
    if let Some(count) = count {
        write(pool, setting_keys::GAME_COUNT, &count).await?;
    }
    write(pool, setting_keys::DIVISION_FILTER, &filters.division).await?;
    write(pool, setting_keys::GAME_MODE_FILTER, &filters.game_modes).await
}

pub(crate) async fn save_charts(pool: &SqlitePool, charts: &[ChartSettings]) -> Result<(), StoreError> {
    write(pool, CHARTS_SETTINGS_KEY, &charts).await
}

pub(crate) async fn save_layout(pool: &SqlitePool, layout: &DockAreaState) -> Result<(), StoreError> {
    write(pool, CHARTS_DOCK_LAYOUT_KEY, layout).await
}

pub(crate) async fn clear_games(pool: &SqlitePool) -> Result<(), StoreError> {
    queries::clear_session_stats(pool).await.map_err(|source| StoreError::Clear { source })?;
    Ok(())
}

pub(crate) async fn clear_ship(pool: &SqlitePool, ship_id: i64) -> Result<(), StoreError> {
    queries::clear_session_stats_for_ship(pool, ship_id).await.map_err(|source| StoreError::Clear { source })?;
    Ok(())
}

async fn read<T>(pool: &SqlitePool, key: &'static str) -> Result<Option<T>, StoreError>
where
    T: serde::de::DeserializeOwned,
{
    queries::try_get_setting(pool, key).await.map_err(|source| StoreError::ReadSetting { key, source })
}

async fn write<T>(pool: &SqlitePool, key: &'static str, value: &T) -> Result<(), StoreError>
where
    T: serde::Serialize,
{
    queries::set_setting(pool, key, value).await.map_err(|source| StoreError::WriteSetting { key, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wows_toolkit_viewmodel::stats::chart::ChartMode;

    #[test]
    fn nothing_saved_opens_one_damage_chart_on_the_right() {
        let restored = restore(None, None).expect("the default restores");

        assert_eq!(restored.charts.len(), 1);
        assert_eq!(restored.charts[0].stat, ChartableStat::Damage);
        assert_eq!(layout::charts(&restored.layout).expect("charts"), [ChartId(0)]);
    }

    #[test]
    fn a_saved_empty_list_opens_no_chart() {
        let restored = restore(Some(Vec::new()), None).expect("restores");

        assert!(restored.charts.is_empty());
        assert!(restored.layout.right.is_none());
    }

    #[test]
    fn a_saved_layout_decides_the_charts_and_keeps_their_settings() {
        let layout = layout::default_layout(&[ChartId(3)]);
        let state = layout::to_state(&layout).expect("the layout writes");
        let saved = vec![
            ChartSettings { id: Some(ChartId(3)), mode: ChartMode::Bar, ..ChartSettings::default() },
            ChartSettings { id: Some(ChartId(5)), stat: ChartableStat::Frags, ..ChartSettings::default() },
        ];

        let restored = restore(Some(saved), Some(&state)).expect("restores");

        assert_eq!(restored.charts.iter().map(|chart| chart.id).collect::<Vec<_>>(), [Some(ChartId(3)), Some(ChartId(5))]);
        assert_eq!(restored.charts[0].mode, ChartMode::Bar);
        assert_eq!(layout::charts(&restored.layout).expect("charts"), [ChartId(3), ChartId(5)]);
    }

    #[test]
    fn rows_without_ids_are_numbered_by_position() {
        let saved = vec![ChartSettings::default(), ChartSettings::default(), ChartSettings { id: Some(ChartId(4)), ..ChartSettings::default() }];

        let numbered = with_ids(saved);

        assert_eq!(numbered.iter().map(|chart| chart.id).collect::<Vec<_>>(), [Some(ChartId(0)), Some(ChartId(5)), Some(ChartId(4))]);
    }

    #[test]
    fn the_bar_state_round_trips_and_clamps_the_count() {
        let state = FilterState {
            limit_enabled: true,
            limit_count: 5_000,
            division: super::super::snapshot::DivisionChoice::Solo,
            game_modes: vec!["pvp".to_owned()],
        };

        let filters = Scope::filters_from(&state);

        assert_eq!(filters.limit, GameLimit::Recent(999));
        assert_eq!(filters.division, DivisionFilter::SoloOnly);
        assert!(filters.game_modes.contains("pvp"));
    }
}
