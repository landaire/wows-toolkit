//! The session Stats tab, and the body a directory stats tab shares with it.
//!
//! A filter bar narrows the games every panel below it shows. Below it a dock holds the Overview
//! and Ships panels and any number of chart panels. The session's filters, charts and dock layout
//! are saved; a directory's are not.

mod art;
mod chart;
mod overview;
#[cfg(test)]
mod panel_tests;
mod rating;
mod ships;

use std::collections::BTreeSet;
use std::sync::Arc;

use landaire_ui::CssLength;
use landaire_ui::DockProps;
use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::DockTarget;
use landaire_ui::FoldingRegion;
use landaire_ui::NumberInputProps;
use landaire_ui::NumberPattern;
use landaire_ui::NumberRange;
use landaire_ui::PanelEntry;
use landaire_ui::PanelFactory;
use landaire_ui::PanelId;
use landaire_ui::PanelKind;
use landaire_ui::PanelRegistry;
use landaire_ui::PanelSide;
use landaire_ui::RegionExtent;
use landaire_ui::RegionLimits;
use landaire_ui::SavedLayout;
use landaire_ui::SavedNode;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_replays::types::GameParamId;
use wows_toolkit_config::queries;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::GameLimit;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::all_match_groups;
use wows_toolkit_viewmodel::stats::chart::ChartId;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartSettings;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::filter_games;
use wows_toolkit_viewmodel::stats::filter_games_per_ship;
use wows_toolkit_viewmodel::stats::match_group_display_name;
use wows_toolkit_viewmodel::stats::setting_keys;
use zgui::component;
use zgui::css;
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
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::reactive::use_local_context;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::AlertDialogActionProps;
use zgui_ui::AlertDialogCancelProps;
use zgui_ui::AlertDialogContentProps;
use zgui_ui::AlertDialogDescriptionProps;
use zgui_ui::AlertDialogFooterProps;
use zgui_ui::AlertDialogHeaderProps;
use zgui_ui::AlertDialogProps;
use zgui_ui::AlertDialogTitleProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuCheckboxItemProps;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuLabelProps;
use zgui_ui::SeparatorOrientation;
use zgui_ui::SeparatorProps;
use zgui_ui::ToggleProps;
use zgui_ui::ToggleSize;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::mark::PLUS;
use zgui_ui_primitives::Binding;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::tabs::PanelToggle;
use crate::shell::tabs::TabChrome;
use crate::shell::tabs::panel_side_name;
use crate::tabs::stats::chart::StatsChartProps;
use crate::tabs::stats::overview::StatsOverviewProps;
use crate::tabs::stats::ships::StatsShipsProps;

/// Games shown when the limit is switched on without a saved count. The egui drag value opens at
/// this and clamps to 1..=999.
const DEFAULT_GAME_LIMIT: usize = 25;
const MIN_GAME_LIMIT: usize = 1;
const MAX_GAME_LIMIT: usize = 999;

/// The settings row the session's charts are kept in, shared with the GPUI front end.
const CHARTS_SETTINGS_KEY: &str = "stats_charts";
/// The session's dock layout. Not the GPUI row `stats_charts_dock_layout`: that holds the GPUI
/// dock's own tree, which this dock cannot read and must not overwrite.
const CHARTS_DOCK_LAYOUT_KEY: &str = "stats_charts_dock_layout_zgui";

const OVERVIEW_KIND: &str = "StatsOverviewPanel";
const SHIPS_KIND: &str = "StatsShipsPanel";
const CHART_KIND: &str = "StatsChartPanel";
const OVERVIEW_ID: &str = "overview";
const SHIPS_ID: &str = "ships";
const CHART_ID_PREFIX: &str = "chart-";

const SHEET: &str = "wt-stats";

const CSS: &str = css!(
    ".wt-stats {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
    }
    .wt-stats__bar {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 4px 8px;
        background-color: var(--lui-color-surface);
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-stats__group {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
    }
    .wt-stats__rule {
        flex: none;
        height: 16px;
        margin: 0 4px;
    }
    .wt-stats__dim {
        font-size: var(--zui-type-size-xs);
        color: var(--wt-color-text-dim);
    }
    .wt-stats__error {
        font-size: var(--zui-type-size-xs);
        color: var(--wt-color-error);
    }
    .wt-stats__limit {
        width: 90px;
    }
    .wt-stats__spacer {
        flex: 1 1 auto;
    }
    .wt-stats__dock {
        flex: 1 1 auto;
        min-height: 0;
    }
    .wt-stats__chart-title {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
    }"
);

/// Which games a [`StatsBody`] shows, and whether its filters, charts and layout are saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsScope {
    /// The recorded session: saved, and its games can be cleared.
    Session,
    /// Every replay in one directory: read only, nothing saved.
    Directory,
}

/// The recorded session as the Stats tab holds it, for a [`StatsBody`] that clears its games and
/// saves its filters.
#[derive(Clone, Copy)]
pub struct SessionStats {
    pub games: RwSignal<Arc<[PerGameStat]>>,
    pub filters: RwSignal<StatsFilters>,
    /// Whether the reader changed a filter, which then wins over the saved ones a late load reads.
    pub filters_edited: StoredValue<bool>,
}

/// The Stats tab over the recorded session, read from the config database once it is open.
#[component]
pub fn StatsTab() -> impl IntoView {
    let services = Services::current();
    let session = SessionStats {
        games: RwSignal::new(Arc::from(Vec::new())),
        filters: RwSignal::new(StatsFilters::default()),
        filters_edited: StoredValue::new(false),
    };
    let personal_rating = RwSignal::new(None::<Arc<PersonalRatingData>>);
    let load_started = StoredValue::new(false);

    // The read waits for the startup settings, which is when the pool opens.
    effect(move |_| {
        let Some(proxy_url) = services.settings.read(|settings| settings.proxy_url.clone()) else { return };
        if load_started.get_value() {
            return;
        }
        let Some(pool) = services.settings.pool() else { return };
        load_started.set_value(true);
        spawn_local(async move {
            let local = services.on_tokio({
                let pool = pool.clone();
                async move { wows_toolkit_services::stats::load_games_and_filters(&pool).await }
            });
            match local.await {
                Ok((games, mut filters)) => {
                    // A mode saved for games no longer recorded would hide every game.
                    let modes = all_match_groups(&games);
                    let before = filters.game_modes.len();
                    filters.game_modes.retain(|mode| modes.contains(mode));
                    session.games.set(Arc::from(games));
                    if !session.filters_edited.get_value() {
                        if filters.game_modes.len() != before {
                            save_filters(services, &filters);
                        }
                        session.filters.set(filters);
                    }
                }
                Err(err) => tracing::warn!("stats: the session was not read: {err}"),
            }
            // The expected-values refresh may go to the network; the games show before it lands.
            let rating = services
                .on_tokio(async move {
                    wows_toolkit_services::personal_rating::load_refreshed(crate::http::USER_AGENT, &proxy_url).await
                })
                .await;
            match rating {
                Ok(table) => personal_rating.set(table.map(Arc::new)),
                Err(err) => tracing::warn!("stats: the personal rating table was not read: {err}"),
            }
        });
    });

    view! {
        StatsBody(
            scope = StatsScope::Session,
            games = session.games,
            personal_rating = personal_rating,
            session = session,
            attr:data-testid = "stats-tab"
        )
    }
}

/// Runs `run` now and whenever what it reads changes, for as long as the calling scope lives.
fn effect(run: impl FnMut(Option<()>) + 'static) {
    let effect = RenderEffect::new(run);
    on_cleanup_local(move || drop(effect));
}

/// One open chart: its id and the settings its panel reads and writes.
#[derive(Clone, Copy)]
struct ChartSlot {
    id: ChartId,
    settings: RwSignal<ChartSettings>,
}

/// Why the saved charts could not be read or written, and which, for the retry.
#[derive(Clone, Debug, PartialEq)]
struct ChartsError {
    reason: String,
    on_save: bool,
}

/// The filter bar over the dock of Overview, Ships and charts, for `games` oldest first.
///
/// `session`, when given, is where the filters live and what the Clear buttons empty.
#[component]
pub fn StatsBody(
    scope: StatsScope,
    #[prop(into)] games: Signal<Arc<[PerGameStat]>>,
    #[prop(into)] personal_rating: Signal<Option<Arc<PersonalRatingData>>>,
    #[prop(optional)] session: Option<SessionStats>,
    #[prop(attrs)] attrs: zgui::view::Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let shell = use_local_context::<Shell>();
    let persists = scope == StatsScope::Session;
    let filters = session.map_or_else(|| RwSignal::new(StatsFilters::default()), |session| session.filters);
    let last_count = RwSignal::new(DEFAULT_GAME_LIMIT);
    let clear_error = RwSignal::new(None::<String>);
    let charts_error = RwSignal::new(None::<ChartsError>);

    let available_modes = Memo::new(move |_| games.with(|games| all_match_groups(games)));
    let filtered: Signal<Arc<[PerGameStat]>> = Memo::new(move |_| {
        games.with(|games| filters.with(|filters| filter_games(games, filters).into_iter().cloned().collect()))
    })
    .into();
    let per_ship: Signal<Arc<[PerGameStat]>> = Memo::new(move |_| {
        games.with(|games| filters.with(|filters| filter_games_per_ship(games, filters).into_iter().cloned().collect()))
    })
    .into();

    // A count read back from the session is what the limit's box shows.
    effect(move |_| {
        if let GameLimit::Recent(count) = filters.get().limit {
            last_count.set(count.clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT));
        }
    });

    let change_filters = move |change: &dyn Fn(&mut StatsFilters)| {
        filters.update(|filters| {
            change(filters);
            available_modes.with_untracked(|modes| filters.game_modes.retain(|mode| modes.contains(mode)));
        });
        if let Some(session) = session {
            session.filters_edited.set_value(true);
        }
        if persists {
            save_filters(services, &filters.get_untracked());
        }
    };

    let charts = RwSignal::new_local(Vec::<ChartSlot>::new());
    let next_chart_id = StoredValue::new(0_usize);
    let dock = DockState::new(PanelRegistry::new());
    size_regions(dock);
    register_panels(
        dock,
        PanelSources { charts, games, filtered, per_ship, filters, personal_rating, session, clear_error, services },
    );

    let open_chart = move |settings: ChartSettings, placement: DockTarget| {
        let id = settings.id.unwrap_or_else(|| next_chart_id.get_value());
        next_chart_id.set_value(next_chart_id.get_value().max(id.saturating_add(1)));
        let slot = chart_slot(charts, id, settings);
        if let Err(err) = dock.add_panel(placement, chart_entry(slot.id)) {
            tracing::warn!("stats: chart {id} was not added: {err}");
        }
    };

    // Overview and Ships share the main tab group; the first chart opens beside them.
    if let Ok(group) = dock.add_panel(
        DockTarget::Region(DockRegion::Center),
        PanelEntry { closable: false, ..PanelEntry::new(PanelId::new(OVERVIEW_ID), PanelKind::new(OVERVIEW_KIND)) },
    ) {
        let ships =
            PanelEntry { closable: false, ..PanelEntry::new(PanelId::new(SHIPS_ID), PanelKind::new(SHIPS_KIND)) };
        if let Err(err) = dock.add_panel(DockTarget::Group(group), ships) {
            tracing::warn!("stats: the Ships panel was not added: {err}");
        }
    }
    open_chart(ChartSettings::default(), DockTarget::Region(DockRegion::Right));
    let _ = dock.activate(&PanelId::new(OVERVIEW_ID));

    let persistence = ChartPersistence {
        enabled: StoredValue::new(!persists),
        user_modified: StoredValue::new(false),
        restoring: StoredValue::new(false),
        saved_layout: StoredValue::new(None),
    };

    // A chart whose tab was closed is let go, and is not reopened by the next load.
    effect(move |_| {
        let open: BTreeSet<ChartId> = dock.panels().iter().filter_map(|entry| chart_id(&entry.id)).collect();
        let before = charts.with_untracked(Vec::len);
        charts.update(|charts| charts.retain(|slot| open.contains(&slot.id)));
        if charts.with_untracked(Vec::len) != before && !persistence.restoring.get_value() {
            persistence.user_modified.set_value(true);
            persistence.enabled.set_value(true);
        }
    });

    if persists {
        // Every chart's settings, written as one row whenever any of them changes.
        effect(move |previous| {
            let settings: Vec<ChartSettings> =
                charts.with(|charts| charts.iter().map(|slot| slot.settings.get()).collect());
            // The first run sees the default chart the tab made; later ones are the reader's.
            if previous.is_none() || persistence.restoring.get_value() {
                return;
            }
            persistence.user_modified.set_value(true);
            persistence.enabled.set_value(true);
            services.settings.save(CHARTS_SETTINGS_KEY, &settings);
        });
        effect(move |_| {
            let layout = dock.dump();
            if persistence.restoring.get_value() {
                return;
            }
            // The first run, and a restore, see the layout the tab itself made.
            if persistence.saved_layout.with_value(|saved| saved.as_ref() == Some(&layout)) {
                return;
            }
            let first = persistence.saved_layout.with_value(Option::is_none);
            persistence.saved_layout.set_value(Some(layout.clone()));
            if first {
                return;
            }
            persistence.user_modified.set_value(true);
            persistence.enabled.set_value(true);
            services.settings.save(CHARTS_DOCK_LAYOUT_KEY, &layout);
        });
        load_charts(services, dock, charts, next_chart_id, persistence, charts_error, open_chart);
    }

    publish_toggles(dock);

    let limit_checked = Binding::controlled(
        Signal::derive_local(move || {
            if matches!(filters.get().limit, GameLimit::Recent(_)) { Checked::Yes } else { Checked::No }
        }),
        move |checked: Checked| {
            let count = last_count.get_untracked();
            change_filters(&|filters| {
                filters.limit = if checked == Checked::Yes { GameLimit::Recent(count) } else { GameLimit::All };
            });
        },
    );
    let limit_value = Binding::controlled(Signal::derive_local(move || last_count.get() as f64), move |value: f64| {
        if !value.is_finite() || value < 0.0 {
            return;
        }
        let count = (value as usize).clamp(MIN_GAME_LIMIT, MAX_GAME_LIMIT);
        last_count.set(count);
        if matches!(filters.get_untracked().limit, GameLimit::Recent(_)) {
            change_filters(&|filters| filters.limit = GameLimit::Recent(count));
        }
    });
    let limit_disabled = Signal::derive_local(move || !matches!(filters.get().limit, GameLimit::Recent(_)));

    let division_toggle = move |division: DivisionFilter, test_id: &'static str| {
        let pressed =
            Binding::controlled(Signal::derive_local(move || filters.get().division == division), move |_: bool| {
                if filters.get_untracked().division != division {
                    change_filters(&|filters| filters.division = division);
                }
            });
        view! {
            Toggle(pressed = pressed, size = ToggleSize::Sm, attr:data-testid = test_id) {{division.label()}}
        }
    };

    let mode_menu = move || {
        (available_modes.with(BTreeSet::len) > 1).then(|| {
            view! {
                Separator(orientation = SeparatorOrientation::Vertical, class = "wt-stats__rule")
                ModeMenu(filters = filters, modes = available_modes.into(), change = UnsyncCallback::new(move |mode: Option<String>| {
                    change_filters(&|filters| match &mode {
                        None => filters.game_modes.clear(),
                        Some(mode) => {
                            if !filters.game_modes.remove(mode) {
                                filters.game_modes.insert(mode.clone());
                            }
                        }
                    });
                }))
            }
            .into_view()
        })
    };

    let add_chart = move |stat: ChartableStat| {
        persistence.user_modified.set_value(true);
        persistence.enabled.set_value(true);
        open_chart(ChartSettings { stat, ..ChartSettings::default() }, DockTarget::Region(DockRegion::Right));
    };

    let charts_error_view = move || {
        charts_error.get().map(|error| {
            let retry_save = error.on_save;
            view! {
                row(class = "wt-stats__group") {
                    text(class = "wt-stats__error") {{error.reason}}
                    Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline, attr:data-testid = "stats-retry-chart-load", on:click = move |_| {
                        charts_error.set(None);
                        if retry_save {
                            keep_current_layout(services, dock, charts, persistence);
                        } else {
                            load_charts(services, dock, charts, next_chart_id, persistence, charts_error, open_chart);
                        }
                    }) {{t!("ui.buttons.retry").into_owned()}}
                    Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline, attr:data-testid = "stats-keep-current-layout", on:click = move |_| {
                        charts_error.set(None);
                        keep_current_layout(services, dock, charts, persistence);
                    }) {{t!("ui.stats.keep_current_layout").into_owned()}}
                }
            }
            .into_view()
        })
    };

    let clear_button = move || {
        session.map(|session| {
            let empty = Signal::derive_local(move || session.games.with(|games| games.is_empty()));
            view! {
                Tooltip {
                    TooltipTrigger {
                        Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline, disabled = empty, attr:data-testid = "stats-clear", on:click = move |_| {
                            if let Some(shell) = shell {
                                confirm_clear_session(services, shell, session, clear_error);
                            }
                        }) {
                            AppIcon(glyph = Glyph::Eraser)
                            text {{t!("ui.stats.clear").into_owned()}}
                        }
                    }
                    TooltipContent {{t!("ui.stats.clear_tooltip").into_owned()}}
                }
            }
            .into_view()
        })
    };

    let locale = services.locale;
    view! {
        box(class = "wt-stats", {..attrs}) {
            row(class = "wt-stats__bar", attr:data-testid = "stats-filter-bar") {
                row(class = "wt-stats__group") {
                    Tooltip {
                        TooltipTrigger {
                            row(class = "wt-stats__group") {
                                Checkbox(checked = limit_checked, attr:data-testid = "stats-limit-enabled")
                                text {{move || { locale.track(); t!("ui.stats.limit_to_recent").into_owned() }}}
                            }
                        }
                        TooltipContent {{t!("ui.stats.limit_to_recent_tooltip").into_owned()}}
                    }
                    NumberInput(
                        value = limit_value,
                        range = NumberRange::new(1.0).with_min(MIN_GAME_LIMIT as f64).with_max(MAX_GAME_LIMIT as f64),
                        pattern = NumberPattern::Digits,
                        disabled = limit_disabled,
                        class = "wt-stats__limit",
                        attr:data-testid = "stats-limit-count"
                    )
                }
                Separator(orientation = SeparatorOrientation::Vertical, class = "wt-stats__rule")
                row(class = "wt-stats__group") {
                    text(class = "wt-stats__dim") {{move || { locale.track(); t!("ui.stats.division_label").into_owned() }}}
                    {division_toggle(DivisionFilter::All, "stats-division-all")}
                    {division_toggle(DivisionFilter::SoloOnly, "stats-division-solo")}
                    {division_toggle(DivisionFilter::DivOnly, "stats-division-div")}
                }
                {mode_menu}
                Separator(orientation = SeparatorOrientation::Vertical, class = "wt-stats__rule")
                AddChartMenu(add = UnsyncCallback::new(add_chart))
                box(class = "wt-stats__spacer") {}
                {charts_error_view}
                {move || clear_error.get().map(|reason| view! { text(class = "wt-stats__error", attr:data-testid = "stats-clear-error") {{reason}} }.into_view())}
                {clear_button}
            }
            Dock(
                state = dock,
                class = "wt-stats__dock",
                attr:data-testid = match scope {
                    StatsScope::Session => "stats-dock",
                    StatsScope::Directory => "directory-stats-dock",
                }
            )
        }
    }
}

/// What every panel the dock builds reads from.
#[derive(Clone, Copy)]
struct PanelSources {
    charts: RwSignal<Vec<ChartSlot>, LocalStorage>,
    games: Signal<Arc<[PerGameStat]>>,
    filtered: Signal<Arc<[PerGameStat]>>,
    per_ship: Signal<Arc<[PerGameStat]>>,
    filters: RwSignal<StatsFilters>,
    personal_rating: Signal<Option<Arc<PersonalRatingData>>>,
    session: Option<SessionStats>,
    clear_error: RwSignal<Option<String>>,
    services: Services,
}

fn register_panels(dock: DockState, sources: PanelSources) {
    let PanelSources { charts, games, filtered, per_ship, filters, personal_rating, session, clear_error, services } =
        sources;
    dock.register(
        PanelKind::new(OVERVIEW_KIND),
        PanelFactory::new(
            |_| t!("ui.stats.overview").into_owned(),
            move |_| AnyView::new(view! { StatsOverview(games = filtered, personal_rating = personal_rating) }),
        ),
    );
    dock.register(
        PanelKind::new(SHIPS_KIND),
        PanelFactory::new(
            |_| t!("ui.stats.ships").into_owned(),
            move |_| {
                let on_clear_ship = UnsyncCallback::new(move |ship: GameParamId| {
                    if let Some(session) = session {
                        clear_ship(services, session, clear_error, ship);
                    }
                });
                AnyView::new(view! {
                    StatsShips(
                        games = per_ship,
                        personal_rating = personal_rating,
                        clearable = session.is_some(),
                        on_clear_ship = on_clear_ship
                    )
                })
            },
        ),
    );
    let slot_of = move |id: &PanelId| {
        let id = chart_id(id)?;
        Some(chart_slot(charts, id, ChartSettings { id: Some(id), ..ChartSettings::default() }))
    };
    dock.register(
        PanelKind::new(CHART_KIND),
        PanelFactory::new(
            move |id| slot_of(id).map(|slot| chart_title(&slot.settings.get())).unwrap_or_default(),
            move |id| match slot_of(id) {
                Some(slot) => AnyView::new(view! {
                    StatsChart(settings = slot.settings, games = games, filters = filters, personal_rating = personal_rating)
                }),
                None => AnyView::new(()),
            },
        )
        .with_title(move |id| {
            let Some(slot) = slot_of(id) else { return AnyView::new(()) };
            let glyph = Signal::derive_local(move || match slot.settings.with(|settings| settings.mode) {
                ChartMode::Line => Glyph::ChartLine,
                ChartMode::Bar => Glyph::ChartColumn,
            });
            AnyView::new(view! {
                row(class = "wt-stats__chart-title") {
                    AppIcon(glyph = glyph)
                    text {{move || chart_title(&slot.settings.get())}}
                }
            })
        }),
    );
}

/// The slot for chart `id`, made with `settings` when there is none yet.
fn chart_slot(charts: RwSignal<Vec<ChartSlot>, LocalStorage>, id: ChartId, settings: ChartSettings) -> ChartSlot {
    if let Some(slot) = charts.with_untracked(|charts| charts.iter().find(|slot| slot.id == id).copied()) {
        return slot;
    }
    let slot = ChartSlot { id, settings: RwSignal::new(ChartSettings { id: Some(id), ..settings }) };
    charts.update(|charts| charts.push(slot));
    slot
}

/// A 500px right region for charts, with floors that keep a chart readable.
fn size_regions(dock: DockState) {
    let px = |pixels| CssLength::new(pixels).expect("a finite, non-negative length");
    dock.set_region_extent(FoldingRegion::Right, RegionExtent::Length(px(500.0)));
    dock.set_region_limits(FoldingRegion::Right, RegionLimits { min: Some(px(280.0)), max: None });
    dock.set_region_limits(FoldingRegion::Bottom, RegionLimits { min: Some(px(200.0)), max: None });
}

fn chart_entry(id: ChartId) -> PanelEntry {
    PanelEntry::new(PanelId::new(format!("{CHART_ID_PREFIX}{id}")), PanelKind::new(CHART_KIND))
}

fn chart_id(panel: &PanelId) -> Option<ChartId> {
    panel.as_str().strip_prefix(CHART_ID_PREFIX)?.parse().ok()
}

/// A chart's tab title: its statistic, and how it is plotted when that is not the plain line.
fn chart_title(settings: &ChartSettings) -> String {
    let stat = settings.stat.label();
    match settings.mode {
        ChartMode::Bar if settings.stat != ChartableStat::WinRate => t!("stat.avg_prefix", name = stat).into_owned(),
        ChartMode::Bar => stat,
        ChartMode::Line if settings.combined => format!("{stat} {}", t!("chart.combined_suffix")),
        ChartMode::Line if settings.running => format!("{stat} {}", t!("chart.rolling_average_suffix")),
        ChartMode::Line => stat,
    }
}

/// Puts one status bar toggle up per side region that holds a panel.
fn publish_toggles(dock: DockState) {
    let Some(chrome) = TabChrome::current() else { return };
    let sides = [
        (DockRegion::Left, PanelSide::Left, "stats-dock-toggle-left"),
        (DockRegion::Right, PanelSide::Right, "stats-dock-toggle-right"),
        (DockRegion::Bottom, PanelSide::Bottom, "stats-dock-toggle-bottom"),
    ]
    .map(|(region, side, test_id)| {
        let open = RwSignal::new_local(dock.is_region_open(region));
        // What the dock last showed, so a dock change such as a restore is not written back.
        let shown = StoredValue::new(dock.is_region_open(region));
        // The toggle writes `open`; the dock follows it, and `open` follows the dock.
        effect(move |_| {
            let want = open.get();
            if shown.get_value() != want {
                shown.set_value(want);
                dock.set_region_open(region, want);
            }
        });
        effect(move |_| {
            let now = dock.is_region_open(region);
            shown.set_value(now);
            if open.get_untracked() != now {
                open.set(now);
            }
        });
        (region, side, test_id, open)
    });
    effect(move |_| {
        let toggles = sides
            .iter()
            .filter(|(region, ..)| !dock.is_region_empty(*region))
            .map(|&(_, side, test_id, open)| PanelToggle { test_id, name: panel_side_name(side), side, open })
            .collect();
        chrome.set_toggles(toggles);
    });
}

/// Where the saved charts are in being read and written.
#[derive(Clone, Copy)]
struct ChartPersistence {
    /// Whether changes are written. Off until the saved charts are read, so the default chart
    /// does not overwrite them.
    enabled: StoredValue<bool>,
    /// Whether the reader changed the charts before the saved ones were read, which then win.
    user_modified: StoredValue<bool>,
    /// Set while a load puts the saved charts back, which is not a change to write.
    restoring: StoredValue<bool>,
    /// The layout last saved or restored, so a layout the tab made itself is not written.
    saved_layout: StoredValue<Option<SavedLayout>>,
}

/// Reopens the charts and dock arrangement the session was left with.
fn load_charts(
    services: Services,
    dock: DockState,
    charts: RwSignal<Vec<ChartSlot>, LocalStorage>,
    next_chart_id: StoredValue<usize>,
    persistence: ChartPersistence,
    charts_error: RwSignal<Option<ChartsError>>,
    open_chart: impl Fn(ChartSettings, DockTarget) + Copy + 'static,
) {
    // The pool opens with the startup settings, which may not have landed yet.
    let started = StoredValue::new(false);
    effect(move |_| {
        if services.settings.read(|_| ()).is_none() || started.get_value() {
            return;
        }
        let Some(pool) = services.settings.pool() else { return };
        started.set_value(true);
        spawn_local(async move {
            let stored = services
                .on_tokio(async move {
                    let charts = queries::try_get_setting::<Vec<ChartSettings>>(&pool, CHARTS_SETTINGS_KEY).await;
                    let layout = queries::try_get_setting::<SavedLayout>(&pool, CHARTS_DOCK_LAYOUT_KEY).await;
                    (charts, layout)
                })
                .await;
            let (saved, layout) = match stored {
                Ok((Ok(saved), Ok(layout))) => (saved, layout),
                Ok((Err(err), _)) => {
                    return fail_load(charts_error, format!("Could not read saved chart settings: {err}"));
                }
                Ok((_, Err(err))) => {
                    return fail_load(charts_error, format!("Could not read saved chart layout: {err}"));
                }
                Err(err) => return fail_load(charts_error, err.to_string()),
            };
            if persistence.user_modified.get_value() {
                persistence.enabled.set_value(true);
                return;
            }
            persistence.restoring.set_value(true);
            restore_charts(dock, charts, next_chart_id, saved, layout, open_chart);
            persistence.saved_layout.set_value(Some(dock.dump_untracked()));
            persistence.restoring.set_value(false);
            persistence.enabled.set_value(true);
        });
    });
}

fn fail_load(charts_error: RwSignal<Option<ChartsError>>, reason: String) {
    tracing::warn!("stats: {reason}");
    charts_error.set(Some(ChartsError { reason, on_save: false }));
}

/// Puts `saved` charts in `layout` back. Without a layout the first saved chart replaces the
/// default one and the rest open beside it.
fn restore_charts(
    dock: DockState,
    charts: RwSignal<Vec<ChartSlot>, LocalStorage>,
    next_chart_id: StoredValue<usize>,
    saved: Option<Vec<ChartSettings>>,
    layout: Option<SavedLayout>,
    open_chart: impl Fn(ChartSettings, DockTarget),
) {
    let saved = saved.map(|saved| {
        saved
            .into_iter()
            .enumerate()
            .map(|(index, settings)| ChartSettings { id: Some(settings.id.unwrap_or(index)), ..settings })
            .collect::<Vec<_>>()
    });
    match layout {
        Some(layout) => {
            let laid_out = layout_chart_ids(&layout);
            // The slots the restored panels find, made before the dock builds them.
            charts.set(Vec::new());
            for &id in &laid_out {
                let settings = saved
                    .as_ref()
                    .and_then(|saved| saved.iter().find(|settings| settings.id == Some(id)).cloned())
                    .unwrap_or_default();
                chart_slot(charts, id, settings);
            }
            next_chart_id.set_value(laid_out.iter().max().map_or(0, |id| id.saturating_add(1)));
            if let Err(err) = dock.restore(&layout) {
                tracing::warn!("stats: the saved chart layout was not restored: {err}");
            }
            if let Some(saved) = &saved {
                // A chart closed in the other front end has no row; its panel goes too.
                for &id in laid_out.iter().filter(|&&id| !saved.iter().any(|settings| settings.id == Some(id))) {
                    let _ = dock.remove(&chart_entry(id).id);
                }
            }
            for settings in saved.into_iter().flatten() {
                if !settings.id.is_some_and(|id| laid_out.contains(&id)) {
                    open_chart(settings, DockTarget::Region(DockRegion::Right));
                }
            }
        }
        None => {
            let Some(saved) = saved else { return };
            let mut saved = saved.into_iter();
            let default = charts.with_untracked(|charts| charts.first().copied());
            match (default, saved.next()) {
                (Some(slot), Some(first)) => slot.settings.set(ChartSettings { id: Some(slot.id), ..first }),
                (Some(slot), None) => {
                    // An empty saved list is a session whose charts were all closed.
                    let _ = dock.remove(&chart_entry(slot.id).id);
                }
                (None, first) => {
                    if let Some(first) = first {
                        open_chart(first, DockTarget::Region(DockRegion::Right));
                    }
                }
            }
            for settings in saved {
                open_chart(ChartSettings { id: None, ..settings }, DockTarget::Region(DockRegion::Right));
            }
        }
    }
}

/// The chart ids `layout` holds, in tab order.
fn layout_chart_ids(layout: &SavedLayout) -> Vec<ChartId> {
    let mut ids = Vec::new();
    let mut pending: Vec<&SavedNode> = [&layout.center, &layout.left, &layout.right, &layout.bottom]
        .iter()
        .filter_map(|region| region.root.as_ref())
        .collect();
    while let Some(node) = pending.pop() {
        match node {
            SavedNode::Tabs { panels, .. } => ids.extend(
                panels.iter().filter(|entry| entry.kind.as_str() == CHART_KIND).filter_map(|entry| chart_id(&entry.id)),
            ),
            SavedNode::Split { parts, .. } => pending.extend(parts.iter().map(|part| &part.node)),
        }
    }
    ids
}

/// Writes the charts and layout on screen over the saved ones, which could not be read or written.
fn keep_current_layout(
    services: Services,
    dock: DockState,
    charts: RwSignal<Vec<ChartSlot>, LocalStorage>,
    persistence: ChartPersistence,
) {
    persistence.user_modified.set_value(true);
    persistence.enabled.set_value(true);
    let settings: Vec<ChartSettings> =
        charts.with_untracked(|charts| charts.iter().map(|slot| slot.settings.get_untracked()).collect());
    let layout = dock.dump_untracked();
    services.settings.save(CHARTS_SETTINGS_KEY, &settings);
    services.settings.save(CHARTS_DOCK_LAYOUT_KEY, &layout);
    persistence.saved_layout.set_value(Some(layout));
}

/// Writes the filter bar's state to the rows `SessionData::load` reads.
fn save_filters(services: Services, filters: &StatsFilters) {
    let (limit_enabled, count) = match filters.limit {
        GameLimit::All => (false, None),
        GameLimit::Recent(count) => (true, Some(count)),
    };
    services.settings.save(setting_keys::LIMIT_ENABLED, &limit_enabled);
    if let Some(count) = count {
        services.settings.save(setting_keys::GAME_COUNT, &count);
    }
    services.settings.save(setting_keys::DIVISION_FILTER, &filters.division);
    services.settings.save(setting_keys::GAME_MODE_FILTER, &filters.game_modes);
}

/// Asks before forgetting every recorded game.
fn confirm_clear_session(
    services: Services,
    shell: Shell,
    session: SessionStats,
    clear_error: RwSignal<Option<String>>,
) {
    if session.games.with_untracked(|games| games.is_empty()) {
        return;
    }
    shell.dialogs.open(move |handle| {
        let open = RwSignal::new_local(true);
        AnyView::new(view! {
            AlertDialog(open = open, on_open_change = handle.on_open_change()) {
                AlertDialogContent(attr:data-testid = "stats-clear-dialog") {
                    AlertDialogHeader {
                        AlertDialogTitle {{t!("ui.stats.clear").into_owned()}}
                        AlertDialogDescription {{t!("confirm.clear_all_session_stats").into_owned()}}
                    }
                    AlertDialogFooter {
                        AlertDialogCancel {{t!("ui.buttons.cancel").into_owned()}}
                        AlertDialogAction(attr:data-testid = "stats-clear-confirm", on:click = move |_| clear_session(services, session, clear_error)) {
                            {t!("ui.stats.clear").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

/// Forgets every recorded game once the delete has gone through, so a failure does not leave an
/// empty session that comes back at the next start.
fn clear_session(services: Services, session: SessionStats, clear_error: RwSignal<Option<String>>) {
    let Some(pool) = services.settings.pool() else { return };
    spawn_local(async move {
        let cleared = services.on_tokio(async move { queries::clear_session_stats(&pool).await }).await;
        match cleared {
            Ok(Ok(_)) => {
                clear_error.set(None);
                session.games.set(Arc::from(Vec::new()));
            }
            Ok(Err(err)) => report_clear_failure(clear_error, &err.to_string()),
            Err(err) => report_clear_failure(clear_error, &err.to_string()),
        }
    });
}

/// Forgets one ship's games, which the Ships panel asks for.
fn clear_ship(services: Services, session: SessionStats, clear_error: RwSignal<Option<String>>, ship: GameParamId) {
    let Some(pool) = services.settings.pool() else { return };
    spawn_local(async move {
        let cleared = services
            .on_tokio(async move { queries::clear_session_stats_for_ship(&pool, ship.raw() as i64).await })
            .await;
        match cleared {
            Ok(Ok(_)) => {
                clear_error.set(None);
                session.games.update(|games| {
                    *games = games.iter().filter(|game| game.ship_id != ship).cloned().collect();
                });
            }
            Ok(Err(err)) => report_clear_failure(clear_error, &err.to_string()),
            Err(err) => report_clear_failure(clear_error, &err.to_string()),
        }
    });
}

fn report_clear_failure(clear_error: RwSignal<Option<String>>, reason: &str) {
    tracing::warn!("stats: the session was not cleared: {reason}");
    clear_error.set(Some(t!("ui.stats.clear_failed", reason = reason).into_owned()));
}

/// The game mode filter: every mode, or the ones ticked. `change` gets `None` for every mode and
/// a mode's name to tick or untick it.
#[component]
fn ModeMenu(
    filters: RwSignal<StatsFilters>,
    modes: Signal<BTreeSet<String>>,
    change: UnsyncCallback<Option<String>>,
) -> impl IntoView {
    let selected = move || filters.with(|filters| filters.game_modes.len());
    let label = move || match selected() {
        0 => t!("ui.stats.mode_label").into_owned(),
        count => format!("{} ({count})", t!("ui.stats.mode_label")),
    };
    let variant = if selected() > 0 { ButtonVariant::Secondary } else { ButtonVariant::Outline };
    let all = Binding::controlled(Signal::derive_local(move || selected() == 0), move |_: bool| change.run(None));

    view! {
        DropdownMenu {
            DropdownMenuTrigger(size = ButtonSize::Sm, variant = variant, attr:data-testid = "stats-mode-filter-trigger") {{label}}
            DropdownMenuContent(attr:data-testid = "stats-mode-filter") {
                MenuLabel {{t!("ui.stats.mode_label").into_owned()}}
                MenuCheckboxItem(checked = all, close_on_select = false, attr:data-testid = "stats-mode-all") {{t!("ui.stats.div_all").into_owned()}}
                for mode in move || modes.get().into_iter().collect::<Vec<_>>(), key = |mode: &String| mode.clone() {
                    {mode_item(filters, change, mode)}
                }
            }
        }
    }
}

fn mode_item(filters: RwSignal<StatsFilters>, change: UnsyncCallback<Option<String>>, mode: String) -> impl IntoView {
    let name = match_group_display_name(&mode).to_owned();
    let test_id = format!("stats-mode-{mode}");
    let read = mode.clone();
    let checked = Binding::controlled(
        Signal::derive_local(move || filters.with(|filters| filters.game_modes.contains(&read))),
        move |_: bool| change.run(Some(mode.clone())),
    );
    view! {
        MenuCheckboxItem(checked = checked, close_on_select = false, attr:data-testid = test_id) {{name}}
    }
}

/// The Add-chart control: a menu of statistics, so the chart opens on the one asked for.
#[component]
fn AddChartMenu(add: UnsyncCallback<ChartableStat>) -> impl IntoView {
    view! {
        DropdownMenu {
            DropdownMenuTrigger(size = ButtonSize::Sm, variant = ButtonVariant::Outline, attr:data-testid = "stats-add-chart") {
                Icon(icon = PLUS)
                text {{t!("ui.stats.add_chart").into_owned()}}
            }
            DropdownMenuContent(attr:data-testid = "stats-add-chart-menu") {
                for stat in move || ChartableStat::ALL.to_vec(), key = |stat: &ChartableStat| *stat as usize {
                    MenuItem(on_select = UnsyncCallback::new(move |()| add.run(stat)), attr:data-testid = format!("stats-add-chart-stat-{}", stat as usize)) {
                        {t!(stat.translation_key()).into_owned()}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
