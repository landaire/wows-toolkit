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
use rust_i18n::t;

use std::sync::Arc;

use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::PerformanceInfo;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::all_match_groups;
use wows_toolkit_viewmodel::stats::chart::ChartBar;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartSeries;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::chart::bar_chart_series;
use wows_toolkit_viewmodel::stats::chart::line_chart_series;
use wows_toolkit_viewmodel::stats::chart::ships_played;
use wows_toolkit_viewmodel::stats::filter_games_per_ship;
use wows_toolkit_viewmodel::stats::match_group_display_name;
use wows_toolkit_viewmodel::stats::per_ship_performance;

use crate::ui::selectable;

use super::plot;
use super::plot::PlotView;
use super::plot_image;

/// The settings menu's box. Wide enough for a ship name, and no wider than a
/// menu needs to be; a session spanning dozens of ships scrolls rather than
/// covering the plot.
const SETTINGS_MIN_WIDTH: Pixels = px(240.);
const SETTINGS_MAX_WIDTH: Pixels = px(320.);
const SETTINGS_MAX_HEIGHT: Pixels = px(420.);

/// Distinguishes one chart pane's element ids from another's.
pub type ChartId = usize;

/// One chart pane as a settings row remembers it.
///
/// The games themselves are not here: they come from the session, and a
/// chart is a way of looking at them rather than a copy.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChartSettings {
    #[serde(default)]
    pub id: Option<ChartId>,
    #[serde(default)]
    pub stat: ChartableStat,
    #[serde(default)]
    pub mode: ChartMode,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub combined: bool,
    #[serde(default)]
    pub show_values: bool,
    /// The chart's own narrowing, when it was taken off the tab's.
    #[serde(default)]
    pub own_filters: Option<StatsFilters>,
    /// The ships plotted, when the reader picked them. Empty follows the
    /// session, which is what a new chart does.
    #[serde(default)]
    pub selected_ships: Vec<u64>,
}

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
    /// Every recorded game, before any narrowing, so an override can select
    /// from the whole session rather than from the tab's selection.
    all_games: Vec<PerGameStat>,
    /// What the tab's filter bar currently says, followed unless this chart
    /// has been taken off it.
    tab_filters: StatsFilters,
    /// This chart's own narrowing. `None` follows the tab, which is what a
    /// new chart does; `Some` is an override the reader set here.
    own_filters: Option<StatsFilters>,
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
    /// Where the plot last drew, so a copy is the size it is on screen.
    /// `None` before the first frame, which is when there is nothing to copy.
    plot_bounds: Option<Bounds<Pixels>>,
    /// Where the pointer went down and where it was last seen, while a drag
    /// is panning the plot.
    drag: Option<Point<Pixels>>,
    /// Absent until the expected values are cached, which is what decides
    /// whether personal rating is offered at all.
    personal_rating: Option<Arc<PersonalRatingData>>,
    focus_handle: FocusHandle,
}

/// This chart was set up differently, so the tab writes the set back.
pub struct ChartSettingsChanged;

impl EventEmitter<PanelEvent> for StatsChartPanel {}
impl EventEmitter<ChartSettingsChanged> for StatsChartPanel {}

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
            all_games: Vec::new(),
            tab_filters: StatsFilters::default(),
            own_filters: None,
            ships: Vec::new(),
            played: Vec::new(),
            selected_ships: Vec::new(),
            selection_touched: false,
            view: PlotView::default(),
            plot_bounds: None,
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
    /// Says the chart is set up differently now, so the tab remembers it.
    fn settings_changed(&self, cx: &mut Context<Self>) {
        cx.emit(ChartSettingsChanged);
    }

    /// How this chart is currently set up.
    pub fn settings(&self) -> ChartSettings {
        ChartSettings {
            id: Some(self.id),
            stat: self.stat,
            mode: self.mode,
            running: self.running,
            combined: self.combined,
            show_values: self.show_values,
            own_filters: self.own_filters.clone(),
            selected_ships: if self.selection_touched {
                self.selected_ships.iter().map(|id| id.raw()).collect()
            } else {
                Vec::new()
            },
        }
    }

    /// Puts `settings` back on this chart.
    ///
    /// Called before the session is handed over, so the narrowing is in place
    /// the first time the games are filtered.
    pub fn apply_settings(&mut self, settings: ChartSettings, cx: &mut Context<Self>) {
        if let Some(id) = settings.id {
            self.id = id;
        }
        self.stat = settings.stat;
        self.mode = settings.mode;
        self.running = settings.running;
        self.combined = settings.combined;
        self.show_values = settings.show_values;
        self.own_filters = settings.own_filters;
        if !settings.selected_ships.is_empty() {
            self.selected_ships = settings.selected_ships.into_iter().map(GameParamId::from).collect();
            self.selection_touched = true;
        }
        cx.notify();
    }

    pub fn id(&self) -> ChartId {
        self.id
    }

    /// Adopts the whole session and the tab's filters.
    ///
    /// Both, not just the selection the tab made: a chart the reader has
    /// taken off the tab's filters narrows the session itself, and one that
    /// has not needs the tab's answer.
    pub fn set_games(&mut self, all_games: &[PerGameStat], tab_filters: &StatsFilters, cx: &mut Context<Self>) {
        self.all_games = all_games.to_vec();
        self.tab_filters = tab_filters.clone();
        self.rebuild(cx);
    }

    /// Every match group the session holds, which is what an override can
    /// narrow to.
    fn offered_modes(&self) -> Vec<String> {
        all_match_groups(&self.all_games).into_iter().collect()
    }

    /// The filters this chart is drawn under: its own if it has any, the
    /// tab's otherwise.
    fn active_filters(&self) -> &StatsFilters {
        self.own_filters.as_ref().unwrap_or(&self.tab_filters)
    }

    /// Whether this chart narrows the session itself rather than following
    /// the tab's filter bar.
    fn overrides_tab(&self) -> bool {
        self.own_filters.is_some()
    }

    /// Takes this chart off the tab's filters, or puts it back on them.
    ///
    /// Taking it off starts from what the tab currently says, so the plot
    /// does not jump the moment the override is turned on.
    fn set_overrides_tab(&mut self, overrides: bool, cx: &mut Context<Self>) {
        self.own_filters = overrides.then(|| self.tab_filters.clone());
        self.settings_changed(cx);
        self.rebuild(cx);
    }

    fn set_own_division(&mut self, division: DivisionFilter, cx: &mut Context<Self>) {
        let Some(filters) = self.own_filters.as_mut() else { return };
        filters.division = division;
        self.settings_changed(cx);
        self.rebuild(cx);
    }

    fn toggle_own_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
        let Some(filters) = self.own_filters.as_mut() else { return };
        if !filters.game_modes.remove(mode) {
            filters.game_modes.insert(mode.to_string());
        }
        self.rebuild(cx);
    }

    fn clear_own_modes(&mut self, cx: &mut Context<Self>) {
        let Some(filters) = self.own_filters.as_mut() else { return };
        filters.game_modes.clear();
        self.rebuild(cx);
    }

    /// Re-narrows the session and rebuilds everything drawn from it.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        // Per ship, as the egui charts count it (`ui/stats_tab.rs:541`): a chart
        // of ten ships under "last 25" plots 25 battles in each.
        let games = filter_games_per_ship(&self.all_games, self.active_filters());
        self.ships = per_ship_performance(&games);
        self.games = games.iter().map(|game| (*game).clone()).collect();
        self.played = ships_played(&games);
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
    /// Draws the chart again into an image and puts it on the clipboard.
    ///
    /// Drawn rather than captured: GPUI will not hand back a rendered window
    /// (`render_to_image` is unimplemented off the test platform), so the
    /// same plot description goes through an image canvas instead. The
    /// geometry is identical; the glyphs come from the system's own fonts,
    /// which is also what lets a Japanese or Russian label render.
    fn copy_as_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.plot_bounds else { return };
        let width = bounds.size.width.as_f32().round() as u32;
        let height = bounds.size.height.as_f32().round() as u32;

        let series = self.series();
        let bars = self.bars();
        let x_label = if self.mode == ChartMode::Bar {
            t!("ui.stats.column_ship").into_owned()
        } else {
            t!("ui.stats.axis_game").into_owned()
        };
        let value_label = self.value_label();
        let plot = plot::Plot {
            series: &series,
            bars: &bars,
            x_label: &x_label,
            y_label: &value_label,
            show_values: self.show_values,
            view: self.view,
        };

        let theme = cx.theme();
        let colors = plot::Colors { axis: theme.border, grid: theme.border.opacity(0.4), text: theme.muted_foreground };

        let Some((width, height, pixels)) = plot_image::render(&plot, colors, width, height) else {
            crate::toast::failed(t!("ui.stats.copy_image_failed").into_owned(), window, cx);
            return;
        };

        // GPUI's clipboard carries no image format, so this goes through
        // `arboard` directly, as the file-list copy in the replay browser
        // does.
        let image = arboard::ImageData { width: width as usize, height: height as usize, bytes: pixels.into() };
        match arboard::Clipboard::new().and_then(|mut board| board.set_image(image)) {
            Ok(()) => crate::toast::ok(t!("ui.stats.copy_image_done").into_owned(), window, cx),
            Err(err) => {
                tracing::warn!("stats: the chart image could not be copied: {err}");
                crate::toast::failed(t!("ui.stats.copy_image_failed").into_owned(), window, cx);
            }
        }
    }

    fn value_label(&self) -> String {
        if self.mode == ChartMode::Bar {
            return t!("ui.stats.series_average", stat = self.stat.label()).into_owned();
        }
        if self.running || self.combined {
            return t!("ui.stats.series_running", stat = self.stat.label()).into_owned();
        }
        self.stat.label().to_string()
    }

    fn tab_title(&self) -> String {
        let stat = self.stat.label();
        match self.mode {
            ChartMode::Bar if self.stat != ChartableStat::WinRate => t!("stat.avg_prefix", name = stat).into_owned(),
            ChartMode::Bar => stat,
            ChartMode::Line if self.combined => {
                format!("{stat} {}", t!("chart.combined_suffix"))
            }
            ChartMode::Line if self.running => {
                format!("{stat} {}", t!("chart.rolling_average_suffix"))
            }
            ChartMode::Line => stat,
        }
    }

    fn set_mode(&mut self, mode: ChartMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.settle_stat();
        self.view.reset();
        self.settings_changed(cx);
        cx.notify();
    }

    pub(crate) fn set_stat(&mut self, stat: ChartableStat, cx: &mut Context<Self>) {
        if self.stat == stat {
            return;
        }
        self.stat = stat;
        self.view.reset();
        self.settings_changed(cx);
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
        self.settings_changed(cx);
        cx.notify();
    }

    fn set_running(&mut self, running: bool, cx: &mut Context<Self>) {
        self.running = running;
        self.settle_stat();
        self.view.reset();
        self.settings_changed(cx);
        cx.notify();
    }

    fn set_show_values(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_values = show;
        self.settings_changed(cx);
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
        self.settings_changed(cx);
        cx.notify();
    }

    fn select_all_ships(&mut self, cx: &mut Context<Self>) {
        self.selection_touched = true;
        self.selected_ships = self.played.iter().map(|(id, _)| *id).collect();
        self.settings_changed(cx);
        cx.notify();
    }

    fn select_no_ships(&mut self, cx: &mut Context<Self>) {
        self.selection_touched = true;
        self.selected_ships.clear();
        self.settings_changed(cx);
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
        let overrides = self.overrides_tab();
        let active = self.active_filters().clone();
        let modes = self.offered_modes();

        let trigger = Button::new(("chart-settings", id)).label(t!("ui.stats.settings").to_string()).compact();
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

            let filters_entity = entity.clone();
            let division_entity = entity.clone();
            let mode_filter_entity = entity.clone();
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
                .min_w(SETTINGS_MIN_WIDTH)
                .max_w(SETTINGS_MAX_WIDTH)
                .max_h(SETTINGS_MAX_HEIGHT)
                .gap_2()
                .p_2()
                .id(("chart-settings-body", id))
                .overflow_scroll()
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.stats.statistic").to_string()))
                .child(h_flex().flex_wrap().gap_1().children(stat_buttons))
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.stats.chart").to_string()))
                .child(h_flex().gap_1().children(mode_buttons))
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.stats.options").to_string()))
                .child(
                    Checkbox::new(("chart-combined", id))
                        .label(t!("ui.stats.combine_ships").to_string())
                        .checked(combined)
                        .on_click(move |checked, _window, cx| {
                            let checked = *checked;
                            combined_entity.update(cx, |this, cx| this.set_combined(checked, cx));
                        }),
                )
                .when(mode == ChartMode::Line, |this| {
                    this.child(
                        Checkbox::new(("chart-running", id))
                            .label(t!("ui.stats.running_average").to_string())
                            .checked(running || combined)
                            // Combining the ships is already a running line.
                            .disabled(combined)
                            .on_click(move |checked, _window, cx| {
                                let checked = *checked;
                                running_entity.update(cx, |this, cx| this.set_running(checked, cx));
                            }),
                    )
                })
                .child(
                    Checkbox::new(("chart-values", id))
                        .label(t!("ui.stats.value_labels").to_string())
                        .checked(show_values)
                        .on_click(move |checked, _window, cx| {
                            let checked = *checked;
                            values_entity.update(cx, |this, cx| this.set_show_values(checked, cx));
                        }),
                )
                // This chart's own narrowing, when it has been taken off
                // the tab's bar. The tab's filters stay the default, so a
                // reader who never opens this sees one set of filters.
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.stats.filters").to_string()))
                .child({
                    let entity = filters_entity.clone();
                    Checkbox::new(("chart-own-filters", id))
                        .label(t!("ui.stats.own_filters").to_string())
                        .checked(overrides)
                        .tooltip(t!("ui.stats.own_filters_tooltip").to_string())
                        .on_click(move |checked, _window, cx| {
                            let checked = *checked;
                            entity.update(cx, |this, cx| this.set_overrides_tab(checked, cx));
                        })
                })
                .when(overrides, |this| {
                    let division_entity = division_entity.clone();
                    let mode_entity = mode_filter_entity.clone();
                    let all_modes_entity = mode_filter_entity.clone();
                    this.child(h_flex().gap_1().flex_wrap().children(DivisionFilter::ALL.into_iter().map(
                        move |offered| {
                            let entity = division_entity.clone();
                            let chosen = active.division == offered;
                            Button::new(("chart-division", id * DivisionFilter::ALL.len() + offered as usize))
                                .label(t!(offered.translation_key()).to_string())
                                .compact()
                                .selected(chosen)
                                .on_click(move |_event, _window, cx| {
                                    entity.update(cx, |this, cx| this.set_own_division(offered, cx));
                                })
                        },
                    )))
                    .when(modes.len() > 1, |this| {
                        let modes = modes.clone();
                        let chosen_modes = active.game_modes.clone();
                        this.child(
                            h_flex()
                                .gap_1()
                                .flex_wrap()
                                .child(
                                    Button::new(("chart-mode-all", id))
                                        .label(t!("ui.stats.all_ships").to_string())
                                        .compact()
                                        .selected(chosen_modes.is_empty())
                                        .on_click(move |_event, _window, cx| {
                                            all_modes_entity.update(cx, |this, cx| this.clear_own_modes(cx));
                                        }),
                                )
                                .children(modes.into_iter().enumerate().map(move |(ix, group)| {
                                    let entity = mode_entity.clone();
                                    let chosen = chosen_modes.contains(&group);
                                    let picked = group.clone();
                                    Button::new(("chart-mode-filter", id * 64 + ix))
                                        .label(match_group_display_name(&group).to_string())
                                        .compact()
                                        .selected(chosen)
                                        .on_click(move |_event, _window, cx| {
                                            let picked = picked.clone();
                                            entity.update(cx, |this, cx| this.toggle_own_mode(&picked, cx));
                                        })
                                })),
                        )
                    })
                })
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.stats.ships").to_string()))
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new(("chart-ships-all", id))
                                .label(t!("ui.stats.all_ships").to_string())
                                .compact()
                                .on_click(move |_event, _window, cx| {
                                    all_entity.update(cx, |this, cx| this.select_all_ships(cx));
                                }),
                        )
                        .child(
                            Button::new(("chart-ships-none", id))
                                .label(t!("ui.stats.no_ships").to_string())
                                .compact()
                                .on_click(move |_event, _window, cx| {
                                    none_entity.update(cx, |this, cx| this.select_no_ships(cx));
                                }),
                        ),
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

    fn dump(&self, _cx: &App) -> gpui_kit::component::dock::PanelState {
        gpui_kit::component::dock::PanelState {
            panel_name: "StatsChartPanel".to_string(),
            children: Vec::new(),
            info: gpui_kit::component::dock::PanelInfo::panel(serde_json::json!({ "id": self.id })),
        }
    }
}

impl Panel for StatsChartPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1()
            .items_center()
            .child(crate::icons::icon(if self.mode == ChartMode::Line {
                crate::icons::CHART_LINE
            } else {
                crate::icons::CHART_BAR
            }))
            .child(self.tab_title())
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
            .child(div().flex_1())
            .child(
                Button::new(("chart-copy-image", id))
                    .label(t!("ui.stats.copy_image").to_string())
                    .compact()
                    .xsmall()
                    .on_click(cx.listener(|this, _event, window, cx| this.copy_as_image(window, cx))),
            )
            .when(!self.view.is_default(), |this| {
                this.child(
                    Button::new(("chart-reset-view", id))
                        .label(t!("ui.stats.reset_view").to_string())
                        .compact()
                        .xsmall()
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.view.reset();
                            cx.notify();
                        })),
                )
            });

        let x_label = if self.mode == ChartMode::Bar {
            t!("ui.stats.column_ship").into_owned()
        } else {
            t!("ui.stats.axis_game").into_owned()
        };
        let value_label = self.value_label();
        let view = self.view;
        let show_values = self.show_values;
        let measured = cx.entity();
        let surface = canvas(
            move |bounds, _window, cx| {
                measured.update(cx, |this: &mut Self, _cx| this.plot_bounds = Some(bounds));
            },
            move |bounds, _prepaint, window, cx| {
                plot::paint(
                    &plot::Plot {
                        series: &series,
                        bars: &bars,
                        x_label: &x_label,
                        y_label: &value_label,
                        show_values,
                        view,
                    },
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
                .child(
                    div()
                        .text_sm()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.stats.nothing_to_plot").to_string()),
                )
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

        v_flex()
            .size_full()
            .child(toolbar)
            .child(
                // Titled over the plot, centred, the way the egui chart heads
                // its own group.
                div()
                    .flex_none()
                    .w_full()
                    .py_1()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_center()
                    .child(self.value_label()),
            )
            .child(div().flex_1().min_h(px(0.)).p_2().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::ChartSettings;
    use wows_toolkit_viewmodel::stats::DivisionFilter;
    use wows_toolkit_viewmodel::stats::GameLimit;
    use wows_toolkit_viewmodel::stats::StatsFilters;
    use wows_toolkit_viewmodel::stats::chart::ChartMode;
    use wows_toolkit_viewmodel::stats::chart::ChartableStat;

    /// A chart comes back set up the way it was left, override and ship
    /// selection included.
    #[test]
    fn a_chart_round_trips_through_the_settings_row() {
        let saved = ChartSettings {
            stat: ChartableStat::Frags,
            mode: ChartMode::Bar,
            running: true,
            combined: true,
            show_values: true,
            own_filters: Some(StatsFilters {
                limit: GameLimit::Recent(25),
                division: DivisionFilter::DivOnly,
                game_modes: ["RandomBattle".to_string()].into_iter().collect(),
            }),
            selected_ships: vec![4288575440, 3541279184],
        };

        let json = serde_json::to_string(&saved).expect("a chart serializes");
        let read: ChartSettings = serde_json::from_str(&json).expect("and reads back");
        assert_eq!(read, saved);

        // A row written before a field existed still reads, on the defaults.
        let older: ChartSettings = serde_json::from_str("{}").expect("an empty row reads");
        assert_eq!(older, ChartSettings::default());
    }
}
