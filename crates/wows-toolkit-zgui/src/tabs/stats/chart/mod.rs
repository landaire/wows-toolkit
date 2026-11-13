//! One chart panel: its settings menu and ship picker over a plot of the games.
//!
//! What a chart shows is decided by [`model`]; where the axes, lines and labels go is decided by
//! the shared plot layout, which the window draws through [`plot`] and a copy draws through the
//! shared image renderer.

mod model;
mod plot;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::sync::Arc;

use landaire_ui::EscapeClears;
use landaire_ui::MarkedTextProps;
use landaire_ui::Toasts;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use thiserror::Error;
use wows_replays::types::GameParamId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartSettings;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::chart::selectable_stats;
use wows_toolkit_viewmodel::stats::chart::value_axis_label;
use wows_toolkit_viewmodel::stats::match_group_display_name;
use wows_toolkit_viewmodel::stats::plot::PlotPoint;
use wows_toolkit_viewmodel::stats::plot::PlotRect;
use wows_toolkit_viewmodel::stats::plot::PlotView;
use wows_toolkit_viewmodel::stats::plot::has_drawable_area;
use wows_toolkit_viewmodel::stats::plot::image;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use zgui::component;
use zgui::css;
use zgui::custom::Custom;
use zgui::custom::CustomHandle;
use zgui::elements::Element;
use zgui::prelude::Fonts;
use zgui::prelude::PointerButton;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::reactive::use_context;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::install_stylesheet;
use zgui::vocab::ScrollDelta;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::InputProps;
use zgui_ui::LabelProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::ToggleProps;
use zgui_ui::ToggleSize;
use zgui_ui_primitives::Binding;

use crate::i18n::Locale;
use crate::icons::AppIconProps;
use crate::services::Services;
use crate::tabs::stats::chart::model::ChartModel;
use crate::tabs::stats::chart::plot::PlotData;
use crate::tabs::stats::chart::plot::PlotElement;
use crate::tabs::stats::chart::plot::ThemeColors;
use crate::text_marks::Marks;
use crate::text_marks::marks;

const SHEET: &str = "wt-stats-chart";

const CSS: &str = css!(
    ".wt-chart {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
    }
    .wt-chart__toolbar {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-chart__spacer {
        flex: 1 1 auto;
    }
    .wt-chart__title {
        flex: none;
        padding: 4px 0;
        font-size: var(--zui-type-size-sm);
        font-weight: 700;
        text-align: center;
    }
    .wt-chart__body {
        display: flex;
        flex: 1 1 auto;
        min-height: 0;
        padding: 8px;
    }
    .wt-chart__empty {
        display: flex;
        flex: 1 1 auto;
        align-items: center;
        justify-content: center;
        font-size: var(--zui-type-size-sm);
        color: var(--wt-color-text-dim);
    }
    .wt-chart__plot {
        display: block;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        --wt-plot-axis: var(--lui-color-border);
        --wt-plot-grid: color-mix(in srgb, var(--lui-color-border) 40%, transparent);
        --wt-plot-text: var(--wt-color-text-dim);
        --wt-plot-background: var(--lui-color-panel);
    }
    .wt-chart__gone {
        display: none;
    }
    .wt-chart__canvas {
        display: block;
        width: 100%;
        height: 100%;
    }
    .wt-chart__menu {
        display: flex;
        flex-direction: column;
        gap: 8px;
        min-width: 240px;
        max-width: 320px;
        max-height: 420px;
        overflow-y: auto;
        padding: 8px;
    }
    .wt-chart__heading {
        font-size: var(--zui-type-size-sm);
        font-weight: 700;
    }
    .wt-chart__heading-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
    }
    .wt-chart__wrap {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        gap: 4px;
    }
    .wt-chart__row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
    }
    .wt-chart__search {
        flex: 1 1 auto;
    }
    .wt-chart__dim {
        font-size: var(--zui-type-size-xs);
        color: var(--wt-color-text-dim);
    }
    .wt-chart__ships[data-narrowed=true] {
        background-color: var(--zui-color-secondary);
    }"
);

/// `games` is the whole scope, oldest first; `filters` is the tab's filter bar, which the chart
/// follows unless its settings carry their own. The chart writes its changes to `settings`.
#[component]
pub fn StatsChart(
    settings: RwSignal<ChartSettings>,
    #[prop(into)] games: Signal<Arc<[PerGameStat]>>,
    #[prop(into)] filters: Signal<StatsFilters>,
    #[prop(into)] personal_rating: Signal<Option<Arc<PersonalRatingData>>>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let toasts = Toasts::current();

    let model = Memo::new(move |_| {
        settings.with(|settings| {
            games.with(|games| {
                filters.with(|filters| {
                    personal_rating.with(|rating| ChartModel::new(settings, games, filters, rating.as_deref()))
                })
            })
        })
    });
    let rated = Signal::derive(move || personal_rating.with(Option::is_some));
    let view_state = RwSignal::new_local(PlotView::new());

    // A different statistic or way of plotting it is a different picture, so the view goes back
    // to the whole of it, as it does in GPUI.
    effect(move |previous: Option<(ChartableStat, ChartMode, bool, bool)>| {
        let shape = settings.with(|settings| (settings.stat, settings.mode, settings.running, settings.combined));
        if previous.is_some_and(|previous| previous != shape) {
            view_state.set(PlotView::new());
        }
        shape
    });

    let (plot, element) = PlotSurface::new(model, settings, view_state, locale);
    let empty = Signal::derive_local(move || model.with(ChartModel::is_empty));
    let copy = move || copy_image(plot.area, plot.handle, toasts);

    view! {
        box(class = "wt-chart", attr:data-testid = "stats-chart") {
            row(class = "wt-chart__toolbar") {
                SettingsMenu(settings = settings, model = model, filters = filters, rated = rated, locale = locale)
                ShipMenu(settings = settings, model = model, locale = locale)
                box(class = "wt-chart__spacer")
                box(class:wt-chart__gone = move || empty.get()) {
                    Button(size = ButtonSize::Xs, variant = ButtonVariant::Outline, attr:data-testid = "chart-copy-image", on:click = move |_| copy()) {
                        {tr(locale, || t!("ui.stats.copy_image"))}
                    }
                }
                box(class:wt-chart__gone = move || view_state.with(PlotView::is_default)) {
                    Button(size = ButtonSize::Xs, variant = ButtonVariant::Outline, attr:data-testid = "chart-reset-view", on:click = move |_| view_state.set(PlotView::new())) {
                        {tr(locale, || t!("ui.stats.reset_view"))}
                    }
                }
            }
            text(class = "wt-chart__title", attr:data-testid = "chart-title") {{move || value_label(settings, model, locale)}}
            box(class = "wt-chart__body") {
                box(class = "wt-chart__empty", class:wt-chart__gone = move || !empty.get(), attr:data-testid = "chart-empty") {
                    {tr(locale, || t!("ui.stats.nothing_to_plot"))}
                }
                {plot.view(empty, element)}
            }
        }
    }
}

/// The plot element, the box around it that takes the pointer, and the handle its drawing is
/// changed through.
#[derive(Clone, Copy)]
struct PlotSurface {
    area: NodeRef,
    handle: StoredValue<CustomHandle<PlotElement>, LocalStorage>,
    view_state: RwSignal<PlotView, LocalStorage>,
    pannable: Signal<bool, LocalStorage>,
    drag: RwSignal<Option<Drag>, LocalStorage>,
}

/// Where the pointer was last seen while the primary button is held over a line plot, in window
/// CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Pressed and not yet moved. The pointer is not captured yet, since a capture keeps the
    /// press from counting as a click and so from making a double click.
    Pressed(PlotPoint),
    /// Moving with the pointer captured, panning the view.
    Panning(PlotPoint),
}

impl PlotSurface {
    fn new(
        model: Memo<ChartModel>,
        settings: RwSignal<ChartSettings>,
        view_state: RwSignal<PlotView, LocalStorage>,
        locale: Locale,
    ) -> (Self, Element<Custom>) {
        let fonts = use_context::<Fonts>().expect("every window provides its fonts");
        let (element, handle) = zgui::custom::custom(PlotElement::new(fonts.shaper()));
        let handle = StoredValue::new_local(handle);

        // The element is retained, so what it draws is handed to it and it is told to paint again.
        effect(move |_| {
            locale.track();
            let data = model.with(|model| {
                let (x_label, y_label) = settings.with(|settings| {
                    let x_label = match settings.mode {
                        ChartMode::Bar => t!("ui.stats.column_ship").into_owned(),
                        ChartMode::Line => t!("ui.stats.axis_game").into_owned(),
                    };
                    (x_label, value_axis_label(model.stat, settings.mode, settings.running, settings.combined))
                });
                PlotData {
                    series: model.series.clone(),
                    bars: model.bars.clone(),
                    x_label,
                    y_label,
                    show_values: settings.with(|settings| settings.show_values),
                    view: view_state.get(),
                }
            });
            handle.with_value(|handle| {
                let changed = handle.update(|element| {
                    let changed = element.data != data;
                    element.data = data;
                    changed
                });
                if changed {
                    handle.repaint();
                }
            });
        });

        let surface = Self {
            area: NodeRef::new(),
            handle,
            view_state,
            // A bar chart is a comparison of whole bars, not a curve to be read into: the egui bar
            // plot refuses drag and zoom for the same reason.
            pannable: Signal::derive_local(move || settings.with(|settings| settings.mode == ChartMode::Line)),
            drag: RwSignal::new_local(None),
        };

        (surface, element)
    }

    /// Where `position`, in window CSS pixels, sits in the plot's own logical pixels.
    fn local(&self, x_css: f32, y_css: f32) -> Option<PlotPoint> {
        let bounds = self.area.window_bounds()?;
        let scale = self.area.scale();
        Some(PlotPoint::new(x_css - bounds.origin.x.0 / scale, y_css - bounds.origin.y.0 / scale))
    }

    fn view(self, empty: Signal<bool, LocalStorage>, element: Element<Custom>) -> impl IntoView {
        let Self { area, view_state, pannable, drag, .. } = self;

        view! {
            box(
                node_ref = area,
                class = "wt-chart__plot",
                class:wt-chart__gone = move || empty.get(),
                attr:data-testid = "chart-plot",
                on:pointer_down = move |ev| {
                    if !pannable.get_untracked() || ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    drag.set(Some(Drag::Pressed(PlotPoint::new(ev.position.x.0, ev.position.y.0))));
                },
                on:pointer_move = move |ev| {
                    let last = match drag.get_untracked() {
                        None => return,
                        Some(Drag::Panning(last)) => last,
                        Some(Drag::Pressed(last)) => {
                            ev.capture_pointer();
                            last
                        }
                    };
                    let (x, y) = (ev.position.x.0, ev.position.y.0);
                    view_state.update(|view| view.drag(x - last.x, y - last.y));
                    drag.set(Some(Drag::Panning(PlotPoint::new(x, y))));
                },
                on:pointer_up = move |ev| {
                    if matches!(drag.get_untracked(), Some(Drag::Panning(_))) {
                        ev.release_pointer();
                    }
                    drag.set(None);
                },
                on:pointer_cancel = move |ev| {
                    if matches!(drag.get_untracked(), Some(Drag::Panning(_))) {
                        ev.release_pointer();
                    }
                    drag.set(None);
                },
                on:double_click = move |_| {
                    if pannable.get_untracked() {
                        drag.set(None);
                        view_state.set(PlotView::new());
                    }
                },
                on:wheel = move |ev| {
                    if !pannable.get_untracked() {
                        return;
                    }
                    let Some(cursor) = self.local(ev.position.x.0, ev.position.y.0) else { return };
                    // A wheel turned away from the reader reports a negative delta.
                    let delta = match ev.delta {
                        ScrollDelta::Lines { y, .. } => y,
                        ScrollDelta::Pixels(size) => size.height.0,
                        _ => 0.0,
                    };
                    view_state.update(|view| view.wheel(-delta, cursor));
                    ev.prevent_default();
                }
            ) {
                {element.class("wt-chart__canvas")}
            }
        }
    }
}

/// Why a chart could not be put on the clipboard.
#[derive(Debug, Error)]
enum CopyImageError {
    #[error("no image of {width}x{height} can be drawn")]
    Size { width: u32, height: u32 },
    #[error("the clipboard refused the image: {0}")]
    Clipboard(#[from] arboard::Error),
}

/// Draws the chart again into an image, at the size it is on screen, and puts it on the
/// clipboard.
///
/// zgui's clipboard writes text only, so this goes through `arboard` as the GPUI front end does.
fn copy_image(area: NodeRef, handle: StoredValue<CustomHandle<PlotElement>, LocalStorage>, toasts: Option<Toasts>) {
    let Some(bounds) = area.window_bounds() else { return };
    let scale = area.scale();
    let width = (bounds.size.width.0 / scale).round();
    let height = (bounds.size.height.0 / scale).round();
    if !has_drawable_area(PlotRect::new(0.0, 0.0, width, height)) {
        return;
    }
    let (data, colors) = handle.with_value(|handle| handle.update(|element| (element.data.clone(), element.colors)));
    let Some(ThemeColors { plot, background }) = colors else { return };
    let (width, height) = (width as u32, height as u32);

    spawn_local(async move {
        let copied = blocking(move || -> Result<(), CopyImageError> {
            let drawn = image::render(&data.plot(), plot, background, width, height)
                .ok_or(CopyImageError::Size { width, height })?;
            let image = arboard::ImageData {
                width: drawn.width as usize,
                height: drawn.height as usize,
                bytes: drawn.rgba.into(),
            };
            arboard::Clipboard::new()?.set_image(image)?;
            Ok(())
        })
        .await;

        let Some(toasts) = toasts else { return };
        match copied {
            Ok(()) => {
                toasts.ok(t!("ui.stats.copy_image_done").into_owned());
            }
            Err(err) => {
                tracing::warn!("stats: the chart image could not be copied: {err}");
                toasts.failed(t!("ui.stats.copy_image_failed").into_owned());
            }
        }
    });
}

/// The statistic, its mode, the display options and the chart's own filters.
#[component]
fn SettingsMenu(
    settings: RwSignal<ChartSettings>,
    model: Memo<ChartModel>,
    filters: Signal<StatsFilters>,
    rated: Signal<bool>,
    locale: Locale,
) -> impl IntoView {
    let flag = move |read: fn(&ChartSettings) -> bool| Signal::derive_local(move || settings.with(read));
    let combined = flag(|settings| settings.combined);
    let running = flag(|settings| settings.running);
    let line = flag(|settings| settings.mode == ChartMode::Line);
    let own = flag(|settings| settings.own_filters.is_some());

    let stat_toggle = move |stat: ChartableStat| {
        let pressed = Binding::controlled(
            Signal::derive_local(move || model.with(|model| model.stat) == stat),
            move |_: bool| {
                if settings.with_untracked(|settings| settings.stat) != stat {
                    settings.update(|settings| model::set_stat(settings, stat));
                }
            },
        );
        view! {
            Toggle(pressed = pressed, size = ToggleSize::Sm, attr:data-testid = format!("chart-stat-{}", stat.translation_key())) {
                {tr(locale, move || stat.label().into())}
            }
        }
    };
    let mode_toggle = move |mode: ChartMode| {
        let pressed = Binding::controlled(
            Signal::derive_local(move || settings.with(|settings| settings.mode) == mode),
            move |_: bool| {
                if settings.with_untracked(|settings| settings.mode) != mode {
                    let rated = rated.get_untracked();
                    settings.update(|settings| model::set_mode(settings, mode, rated));
                }
            },
        );
        // One line over every ship has no bar form, so the mode is fixed while the ships are combined.
        view! {
            Toggle(pressed = pressed, size = ToggleSize::Sm, disabled = combined, attr:data-testid = format!("chart-mode-{}", mode.label())) {
                {mode.label()}
            }
        }
    };
    let division_toggle = move |division: DivisionFilter| {
        let chosen = move || model.with(|model| model.filters.division) == division;
        let pressed = Binding::controlled(Signal::derive_local(chosen), move |_: bool| {
            settings.update(|settings| model::set_own_division(settings, division));
        });
        view! {
            Toggle(pressed = pressed, size = ToggleSize::Sm, attr:data-testid = format!("chart-division-{}", division.label())) {
                {tr(locale, move || t!(division.translation_key()))}
            }
        }
    };
    let mode_filters = move || {
        let all = Binding::controlled(
            Signal::derive_local(move || model.with(|model| model.filters.game_modes.is_empty())),
            move |_: bool| settings.update(|settings| model::toggle_own_mode(settings, None)),
        );
        view! {
            row(class = "wt-chart__wrap", class:wt-chart__gone = move || model.with(|model| model.modes.len()) < 2) {
                Toggle(pressed = all, size = ToggleSize::Sm, attr:data-testid = "chart-mode-filter-all") {
                    {tr(locale, || t!("ui.stats.div_all"))}
                }
                for group in move || model.with(|model| model.modes.clone()), key = |group: &String| group.clone() {
                    {group_toggle(settings, model, group)}
                }
            }
        }
    };

    view! {
        Tooltip {
            TooltipTrigger {
                Popover {
                    PopoverTrigger(size = ButtonSize::IconSm, variant = ButtonVariant::Outline, a11y:label = t!("ui.stats.settings").into_owned(), attr:data-testid = "chart-settings") {
                        AppIcon(glyph = Glyph::Settings)
                    }
                    PopoverContent(class = "wt-chart__menu", attr:data-testid = "chart-settings-menu") {
                        text(class = "wt-chart__heading") {{tr(locale, || t!("ui.stats.statistic"))}}
                        row(class = "wt-chart__wrap") {
                            for stat in move || settings.with(|settings| selectable_stats(settings.mode, settings.running, settings.combined, rated.get())), key = |stat: &ChartableStat| stat.translation_key() {
                                {stat_toggle(stat)}
                            }
                        }
                        text(class = "wt-chart__heading") {{tr(locale, || t!("ui.stats.chart"))}}
                        row(class = "wt-chart__wrap") {
                            {mode_toggle(ChartMode::Line)}
                            {mode_toggle(ChartMode::Bar)}
                        }
                        text(class = "wt-chart__heading") {{tr(locale, || t!("ui.stats.options"))}}
                        CheckRow(
                            checked = combined,
                            label = Signal::derive_local(move || { locale.track(); t!("ui.stats.combine_ships").into_owned() }),
                            change = UnsyncCallback::new(move |on: bool| {
                                let rated = rated.get_untracked();
                                settings.update(|settings| model::set_combined(settings, on, rated));
                            }),
                            test_id = "chart-combined"
                        )
                        // Combining the ships is already a running line.
                        box(class = "wt-chart__slot", class:wt-chart__gone = move || !line.get()) {
                            CheckRow(
                                checked = Signal::derive_local(move || running.get() || combined.get()),
                                disabled = combined,
                                label = Signal::derive_local(move || { locale.track(); t!("ui.stats.running_average").into_owned() }),
                                change = UnsyncCallback::new(move |on: bool| {
                                    let rated = rated.get_untracked();
                                    settings.update(|settings| model::set_running(settings, on, rated));
                                }),
                                test_id = "chart-running"
                            )
                        }
                        CheckRow(
                            checked = flag(|settings| settings.show_values),
                            label = Signal::derive_local(move || { locale.track(); t!("ui.stats.value_labels").into_owned() }),
                            change = UnsyncCallback::new(move |on: bool| settings.update(|settings| settings.show_values = on)),
                            test_id = "chart-values"
                        )
                        // The chart's own narrowing, when it has been taken off the tab's bar. The tab's
                        // filters stay the default, so a reader who never opens this sees one set.
                        text(class = "wt-chart__heading") {{tr(locale, || t!("ui.stats.filters"))}}
                        Tooltip {
                            TooltipTrigger {
                                CheckRow(
                                    checked = own,
                                    label = Signal::derive_local(move || { locale.track(); t!("ui.stats.own_filters").into_owned() }),
                                    change = UnsyncCallback::new(move |on: bool| {
                                        let tab = filters.get_untracked();
                                        settings.update(|settings| model::set_own_filters(settings, on, &tab));
                                    }),
                                    test_id = "chart-own-filters"
                                )
                            }
                            TooltipContent {{t!("ui.stats.own_filters_tooltip").into_owned()}}
                        }
                        column(class = "wt-chart__slot", class:wt-chart__gone = move || !own.get()) {
                            row(class = "wt-chart__wrap") {
                                {division_toggle(DivisionFilter::All)}
                                {division_toggle(DivisionFilter::SoloOnly)}
                                {division_toggle(DivisionFilter::DivOnly)}
                            }
                            {mode_filters()}
                        }
                    }
                }
            }
            TooltipContent {{tr(locale, || t!("ui.stats.settings"))}}
        }
    }
}

fn group_toggle(settings: RwSignal<ChartSettings>, model: Memo<ChartModel>, group: String) -> impl IntoView {
    let name = match_group_display_name(&group).to_owned();
    let test_id = format!("chart-mode-filter-{group}");
    let read = group.clone();
    let pressed = Binding::controlled(
        Signal::derive_local(move || model.with(|model| model.filters.game_modes.contains(&read))),
        move |_: bool| settings.update(|settings| model::toggle_own_mode(settings, Some(&group))),
    );

    view! { Toggle(pressed = pressed, size = ToggleSize::Sm, attr:data-testid = test_id) {{name}} }
}

/// Which ships are plotted, with a search over the ships played.
#[component]
fn ShipMenu(settings: RwSignal<ChartSettings>, model: Memo<ChartModel>, locale: Locale) -> impl IntoView {
    let query = RwSignal::new_local(String::new());
    let counts = move || {
        model.with(|model| {
            let selected = model.played.iter().filter(|ship| model.selected.contains(&ship.ship_id)).count();
            (selected, model.played.len())
        })
    };
    let count_label = move || {
        locale.track();
        let (selected, total) = counts();
        t!("ui.stats.ship_selection_count", selected = selected, total = total).into_owned()
    };
    let filter = Memo::new(move |_| FilterQuery::new(&query.get()));
    let visible = Memo::new(move |_| {
        filter.with(|filter| {
            model.with(|model| {
                model::visible_ships(&model.played, filter)
                    .into_iter()
                    .map(|ship| (ship.ship_id, ship.name.clone()))
                    .collect::<Vec<(GameParamId, String)>>()
            })
        })
    });
    let none_visible = Signal::derive_local(move || visible.with(Vec::is_empty));
    let pick_visible = move |chosen: bool| {
        let ships: Vec<GameParamId> = visible.with_untracked(|visible| visible.iter().map(|(ship, _)| *ship).collect());
        let played = model.with_untracked(|model| model.played.clone());
        settings.update(|settings| model::set_ships_selected(settings, &played, &ships, chosen));
    };
    let ship_row = move |(ship, name): (GameParamId, String)| {
        let label = name.clone();
        view! {
            CheckRow(
                checked = Signal::derive_local(move || model.with(|model| model.selected.contains(&ship))),
                label = Signal::derive_local(move || label.clone()),
                marks = Signal::derive_local(move || filter.with(|filter| marks(filter, &name))),
                change = UnsyncCallback::new(move |_: bool| {
                    let played = model.with_untracked(|model| model.played.clone());
                    settings.update(|settings| model::toggle_ship(settings, &played, ship));
                }),
                test_id = format!("chart-ship-{}", ship.raw())
            )
        }
    };
    let escape = EscapeClears::new(query);

    view! {
        Tooltip {
            TooltipTrigger {
                Popover {
                    PopoverTrigger(
                        size = ButtonSize::Sm,
                        variant = ButtonVariant::Outline,
                        class = "wt-chart__ships",
                        attr:data-narrowed = move || { let (selected, total) = counts(); Some((selected < total).to_string()) },
                        a11y:label = move || format!("{}: {}", t!("ui.stats.filter_ships"), count_label()),
                        attr:data-testid = "chart-ship-filter"
                    ) {
                        AppIcon(glyph = Glyph::Funnel)
                        text {{tr(locale, || t!("ui.stats.ships"))}}
                        text(class = "wt-chart__dim") {{count_label}}
                    }
                    PopoverContent(class = "wt-chart__menu", dismiss_on_escape = escape.dismiss_on_escape(), attr:data-testid = "chart-ship-filter-menu") {
                        row(class = "wt-chart__heading-row") {
                            text(class = "wt-chart__heading") {{tr(locale, || t!("ui.stats.ships"))}}
                            text(class = "wt-chart__dim") {{count_label}}
                        }
                        row(class = "wt-chart__row") {
                            box(class = "wt-chart__search") {
                                Input(
                                    value = query,
                                    placeholder = t!("ui.stats.search_ships").into_owned(),
                                    node_ref = escape.node_ref(),
                                    attr:data-testid = "chart-ship-search",
                                    on:key_down = move |ev| {
                                        escape.key_down(ev);
                                    }
                                )
                            }
                            box(class:wt-chart__gone = move || query.with(String::is_empty)) {
                                Tooltip {
                                    TooltipTrigger {
                                        Button(size = ButtonSize::Sm, variant = ButtonVariant::Ghost, a11y:label = t!("ui.stats.clear").into_owned(), attr:data-testid = "chart-ship-search-clear", on:click = move |_| query.set(String::new())) {
                                            AppIcon(glyph = Glyph::X)
                                        }
                                    }
                                    TooltipContent {{tr(locale, || t!("ui.stats.clear"))}}
                                }
                            }
                        }
                        row(class = "wt-chart__row") {
                            Tooltip {
                                TooltipTrigger {
                                    Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline, disabled = none_visible, attr:data-testid = "chart-ships-all", on:click = move |_| pick_visible(true)) {
                                        {tr(locale, || t!("ui.stats.all_ships"))}
                                    }
                                }
                                TooltipContent {{tr(locale, || t!("ui.stats.all_ships_tooltip"))}}
                            }
                            Tooltip {
                                TooltipTrigger {
                                    Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline, disabled = none_visible, attr:data-testid = "chart-ships-none", on:click = move |_| pick_visible(false)) {
                                        {tr(locale, || t!("ui.stats.no_ships"))}
                                    }
                                }
                                TooltipContent {{tr(locale, || t!("ui.stats.no_ships_tooltip"))}}
                            }
                        }
                        text(
                            class = "wt-chart__dim",
                            class:wt-chart__gone = move || !(none_visible.get() && query.with(|query| !query.trim().is_empty())),
                            attr:data-testid = "chart-no-ship-matches"
                        ) {{tr(locale, || t!("ui.stats.no_ship_matches"))}}
                        for ship in move || visible.get(), key = |ship: &(GameParamId, String)| ship.0 {
                            {ship_row(ship)}
                        }
                    }
                }
            }
            TooltipContent {{tr(locale, || t!("ui.stats.filter_ships"))}}
        }
    }
}

/// A checkbox and the words naming it, marked where `marks` says, either of which toggles it.
#[component]
fn CheckRow(
    #[prop(into)] checked: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(into, default = Signal::stored_local(Marks::default()))] marks: Signal<Marks, LocalStorage>,
    change: UnsyncCallback<bool>,
    #[prop(into)] test_id: String,
) -> impl IntoView {
    let name = NodeRef::new();
    let control = NodeRef::new();
    let binding = Binding::controlled(
        Signal::derive_local(move || if checked.get() { Checked::Yes } else { Checked::No }),
        move |next: Checked| change.run(next == Checked::Yes),
    );

    view! {
        row(class = "wt-chart__row") {
            Checkbox(node_ref = control, checked = binding, disabled = disabled, labelled_by = name, attr:data-testid = test_id)
            Label(node_ref = name, control = control, on:click = move |_| {
                if !disabled.get_untracked() {
                    change.run(!checked.get_untracked());
                }
            }) {{move || view! { MarkedText(text = label.get(), ranges = marks.get()) }}}
        }
    }
}

/// The string `text` reads, read again when the language changes.
fn tr(locale: Locale, text: impl Fn() -> Cow<'static, str> + Copy) -> impl Fn() -> String + Copy {
    move || {
        locale.track();
        text().into_owned()
    }
}

fn value_label(settings: RwSignal<ChartSettings>, model: Memo<ChartModel>, locale: Locale) -> String {
    locale.track();
    let stat = model.with(|model| model.stat);
    settings.with(|settings| value_axis_label(stat, settings.mode, settings.running, settings.combined))
}

/// Runs `run` now and whenever what it reads changes, for as long as the calling scope lives.
fn effect<T: 'static>(run: impl FnMut(Option<T>) -> T + 'static) {
    let effect = RenderEffect::new(run);
    on_cleanup_local(move || drop(effect));
}
