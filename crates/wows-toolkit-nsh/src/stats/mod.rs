//! The Stats tab: the recorded session, or every replay in a directory,
//! filtered and aggregated per ship, with charts in a dock.
//!
//! The aggregation, the chart model and the plot layout live in
//! `wows_toolkit_viewmodel::stats`, shared with the GPUI and egui apps; this
//! module keeps each view's state, saves the session's settings in the rows
//! the other apps use, and draws the charts on frame surfaces.

mod chart;
mod layout;
mod scope;
mod snapshot;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;

use nsh_base::dock::DockAreaLayout;
use nsh_core::prelude::*;
use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart_model::ChartId;
use wows_toolkit_viewmodel::stats::chart_model::ChartModel;

pub use chart::ChartSnapshot;
pub use chart::StatChoice;
pub use chart::StatOption;
pub use snapshot::DivisionChoice;
pub use snapshot::FilterState;
pub use snapshot::StatsSnapshot;

use chart::Chart;
use chart::ChartImageError;
use chart::ChartPalette;
use layout::LayoutError;
use scope::Scope;
use scope::StoreError;

use crate::text::text;

/// The Stats tab's fixed text, in the reader's language.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct StatsText {
    pub overview: String,
    pub ships: String,
    pub limit_to_recent: String,
    pub limit_to_recent_tooltip: String,
    pub division: String,
    pub division_choices: Vec<String>,
    pub mode: String,
    pub all: String,
    pub add_chart: String,
    pub add_chart_choices: Vec<StatOption>,
    pub clear: String,
    pub clear_tooltip: String,
    pub clear_confirm: String,
    pub cancel: String,
    pub retry: String,
    pub keep_current_layout: String,
    pub no_games: String,
    pub search_ships: String,
    pub no_ship_matches: String,
    pub copy_table: String,
    pub copy_markdown: String,
    pub copy_csv: String,
    pub copied: String,
    pub clear_ship_confirm: String,
    pub achievements: String,
    pub column_ship: String,
    pub column_battles: String,
    pub column_win_rate: String,
    pub column_avg_damage: String,
    pub column_avg_frags: String,
    pub personal_rating: String,
    pub settings: String,
    pub statistic: String,
    pub chart: String,
    pub chart_line: String,
    pub chart_bar: String,
    pub options: String,
    pub combine_ships: String,
    pub running_average: String,
    pub value_labels: String,
    pub filters: String,
    pub own_filters: String,
    pub own_filters_tooltip: String,
    pub filter_ships: String,
    pub all_ships: String,
    pub no_ships: String,
    pub all_ships_tooltip: String,
    pub no_ships_tooltip: String,
    pub copy_image: String,
    pub copy_image_done: String,
    pub copy_image_failed: String,
    pub reset_view: String,
    pub nothing_to_plot: String,
}

/// A view's figures and charts, and whether saving the change failed.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ScopeView {
    pub stats: StatsSnapshot,
    pub charts: Vec<ChartSnapshot>,
    /// Why the change was made but not saved.
    pub save_error: Option<String>,
}

/// The session view as it opens.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct SessionView {
    pub view: ScopeView,
    pub layout: DockAreaLayout,
    /// Why the saved charts or layout could not be restored; the default
    /// layout shows instead.
    pub layout_error: Option<String>,
}

/// A chart after a change, and whether saving it failed.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ChartUpdate {
    pub chart: ChartSnapshot,
    pub save_error: Option<String>,
}

/// The theme colours charts are drawn in, as CSS colours.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ChartTheme {
    pub background: String,
    pub axis: String,
    pub text: String,
}

/// Why a Stats call failed.
#[derive(Debug, thiserror::Error)]
pub enum StatsError {
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Layout(#[from] LayoutError),
    #[error("{0}")]
    Image(#[from] ChartImageError),
    #[error("no stats view is open as {scope}")]
    UnknownScope { scope: String },
    #[error("chart {chart} is not open")]
    UnknownChart { chart: u32 },
    #[error("{value} is not a ship id")]
    ShipId { value: String },
    #[error("the session's games are kept; a directory view has nothing to clear")]
    NotClearable,
    #[error("{color} is not a CSS colour")]
    Color { color: String },
    #[error("the stats task did not finish: {reason}")]
    Task { reason: String },
}

/// Which view a call is about, as the TSX view names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ScopeKey {
    Session,
}

impl ScopeKey {
    fn parse(scope: &str) -> Result<Self, StatsError> {
        match scope {
            "session" => Ok(Self::Session),
            _ => Err(StatsError::UnknownScope { scope: scope.to_owned() }),
        }
    }
}

/// Every open view.
#[derive(Default)]
struct Scopes {
    by_key: HashMap<ScopeKey, Scope>,
}

static SCOPES: LazyLock<Mutex<Scopes>> = LazyLock::new(Mutex::default);

/// Runs `work` on the scope `scope` names.
fn with_scope<T>(scope: &str, work: impl FnOnce(&mut Scope) -> Result<T, StatsError>) -> Result<T, StatsError> {
    let key = ScopeKey::parse(scope)?;
    let mut scopes = SCOPES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(scope) = scopes.by_key.get_mut(&key) else {
        return Err(StatsError::UnknownScope { scope: scope.to_owned() });
    };
    work(scope)
}

/// Runs `work` on chart `chart` of `scope`.
fn with_chart<T>(scope: &str, chart: u32, work: impl FnOnce(&Chart) -> T) -> Result<T, StatsError> {
    with_scope(scope, |opened| {
        let id = ChartId(usize::try_from(chart).map_err(|_| StatsError::UnknownChart { chart })?);
        let found = opened.chart(id).ok_or(StatsError::UnknownChart { chart })?;
        Ok(work(found))
    })
}

/// Runs blocking `work` off the UI thread.
async fn off_thread<T>(work: impl FnOnce() -> Result<T, StatsError> + Send + 'static) -> Result<T, StatsError>
where
    T: Send + 'static,
{
    blocking(work).await.map_err(|error| StatsError::Task { reason: error.to_string() })?
}

/// The expected-values table the ratings are computed against, from the
/// cache the other front ends share.
fn personal_rating_table() -> Option<Arc<PersonalRatingData>> {
    match wows_toolkit_viewmodel::personal_rating::load_cached() {
        Ok(table) => Some(Arc::new(table)),
        Err(error) => {
            log::info!("stats: no personal rating table: {error}");
            None
        }
    }
}

/// Writes the session's charts if `scope` is the session; the error, if
/// any, is reported beside the change rather than undoing it.
fn persist_charts(scope: &str) -> Option<String> {
    let charts = with_scope(scope, |opened| Ok(opened.persists().then(|| opened.chart_settings()))).ok().flatten()?;
    scope::with_database(async |pool| scope::save_charts(pool, &charts).await).err().map(|error| error.to_string())
}

fn scope_view(scope: &Scope, save_error: Option<String>) -> ScopeView {
    ScopeView { stats: scope.snapshot(), charts: scope.chart_snapshots(), save_error }
}

fn chart_update(scope: String, chart: u32, change: impl FnOnce(&mut ChartModel) + Send + 'static) -> Result<ChartUpdate, StatsError> {
    let snapshot = with_chart(&scope, chart, |opened| {
        opened.update(change);
        opened.snapshot()
    })?;
    Ok(ChartUpdate { chart: snapshot, save_error: persist_charts(&scope) })
}

fn ship_id(value: &str) -> Result<GameParamId, StatsError> {
    value.parse::<u64>().map(GameParamId::from).map_err(|_| StatsError::ShipId { value: value.to_owned() })
}

/// Session statistics services exposed to the TSX interface.
#[module]
pub struct Stats;

#[module]
impl Stats {
    /// The tab's fixed text in the selected locale.
    pub fn text() -> StatsText {
        stats_text()
    }

    /// Reads the recorded session, its saved filters, charts and layout.
    pub async fn open_session() -> Result<SessionView, StatsError> {
        off_thread(open_session).await
    }

    /// Reads the recorded games again, keeping the filters and charts.
    pub async fn refresh_session() -> Result<ScopeView, StatsError> {
        off_thread(|| {
            let games = scope::with_database(async |pool| scope::load_games(pool).await)?;
            let table = personal_rating_table();
            with_scope("session", |session| {
                session.set_personal_rating(table);
                session.set_games(games);
                Ok(scope_view(session, None))
            })
        })
        .await
    }

    /// Applies the filter bar, saving it for the session.
    pub async fn set_filters(scope: String, filters: FilterState) -> Result<ScopeView, StatsError> {
        off_thread(move || {
            let (persists, applied) = with_scope(&scope, |opened| {
                opened.remember_count(&filters);
                opened.set_filters(Scope::filters_from(&filters));
                Ok((opened.persists(), opened.filters().clone()))
            })?;
            let save_error = persists
                .then(|| scope::with_database(async |pool| scope::save_filters(pool, &applied).await).err())
                .flatten()
                .map(|error| error.to_string());
            with_scope(&scope, |opened| Ok(scope_view(opened, save_error)))
        })
        .await
    }

    /// Forgets every recorded game.
    pub async fn clear_session() -> Result<ScopeView, StatsError> {
        off_thread(|| {
            // The rows go before the view forgets them: a failure would
            // otherwise show an empty session that comes back at the next
            // load.
            scope::with_database(async |pool| scope::clear_games(pool).await)?;
            with_scope("session", |session| {
                session.set_games(Vec::new());
                Ok(scope_view(session, None))
            })
        })
        .await
    }

    /// Forgets one ship's recorded games.
    pub async fn clear_ship(ship: String) -> Result<ScopeView, StatsError> {
        let ship = ship_id(&ship)?;
        off_thread(move || {
            let stored = i64::try_from(ship.raw()).map_err(|_| StatsError::ShipId { value: ship.raw().to_string() })?;
            scope::with_database(async |pool| scope::clear_ship(pool, stored).await)?;
            with_scope("session", |session| {
                let games = session.games().iter().filter(|game| game.ship_id != ship).cloned().collect();
                session.set_games(games);
                Ok(scope_view(session, None))
            })
        })
        .await
    }

    /// Opens a chart plotting `stat`.
    pub async fn add_chart(scope: String, stat: StatChoice) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            let snapshot = with_scope(&scope, |opened| {
                let id = opened.add_chart_for(stat.stat());
                opened.chart(id).map(Chart::snapshot).ok_or(StatsError::UnknownChart { chart: 0 })
            })?;
            Ok(ChartUpdate { chart: snapshot, save_error: persist_charts(&scope) })
        })
        .await
    }

    /// Lets go of a chart whose panel was closed.
    pub async fn remove_chart(scope: String, chart: u32) -> Result<Option<String>, StatsError> {
        off_thread(move || {
            let id = ChartId(usize::try_from(chart).map_err(|_| StatsError::UnknownChart { chart })?);
            let removed = with_scope(&scope, |opened| Ok(opened.remove_chart(id)))?;
            Ok(removed.then(|| persist_charts(&scope)).flatten())
        })
        .await
    }

    pub async fn chart_stat(scope: String, chart: u32, stat: StatChoice) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            chart_update(scope, chart, move |model| {
                model.set_stat(stat.stat());
            })
        })
        .await
    }

    pub async fn chart_mode(scope: String, chart: u32, line: bool) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            chart_update(scope, chart, move |model| {
                model.set_mode(if line { ChartMode::Line } else { ChartMode::Bar });
            })
        })
        .await
    }

    pub async fn chart_combined(scope: String, chart: u32, combined: bool) -> Result<ChartUpdate, StatsError> {
        off_thread(move || chart_update(scope, chart, move |model| model.set_combined(combined))).await
    }

    pub async fn chart_running(scope: String, chart: u32, running: bool) -> Result<ChartUpdate, StatsError> {
        off_thread(move || chart_update(scope, chart, move |model| model.set_running(running))).await
    }

    pub async fn chart_values(scope: String, chart: u32, show: bool) -> Result<ChartUpdate, StatsError> {
        off_thread(move || chart_update(scope, chart, move |model| model.set_show_values(show))).await
    }

    /// Takes the chart off the tab's filters, or puts it back on them.
    pub async fn chart_own_filters(scope: String, chart: u32, own: bool) -> Result<ChartUpdate, StatsError> {
        off_thread(move || chart_update(scope, chart, move |model| model.set_overrides_tab(own))).await
    }

    pub async fn chart_division(scope: String, chart: u32, division: DivisionChoice) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            chart_update(scope, chart, move |model| {
                model.set_own_division(division.filter());
            })
        })
        .await
    }

    pub async fn chart_toggle_mode(scope: String, chart: u32, mode: String) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            chart_update(scope, chart, move |model| {
                model.toggle_own_mode(&mode);
            })
        })
        .await
    }

    pub async fn chart_clear_modes(scope: String, chart: u32) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            chart_update(scope, chart, |model| {
                model.clear_own_modes();
            })
        })
        .await
    }

    pub async fn chart_toggle_ship(scope: String, chart: u32, ship: String) -> Result<ChartUpdate, StatsError> {
        let ship = ship_id(&ship)?;
        off_thread(move || chart_update(scope, chart, move |model| model.toggle_ship(ship))).await
    }

    /// Selects or deselects the listed ships at once.
    pub async fn chart_select_ships(
        scope: String,
        chart: u32,
        ships: Vec<String>,
        selected: bool,
    ) -> Result<ChartUpdate, StatsError> {
        let ships = ships.iter().map(|ship| ship_id(ship)).collect::<Result<Vec<_>, _>>()?;
        off_thread(move || chart_update(scope, chart, move |model| model.set_ships_selected(&ships, selected))).await
    }

    /// Puts the chart's view back on the whole plot.
    pub fn chart_reset_view(scope: String, chart: u32) -> Result<ChartSnapshot, StatsError> {
        with_chart(&scope, chart, |opened| {
            opened.update(|model| model.view_mut().reset());
            opened.snapshot()
        })
    }

    /// Resolves once the chart's view is moved away from, or back to, the
    /// whole plot: whether it differs from `moved`, the state the caller
    /// shows. Resolves with an error once the chart is closed.
    pub async fn chart_view_moved(scope: String, chart: u32, moved: bool) -> Result<bool, StatsError> {
        let mut changes = with_chart(&scope, chart, Chart::view_moved)?;
        let changed = changes.wait_for(|current| *current != moved).await.map(|current| *current);
        changed.map_err(|_| StatsError::UnknownChart { chart })
    }

    /// The chart as a PNG `data:` URL, for `Clipboard.writeImage`.
    pub async fn chart_image(scope: String, chart: u32) -> Result<String, StatsError> {
        off_thread(move || with_chart(&scope, chart, Chart::image)?.map_err(StatsError::from)).await
    }

    /// Draws every chart in the theme's colours.
    pub fn set_chart_theme(theme: ChartTheme) -> Result<(), StatsError> {
        let parse = |css: &str| chart::parse_color(css).ok_or_else(|| StatsError::Color { color: css.to_owned() });
        let axis = parse(&theme.axis)?;
        chart::set_palette(ChartPalette {
            background: parse(&theme.background)?,
            colors: wows_toolkit_viewmodel::stats::plot::Colors { axis, grid: axis.opacity(0.4), text: parse(&theme.text)? },
        });

        let scopes = SCOPES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for scope in scopes.by_key.values() {
            scope.redraw_charts();
        }
        Ok(())
    }

    /// Saves the dock arrangement the session view reports.
    pub async fn save_layout(layout: DockAreaLayout) -> Result<(), StatsError> {
        off_thread(move || {
            let state = layout::to_state(&layout)?;
            scope::with_database(async |pool| scope::save_layout(pool, &state).await)?;
            Ok(())
        })
        .await
    }

    /// Saves the open charts and the arrangement shown, replacing a saved
    /// pair that could not be restored.
    pub async fn keep_current_layout(layout: DockAreaLayout) -> Result<(), StatsError> {
        off_thread(move || {
            let state = layout::to_state(&layout)?;
            let charts = with_scope("session", |session| Ok(session.chart_settings()))?;
            scope::with_database(async |pool| {
                scope::save_charts(pool, &charts).await?;
                scope::save_layout(pool, &state).await
            })?;
            Ok(())
        })
        .await
    }

    /// `layout` with chart `chart` in the right dock, opened.
    pub fn layout_with_chart(layout: DockAreaLayout, chart: u32) -> DockAreaLayout {
        match usize::try_from(chart) {
            Ok(id) => layout::with_chart(layout, ChartId(id)),
            Err(_) => layout,
        }
    }

    /// `layout` without chart `chart`.
    pub fn layout_without_chart(layout: DockAreaLayout, chart: u32) -> DockAreaLayout {
        match usize::try_from(chart) {
            Ok(id) => layout::without_chart(layout, ChartId(id)),
            Err(_) => layout,
        }
    }

    /// `text` folded the way ship names are for search.
    pub fn search_key(text: String) -> String {
        wows_toolkit_viewmodel::search::normalize_ship_search(&text)
    }
}

fn open_session() -> Result<SessionView, StatsError> {
    let stored = scope::with_database(async |pool| scope::load(pool).await)?;
    let table = personal_rating_table();

    let (charts, layout_error) = match (stored.charts, stored.layout) {
        (Ok(charts), Ok(layout)) => match scope::restore(charts.clone(), layout.as_ref()) {
            Ok(restored) => (Ok(restored), None),
            Err(error) => {
                log::warn!("stats: the saved chart layout could not be restored: {error}");
                // The charts keep their settings; only the arrangement is
                // the default.
                let restored = scope::restore(charts, None)?;
                (Ok(restored), Some("Saved chart layout could not be restored".to_owned()))
            }
        },
        (Err(error), _) => (scope::restore(None, None), Some(format!("Could not read saved chart settings: {error}"))),
        (_, Err(error)) => (scope::restore(None, None), Some(format!("Could not read saved chart layout: {error}"))),
    };
    let restored = charts?;

    let mut session = Scope::new("session".to_owned(), true);
    session.set_personal_rating(table);
    session.set_filters(stored.filters);
    session.set_games(stored.games);
    for settings in restored.charts {
        session.add_chart(settings);
    }
    let view = scope_view(&session, None);

    let mut scopes = SCOPES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    scopes.by_key.insert(ScopeKey::Session, session);

    Ok(SessionView { view, layout: restored.layout, layout_error })
}

fn stats_text() -> StatsText {
    use wows_toolkit_viewmodel::stats::DivisionFilter;
    use wows_toolkit_viewmodel::stats::chart::ChartableStat;

    StatsText {
        overview: text("ui.stats.overview"),
        ships: text("ui.stats.ships"),
        limit_to_recent: text("ui.stats.limit_to_recent"),
        limit_to_recent_tooltip: text("ui.stats.limit_to_recent_tooltip"),
        division: text("ui.stats.division_label"),
        division_choices: DivisionFilter::ALL.iter().map(|choice| text(choice.translation_key())).collect(),
        mode: text("ui.stats.mode_label"),
        all: text("ui.stats.div_all"),
        add_chart: text("ui.stats.add_chart"),
        add_chart_choices: ChartableStat::ALL
            .iter()
            .map(|stat| StatOption { stat: StatChoice::from_stat(*stat), label: stat.label(), selected: false })
            .collect(),
        clear: text("ui.stats.clear"),
        clear_tooltip: text("ui.stats.clear_tooltip"),
        clear_confirm: text("confirm.clear_all_session_stats"),
        cancel: text("ui.buttons.cancel"),
        retry: text("ui.buttons.retry"),
        keep_current_layout: text("ui.stats.keep_current_layout"),
        no_games: text("ui.stats.no_games"),
        search_ships: text("ui.stats.search_ships"),
        no_ship_matches: text("ui.stats.no_ship_matches"),
        copy_table: text("ui.stats.copy_table"),
        copy_markdown: text("ui.stats.copy_markdown"),
        copy_csv: text("ui.stats.copy_csv"),
        copied: text("ui.stats.copied"),
        clear_ship_confirm: text("ui.stats.clear_ship_confirm"),
        achievements: text("ui.replay.sections.achievements"),
        column_ship: text("ui.stats.column_ship"),
        column_battles: text("ui.stats.column_battles"),
        column_win_rate: text("ui.stats.column_win_rate"),
        column_avg_damage: text("ui.stats.column_avg_damage"),
        column_avg_frags: text("ui.stats.column_avg_frags"),
        personal_rating: text("ui.stats.pr"),
        settings: text("ui.stats.settings"),
        statistic: text("ui.stats.statistic"),
        chart: text("ui.stats.chart"),
        chart_line: text("ui.stats.chart_line"),
        chart_bar: text("ui.stats.chart_bar"),
        options: text("ui.stats.options"),
        combine_ships: text("ui.stats.combine_ships"),
        running_average: text("ui.stats.running_average"),
        value_labels: text("ui.stats.value_labels"),
        filters: text("ui.stats.filters"),
        own_filters: text("ui.stats.own_filters"),
        own_filters_tooltip: text("ui.stats.own_filters_tooltip"),
        filter_ships: text("ui.stats.filter_ships"),
        all_ships: text("ui.stats.all_ships"),
        no_ships: text("ui.stats.no_ships"),
        all_ships_tooltip: text("ui.stats.all_ships_tooltip"),
        no_ships_tooltip: text("ui.stats.no_ships_tooltip"),
        copy_image: text("ui.stats.copy_image"),
        copy_image_done: text("ui.stats.copy_image_done"),
        copy_image_failed: text("ui.stats.copy_image_failed"),
        reset_view: text("ui.stats.reset_view"),
        nothing_to_plot: text("ui.stats.nothing_to_plot"),
    }
}
