//! One chart sub-tab: a settings menu over a plot of the filtered games.
//!
//! Mirrors the egui Stats tab's chart panes, which are added and closed
//! individually and each hold their own statistic, mode, ship selection and
//! display options (`ui/stats_tab.rs`'s chart toolbar and
//! `ui/session_stats_chart.rs`). What each pane plots comes from the shared
//! series builders; the surface it is painted on is `super::plot`.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use std::sync::Arc;

use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::PerformanceInfo;
use wows_toolkit_viewmodel::stats::chart::ChartBar;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartSeries;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::chart::bar_chart_series;
use wows_toolkit_viewmodel::stats::chart::line_chart_series;
use wows_toolkit_viewmodel::stats::chart::ships_played;
use wows_toolkit_viewmodel::stats::per_ship_performance;

use crate::ui::selectable;

use super::plot;
use super::plot::PlotView;

/// The settings menu's box. Wide enough for a ship name, capped so a session
/// spanning dozens of ships scrolls rather than covering the plot.
const SETTINGS_WIDTH: Pixels = px(300.);
const SETTINGS_MAX_HEIGHT: Pixels = px(420.);

/// Distinguishes one chart pane's element ids from another's.
pub type ChartId = usize;

pub struct StatsChartPanel {
    id: ChartId,
    stat: ChartableStat,
    mode: ChartMode,
    /// Whether each point is the average of everything up to it, which is the
    /// egui pane's "rolling average".
    running: bool,
    /// Whether every selected ship is drawn as one line instead of one each.
    combined: bool,
    /// Whether each point carries its own value.
    show_values: bool,
    games: Vec<PerGameStat>,
    ships: Vec<(String, PerformanceInfo)>,
    /// The ships the games were played in, in the order first played: what
    /// the settings menu lists.
    played: Vec<(GameParamId, String)>,
    selected_ships: Vec<GameParamId>,
    /// Until the selection is touched, it follows the games: a session that
    /// gains a ship plots it rather than leaving it out of a selection made
    /// before it was played.
    selection_touched: bool,
    view: PlotView,
    /// Where the pointer went down and where it was last seen, while a drag
    /// is panning the plot.
    drag: Option<Point<Pixels>>,
    /// Absent until the expected values are cached, which is what decides
    /// whether personal rating is offered at all.
    personal_rating: Option<Arc<PersonalRatingData>>,
    focus_handle: FocusHandle,
}

impl EventEmitter<PanelEvent> for StatsChartPanel {}

impl StatsChartPanel {
    pub fn new(id: ChartId, cx: &mut Context<Self>) -> Self {
        Self {
            id,
            stat: ChartableStat::default(),
            mode: ChartMode::default(),
            running: false,
            combined: false,
            show_values: false,
            games: Vec::new(),
            ships: Vec::new(),
            played: Vec::new(),
            selected_ships: Vec::new(),
            selection_touched: false,
            view: PlotView::default(),
            drag: None,
            personal_rating: None,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Adopts the expected-values table. A pane plotting personal rating when
    /// the table turns out to be unavailable falls back rather than plotting
    /// a run of zeroes.
    pub fn set_personal_rating(&mut self, table: Option<Arc<PersonalRatingData>>, cx: &mut Context<Self>) {
        self.personal_rating = table;
        if self.stat.requires_personal_rating() && !self.can_plot_personal_rating() {
            self.stat = ChartableStat::Damage;
        }
        cx.notify();
    }

    fn can_plot_personal_rating(&self) -> bool {
        self.personal_rating.is_some()
    }

    /// Adopts the games the filter bar selected. Both shapes are kept so
    /// switching mode does not need the tab to push the data again.
    pub fn set_games(&mut self, games: &[&PerGameStat], cx: &mut Context<Self>) {
        self.ships = per_ship_performance(games);
        self.games = games.iter().map(|game| (*game).clone()).collect();
        self.played = ships_played(games);
        // Listed by name: a long session's picker has no other order anyone
        // could look a ship up in.
        self.played.sort_by(|left, right| left.1.cmp(&right.1));
        if !self.selection_touched {
            self.selected_ships = self.played.iter().map(|(id, _)| *id).collect();
        }
        cx.notify();
    }

    /// Statistics this pane can plot.
    ///
    /// Personal rating is offered only once the expected-values table is
    /// cached; without it there is nothing to rate against. Win rate and
    /// personal rating carry no meaning for a single game, so a line plots
    /// them only as a running figure -- the same rule the egui pane applies.
    fn selectable_stats(&self) -> Vec<ChartableStat> {
        ChartableStat::ALL
            .into_iter()
            .filter(|stat| !stat.requires_personal_rating() || self.can_plot_personal_rating())
            .filter(|stat| self.mode == ChartMode::Bar || stat.is_per_game() || self.running || self.combined)
            .collect()
    }

    /// The lines to draw, empty in bar mode.
    fn series(&self) -> Vec<ChartSeries> {
        if self.mode == ChartMode::Bar {
            return Vec::new();
        }
        let games: Vec<&PerGameStat> = self.games.iter().collect();
        line_chart_series(
            &games,
            self.stat,
            &self.selected_ships,
            self.personal_rating.as_deref(),
            self.running || self.combined,
            self.combined,
        )
    }

    /// The bars to draw, empty in line mode.
    fn bars(&self) -> Vec<ChartBar> {
        if self.mode == ChartMode::Line {
            return Vec::new();
        }
        bar_chart_series(&self.ships, self.stat, &self.selected_ships, self.personal_rating.as_deref())
    }

    /// What the value axis is called: the statistic, plus how it is being
    /// read when that is not the raw per-game figure.
    fn value_label(&self) -> String {
        if self.mode == ChartMode::Bar {
            return format!("{} (average)", self.stat.label());
        }
        if self.running || self.combined {
            return format!("{} (running)", self.stat.label());
        }
        self.stat.label().to_string()
    }

    fn set_mode(&mut self, mode: ChartMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.settle_stat();
        self.view.reset();
        cx.notify();
    }

    fn set_stat(&mut self, stat: ChartableStat, cx: &mut Context<Self>) {
        if self.stat == stat {
            return;
        }
        self.stat = stat;
        self.view.reset();
        cx.notify();
    }

    fn set_combined(&mut self, combined: bool, cx: &mut Context<Self>) {
        self.combined = combined;
        // One line over every ship is a line, whatever the mode said.
        if combined {
            self.mode = ChartMode::Line;
        }
        self.settle_stat();
        self.view.reset();
        cx.notify();
    }

    fn set_running(&mut self, running: bool, cx: &mut Context<Self>) {
        self.running = running;
        self.settle_stat();
        self.view.reset();
        cx.notify();
    }

    fn set_show_values(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_values = show;
        cx.notify();
    }

    /// Falls back to a statistic the current mode can actually plot.
    fn settle_stat(&mut self) {
        if !self.selectable_stats().contains(&self.stat) {
            self.stat = ChartableStat::Damage;
        }
    }

    fn toggle_ship(&mut self, ship: GameParamId, cx: &mut Context<Self>) {
        self.selection_touched = true;
        if let Some(at) = self.selected_ships.iter().position(|selected| *selected == ship) {
            self.selected_ships.remove(at);
        } else {
            self.selected_ships.push(ship);
        }
        cx.notify();
    }

    fn select_all_ships(&mut self, cx: &mut Context<Self>) {
        self.selection_touched = true;
        self.selected_ships = self.played.iter().map(|(id, _)| *id).collect();
        cx.notify();
    }

    fn select_no_ships(&mut self, cx: &mut Context<Self>) {
        self.selection_touched = true;
        self.selected_ships.clear();
        cx.notify();
    }

    fn start_drag(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        if event.click_count >= 2 {
            self.drag = None;
            self.view.reset();
            cx.notify();
            return;
        }
        self.drag = Some(event.position);
    }

    fn drag_view(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(last) = self.drag else { return };
        self.view.drag(event.position.x - last.x, event.position.y - last.y);
        self.drag = Some(event.position);
        cx.notify();
    }

    fn end_drag(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    fn zoom_view(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(window.line_height()).y.as_f32();
        if delta == 0.0 {
            return;
        }
        self.view.zoom_about(if delta > 0.0 { 1.1 } else { 1.0 / 1.1 }, event.position);
        cx.notify();
    }

    fn settings_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.id;
        let entity = cx.entity();
        let stats = self.selectable_stats();
        let (stat, mode, running, combined, show_values) =
            (self.stat, self.mode, self.running, self.combined, self.show_values);
        let played = self.played.clone();
        let selected = self.selected_ships.clone();

        let trigger = Button::new(("chart-settings", id)).label("Settings").compact();
        Popover::new(("chart-settings-menu", id)).trigger(trigger).content(move |_state, _window, _cx| {
            let stat_entity = entity.clone();
            let stat_buttons: Vec<_> = stats
                .iter()
                .copied()
                .map(|offered| {
                    let chosen = stat == offered;
                    let entity = stat_entity.clone();
                    selectable(
                        ("chart-stat", id * ChartableStat::ALL.len() + offered as usize),
                        chosen,
                        Button::new(("chart-stat-button", id * ChartableStat::ALL.len() + offered as usize))
                            .label(offered.label())
                            .compact()
                            .selected(chosen)
                            .on_click(move |_event, _window, cx| {
                                entity.update(cx, |this, cx| this.set_stat(offered, cx));
                            }),
                    )
                })
                .collect();

            let mode_entity = entity.clone();
            let mode_buttons: Vec<_> = ChartMode::ALL
                .into_iter()
                .map(|offered| {
                    let chosen = mode == offered;
                    let entity = mode_entity.clone();
                    selectable(
                        ("chart-mode", id * ChartMode::ALL.len() + offered as usize),
                        chosen,
                        Button::new(("chart-mode-button", id * ChartMode::ALL.len() + offered as usize))
                            .label(offered.label())
                            .compact()
                            .selected(chosen)
                            // One line over every ship has no bar form, so the
                            // mode is fixed while the ships are combined.
                            .disabled(combined)
                            .on_click(move |_event, _window, cx| {
                                entity.update(cx, |this, cx| this.set_mode(offered, cx));
                            }),
                    )
                })
                .collect();

            let combined_entity = entity.clone();
            let running_entity = entity.clone();
            let values_entity = entity.clone();
            let all_entity = entity.clone();
            let none_entity = entity.clone();
            let ship_entity = entity.clone();

            let ship_rows: Vec<_> = played
                .iter()
                .cloned()
                .enumerate()
                .map(|(row, (ship_id, name))| {
                    let entity = ship_entity.clone();
                    Checkbox::new(("chart-ship", id * 4096 + row))
                        .label(name)
                        .checked(selected.contains(&ship_id))
                        .on_click(move |_checked, _window, cx| {
                            entity.update(cx, |this, cx| this.toggle_ship(ship_id, cx));
                        })
                })
                .collect();

            v_flex()
                .w(SETTINGS_WIDTH)
                .max_h(SETTINGS_MAX_HEIGHT)
                .gap_2()
                .p_2()
                .id(("chart-settings-body", id))
                .overflow_scroll()
                .child(div().text_sm().font_weight(FontWeight::BOLD).child("Statistic"))
                .child(h_flex().flex_wrap().gap_1().children(stat_buttons))
                .child(div().text_sm().font_weight(FontWeight::BOLD).child("Chart"))
                .child(h_flex().gap_1().children(mode_buttons))
                .child(div().text_sm().font_weight(FontWeight::BOLD).child("Options"))
                .child(Checkbox::new(("chart-combined", id)).label("Combine ships").checked(combined).on_click(
                    move |checked, _window, cx| {
                        let checked = *checked;
                        combined_entity.update(cx, |this, cx| this.set_combined(checked, cx));
                    },
                ))
                .when(mode == ChartMode::Line, |this| {
                    this.child(
                        Checkbox::new(("chart-running", id))
                            .label("Running average")
                            .checked(running || combined)
                            // Combining the ships is already a running line.
                            .disabled(combined)
                            .on_click(move |checked, _window, cx| {
                                let checked = *checked;
                                running_entity.update(cx, |this, cx| this.set_running(checked, cx));
                            }),
                    )
                })
                .child(Checkbox::new(("chart-values", id)).label("Value labels").checked(show_values).on_click(
                    move |checked, _window, cx| {
                        let checked = *checked;
                        values_entity.update(cx, |this, cx| this.set_show_values(checked, cx));
                    },
                ))
                .child(div().text_sm().font_weight(FontWeight::BOLD).child("Ships"))
                .child(
                    h_flex()
                        .gap_1()
                        .child(Button::new(("chart-ships-all", id)).label("All").compact().on_click(
                            move |_event, _window, cx| {
                                all_entity.update(cx, |this, cx| this.select_all_ships(cx));
                            },
                        ))
                        .child(Button::new(("chart-ships-none", id)).label("None").compact().on_click(
                            move |_event, _window, cx| {
                                none_entity.update(cx, |this, cx| this.select_no_ships(cx));
                            },
                        )),
                )
                .children(ship_rows)
                .into_any_element()
        })
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
        SharedString::from(self.value_label())
    }
}

impl Render for StatsChartPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let id = self.id;
        let series = self.series();
        let bars = self.bars();
        let empty = series.is_empty() && bars.is_empty();

        let toolbar = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(self.settings_menu(cx))
            .child(div().text_sm().child(self.value_label()))
            .child(div().flex_1())
            .when(!self.view.is_default(), |this| {
                this.child(Button::new(("chart-reset-view", id)).label("Reset view").compact().xsmall().on_click(
                    cx.listener(|this, _event, _window, cx| {
                        this.view.reset();
                        cx.notify();
                    }),
                ))
            });

        let x_label = if self.mode == ChartMode::Bar { "Ship" } else { "Game" };
        let value_label = self.value_label();
        let view = self.view;
        let show_values = self.show_values;
        let surface = canvas(
            |_bounds, _window, _cx| {},
            move |bounds, _prepaint, window, cx| {
                plot::paint(
                    &plot::Plot { series: &series, bars: &bars, x_label, y_label: &value_label, show_values, view },
                    bounds,
                    window,
                    cx,
                );
            },
        )
        .size_full();

        let body: AnyElement = if empty {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().opacity(0.6).child("Nothing to plot for the current filters"))
                .into_any_element()
        } else {
            // A bar chart is a comparison of whole bars, not a curve to be
            // read into: the egui bar plot refuses drag, zoom and scroll for
            // the same reason (`ui/session_stats_chart.rs`).
            let pannable = self.mode == ChartMode::Line;
            div()
                .id(("chart-surface", id))
                .size_full()
                .when(pannable, |this| {
                    this.on_mouse_down(MouseButton::Left, cx.listener(Self::start_drag))
                        .on_mouse_move(cx.listener(Self::drag_view))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::end_drag))
                        .on_mouse_up_out(MouseButton::Left, cx.listener(Self::end_drag))
                        .on_scroll_wheel(cx.listener(Self::zoom_view))
                })
                .child(surface)
                .into_any_element()
        };

        v_flex().size_full().child(toolbar).child(div().flex_1().min_h(px(0.)).p_2().child(body))
    }
}
