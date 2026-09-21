//! The Stats tab's Ships panel: one collapsible table per ship played.
//!
//! Mirrors the egui app's per-ship section of `build_stats_overview`: a header
//! carrying the record and the rating, a copy menu writing the same Markdown
//! and CSV, and a min/max/total/average table underneath. The rows themselves
//! come from `wows_toolkit_viewmodel::stats::table`, so both front ends show
//! and copy the same figures.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

use wows_replays::types::GameParamId;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::PerformanceInfo;
use wows_toolkit_viewmodel::stats::PrStats;
use wows_toolkit_viewmodel::stats::table as stats_table;

use crate::icons;

const LABEL_COLUMN_WIDTH: Pixels = px(140.);
const CELL_COLUMN_WIDTH: Pixels = px(110.);
const COPY_MENU_WIDTH: Pixels = px(180.);

/// One ship's section, built once per filter change rather than per frame.
///
/// The two clipboard documents are built here too: `Render` runs every frame
/// and the popover that offers them is usually closed.
struct ShipSection {
    /// Element ids and the expansion set are keyed by this rather than by the
    /// name, which is not unique across ships.
    ship_id: GameParamId,
    ship: SharedString,
    /// The collapsed line: record, win rate, and the rating when there is one.
    header: SharedString,
    rows: Vec<stats_table::StatRow>,
    /// `sort_key` of this ship's most recent game, which orders the list.
    last_played: String,
    markdown: SharedString,
    csv: SharedString,
}

fn english_column(column: stats_table::Column) -> String {
    column.english().to_string()
}

fn english_label(label: stats_table::StatLabel) -> String {
    label.english().to_string()
}

pub struct StatsShipsPanel {
    sections: Vec<ShipSection>,
    /// Ships whose table is open, so the set survives a filter change that
    /// reorders or drops ships.
    expanded: HashSet<GameParamId>,
    /// The games the filter bar selected, kept so the sections can be rebuilt
    /// when the expected-values table arrives after them.
    games: Vec<PerGameStat>,
    personal_rating: Option<Arc<PersonalRatingData>>,
    scroll: ScrollHandle,
    focus_handle: FocusHandle,
}

impl EventEmitter<PanelEvent> for StatsShipsPanel {}

impl StatsShipsPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            sections: Vec::new(),
            expanded: HashSet::new(),
            games: Vec::new(),
            personal_rating: None,
            scroll: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    /// The expected-values table can arrive after the session, so the
    /// sections are rebuilt rather than left showing no rating until the user
    /// next touches a filter.
    pub fn set_personal_rating(&mut self, table: Option<Arc<PersonalRatingData>>, cx: &mut Context<Self>) {
        self.personal_rating = table;
        self.rebuild();
        cx.notify();
    }

    /// Adopts the games the filter bar has already selected.
    pub fn set_games(&mut self, games: &[&PerGameStat], cx: &mut Context<Self>) {
        self.games = games.iter().map(|game| (*game).clone()).collect();
        self.rebuild();
        cx.notify();
    }

    fn rebuild(&mut self) {
        // Grouped by id, not by name: two ships can carry one display name,
        // and a rating is scored against a single ship's expected values.
        let mut by_ship: BTreeMap<u64, Vec<&PerGameStat>> = BTreeMap::new();
        for game in &self.games {
            by_ship.entry(game.ship_id.raw()).or_default().push(game);
        }

        let mut sections: Vec<ShipSection> = by_ship
            .into_iter()
            .map(|(ship_id, games)| {
                let ship_id = GameParamId::from(ship_id);
                let info = PerformanceInfo::from_games(&games);
                // A group exists only because a game created it, so there is
                // always a win rate and always a name to read.
                let win_rate = info.win_rate().unwrap_or_default();
                let ship = games.first().map(|game| game.ship_name.as_str()).unwrap_or_default();
                let rating = self.personal_rating.as_ref().and_then(|table| PrStats::from_games(&games, table));

                let record = if info.draws() > 0 {
                    format!("{}W/{}L/{}D", info.wins(), info.losses(), info.draws())
                } else {
                    format!("{}W/{}L", info.wins(), info.losses())
                };
                let header = SharedString::from(match rating.as_ref() {
                    Some(rating) => format!("{ship} {record} ({win_rate:.0}%) - PR: {:.0}", rating.average.pr),
                    None => format!("{ship} {record} ({win_rate:.0}%)"),
                });
                let rows = stats_table::ship_rows(&info, rating.as_ref(), None);

                ShipSection {
                    ship_id,
                    ship: SharedString::from(ship.to_string()),
                    markdown: SharedString::from(stats_table::to_markdown(
                        &header,
                        &rows,
                        english_column,
                        english_label,
                    )),
                    csv: SharedString::from(stats_table::to_csv(&rows, english_column, english_label)),
                    header,
                    rows,
                    last_played: info.last_played().to_string(),
                }
            })
            .collect();

        // Most recently played first, which is the order the egui list opens
        // in, with the id breaking a tie so the order is total.
        sections.sort_by(|a, b| b.last_played.cmp(&a.last_played).then_with(|| a.ship_id.raw().cmp(&b.ship_id.raw())));

        self.sections = sections;
    }

    fn toggle(&mut self, ship_id: GameParamId, cx: &mut Context<Self>) {
        if !self.expanded.remove(&ship_id) {
            self.expanded.insert(ship_id);
        }
        cx.notify();
    }
}

impl Focusable for StatsShipsPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for StatsShipsPanel {
    fn panel_name(&self) -> &'static str {
        "StatsShipsPanel"
    }

    /// Not closable, for the same reason the Overview is not: nothing reopens
    /// it, and the egui tab's per-ship table is always there.
    fn closable(&self, _cx: &App) -> bool {
        false
    }
}

impl Panel for StatsShipsPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Ships")
    }
}

impl Render for StatsShipsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.sections.is_empty() {
            return v_flex()
                .id("stats-ships")
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().opacity(0.6).child("No games recorded for the current filters"))
                .into_any_element();
        }

        let border = cx.theme().border;
        let entity = cx.entity();

        let sections = self.sections.iter().map(|section| {
            let open = self.expanded.contains(&section.ship_id);
            let ship_id = section.ship_id;
            let ship = section.ship.clone();
            let panel = entity.clone();

            let header = h_flex()
                .id(SharedString::from(format!("ship-header-{ship}")))
                .w_full()
                .gap_2()
                .items_center()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(border)
                .child(
                    Button::new(SharedString::from(format!("ship-toggle-{ship_id}")))
                        .ghost()
                        .compact()
                        .flex_1()
                        .justify_start()
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                // The caret is a glyph in the icon font, so it
                                // goes through `icons::icon` rather than into a
                                // label the UI font would render as a box.
                                .child(icons::icon(if open { icons::CARET_DOWN } else { icons::CARET_RIGHT }))
                                .child(div().text_sm().child(section.header.clone())),
                        )
                        .on_click(move |_event, _window, cx: &mut App| {
                            panel.update(cx, |this, cx| this.toggle(ship_id, cx));
                        }),
                )
                .child(copy_menu(section));

            let table = open.then(|| {
                let heading = h_flex().w_full().gap_2().px_2().py_1().child(div().w(LABEL_COLUMN_WIDTH)).children(
                    stats_table::COLUMNS.map(|column| {
                        div().w(CELL_COLUMN_WIDTH).text_xs().font_weight(FontWeight::BOLD).child(column.english())
                    }),
                );

                let rows = section.rows.iter().enumerate().map(|(ix, row)| {
                    h_flex()
                        .w_full()
                        .gap_2()
                        .px_2()
                        .py(px(2.))
                        .when_some(crate::ui::stripe(ix, cx), |el, color| el.bg(color))
                        .child(div().w(LABEL_COLUMN_WIDTH).text_sm().child(row.label.english()))
                        .children(stats_table::COLUMNS.map(|column| {
                            let cell = row.cell(column);
                            div()
                                .id(SharedString::from(format!("ship-cell-{ship_id}-{column:?}-{:?}", row.label)))
                                .w(CELL_COLUMN_WIDTH)
                                .text_sm()
                                .when(column == stats_table::Column::Average, |this| this.font_weight(FontWeight::BOLD))
                                // A rating is coloured by the band it carries,
                                // and names that band on hover as the egui chip
                                // does. Dark only, like the rest of the port.
                                .when_some(cell.rating.as_ref(), |this, rating| {
                                    this.text_color(rgb(personal_rating::chip_text(
                                        rating.category,
                                        crate::theme::is_dark_mode(),
                                    )))
                                    .tooltip({
                                        let name = rating.category.name().to_string();
                                        move |window, cx| Tooltip::new(name.clone()).build(window, cx)
                                    })
                                })
                                .child(cell.text.clone())
                        }))
                });

                v_flex().w_full().py_1().border_b_1().border_color(border).child(heading).children(rows)
            });

            v_flex().w_full().child(header).when_some(table, |this, table| this.child(table))
        });

        v_flex()
            .id("stats-ships")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .children(sections)
            .into_any_element()
    }
}

/// The copy dropdown, writing the same two documents the egui menu does.
fn copy_menu(section: &ShipSection) -> impl IntoElement {
    let markdown = section.markdown.clone();
    let csv = section.csv.clone();
    let ship_id = section.ship_id;

    let trigger = Button::new(SharedString::from(format!("ship-copy-{ship_id}")))
        .child(icons::icon(icons::COPY))
        .compact()
        .tooltip("Copy this table");

    Popover::new(SharedString::from(format!("ship-copy-menu-{ship_id}"))).trigger(trigger).content(
        move |_state, _window, _cx| {
            let markdown = markdown.clone();
            let csv = csv.clone();
            v_flex()
                .w(COPY_MENU_WIDTH)
                .gap_1()
                .p_1()
                .child(
                    Button::new(SharedString::from(format!("copy-markdown-{ship_id}")))
                        .label("Copy as Markdown")
                        .compact()
                        .on_click(move |_event, _window, cx: &mut App| {
                            cx.write_to_clipboard(ClipboardItem::new_string(markdown.to_string()));
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("copy-csv-{ship_id}")))
                        .label("Copy as CSV")
                        .compact()
                        .on_click(move |_event, _window, cx: &mut App| {
                            cx.write_to_clipboard(ClipboardItem::new_string(csv.to_string()));
                        }),
                )
        },
    )
}

// The test module imports by name: `use super::*` would pull in the glob
// `gpui_kit::*` re-export of GPUI's own `test` macro, which shadows Rust's
// `#[test]` and sends the attribute into infinite expansion.
#[cfg(test)]
mod tests {
    use gpui_kit::AppContext;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt;

    use super::StatsShipsPanel;
    use wows_toolkit_viewmodel::stats::PerGameStat;
    use wows_toolkit_viewmodel::stats::table::StatLabel;

    fn game(ship: &str, ship_id: u64, sort_key: &str, damage: u64, frags: i64, win: bool) -> PerGameStat {
        PerGameStat {
            ship_name: ship.to_string(),
            ship_id: ship_id.into(),
            game_time: sort_key.to_string(),
            sort_key: sort_key.to_string(),
            player_id: 1,
            damage,
            spotting_damage: damage / 10,
            frags,
            raw_xp: 1_500,
            base_xp: 900,
            is_win: win,
            is_loss: !win,
            is_draw: false,
            is_div: false,
            match_group: "pvp".to_string(),
            achievements: Vec::new(),
        }
    }

    const YAMATO: u64 = 1;
    const SHIMA: u64 = 2;

    /// The header is the line the egui section shows, and the ships are
    /// ordered by when they were last played.
    #[gpui_kit::test]
    fn each_ship_carries_its_record_and_the_newest_leads(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| StatsShipsPanel::new(cx));

        let games = [
            game("Yamato", YAMATO, "2026-02-13 14:00:00", 100_000, 2, true),
            game("Yamato", YAMATO, "2026-02-13 15:00:00", 50_000, 0, false),
            game("Shima", SHIMA, "2026-02-13 16:00:00", 70_000, 1, true),
        ];

        window
            .update(cx, |panel, _window, cx| {
                let refs: Vec<&PerGameStat> = games.iter().collect();
                panel.set_games(&refs, cx);

                let headers: Vec<String> = panel.sections.iter().map(|s| s.header.to_string()).collect();
                assert_eq!(headers, vec!["Shima 1W/0L (100%)", "Yamato 1W/1L (50%)"]);
            })
            .expect("the test window stays open");
    }

    /// Two ships that share a display name stay two sections: the rating is
    /// scored against one ship's expected values, so pooling them would rate
    /// both as the first.
    #[gpui_kit::test]
    fn two_ships_sharing_a_name_stay_apart(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| StatsShipsPanel::new(cx));

        let games = [
            game("Mikasa", YAMATO, "2026-02-13 14:00:00", 100_000, 2, true),
            game("Mikasa", SHIMA, "2026-02-13 15:00:00", 50_000, 0, false),
        ];

        window
            .update(cx, |panel, _window, cx| {
                let refs: Vec<&PerGameStat> = games.iter().collect();
                panel.set_games(&refs, cx);

                assert_eq!(panel.sections.len(), 2, "one section per ship, not per name");
            })
            .expect("the test window stays open");
    }

    /// Without an expected-values table nothing rates, so the table opens on
    /// damage rather than on an invented rating row.
    #[gpui_kit::test]
    fn an_unrated_session_has_no_rating_row(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| StatsShipsPanel::new(cx));
        let games = [game("Yamato", YAMATO, "2026-02-13 14:00:00", 100_000, 2, true)];

        window
            .update(cx, |panel, _window, cx| {
                let refs: Vec<&PerGameStat> = games.iter().collect();
                panel.set_games(&refs, cx);

                let section = panel.sections.first().expect("the ship is listed");
                assert_eq!(section.rows[0].label, StatLabel::Damage);
            })
            .expect("the test window stays open");
    }

    /// A section opens and closes on its own header, and only that section's.
    #[gpui_kit::test]
    fn clicking_a_header_opens_only_that_ship(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| StatsShipsPanel::new(cx));
        let games = [
            game("Yamato", YAMATO, "2026-02-13 14:00:00", 100_000, 2, true),
            game("Shima", SHIMA, "2026-02-13 16:00:00", 70_000, 1, true),
        ];

        window
            .update(cx, |panel, _window, cx| {
                let refs: Vec<&PerGameStat> = games.iter().collect();
                panel.set_games(&refs, cx);
            })
            .expect("the test window stays open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(format!("ship-toggle-{SHIMA}"), cx);
        })
        .expect("the test window stays open");

        window
            .update(cx, |panel, _window, _cx| {
                assert!(panel.expanded.contains(&SHIMA.into()), "the clicked ship opened");
                assert!(!panel.expanded.contains(&YAMATO.into()), "its neighbour stayed closed");
            })
            .expect("the test window stays open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(format!("ship-toggle-{SHIMA}"), cx);
        })
        .expect("the test window stays open");

        window
            .update(cx, |panel, _window, _cx| {
                assert!(panel.expanded.is_empty(), "clicking again closes it");
            })
            .expect("the test window stays open");
    }

    /// What a copy writes is the same table that is on screen.
    #[gpui_kit::test]
    fn the_copy_menu_writes_the_table_it_shows(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| StatsShipsPanel::new(cx));
        let games = [game("Yamato", YAMATO, "2026-02-13 14:00:00", 100_000, 2, true)];

        window
            .update(cx, |panel, _window, cx| {
                let refs: Vec<&PerGameStat> = games.iter().collect();
                panel.set_games(&refs, cx);

                let section = panel.sections.first().expect("the ship is listed");
                assert!(section.markdown.starts_with("**Yamato 1W/0L (100%)**"), "the copy carries the header line");
                assert!(section.markdown.contains("| Damage | 100,000 | 100,000 | 100,000 | **100,000** |"));

                assert!(section.csv.starts_with(",Min,Max,Total,Average"));
                assert!(section.csv.contains("Spotting Damage,10,000,10,000,10,000,10,000"));
            })
            .expect("the test window stays open");
    }

    /// The expected-values table can land after the session; the sections
    /// pick it up rather than waiting for the next filter change.
    #[gpui_kit::test]
    fn a_rating_table_arriving_late_still_rates_the_ships(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| StatsShipsPanel::new(cx));
        let table = std::sync::Arc::new(crate::replay_inspector::test_support::fixture_personal_rating_data());
        let rated_ship = crate::replay_inspector::test_support::FIXTURE_PR_SHIP_ID;
        let games = [game("Yamato", rated_ship, "2026-02-13 14:00:00", 100_000, 2, true)];

        window
            .update(cx, |panel, _window, cx| {
                let refs: Vec<&PerGameStat> = games.iter().collect();
                panel.set_games(&refs, cx);
                assert_eq!(panel.sections[0].rows[0].label, StatLabel::Damage, "nothing rates yet");

                panel.set_personal_rating(Some(table.clone()), cx);
                assert_eq!(
                    panel.sections[0].rows[0].label,
                    StatLabel::PersonalRating,
                    "the late table reaches the rows"
                );
            })
            .expect("the test window stays open");
    }
}
