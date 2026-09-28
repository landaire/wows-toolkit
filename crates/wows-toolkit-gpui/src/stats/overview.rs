//! The Stats tab's Overview panel: the session's record and best games on one
//! line, then a per-ship table and the achievements earned.
//!
//! Mirrors the egui app's `build_stats_overview`, which lays the summary out
//! as a wrapping row of separated facts above the same detail.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use wowsunpack::vfs::VfsPath;

use crate::replay_inspector::icons::IconCache;
use gpui_kit::SvgRenderer;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use std::collections::HashMap;
use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::personal_rating;

use std::sync::Arc;

use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingResult;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::PerformanceInfo;
use wows_toolkit_viewmodel::stats::SerializableAchievement;
use wows_toolkit_viewmodel::stats::SessionSummary;
use wows_toolkit_viewmodel::stats::aggregate_achievements;
use wows_toolkit_viewmodel::stats::per_ship_performance;
use wows_toolkit_viewmodel::stats::session_personal_rating;

const ROW_HEIGHT: Pixels = px(24.);
const LIST_OVERDRAW: Pixels = px(200.);
const SHIP_COLUMN_WIDTH: Pixels = px(200.);
const NUMBER_COLUMN_WIDTH: Pixels = px(96.);
/// One achievement's column. The icon is the egui roundup's size; the column
/// is wider than the icon so a two-word name wraps rather than being cut.
const ACHIEVEMENT_COLUMN_WIDTH: Pixels = px(84.);
const ACHIEVEMENT_ICON_SIZE: Pixels = px(48.);

/// What the panel shows, recomputed when the games or the filters change.
#[derive(Default)]
struct Computed {
    summary: SessionSummary,
    ships: Vec<(String, PerformanceInfo)>,
    achievements: Vec<SerializableAchievement>,
    /// Absent without an expected-values table, or when no game rated.
    personal_rating: Option<PersonalRatingResult>,
}

pub struct StatsOverviewPanel {
    /// Achievement art from the installed build, keyed as `IconCache` keys
    /// them. Absent until the tab is handed game data, which is when the
    /// generic glyph gives way to the real icon.
    icons: IconCache,
    /// The name each ship in the session goes by, for the records that name
    /// only an id.
    ship_names: HashMap<GameParamId, String>,
    computed: Computed,
    personal_rating: Option<Arc<PersonalRatingData>>,
    list_state: ListState,
    scroll: ScrollHandle,
    focus_handle: FocusHandle,
}

impl EventEmitter<PanelEvent> for StatsOverviewPanel {}

impl StatsOverviewPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            icons: IconCache::new(),
            ship_names: HashMap::new(),
            computed: Computed::default(),
            personal_rating: None,
            list_state: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            scroll: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Loads the art for the achievements on screen from `vfs`.
    ///
    /// Driven by the tab, which gets the build from the replay inspector
    /// rather than opening a second one.
    pub fn set_game_data(&mut self, vfs: &VfsPath, svg: &SvgRenderer, cx: &mut Context<Self>) {
        self.icons.populate_achievements(&self.computed.achievements, vfs, svg);
        cx.notify();
    }

    /// Adopts the games the filter bar has already selected. Filtering happens
    /// once in the tab rather than per panel, so every panel sees the same set.
    pub fn set_personal_rating(&mut self, table: Option<Arc<PersonalRatingData>>, cx: &mut Context<Self>) {
        self.personal_rating = table;
        cx.notify();
    }

    pub fn set_games(&mut self, games: &[&PerGameStat], cx: &mut Context<Self>) {
        // The records name the ship that set them, which needs a name for the
        // id `SessionSummary` carries; the games themselves have one.
        self.ship_names = games.iter().map(|game| (game.ship_id, game.ship_name.clone())).collect();
        self.computed = Computed {
            summary: SessionSummary::from_games(games),
            ships: per_ship_performance(games),
            achievements: aggregate_achievements(games),
            personal_rating: session_personal_rating(games, self.personal_rating.as_deref()),
        };
        self.list_state.reset(self.computed.ships.len());
        cx.notify();
    }
}

impl StatsOverviewPanel {
    /// The name of the ship an id names, or the id itself when the session
    /// holds no game in it -- which cannot happen for a record set in one.
    fn ship_name(&self, ship: GameParamId) -> String {
        self.ship_names.get(&ship).cloned().unwrap_or_else(|| ship.raw().to_string())
    }
}

impl Focusable for StatsOverviewPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for StatsOverviewPanel {
    fn panel_name(&self) -> &'static str {
        "StatsOverviewPanel"
    }

    /// The egui tab refuses to close its Overview, so this one does too.
    fn closable(&self, _cx: &App) -> bool {
        false
    }
}

impl Panel for StatsOverviewPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(t!("ui.stats.overview").into_owned())
    }
}

impl Render for StatsOverviewPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let dim = crate::theme::text_dim();
        let summary = &self.computed.summary;

        if summary.games_played() == 0 && self.computed.ships.is_empty() {
            return v_flex()
                .id("stats-overview")
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().text_color(crate::theme::text_dim()).child(t!("ui.stats.no_games").to_string()))
                .into_any_element();
        }

        let record = match summary.win_rate() {
            Some(rate) => format!("{} ({rate:.1}%)", summary.record_label()),
            None => summary.record_label(),
        };

        let summary_row = h_flex()
            .flex_none()
            .flex_wrap()
            .gap_3()
            .items_center()
            .px_2()
            .py_2()
            .border_b_1()
            .border_color(border)
            .child(div().text_sm().font_weight(FontWeight::BOLD).child(record))
            .when_some(self.computed.personal_rating.as_ref(), |this, rating| {
                // The band is the text's own colour over the panel, which is
                // what `chip_text` is solved for; filling behind it with
                // `chip_hue` would put one tone of a band on another.
                let dark = crate::theme::is_dark_mode();
                this.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(personal_rating::chip_text(rating.category, dark)))
                        .child(format!("{} {:.0} ({})", t!("ui.stats.pr"), rating.pr, rating.category.name())),
                )
            })
            .child(
                div()
                    .text_sm()
                    .text_color(crate::theme::text_dim())
                    .child(t!("ui.stats.frags", count = summary.total_frags).to_string()),
            )
            .when_some(summary.best_frags, |this, (ship, frags)| {
                let ship = self.ship_name(ship);
                this.child(
                    div()
                        .text_sm()
                        .text_color(dim)
                        .child(t!("ui.stats.best_frags", ship = ship, count = frags).to_string()),
                )
            })
            .when_some(summary.best_damage, |this, (ship, damage)| {
                let ship = self.ship_name(ship);
                this.child(
                    div()
                        .text_sm()
                        .text_color(dim)
                        .child(t!("ui.stats.max_damage", ship = ship, damage = separate_thousands(damage)).to_string()),
                )
            });

        let header = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .w(SHIP_COLUMN_WIDTH)
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .child(t!("ui.stats.column_ship").to_string()),
            )
            .child(
                div()
                    .w(NUMBER_COLUMN_WIDTH)
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .child(t!("ui.stats.column_battles").to_string()),
            )
            .child(
                div()
                    .w(NUMBER_COLUMN_WIDTH)
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .child(t!("ui.stats.column_win_rate").to_string()),
            )
            .child(
                div()
                    .w(NUMBER_COLUMN_WIDTH)
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .child(t!("ui.stats.column_avg_damage").to_string()),
            )
            .child(
                div()
                    .w(NUMBER_COLUMN_WIDTH)
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .child(t!("ui.stats.column_avg_frags").to_string()),
            );

        let ships: Vec<(String, PerformanceInfo)> = self.computed.ships.clone();
        let render_row = move |ix: usize, _window: &mut Window, cx: &mut App| {
            let Some((ship, info)) = ships.get(ix) else {
                return div().into_any_element();
            };
            h_flex()
                .id(ix)
                .w_full()
                .h(ROW_HEIGHT)
                .gap_2()
                .items_center()
                .px_2()
                .when_some(crate::ui::stripe(ix, cx), |row, color| row.bg(color))
                .child(div().w(SHIP_COLUMN_WIDTH).text_sm().child(ship.clone()))
                .child(div().w(NUMBER_COLUMN_WIDTH).text_sm().child(info.total_games().to_string()))
                .child(div().w(NUMBER_COLUMN_WIDTH).text_sm().child(optional_percent(info.win_rate())))
                .child(div().w(NUMBER_COLUMN_WIDTH).text_sm().child(optional_number(info.avg_damage())))
                .child(div().w(NUMBER_COLUMN_WIDTH).text_sm().child(optional_decimal(info.avg_frags())))
                .into_any_element()
        };

        let table = div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .child(list(self.list_state.clone(), render_row).size_full())
            .child(Scrollbar::vertical(&self.list_state));

        let achievements = (!self.computed.achievements.is_empty()).then(|| {
            v_flex()
                .flex_none()
                .max_h(px(190.))
                .gap_1()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(border)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .child(t!("ui.replay.sections.achievements").to_string()),
                )
                .child(div().id("stats-achievements").overflow_y_scroll().track_scroll(&self.scroll).child(
                    h_flex().flex_wrap().gap_3().children(self.computed.achievements.iter().enumerate().map(
                        |(ix, earned)| {
                            let art = self.icons.get_keyed(&format!("achievement:{}", earned.icon_key));
                            let hover = if earned.description.is_empty() {
                                earned.display_name.clone()
                            } else {
                                format!("{}: {}", earned.display_name, earned.description)
                            };
                            v_flex()
                                .id(("stats-achievement", ix))
                                .w(ACHIEVEMENT_COLUMN_WIDTH)
                                .items_center()
                                .gap_0()
                                .tooltip(move |window, cx| Tooltip::new(hover.clone()).build(window, cx))
                                .child(match art {
                                    Some(image) => {
                                        img(image).w(ACHIEVEMENT_ICON_SIZE).h(ACHIEVEMENT_ICON_SIZE).into_any_element()
                                    }
                                    None => Icon::new(IconName::Star).into_any_element(),
                                })
                                .child(
                                    div().text_xs().font_weight(FontWeight::BOLD).child(format!("x{}", earned.count)),
                                )
                                // Wrapped, not truncated: the name is what
                                // tells two achievements apart, and half of
                                // one tells the reader nothing.
                                .child(
                                    div()
                                        .text_xs()
                                        .text_center()
                                        .text_color(crate::theme::text_dim())
                                        .child(earned.display_name.clone()),
                                )
                        },
                    )),
                ))
        });

        v_flex()
            .id("stats-overview")
            .size_full()
            .child(summary_row)
            .child(header)
            .child(table)
            .when_some(achievements, |this, section| this.child(section))
            .into_any_element()
    }
}

/// Thousands-separated, matching the egui app's number formatting.
fn separate_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A dash for an absent value, so an empty cell is never read as a zero.
fn optional_percent(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.1}%")).unwrap_or_else(|| "-".to_string())
}

fn optional_number(value: Option<f64>) -> String {
    value.map(|value| separate_thousands(value.round() as u64)).unwrap_or_else(|| "-".to_string())
}

fn optional_decimal(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.2}")).unwrap_or_else(|| "-".to_string())
}

// The test module imports by name: `use super::*` would pull in the glob
// `gpui_kit::*` re-export of GPUI's own `test` macro, which shadows Rust's
// `#[test]` and sends the attribute into infinite expansion.
#[cfg(test)]
mod tests {
    use super::optional_decimal;
    use super::optional_number;
    use super::optional_percent;
    use super::separate_thousands;

    #[test]
    fn thousands_are_separated_from_the_right() {
        assert_eq!(separate_thousands(0), "0");
        assert_eq!(separate_thousands(999), "999");
        assert_eq!(separate_thousands(1_000), "1,000");
        assert_eq!(separate_thousands(12_345), "12,345");
        assert_eq!(separate_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn an_absent_value_reads_as_a_dash_rather_than_a_zero() {
        assert_eq!(optional_percent(None), "-");
        assert_eq!(optional_number(None), "-");
        assert_eq!(optional_decimal(None), "-");
        assert_eq!(optional_percent(Some(52.25)), "52.2%");
        assert_eq!(optional_number(Some(75_000.4)), "75,000");
        assert_eq!(optional_decimal(Some(1.005)), "1.00");
    }
}
