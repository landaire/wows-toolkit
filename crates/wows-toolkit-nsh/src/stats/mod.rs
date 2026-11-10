//! The Stats tab: the recorded session, or every replay in a directory,
//! filtered and aggregated per ship, with charts in a dock.
//!
//! The aggregation, the chart model and the plot layout live in
//! `wows_toolkit_viewmodel::stats`, shared with the GPUI and egui apps; this
//! module keeps each view's state, saves the session's settings in the rows
//! the other apps use, and draws the charts on frame surfaces.

mod art;
mod chart;
mod directory;
mod error;
mod ids;
mod layout;
mod scope;
mod snapshot;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use nsh_base::dock::DockAreaLayout;
use nsh_core::prelude::*;
use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::chart_model::ChartModel;

use chart::Chart;
use chart::ChartPalette;
use directory::DirectoryId;
use directory::DirectoryRead;
use directory::DirectoryRevision;
use error::RestoreError;
use error::StatsError;
use error::StoreError;
use ids::MatchGroup;
use ids::ParamIdText;
use ids::ScopeName;
use ids::ViewChartId;
use scope::Scope;
use crate::text::text;

pub use chart::ChartSnapshot;
pub use chart::StatChoice;
pub use chart::StatOption;
pub use directory::DirectoryStatus;
pub use snapshot::DivisionChoice;
pub use snapshot::FilterState;
pub use snapshot::StatsSnapshot;

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
    pub directory_reload: String,
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

/// Which view a call is about, as the TSX view names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ScopeKey {
    Session,
    Directory(DirectoryId),
}

impl ScopeKey {
    fn parse(scope: &ScopeName) -> Result<Self, StatsError> {
        if scope.is_session() {
            return Ok(Self::Session);
        }

        DirectoryId::parse(scope).map(Self::Directory).ok_or_else(|| StatsError::UnknownScope { scope: scope.clone() })
    }
}

/// Every open view.
struct Scopes {
    by_key: HashMap<ScopeKey, Scope>,
    directories: HashMap<DirectoryId, DirectoryRead>,
    next_directory: DirectoryId,
}

impl Scopes {
    fn new() -> Self {
        Self { by_key: HashMap::new(), directories: HashMap::new(), next_directory: DirectoryId::FIRST }
    }
}

/// Runs `work` on directory view `id` and its read.
fn with_directory<T>(
    id: DirectoryId,
    work: impl FnOnce(&mut Scope, &mut DirectoryRead) -> Result<T, StatsError>,
) -> Result<T, StatsError> {
    let mut scopes = SCOPES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let scopes = &mut *scopes;
    let unknown = || StatsError::UnknownScope { scope: id.scope_name() };
    let read = scopes.directories.get_mut(&id).ok_or_else(unknown)?;
    let scope = scopes.by_key.get_mut(&ScopeKey::Directory(id)).ok_or_else(unknown)?;
    work(scope, read)
}

/// A directory view as it opens.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct DirectoryOpened {
    pub scope: ScopeName,
    pub layout: DockAreaLayout,
    pub status: DirectoryStatus,
}

/// Opens a stats view over `root`, or returns the one already open for it.
fn open_directory(root: PathBuf) -> Result<DirectoryOpened, StatsError> {
    let game_data = crate::game_data::cache().ok_or(StatsError::GameDataUnavailable)?;
    let root = wows_toolkit_replay_data::directories::canonical_root(root);

    let (id, fresh) = {
        let mut scopes = SCOPES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match scopes.directories.iter().find(|(_, read)| *read.root() == root).map(|(id, _)| *id) {
            Some(id) => (id, false),
            None => {
                let id = scopes.next_directory;
                scopes.next_directory = id.next();
                scopes.directories.insert(id, DirectoryRead::new(root));
                scopes.by_key.insert(ScopeKey::Directory(id), directory::new_scope(id));
                (id, true)
            }
        }
    };
    if fresh {
        directory::start(id, game_data);
    }

    let (status, charts) = with_directory(id, |scope, read| Ok((read.status(scope_view(scope, None)), scope.chart_ids())))?;
    Ok(DirectoryOpened { scope: id.scope_name(), layout: layout::default_layout(&charts), status })
}

static SCOPES: LazyLock<Mutex<Scopes>> = LazyLock::new(|| Mutex::new(Scopes::new()));

/// Held across reading the session's state and writing it, so the rows
/// always end with the latest state: a write that waited reads what the one
/// before it already changed.
static WRITES: Mutex<()> = Mutex::new(());

/// One state of a `GenerationCounter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Generation(u64);

/// Counts changes of one kind, so work that read an older state can tell it
/// is stale.
struct GenerationCounter(AtomicU64);

impl GenerationCounter {
    const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    fn current(&self) -> Generation {
        Generation(self.0.load(Ordering::Acquire))
    }

    /// Moves past every earlier generation and returns the new one.
    fn advance(&self) -> Generation {
        Generation(self.0.fetch_add(1, Ordering::AcqRel) + 1)
    }
}

/// Advanced by every clear, so a refresh that read the rows before it does
/// not put the cleared games back.
static SESSION_GENERATION: GenerationCounter = GenerationCounter::new();

/// Advanced by every layout save, so one that waited behind a newer one is
/// dropped.
static LAYOUT_GENERATION: GenerationCounter = GenerationCounter::new();

fn hold_writes() -> std::sync::MutexGuard<'static, ()> {
    WRITES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs `work` on the scope `scope` names.
fn with_scope<T>(scope: &ScopeName, work: impl FnOnce(&mut Scope) -> Result<T, StatsError>) -> Result<T, StatsError> {
    let key = ScopeKey::parse(scope)?;
    let mut scopes = SCOPES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(opened) = scopes.by_key.get_mut(&key) else {
        return Err(StatsError::UnknownScope { scope: scope.clone() });
    };
    work(opened)
}

/// Runs `work` on chart `chart` of `scope`.
fn with_chart<T>(scope: &ScopeName, chart: ViewChartId, work: impl FnOnce(&Chart) -> T) -> Result<T, StatsError> {
    with_scope(scope, |opened| {
        let found = chart.chart().and_then(|id| opened.chart(id)).ok_or(StatsError::UnknownChart { chart })?;
        Ok(work(found))
    })
}

/// Runs blocking `work` off the UI thread.
async fn off_thread<T>(work: impl FnOnce() -> Result<T, StatsError> + Send + 'static) -> Result<T, StatsError>
where
    T: Send + 'static,
{
    blocking(work).await.map_err(|source| StatsError::Task { source })?
}

/// The expected-values table the ratings are computed against, as the game
/// data service keeps it refreshed. `None` until it has loaded; the tab reads
/// it again each time it is shown.
fn personal_rating_table() -> Option<Arc<PersonalRatingData>> {
    crate::game_data::personal_rating()
}

/// Writes the session's charts if `scope` is the session; the error, if
/// any, is reported beside the change rather than undoing it.
fn persist_charts(scope: &ScopeName) -> Option<StoreError> {
    let _writes = hold_writes();
    // A scope closed since the change has nothing left to save.
    let charts = with_scope(scope, |opened| Ok(opened.persists().then(|| opened.chart_settings()))).ok().flatten()?;
    scope::with_database(async |pool| scope::save_charts(pool, &charts).await).err()
}

fn scope_view(scope: &Scope, save_error: Option<StoreError>) -> ScopeView {
    ScopeView { stats: scope.snapshot(), charts: scope.chart_snapshots(), save_error: save_error.map(|error| error.to_string()) }
}

fn chart_update(
    scope: ScopeName,
    chart: ViewChartId,
    change: impl FnOnce(&mut ChartModel) + Send + 'static,
) -> Result<ChartUpdate, StatsError> {
    let snapshot = with_chart(&scope, chart, |opened| {
        opened.update(change);
        opened.snapshot()
    })?;
    Ok(ChartUpdate { chart: snapshot, save_error: persist_charts(&scope).map(|error| error.to_string()) })
}

fn ship_id(value: &ParamIdText) -> Result<GameParamId, StatsError> {
    value.parse().map_err(|source| StatsError::ShipId { value: value.clone(), source })
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
            let generation = SESSION_GENERATION.current();
            let games = scope::with_database(async |pool| scope::load_games(pool).await)?;
            let table = personal_rating_table();
            with_scope(&ScopeName::session(), |session| {
                session.set_personal_rating(table);
                if SESSION_GENERATION.current() == generation {
                    session.set_games(games);
                }
                Ok(scope_view(session, None))
            })
        })
        .await
    }

    /// Applies the filter bar, saving it for the session.
    pub async fn set_filters(scope: ScopeName, filters: FilterState) -> Result<ScopeView, StatsError> {
        off_thread(move || {
            let persists = with_scope(&scope, |opened| {
                opened.remember_count(&filters);
                opened.set_filters(Scope::filters_from(&filters));
                Ok(opened.persists())
            })?;
            let save_error = if persists {
                let _writes = hold_writes();
                let latest = with_scope(&scope, |opened| Ok(opened.filters().clone()))?;
                scope::with_database(async |pool| scope::save_filters(pool, &latest).await).err()
            } else {
                None
            };
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
            SESSION_GENERATION.advance();
            scope::with_database(async |pool| scope::clear_games(pool).await)?;
            with_scope(&ScopeName::session(), |session| {
                session.set_games(Vec::new());
                Ok(scope_view(session, None))
            })
        })
        .await
    }

    /// Forgets one ship's recorded games.
    pub async fn clear_ship(ship: ParamIdText) -> Result<ScopeView, StatsError> {
        let ship = ship_id(&ship)?;
        off_thread(move || {
            let stored = i64::try_from(ship.raw()).map_err(|_| StatsError::ShipIdOutOfRange { ship })?;
            SESSION_GENERATION.advance();
            scope::with_database(async |pool| scope::clear_ship(pool, stored).await)?;
            with_scope(&ScopeName::session(), |session| {
                let games = session.games().iter().filter(|game| game.ship_id != ship).cloned().collect();
                session.set_games(games);
                Ok(scope_view(session, None))
            })
        })
        .await
    }

    /// Opens a chart plotting `stat`.
    pub async fn add_chart(scope: ScopeName, stat: StatChoice) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            let snapshot = with_scope(&scope, |opened| Ok(opened.add_chart_for(stat.stat()).snapshot()))?;
            Ok(ChartUpdate { chart: snapshot, save_error: persist_charts(&scope).map(|error| error.to_string()) })
        })
        .await
    }

    /// Lets go of a chart whose panel was closed.
    pub async fn remove_chart(scope: ScopeName, chart: ViewChartId) -> Result<Option<String>, StatsError> {
        off_thread(move || {
            let id = chart.chart().ok_or(StatsError::UnknownChart { chart })?;
            let removed = with_scope(&scope, |opened| Ok(opened.remove_chart(id)))?;
            Ok(removed.then(|| persist_charts(&scope)).flatten().map(|error| error.to_string()))
        })
        .await
    }

    pub async fn chart_stat(scope: ScopeName, chart: ViewChartId, stat: StatChoice) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            chart_update(scope, chart, move |model| {
                model.set_stat(stat.stat());
            })
        })
        .await
    }

    pub async fn chart_mode(scope: ScopeName, chart: ViewChartId, line: bool) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            chart_update(scope, chart, move |model| {
                model.set_mode(if line { ChartMode::Line } else { ChartMode::Bar });
            })
        })
        .await
    }

    pub async fn chart_combined(scope: ScopeName, chart: ViewChartId, combined: bool) -> Result<ChartUpdate, StatsError> {
        off_thread(move || chart_update(scope, chart, move |model| model.set_combined(combined))).await
    }

    pub async fn chart_running(scope: ScopeName, chart: ViewChartId, running: bool) -> Result<ChartUpdate, StatsError> {
        off_thread(move || chart_update(scope, chart, move |model| model.set_running(running))).await
    }

    pub async fn chart_values(scope: ScopeName, chart: ViewChartId, show: bool) -> Result<ChartUpdate, StatsError> {
        off_thread(move || chart_update(scope, chart, move |model| model.set_show_values(show))).await
    }

    /// Takes the chart off the tab's filters, or puts it back on them.
    pub async fn chart_own_filters(scope: ScopeName, chart: ViewChartId, own: bool) -> Result<ChartUpdate, StatsError> {
        off_thread(move || chart_update(scope, chart, move |model| model.set_overrides_tab(own))).await
    }

    pub async fn chart_division(
        scope: ScopeName,
        chart: ViewChartId,
        division: DivisionChoice,
    ) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            chart_update(scope, chart, move |model| {
                model.set_own_division(division.filter());
            })
        })
        .await
    }

    pub async fn chart_toggle_mode(scope: ScopeName, chart: ViewChartId, mode: MatchGroup) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            chart_update(scope, chart, move |model| {
                model.toggle_own_mode(mode.as_str());
            })
        })
        .await
    }

    pub async fn chart_clear_modes(scope: ScopeName, chart: ViewChartId) -> Result<ChartUpdate, StatsError> {
        off_thread(move || {
            chart_update(scope, chart, |model| {
                model.clear_own_modes();
            })
        })
        .await
    }

    pub async fn chart_toggle_ship(scope: ScopeName, chart: ViewChartId, ship: ParamIdText) -> Result<ChartUpdate, StatsError> {
        let ship = ship_id(&ship)?;
        off_thread(move || chart_update(scope, chart, move |model| model.toggle_ship(ship))).await
    }

    /// Selects or deselects the listed ships at once.
    pub async fn chart_select_ships(
        scope: ScopeName,
        chart: ViewChartId,
        ships: Vec<ParamIdText>,
        selected: bool,
    ) -> Result<ChartUpdate, StatsError> {
        let ships = ships.iter().map(ship_id).collect::<Result<Vec<_>, _>>()?;
        off_thread(move || chart_update(scope, chart, move |model| model.set_ships_selected(&ships, selected))).await
    }

    /// Puts the chart's view back on the whole plot.
    pub fn chart_reset_view(scope: ScopeName, chart: ViewChartId) -> Result<ChartSnapshot, StatsError> {
        with_chart(&scope, chart, |opened| {
            opened.update(|model| model.view_mut().reset());
            opened.snapshot()
        })
    }

    /// Resolves once the chart's view is moved away from, or back to, the
    /// whole plot: whether it differs from `moved`, the state the caller
    /// shows. Resolves with an error once the chart is closed.
    pub async fn chart_view_moved(scope: ScopeName, chart: ViewChartId, moved: bool) -> Result<bool, StatsError> {
        let mut changes = with_chart(&scope, chart, Chart::view_moved)?;
        let changed = changes.wait_for(|current| *current != moved).await.map(|current| *current);
        // The chart closed, dropping the sender.
        changed.map_err(|_| StatsError::UnknownChart { chart })
    }

    /// The chart as a PNG `data:` URL, for `Clipboard.writeImage`.
    pub async fn chart_image(scope: ScopeName, chart: ViewChartId) -> Result<String, StatsError> {
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
        let generation = LAYOUT_GENERATION.advance();
        off_thread(move || {
            let state = layout::to_state(&layout)?;
            let _writes = hold_writes();
            if LAYOUT_GENERATION.current() != generation {
                return Ok(());
            }
            scope::with_database(async |pool| scope::save_layout(pool, &state).await)?;
            Ok(())
        })
        .await
    }

    /// Saves the open charts and the arrangement shown, replacing a saved
    /// pair that could not be restored.
    pub async fn keep_current_layout(layout: DockAreaLayout) -> Result<(), StatsError> {
        LAYOUT_GENERATION.advance();
        off_thread(move || {
            let state = layout::to_state(&layout)?;
            let _writes = hold_writes();
            let charts = with_scope(&ScopeName::session(), |session| Ok(session.chart_settings()))?;
            scope::with_database(async |pool| {
                scope::save_charts(pool, &charts).await?;
                scope::save_layout(pool, &state).await
            })?;
            Ok(())
        })
        .await
    }

    /// Opens a stats view over every replay in `path`, as a replay listing's
    /// Directory Stats button does.
    pub async fn open_directory(path: String) -> Result<DirectoryOpened, StatsError> {
        off_thread(move || open_directory(PathBuf::from(path))).await
    }

    /// Resolves once the directory view has moved past `revision`.
    pub async fn directory_status(scope: ScopeName, revision: DirectoryRevision) -> Result<DirectoryStatus, StatsError> {
        let id = DirectoryId::parse(&scope).ok_or_else(|| StatsError::UnknownScope { scope: scope.clone() })?;
        let mut revisions = with_directory(id, |_, read| Ok(read.revisions()))?;
        // The view closed, dropping the sender.
        revisions.wait_for(|current| *current != revision).await.map_err(|_| StatsError::UnknownScope { scope })?;

        with_directory(id, |scope, read| Ok(read.status(scope_view(scope, None))))
    }

    /// Reads the directory again, picking up replays added or removed since.
    pub fn reload_directory(scope: ScopeName) -> Result<(), StatsError> {
        let id = DirectoryId::parse(&scope).ok_or_else(|| StatsError::UnknownScope { scope: scope.clone() })?;
        let game_data = crate::game_data::cache().ok_or(StatsError::GameDataUnavailable)?;
        if !with_directory(id, |_, read| Ok(read.is_finished()))? {
            return Err(StatsError::StillReading);
        }

        directory::start(id, game_data);
        Ok(())
    }

    /// Closes a directory view, which stops its read.
    pub fn close_directory(scope: ScopeName) -> Result<(), StatsError> {
        let id = DirectoryId::parse(&scope).ok_or_else(|| StatsError::UnknownScope { scope: scope.clone() })?;
        let mut scopes = SCOPES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(read) = scopes.directories.remove(&id) {
            read.stop();
        }
        scopes.by_key.remove(&ScopeKey::Directory(id));
        Ok(())
    }

    /// `text` folded the way ship names are for search.
    pub fn search_key(text: String) -> String {
        wows_toolkit_viewmodel::search::normalize_ship_search(&text)
    }
}

fn open_session() -> Result<SessionView, StatsError> {
    // A session already open lets go of its charts first: their surfaces are
    // the ones the new charts draw on.
    let previous = SCOPES.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).by_key.remove(&ScopeKey::Session);
    drop(previous);

    let stored = scope::with_database(async |pool| scope::load(pool).await)?;
    let table = personal_rating_table();

    let (charts, layout_error) = match (stored.charts, stored.layout) {
        (Ok(charts), Ok(layout)) => match scope::restore(charts.clone(), layout.as_ref()) {
            Ok(restored) => (Ok(restored), None),
            Err(source) => {
                log::warn!("stats: the saved chart layout could not be restored: {source}");
                // The charts keep their settings; only the arrangement is
                // the default.
                let restored = scope::restore(charts, None)?;
                (Ok(restored), Some(RestoreError::Layout { source }))
            }
        },
        (Err(source), _) => (scope::restore(None, None), Some(RestoreError::ReadCharts { source })),
        (_, Err(source)) => (scope::restore(None, None), Some(RestoreError::ReadLayout { source })),
    };
    let restored = charts?;

    let mut session = Scope::new(ScopeName::session(), true);
    session.set_personal_rating(table);
    session.set_filters(stored.filters);
    session.set_games(stored.games);
    for settings in restored.charts {
        session.add_chart(settings);
    }
    let view = scope_view(&session, None);

    let mut scopes = SCOPES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    scopes.by_key.insert(ScopeKey::Session, session);

    Ok(SessionView { view, layout: restored.layout, layout_error: layout_error.map(|error| error.to_string()) })
}

fn stats_text() -> StatsText {
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
        directory_reload: text("ui.stats.directory_reload"),
    }
}
