//! Session statistics: the per-game rows the shared config database stores,
//! the filters the tab applies to them, and the per-ship aggregate the table
//! shows.
//!
//! Ported from the egui app's `data/session_stats.rs`. The aggregate's
//! minimums are `Option` here rather than sentinel `MAX` values reset to zero
//! after the fact, so "no games" is a state the type admits instead of a
//! number that happens to mean nothing.

pub mod table;

pub mod chart;

use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::GameParamId;

use crate::personal_rating::PersonalRatingData;
use crate::personal_rating::PersonalRatingResult;
use crate::personal_rating::ShipBattleStats;
use wows_toolkit_config::queries::SessionStatRow;

/// One achievement earned, as the session rows store it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializableAchievement {
    pub game_param_id: GameParamId,
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub count: usize,
}

/// Which games the division filter keeps.
///
/// The serde shape matches what the egui app stores under
/// `session_stats_division_filter`, so both front ends read one saved value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DivisionFilter {
    #[default]
    All,
    SoloOnly,
    DivOnly,
}

impl DivisionFilter {
    /// Every choice, in the order both filter bars list them.
    pub const ALL: [DivisionFilter; 3] = [Self::All, Self::SoloOnly, Self::DivOnly];

    /// The catalogue key this choice's name lives under.
    pub const fn translation_key(self) -> &'static str {
        match self {
            Self::All => "ui.stats.div_all",
            Self::SoloOnly => "ui.stats.div_solo",
            Self::DivOnly => "ui.stats.div_div",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::SoloOnly => "Solo",
            Self::DivOnly => "Division",
        }
    }

    fn keeps(self, game: &PerGameStat) -> bool {
        match self {
            Self::All => true,
            Self::SoloOnly => !game.is_div,
            Self::DivOnly => game.is_div,
        }
    }
}

/// The replay metadata's match-group code, as a display name.
pub fn match_group_display_name(match_group: &str) -> &str {
    match match_group {
        "pvp" => "Random",
        "ranked" => "Ranked",
        "cooperative" => "Co-op",
        "clan" => "Clan Battle",
        "brawl" => "Brawl",
        "event" => "Event",
        "pve" => "PvE",
        "" => "Unknown",
        other => other,
    }
}

/// One recorded game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerGameStat {
    pub ship_name: String,
    pub ship_id: GameParamId,
    pub game_time: String,
    /// `game_time` rewritten so lexicographic order is chronological.
    pub sort_key: String,
    pub player_id: i64,
    pub damage: u64,
    pub spotting_damage: u64,
    pub frags: i64,
    pub raw_xp: i64,
    pub base_xp: i64,
    pub is_win: bool,
    pub is_loss: bool,
    pub is_draw: bool,
    pub is_div: bool,
    pub match_group: String,
    pub achievements: Vec<SerializableAchievement>,
}

impl PerGameStat {
    /// This game's personal rating, against the expected-values table.
    ///
    /// `None` without a table, rather than zero, so a chart or a badge shows
    /// nothing instead of a rating the data cannot support.
    pub fn personal_rating(&self, table: Option<&PersonalRatingData>) -> Option<f64> {
        let table = table?;
        let stats = ShipBattleStats {
            ship_id: self.ship_id,
            battles: 1,
            damage: self.damage,
            wins: u32::from(self.is_win),
            frags: self.frags,
        };
        table.calculate_pr(&[stats]).map(|result| result.pr)
    }

    /// Adopts a stored row.
    ///
    /// A row whose achievement blob will not parse contributes no
    /// achievements rather than failing the load: the rest of the row is
    /// still sound, and the overview counts what it can read.
    pub fn from_row(row: SessionStatRow) -> Self {
        Self {
            ship_name: row.ship_name,
            ship_id: (row.ship_id as u64).into(),
            game_time: row.game_time,
            sort_key: row.sort_key,
            player_id: row.player_id,
            damage: row.damage as u64,
            spotting_damage: row.spotting_damage as u64,
            frags: row.frags,
            raw_xp: row.raw_xp,
            base_xp: row.base_xp,
            is_win: row.is_win,
            is_loss: row.is_loss,
            is_draw: row.is_draw,
            is_div: row.is_div,
            match_group: row.match_group,
            achievements: serde_json::from_str(&row.achievements).unwrap_or_default(),
        }
    }
}

/// Session-wide totals over the filtered games, as the overview line reads
/// them left to right.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSummary {
    pub wins: usize,
    pub losses: usize,
    pub draws: usize,
    pub total_frags: i64,
    /// Best single game, with the ship that did it. `None` with no games.
    pub best_frags: Option<(GameParamId, i64)>,
    pub best_damage: Option<(GameParamId, u64)>,
}

impl SessionSummary {
    pub fn from_games(games: &[&PerGameStat]) -> Self {
        Self {
            wins: games.iter().filter(|game| game.is_win).count(),
            losses: games.iter().filter(|game| game.is_loss).count(),
            draws: games.iter().filter(|game| game.is_draw).count(),
            total_frags: games.iter().map(|game| game.frags).sum(),
            best_frags: games.iter().map(|game| (game.ship_id, game.frags)).max_by_key(|entry| entry.1),
            best_damage: games.iter().map(|game| (game.ship_id, game.damage)).max_by_key(|entry| entry.1),
        }
    }

    /// Games that reached a result. A game with no result counts for none of
    /// win, loss or draw, so this is not simply the game count.
    pub fn games_played(&self) -> usize {
        self.wins + self.losses + self.draws
    }

    /// `None` when no game reached a result, rather than a zero percent that
    /// reads as having lost them all.
    pub fn win_rate(&self) -> Option<f64> {
        let played = self.games_played();
        (played > 0).then(|| (self.wins as f64 / played as f64) * 100.0)
    }

    /// The egui overview's win/loss line: draws appear only when there are any.
    pub fn record_label(&self) -> String {
        if self.draws > 0 {
            format!("{}W/{}L/{}D", self.wins, self.losses, self.draws)
        } else {
            format!("{}W/{}L", self.wins, self.losses)
        }
    }
}

/// Totals each achievement across the filtered games, most-earned first.
///
/// The order the egui roundup lists them in: the one earned twenty times is
/// what a reader looks at, not whichever game happened to be read first.
pub fn aggregate_achievements(games: &[&PerGameStat]) -> Vec<SerializableAchievement> {
    let mut totals: Vec<SerializableAchievement> = Vec::new();
    for game in games {
        for earned in &game.achievements {
            match totals.iter_mut().find(|existing| existing.game_param_id == earned.game_param_id) {
                Some(existing) => existing.count += earned.count,
                None => totals.push(earned.clone()),
            }
        }
    }
    // Most-earned first, then by name, which is the order the egui roundup
    // lists them in: the achievement earned twenty times is the one worth
    // reading first.
    totals.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.display_name.cmp(&b.display_name)));
    totals
}

/// Keys the stored filter settings live under, shared so both front ends
/// read and write the same rows.
pub mod setting_keys {
    pub const LIMIT_ENABLED: &str = "session_stats_limit_enabled";
    pub const GAME_COUNT: &str = "session_stats_game_count";
    pub const DIVISION_FILTER: &str = "session_stats_division_filter";
    pub const GAME_MODE_FILTER: &str = "session_stats_game_mode_filter";
}

/// How many recent games the tab looks at.
///
/// An enum rather than a count plus an `enabled` flag, which admits the
/// meaningless "disabled, 25 games" pairing the egui settings carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameLimit {
    #[default]
    All,
    Recent(usize),
}

/// Everything the shared filter bar controls.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsFilters {
    pub limit: GameLimit,
    pub division: DivisionFilter,
    /// Match groups to keep. Empty means every group, matching the egui app's
    /// "All" state rather than meaning "none".
    pub game_modes: std::collections::BTreeSet<String>,
}

impl StatsFilters {
    fn keeps(&self, game: &PerGameStat) -> bool {
        self.division.keeps(game) && (self.game_modes.is_empty() || self.game_modes.contains(&game.match_group))
    }
}

/// Applies the filter bar. `games` is expected in `sort_key` order, which is
/// how the database returns it, so the recency limit takes from the end.
pub fn filter_games<'a>(games: &'a [PerGameStat], filters: &StatsFilters) -> Vec<&'a PerGameStat> {
    let kept: Vec<&PerGameStat> = games.iter().filter(|game| filters.keeps(game)).collect();
    match filters.limit {
        GameLimit::All => kept,
        GameLimit::Recent(count) => {
            let start = kept.len().saturating_sub(count);
            kept[start..].to_vec()
        }
    }
}

/// Every match group present in `games`, for the filter bar's mode buttons.
pub fn all_match_groups(games: &[PerGameStat]) -> std::collections::BTreeSet<String> {
    games.iter().map(|game| game.match_group.clone()).collect()
}

/// Aggregate over a set of games, as the per-ship table row shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PerformanceInfo {
    ship_id: Option<GameParamId>,
    wins: usize,
    losses: usize,
    draws: usize,
    total_games: usize,
    total_frags: i64,
    max_frags: Option<i64>,
    min_frags: Option<i64>,
    total_damage: u64,
    max_damage: Option<u64>,
    min_damage: Option<u64>,
    total_spotting_damage: u64,
    max_spotting_damage: Option<u64>,
    min_spotting_damage: Option<u64>,
    total_xp: i64,
    max_xp: Option<i64>,
    min_xp: Option<i64>,
    total_win_adjusted_xp: i64,
    max_win_adjusted_xp: Option<i64>,
    min_win_adjusted_xp: Option<i64>,
    /// `sort_key` of the most recent game, so the column sorts chronologically.
    last_played: String,
}

impl PerformanceInfo {
    pub fn from_games(games: &[&PerGameStat]) -> Self {
        let mut info = PerformanceInfo::default();

        for game in games {
            if info.ship_id.is_none() {
                info.ship_id = Some(game.ship_id);
            }

            if game.is_win {
                info.wins += 1;
            } else if game.is_loss {
                info.losses += 1;
            } else if game.is_draw {
                info.draws += 1;
            }

            info.total_frags += game.frags;
            info.max_frags = Some(info.max_frags.map_or(game.frags, |max| max.max(game.frags)));
            info.min_frags = Some(info.min_frags.map_or(game.frags, |min| min.min(game.frags)));

            info.total_damage += game.damage;
            info.max_damage = Some(info.max_damage.map_or(game.damage, |max| max.max(game.damage)));
            info.min_damage = Some(info.min_damage.map_or(game.damage, |min| min.min(game.damage)));

            info.total_spotting_damage += game.spotting_damage;
            info.max_spotting_damage =
                Some(info.max_spotting_damage.map_or(game.spotting_damage, |max| max.max(game.spotting_damage)));
            info.min_spotting_damage =
                Some(info.min_spotting_damage.map_or(game.spotting_damage, |min| min.min(game.spotting_damage)));

            info.total_xp += game.raw_xp;
            info.max_xp = Some(info.max_xp.map_or(game.raw_xp, |max| max.max(game.raw_xp)));
            info.min_xp = Some(info.min_xp.map_or(game.raw_xp, |min| min.min(game.raw_xp)));

            info.total_win_adjusted_xp += game.base_xp;
            info.max_win_adjusted_xp = Some(info.max_win_adjusted_xp.map_or(game.base_xp, |max| max.max(game.base_xp)));
            info.min_win_adjusted_xp = Some(info.min_win_adjusted_xp.map_or(game.base_xp, |min| min.min(game.base_xp)));

            info.total_games += 1;

            if game.sort_key > info.last_played {
                info.last_played = game.sort_key.clone();
            }
        }

        info
    }

    pub fn ship_id(&self) -> Option<GameParamId> {
        self.ship_id
    }

    pub fn wins(&self) -> usize {
        self.wins
    }

    pub fn losses(&self) -> usize {
        self.losses
    }

    pub fn draws(&self) -> usize {
        self.draws
    }

    pub fn total_games(&self) -> usize {
        self.total_games
    }

    pub fn last_played(&self) -> &str {
        &self.last_played
    }

    /// Wins over games played. `None` with no games, rather than zero, which
    /// a zero-game ship would otherwise be sorted and coloured by.
    pub fn win_rate(&self) -> Option<f64> {
        (self.total_games > 0).then(|| (self.wins as f64 / self.total_games as f64) * 100.0)
    }

    pub fn total_frags(&self) -> i64 {
        self.total_frags
    }

    pub fn max_frags(&self) -> Option<i64> {
        self.max_frags
    }

    pub fn min_frags(&self) -> Option<i64> {
        self.min_frags
    }

    pub fn avg_frags(&self) -> Option<f64> {
        (self.total_games > 0).then(|| self.total_frags as f64 / self.total_games as f64)
    }

    pub fn total_damage(&self) -> u64 {
        self.total_damage
    }

    pub fn max_damage(&self) -> Option<u64> {
        self.max_damage
    }

    pub fn min_damage(&self) -> Option<u64> {
        self.min_damage
    }

    pub fn avg_damage(&self) -> Option<f64> {
        (self.total_games > 0).then(|| self.total_damage as f64 / self.total_games as f64)
    }

    pub fn total_spotting_damage(&self) -> u64 {
        self.total_spotting_damage
    }

    pub fn max_spotting_damage(&self) -> Option<u64> {
        self.max_spotting_damage
    }

    pub fn min_spotting_damage(&self) -> Option<u64> {
        self.min_spotting_damage
    }

    pub fn avg_spotting_damage(&self) -> Option<f64> {
        (self.total_games > 0).then(|| self.total_spotting_damage as f64 / self.total_games as f64)
    }

    pub fn total_xp(&self) -> i64 {
        self.total_xp
    }

    pub fn max_xp(&self) -> Option<i64> {
        self.max_xp
    }

    pub fn min_xp(&self) -> Option<i64> {
        self.min_xp
    }

    pub fn avg_xp(&self) -> Option<f64> {
        (self.total_games > 0).then(|| self.total_xp as f64 / self.total_games as f64)
    }

    pub fn total_win_adjusted_xp(&self) -> i64 {
        self.total_win_adjusted_xp
    }

    pub fn max_win_adjusted_xp(&self) -> Option<i64> {
        self.max_win_adjusted_xp
    }

    pub fn min_win_adjusted_xp(&self) -> Option<i64> {
        self.min_win_adjusted_xp
    }

    pub fn avg_win_adjusted_xp(&self) -> Option<f64> {
        (self.total_games > 0).then(|| self.total_win_adjusted_xp as f64 / self.total_games as f64)
    }

    /// This ship's rating over the games in view, from the summed battles
    /// rather than the mean of their individual ratings.
    ///
    /// `None` when the ship is unknown, which is what a zero-game aggregate
    /// carries, or when the table has no expected values for it.
    pub fn personal_rating(&self, table: &PersonalRatingData) -> Option<PersonalRatingResult> {
        let ship_id = self.ship_id?;
        let stats = ShipBattleStats {
            ship_id,
            battles: self.total_games as u32,
            damage: self.total_damage,
            wins: self.wins as u32,
            frags: self.total_frags,
        };
        table.calculate_pr(&[stats])
    }
}

/// The session's rating across every ship played.
///
/// Each ship contributes its own aggregate, which is what the rating formula
/// expects: it weights a ship against that ship's expected values, not against
/// a pooled average.
pub fn session_personal_rating(
    games: &[&PerGameStat],
    table: Option<&PersonalRatingData>,
) -> Option<PersonalRatingResult> {
    let table = table?;

    let mut per_ship: std::collections::HashMap<GameParamId, ShipBattleStats> = std::collections::HashMap::new();
    for game in games {
        let entry = per_ship.entry(game.ship_id).or_insert(ShipBattleStats {
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
    }

    let stats: Vec<ShipBattleStats> = per_ship.into_values().collect();
    table.calculate_pr(&stats)
}

/// One ship's rating across the games in view: its worst, its best, and the
/// rating of the aggregate.
///
/// `average` is the rating computed from the summed battles rather than the
/// mean of the per-game ratings, which is the same formula the ship's header
/// figure uses; averaging ratings would weight a one-shot game as heavily as
/// a long one.
#[derive(Debug, Clone, PartialEq)]
pub struct PrStats {
    pub min: PersonalRatingResult,
    pub max: PersonalRatingResult,
    pub average: PersonalRatingResult,
}

impl PrStats {
    /// `None` when no game in `games` could be rated, which is also what a
    /// missing expected-values table produces, and when the games name more
    /// than one ship: the aggregate is scored against one ship's expected
    /// values, so pooling two ships into it would rate them both as the
    /// first.
    pub fn from_games(games: &[&PerGameStat], table: &PersonalRatingData) -> Option<Self> {
        let first = games.first()?;
        let ship_id = first.ship_id;
        if games.iter().any(|game| game.ship_id != ship_id) {
            return None;
        }

        let ratings: Vec<f64> = games.iter().filter_map(|game| game.personal_rating(Some(table))).collect();
        if ratings.is_empty() {
            return None;
        }
        let min = ratings.iter().copied().fold(f64::INFINITY, f64::min);
        let max = ratings.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        let aggregate = ShipBattleStats {
            ship_id,
            battles: games.len() as u32,
            damage: games.iter().map(|game| game.damage).sum(),
            wins: games.iter().filter(|game| game.is_win).count() as u32,
            frags: games.iter().map(|game| game.frags).sum(),
        };
        let average = table.calculate_pr(&[aggregate])?;

        Some(Self { min: PersonalRatingResult::new(min), max: PersonalRatingResult::new(max), average })
    }
}

/// Groups filtered games by ship, newest-played ship first -- the order the
/// egui table opens in.
pub fn per_ship_performance(games: &[&PerGameStat]) -> Vec<(String, PerformanceInfo)> {
    let mut by_ship: std::collections::BTreeMap<String, Vec<&PerGameStat>> = std::collections::BTreeMap::new();
    for game in games {
        by_ship.entry(game.ship_name.clone()).or_default().push(game);
    }

    let mut rows: Vec<(String, PerformanceInfo)> =
        by_ship.into_iter().map(|(ship, games)| (ship, PerformanceInfo::from_games(&games))).collect();
    rows.sort_by(|a, b| b.1.last_played().cmp(a.1.last_played()).then_with(|| a.0.cmp(&b.0)));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(ship: &str, sort_key: &str, damage: u64, frags: i64, win: bool, div: bool, mode: &str) -> PerGameStat {
        PerGameStat {
            ship_name: ship.to_string(),
            ship_id: 1u64.into(),
            game_time: sort_key.to_string(),
            sort_key: sort_key.to_string(),
            player_id: 7,
            damage,
            spotting_damage: damage / 10,
            frags,
            raw_xp: damage as i64 / 100,
            base_xp: damage as i64 / 200,
            is_win: win,
            is_loss: !win,
            is_draw: false,
            is_div: div,
            match_group: mode.to_string(),
            achievements: Vec::new(),
        }
    }

    fn sample() -> Vec<PerGameStat> {
        vec![
            game("Yamato", "2026-01-01 10:00:00", 100_000, 2, true, false, "pvp"),
            game("Yamato", "2026-01-02 10:00:00", 50_000, 0, false, true, "ranked"),
            game("Shimakaze", "2026-01-03 10:00:00", 80_000, 3, true, false, "pvp"),
        ]
    }

    #[test]
    fn match_group_codes_map_to_display_names_and_pass_unknown_codes_through() {
        assert_eq!(match_group_display_name("pvp"), "Random");
        assert_eq!(match_group_display_name("cooperative"), "Co-op");
        assert_eq!(match_group_display_name(""), "Unknown");
        assert_eq!(match_group_display_name("skirmish"), "skirmish");
    }

    #[test]
    fn the_division_filter_keeps_only_matching_games() {
        let games = sample();
        let solo = StatsFilters { division: DivisionFilter::SoloOnly, ..Default::default() };
        let div = StatsFilters { division: DivisionFilter::DivOnly, ..Default::default() };

        assert_eq!(filter_games(&games, &solo).len(), 2);
        assert_eq!(filter_games(&games, &div).len(), 1);
        assert_eq!(filter_games(&games, &StatsFilters::default()).len(), 3);
    }

    #[test]
    fn an_empty_game_mode_set_means_every_mode_rather_than_none() {
        let games = sample();
        assert_eq!(filter_games(&games, &StatsFilters::default()).len(), 3);

        let ranked = StatsFilters { game_modes: ["ranked".to_string()].into_iter().collect(), ..Default::default() };
        assert_eq!(filter_games(&games, &ranked).len(), 1);
    }

    #[test]
    fn the_recency_limit_keeps_the_newest_games() {
        let games = sample();
        let limited = StatsFilters { limit: GameLimit::Recent(2), ..Default::default() };
        let kept = filter_games(&games, &limited);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].sort_key, "2026-01-02 10:00:00");
        assert_eq!(kept[1].sort_key, "2026-01-03 10:00:00", "the newest game is kept");
    }

    #[test]
    fn a_limit_larger_than_the_game_count_keeps_everything() {
        let games = sample();
        let limited = StatsFilters { limit: GameLimit::Recent(99), ..Default::default() };
        assert_eq!(filter_games(&games, &limited).len(), 3);
    }

    #[test]
    fn the_limit_applies_after_the_other_filters() {
        let games = sample();
        let filters =
            StatsFilters { limit: GameLimit::Recent(2), division: DivisionFilter::SoloOnly, ..Default::default() };
        let kept = filter_games(&games, &filters);
        assert_eq!(kept.len(), 2, "both solo games survive the limit");
        assert!(kept.iter().all(|game| !game.is_div));
    }

    #[test]
    fn aggregation_totals_and_extremes_cover_every_game() {
        let games = sample();
        let yamato: Vec<&PerGameStat> = games.iter().filter(|g| g.ship_name == "Yamato").collect();
        let info = PerformanceInfo::from_games(&yamato);

        assert_eq!(info.total_games(), 2);
        assert_eq!(info.wins(), 1);
        assert_eq!(info.losses(), 1);
        assert_eq!(info.total_damage(), 150_000);
        assert_eq!(info.max_damage(), Some(100_000));
        assert_eq!(info.min_damage(), Some(50_000));
        assert_eq!(info.avg_damage(), Some(75_000.0));
        assert_eq!(info.win_rate(), Some(50.0));
        assert_eq!(info.last_played(), "2026-01-02 10:00:00");

        // The other four statistics aggregate on the same rule; asserting
        // damage alone would not catch one of them reading the wrong field.
        assert_eq!(info.total_frags(), 2);
        assert_eq!(info.max_frags(), Some(2));
        assert_eq!(info.min_frags(), Some(0), "a frag-less game is a recorded zero, not an absent minimum");
        assert_eq!(info.total_spotting_damage(), 15_000);
        assert_eq!(info.max_spotting_damage(), Some(10_000));
        assert_eq!(info.min_spotting_damage(), Some(5_000));
        assert_eq!(info.total_xp(), 1_500);
        assert_eq!(info.max_xp(), Some(1_000));
        assert_eq!(info.min_xp(), Some(500));
        assert_eq!(info.total_win_adjusted_xp(), 750);
        assert_eq!(info.max_win_adjusted_xp(), Some(500));
        assert_eq!(info.min_win_adjusted_xp(), Some(250));
    }

    /// A draw is neither a win nor a loss, but it is a game played: it counts
    /// toward the win rate's denominator and shows in the record line.
    #[test]
    fn a_draw_is_counted_and_dilutes_the_win_rate() {
        let mut drawn = game("Yamato", "2026-01-03 10:00:00", 60_000, 1, false, false, "pvp");
        drawn.is_loss = false;
        drawn.is_draw = true;

        let info = PerformanceInfo::from_games(&[&drawn]);

        assert_eq!(info.draws(), 1);
        assert_eq!(info.wins(), 0);
        assert_eq!(info.losses(), 0);
        assert_eq!(info.total_games(), 1);
        assert_eq!(info.win_rate(), Some(0.0), "a draw is a game played, not an absent one");
    }

    #[test]
    fn aggregating_no_games_reports_absent_minimums_rather_than_zero() {
        let info = PerformanceInfo::from_games(&[]);
        assert_eq!(info.total_games(), 0);
        assert_eq!(info.min_damage(), None);
        assert_eq!(info.min_frags(), None);
        assert_eq!(info.win_rate(), None, "no games is not a zero win rate");
        assert_eq!(info.avg_damage(), None);
        assert_eq!(info.ship_id(), None);
    }

    #[test]
    fn per_ship_rows_are_ordered_by_most_recently_played() {
        let games = sample();
        let all: Vec<&PerGameStat> = games.iter().collect();
        let rows = per_ship_performance(&all);
        let ships: Vec<&str> = rows.iter().map(|(ship, _)| ship.as_str()).collect();
        assert_eq!(ships, vec!["Shimakaze", "Yamato"], "Shimakaze was played last");
    }

    #[test]
    fn every_match_group_present_is_offered_to_the_filter_bar() {
        let groups = all_match_groups(&sample());
        assert_eq!(groups.into_iter().collect::<Vec<_>>(), vec!["pvp".to_string(), "ranked".to_string()]);
    }

    fn achievement(id: u64, count: usize) -> SerializableAchievement {
        SerializableAchievement {
            game_param_id: id.into(),
            display_name: format!("Achievement {id}"),
            description: String::new(),
            icon_key: String::new(),
            count,
        }
    }

    fn result_of(win: bool, loss: bool, draw: bool, frags: i64, damage: u64) -> PerGameStat {
        let mut stat = game("Yamato", "2026-01-01 00:00:00", damage, frags, win, false, "pvp");
        stat.is_win = win;
        stat.is_loss = loss;
        stat.is_draw = draw;
        stat
    }

    #[test]
    fn the_record_line_shows_draws_only_when_there_are_any() {
        let games = [result_of(true, false, false, 1, 10), result_of(false, true, false, 0, 5)];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        assert_eq!(SessionSummary::from_games(&refs).record_label(), "1W/1L");

        let with_draw = [result_of(true, false, false, 1, 10), result_of(false, false, true, 0, 5)];
        let refs: Vec<&PerGameStat> = with_draw.iter().collect();
        assert_eq!(SessionSummary::from_games(&refs).record_label(), "1W/0L/1D");
    }

    #[test]
    fn the_win_rate_counts_only_games_that_reached_a_result() {
        let games = [
            result_of(true, false, false, 0, 0),
            result_of(false, true, false, 0, 0),
            result_of(false, false, false, 0, 0),
        ];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let summary = SessionSummary::from_games(&refs);
        assert_eq!(summary.games_played(), 2, "the game with no result is not counted");
        assert_eq!(summary.win_rate(), Some(50.0));
    }

    #[test]
    fn a_session_with_no_finished_games_has_no_win_rate_and_no_best_game() {
        let summary = SessionSummary::from_games(&[]);
        assert_eq!(summary.win_rate(), None);
        assert_eq!(summary.best_damage, None);
        assert_eq!(summary.best_frags, None);
    }

    #[test]
    fn the_best_game_carries_the_ship_that_played_it() {
        let games = [result_of(true, false, false, 2, 90_000), result_of(false, true, false, 5, 10_000)];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let summary = SessionSummary::from_games(&refs);
        assert_eq!(summary.best_frags.map(|(_, frags)| frags), Some(5));
        assert_eq!(summary.best_damage.map(|(_, damage)| damage), Some(90_000));
        assert_eq!(summary.total_frags, 7);
    }

    /// Most-earned first is what the roundup reads down, so the count decides
    /// the order rather than which game happened to be parsed first.
    #[test]
    fn achievements_are_totalled_across_games_and_ordered_by_how_often_they_were_earned() {
        let mut first = result_of(true, false, false, 0, 0);
        first.achievements = vec![achievement(20, 1), achievement(10, 2)];
        let mut second = result_of(true, false, false, 0, 0);
        second.achievements = vec![achievement(10, 3)];

        let games = [first, second];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let totals = aggregate_achievements(&refs);

        assert_eq!(totals.len(), 2);
        assert_eq!(totals[0].count, 5, "the repeated achievement accumulates and leads");
        assert_eq!(totals[0].game_param_id, 10u64.into());
        assert_eq!(totals[1].game_param_id, 20u64.into(), "the one earned once comes after");
    }
}

#[cfg(test)]
mod rating_tests {
    use super::PerGameStat;
    use super::PerformanceInfo;
    use super::PrStats;
    use super::session_personal_rating;
    use crate::personal_rating::PersonalRatingData;

    fn game() -> PerGameStat {
        PerGameStat {
            ship_name: "Yamato".into(),
            ship_id: 1u64.into(),
            game_time: String::new(),
            sort_key: String::new(),
            player_id: 1,
            damage: 100_000,
            spotting_damage: 0,
            frags: 2,
            raw_xp: 0,
            base_xp: 0,
            is_win: true,
            is_loss: false,
            is_draw: false,
            is_div: false,
            match_group: "pvp".into(),
            achievements: Vec::new(),
        }
    }

    /// The expected-values table the app ships, so a rating test runs against
    /// real figures rather than invented ones.
    fn fixture_table() -> PersonalRatingData {
        let mut table = PersonalRatingData::new();
        table.load_from_bytes(crate::personal_rating::EXPECTED_VALUES_FIXTURE).expect("the fixture parses");
        table
    }

    /// A ship the fixture carries expected values for.
    const RATED_SHIP: u64 = 3374266064;

    fn rated_game(damage: u64, frags: i64, win: bool) -> PerGameStat {
        PerGameStat { ship_id: RATED_SHIP.into(), damage, frags, is_win: win, is_loss: !win, ..game() }
    }

    #[test]
    fn a_ships_rating_spans_its_worst_and_best_game() {
        let table = fixture_table();
        let expected = table.get_ship_expected(RATED_SHIP.into()).expect("the fixture rates this ship");

        let strong =
            rated_game((expected.average_damage_dealt * 2.0) as u64, (expected.average_frags * 2.0) as i64, true);
        let weak = rated_game((expected.average_damage_dealt * 0.5) as u64, 0, false);
        let games = [&strong, &weak];

        let stats = PrStats::from_games(&games, &table).expect("both games rate");

        assert!(stats.max.pr > stats.min.pr, "the high-damage game rates higher");
        assert!(
            stats.min.pr <= stats.average.pr && stats.average.pr <= stats.max.pr,
            "the aggregate sits between the two"
        );
    }

    #[test]
    fn a_ship_the_table_does_not_carry_has_no_rating() {
        let table = PersonalRatingData::new();
        let played = rated_game(100_000, 2, true);
        assert!(PrStats::from_games(&[&played], &table).is_none());
    }

    #[test]
    fn a_ships_aggregate_rating_comes_from_its_summed_battles() {
        let table = fixture_table();
        let strong = rated_game(120_000, 3, true);
        let weak = rated_game(10_000, 0, false);
        let info = PerformanceInfo::from_games(&[&strong, &weak]);

        let aggregate = info.personal_rating(&table).expect("the fixture rates this ship");
        let stats = PrStats::from_games(&[&strong, &weak], &table).expect("both games rate");

        assert!((aggregate.pr - stats.average.pr).abs() < 1e-9, "the table row and the header report one figure");
    }

    #[test]
    fn a_ship_with_no_games_has_no_aggregate_rating() {
        let info = PerformanceInfo::from_games(&[]);
        assert!(info.personal_rating(&fixture_table()).is_none(), "no ship played, so nothing to rate");
    }

    #[test]
    fn without_an_expected_values_table_there_is_no_rating_rather_than_a_zero() {
        let games = [game()];
        let refs: Vec<&PerGameStat> = games.iter().collect();

        assert_eq!(games[0].personal_rating(None), None);
        assert!(session_personal_rating(&refs, None).is_none());
    }
}
