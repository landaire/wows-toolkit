//! Turning session games into chart series.
//!
//! A line chart walks the games in order; a bar chart compares per-ship
//! averages. Which statistic either plots is the same list in both front ends.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::GameParamId;

use super::PerGameStat;
use super::PerformanceInfo;
use crate::personal_rating::PersonalRatingData;
use crate::personal_rating::ShipBattleStats;

/// A statistic a chart can plot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartableStat {
    #[default]
    Damage,
    SpottingDamage,
    Frags,
    RawXp,
    BaseXp,
    WinRate,
    PersonalRating,
}

impl ChartableStat {
    /// Alphabetical, the order the egui picker lists them in.
    pub const ALL: [ChartableStat; 7] = [
        Self::BaseXp,
        Self::Damage,
        Self::Frags,
        Self::PersonalRating,
        Self::RawXp,
        Self::SpottingDamage,
        Self::WinRate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Damage => "Damage",
            Self::SpottingDamage => "Spotting damage",
            Self::Frags => "Frags",
            Self::RawXp => "Raw XP",
            Self::BaseXp => "Base XP",
            Self::WinRate => "Win rate",
            Self::PersonalRating => "Personal rating",
        }
    }

    /// Whether plotting this needs the expected-values table that personal
    /// rating is computed against. A caller without that table leaves the
    /// statistic out of its picker rather than plotting zeroes.
    pub fn requires_personal_rating(self) -> bool {
        matches!(self, Self::PersonalRating)
    }

    /// Whether a single game carries this statistic.
    ///
    /// A win rate over one game is either 0 or 100, which plots as noise, so
    /// the line chart has nothing to draw for it; the bar chart, comparing
    /// ships over many games, does.
    pub fn is_per_game(self) -> bool {
        !matches!(self, Self::WinRate | Self::PersonalRating)
    }

    /// This game's value, absent when the statistic is not a per-game one.
    pub fn of_game(self, game: &PerGameStat) -> Option<f64> {
        match self {
            Self::Damage => Some(game.damage as f64),
            Self::SpottingDamage => Some(game.spotting_damage as f64),
            Self::Frags => Some(game.frags as f64),
            Self::RawXp => Some(game.raw_xp as f64),
            Self::BaseXp => Some(game.base_xp as f64),
            Self::WinRate | Self::PersonalRating => None,
        }
    }

    /// This ship's value over the games it played, absent when the aggregate
    /// has no games to average.
    pub fn of_ship(self, info: &PerformanceInfo) -> Option<f64> {
        match self {
            Self::Damage => info.avg_damage(),
            Self::SpottingDamage => info.avg_spotting_damage(),
            Self::Frags => info.avg_frags(),
            Self::RawXp => info.avg_xp(),
            Self::BaseXp => info.avg_win_adjusted_xp(),
            Self::WinRate => info.win_rate(),
            Self::PersonalRating => None,
        }
    }
}

/// How a chart draws its series.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartMode {
    /// The statistic per game, in the order they were played.
    #[default]
    Line,
    /// The statistic averaged per ship.
    Bar,
}

impl ChartMode {
    pub const ALL: [ChartMode; 2] = [Self::Line, Self::Bar];

    pub fn label(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Bar => "Bar",
        }
    }
}

/// One plotted value with the label its axis shows.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesPoint {
    pub label: String,
    pub value: f64,
}

/// The statistic per game, oldest first.
///
/// Games the statistic does not apply to contribute no point rather than a
/// zero, so a gap in the data does not read as a run of bad games.
pub fn line_series(games: &[&PerGameStat], stat: ChartableStat) -> Vec<SeriesPoint> {
    games
        .iter()
        .filter_map(|game| stat.of_game(game).map(|value| SeriesPoint { label: game.game_time.clone(), value }))
        .collect()
}

/// Smooths a series over a trailing window.
///
/// A window of one, or wider than the series, returns it unchanged; the
/// rolling average is a reading aid, not a filter that can empty the chart.
pub fn rolling_average(points: &[SeriesPoint], window: usize) -> Vec<SeriesPoint> {
    let window = window.max(1);
    if window <= 1 {
        return points.to_vec();
    }

    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let start = (index + 1).saturating_sub(window);
            let span = &points[start..=index];
            let mean = span.iter().map(|point| point.value).sum::<f64>() / span.len() as f64;
            SeriesPoint { label: point.label.clone(), value: mean }
        })
        .collect()
}

/// The statistic per ship, highest first, so the tallest bar leads.
pub fn bar_series(ships: &[(String, PerformanceInfo)], stat: ChartableStat) -> Vec<SeriesPoint> {
    let mut points: Vec<SeriesPoint> = ships
        .iter()
        .filter_map(|(ship, info)| stat.of_ship(info).map(|value| SeriesPoint { label: ship.clone(), value }))
        .collect();
    points.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
    points
}

/// A colour on a chart, as the eight bits per channel both front ends take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// The colour drawn for one ship.
///
/// Hue from the identifier's hash, at a fixed saturation and value, so a ship
/// keeps the same colour across sessions and across the two front ends.
pub fn ship_color(id: GameParamId) -> Rgb {
    use std::hash::Hash;
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    let hue = (hasher.finish() % 360) as f32;

    let (saturation, value) = (0.7f32, 0.9f32);
    let c = value * saturation;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = value - c;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Rgb { r: ((r + m) * 255.0) as u8, g: ((g + m) * 255.0) as u8, b: ((b + m) * 255.0) as u8 }
}

/// The colour of the one line drawn when the ships are combined.
pub const COMBINED_COLOR: Rgb = Rgb { r: 100, g: 180, b: 255 };

/// The name shown for the combined line.
pub const COMBINED_LABEL: &str = "Combined";

/// One plotted line: what the legend calls it, the colour it is drawn in, and
/// its points in play order.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    pub name: String,
    pub color: Rgb,
    pub points: Vec<SeriesPoint>,
}

/// The ships the games were played in, in the order first played.
///
/// The chart's ship filter lists these, and a filter that has never been
/// touched selects all of them.
pub fn ships_played(games: &[&PerGameStat]) -> Vec<(GameParamId, String)> {
    let mut seen: Vec<(GameParamId, String)> = Vec::new();
    for game in games {
        if !seen.iter().any(|(id, _)| *id == game.ship_id) {
            seen.push((game.ship_id, game.ship_name.clone()));
        }
    }
    seen
}

/// The lines a line chart draws.
///
/// `combined` collapses every selected ship into one line; otherwise each
/// ship is its own line, in its own colour. `running` replaces each point
/// with the average of everything up to it, which is what makes win rate and
/// personal rating plottable at all: both are meaningless over a single game
/// (a win is 0 or 100), and both are accumulated rather than averaged.
pub fn line_chart_series(
    games: &[&PerGameStat],
    stat: ChartableStat,
    selected: &[GameParamId],
    rating: Option<&PersonalRatingData>,
    running: bool,
    combined: bool,
) -> Vec<ChartSeries> {
    let chosen: Vec<&PerGameStat> = games.iter().copied().filter(|game| selected.contains(&game.ship_id)).collect();
    if chosen.is_empty() {
        return Vec::new();
    }

    if combined {
        let points = accumulated_points(&chosen, stat, rating);
        if points.is_empty() {
            return Vec::new();
        }
        return vec![ChartSeries { name: COMBINED_LABEL.to_string(), color: COMBINED_COLOR, points }];
    }

    ships_played(&chosen)
        .into_iter()
        .filter_map(|(ship_id, name)| {
            let ship_games: Vec<&PerGameStat> = chosen.iter().copied().filter(|game| game.ship_id == ship_id).collect();
            let points =
                if running { accumulated_points(&ship_games, stat, rating) } else { line_series(&ship_games, stat) };
            (!points.is_empty()).then(|| ChartSeries { name, color: ship_color(ship_id), points })
        })
        .collect()
}

/// Each game's value averaged over every game up to it.
///
/// Win rate counts the wins so far over the games so far, and personal rating
/// is recomputed from everything played so far rather than averaged, since a
/// rating is not the mean of the ratings it is made of.
fn accumulated_points(
    games: &[&PerGameStat],
    stat: ChartableStat,
    rating: Option<&PersonalRatingData>,
) -> Vec<SeriesPoint> {
    match stat {
        ChartableStat::WinRate => {
            let mut wins = 0u64;
            games
                .iter()
                .enumerate()
                .map(|(index, game)| {
                    wins += u64::from(game.is_win);
                    SeriesPoint { label: game.game_time.clone(), value: wins as f64 / (index + 1) as f64 * 100.0 }
                })
                .collect()
        }
        ChartableStat::PersonalRating => {
            let Some(table) = rating else { return Vec::new() };
            let mut played: HashMap<GameParamId, ShipBattleStats> = HashMap::new();
            games
                .iter()
                .filter_map(|game| {
                    let entry = played.entry(game.ship_id).or_insert(ShipBattleStats {
                        ship_id: game.ship_id,
                        battles: 0,
                        damage: 0,
                        wins: 0,
                        frags: 0,
                    });
                    entry.battles += 1;
                    entry.damage += game.damage;
                    entry.wins += u32::from(game.is_win);
                    entry.frags += game.frags;

                    let stats: Vec<ShipBattleStats> = played.values().cloned().collect();
                    let value = table.calculate_pr(&stats)?.pr;
                    Some(SeriesPoint { label: game.game_time.clone(), value })
                })
                .collect()
        }
        _ => {
            let mut sum = 0.0;
            games
                .iter()
                .enumerate()
                .filter_map(|(index, game)| {
                    sum += stat.of_game(game)?;
                    Some(SeriesPoint { label: game.game_time.clone(), value: sum / (index + 1) as f64 })
                })
                .collect()
        }
    }
}

/// One bar: the ship it is drawn for, in that ship's colour.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartBar {
    pub label: String,
    pub value: f64,
    pub color: Rgb,
}

/// The bars a bar chart draws, for the selected ships only.
///
/// In the order `ships` are given, which is the order the ships table lists
/// them in, so a ship sits in the same place on both.
pub fn bar_chart_series(
    ships: &[(String, PerformanceInfo)],
    stat: ChartableStat,
    selected: &[GameParamId],
    rating: Option<&PersonalRatingData>,
) -> Vec<ChartBar> {
    ships
        .iter()
        .filter(|(_, info)| info.ship_id().is_none_or(|id| selected.contains(&id)))
        .filter_map(|(ship, info)| {
            // A ship's rating is computed from its whole record, not averaged
            // out of its games, so it is not one of `of_ship`'s aggregates.
            let value = match stat {
                ChartableStat::PersonalRating => info.personal_rating(rating?).map(|result| result.pr)?,
                _ => stat.of_ship(info)?,
            };
            let color = info.ship_id().map_or(COMBINED_COLOR, ship_color);
            Some(ChartBar { label: ship.clone(), value, color })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::PerGameStat;
    use super::super::per_ship_performance;
    use super::ChartMode;
    use super::ChartableStat;
    use super::SeriesPoint;
    use super::bar_series;
    use super::line_series;
    use super::rolling_average;

    fn game(ship: &str, time: &str, damage: u64, frags: i64, win: bool) -> PerGameStat {
        PerGameStat {
            ship_name: ship.to_string(),
            ship_id: 1u64.into(),
            game_time: time.to_string(),
            sort_key: time.to_string(),
            player_id: 1,
            damage,
            spotting_damage: damage / 10,
            frags,
            raw_xp: damage as i64 / 100,
            base_xp: damage as i64 / 200,
            is_win: win,
            is_loss: !win,
            is_draw: false,
            is_div: false,
            match_group: "pvp".to_string(),
            achievements: Vec::new(),
        }
    }

    #[test]
    fn a_line_series_follows_the_games_in_order() {
        let games = [game("Yamato", "t1", 10_000, 1, true), game("Yamato", "t2", 30_000, 0, false)];
        let refs: Vec<&PerGameStat> = games.iter().collect();

        let points = line_series(&refs, ChartableStat::Damage);

        assert_eq!(points.len(), 2);
        assert_eq!(points[0], SeriesPoint { label: "t1".into(), value: 10_000.0 });
        assert_eq!(points[1].value, 30_000.0);
    }

    #[test]
    fn a_statistic_with_no_per_game_meaning_plots_no_line_points() {
        let games = [game("Yamato", "t1", 10_000, 1, true)];
        let refs: Vec<&PerGameStat> = games.iter().collect();

        assert!(line_series(&refs, ChartableStat::WinRate).is_empty());
        assert!(line_series(&refs, ChartableStat::PersonalRating).is_empty());
        assert!(!ChartableStat::WinRate.is_per_game());
        assert!(ChartableStat::Damage.is_per_game());
    }

    #[test]
    fn a_bar_series_averages_each_ship_and_leads_with_the_tallest() {
        let games = [
            game("Yamato", "t1", 10_000, 1, true),
            game("Yamato", "t2", 20_000, 1, true),
            game("Shimakaze", "t3", 50_000, 2, false),
        ];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = per_ship_performance(&refs);

        let points = bar_series(&ships, ChartableStat::Damage);

        assert_eq!(points.len(), 2);
        assert_eq!(points[0].label, "Shimakaze", "50k leads 15k");
        assert_eq!(points[0].value, 50_000.0);
        assert_eq!(points[1].value, 15_000.0, "Yamato averages its two games");
    }

    #[test]
    fn a_bar_series_can_plot_win_rate_even_though_a_line_cannot() {
        let games = [game("Yamato", "t1", 1, 0, true), game("Yamato", "t2", 1, 0, false)];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = per_ship_performance(&refs);

        let points = bar_series(&ships, ChartableStat::WinRate);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].value, 50.0);
    }

    #[test]
    fn a_rolling_average_smooths_over_a_trailing_window() {
        let points = vec![
            SeriesPoint { label: "t1".into(), value: 0.0 },
            SeriesPoint { label: "t2".into(), value: 10.0 },
            SeriesPoint { label: "t3".into(), value: 20.0 },
        ];

        let smoothed = rolling_average(&points, 2);

        assert_eq!(smoothed[0].value, 0.0, "the first point has only itself");
        assert_eq!(smoothed[1].value, 5.0);
        assert_eq!(smoothed[2].value, 15.0);
        assert_eq!(smoothed[2].label, "t3", "labels are preserved");
    }

    #[test]
    fn a_rolling_window_of_one_or_wider_than_the_series_changes_nothing_structural() {
        let points =
            vec![SeriesPoint { label: "t1".into(), value: 4.0 }, SeriesPoint { label: "t2".into(), value: 8.0 }];

        assert_eq!(rolling_average(&points, 1), points);
        assert_eq!(rolling_average(&points, 0), points, "a zero window is treated as one");
        assert_eq!(rolling_average(&points, 99).len(), 2, "a wide window still yields every point");
    }

    #[test]
    fn personal_rating_is_marked_as_needing_data_the_series_builders_do_not_hold() {
        assert!(ChartableStat::PersonalRating.requires_personal_rating());
        assert!(!ChartableStat::Damage.requires_personal_rating());
    }

    #[test]
    fn a_line_is_drawn_per_ship_in_that_ship_s_own_colour() {
        let mut yamato = game("Yamato", "t1", 10_000, 1, true);
        yamato.ship_id = 7u64.into();
        let mut shima = game("Shimakaze", "t2", 50_000, 2, false);
        shima.ship_id = 9u64.into();
        let games = [yamato, shima];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let selected: Vec<_> = super::ships_played(&refs).into_iter().map(|(id, _)| id).collect();

        let series = super::line_chart_series(&refs, ChartableStat::Damage, &selected, None, false, false);

        assert_eq!(series.len(), 2, "one line per ship");
        assert_eq!(series[0].name, "Yamato", "in the order first played");
        assert_ne!(series[0].color, series[1].color, "each ship is told apart by its colour");
        assert_eq!(series[0].color, super::ship_color(7u64.into()), "and keeps that colour");
    }

    #[test]
    fn an_unselected_ship_is_not_drawn() {
        let mut yamato = game("Yamato", "t1", 10_000, 1, true);
        yamato.ship_id = 7u64.into();
        let mut shima = game("Shimakaze", "t2", 50_000, 2, false);
        shima.ship_id = 9u64.into();
        let games = [yamato, shima];
        let refs: Vec<&PerGameStat> = games.iter().collect();

        let series = super::line_chart_series(&refs, ChartableStat::Damage, &[7u64.into()], None, false, false);

        assert_eq!(series.len(), 1);
        assert_eq!(series[0].name, "Yamato");
    }

    #[test]
    fn combining_the_ships_draws_one_running_line() {
        let mut first = game("Yamato", "t1", 10_000, 1, true);
        first.ship_id = 7u64.into();
        let mut second = game("Shimakaze", "t2", 30_000, 2, false);
        second.ship_id = 9u64.into();
        let games = [first, second];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let selected = [7u64.into(), 9u64.into()];

        let series = super::line_chart_series(&refs, ChartableStat::Damage, &selected, None, false, true);

        assert_eq!(series.len(), 1);
        assert_eq!(series[0].name, super::COMBINED_LABEL);
        assert_eq!(series[0].points[0].value, 10_000.0);
        assert_eq!(series[0].points[1].value, 20_000.0, "the average of both games, not the second's own figure");
    }

    /// A win is 0 or 100 over one game, so the line plots the rate so far.
    #[test]
    fn a_running_win_rate_counts_the_wins_so_far() {
        let games = [game("Yamato", "t1", 1, 0, true), game("Yamato", "t2", 1, 0, false)];
        let refs: Vec<&PerGameStat> = games.iter().collect();

        let series = super::line_chart_series(&refs, ChartableStat::WinRate, &[1u64.into()], None, true, false);

        assert_eq!(series.len(), 1);
        assert_eq!(series[0].points[0].value, 100.0);
        assert_eq!(series[0].points[1].value, 50.0);
    }

    #[test]
    fn a_ship_keeps_one_colour_and_two_ships_rarely_share_it() {
        assert_eq!(super::ship_color(3u64.into()), super::ship_color(3u64.into()));
        assert_ne!(super::ship_color(3u64.into()), super::ship_color(4u64.into()));
    }

    #[test]
    fn a_bar_is_drawn_per_selected_ship_in_the_order_the_ships_table_lists_them() {
        let mut first = game("Yamato", "t1", 10_000, 1, true);
        first.ship_id = 7u64.into();
        let mut second = game("Shimakaze", "t2", 50_000, 2, false);
        second.ship_id = 9u64.into();
        let games = [first, second];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = per_ship_performance(&refs);

        let bars = super::bar_chart_series(&ships, ChartableStat::Damage, &[7u64.into(), 9u64.into()], None);
        assert_eq!(bars.len(), 2);
        let names: Vec<&str> = bars.iter().map(|bar| bar.label.as_str()).collect();
        let listed: Vec<&str> = ships.iter().map(|(ship, _)| ship.as_str()).collect();
        assert_eq!(names, listed);
        assert_eq!(
            bars.iter().find(|bar| bar.label == "Shimakaze").expect("a bar").color,
            super::ship_color(9u64.into())
        );

        let bars = super::bar_chart_series(&ships, ChartableStat::Damage, &[7u64.into()], None);
        assert_eq!(bars.len(), 1, "an unselected ship has no bar");
        assert_eq!(bars[0].label, "Yamato");
    }

    #[test]
    fn every_stat_and_mode_is_labelled() {
        assert_eq!(ChartableStat::ALL.len(), 7);
        assert!(ChartableStat::ALL.iter().all(|stat| !stat.label().is_empty()));
        assert_eq!(ChartMode::ALL.len(), 2);
        assert!(ChartMode::ALL.iter().all(|mode| !mode.label().is_empty()));
    }
}
