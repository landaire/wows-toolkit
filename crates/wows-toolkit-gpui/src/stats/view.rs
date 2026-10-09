//! The Stats tab: a filter bar that applies to every sub-tab, over a dock
//! holding the Overview.
//!
//! Mirrors the egui app's `build_stats_tab`: the limit, division and game-mode
//! controls sit above the dock and narrow what all of it shows.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::ElementExt as _;
use gpui_kit::component::IconName;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::dock::DockArea;
use gpui_kit::component::dock::DockAreaState;
use gpui_kit::component::dock::DockEvent;
use gpui_kit::component::dock::DockPlacement;
use gpui_kit::component::dock::DockSizing;
use gpui_kit::component::dock::DockSkin;
use gpui_kit::component::dock::PanelId;
use gpui_kit::component::dock::panel_handle;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::input::NumberInput;
use gpui_kit::component::input::NumberInputEvent;
use gpui_kit::component::input::StepAction;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_toolkit_config::queries;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;

use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::GameLimit;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::all_match_groups;
use wows_toolkit_viewmodel::stats::filter_games;
use wows_toolkit_viewmodel::stats::filter_games_per_ship;
use wows_toolkit_viewmodel::stats::match_group_display_name;
use wows_toolkit_viewmodel::stats::setting_keys;

use crate::ui::selectable;

use super::chart_panel::ChartId;
use super::chart_panel::ChartSettings;
use super::chart_panel::ChartSettingsChanged;
use super::chart_panel::StatsChartPanel;
use super::load::SessionData;
use super::overview::StatsOverviewPanel;
use super::ships::ShipsPanelEvent;
use super::ships::StatsShipsPanel;

/// Games shown when the limit is switched on without a saved count. The egui
/// drag value opens at this and clamps to 1..=999.
const DEFAULT_GAME_LIMIT: usize = 25;
const MIN_GAME_LIMIT: usize = 1;
const MAX_GAME_LIMIT: usize = 999;
const MIN_STATS_PANEL_WIDTH: Pixels = px(280.);
const MIN_CHART_PANEL_WIDTH: Pixels = px(200.);

pub struct StatsView {
    /// Every recorded game, oldest first. The filters narrow this per render.
    games: Vec<PerGameStat>,
    filters: StatsFilters,
    last_game_limit_count: usize,
    /// Match groups present in `games`, so the bar only offers modes that
    /// actually occur.
    available_modes: Vec<String>,
    limit_input: Entity<InputState>,
    dock_area: Entity<DockArea>,
    chart_split_dragging: Rc<Cell<bool>>,
    overview: Entity<StatsOverviewPanel>,
    ships: Entity<StatsShipsPanel>,
    /// One per open chart sub-tab. The egui tab opens with a chart alongside
    /// the overview and lets more be added.
    charts: Vec<Entity<StatsChartPanel>>,
    /// Ids are never reused, so a closed chart's element ids cannot collide
    /// with a later one's.
    next_chart_id: usize,
    /// Why the last clear did not go through, if it did not.
    clear_error: Option<SharedString>,
    /// Handed to every panel so the rating is computed against one table.
    personal_rating: Option<std::sync::Arc<wows_toolkit_viewmodel::personal_rating::PersonalRatingData>>,
    focus_handle: FocusHandle,
    /// Whether reading saved charts has started, once the config pool is available.
    charts_load_started: bool,
    charts_load_finished: bool,
    charts_restoring: bool,
    charts_load_error: Option<SharedString>,
    charts_error_is_save: bool,
    chart_save_failed: bool,
    dock_save_failed: bool,
    charts_user_modified: bool,
    charts_persistence_enabled: bool,
    settings_write_lock: Arc<futures::lock::Mutex<()>>,
    _chart_subscriptions: Vec<Subscription>,
    _chart_save_task: Option<Task<()>>,
    chart_save_generation: u64,
    _dock_save_task: Option<Task<()>>,
    dock_save_generation: u64,
    _keep_layout_save_task: Option<Task<()>>,
    keep_layout_save_generation: u64,
    _subscriptions: Vec<Subscription>,
}

/// The settings row the Stats tab keeps its charts in.
const CHARTS_SETTINGS_KEY: &str = "stats_charts";
const CHARTS_DOCK_LAYOUT_KEY: &str = "stats_charts_dock_layout";

impl StatsView {
    /// Rebuilds the tab and its dock panels after the shared locale changes.
    pub fn set_locale(&mut self, cx: &mut Context<Self>) {
        self.overview.update(cx, |_, cx| cx.notify());
        self.ships.update(cx, |panel, cx| panel.set_locale(cx));
        for chart in &self.charts {
            chart.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    /// Writes the open charts back to the settings row, so the tab reopens
    /// with the charts it was left with rather than one default chart.
    /// The whole set is written on every change because a chart can be added,
    /// removed or configured independently.
    fn on_chart_settings_changed(
        &mut self,
        _chart: Entity<StatsChartPanel>,
        _event: &ChartSettingsChanged,
        cx: &mut Context<Self>,
    ) {
        self._keep_layout_save_task.take();
        self.keep_layout_save_generation = self.keep_layout_save_generation.wrapping_add(1);
        self.charts_user_modified = true;
        self.charts_persistence_enabled = true;
        self.save_charts(cx);
    }

    fn save_charts(&mut self, cx: &mut Context<Self>) {
        if !self.charts_persistence_enabled {
            return;
        }
        let settings: Vec<ChartSettings> = self.charts.iter().map(|chart| chart.read(cx).settings()).collect();
        let Some(pool) = crate::settings_store::pool(cx) else {
            tracing::warn!("stats: chart settings were not saved because the config database is not open");
            self.chart_save_failed = true;
            self.charts_load_error = Some("The settings database is not available".into());
            self.charts_error_is_save = true;
            cx.notify();
            return;
        };
        self._chart_save_task.take();
        self.chart_save_generation = self.chart_save_generation.wrapping_add(1);
        let generation = self.chart_save_generation;
        let write_lock = self.settings_write_lock.clone();
        self._chart_save_task =
            Some(cx.spawn(async move |this, cx| {
                let _guard = write_lock.lock().await;
                if !matches!(this.update(cx, |this, _cx| this.chart_save_generation == generation), Ok(true)) {
                    return;
                }
                let written = crate::runtime::spawn(cx, async move {
                    queries::set_setting(&pool, CHARTS_SETTINGS_KEY, &settings).await
                })
                .await;
                let failure = match written {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(format!("Chart settings could not be saved: {error}")),
                    Err(error) => Some(format!("Chart settings write did not complete: {error}")),
                };
                let _ = this.update(cx, |this, cx| {
                    if this.chart_save_generation == generation {
                        if let Some(failure) = failure {
                            this.charts_load_error = Some(failure.into());
                            this.charts_error_is_save = true;
                            this.chart_save_failed = true;
                            cx.notify();
                        } else {
                            this.chart_save_failed = false;
                            if this.charts_error_is_save && !this.dock_save_failed {
                                this.charts_load_error = None;
                                this.charts_error_is_save = false;
                                cx.notify();
                            }
                        }
                    }
                });
            }));
    }

    fn keep_current_chart_layout(&mut self, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else {
            self.charts_load_error = Some("The settings database is not available".into());
            self.charts_error_is_save = true;
            self.chart_save_failed = true;
            self.dock_save_failed = true;
            cx.notify();
            return;
        };
        self.charts_user_modified = true;
        let settings: Vec<ChartSettings> = self.charts.iter().map(|chart| chart.read(cx).settings()).collect();
        let layout = self.dock_area.read(cx).dump(cx);
        self._chart_save_task.take();
        self._dock_save_task.take();
        self.chart_save_generation = self.chart_save_generation.wrapping_add(1);
        self.dock_save_generation = self.dock_save_generation.wrapping_add(1);
        self.keep_layout_save_generation = self.keep_layout_save_generation.wrapping_add(1);
        let generation = self.keep_layout_save_generation;
        let write_lock = self.settings_write_lock.clone();
        self._keep_layout_save_task = Some(cx.spawn(async move |this, cx| {
            let _guard = write_lock.lock().await;
            if !matches!(this.update(cx, |this, _cx| this.keep_layout_save_generation == generation), Ok(true)) {
                return;
            }
            let written = crate::runtime::spawn(cx, async move {
                let charts = queries::set_setting(&pool, CHARTS_SETTINGS_KEY, &settings).await;
                let dock = queries::set_setting(&pool, CHARTS_DOCK_LAYOUT_KEY, &layout).await;
                (charts, dock)
            })
            .await;
            let (chart_failure, dock_failure) = match written {
                Ok((charts, dock)) => (
                    charts.err().map(|error| format!("Could not save chart settings: {error}")),
                    dock.err().map(|error| format!("Could not save chart layout: {error}")),
                ),
                Err(error) => {
                    let reason = format!("Chart settings could not be saved: {error}");
                    (Some(reason.clone()), Some(reason))
                }
            };
            let _ = this.update(cx, |this, cx| {
                if this.keep_layout_save_generation != generation {
                    return;
                }
                this.chart_save_failed = chart_failure.is_some();
                this.dock_save_failed = dock_failure.is_some();
                let failure = chart_failure.or(dock_failure);
                if let Some(failure) = failure {
                    this.charts_load_error = Some(failure.into());
                    this.charts_error_is_save = true;
                } else {
                    this.charts_load_error = None;
                    this.charts_error_is_save = false;
                    this.charts_load_finished = true;
                    this.charts_persistence_enabled = true;
                }
                cx.notify();
            });
        }));
    }

    fn schedule_dock_layout_save(&mut self, dock: Entity<DockArea>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else {
            self.charts_load_error = Some("The settings database is not available".into());
            self.charts_error_is_save = true;
            self.dock_save_failed = true;
            cx.notify();
            return;
        };
        self._dock_save_task.take();
        self.dock_save_generation = self.dock_save_generation.wrapping_add(1);
        let generation = self.dock_save_generation;
        let write_lock = self.settings_write_lock.clone();
        self._dock_save_task =
            Some(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let _guard = write_lock.lock().await;
                let layout = this
                    .update(cx, |this, cx| (this.dock_save_generation == generation).then(|| dock.read(cx).dump(cx)));
                let Ok(Some(layout)) = layout else { return };
                let written = crate::runtime::spawn(cx, async move {
                    queries::set_setting(&pool, CHARTS_DOCK_LAYOUT_KEY, &layout).await
                })
                .await;
                let failure = match written {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(format!("Chart layout could not be saved: {error}")),
                    Err(error) => Some(format!("Chart layout write did not complete: {error}")),
                };
                let _ = this.update(cx, |this, cx| {
                    if this.dock_save_generation == generation {
                        if let Some(failure) = failure {
                            this.charts_load_error = Some(failure.into());
                            this.charts_error_is_save = true;
                            this.dock_save_failed = true;
                            cx.notify();
                        } else {
                            this.dock_save_failed = false;
                            if this.charts_error_is_save && !this.chart_save_failed {
                                this.charts_load_error = None;
                                this.charts_error_is_save = false;
                                cx.notify();
                            }
                        }
                    }
                });
            }));
    }

    /// Rebuilds the saved dock with this view's live panels.
    fn restore_dock_layout(&mut self, state: DockAreaState, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut chart_ids = Vec::new();
        if !collect_chart_ids(&state.center, &mut chart_ids)
            || state.left_dock.as_ref().is_some_and(|dock| !collect_chart_ids(dock.panel(), &mut chart_ids))
            || state.right_dock.as_ref().is_some_and(|dock| !collect_chart_ids(dock.panel(), &mut chart_ids))
            || state.bottom_dock.as_ref().is_some_and(|dock| !collect_chart_ids(dock.panel(), &mut chart_ids))
        {
            tracing::warn!("stats: saved dock layout has an invalid chart panel; keeping the default layout");
            return false;
        }
        let mut unique_ids = std::collections::HashSet::new();
        if chart_ids.iter().any(|id| !unique_ids.insert(*id)) {
            tracing::warn!("stats: saved dock layout repeats a chart panel; keeping the default layout");
            return false;
        }

        let chart_entities: HashMap<ChartId, Entity<StatsChartPanel>> =
            chart_ids.iter().copied().map(|id| (id, cx.new(|cx| StatsChartPanel::new(id, window, cx)))).collect();
        let chart_registry: Arc<Mutex<HashMap<ChartId, WeakEntity<StatsChartPanel>>>> =
            Arc::new(Mutex::new(chart_entities.iter().map(|(id, chart)| (*id, chart.downgrade())).collect()));

        let overview = self.overview.downgrade();
        gpui_kit::component::dock::register_panel(cx, "StatsOverviewPanel", move |_state, _window, cx| {
            let panel = overview.upgrade().unwrap_or_else(|| cx.new(StatsOverviewPanel::new));
            panel_handle(panel)
        });
        let ships = self.ships.downgrade();
        gpui_kit::component::dock::register_panel(cx, "StatsShipsPanel", move |_state, window, cx| {
            let panel = ships.upgrade().unwrap_or_else(|| cx.new(|cx| StatsShipsPanel::new(window, cx)));
            panel_handle(panel)
        });
        let chart_registry_for_build = chart_registry.clone();
        gpui_kit::component::dock::register_panel(cx, "StatsChartPanel", move |context, window, cx| {
            let id = chart_id_from_state(context.state()).unwrap_or(0);
            let panel = chart_registry_for_build
                .lock()
                .expect("Stats chart restore registry lock is not poisoned")
                .get(&id)
                .and_then(WeakEntity::upgrade)
                .unwrap_or_else(|| cx.new(|cx| StatsChartPanel::new(id, window, cx)));
            panel_handle(panel)
        });

        self.charts = chart_ids.iter().filter_map(|id| chart_entities.get(id).cloned()).collect();
        self._chart_subscriptions =
            self.charts.iter().map(|chart| cx.subscribe(chart, Self::on_chart_settings_changed)).collect();
        self.next_chart_id = chart_ids.iter().copied().max().map_or(0, |id| id.saturating_add(1));

        let loaded = self.dock_area.update(cx, |dock, cx| dock.load(state, window, cx));
        match loaded {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!("stats: saved dock layout could not be loaded: {error:#}");
                false
            }
        }
    }

    /// Reopens the charts and dock arrangement the tab was left with.
    fn load_charts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.charts_load_started {
            return;
        }
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        self.charts_load_started = true;
        self.charts_load_finished = false;
        cx.spawn_in(window, async move |this, cx| {
            let stored = crate::runtime::spawn(cx, async move {
                let charts =
                    wows_toolkit_config::queries::try_get_setting::<Vec<ChartSettings>>(&pool, CHARTS_SETTINGS_KEY)
                        .await;
                let dock =
                    wows_toolkit_config::queries::try_get_setting::<DockAreaState>(&pool, CHARTS_DOCK_LAYOUT_KEY).await;
                (charts, dock)
            })
            .await;
            let (saved, saved_layout) = match stored {
                Ok(saved) => saved,
                Err(error) => {
                    let message: SharedString = error.to_string().into();
                    let _ = this.update(cx, |this, cx| {
                        this.charts_load_started = false;
                        this.charts_load_finished = true;
                        this.charts_load_error = Some(message);
                        this.charts_error_is_save = false;
                        cx.notify();
                    });
                    return;
                }
            };
            let (saved, saved_layout) = match (saved, saved_layout) {
                (Ok(saved), Ok(layout)) => (saved, layout),
                (charts, layout) => {
                    let message: SharedString = match (charts, layout) {
                        (Err(error), _) => format!("Could not read saved chart settings: {error}").into(),
                        (_, Err(error)) => format!("Could not read saved chart layout: {error}").into(),
                        _ => unreachable!(),
                    };
                    let _ = this.update(cx, |this, cx| {
                        this.charts_load_started = false;
                        this.charts_load_finished = true;
                        this.charts_load_error = Some(message);
                        this.charts_error_is_save = false;
                        cx.notify();
                    });
                    return;
                }
            };
            let _ = this.update_in(cx, |this, window, cx| {
                if this.charts_user_modified {
                    this.charts_load_finished = true;
                    return;
                }
                this.charts_load_error = None;
                this.charts_error_is_save = false;
                this.charts_restoring = true;
                let saved_is_empty = saved.as_ref().is_some_and(Vec::is_empty);
                let mut restored_layout = false;
                if let Some(layout) = saved_layout {
                    restored_layout = this.restore_dock_layout(layout, window, cx);
                    if !restored_layout {
                        this.charts_restoring = false;
                        this.charts_load_finished = true;
                        this.charts_load_error = Some("Saved chart layout could not be restored".into());
                        this.charts_error_is_save = false;
                        cx.notify();
                        return;
                    }
                }
                if !restored_layout {
                    if let Some(first) = this.charts.first().cloned() {
                        this.dock_area.update(cx, |dock, cx| {
                            if dock.panel(PanelId::from(first.entity_id())).is_none() {
                                dock.add_panel_view(
                                    panel_handle(first.clone()),
                                    DockPlacement::Right,
                                    Some(px(500.)),
                                    window,
                                    cx,
                                );
                            }
                        });
                    }
                }
                if saved_is_empty {
                    let charts = std::mem::take(&mut this.charts);
                    this._chart_subscriptions.clear();
                    this.dock_area.update(cx, |dock, cx| {
                        for chart in charts {
                            dock.remove_panel(chart, window, cx);
                        }
                    });
                    this.charts_restoring = false;
                    this.charts_persistence_enabled = true;
                    this.charts_load_finished = true;
                    this.save_charts(cx);
                    cx.notify();
                    return;
                }
                if let Some(saved) = saved {
                    if !restored_layout {
                        let mut saved = saved.into_iter();
                        if let (Some(chart), Some(settings)) = (this.charts.first().cloned(), saved.next()) {
                            chart.update(cx, |panel, cx| panel.apply_settings(settings, cx));
                        }
                        for settings in saved {
                            this.add_chart_with(settings, window, cx);
                        }
                    } else {
                        let saved_ids: Vec<_> =
                            saved.iter().enumerate().map(|(index, settings)| settings.id.unwrap_or(index)).collect();
                        for (id, settings) in saved_ids.into_iter().zip(saved) {
                            if let Some(chart) = this.charts.iter().find(|chart| chart.read(cx).id() == id).cloned() {
                                chart.update(cx, |panel, cx| panel.apply_settings(settings, cx));
                            } else {
                                this.add_chart_with(ChartSettings { id: Some(id), ..settings }, window, cx);
                            }
                        }
                    }
                }
                if this.charts.is_empty() {
                    this.charts_restoring = false;
                    this.charts_persistence_enabled = true;
                    this.charts_load_finished = true;
                    cx.notify();
                    return;
                }
                if !restored_layout {
                    this.next_chart_id =
                        this.charts.iter().map(|chart| chart.read(cx).id()).max().map_or(0, |id| id.saturating_add(1));
                }
                this.push_filtered(cx);
                this.charts_restoring = false;
                this.charts_persistence_enabled = true;
                this.charts_load_finished = true;
                this.save_charts(cx);
                cx.notify();
            });
        })
        .detach();
    }
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let overview = cx.new(StatsOverviewPanel::new);
        // The skinned area is what draws a tab bar over a group holding more
        // than one panel; a bare one renders only the displayed panel.
        let (dock_area, _) = DockSkin::dock_area("stats-dock", None, window, cx);
        dock_area.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(overview.clone()), DockPlacement::Center, None, window, cx);
        });
        let ships = cx.new(|cx| StatsShipsPanel::new(window, cx));
        dock_area.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(ships.clone()), DockPlacement::Center, None, window, cx);
        });
        let first_chart = cx.new(|cx| StatsChartPanel::new(0, window, cx));
        dock_area.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(first_chart.clone()), DockPlacement::Right, Some(px(500.)), window, cx);
            // Overview and Ships share the main tab group; Charts opens beside them.
            dock.select_panel(PanelId::from(overview.entity_id()), window, cx);
        });

        let limit_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(DEFAULT_GAME_LIMIT.to_string())
                .pattern(regex::Regex::new(r"^\d*$").expect("a digits-only pattern is a valid regex"))
        });

        let subscriptions = vec![
            cx.subscribe_in(&limit_input, window, Self::on_limit_step),
            cx.subscribe_in(&limit_input, window, Self::on_limit_changed),
            cx.subscribe(&ships, Self::on_ships_event),
            cx.subscribe_in(&dock_area, window, |this, dock, event, window, cx| {
                if matches!(event, DockEvent::LayoutChanged) {
                    if this.charts_restoring {
                        return;
                    }
                    this._keep_layout_save_task.take();
                    this.keep_layout_save_generation = this.keep_layout_save_generation.wrapping_add(1);
                    this.charts_user_modified = true;
                    this.drop_closed_charts(cx);
                    this.charts_persistence_enabled = true;
                    if this.charts_persistence_enabled {
                        this.schedule_dock_layout_save(dock.clone(), window, cx);
                    }
                }
            }),
        ];
        let chart_subscriptions = vec![cx.subscribe(&first_chart, Self::on_chart_settings_changed)];

        Self {
            games: Vec::new(),
            filters: StatsFilters::default(),
            last_game_limit_count: DEFAULT_GAME_LIMIT,
            available_modes: Vec::new(),
            limit_input,
            dock_area,
            chart_split_dragging: Rc::new(Cell::new(false)),
            overview,
            ships,
            charts: vec![first_chart],
            next_chart_id: 1,
            clear_error: None,
            personal_rating: None,
            focus_handle: cx.focus_handle(),
            charts_load_started: false,
            charts_load_finished: false,
            charts_restoring: false,
            charts_load_error: None,
            charts_error_is_save: false,
            chart_save_failed: false,
            dock_save_failed: false,
            charts_user_modified: false,
            charts_persistence_enabled: false,
            settings_write_lock: Arc::new(futures::lock::Mutex::new(())),
            _chart_subscriptions: chart_subscriptions,
            _chart_save_task: None,
            chart_save_generation: 0,
            _dock_save_task: None,
            dock_save_generation: 0,
            _keep_layout_save_task: None,
            keep_layout_save_generation: 0,
            _subscriptions: subscriptions,
        }
    }

    /// Adopts the session read from the config database.
    pub fn apply_session(&mut self, data: SessionData, window: &mut Window, cx: &mut Context<Self>) {
        self.available_modes = all_match_groups(&data.games).into_iter().collect();
        self.games = data.games;
        self.filters = data.filters;
        self.personal_rating = data.personal_rating;

        let table = self.personal_rating.clone();
        self.overview.update(cx, |panel, cx| panel.set_personal_rating(table.clone(), cx));
        self.ships.update(cx, |panel, cx| panel.set_personal_rating(table.clone(), cx));
        for chart in &self.charts {
            chart.update(cx, |panel, cx| panel.set_personal_rating(table.clone(), cx));
        }

        if let GameLimit::Recent(count) = self.filters.limit {
            self.last_game_limit_count = count.clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT);
            self.filters.limit = GameLimit::Recent(self.last_game_limit_count);
            self.limit_input
                .update(cx, |state, cx| state.set_value(self.last_game_limit_count.to_string(), window, cx));
        }
        self.push_filtered(cx);
    }

    /// Recomputes the filtered set once and hands it to every panel.
    /// Applies the filters to every panel, and writes them back.
    ///
    /// The rows are the egui tab's own, so a session filtered in one app
    /// opens filtered the same way in the other; every path that changes a
    /// filter goes through here, so none of them can forget to save.
    fn push_filtered(&mut self, cx: &mut Context<Self>) {
        self.filters.game_modes.retain(|mode| self.available_modes.contains(mode));
        self.save_filters(cx);
        self.drop_closed_charts(cx);
        // The summary counts the session's own last N; the per-ship table counts
        // N per ship, which is the split the egui tab makes between its two
        // reads (`ui/stats_tab.rs:327` against `:406`).
        let filtered = filter_games(&self.games, &self.filters);
        let per_ship = filter_games_per_ship(&self.games, &self.filters);
        self.overview.update(cx, |panel, cx| panel.set_games(&filtered, cx));
        self.ships.update(cx, |panel, cx| panel.set_games(&per_ship, cx));
        // A chart gets the whole session and the bar's answer: one that has
        // been taken off the bar narrows the session itself.
        for chart in &self.charts {
            chart.update(cx, |panel, cx| panel.set_games(&self.games, &self.filters, cx));
        }
        cx.notify();
    }

    /// Lets go of the charts whose tab has been closed.
    ///
    /// The dock owns what is on screen; this list is only how the tab feeds
    /// them. A closed chart left here would keep its own copy of every
    /// filtered game and be handed each new one forever.
    fn drop_closed_charts(&mut self, cx: &mut Context<Self>) {
        let before = self.charts.len();
        let dock = self.dock_area.read(cx);
        self.charts.retain(|chart| dock.panel(PanelId::from(chart.entity_id())).is_some());
        // A chart that was closed is one the tab must not reopen.
        if self.charts.len() != before {
            self.charts_user_modified = true;
            self.charts_persistence_enabled = true;
            self.save_charts(cx);
        }
    }

    /// Asks before forgetting every recorded game.
    fn confirm_clear_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.games.is_empty() {
            return;
        }
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _window, _cx| {
            let view = view.clone();
            alert
                .title(t!("ui.stats.clear").into_owned())
                .description(t!("confirm.clear_all_session_stats").into_owned())
                .show_cancel(true)
                .on_ok(move |_event, _window, cx| {
                    view.update(cx, |this, cx| this.clear_session(cx));
                    true
                })
        });
    }

    /// Forgets every recorded game and refreshes the panels that read them.
    fn clear_session(&mut self, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        // The rows go once the delete has gone through, not before: a
        // failure would otherwise leave the tab showing an empty session that
        // comes back at the next load, with nothing saying why.
        cx.spawn(async move |this, cx| {
            let cleared = crate::runtime::spawn(cx, async move { queries::clear_session_stats(&pool).await }).await;
            let _ = this.update(cx, |this, cx| match cleared {
                Ok(Ok(_)) => {
                    this.clear_error = None;
                    this.games.clear();
                    this.available_modes.clear();
                    this.push_filtered(cx);
                }
                Ok(Err(err)) => this.report_clear_failure(&err.to_string(), cx),
                Err(err) => this.report_clear_failure(&err.to_string(), cx),
            });
        })
        .detach();
    }

    /// Says a clear did not go through, where the button that asked for it is.
    fn report_clear_failure(&mut self, reason: &str, cx: &mut Context<Self>) {
        tracing::warn!("stats: the session was not cleared: {reason}");
        self.clear_error = Some(t!("ui.stats.clear_failed", reason = reason).into_owned().into());
        cx.notify();
    }

    /// Hands the roundup the build's art for its achievements.
    pub fn set_game_data(
        &mut self,
        vfs: &wowsunpack::vfs::VfsPath,
        provider: Arc<wowsunpack::game_params::provider::GameMetadataProvider>,
        cx: &mut Context<Self>,
    ) {
        self.ships.update(cx, |panel, cx| panel.set_game_data(vfs, provider.clone(), cx));
        self.overview.update(cx, |panel, cx| panel.set_game_data(vfs, provider, cx));
    }

    /// Forgets one ship's games, which the Ships panel asks for but the tab
    /// owns.
    fn on_ships_event(&mut self, _panel: Entity<StatsShipsPanel>, event: &ShipsPanelEvent, cx: &mut Context<Self>) {
        let ShipsPanelEvent::ClearShip(ship) = event;
        let ship = *ship;

        let Some(pool) = crate::settings_store::pool(cx) else { return };

        cx.spawn(async move |this, cx| {
            let cleared = crate::runtime::spawn(cx, async move {
                queries::clear_session_stats_for_ship(&pool, ship.raw() as i64).await
            })
            .await;
            let _ = this.update(cx, |this, cx| match cleared {
                Ok(Ok(_)) => {
                    this.clear_error = None;
                    this.games.retain(|game| game.ship_id != ship);
                    this.available_modes = all_match_groups(&this.games).into_iter().collect();
                    this.push_filtered(cx);
                }
                Ok(Err(err)) => this.report_clear_failure(&err.to_string(), cx),
                Err(err) => this.report_clear_failure(&err.to_string(), cx),
            });
        })
        .detach();
    }

    /// Writes the filter bar's state to the rows `load::load_filters` reads.
    fn save_filters(&self, cx: &mut Context<Self>) {
        let (limit_enabled, count) = match self.filters.limit {
            GameLimit::All => (false, None),
            GameLimit::Recent(count) => (true, Some(count)),
        };
        crate::settings_store::save(setting_keys::LIMIT_ENABLED, &limit_enabled, cx);
        if let Some(count) = count {
            crate::settings_store::save(setting_keys::GAME_COUNT, &count, cx);
        }
        crate::settings_store::save(setting_keys::DIVISION_FILTER, &self.filters.division, cx);
        crate::settings_store::save(setting_keys::GAME_MODE_FILTER, &self.filters.game_modes, cx);
    }

    /// Opens another chart sub-tab plotting `stat`.
    ///
    /// The statistic is chosen when the chart is asked for rather than after
    /// it opens: a new chart that always plotted damage meant opening one,
    /// finding its settings and changing it every time.
    fn add_chart(&mut self, stat: ChartableStat, window: &mut Window, cx: &mut Context<Self>) {
        self._keep_layout_save_task.take();
        self.keep_layout_save_generation = self.keep_layout_save_generation.wrapping_add(1);
        self.charts_user_modified = true;
        self.charts_persistence_enabled = true;
        self.add_chart_with(ChartSettings { stat, ..ChartSettings::default() }, window, cx);
        self.save_charts(cx);
    }

    /// Opens a chart already set up the way `settings` says.
    fn add_chart_with(&mut self, settings: ChartSettings, window: &mut Window, cx: &mut Context<Self>) {
        let id = settings.id.unwrap_or(self.next_chart_id);
        self.next_chart_id = self.next_chart_id.max(id.saturating_add(1));

        let chart = cx.new(|cx| StatsChartPanel::new(id, window, cx));
        self._chart_subscriptions.push(cx.subscribe(&chart, Self::on_chart_settings_changed));
        let table = self.personal_rating.clone();
        let games = self.games.clone();
        let filters = self.filters.clone();
        chart.update(cx, |panel, cx| {
            // Before the session, so the narrowing is in place the first time
            // the games are filtered.
            panel.apply_settings(settings, cx);
            panel.set_personal_rating(table, cx);
            panel.set_games(&games, &filters, cx);
        });

        self.dock_area.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(chart.clone()), DockPlacement::Right, Some(px(500.)), window, cx);
        });
        self.charts.push(chart);
        cx.notify();
    }

    fn set_limit_enabled(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        if enabled {
            if let Ok(count) = self.limit_input.read(cx).value().parse::<usize>() {
                self.last_game_limit_count = count.clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT);
            }
            self.limit_input.update(cx, |input, cx| {
                input.set_value(self.last_game_limit_count.to_string(), window, cx);
            });
            self.filters.limit = GameLimit::Recent(self.last_game_limit_count);
        } else {
            self.filters.limit = GameLimit::All;
        }
        self.push_filtered(cx);
    }

    /// The stepper reports the direction; the owner applies it and writes the
    /// value back, which is how `NumberInput` is driven.
    fn on_limit_step(
        &mut self,
        state: &Entity<InputState>,
        event: &NumberInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let NumberInputEvent::Step(action) = event;
        let GameLimit::Recent(current) = self.filters.limit else {
            return;
        };
        let next = match action {
            StepAction::Increment => current.saturating_add(1),
            StepAction::Decrement => current.saturating_sub(1),
        }
        .clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT);

        state.update(cx, |input, cx| input.set_value(next.to_string(), window, cx));
        self.set_limit_count(next, window, cx);
    }

    /// A typed value counts as well as a stepped one.
    fn on_limit_changed(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let InputEvent::Change = event else { return };
        let Ok(count) = state.read(cx).value().parse::<usize>() else {
            // A half-typed or empty box is not a limit of zero; the last
            // applied value stands until the field parses again.
            return;
        };
        self.set_limit_count(count, window, cx);
    }

    fn set_limit_count(&mut self, count: usize, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.filters.limit, GameLimit::All) {
            return;
        }
        self.last_game_limit_count = count.clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT);
        if count != self.last_game_limit_count {
            self.limit_input.update(cx, |input, cx| {
                input.set_value(self.last_game_limit_count.to_string(), window, cx);
            });
        }
        self.filters.limit = GameLimit::Recent(self.last_game_limit_count);
        self.push_filtered(cx);
    }

    fn set_division(&mut self, division: DivisionFilter, cx: &mut Context<Self>) {
        if self.filters.division == division {
            return;
        }
        self.filters.division = division;
        self.push_filtered(cx);
    }

    /// Toggles one mode. An empty set means every mode, so clearing the last
    /// selection returns the bar to All rather than hiding everything.
    fn toggle_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
        if !self.filters.game_modes.remove(mode) {
            self.filters.game_modes.insert(mode.to_string());
        }
        self.push_filtered(cx);
    }

    fn clear_modes(&mut self, cx: &mut Context<Self>) {
        if self.filters.game_modes.is_empty() {
            return;
        }
        self.filters.game_modes.clear();
        self.push_filtered(cx);
    }
}

/// The Add-chart control: a menu of statistics, so the chart opens on the one
/// that was asked for.
fn add_chart_menu(view: Entity<StatsView>) -> impl IntoElement {
    let trigger =
        Button::new("stats-add-chart").icon(IconName::Plus).label(t!("ui.stats.add_chart").to_string()).compact();

    Popover::new("stats-add-chart-menu").trigger(trigger).content(move |_state, _window, _cx| {
        let view = view.clone();
        v_flex().min_w(px(180.)).gap_0().p_1().children(ChartableStat::ALL.into_iter().map(move |stat| {
            let view = view.clone();
            Button::new(("stats-add-chart-stat", stat as usize))
                .label(t!(stat.translation_key()).to_string())
                .ghost()
                .compact()
                .justify_start()
                .w_full()
                .on_click(move |_event, window, cx: &mut App| {
                    view.update(cx, |this, cx| this.add_chart(stat, window, cx));
                })
        }))
    })
}

impl Focusable for StatsView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn chart_id_from_state(state: &gpui_kit::component::dock::PanelState) -> Option<ChartId> {
    let gpui_kit::component::dock::PanelInfo::Panel(value) = &state.info else { return None };
    usize::try_from(value.get("id")?.as_u64()?).ok()
}

fn collect_chart_ids(state: &gpui_kit::component::dock::PanelState, chart_ids: &mut Vec<ChartId>) -> bool {
    match &state.info {
        gpui_kit::component::dock::PanelInfo::Stack { .. } | gpui_kit::component::dock::PanelInfo::Tabs { .. } => {
            state.children.iter().all(|child| collect_chart_ids(child, chart_ids))
        }
        gpui_kit::component::dock::PanelInfo::Panel(_) if state.panel_name == "StatsChartPanel" => {
            let Some(id) = chart_id_from_state(state) else { return false };
            chart_ids.push(id);
            true
        }
        gpui_kit::component::dock::PanelInfo::Panel(_) => true,
    }
}

impl Render for StatsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = cx.theme().primary;
        // Read when the pool is ready; an early frame can precede its setup.
        if !self.charts_load_started && self.charts_load_error.is_none() {
            self.load_charts(window, cx);
        }
        let border = cx.theme().border;
        let limited = matches!(self.filters.limit, GameLimit::Recent(_));

        let division_buttons = [DivisionFilter::All, DivisionFilter::SoloOnly, DivisionFilter::DivOnly].map(|filter| {
            let chosen = self.filters.division == filter;
            selectable(
                ("stats-division", filter as usize),
                chosen,
                Button::new(("stats-division-button", filter as usize))
                    .label(filter.label())
                    .compact()
                    .selected(chosen)
                    .on_click(cx.listener(move |this, _event, _window, cx| this.set_division(filter, cx))),
            )
        });

        let stats_view = cx.entity();
        let selected_modes = self.filters.game_modes.clone();
        let mode_row = (self.available_modes.len() > 1).then(|| {
            let selected_count = self.filters.game_modes.len();
            let modes = self.available_modes.clone();
            let selected_names = modes
                .iter()
                .filter(|mode| selected_modes.contains(*mode))
                .map(|mode| match_group_display_name(mode).to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Popover::new("stats-mode-filter")
                .trigger(
                    Button::new("stats-mode-filter-trigger")
                        .label(if selected_count == 0 {
                            t!("ui.stats.mode_label").to_string()
                        } else {
                            format!("{} ({selected_count})", t!("ui.stats.mode_label"))
                        })
                        .compact()
                        .selected(selected_count > 0)
                        .tooltip(if selected_count == 0 { t!("ui.stats.div_all").to_string() } else { selected_names }),
                )
                .content(move |_state, _window, _cx| {
                    let all_selected = selected_count == 0;
                    let view = stats_view.clone();
                    v_flex()
                        .min_w(px(220.))
                        .gap_1()
                        .p_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(crate::theme::text_dim())
                                .child(t!("ui.stats.mode_label").to_string()),
                        )
                        .child(selectable(
                            "stats-mode-all",
                            all_selected,
                            Button::new("stats-mode-all-button")
                                .label(t!("ui.stats.div_all").to_string())
                                .compact()
                                .selected(all_selected)
                                .w_full()
                                .justify_start()
                                .on_click(move |_event, _window, cx: &mut App| {
                                    view.update(cx, |this, cx| this.clear_modes(cx));
                                }),
                        ))
                        .children(modes.iter().enumerate().map(|(index, mode)| {
                            let mode = mode.clone();
                            let chosen = selected_modes.contains(&mode);
                            let view = stats_view.clone();
                            selectable(
                                ("stats-mode", index),
                                chosen,
                                Button::new(("stats-mode-button", index))
                                    .label(match_group_display_name(&mode).to_string())
                                    .compact()
                                    .selected(chosen)
                                    .w_full()
                                    .justify_start()
                                    .on_click(move |_event, _window, cx: &mut App| {
                                        let mode = mode.clone();
                                        view.update(cx, |this, cx| this.toggle_mode(&mode, cx));
                                    }),
                            )
                        }))
                })
        });

        let filter_bar = h_flex()
            .flex_none()
            .flex_wrap()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Checkbox::new("stats-limit-enabled")
                            .label(t!("ui.stats.limit_to_recent").to_string())
                            .tooltip(t!("ui.stats.limit_to_recent_tooltip").to_string())
                            .checked(limited)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                this.set_limit_enabled(*checked, window, cx)
                            })),
                    )
                    .child(
                        // NumberInput carries no id of its own, so the wrapper
                        // is what the interaction tests address.
                        div()
                            .id("stats-limit-count")
                            .test_support()
                            .w(px(90.))
                            .child(NumberInput::new(&self.limit_input).small().disabled(!limited)),
                    ),
            )
            .child(crate::ui::rule_v(cx))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(crate::theme::text_dim())
                            .child(t!("ui.stats.division_label").to_string()),
                    )
                    .children(division_buttons),
            )
            .when_some(mode_row, |this, row| this.child(crate::ui::rule_v(cx)).child(row))
            .child(crate::ui::rule_v(cx))
            .child(add_chart_menu(cx.entity()))
            .child(div().flex_1())
            .when_some(self.charts_load_error.clone(), |this, reason| {
                let view = cx.entity();
                let retry_view = view.clone();
                let retry_save = self.charts_error_is_save;
                this.child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(div().text_xs().text_color(rgb(crate::theme::semantic().error)).child(reason))
                        .child(
                            Button::new("stats-retry-chart-load")
                                .label(t!("ui.buttons.retry").to_string())
                                .compact()
                                .on_click(move |_event, window, cx: &mut App| {
                                    retry_view.update(cx, |this, cx| {
                                        if retry_save {
                                            this.keep_current_chart_layout(cx);
                                        } else {
                                            this.charts_load_error = None;
                                            this.charts_error_is_save = false;
                                            this.charts_load_started = false;
                                            this.load_charts(window, cx);
                                        }
                                    });
                                }),
                        )
                        .child(
                            Button::new("stats-keep-current-layout")
                                .label(t!("ui.stats.keep_current_layout").to_string())
                                .compact()
                                .on_click(move |_event, _window, cx: &mut App| {
                                    view.update(cx, |this, cx| this.keep_current_chart_layout(cx));
                                }),
                        ),
                )
            })
            .when_some(self.clear_error.clone(), |this, reason| {
                this.child(div().text_xs().text_color(rgb(crate::theme::semantic().error)).child(reason))
            })
            .child(
                Button::new("stats-clear")
                    .child(crate::icons::icon(crate::icons::ERASER))
                    .label(t!("ui.stats.clear").into_owned())
                    .compact()
                    .disabled(self.games.is_empty())
                    .tooltip(t!("ui.stats.clear_tooltip").to_string())
                    .on_click(cx.listener(|this, _event, window, cx| this.confirm_clear_session(window, cx))),
            );

        let dock_bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let dock_bounds_for_layout = dock_bounds.clone();
        let dock_area = self.dock_area.clone();
        let dock_for_layout = dock_area.clone();
        let resize_loaded_layout = self.charts_load_finished && self.charts_persistence_enabled;
        let dock_layout = div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .on_prepaint(move |bounds, window, cx| {
                dock_bounds_for_layout.set(bounds);
                // A layout adjustment emits LayoutChanged; wait for saved settings before enabling writes.
                if !resize_loaded_layout {
                    return;
                }
                let dock = dock_for_layout.read(cx);
                let Some(size) = dock.dock_size(DockPlacement::Right) else { return };
                // An absent left dock occupies no width.
                let opposite = dock.dock_size(DockPlacement::Left).unwrap_or(px(0.));
                let maximum = (bounds.size.width - opposite - MIN_STATS_PANEL_WIDTH).max(MIN_CHART_PANEL_WIDTH);
                let fitted = size.clamp(MIN_CHART_PANEL_WIDTH, maximum);
                if size != fitted {
                    let dock = dock_for_layout.clone();
                    window.defer(cx, move |window, cx| {
                        dock.update(cx, |dock, cx| {
                            dock.set_dock_size(DockPlacement::Right, fitted, window, cx);
                        });
                    });
                }
            })
            .child(dock_area.clone());
        let chart_splitter = dock_area.read(cx).dock_size(DockPlacement::Right).map(|size| {
            let drag_active = self.chart_split_dragging.clone();
            let drag_start = drag_active.clone();
            let bounds = dock_bounds.clone();
            let dock_for_resize = dock_area.clone();
            div()
                .id("stats-chart-splitter")
                .absolute()
                .top_0()
                .bottom_0()
                .right(size - px(8.))
                .w(px(16.))
                .cursor_col_resize()
                .group("stats-chart-splitter")
                .occlude()
                .on_mouse_down(MouseButton::Left, move |_event: &MouseDownEvent, _, cx: &mut App| {
                    drag_start.set(true);
                    cx.stop_propagation();
                })
                .child(
                    canvas(
                        |_, _, _| (),
                        move |_, (), window, _| {
                            let dock = dock_for_resize.clone();
                            let bounds = bounds.clone();
                            let drag_active_move = drag_active.clone();
                            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                                if !phase.bubble() || !drag_active_move.get() {
                                    return;
                                }
                                let area_bounds = bounds.get();
                                dock.update(cx, |dock, cx| {
                                    let sizing = DockSizing::new(DockPlacement::Right)
                                        .with_area_bounds(area_bounds)
                                        .with_opposite_dock_size(dock.dock_size(DockPlacement::Left).unwrap_or(px(0.)));
                                    // An absent left dock occupies no width.
                                    let opposite = dock.dock_size(DockPlacement::Left).unwrap_or(px(0.));
                                    let maximum = (area_bounds.size.width - opposite - MIN_STATS_PANEL_WIDTH)
                                        .max(MIN_CHART_PANEL_WIDTH);
                                    let size = sizing
                                        .clamp(sizing.size_from_pointer(event.position))
                                        .clamp(MIN_CHART_PANEL_WIDTH, maximum);
                                    dock.set_dock_size(DockPlacement::Right, size, window, cx);
                                });
                            });
                            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, _| {
                                if phase.bubble() && event.button == MouseButton::Left {
                                    drag_active.set(false);
                                }
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(7.))
                        .w(px(2.))
                        .bg(border)
                        .group_hover("stats-chart-splitter", |this| this.bg(accent)),
                )
        });
        let dock_layout = dock_layout.children(chart_splitter);

        v_flex().id("stats-root").track_focus(&self.focus_handle).size_full().child(filter_bar).child(dock_layout)
    }
}
