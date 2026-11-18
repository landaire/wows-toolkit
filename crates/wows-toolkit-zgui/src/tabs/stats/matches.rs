//! The Matches panel: every game the other filters leave, newest first, each with a box that keeps
//! it in the stats or leaves it out.

use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::ColumnAlign;
use landaire_ui::ColumnSizing;
use landaire_ui::ColumnWidth;
use landaire_ui::EscapeClears;
use landaire_ui::GridColumn;
use landaire_ui::GridProps;
use landaire_ui::MenuItems;
use landaire_ui::RowHeight;
use landaire_ui::RowMenu;
use landaire_ui::RowMode;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::formatting::separate_number;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::match_group_display_name;
use wows_toolkit_viewmodel::text_filter::FilterKey;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::InputProps;
use zgui_ui::MenuItemProps;
use zgui_ui_primitives::Binding;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::tabs::stats::FilterEditor;
use crate::tabs::stats::check_row::CheckRowProps;

const SHEET: &str = "wt-stats-matches";

/// Each row is this tall, which is what lets the table build only the rows in view: a directory
/// can hold thousands of games.
const ROW_HEIGHT: RowHeight = RowHeight(26.0);

const CSS: &str = css!(
    ".wt-stats-matches {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
    }
    .wt-stats-matches__bar {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 8px;
    }
    .wt-stats-matches__search {
        flex: 1 1 160px;
        min-width: 120px;
    }
    .wt-stats-matches__count {
        font-size: 0.85em;
        color: var(--wt-color-text-dim);
    }
    .wt-stats-matches .lui-grid { flex: 1 1 auto; }
    .wt-stats-matches__empty {
        display: flex;
        flex: 1 1 auto;
        align-items: center;
        justify-content: center;
        color: var(--wt-color-text-dim);
    }
    .wt-stats-match__left-out {
        color: var(--wt-color-text-dim);
        text-decoration: line-through;
    }
    .wt-stats-match__win { color: var(--wt-color-win); }
    .wt-stats-match__loss { color: var(--wt-color-loss); }"
);

/// One game in the table, read from the tab's games where it sits.
#[derive(Clone)]
struct MatchRow {
    games: Arc<[PerGameStat]>,
    at: usize,
}

impl MatchRow {
    fn game(&self) -> &PerGameStat {
        &self.games[self.at]
    }
}

/// The same game of the same read: a new read of the games is a new row, though at the same place.
impl PartialEq for MatchRow {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at && Arc::ptr_eq(&self.games, &other.games)
    }
}

/// `games` are every game the tab holds, oldest first.
#[component]
pub fn StatsMatches(#[prop(into)] games: Signal<Arc<[PerGameStat]>>, editor: FilterEditor) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let filters = editor.filters();
    let query = RwSignal::new_local(String::new());
    let filter = Memo::new(move |_| FilterQuery::new(&query.get()));
    let left_out_only = RwSignal::new_local(false);

    // Positions of the games the other filters admit, newest first.
    let admitted = Memo::new(move |_| {
        games.with(|games| {
            filters.with(|filters| {
                games
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, game)| filters.admits(game))
                    .map(|(at, _)| at)
                    .collect::<Vec<_>>()
            })
        })
    });
    let left_out = Memo::new(move |_| {
        games.with(|games| {
            filters.with(|filters| {
                admitted
                    .with(|admitted| admitted.iter().filter(|&&at| filters.excluded_games.contains(&games[at])).count())
            })
        })
    });
    // Each game's ship name folded once, so a keystroke only scores.
    let keys = Memo::new(move |_| {
        games.with(|games| games.iter().map(|game| FilterKey::new(&game.ship_name)).collect::<Vec<_>>())
    });
    let listed = Memo::new(move |_| {
        let games = games.get();
        let only_left_out = left_out_only.get();
        filters.with(|filters| {
            filter.with(|filter| {
                admitted.with(|admitted| {
                    admitted
                        .iter()
                        .filter(|&&at| {
                            (!only_left_out || filters.excluded_games.contains(&games[at]))
                                && keys.with(|keys| filter.score(&keys[at]).is_some())
                        })
                        .map(|&at| MatchRow { games: Arc::clone(&games), at })
                        .collect::<Vec<_>>()
                })
            })
        })
    });
    // The grid takes a local signal, which reads the projection without working it out again.
    let rows: Signal<Vec<MatchRow>, LocalStorage> = Signal::derive_local(move || listed.get());

    let count = move || {
        locale.track();
        t!(
            "ui.stats.match_count",
            shown = rows.with(Vec::len),
            total = admitted.with(Vec::len),
            left_out = left_out.get()
        )
        .into_owned()
    };
    let none_left_out = Signal::derive_local(move || filters.with(|filters| filters.excluded_games.is_empty()));
    let placeholder = move || {
        locale.track();
        t!("ui.stats.search_matches").into_owned()
    };
    let escape = EscapeClears::new(query);

    view! {
        column(class = "wt-stats-matches", attr:data-testid = "stats-matches") {
            row(class = "wt-stats-matches__bar") {
                box(class = "wt-stats-matches__search") {
                    Input(
                        value = query,
                        placeholder = placeholder(),
                        label = placeholder(),
                        attr:data-testid = "stats-matches-search",
                        on:key_down = move |ev| {
                            escape.key_down(ev);
                        }
                    )
                }
                CheckRow(
                    checked = left_out_only,
                    label = Signal::derive_local(move || { locale.track(); t!("ui.stats.left_out_only").into_owned() }),
                    change = UnsyncCallback::new(move |on: bool| left_out_only.set(on)),
                    test_id = "stats-matches-left-out-only"
                )
                Tooltip {
                    TooltipTrigger {
                        Button(
                            size = ButtonSize::Sm,
                            variant = ButtonVariant::Outline,
                            disabled = none_left_out,
                            attr:data-testid = "stats-matches-keep-all",
                            on:click = move |_| editor.edit(|filters| filters.excluded_games.clear())
                        ) {
                            AppIcon(glyph = Glyph::RotateCcw)
                            text {{move || { locale.track(); t!("ui.stats.keep_all_games").into_owned() }}}
                        }
                    }
                    TooltipContent {{move || { locale.track(); t!("ui.stats.keep_all_games_tooltip").into_owned() }}}
                }
                text(class = "wt-stats-matches__count", class = TABULAR_CLASS, attr:data-testid = "stats-matches-count") {{count}}
            }
            if move || rows.with(Vec::is_empty) {
                box(class = "wt-stats-matches__empty") {
                    text {{locale.text("ui.stats.no_matches")}}
                }
            } else {
                {move || {
                    locale.track();
                    match_grid(editor, rows)
                }}
            }
        }
    }
}

fn match_grid(editor: FilterEditor, rows: Signal<Vec<MatchRow>, LocalStorage>) -> AnyView {
    let filters = editor.filters();
    let figures = wows_toolkit_viewmodel::locale();
    let number = ColumnSizing::Fixed(ColumnWidth(88.0));
    let left_out = move |row: &MatchRow| filters.with(|filters| filters.excluded_games.contains(row.game()));
    let damage = {
        let figures = figures.clone();
        move |row: &MatchRow| separate_number(row.game().damage, Some(&figures))
    };
    let base_xp = move |row: &MatchRow| separate_number(row.game().base_xp, Some(&figures));

    let columns = vec![
        GridColumn::new("kept", String::new(), move |row: &MatchRow| keep_cell(editor, row))
            .sizing(ColumnSizing::Fixed(ColumnWidth(36.0)))
            .resizable(false)
            .reorderable(false)
            .pinnable(false),
        GridColumn::new("time", t!("ui.stats.column_time").into_owned(), move |row: &MatchRow| {
            let time = row.game().game_time.clone();
            let row = row.clone();
            AnyView::new(view! {
                text(class:wt-stats-match__left-out = move || left_out(&row)) {{time}}
            })
        })
        .text(|row| row.game().game_time.clone())
        .sizing(ColumnSizing::Fixed(ColumnWidth(150.0))),
        GridColumn::new("ship", t!("ui.stats.column_ship").into_owned(), |row: &MatchRow| row.game().ship_name.clone())
            .text(|row| row.game().ship_name.clone())
            .sizing(ColumnSizing::Fixed(ColumnWidth(160.0))),
        GridColumn::new("mode", t!("ui.stats.column_mode").into_owned(), |row: &MatchRow| mode_text(row.game()))
            .text(|row| mode_text(row.game()))
            .sizing(ColumnSizing::Fixed(ColumnWidth(150.0))),
        GridColumn::new("result", t!("ui.stats.column_result").into_owned(), result_cell)
            .text(|row| result_text(row.game()))
            .sizing(ColumnSizing::Fixed(ColumnWidth(72.0))),
        GridColumn::new("damage", t!("stat.damage").into_owned(), damage.clone())
            .text(damage)
            .sizing(number)
            .align(ColumnAlign::End),
        GridColumn::new("frags", t!("stat.frags").into_owned(), |row: &MatchRow| row.game().frags.to_string())
            .text(|row| row.game().frags.to_string())
            .sizing(ColumnSizing::Fixed(ColumnWidth(56.0)))
            .align(ColumnAlign::End),
        GridColumn::new("base-xp", t!("stat.base_xp").into_owned(), base_xp.clone())
            .text(base_xp)
            .sizing(number)
            .align(ColumnAlign::End),
    ];
    let row_menu: RowMenu<usize> = Rc::new(move |at: usize| {
        let row = rows.with_untracked(|rows| rows.iter().find(|row| row.at == at).cloned())?;
        Some(MenuItems::new(move || menu_items(editor, row.clone())))
    });

    AnyView::new(view! {
        Grid(
            rows = rows,
            columns = columns,
            row_key = |row: &MatchRow| row.at,
            mode = RowMode::fixed(ROW_HEIGHT),
            row_menu = row_menu,
            label = t!("ui.stats.matches").into_owned(),
            attr:data-testid = "stats-matches-grid"
        )
    })
}

/// The box that keeps the game in the stats while ticked.
fn keep_cell(editor: FilterEditor, row: &MatchRow) -> AnyView {
    let filters = editor.filters();
    let read = row.clone();
    let write = row.clone();
    let row_label = row.clone();
    let kept = Binding::controlled(
        Signal::derive_local(move || {
            if filters.with(|filters| filters.excluded_games.contains(read.game())) {
                Checked::No
            } else {
                Checked::Yes
            }
        }),
        move |next: Checked| editor.edit(|filters| filters.excluded_games.set(write.game(), next != Checked::Yes)),
    );
    let label = move || {
        if filters.with(|filters| filters.excluded_games.contains(row_label.game())) {
            t!("ui.stats.keep_game").into_owned()
        } else {
            t!("ui.stats.leave_out_game").into_owned()
        }
    };

    // The box acts on its own rather than also selecting the row.
    AnyView::new(view! {
        row(on:click = |ev: &mut EventCx<'_, events::Click>| ev.stop_propagation()) {
            Checkbox(checked = kept, a11y:label = label, attr:data-testid = format!("stats-match-keep-{}", row.at))
        }
    })
}

fn menu_items(editor: FilterEditor, row: MatchRow) -> AnyView {
    let left_out = editor.filters().with_untracked(|filters| filters.excluded_games.contains(row.game()));
    let ship = row.game().ship_id;
    let toggled = row.clone();
    let game_label = if left_out { t!("ui.stats.keep_game") } else { t!("ui.stats.leave_out_game") };

    AnyView::new(view! {
        MenuItem(
            attr:data-testid = format!("stats-match-toggle-{}", row.at),
            on_select = UnsyncCallback::new(move |()| editor.edit(|filters| filters.excluded_games.set(toggled.game(), !left_out)))
        ) {
            {game_label.into_owned()}
        }
        MenuItem(
            attr:data-testid = format!("stats-match-leave-out-ship-{}", row.at),
            on_select = UnsyncCallback::new(move |()| editor.edit(|filters| filters.set_ship_excluded(ship, true)))
        ) {
            {t!("ui.stats.leave_out_ship", ship = row.game().ship_name.as_str()).into_owned()}
        }
    })
}

fn mode_text(game: &PerGameStat) -> String {
    let division = if game.is_div { t!("ui.stats.div_div") } else { t!("ui.stats.div_solo") };
    format!("{}, {division}", match_group_display_name(&game.match_group))
}

fn result_text(game: &PerGameStat) -> String {
    let key = match (game.is_win, game.is_loss, game.is_draw) {
        (true, _, _) => "ui.search.outcome_win",
        (_, true, _) => "ui.search.outcome_loss",
        (_, _, true) => "ui.search.outcome_draw",
        _ => "ui.search.outcome_unknown",
    };
    t!(key).into_owned()
}

fn result_cell(row: &MatchRow) -> AnyView {
    let game = row.game();
    AnyView::new(view! {
        text(class:wt-stats-match__win = game.is_win, class:wt-stats-match__loss = game.is_loss) {{result_text(game)}}
    })
}
