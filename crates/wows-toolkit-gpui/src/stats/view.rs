//! The Stats tab: a filter bar that applies to every sub-tab, over a dock
//! holding the Overview.
//!
//! Mirrors the egui app's `build_stats_tab`: the limit, division and game-mode
//! controls sit above the dock and narrow what all of it shows.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::IconName;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::DockArea;
use gpui_kit::component::dock::DockPlacement;
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

pub struct StatsView {
    /// Every recorded game, oldest first. The filters narrow this per render.
    games: Vec<PerGameStat>,
    filters: StatsFilters,
    /// Match groups present in `games`, so the bar only offers modes that
    /// actually occur.
    available_modes: Vec<String>,
    limit_input: Entity<InputState>,
    dock_area: Entity<DockArea>,
    overview: Entity<StatsOverviewPanel>,
    ships: Entity<StatsShipsPanel>,
    /// One per open chart sub-tab. The egui tab opens with a chart alongside
    /// the overview and lets more be added.
    charts: Vec<Entity<StatsChartPanel>>,
    /// Ids are never reused, so a closed chart's element ids cannot collide
    /// with a later one's.
    next_chart_id: usize,
    /// Whether the clear button has been pressed once and is waiting to be
    /// confirmed.
    clear_armed: bool,
    /// Why the last clear did not go through, if it did not.
    clear_error: Option<SharedString>,
    /// Handed to every panel so the rating is computed against one table.
    personal_rating: Option<std::sync::Arc<wows_toolkit_viewmodel::personal_rating::PersonalRatingData>>,
    focus_handle: FocusHandle,
    /// Whether the saved charts have been read back yet. One shot, on the
    /// first frame.
    charts_loaded: bool,
    _subscriptions: Vec<Subscription>,
}

/// The settings row the Stats tab keeps its charts in.
const CHARTS_SETTINGS_KEY: &str = "stats_charts";

impl StatsView {
    /// Writes the open charts back to the settings row, so the tab reopens
    /// with the charts it was left with rather than one default chart.
    ///
    /// The whole set is written on every change: a chart's place in the dock
    /// is what its index means, so there is nothing smaller to write.
    /// A chart was set up differently, so the set is written back.
    fn on_chart_settings_changed(
        &mut self,
        _chart: Entity<StatsChartPanel>,
        _event: &ChartSettingsChanged,
        cx: &mut Context<Self>,
    ) {
        self.save_charts(cx);
    }

    fn save_charts(&self, cx: &mut Context<Self>) {
        let settings: Vec<ChartSettings> = self.charts.iter().map(|chart| chart.read(cx).settings()).collect();
        crate::settings_store::save(CHARTS_SETTINGS_KEY, &settings, cx);
    }

    /// Reopens the charts the tab was left with.
    ///
    /// The first chart is already open, so it takes the first saved setting
    /// and the rest are added beside it. A row with nothing in it leaves that
    /// one chart on its defaults, which is what a first run shows.
    fn load_charts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let stored = crate::runtime::spawn(cx, async move {
                wows_toolkit_config::queries::get_setting::<Vec<ChartSettings>>(&pool, CHARTS_SETTINGS_KEY).await
            })
            .await;
            let Ok(Some(saved)) = stored else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                let mut saved = saved.into_iter();
                if let Some(first) = saved.next()
                    && let Some(chart) = this.charts.first().cloned()
                {
                    chart.update(cx, |panel, cx| panel.apply_settings(first, cx));
                }
                for settings in saved {
                    this.add_chart_with(settings, window, cx);
                }
                this.push_filtered(cx);
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
        let ships = cx.new(StatsShipsPanel::new);
        dock_area.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(ships.clone()), DockPlacement::Center, None, window, cx);
        });
        let first_chart = cx.new(|cx| StatsChartPanel::new(0, cx));
        dock_area.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(first_chart.clone()), DockPlacement::Center, None, window, cx);
            // Each add activates what it added, so the chart would be showing;
            // the egui tab opens on its overview.
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
            cx.subscribe(&first_chart, Self::on_chart_settings_changed),
        ];

        Self {
            games: Vec::new(),
            filters: StatsFilters::default(),
            available_modes: Vec::new(),
            limit_input,
            dock_area,
            overview,
            ships,
            charts: vec![first_chart],
            next_chart_id: 1,
            clear_armed: false,
            clear_error: None,
            personal_rating: None,
            focus_handle: cx.focus_handle(),
            charts_loaded: false,
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
            self.limit_input.update(cx, |state, cx| state.set_value(count.to_string(), window, cx));
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
            self.save_charts(cx);
        }
    }

    /// Forgets every recorded game, once the button has been pressed twice.
    ///
    /// The first press arms it and says so; the second empties the table the
    /// session is kept in and the panels reading from it.
    fn clear_session(&mut self, cx: &mut Context<Self>) {
        if !self.clear_armed {
            self.clear_armed = true;
            cx.notify();
            return;
        }
        self.clear_armed = false;

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
    pub fn set_game_data(&mut self, vfs: &wowsunpack::vfs::VfsPath, cx: &mut Context<Self>) {
        let svg = gpui_kit::SvgRenderer::new(cx.asset_source().clone());
        self.overview.update(cx, |panel, cx| panel.set_game_data(vfs, &svg, cx));
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
        self.add_chart_with(ChartSettings { stat, ..ChartSettings::default() }, window, cx);
        self.save_charts(cx);
    }

    /// Opens a chart already set up the way `settings` says.
    fn add_chart_with(&mut self, settings: ChartSettings, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_chart_id;
        self.next_chart_id += 1;

        let chart = cx.new(|cx| StatsChartPanel::new(id, cx));
        self._subscriptions.push(cx.subscribe(&chart, Self::on_chart_settings_changed));
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
            dock.add_panel_view(panel_handle(chart.clone()), DockPlacement::Center, None, window, cx);
        });
        self.charts.push(chart);
        cx.notify();
    }

    fn set_limit_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let count = self.limit_input.read(cx).value().parse::<usize>().unwrap_or(DEFAULT_GAME_LIMIT);
        self.filters.limit =
            if enabled { GameLimit::Recent(count.clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT)) } else { GameLimit::All };
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
        self.set_limit_count(next, cx);
    }

    /// A typed value counts as well as a stepped one.
    fn on_limit_changed(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let InputEvent::Change = event else { return };
        let Ok(count) = state.read(cx).value().parse::<usize>() else {
            // A half-typed or empty box is not a limit of zero; the last
            // applied value stands until the field parses again.
            return;
        };
        self.set_limit_count(count, cx);
    }

    fn set_limit_count(&mut self, count: usize, cx: &mut Context<Self>) {
        if matches!(self.filters.limit, GameLimit::All) {
            return;
        }
        self.filters.limit = GameLimit::Recent(count.clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT));
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

impl Render for StatsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Read on the first frame rather than in `new`, which runs before the
        // config database is open.
        if !std::mem::replace(&mut self.charts_loaded, true) {
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

        let mode_row = (self.available_modes.len() > 1).then(|| {
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div().text_xs().text_color(crate::theme::text_dim()).child(t!("ui.stats.mode_label").to_string()),
                )
                .child(selectable(
                    "stats-mode-all",
                    self.filters.game_modes.is_empty(),
                    Button::new("stats-mode-all-button")
                        .label(t!("ui.stats.div_all").to_string())
                        .compact()
                        .selected(self.filters.game_modes.is_empty())
                        .on_click(cx.listener(|this, _event, _window, cx| this.clear_modes(cx))),
                ))
                .children(self.available_modes.iter().enumerate().map(|(index, mode)| {
                    let mode = mode.clone();
                    let chosen = self.filters.game_modes.contains(&mode);
                    selectable(
                        ("stats-mode", index),
                        chosen,
                        Button::new(("stats-mode-button", index))
                            .label(match_group_display_name(&mode).to_string())
                            .compact()
                            .selected(chosen)
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                let mode = mode.clone();
                                this.toggle_mode(&mode, cx);
                            })),
                    )
                }))
        });

        let filter_bar = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(
                Checkbox::new("stats-limit-enabled")
                    .label(t!("ui.stats.limit_to_recent").to_string())
                    .checked(limited)
                    .on_click(cx.listener(|this, checked: &bool, _window, cx| this.set_limit_enabled(*checked, cx))),
            )
            .child(
                // NumberInput carries no id of its own, so the wrapper is what
                // the interaction tests address.
                div()
                    .id("stats-limit-count")
                    .test_support()
                    .w(px(90.))
                    .child(NumberInput::new(&self.limit_input).small().disabled(!limited)),
            )
            .child(crate::ui::rule_v(cx))
            .child(
                div().text_xs().text_color(crate::theme::text_dim()).child(t!("ui.stats.division_label").to_string()),
            )
            .children(division_buttons)
            .when_some(mode_row, |this, row| this.child(crate::ui::rule_v(cx)).child(row))
            .child(crate::ui::rule_v(cx))
            .child(add_chart_menu(cx.entity()))
            .child(div().flex_1())
            .when_some(self.clear_error.clone(), |this, reason| {
                this.child(div().text_xs().text_color(rgb(crate::theme::semantic().error)).child(reason))
            })
            // Two presses rather than a dialog: the first says what the second
            // will do, and clicking anything else forgets it. The egui tab
            // asks the same question through its confirm panel.
            .child(
                Button::new("stats-clear")
                    .child(crate::icons::icon(crate::icons::ERASER))
                    .label(if self.clear_armed {
                        t!("ui.stats.clear_confirm").into_owned()
                    } else {
                        t!("ui.stats.clear").into_owned()
                    })
                    .compact()
                    .selected(self.clear_armed)
                    .disabled(self.games.is_empty())
                    .tooltip(t!("ui.stats.clear_tooltip").to_string())
                    .on_click(cx.listener(|this, _event, _window, cx| this.clear_session(cx))),
            );

        v_flex()
            .id("stats-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(filter_bar)
            .child(div().flex_1().min_h(px(0.)).child(self.dock_area.clone()))
    }
}
