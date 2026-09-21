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

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
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
struct ShipSection {
    ship: SharedString,
    /// The collapsed line: record, win rate, and the rating when there is one.
    header: SharedString,
    rows: Vec<stats_table::StatRow>,
    /// `sort_key` of this ship's most recent game, which orders the list.
    last_played: String,
    /// The rating row's band, which colours its cells. Absent when the ship
    /// could not be rated, in which case there is no rating row either.
    rating: Option<PrStats>,
}

impl ShipSection {
    /// The Markdown a copy writes, headed by the same line the section shows.
    fn markdown(&self) -> String {
        stats_table::to_markdown(&self.header, &self.rows, english_column, english_label)
    }

    fn csv(&self) -> String {
        stats_table::to_csv(&self.rows, english_column, english_label)
    }
}

/// The band a rating cell is coloured by. The total column carries no rating,
/// so it carries no band either.
fn rating_band(rating: PrStats, column: stats_table::Column) -> Option<PersonalRatingCategory> {
    let value = match column {
        stats_table::Column::Min => rating.min,
        stats_table::Column::Max => rating.max,
        stats_table::Column::Average => rating.average,
        stats_table::Column::Total => return None,
    };
    Some(PersonalRatingCategory::from_pr(value))
}

fn english_column(column: stats_table::Column) -> String {
    column.english().to_string()
}

fn english_label(label: stats_table::StatLabel) -> String {
    label.english().to_string()
}

pub struct StatsShipsPanel {
    sections: Vec<ShipSection>,
    /// Ships whose table is open. Keyed by name so the set survives a filter
    /// change that reorders or drops ships.
    expanded: HashSet<SharedString>,
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
            personal_rating: None,
            scroll: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn set_personal_rating(&mut self, table: Option<Arc<PersonalRatingData>>, cx: &mut Context<Self>) {
        self.personal_rating = table;
        cx.notify();
    }

    /// Adopts the games the filter bar has already selected.
    pub fn set_games(&mut self, games: &[&PerGameStat], cx: &mut Context<Self>) {
        let mut by_ship: BTreeMap<&str, Vec<&PerGameStat>> = BTreeMap::new();
        for game in games {
            by_ship.entry(game.ship_name.as_str()).or_default().push(game);
        }

        let mut sections: Vec<ShipSection> = by_ship
            .into_iter()
            .filter_map(|(ship, games)| {
                let info = PerformanceInfo::from_games(&games);
                // A ship whose games all lack a result has no record to show,
                // which is the same line the egui section skips.
                let win_rate = info.win_rate()?;
                let rating = self.personal_rating.as_ref().and_then(|table| PrStats::from_games(&games, table));

                let record = if info.draws() > 0 {
                    format!("{}W/{}L/{}D", info.wins(), info.losses(), info.draws())
                } else {
                    format!("{}W/{}L", info.wins(), info.losses())
                };
                let header = match rating.as_ref() {
                    Some(rating) => format!("{ship} {record} ({win_rate:.0}%) - PR: {:.0}", rating.average),
                    None => format!("{ship} {record} ({win_rate:.0}%)"),
                };

                Some(ShipSection {
                    ship: SharedString::from(ship.to_string()),
                    header: SharedString::from(header),
                    rows: stats_table::ship_rows(&info, rating.as_ref(), None),
                    last_played: info.last_played().to_string(),
                    rating,
                })
            })
            .collect();

        // Most recently played first, which is the order the egui list opens
        // in, with the name breaking a tie so the order is total.
        sections.sort_by(|a, b| b.last_played.cmp(&a.last_played).then_with(|| a.ship.cmp(&b.ship)));

        self.sections = sections;
        cx.notify();
    }

    fn toggle(&mut self, ship: &SharedString, cx: &mut Context<Self>) {
        if !self.expanded.remove(ship) {
            self.expanded.insert(ship.clone());
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
            let open = self.expanded.contains(&section.ship);
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
                    Button::new(SharedString::from(format!("ship-toggle-{ship}")))
                        .ghost()
                        .compact()
                        .flex_1()
                        .justify_start()
                        .label(format!(
                            "{} {}",
                            if open { icons::CARET_DOWN } else { icons::CARET_RIGHT },
                            section.header
                        ))
                        .on_click(move |_event, _window, cx: &mut App| {
                            panel.update(cx, |this, cx| this.toggle(&ship, cx));
                        }),
                )
                .child(copy_menu(section));

            let table = open.then(|| {
                let heading = h_flex().w_full().gap_2().px_2().py_1().child(div().w(LABEL_COLUMN_WIDTH)).children(
                    stats_table::COLUMNS.map(|column| {
                        div().w(CELL_COLUMN_WIDTH).text_xs().font_weight(FontWeight::BOLD).child(column.english())
                    }),
                );

                let rating = section.rating;
                let rows = section.rows.iter().map(move |row| {
                    let rated = row.label == stats_table::StatLabel::PersonalRating;
                    h_flex()
                        .w_full()
                        .gap_2()
                        .px_2()
                        .py(px(1.))
                        .child(div().w(LABEL_COLUMN_WIDTH).text_sm().child(row.label.english()))
                        .children(stats_table::COLUMNS.map(|column| {
                            // Each rating cell takes its band from its own
                            // figure, not from the text it was formatted into.
                            let band = rated.then(|| rating.and_then(|rating| rating_band(rating, column))).flatten();
                            div()
                                .w(CELL_COLUMN_WIDTH)
                                .text_sm()
                                .when(column == stats_table::Column::Average, |this| this.font_weight(FontWeight::BOLD))
                                .when_some(band, |this, band| {
                                    this.text_color(rgb(wows_toolkit_viewmodel::personal_rating::chip_text(band, true)))
                                })
                                .child(row.cell(column).to_string())
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
    let markdown = section.markdown();
    let csv = section.csv();
    let ship = section.ship.clone();

    let trigger = Button::new(SharedString::from(format!("ship-copy-{ship}")))
        .child(icons::icon(icons::COPY))
        .compact()
        .disabled(false)
        .tooltip("Copy this table");

    Popover::new(SharedString::from(format!("ship-copy-menu-{ship}"))).trigger(trigger).content(
        move |_state, _window, _cx| {
            let markdown = markdown.clone();
            let csv = csv.clone();
            v_flex()
                .w(COPY_MENU_WIDTH)
                .gap_1()
                .p_1()
                .child(Button::new("copy-markdown").label("Copy as Markdown").compact().on_click(
                    move |_event, _window, cx: &mut App| {
                        cx.write_to_clipboard(ClipboardItem::new_string(markdown.clone()));
                    },
                ))
                .child(Button::new("copy-csv").label("Copy as CSV").compact().on_click(
                    move |_event, _window, cx: &mut App| {
                        cx.write_to_clipboard(ClipboardItem::new_string(csv.clone()));
                    },
                ))
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

    fn game(ship: &str, sort_key: &str, damage: u64, frags: i64, win: bool) -> PerGameStat {
        PerGameStat {
            ship_name: ship.to_string(),
            ship_id: 1u64.into(),
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

    /// The header is the line the egui section shows, and the ships are
    /// ordered by when they were last played.
    #[gpui_kit::test]
    fn each_ship_carries_its_record_and_the_newest_leads(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| StatsShipsPanel::new(cx));

        let games = [
            game("Yamato", "2026-02-13 14:00:00", 100_000, 2, true),
            game("Yamato", "2026-02-13 15:00:00", 50_000, 0, false),
            game("Shima", "2026-02-13 16:00:00", 70_000, 1, true),
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

    /// Without an expected-values table nothing rates, so the table opens on
    /// damage rather than on an invented rating row.
    #[gpui_kit::test]
    fn an_unrated_session_has_no_rating_row(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| StatsShipsPanel::new(cx));
        let games = [game("Yamato", "2026-02-13 14:00:00", 100_000, 2, true)];

        window
            .update(cx, |panel, _window, cx| {
                let refs: Vec<&PerGameStat> = games.iter().collect();
                panel.set_games(&refs, cx);

                let section = panel.sections.first().expect("the ship is listed");
                assert!(section.rating.is_none(), "no table was handed over");
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
            game("Yamato", "2026-02-13 14:00:00", 100_000, 2, true),
            game("Shima", "2026-02-13 16:00:00", 70_000, 1, true),
        ];

        window
            .update(cx, |panel, _window, cx| {
                let refs: Vec<&PerGameStat> = games.iter().collect();
                panel.set_games(&refs, cx);
            })
            .expect("the test window stays open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("ship-toggle-Shima", cx);
        })
        .expect("the test window stays open");

        window
            .update(cx, |panel, _window, _cx| {
                assert!(panel.expanded.contains("Shima"), "the clicked ship opened");
                assert!(!panel.expanded.contains("Yamato"), "its neighbour stayed closed");
            })
            .expect("the test window stays open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("ship-toggle-Shima", cx);
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
        let games = [game("Yamato", "2026-02-13 14:00:00", 100_000, 2, true)];

        window
            .update(cx, |panel, _window, cx| {
                let refs: Vec<&PerGameStat> = games.iter().collect();
                panel.set_games(&refs, cx);

                let section = panel.sections.first().expect("the ship is listed");
                let markdown = section.markdown();
                assert!(markdown.starts_with("**Yamato 1W/0L (100%)**"), "the copy carries the header line");
                assert!(markdown.contains("| Damage | 100,000 | 100,000 | 100,000 | **100,000** |"));

                let csv = section.csv();
                assert!(csv.starts_with(",Min,Max,Total,Average"));
                assert!(csv.contains("Spotting Damage,10,000,10,000,10,000,10,000"));
            })
            .expect("the test window stays open");
    }
}
