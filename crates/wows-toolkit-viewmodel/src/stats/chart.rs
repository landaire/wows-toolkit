//! Turning session games into chart series.
//!
//! A line chart walks the games in order; a bar chart compares per-ship
//! averages. Which statistic either plots is the same list in both front ends.

use serde::Deserialize;
use serde::Serialize;

use super::PerGameStat;
use super::PerformanceInfo;

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
        .filter_map(|game| {
            stat.of_game(game).map(|value| SeriesPoint { label: game.game_time.clone(), value })
        })
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
        let points = vec![SeriesPoint { label: "t1".into(), value: 4.0 }, SeriesPoint { label: "t2".into(), value: 8.0 }];

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
    fn every_stat_and_mode_is_labelled() {
        assert_eq!(ChartableStat::ALL.len(), 7);
        assert!(ChartableStat::ALL.iter().all(|stat| !stat.label().is_empty()));
        assert_eq!(ChartMode::ALL.len(), 2);
        assert!(ChartMode::ALL.iter().all(|mode| !mode.label().is_empty()));
    }
}
