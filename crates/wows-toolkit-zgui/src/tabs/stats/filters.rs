//! The filter bar's ship and date controls.

use landaire_ui::EscapeClears;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_replays::types::GameParamId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::stats::DateSpan;
use wows_toolkit_viewmodel::stats::GameDay;
use wows_toolkit_viewmodel::text_filter::FilterKey;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use zgui::component;
use zgui::css;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CalendarMode;
use zgui_ui::Date;
use zgui_ui::DatePickerProps;
use zgui_ui::DateRange;
use zgui_ui::InputProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui_primitives::Binding;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::tabs::stats::FilterEditor;
use crate::tabs::stats::check_row::CheckRowProps;
use crate::text_marks::marks;

const SHEET: &str = "wt-stats-filters";

const CSS: &str = css!(
    ".wt-stats-filters__menu {
        display: flex;
        flex-direction: column;
        gap: 8px;
        min-width: 240px;
        max-width: 320px;
        max-height: 420px;
        overflow-y: auto;
        padding: 8px;
    }
    .wt-stats-filters__heading-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
    }
    .wt-stats-filters__heading {
        font-size: var(--zui-type-size-sm);
        font-weight: 700;
    }
    .wt-stats-filters__row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
    }
    .wt-stats-filters__search {
        flex: 1 1 auto;
    }
    .wt-stats-filters__dim {
        font-size: var(--zui-type-size-xs);
        color: var(--wt-color-text-dim);
    }
    .wt-stats-filters__narrowed[data-narrowed=true] {
        background-color: var(--zui-color-secondary);
    }"
);

/// Which ships count toward the stats, with a search over every ship played.
///
/// `ships` are every ship the tab's games were played in, by name.
#[component]
pub fn ShipFilter(editor: FilterEditor, #[prop(into)] ships: Signal<Vec<(GameParamId, String)>>) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let filters = editor.filters();
    let query = RwSignal::new_local(String::new());
    let filter = Memo::new(move |_| FilterQuery::new(&query.get()));
    let keys =
        Memo::new(move |_| ships.with(|ships| ships.iter().map(|(_, name)| FilterKey::new(name)).collect::<Vec<_>>()));
    let visible = Memo::new(move |_| {
        ships.with(|ships| {
            keys.with(|keys| {
                filter.with(|filter| {
                    let ranked = filter.rank(ships.iter().zip(keys), |(_, key)| *key);
                    ranked.into_iter().map(|(ship, _)| ship.clone()).collect::<Vec<_>>()
                })
            })
        })
    });
    let counts = move || {
        ships.with(|ships| {
            filters.with(|filters| {
                let kept = ships.iter().filter(|(ship, _)| !filters.excluded_ships.contains(ship)).count();
                (kept, ships.len())
            })
        })
    };
    let count_label = move || {
        locale.track();
        let (kept, total) = counts();
        t!("ui.stats.ship_selection_count", selected = kept, total = total).into_owned()
    };
    let none_visible = Signal::derive_local(move || visible.with(Vec::is_empty));
    let keep_visible = move |kept: bool| {
        let shown: Vec<GameParamId> = visible.with_untracked(|visible| visible.iter().map(|(ship, _)| *ship).collect());
        editor.edit(|filters| {
            for ship in shown {
                filters.set_ship_excluded(ship, !kept);
            }
        });
    };
    let ship_row = move |(ship, name): (GameParamId, String)| {
        let label = name.clone();
        view! {
            CheckRow(
                checked = Signal::derive_local(move || filters.with(|filters| !filters.excluded_ships.contains(&ship))),
                label = Signal::derive_local(move || label.clone()),
                marks = Signal::derive_local(move || filter.with(|filter| marks(filter, &name))),
                change = UnsyncCallback::new(move |kept: bool| editor.edit(|filters| filters.set_ship_excluded(ship, !kept))),
                test_id = format!("stats-ship-filter-{}", ship.raw())
            )
        }
    };
    let escape = EscapeClears::new(query);
    let tr = move |key: &'static str| {
        move || {
            locale.track();
            t!(key).into_owned()
        }
    };

    view! {
        Tooltip {
            TooltipTrigger {
                Popover {
                    PopoverTrigger(
                        size = ButtonSize::Sm,
                        variant = ButtonVariant::Outline,
                        class = "wt-stats-filters__narrowed",
                        attr:data-narrowed = move || { let (kept, total) = counts(); Some((kept < total).to_string()) },
                        a11y:label = move || format!("{}: {}", t!("ui.stats.ships"), count_label()),
                        attr:data-testid = "stats-ship-filter"
                    ) {
                        AppIcon(glyph = Glyph::Funnel)
                        text {{tr("ui.stats.ships")}}
                        text(class = "wt-stats-filters__dim") {{count_label}}
                    }
                    PopoverContent(class = "wt-stats-filters__menu", dismiss_on_escape = escape.dismiss_on_escape(), attr:data-testid = "stats-ship-filter-menu") {
                        row(class = "wt-stats-filters__heading-row") {
                            text(class = "wt-stats-filters__heading") {{tr("ui.stats.ships")}}
                            text(class = "wt-stats-filters__dim") {{count_label}}
                        }
                        row(class = "wt-stats-filters__row") {
                            box(class = "wt-stats-filters__search") {
                                Input(
                                    value = query,
                                    placeholder = t!("ui.stats.search_ships").into_owned(),
                                    node_ref = escape.node_ref(),
                                    attr:data-testid = "stats-ship-filter-search",
                                    on:key_down = move |ev| {
                                        escape.key_down(ev);
                                    }
                                )
                            }
                        }
                        row(class = "wt-stats-filters__row") {
                            Tooltip {
                                TooltipTrigger {
                                    Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline, disabled = none_visible, attr:data-testid = "stats-ship-filter-all", on:click = move |_| keep_visible(true)) {
                                        {tr("ui.stats.all_ships")}
                                    }
                                }
                                TooltipContent {{tr("ui.stats.keep_ships_tooltip")}}
                            }
                            Tooltip {
                                TooltipTrigger {
                                    Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline, disabled = none_visible, attr:data-testid = "stats-ship-filter-none", on:click = move |_| keep_visible(false)) {
                                        {tr("ui.stats.no_ships")}
                                    }
                                }
                                TooltipContent {{tr("ui.stats.leave_out_ships_tooltip")}}
                            }
                        }
                        for ship in move || visible.get(), key = |ship: &(GameParamId, String)| ship.0 {
                            {ship_row(ship)}
                        }
                    }
                }
            }
            TooltipContent {{tr("ui.stats.ship_filter_tooltip")}}
        }
    }
}

/// The span of days the stats cover, picked on a calendar, with a button that opens it again.
#[component]
pub fn DateFilter(editor: FilterEditor) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let filters = editor.filters();
    let span = Binding::controlled(
        Signal::derive_local(move || filters.with(|filters| picker_range(filters.dates))),
        move |range: Option<DateRange>| {
            let dates = span_of(range);
            if filters.with_untracked(|filters| filters.dates != dates) {
                editor.edit(|filters| filters.dates = dates);
            }
        },
    );
    let open = Signal::derive_local(move || filters.with(|filters| filters.dates.is_open()));
    let today = jiff::Zoned::now().date();
    let today = Date::clamped(i32::from(today.year()), today.month().unsigned_abs(), today.day().unsigned_abs());

    view! {
        row(class = "wt-stats-filters__row") {
            DatePicker(
                mode = CalendarMode::Range,
                range = span,
                months = 2,
                today = today,
                placeholder = t!("ui.stats.all_dates").into_owned(),
                label = t!("ui.stats.dates").into_owned(),
                attr:data-testid = "stats-date-filter"
            )
            if move || !open.get() {
                Tooltip {
                    TooltipTrigger {
                        Button(
                            size = ButtonSize::IconSm,
                            variant = ButtonVariant::Ghost,
                            a11y:label = move || { locale.track(); t!("ui.stats.clear_dates").into_owned() },
                            attr:data-testid = "stats-date-filter-clear",
                            on:click = move |_| editor.edit(|filters| filters.dates = DateSpan::default())
                        ) {
                            AppIcon(glyph = Glyph::X)
                        }
                    }
                    TooltipContent {{move || { locale.track(); t!("ui.stats.clear_dates").into_owned() }}}
                }
            } else {}
        }
    }
}

/// The picker's span for `dates`. A span with only a last day is not one the picker can show.
fn picker_range(dates: DateSpan) -> Option<DateRange> {
    let from = picker_date(dates.from?)?;
    Some(DateRange { from, to: dates.to.and_then(picker_date) })
}

/// The span `range` picks. A first day alone, between the two presses, keeps every day from it on.
fn span_of(range: Option<DateRange>) -> DateSpan {
    let Some(range) = range else { return DateSpan::default() };
    DateSpan { from: game_day(range.from), to: range.to.and_then(game_day) }
}

fn picker_date(day: GameDay) -> Option<Date> {
    Date::new(i32::from(day.0.year()), day.0.month().unsigned_abs(), day.0.day().unsigned_abs())
}

/// `None` for a day outside the years a battle can carry.
fn game_day(date: Date) -> Option<GameDay> {
    let year = i16::try_from(date.year()).ok()?;
    let month = i8::try_from(date.month()).ok()?;
    let day = i8::try_from(date.day()).ok()?;
    jiff::civil::Date::new(year, month, day).ok().map(GameDay)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> GameDay {
        GameDay(text.parse().expect("a test date"))
    }

    #[test]
    fn a_picked_span_reads_back_as_the_same_days() {
        let picked = DateSpan { from: Some(day("2026-02-27")), to: Some(day("2026-03-01")) };
        assert_eq!(span_of(picker_range(picked)), picked);

        let first_press = DateSpan { from: Some(day("2026-02-27")), to: None };
        assert_eq!(span_of(picker_range(first_press)), first_press);

        assert_eq!(picker_range(DateSpan::default()), None);
        assert_eq!(span_of(None), DateSpan::default());
    }
}
