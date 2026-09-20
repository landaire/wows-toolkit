//! One chart sub-tab: a statistic picker and a mode toggle over the plot.
//!
//! Mirrors the egui Stats tab's chart panes, which are added and closed
//! individually and each hold their own statistic, mode and smoothing.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Selectable;
use gpui_kit::component::button::Button;
use gpui_kit::component::chart::BarChart;
use gpui_kit::component::chart::LineChart;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::PerformanceInfo;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::chart::SeriesPoint;
use wows_toolkit_viewmodel::stats::chart::bar_series;
use wows_toolkit_viewmodel::stats::chart::line_series;
use wows_toolkit_viewmodel::stats::chart::rolling_average;
use wows_toolkit_viewmodel::stats::per_ship_performance;

use crate::ui::selectable;

/// Games averaged when smoothing is on, matching the egui chart's window.
const ROLLING_WINDOW: usize = 10;

/// Distinguishes one chart pane's element ids from another's.
pub type ChartId = usize;

pub struct StatsChartPanel {
    id: ChartId,
    stat: ChartableStat,
    mode: ChartMode,
    rolling: bool,
    games: Vec<PerGameStat>,
    ships: Vec<(String, PerformanceInfo)>,
    focus_handle: FocusHandle,
}

impl EventEmitter<PanelEvent> for StatsChartPanel {}

impl StatsChartPanel {
    pub fn new(id: ChartId, cx: &mut Context<Self>) -> Self {
        Self {
            id,
            stat: ChartableStat::default(),
            mode: ChartMode::default(),
            rolling: false,
            games: Vec::new(),
            ships: Vec::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Adopts the games the filter bar selected. Both shapes are kept so
    /// switching mode does not need the tab to push the data again.
    pub fn set_games(&mut self, games: &[&PerGameStat], cx: &mut Context<Self>) {
        self.ships = per_ship_performance(games);
        self.games = games.iter().map(|game| (*game).clone()).collect();
        cx.notify();
    }

    /// Statistics this pane can plot.
    ///
    /// Personal rating needs the expected-values table, which this port does
    /// not load yet, so it is left out rather than offered and plotted flat.
    fn selectable_stats(&self) -> Vec<ChartableStat> {
        ChartableStat::ALL
            .into_iter()
            .filter(|stat| !stat.requires_personal_rating())
            .filter(|stat| self.mode == ChartMode::Bar || stat.is_per_game())
            .collect()
    }

    fn points(&self) -> Vec<SeriesPoint> {
        match self.mode {
            ChartMode::Line => {
                let refs: Vec<&PerGameStat> = self.games.iter().collect();
                let series = line_series(&refs, self.stat);
                if self.rolling { rolling_average(&series, ROLLING_WINDOW) } else { series }
            }
            ChartMode::Bar => bar_series(&self.ships, self.stat),
        }
    }

    fn set_mode(&mut self, mode: ChartMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        // A line cannot plot win rate, so a mode switch that invalidates the
        // current statistic falls back rather than showing an empty chart.
        if !self.selectable_stats().contains(&self.stat) {
            self.stat = ChartableStat::Damage;
        }
        cx.notify();
    }

    fn set_stat(&mut self, stat: ChartableStat, cx: &mut Context<Self>) {
        if self.stat == stat {
            return;
        }
        self.stat = stat;
        cx.notify();
    }
}

impl Focusable for StatsChartPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for StatsChartPanel {
    fn panel_name(&self) -> &'static str {
        "StatsChartPanel"
    }
}

impl Panel for StatsChartPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let smoothed = if self.rolling && self.mode == ChartMode::Line { " (rolling)" } else { "" };
        SharedString::from(format!("{}{smoothed}", self.stat.label()))
    }
}

impl Render for StatsChartPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let accent = cx.theme().primary;
        let id = self.id;
        let points = self.points();

        let stat_buttons = self.selectable_stats().into_iter().map(|stat| {
            let chosen = self.stat == stat;
            selectable(
                ("chart-stat", id * ChartableStat::ALL.len() + stat as usize),
                chosen,
                Button::new(("chart-stat-button", id * ChartableStat::ALL.len() + stat as usize))
                    .label(stat.label())
                    .compact()
                    .selected(chosen)
                    .on_click(cx.listener(move |this, _event, _window, cx| this.set_stat(stat, cx))),
            )
        });

        let mode_buttons = ChartMode::ALL.map(|mode| {
            let chosen = self.mode == mode;
            selectable(
                ("chart-mode", id * ChartMode::ALL.len() + mode as usize),
                chosen,
                Button::new(("chart-mode-button", id * ChartMode::ALL.len() + mode as usize))
                    .label(mode.label())
                    .compact()
                    .selected(chosen)
                    .on_click(cx.listener(move |this, _event, _window, cx| this.set_mode(mode, cx))),
            )
        });

        let toolbar = h_flex()
            .flex_none()
            .flex_wrap()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .children(mode_buttons)
            .child(div().w(px(8.)))
            .children(stat_buttons)
            .when(self.mode == ChartMode::Line, |this| {
                this.child(
                    Checkbox::new(("chart-rolling", id)).label("Rolling average").checked(self.rolling).on_click(
                        cx.listener(|this, checked: &bool, _window, cx| {
                            this.rolling = *checked;
                            cx.notify();
                        }),
                    ),
                )
            });

        let plot: AnyElement = if points.is_empty() {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().opacity(0.6).child("Nothing to plot for the current filters"))
                .into_any_element()
        } else {
            match self.mode {
                ChartMode::Line => LineChart::new(points)
                    .x(|point: &SeriesPoint| point.label.clone())
                    .y(|point: &SeriesPoint| point.value)
                    .stroke(accent)
                    .name(self.stat.label())
                    .id(("chart-line", id))
                    .into_any_element(),
                ChartMode::Bar => BarChart::new(points)
                    .band(|point: &SeriesPoint| point.label.clone())
                    .value(|point: &SeriesPoint| point.value)
                    .fill(move |_, _, _, _| accent)
                    .name(self.stat.label())
                    .id(("chart-bar", id))
                    .into_any_element(),
            }
        };

        v_flex().size_full().child(toolbar).child(div().flex_1().min_h(px(0.)).p_2().child(plot))
    }
}
