//! The Stats tab: a filter bar that applies to every sub-tab, over a dock
//! holding the Overview.
//!
//! Mirrors the egui app's `build_stats_tab`: the limit, division and game-mode
//! controls sit above the dock and narrow what all of it shows.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::DockArea;
use gpui_kit::component::dock::DockPlacement;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::input::NumberInput;
use gpui_kit::component::input::NumberInputEvent;
use gpui_kit::component::input::StepAction;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::GameLimit;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::all_match_groups;
use wows_toolkit_viewmodel::stats::filter_games;
use wows_toolkit_viewmodel::stats::match_group_display_name;
use wows_toolkit_viewmodel::stats::setting_keys;

use crate::ui::selectable;

use super::chart_panel::StatsChartPanel;
use super::load::SessionData;
use super::overview::StatsOverviewPanel;
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
    /// Handed to every panel so the rating is computed against one table.
    personal_rating: Option<std::sync::Arc<wows_toolkit_viewmodel::personal_rating::PersonalRatingData>>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl StatsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let overview = cx.new(StatsOverviewPanel::new);
        let dock_area = cx.new(|cx| DockArea::new("stats-dock", None, window, cx));
        dock_area.update(cx, |dock, cx| {
            dock.add_panel(overview.clone(), DockPlacement::Center, None, window, cx);
        });
        let ships = cx.new(StatsShipsPanel::new);
        dock_area.update(cx, |dock, cx| {
            dock.add_panel(ships.clone(), DockPlacement::Center, None, window, cx);
        });
        let first_chart = cx.new(|cx| StatsChartPanel::new(0, cx));
        dock_area.update(cx, |dock, cx| {
            dock.add_panel(first_chart.clone(), DockPlacement::Center, None, window, cx);
        });

        let limit_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(DEFAULT_GAME_LIMIT.to_string())
                .pattern(regex::Regex::new(r"^\d*$").expect("a digits-only pattern is a valid regex"))
        });

        let subscriptions = vec![
            cx.subscribe_in(&limit_input, window, Self::on_limit_step),
            cx.subscribe_in(&limit_input, window, Self::on_limit_changed),
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
            personal_rating: None,
            focus_handle: cx.focus_handle(),
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
        let filtered = filter_games(&self.games, &self.filters);
        self.overview.update(cx, |panel, cx| panel.set_games(&filtered, cx));
        self.ships.update(cx, |panel, cx| panel.set_games(&filtered, cx));
        for chart in &self.charts {
            chart.update(cx, |panel, cx| panel.set_games(&filtered, cx));
        }
        cx.notify();
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

    /// Opens another chart sub-tab, seeded with what is on screen now.
    fn add_chart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_chart_id;
        self.next_chart_id += 1;

        let chart = cx.new(|cx| StatsChartPanel::new(id, cx));
        let filtered = filter_games(&self.games, &self.filters);
        let table = self.personal_rating.clone();
        chart.update(cx, |panel, cx| {
            panel.set_personal_rating(table, cx);
            panel.set_games(&filtered, cx);
        });

        self.dock_area.update(cx, |dock, cx| {
            dock.add_panel(chart.clone(), DockPlacement::Center, None, window, cx);
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

impl Focusable for StatsView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for StatsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                .child(div().text_xs().opacity(0.6).child("Mode:"))
                .child(selectable(
                    "stats-mode-all",
                    self.filters.game_modes.is_empty(),
                    Button::new("stats-mode-all-button")
                        .label("All")
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
            .flex_wrap()
            .gap_3()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(
                Checkbox::new("stats-limit-enabled")
                    .label("Limit to recent games")
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
            .child(div().text_xs().opacity(0.6).child("Division:"))
            .children(division_buttons)
            .when_some(mode_row, |this, row| this.child(row))
            .child(
                Button::new("stats-add-chart")
                    .label("Add chart")
                    .compact()
                    .on_click(cx.listener(|this, _event, window, cx| this.add_chart(window, cx))),
            );

        v_flex()
            .id("stats-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(filter_bar)
            .child(div().flex_1().min_h(px(0.)).child(self.dock_area.clone()))
    }
}
