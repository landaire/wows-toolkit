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
use gpui_kit::component::dock::DockArea;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::dock::PanelId;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
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
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::chart::ChartBar;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartSeries;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::chart_model::ChartId;
use wows_toolkit_viewmodel::stats::chart_model::ChartModel;
use wows_toolkit_viewmodel::stats::chart_model::ChartSettings;
use wows_toolkit_viewmodel::stats::match_group_display_name;
use wows_toolkit_viewmodel::stats::plot::PlotPoint;
use wows_toolkit_viewmodel::stats::plot_image;

use crate::ui::selectable;

use super::plot;

/// The settings menu's box. Wide enough for a ship name, and no wider than a
/// menu needs to be; a session spanning dozens of ships scrolls rather than
/// covering the plot.
const SETTINGS_MIN_WIDTH: Pixels = px(240.);
const SETTINGS_MAX_WIDTH: Pixels = px(320.);
const SETTINGS_MAX_HEIGHT: Pixels = px(420.);

pub struct StatsChartPanel {
    /// What the chart plots and every rule between its controls.
    model: ChartModel,
    ship_search: Entity<InputState>,
    _ship_search_subscription: Subscription,
    /// Where the plot last drew, so a copy is the size it is on screen.
    /// `None` before the first frame, which is when there is nothing to copy.
    plot_bounds: Option<Bounds<Pixels>>,
    /// Where the pointer was last seen while a drag is panning the plot.
    drag: Option<Point<Pixels>>,
    /// The tab's dock, for the header control that collapses the chart dock.
    dock: WeakEntity<DockArea>,
    focus_handle: FocusHandle,
}

/// This chart was set up differently, so the tab writes the set back.
pub struct ChartSettingsChanged;

impl EventEmitter<PanelEvent> for StatsChartPanel {}
impl EventEmitter<ChartSettingsChanged> for StatsChartPanel {}

impl StatsChartPanel {
    pub fn new(id: ChartId, dock: WeakEntity<DockArea>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ship_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.stats.search_ships").into_owned()));
        let ship_search_subscription = cx.subscribe(&ship_search, |_this, _state, _event: &InputEvent, cx| cx.notify());
        Self {
            model: ChartModel::new(id),
            ship_search,
            _ship_search_subscription: ship_search_subscription,
            plot_bounds: None,
            drag: None,
            dock,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Adopts the expected-values table. A pane plotting personal rating when
    /// the table turns out to be unavailable falls back rather than plotting
    /// a run of zeroes.
    pub fn set_personal_rating(&mut self, table: Option<Arc<PersonalRatingData>>, cx: &mut Context<Self>) {
        self.model.set_personal_rating(table);
        cx.notify();
    }

    /// Says the chart is set up differently now, so the tab remembers it.
    fn settings_changed(&self, cx: &mut Context<Self>) {
        cx.emit(ChartSettingsChanged);
        cx.notify();
    }

    /// How this chart is currently set up.
    pub fn settings(&self) -> ChartSettings {
        self.model.settings()
    }

    /// Puts `settings` back on this chart.
    pub fn apply_settings(&mut self, settings: ChartSettings, cx: &mut Context<Self>) {
        self.model.apply_settings(settings);
        cx.notify();
    }

    pub fn id(&self) -> ChartId {
        self.model.id()
    }

    /// Adopts the whole session and the tab's filters.
    pub fn set_games(&mut self, all_games: &[PerGameStat], tab_filters: &StatsFilters, cx: &mut Context<Self>) {
        self.model.set_games(all_games, tab_filters);
        cx.notify();
    }

    fn set_overrides_tab(&mut self, overrides: bool, cx: &mut Context<Self>) {
        self.model.set_overrides_tab(overrides);
        self.settings_changed(cx);
    }

    fn set_own_division(&mut self, division: DivisionFilter, cx: &mut Context<Self>) {
        if self.model.set_own_division(division) {
            self.settings_changed(cx);
        }
    }

    fn toggle_own_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
        if self.model.toggle_own_mode(mode) {
            self.settings_changed(cx);
        }
    }

    fn clear_own_modes(&mut self, cx: &mut Context<Self>) {
        if self.model.clear_own_modes() {
            self.settings_changed(cx);
        }
    }

    /// The lines to draw, empty in bar mode.
    fn series(&self) -> Vec<ChartSeries> {
        self.model.series()
    }

    /// The bars to draw, empty in line mode.
    fn bars(&self) -> Vec<ChartBar> {
        self.model.bars()
    }

    /// Draws the chart again into an image and puts it on the clipboard.
    ///
    /// Drawn rather than captured: GPUI will not hand back a rendered window
    /// (`render_to_image` is unimplemented off the test platform), so the
    /// same plot description goes through an image canvas instead. The
    /// geometry is identical; the glyphs come from the system's own fonts,
    /// which is also what lets a Japanese or Russian label render.
    fn copy_as_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.plot_bounds else { return };
        if !plot::has_drawable_area(bounds) {
            return;
        }
        let width = bounds.size.width.as_f32().round() as u32;
        let height = bounds.size.height.as_f32().round() as u32;

        let series = self.series();
        let bars = self.bars();
        if series.is_empty() && bars.is_empty() {
            return;
        }
        let x_label = self.model.x_label();
        let value_label = self.model.value_label();
        let plot = wows_toolkit_viewmodel::stats::plot::Plot {
            series: &series,
            bars: &bars,
            x_label: &x_label,
            y_label: &value_label,
            show_values: self.model.show_values(),
            view: self.model.view(),
        };

        let rendered = plot_image::render(&plot, plot::colors(cx), plot_image::COPY_BACKGROUND, width, height, 1.0);
        let Some(mut image) = rendered else {
            crate::toast::failed(t!("ui.stats.copy_image_failed").into_owned(), window, cx);
            return;
        };
        plot_image::unpremultiply(&mut image.rgba);

        // GPUI's clipboard carries no image format, so this goes through
        // `arboard` directly, as the file-list copy in the replay browser
        // does.
        let image =
            arboard::ImageData { width: image.width as usize, height: image.height as usize, bytes: image.rgba.into() };
        match arboard::Clipboard::new().and_then(|mut board| board.set_image(image)) {
            Ok(()) => crate::toast::ok(t!("ui.stats.copy_image_done").into_owned(), window, cx),
            Err(err) => {
                tracing::warn!("stats: the chart image could not be copied: {err}");
                crate::toast::failed(t!("ui.stats.copy_image_failed").into_owned(), window, cx);
            }
        }
    }

    fn set_mode(&mut self, mode: ChartMode, cx: &mut Context<Self>) {
        if self.model.set_mode(mode) {
            self.settings_changed(cx);
        }
    }

    pub(crate) fn set_stat(&mut self, stat: ChartableStat, cx: &mut Context<Self>) {
        if self.model.set_stat(stat) {
            self.settings_changed(cx);
        }
    }

    fn set_combined(&mut self, combined: bool, cx: &mut Context<Self>) {
        self.model.set_combined(combined);
        self.settings_changed(cx);
    }

    fn set_running(&mut self, running: bool, cx: &mut Context<Self>) {
        self.model.set_running(running);
        self.settings_changed(cx);
    }

    fn set_show_values(&mut self, show: bool, cx: &mut Context<Self>) {
        self.model.set_show_values(show);
        self.settings_changed(cx);
    }

    fn toggle_ship(&mut self, ship: GameParamId, cx: &mut Context<Self>) {
        self.model.toggle_ship(ship);
        self.settings_changed(cx);
    }

    fn set_ships_selected(&mut self, ships: &[GameParamId], selected: bool, cx: &mut Context<Self>) {
        self.model.set_ships_selected(ships, selected);
        self.settings_changed(cx);
    }

    fn start_drag(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        if event.click_count >= 2 {
            self.drag = None;
            self.model.view_mut().reset();
            cx.notify();
            return;
        }
        self.drag = Some(event.position);
    }

    fn drag_view(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(last) = self.drag else { return };
        self.model.view_mut().drag((event.position.x - last.x).as_f32(), (event.position.y - last.y).as_f32());
        self.drag = Some(event.position);
        cx.notify();
    }

    fn end_drag(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    fn zoom_view(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(window.line_height()).y.as_f32();
        if delta == 0.0 {
            return;
        }
        let cursor = PlotPoint::new(event.position.x.as_f32(), event.position.y.as_f32());
        self.model.view_mut().zoom_about(if delta > 0.0 { 1.1 } else { 1.0 / 1.1 }, cursor);
        cx.notify();
    }

    fn settings_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.model.id().0;
        let entity = cx.entity();
        let stats = self.model.selectable_stats();
        let (stat, mode, running, combined, show_values) = (
            self.model.stat(),
            self.model.mode(),
            self.model.running(),
            self.model.combined(),
            self.model.show_values(),
        );
        let overrides = self.model.overrides_tab();
        let active = self.model.effective_filters();
        let modes = self.model.offered_modes();

        let trigger = Button::new(("chart-settings", id))
            .child(crate::icons::icon(crate::icons::GEAR_FINE))
            .compact()
            .tooltip(t!("ui.stats.settings").to_string())
            .accessibility_label(t!("ui.stats.settings").to_string());
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
                                        .label(t!("ui.stats.div_all").to_string())
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
                .into_any_element()
        })
    }

    fn ship_filter_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.model.id().0;
        let entity = cx.entity();
        let played = self.model.played().to_vec();
        let ship_search = self.ship_search.clone();
        let ship_query = self.ship_search.read(cx).value().to_string();
        let normalized_query = wows_toolkit_viewmodel::search::normalize_ship_search(&ship_query);
        let selected = self.model.selected_ships().to_vec();
        let selected_count = played.iter().filter(|(ship_id, _)| selected.contains(ship_id)).count();
        let total_count = played.len();
        let selection_count = t!("ui.stats.ship_selection_count", selected = selected_count, total = total_count);
        let accessibility_label = format!("{}: {selection_count}", t!("ui.stats.filter_ships"));

        let trigger = Button::new(("chart-ship-filter", id))
            .child(crate::icons::icon(crate::icons::FUNNEL))
            .child(t!("ui.stats.ships").to_string())
            .child(div().text_xs().text_color(crate::theme::text_dim()).child(selection_count.clone()))
            .compact()
            .selected(selected_count < total_count)
            .tooltip(t!("ui.stats.filter_ships").to_string())
            .accessibility_label(accessibility_label);

        Popover::new(("chart-ship-filter-menu", id)).trigger(trigger).content(move |_state, _window, _cx| {
            let visible: Vec<_> = played
                .iter()
                .filter(|(_, name)| {
                    normalized_query.is_empty()
                        || wows_toolkit_viewmodel::search::normalize_ship_search(name).contains(&normalized_query)
                })
                .cloned()
                .collect();
            let visible_ids: Vec<_> = visible.iter().map(|(ship_id, _)| *ship_id).collect();
            let no_matches = !normalized_query.is_empty() && visible.is_empty();
            let has_visible = !visible_ids.is_empty();
            let all_ids = visible_ids.clone();
            let none_ids = visible_ids;
            let ship_rows: Vec<_> = visible
                .iter()
                .map(|(ship_id, name)| {
                    let ship_id = *ship_id;
                    let entity = entity.clone();
                    Checkbox::new(SharedString::from(format!("chart-ship-{id}-{}", ship_id.raw())))
                        .label(name.clone())
                        .checked(selected.contains(&ship_id))
                        .on_click(move |_checked, _window, cx| {
                            entity.update(cx, |this, cx| this.toggle_ship(ship_id, cx));
                        })
                })
                .collect();
            let clear_search = entity.clone();
            let all_entity = entity.clone();
            let none_entity = entity.clone();

            v_flex()
                .min_w(SETTINGS_MIN_WIDTH)
                .max_w(SETTINGS_MAX_WIDTH)
                .max_h(SETTINGS_MAX_HEIGHT)
                .gap_2()
                .p_2()
                .id(("chart-ship-filter-body", id))
                .overflow_scroll()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.stats.ships").to_string()))
                        .child(div().text_xs().text_color(crate::theme::text_dim()).child(t!(
                            "ui.stats.ship_selection_count",
                            selected = selected_count,
                            total = total_count
                        ))),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(Input::new(&ship_search).id(("chart-ship-search", id)).small().flex_1())
                        .when(!ship_query.is_empty(), |this| {
                            this.child(
                                Button::new(("chart-ship-search-clear", id))
                                    .child(crate::icons::icon(crate::icons::X))
                                    .compact()
                                    .tooltip(t!("ui.stats.clear").to_string())
                                    .accessibility_label(t!("ui.stats.clear").to_string())
                                    .on_click(move |_event, window, cx| {
                                        clear_search.update(cx, |this, cx| {
                                            this.ship_search.update(cx, |state, cx| state.set_value("", window, cx));
                                        });
                                    }),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new(("chart-ships-all", id))
                                .label(t!("ui.stats.all_ships").to_string())
                                .compact()
                                .disabled(!has_visible)
                                .tooltip(t!("ui.stats.all_ships_tooltip").to_string())
                                .on_click(move |_event, _window, cx| {
                                    all_entity.update(cx, |this, cx| this.set_ships_selected(&all_ids, true, cx));
                                }),
                        )
                        .child(
                            Button::new(("chart-ships-none", id))
                                .label(t!("ui.stats.no_ships").to_string())
                                .compact()
                                .disabled(!has_visible)
                                .tooltip(t!("ui.stats.no_ships_tooltip").to_string())
                                .on_click(move |_event, _window, cx| {
                                    none_entity.update(cx, |this, cx| this.set_ships_selected(&none_ids, false, cx));
                                }),
                        ),
                )
                .when(no_matches, |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(crate::theme::text_dim())
                            .child(t!("ui.stats.no_ship_matches").to_string()),
                    )
                })
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
            info: gpui_kit::component::dock::PanelInfo::panel(serde_json::json!({ "id": self.model.id() })),
        }
    }
}

impl Panel for StatsChartPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1()
            .items_center()
            .child(crate::icons::icon(if self.model.mode() == ChartMode::Line {
                crate::icons::CHART_LINE
            } else {
                crate::icons::CHART_BAR
            }))
            .child(self.model.tab_title())
    }

    fn toolbar_buttons(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<Vec<Button>> {
        let collapse = crate::ui::dock_collapse_button(
            ("chart-collapse-dock", self.model.id().0),
            &self.dock,
            PanelId::from(cx.entity_id()),
            cx,
        )?;

        Some(vec![collapse])
    }
}

impl Render for StatsChartPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let id = self.model.id().0;
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
            .child(self.ship_filter_menu(cx))
            .child(div().flex_1())
            .when(!empty && self.plot_bounds.is_some_and(plot::has_drawable_area), |this| {
                this.child(
                    Button::new(("chart-copy-image", id))
                        .label(t!("ui.stats.copy_image").to_string())
                        .compact()
                        .xsmall()
                        .on_click(cx.listener(|this, _event, window, cx| this.copy_as_image(window, cx))),
                )
            })
            .when(!self.model.view().is_default(), |this| {
                this.child(
                    Button::new(("chart-reset-view", id))
                        .label(t!("ui.stats.reset_view").to_string())
                        .compact()
                        .xsmall()
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.model.view_mut().reset();
                            cx.notify();
                        })),
                )
            });

        let x_label = self.model.x_label();
        let value_label = self.model.value_label();
        let view = self.model.view();
        let show_values = self.model.show_values();
        let measured = cx.entity();
        let surface = canvas(
            move |bounds, _window, cx| {
                measured.update(cx, |this: &mut Self, cx| {
                    if this.plot_bounds != Some(bounds) {
                        this.plot_bounds = Some(bounds);
                        cx.notify();
                    }
                });
            },
            move |bounds, _prepaint, window, cx| {
                plot::paint(
                    &wows_toolkit_viewmodel::stats::plot::Plot {
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
            let pannable = self.model.mode() == ChartMode::Line;
            div()
                .id(("chart-surface", id))
                .size_full()
                .when(pannable, |this| {
                    this.on_mouse_down(MouseButton::Left, cx.listener(Self::start_drag))
                        .on_mouse_move(cx.listener(Self::drag_view))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::end_drag))
                        .on_mouse_up_out(MouseButton::Left, cx.listener(Self::end_drag))
                        .on_scroll(cx.listener(Self::zoom_view))
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
                    .child(self.model.value_label()),
            )
            .child(div().flex_1().min_h(px(0.)).p_2().child(body))
    }
}
