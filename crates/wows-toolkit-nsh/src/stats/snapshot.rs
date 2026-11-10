//! What the Stats tab's TSX view draws, computed from a scope's games.
//!
//! Every figure is formatted here, in the reader's locale, so the view only
//! lays text out.

use std::collections::BTreeMap;

use nsh_core::prelude::*;
use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingResult;
use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::GameLimit;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::PerformanceInfo;
use wows_toolkit_viewmodel::stats::PrStats;
use wows_toolkit_viewmodel::stats::SessionSummary;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::aggregate_achievements;
use wows_toolkit_viewmodel::stats::all_match_groups;
use wows_toolkit_viewmodel::stats::filter_games;
use wows_toolkit_viewmodel::stats::filter_games_per_ship;
use wows_toolkit_viewmodel::stats::match_group_display_name;
use wows_toolkit_viewmodel::stats::per_ship_performance_by_id;
use wows_toolkit_viewmodel::stats::session_personal_rating;
use wows_toolkit_viewmodel::stats::table as stats_table;

use crate::text::text;
use crate::text::text_with;

/// The division filter's choices, as the view names them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum DivisionChoice {
    #[default]
    All,
    Solo,
    Division,
}

impl DivisionChoice {
    pub(crate) fn from_filter(filter: DivisionFilter) -> Self {
        match filter {
            DivisionFilter::All => Self::All,
            DivisionFilter::SoloOnly => Self::Solo,
            DivisionFilter::DivOnly => Self::Division,
        }
    }

    pub(crate) fn filter(self) -> DivisionFilter {
        match self {
            Self::All => DivisionFilter::All,
            Self::Solo => DivisionFilter::SoloOnly,
            Self::Division => DivisionFilter::DivOnly,
        }
    }
}

/// The filter bar's state.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct FilterState {
    /// Whether only the most recent games count.
    pub limit_enabled: bool,
    /// The recent-game count, kept while the limit is off so turning it back
    /// on restores it.
    pub limit_count: u32,
    pub division: DivisionChoice,
    /// Match groups kept; empty keeps every group.
    pub game_modes: Vec<String>,
}

/// A match group the session holds, with its display name.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ModeOption {
    pub id: String,
    pub label: String,
    pub selected: bool,
}

/// A personal rating with the colours of its band.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct RatingChip {
    /// The rating, rounded.
    pub text: String,
    /// The band's name.
    pub category: String,
    /// The band's hue as `#rrggbb`; the view lays it under the text faintly.
    pub hue: String,
    /// The text colour over the tint on a dark theme.
    pub dark_text: String,
    /// The text colour over the tint on a light theme.
    pub light_text: String,
}

/// One row of the Overview's per-ship table.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct OverviewShip {
    pub ship_id: String,
    pub name: String,
    pub battles: String,
    pub win_rate: String,
    pub average_damage: String,
    pub average_frags: String,
}

/// One achievement earned across the games in view.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct AchievementTile {
    pub id: String,
    pub name: String,
    pub description: String,
    /// `x3`, as the GPUI tile reads.
    pub count: String,
}

/// The Overview panel.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct OverviewSnapshot {
    pub has_games: bool,
    /// `3W/1L (75.0%)`.
    pub record: String,
    pub rating: Option<RatingChip>,
    pub frags: String,
    pub best_frags: Option<String>,
    pub max_damage: Option<String>,
    pub ships: Vec<OverviewShip>,
    pub achievements: Vec<AchievementTile>,
}

/// One figure of a ship's table.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct StatCell {
    pub text: String,
    pub rating: Option<RatingChip>,
    pub emphasized: bool,
}

/// One statistic of a ship's table.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct StatRow {
    pub label: String,
    pub cells: Vec<StatCell>,
}

/// One ship's section of the Ships panel.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ShipSection {
    pub ship_id: String,
    pub name: String,
    /// The ship's name folded for search.
    pub search_key: String,
    /// The collapsed line, which is also what a screen reader hears.
    pub header: String,
    pub wins: String,
    pub losses: String,
    pub draws: Option<String>,
    pub win_rate: Option<String>,
    pub rating: Option<RatingChip>,
    pub rows: Vec<StatRow>,
    pub markdown: String,
    pub csv: String,
    pub clear_label: String,
}

/// Everything the tab shows outside its charts.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct StatsSnapshot {
    /// Games in the scope before any filter, which is what a clear would
    /// forget.
    pub game_count: u32,
    pub filters: FilterState,
    pub modes: Vec<ModeOption>,
    /// The mode button's label: the count of chosen modes, if any.
    pub mode_label: String,
    /// The chosen modes by name, or "All".
    pub mode_summary: String,
    pub overview: OverviewSnapshot,
    pub columns: Vec<String>,
    pub ships: Vec<ShipSection>,
}

/// The filter bar's state for `filters`, with `last_count` standing for the
/// count while the limit is off.
pub(crate) fn filter_state(filters: &StatsFilters, last_count: usize) -> FilterState {
    let (limit_enabled, count) = match filters.limit {
        GameLimit::All => (false, last_count),
        GameLimit::Recent(count) => (true, count),
    };
    FilterState {
        limit_enabled,
        limit_count: u32::try_from(count).unwrap_or(u32::MAX),
        division: DivisionChoice::from_filter(filters.division),
        game_modes: filters.game_modes.iter().cloned().collect(),
    }
}

/// The snapshot of `games` under `filters`.
pub(crate) fn snapshot(
    games: &[PerGameStat],
    filters: &StatsFilters,
    last_count: usize,
    table: Option<&PersonalRatingData>,
) -> StatsSnapshot {
    let locale = wows_toolkit_viewmodel::locale();
    let modes: Vec<ModeOption> = all_match_groups(games)
        .into_iter()
        .map(|mode| ModeOption {
            label: match_group_display_name(&mode).to_owned(),
            selected: filters.game_modes.contains(&mode),
            id: mode,
        })
        .collect();
    let chosen: Vec<&str> = modes.iter().filter(|mode| mode.selected).map(|mode| mode.label.as_str()).collect();
    let mode_label = if chosen.is_empty() {
        text("ui.stats.mode_label")
    } else {
        format!("{} ({})", text("ui.stats.mode_label"), chosen.len())
    };
    let mode_summary = if chosen.is_empty() { text("ui.stats.div_all") } else { chosen.join(", ") };

    // The summary counts the session's own last N; the per-ship table counts
    // N per ship, which is the split the egui tab makes between its two
    // reads (`ui/stats_tab.rs:327` against `:406`).
    let filtered = filter_games(games, filters);
    let per_ship = filter_games_per_ship(games, filters);

    StatsSnapshot {
        game_count: u32::try_from(games.len()).unwrap_or(u32::MAX),
        filters: filter_state(filters, last_count),
        modes,
        mode_label,
        mode_summary,
        overview: overview(&filtered, table, &locale),
        columns: stats_table::COLUMNS.iter().map(|column| text(column.translation_key())).collect(),
        ships: ship_sections(&per_ship, table, &locale),
    }
}

fn overview(games: &[&PerGameStat], table: Option<&PersonalRatingData>, locale: &str) -> OverviewSnapshot {
    let summary = SessionSummary::from_games(games);
    let ships = per_ship_performance_by_id(games);
    let ship_name = |ship: GameParamId| {
        games.iter().find(|game| game.ship_id == ship).map_or_else(|| ship.raw().to_string(), |game| game.ship_name.clone())
    };

    let record = match summary.win_rate() {
        Some(rate) => format!("{} ({rate:.1}%)", summary.record_label()),
        None => summary.record_label(),
    };
    let mut achievements = aggregate_achievements(games);
    achievements.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.display_name.cmp(&b.display_name)));

    OverviewSnapshot {
        has_games: summary.games_played() > 0 || !ships.is_empty(),
        record,
        rating: session_personal_rating(games, table).map(|rating| rating_chip(&rating)),
        frags: text_with("ui.stats.frags", &[("count", &summary.total_frags.to_string())]),
        best_frags: summary.best_frags.map(|(ship, frags)| {
            text_with("ui.stats.best_frags", &[("ship", &ship_name(ship)), ("count", &frags.to_string())])
        }),
        max_damage: summary.best_damage.map(|(ship, damage)| {
            let damage = wows_toolkit_viewmodel::formatting::separate_number(damage, Some(locale));
            text_with("ui.stats.max_damage", &[("ship", &ship_name(ship)), ("damage", &damage)])
        }),
        ships: ships
            .iter()
            .map(|(name, info)| OverviewShip {
                ship_id: info.ship_id().map_or_else(String::new, |id| id.raw().to_string()),
                name: name.clone(),
                battles: info.total_games().to_string(),
                win_rate: info.win_rate().map_or_else(|| "-".to_owned(), |rate| format!("{rate:.1}%")),
                average_damage: info.avg_damage().map_or_else(
                    || "-".to_owned(),
                    |damage| wows_toolkit_viewmodel::formatting::separate_number(damage.round() as u64, Some(locale)),
                ),
                average_frags: info.avg_frags().map_or_else(|| "-".to_owned(), |frags| format!("{frags:.2}")),
            })
            .collect(),
        achievements: achievements
            .into_iter()
            .map(|achievement| AchievementTile {
                id: achievement.game_param_id.raw().to_string(),
                name: achievement.display_name,
                description: achievement.description,
                count: format!("x{}", achievement.count),
            })
            .collect(),
    }
}

fn ship_sections(games: &[&PerGameStat], table: Option<&PersonalRatingData>, locale: &str) -> Vec<ShipSection> {
    // Grouped by id, not by name: two ships can carry one display name, and a
    // rating is scored against a single ship's expected values.
    let mut by_ship: BTreeMap<u64, Vec<&PerGameStat>> = BTreeMap::new();
    for game in games {
        by_ship.entry(game.ship_id.raw()).or_default().push(game);
    }

    let column_heading = |column: stats_table::Column| text(column.translation_key());
    let stat_heading = |label: stats_table::StatLabel| text(label.translation_key());
    let mut sections: Vec<(String, ShipSection)> = by_ship
        .into_iter()
        .filter_map(|(ship_id, games)| {
            let ship = games.first()?.ship_name.clone();
            let info = PerformanceInfo::from_games(&games);
            let rating = table.and_then(|table| PrStats::from_games(&games, table));
            // A group exists only because a game created it, so there is
            // always a win rate to read.
            let win_rate = info.win_rate().unwrap_or_default();
            let record = if info.draws() > 0 {
                format!("{}W/{}L/{}D", info.wins(), info.losses(), info.draws())
            } else {
                format!("{}W/{}L", info.wins(), info.losses())
            };
            let header = match rating.as_ref() {
                Some(rating) => format!("{ship} {record} ({win_rate:.0}%) - PR: {:.0}", rating.average.pr),
                None => format!("{ship} {record} ({win_rate:.0}%)"),
            };
            let rows = stats_table::ship_rows(&info, rating.as_ref(), Some(locale));
            let section = ShipSection {
                ship_id: ship_id.to_string(),
                search_key: wows_toolkit_viewmodel::search::normalize_ship_search(&ship),
                markdown: stats_table::to_markdown(&header, &rows, column_heading, stat_heading),
                csv: stats_table::to_csv(&rows, column_heading, stat_heading),
                wins: format!("{}W", info.wins()),
                losses: format!("{}L", info.losses()),
                draws: (info.draws() > 0).then(|| format!("{}D", info.draws())),
                win_rate: info.win_rate().map(|rate| format!("({rate:.0}%)")),
                rating: rating.as_ref().map(|rating| rating_chip(&rating.average)),
                rows: rows
                    .iter()
                    .map(|row| StatRow {
                        label: stat_heading(row.label),
                        cells: stats_table::COLUMNS
                            .iter()
                            .map(|column| {
                                let cell = row.cell(*column);
                                StatCell {
                                    text: cell.text.clone(),
                                    rating: cell.rating.as_ref().map(rating_chip),
                                    emphasized: *column == stats_table::Column::Average,
                                }
                            })
                            .collect(),
                    })
                    .collect(),
                clear_label: text_with("ui.stats.remove_games_hint", &[("ship", &ship)]),
                header,
                name: ship,
            };
            Some((info.last_played().to_owned(), section))
        })
        .collect();

    // Most recently played first, which is the order the egui list opens in,
    // with the id breaking a tie so the order is total.
    sections.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.ship_id.cmp(&b.1.ship_id)));
    sections.into_iter().map(|(_, section)| section).collect()
}

pub(crate) fn rating_chip(rating: &PersonalRatingResult) -> RatingChip {
    RatingChip {
        text: format!("{:.0}", rating.pr),
        category: rating.category.name().to_owned(),
        hue: hex(personal_rating::chip_hue(rating.category)),
        dark_text: hex(personal_rating::chip_text(rating.category, true)),
        light_text: hex(personal_rating::chip_text(rating.category, false)),
    }
}

fn hex(color: u32) -> String {
    format!("#{color:06x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(ship: u64, name: &str, sort_key: &str, damage: u64, is_win: bool, match_group: &str) -> PerGameStat {
        PerGameStat {
            ship_name: name.to_owned(),
            ship_id: GameParamId::from(ship),
            game_time: sort_key.to_owned(),
            sort_key: sort_key.to_owned(),
            player_id: 7,
            damage,
            spotting_damage: 10,
            frags: 2,
            raw_xp: 1_500,
            base_xp: 1_000,
            is_win,
            is_loss: !is_win,
            is_draw: false,
            is_div: false,
            match_group: match_group.to_owned(),
            achievements: Vec::new(),
        }
    }

    #[test]
    fn the_snapshot_reads_as_the_gpui_overview_does() {
        wows_toolkit_viewmodel::set_locale("en");
        let games = vec![
            game(1, "Yamato", "2026-01-01", 100_000, true, "pvp"),
            game(2, "Montana", "2026-01-02", 80_000, false, "ranked"),
            game(1, "Yamato", "2026-01-03", 120_000, true, "pvp"),
        ];

        let snapshot = snapshot(&games, &StatsFilters::default(), 25, None);

        assert_eq!(snapshot.game_count, 3);
        assert_eq!(snapshot.overview.record, "2W/1L (66.7%)");
        assert_eq!(snapshot.overview.frags, "6 frags");
        assert_eq!(snapshot.overview.max_damage.as_deref(), Some("Max DMG: Yamato (120,000)"));
        assert_eq!(snapshot.ships.iter().map(|ship| ship.name.as_str()).collect::<Vec<_>>(), ["Yamato", "Montana"]);
        assert_eq!(snapshot.ships[0].header, "Yamato 2W/0L (100%)");
        assert!(snapshot.ships[0].markdown.starts_with("**Yamato 2W/0L (100%)**"));
        assert_eq!(snapshot.modes.iter().map(|mode| mode.label.as_str()).collect::<Vec<_>>(), ["Random", "Ranked"]);
        assert_eq!(snapshot.mode_summary, "All");
        assert!(!snapshot.filters.limit_enabled);
        assert_eq!(snapshot.filters.limit_count, 25);
    }

    #[test]
    fn the_limit_counts_the_session_for_the_overview_and_each_ship_for_the_table() {
        wows_toolkit_viewmodel::set_locale("en");
        let games = vec![
            game(1, "Yamato", "2026-01-01", 100_000, true, "pvp"),
            game(2, "Montana", "2026-01-02", 80_000, false, "pvp"),
            game(1, "Yamato", "2026-01-03", 120_000, true, "pvp"),
        ];
        let filters = StatsFilters { limit: GameLimit::Recent(1), ..StatsFilters::default() };

        let snapshot = snapshot(&games, &filters, 25, None);

        assert_eq!(snapshot.overview.record, "1W/0L (100.0%)");
        assert_eq!(snapshot.ships.len(), 2, "one game of each ship");
        assert!(snapshot.filters.limit_enabled);
        assert_eq!(snapshot.filters.limit_count, 1);
    }
}
