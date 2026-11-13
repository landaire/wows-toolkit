//! The Ships panel: one row per ship played, with its detail table.

mod detail;

use std::cmp::Ordering;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::ColumnAlign;
use landaire_ui::ColumnSizing;
use landaire_ui::ColumnSort;
use landaire_ui::ColumnWidth;
use landaire_ui::EscapeClears;
use landaire_ui::GridColumn;
use landaire_ui::GridProps;
use landaire_ui::MarkedTextProps;
use landaire_ui::MenuItems;
use landaire_ui::RowMenu;
use landaire_ui::RowMode;
use landaire_ui::SortDirection;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::Toasts;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_replays::types::GameParamId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::table::Column;
use wows_toolkit_viewmodel::stats::table::ShipSummary;
use wows_toolkit_viewmodel::stats::table::StatLabel;
use wows_toolkit_viewmodel::stats::table::ship_summaries;
use wows_toolkit_viewmodel::text_filter::FilterKey;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wowsunpack::game_params::types::GameParamProvider;
use zgui::component;
use zgui::css;
use zgui::prelude::ClipboardKind;
use zgui::prelude::Clipboards;
use zgui::prelude::try_use_clipboard;
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
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::PropValue;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::InputProps;
use zgui_ui::MenuItemProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::chevron::CHEVRON_RIGHT;

use crate::game_art::Art;
use crate::game_art::ArtKey;
use crate::game_art::GameArt;
use crate::icons::AppIconProps;
use crate::services::Services;
use crate::tabs::stats::overview::loaded_build;
use crate::tabs::stats::overview::optional_decimal;
use crate::tabs::stats::overview::optional_number;
use crate::tabs::stats::overview::optional_percent;
use crate::tabs::stats::rating;
use crate::tabs::stats::ships::detail::ShipDetailProps;
use crate::text_marks::marks;

const SHEET: &str = "wt-stats-ships";

const CSS: &str = css!(
    ".wt-stats-ships {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
    }
    .wt-stats-ships__search {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 8px;
    }
    .wt-stats-ships__search .zui-input { flex: 1 1 auto; }
    .wt-stats-ships__count {
        font-size: 0.85em;
        color: var(--wt-color-text-dim);
    }
    .wt-stats-ships .lui-grid { flex: 1 1 auto; }
    .wt-stats-ships__empty {
        display: flex;
        flex: 1 1 auto;
        align-items: center;
        justify-content: center;
        color: var(--wt-color-text-dim);
    }
    .wt-stats-ship {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        min-width: 0;
        padding: 2px 0;
    }
    .wt-stats-ship__flag {
        flex: none;
        width: 23px;
        height: 16px;
    }
    .wt-stats-ship__class {
        flex: none;
        width: 16px;
        height: 16px;
    }
    .wt-stats-ship__text {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        gap: 2px;
        min-width: 0;
    }
    .wt-stats-ship__name {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-stats-ship__record {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        gap: 4px;
        font-size: 0.85em;
    }
    .wt-stats-ship__win { color: var(--wt-color-win); }
    .wt-stats-ship__loss { color: var(--wt-color-loss); }
    .wt-stats-chevron {
        display: flex;
        flex: none;
        transform: rotate(0deg);
        transition: transform var(--lui-motion-turn) var(--lui-motion-ease);
    }
    .wt-stats-chevron[data-open] { transform: rotate(90deg); }
    .wt-stats-ship__actions {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 2px;
    }
    .wt-stats-ship__clear[data-armed] {
        background-color: var(--lui-color-selection);
        color: var(--wt-color-warn);
    }"
);

/// One row: a ship's summary, shared between the rows signal, the cells and the detail.
type ShipRow = Arc<ShipSummary>;

/// What a column sorts by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortKey {
    Ship,
    Battles,
    WinRate,
    AvgDamage,
    AvgFrags,
}

impl SortKey {
    const ALL: [Self; 5] = [Self::Ship, Self::Battles, Self::WinRate, Self::AvgDamage, Self::AvgFrags];

    const fn id(self) -> &'static str {
        match self {
            Self::Ship => "ship",
            Self::Battles => "battles",
            Self::WinRate => "win_rate",
            Self::AvgDamage => "avg_damage",
            Self::AvgFrags => "avg_frags",
        }
    }

    fn compare(self, a: &ShipSummary, b: &ShipSummary) -> Ordering {
        match self {
            Self::Ship => a.ship.cmp(&b.ship),
            Self::Battles => a.performance.total_games().cmp(&b.performance.total_games()),
            Self::WinRate => partial(a.performance.win_rate(), b.performance.win_rate()),
            Self::AvgDamage => partial(a.performance.avg_damage(), b.performance.avg_damage()),
            Self::AvgFrags => partial(a.performance.avg_frags(), b.performance.avg_frags()),
        }
    }
}

/// `None` sorts before every figure.
fn partial(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.total_cmp(&b),
        (a, b) => a.is_some().cmp(&b.is_some()),
    }
}

/// `summaries` in `sort`'s order, or in the order given, which is newest played first.
fn sorted(mut rows: Vec<ShipRow>, sort: Option<&ColumnSort>) -> Vec<ShipRow> {
    let Some(sort) = sort else {
        return rows;
    };
    let Some(key) = SortKey::ALL.into_iter().find(|key| key.id() == sort.column.as_str()) else {
        return rows;
    };

    rows.sort_by(|a, b| {
        let order = key.compare(a, b);
        match sort.direction {
            SortDirection::Ascending => order,
            SortDirection::Descending => order.reverse(),
        }
    });

    rows
}

/// The ships whose name matches `filter`, in the order given: the search only narrows the list.
/// `keys` holds each ship's name folded, in the same order.
fn matching(summaries: &[ShipRow], keys: &[FilterKey], filter: &FilterQuery) -> Vec<ShipRow> {
    summaries
        .iter()
        .zip(keys)
        .filter(|(_, key)| filter.score(key).is_some())
        .map(|(summary, _)| Arc::clone(summary))
        .collect()
}

/// A column heading in the reader's own language.
fn column_heading(column: Column) -> String {
    t!(column.translation_key()).into_owned()
}

/// A statistic's name in the reader's own language.
fn stat_heading(label: StatLabel) -> String {
    t!(label.translation_key()).into_owned()
}

/// What the panel's controls act on, shared by every row.
#[derive(Clone, Copy)]
struct ShipsCx {
    expanded: RwSignal<HashSet<GameParamId>, LocalStorage>,
    /// The ship whose clear button has been pressed once and waits to be confirmed.
    armed: RwSignal<Option<GameParamId>, LocalStorage>,
    on_clear_ship: StoredValue<UnsyncCallback<GameParamId>, LocalStorage>,
    toasts: Option<Toasts>,
    /// Read where the panel is built: an event handler has no context to find it in.
    clipboard: StoredValue<Option<Clipboards>, LocalStorage>,
    art: GameArt,
    services: Services,
    /// The ship search, which marks each name where it matched.
    filter: Memo<FilterQuery>,
}

impl ShipsCx {
    fn toggle(self, ship: GameParamId) {
        self.expanded.update(|expanded| {
            if !expanded.remove(&ship) {
                expanded.insert(ship);
            }
        });
    }

    /// Asks the tab to forget `ship`'s games once the button has been pressed twice, or once with
    /// the primary modifier held.
    fn clear(self, ship: GameParamId, skip_confirmation: bool) {
        if !skip_confirmation && self.armed.get_untracked() != Some(ship) {
            self.armed.set(Some(ship));
            return;
        }

        self.armed.set(None);
        self.on_clear_ship.with_value(|on_clear| on_clear.run(ship));
    }

    fn copy(self, text: String) {
        self.clipboard.with_value(|clipboard| {
            if let Some(clipboard) = clipboard {
                clipboard.set_text(ClipboardKind::Standard, text);
            }
        });
        if let Some(toasts) = self.toasts {
            toasts.ok(t!("ui.stats.copied").into_owned());
        }
    }
}

/// `games` are the games the filters leave per ship, oldest first. `on_clear_ship` is offered
/// when `clearable`.
#[component]
pub fn StatsShips(
    #[prop(into)] games: Signal<Arc<[PerGameStat]>>,
    #[prop(into)] personal_rating: Signal<Option<Arc<PersonalRatingData>>>,
    clearable: bool,
    on_clear_ship: UnsyncCallback<GameParamId>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let locale = services.locale;
    let summaries = Memo::new(move |_| {
        locale.track();
        let games = games.get();
        let games: Vec<&PerGameStat> = games.iter().collect();
        // Grouped the way the reader's language groups digits.
        let figures = wows_toolkit_viewmodel::locale();
        ship_summaries(&games, personal_rating.get().as_deref(), Some(&figures))
            .into_iter()
            .map(Arc::new)
            .collect::<Vec<_>>()
    });
    let keys = Memo::new(move |_| {
        summaries.with(|summaries| summaries.iter().map(|summary| FilterKey::new(&summary.ship)).collect::<Vec<_>>())
    });
    let query = RwSignal::new_local(String::new());
    let filter = Memo::new(move |_| FilterQuery::new(&query.get()));

    let cx = ShipsCx {
        expanded: RwSignal::new_local(HashSet::new()),
        armed: RwSignal::new_local(None),
        on_clear_ship: StoredValue::new_local(on_clear_ship),
        toasts: Toasts::current(),
        clipboard: StoredValue::new_local(try_use_clipboard()),
        art: GameArt::new(),
        services,
        filter,
    };
    let sort: RwSignal<Option<ColumnSort>, LocalStorage> = RwSignal::new_local(None);
    let rows = Signal::derive_local(move || {
        let shown =
            summaries.with(|summaries| keys.with(|keys| filter.with(|filter| matching(summaries, keys, filter))));
        sort.with(|sort| sorted(shown, sort.as_ref()))
    });

    // A confirmation stands only for the games it was asked about.
    let disarming = RenderEffect::new(move |_| {
        games.with(|_| ());
        cx.armed.set(None);
    });
    let wanting = RenderEffect::new(move |_| {
        let data = loaded_build(services);
        let keys: Vec<ArtKey> = match &data {
            Some(data) => summaries.with(|summaries| {
                summaries
                    .iter()
                    .filter_map(|summary| data.provider().game_param_by_id(summary.ship_id))
                    .flat_map(|param| {
                        let class = param.species().and_then(|species| species.known()).copied().map(ArtKey::ShipClass);
                        [Some(ArtKey::NationFlag(param.nation().to_owned())), class]
                    })
                    .flatten()
                    .collect()
            }),
            None => Vec::new(),
        };
        cx.art.want(data.as_deref(), keys);
    });
    on_cleanup_local(move || {
        drop(disarming);
        drop(wanting);
    });

    let count = move || {
        let total = summaries.with(Vec::len);
        let visible = rows.with(Vec::len);
        format!("{visible}/{total}")
    };
    let placeholder = move || {
        locale.track();
        t!("ui.stats.search_ships").into_owned()
    };
    let escape = EscapeClears::new(query);

    view! {
        column(class = "wt-stats-ships", attr:data-testid = "stats-ships") {
            if move || summaries.with(Vec::is_empty) {
                box(class = "wt-stats-ships__empty") {
                    text {{locale.text("ui.stats.no_games")}}
                }
            } else {
                row(class = "wt-stats-ships__search") {
                    Input(
                        value = query,
                        placeholder = placeholder(),
                        label = placeholder(),
                        attr:data-testid = "stats-ships-search",
                        on:key_down = move |ev| {
                            escape.key_down(ev);
                        }
                    )
                    if move || query.with(|query| !query.is_empty()) {
                        Tooltip {
                            TooltipTrigger {
                                Button(
                                    variant = ButtonVariant::Ghost,
                                    size = ButtonSize::IconSm,
                                    a11y:label = locale.text("ui.stats.clear"),
                                    attr:data-testid = "stats-ships-clear-search",
                                    on:click = move |_| query.set(String::new())
                                ) {
                                    AppIcon(glyph = Glyph::X)
                                }
                            }
                            TooltipContent {{locale.text("ui.stats.clear")}}
                        }
                    } else {}
                    text(class = "wt-stats-ships__count", class = TABULAR_CLASS, attr:data-testid = "stats-ships-count") {{count}}
                }
                if move || rows.with(Vec::is_empty) {
                    box(class = "wt-stats-ships__empty") {
                        text {{locale.text("ui.stats.no_ship_matches")}}
                    }
                } else {
                    {move || {
                        locale.track();
                        ship_grid(cx, rows, sort, clearable)
                    }}
                }
            }
        }
    }
}

/// The ships, one row each, opening onto the ship's table.
fn ship_grid(
    cx: ShipsCx,
    rows: Signal<Vec<ShipRow>, LocalStorage>,
    sort: RwSignal<Option<ColumnSort>, LocalStorage>,
    clearable: bool,
) -> AnyView {
    let locale = wows_toolkit_viewmodel::locale();
    let number = ColumnSizing::Fixed(ColumnWidth(96.0));
    let damage_locale = locale.clone();
    let columns = vec![
        GridColumn::new(SortKey::Ship.id(), t!("ui.stats.column_ship").into_owned(), move |row: &ShipRow| {
            ship_cell(cx, row)
        })
        .text(|row| row.ship.clone())
        .sizing(ColumnSizing::Fixed(ColumnWidth(240.0)))
        .sortable(true),
        GridColumn::new(SortKey::Battles.id(), t!("ui.stats.column_battles").into_owned(), |row: &ShipRow| {
            row.performance.total_games().to_string()
        })
        .text(|row| row.performance.total_games().to_string())
        .sizing(number)
        .align(ColumnAlign::End)
        .sortable(true),
        GridColumn::new(SortKey::WinRate.id(), t!("ui.stats.column_win_rate").into_owned(), |row: &ShipRow| {
            optional_percent(row.performance.win_rate())
        })
        .text(|row| optional_percent(row.performance.win_rate()))
        .sizing(number)
        .align(ColumnAlign::End)
        .sortable(true),
        GridColumn::new(
            SortKey::AvgDamage.id(),
            t!("ui.stats.column_avg_damage").into_owned(),
            move |row: &ShipRow| optional_number(row.performance.avg_damage(), &damage_locale),
        )
        .text(move |row| optional_number(row.performance.avg_damage(), &locale))
        .sizing(number)
        .align(ColumnAlign::End)
        .sortable(true),
        GridColumn::new(SortKey::AvgFrags.id(), t!("ui.stats.column_avg_frags").into_owned(), |row: &ShipRow| {
            optional_decimal(row.performance.avg_frags())
        })
        .text(|row| optional_decimal(row.performance.avg_frags()))
        .sizing(number)
        .align(ColumnAlign::End)
        .sortable(true),
        GridColumn::new("actions", String::new(), move |row: &ShipRow| actions_cell(cx, row, clearable))
            .sizing(ColumnSizing::Fit)
            .resizable(false)
            .reorderable(false)
            .pinnable(false),
    ];
    let row_menu: RowMenu<GameParamId> = Rc::new(move |ship: GameParamId| {
        let summary = rows.with_untracked(|rows| rows.iter().find(|row| row.ship_id == ship).cloned());
        summary.map(|summary| MenuItems::new(move || copy_items(cx, summary.clone())))
    });

    AnyView::new(view! {
        Grid(
            rows = rows,
            columns = columns,
            row_key = |row: &ShipRow| row.ship_id,
            mode = RowMode::with_detail(|row: &ShipRow| view! { ShipDetail(summary = Arc::clone(row)) }),
            sort = sort,
            on_sort = UnsyncCallback::new(move |next| sort.set(next)),
            expanded = cx.expanded,
            row_menu = row_menu,
            label = t!("ui.stats.ships").into_owned(),
            attr:data-testid = "stats-ships-grid"
        )
    })
}

/// The disclosure chevron, the ship's flag and class, its name and its record.
fn ship_cell(cx: ShipsCx, row: &ShipRow) -> AnyView {
    let ship = row.ship_id;
    let expanded = cx.expanded;
    let open = move || expanded.with(|expanded| expanded.contains(&ship)).then(String::new);
    let param = loaded_build(cx.services).and_then(|data| data.provider().game_param_by_id(ship));
    let flag = param.as_ref().and_then(|param| cx.art.get(&ArtKey::NationFlag(param.nation().to_owned())));
    let class = param
        .as_ref()
        .and_then(|param| param.species()?.known().copied())
        .and_then(|species| cx.art.get(&ArtKey::ShipClass(species)));
    let flag = match flag {
        Some(Art::Raster(bytes)) => {
            Some(AnyView::new(view! { image(class = "wt-stats-ship__flag", src = Some(bytes.url())) }))
        }
        Some(Art::Vector(_)) | None => None,
    };
    let class = match class {
        Some(Art::Vector(svg)) => Some(AnyView::new(
            view! { vector(class = "wt-stats-ship__class", prop:svg = PropValue::from(svg.to_string())) },
        )),
        Some(Art::Raster(_)) | None => None,
    };
    let performance = &row.performance;
    let draws = (performance.draws() > 0).then(|| format!("{}D", performance.draws()));
    let rate = performance.win_rate().map(|rate| format!("({rate:.0}%)"));
    let rating = row.rating.as_ref().map(|rating| {
        let color = rating::text_color(rating.average.category);
        let text = format!("PR: {:.0}", rating.average.pr);
        AnyView::new(view! { text(style:color = color) {{text}} })
    });
    let wins = format!("{}W", performance.wins());
    let name = row.ship.clone();
    let filter = cx.filter;
    let losses = format!("{}L", performance.losses());
    let header = row.header.clone();

    AnyView::new(view! {
        row(class = "wt-stats-ship") {
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::IconXs,
                a11y:label = header,
                a11y:expanded = move || expanded.with(|expanded| expanded.contains(&ship)),
                attr:data-testid = format!("ship-toggle-{ship}"),
                on:click = move |ev: &mut EventCx<'_, events::Click>| {
                    ev.stop_propagation();
                    cx.toggle(ship);
                },
                on:double_click = move |ev: &mut EventCx<'_, events::DoubleClick>| ev.stop_propagation()
            ) {
                box(class = "wt-stats-chevron", attr:data-open = open) {
                    Icon(icon = CHEVRON_RIGHT, size = IconSize::Sm)
                }
            }
            {flag}
            {class}
            column(class = "wt-stats-ship__text") {
                {move || {
                    let ranges = filter.with(|filter| marks(filter, &name));
                    view! { MarkedText(class = "wt-stats-ship__name", text = name.clone(), ranges = ranges) }
                }}
                row(class = "wt-stats-ship__record", class = TABULAR_CLASS) {
                    text(class = "wt-stats-ship__win") {{wins}}
                    text(class = "wt-stats-ship__loss") {{losses}}
                    {draws.map(|draws| AnyView::new(view! { text {{draws}} }))}
                    {rate.map(|rate| AnyView::new(view! { text {{rate}} }))}
                    {rating}
                }
            }
        }
    })
}

/// The copy menu and, for recorded games, the button that forgets the ship's games.
fn actions_cell(cx: ShipsCx, row: &ShipRow, clearable: bool) -> AnyView {
    let ship = row.ship_id;
    let summary = Arc::clone(row);
    let copy = view! {
        Tooltip {
            TooltipTrigger {
                DropdownMenu {
                    DropdownMenuTrigger(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        a11y:label = t!("ui.stats.copy_table").into_owned(),
                        attr:data-testid = format!("ship-copy-{ship}")
                    ) {
                        AppIcon(glyph = Glyph::Copy)
                    }
                    DropdownMenuContent {
                        {copy_items(cx, Arc::clone(&summary))}
                    }
                }
            }
            TooltipContent {{t!("ui.stats.copy_table").into_owned()}}
        }
    };
    let clear = clearable.then(|| {
        let armed = move || cx.armed.get() == Some(ship);
        let name = row.ship.clone();
        let hint = t!("ui.stats.remove_games_hint", ship = name).into_owned();
        let label = {
            let hint = hint.clone();
            move || if armed() { t!("ui.stats.clear_ship_confirm").into_owned() } else { hint.clone() }
        };
        let accessible = {
            let hint = hint.clone();
            move || {
                if armed() { format!("{}: {hint}", t!("ui.stats.clear_ship_confirm")) } else { hint.clone() }
            }
        };
        let glyph = Signal::derive_local(move || if armed() { Glyph::CircleCheck } else { Glyph::Eraser });

        AnyView::new(view! {
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        class = "wt-stats-ship__clear",
                        attr:data-armed = move || armed().then(String::new),
                        attr:data-testid = format!("ship-clear-{ship}"),
                        a11y:label = accessible,
                        on:click = move |ev: &mut EventCx<'_, events::Click>| {
                            ev.stop_propagation();
                            cx.clear(ship, ev.modifiers.primary());
                        }
                    ) {
                        AppIcon(glyph = glyph)
                    }
                }
                TooltipContent {{label.clone()}}
            }
        })
    });

    AnyView::new(view! {
        row(class = "wt-stats-ship__actions") {
            {copy}
            {clear}
        }
    })
}

/// Copy as Markdown and as CSV, writing the same documents the GPUI copy menu writes.
fn copy_items(cx: ShipsCx, summary: ShipRow) -> AnyView {
    let markdown = summary.markdown(column_heading, stat_heading);
    let csv = summary.csv(column_heading, stat_heading);
    let ship = summary.ship_id;

    AnyView::new(view! {
        MenuItem(
            attr:data-testid = format!("copy-markdown-{ship}"),
            on_select = UnsyncCallback::new(move |()| cx.copy(markdown.clone()))
        ) {
            {t!("ui.stats.copy_markdown").into_owned()}
        }
        MenuItem(
            attr:data-testid = format!("copy-csv-{ship}"),
            on_select = UnsyncCallback::new(move |()| cx.copy(csv.clone()))
        ) {
            {t!("ui.stats.copy_csv").into_owned()}
        }
    })
}
