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

pub mod plot;

pub mod replay_set;

pub mod sample;

pub mod saved_charts;

pub mod record;

pub mod session;

use serde::Deserialize;
use serde::Serialize;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::types::GameParamId;
use wowsunpack::rating_scope::RatingScope;

use crate::personal_rating::PersonalRatingCategory;
use crate::personal_rating::PersonalRatingData;
use crate::personal_rating::PersonalRatingResult;
use crate::personal_rating::ShipBattleStats;
use crate::stats::record::RecordBreakdown;
use wows_replay_insights::match_contribution::MatchContributionVersion;
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
    /// The recording player's match contribution, in whole points. `None` when the
    /// game was not rated, or was rated by an older formula.
    pub match_contribution: Option<i64>,
}

impl PerGameStat {
    /// Whether a rating applies to this game. The stored rows keep only the
    /// match group, so that is what classifies it.
    pub fn rating_scope(&self) -> RatingScope {
        RatingScope::of_match_group(&self.match_group)
    }

    /// This game's personal rating, against the expected-values table.
    ///
    /// `None` without a table, rather than zero, so a chart or a badge shows
    /// nothing instead of a rating the data cannot support; and `None` for a
    /// battle kind the rating does not apply to.
    pub fn personal_rating(&self, table: Option<&PersonalRatingData>) -> Option<f64> {
        let table = table?;
        if !self.rating_scope().is_rated() {
            return None;
        }

        let stats = ShipBattleStats {
            ship_id: self.ship_id,
            battles: 1,
            damage: self.damage,
            wins: u32::from(self.is_win),
            frags: self.frags,
        };
        table.calculate_pr(&[stats]).map(|result| result.pr)
    }

    /// Reads one battle off a parsed report.
    ///
    /// `game_time` is the replay's own `dateTime` string rather than the
    /// report's timestamp: it is what both apps store and dedupe on, so a
    /// battle recorded by one is the same row to the other.
    ///
    /// `achievement_id` resolves an achievement's internal name against the
    /// build the replay was recorded on, which is how the stored rows key
    /// them; the normalized report carries only the name. An achievement the
    /// build cannot name is left out rather than stored under a stand-in id,
    /// which would make every unnamed achievement aggregate as one.
    ///
    /// `None` when the report names no recording player, which is the one
    /// case there is no game to record.
    pub fn from_report(
        report: &NormalizedBattleReport,
        game_time: &str,
        achievement_id: &dyn Fn(&str) -> Option<GameParamId>,
    ) -> Option<PerGameStat> {
        let player = report.players.iter().find(|player| player.is_self)?;
        let results = player.server_results.as_ref();
        let battle_result = report.metadata.resolved_battle_result();

        Some(PerGameStat {
            ship_name: player.ship_display_name(),
            ship_id: player.ship_id,
            game_time: game_time.to_string(),
            sort_key: sortable_game_time(game_time),
            player_id: player.db_id.raw(),
            damage: results.and_then(|results| results.damage).unwrap_or_default(),
            spotting_damage: player.spotting_damage().unwrap_or_default(),
            frags: results.and_then(|results| results.kills).unwrap_or_default(),
            raw_xp: results.and_then(|results| results.raw_xp).unwrap_or_default(),
            base_xp: results.and_then(|results| results.xp).unwrap_or_default(),
            is_win: matches!(battle_result, Some(BattleResult::Win(_))),
            is_loss: matches!(battle_result, Some(BattleResult::Loss(_))),
            is_draw: matches!(battle_result, Some(BattleResult::Draw)),
            is_div: player.division_label.is_some(),
            match_group: report.metadata.match_group.clone(),
            achievements: player
                .achievements
                .iter()
                .filter_map(|achievement| {
                    Some(SerializableAchievement {
                        game_param_id: achievement_id(&achievement.name)?,
                        display_name: achievement.display_name.clone(),
                        description: achievement.description.clone(),
                        icon_key: achievement.icon_key.clone(),
                        count: achievement.count,
                    })
                })
                .collect(),
            match_contribution: player.match_contribution.as_ref().map(|rating| rating.mc.round() as i64),
        })
    }

    /// The row this game is stored as.
    pub fn to_row(&self) -> SessionStatRow {
        SessionStatRow {
            id: 0,
            sort_key: self.sort_key.clone(),
            ship_name: self.ship_name.clone(),
            ship_id: self.ship_id.raw() as i64,
            player_id: self.player_id,
            game_time: self.game_time.clone(),
            match_group: self.match_group.clone(),
            damage: self.damage as i64,
            spotting_damage: self.spotting_damage as i64,
            frags: self.frags,
            raw_xp: self.raw_xp,
            base_xp: self.base_xp,
            is_win: self.is_win,
            is_loss: self.is_loss,
            is_draw: self.is_draw,
            is_div: self.is_div,
            achievements: serde_json::to_string(&self.achievements).unwrap_or_else(|_| "[]".to_string()),
            match_contribution: self.match_contribution.map(|mc| mc as f64),
            match_contribution_version: self.match_contribution.map(|_| i64::from(MatchContributionVersion::CURRENT.0)),
        }
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
            // A rating from another formula revision would sit beside current
            // ones as if comparable; it reads as unrated until re-recorded.
            match_contribution: row
                .match_contribution
                .filter(|_| row.match_contribution_version == Some(i64::from(MatchContributionVersion::CURRENT.0)))
                .map(|mc| mc.round() as i64),
        }
    }
}

/// `game_time` rewritten so lexicographic order is chronological.
///
/// The game writes `DD.MM.YYYY HH:MM:SS`, which sorts by day of the month.
/// A string that does not read that way is kept as it is: an unsortable key
/// is better than a wrong one.
pub fn sortable_game_time(game_time: &str) -> String {
    let Some((date, time)) = game_time.split_once(' ') else {
        return game_time.to_string();
    };
    let parts: Vec<&str> = date.split('.').collect();
    let [day, month, year] = parts.as_slice() else {
        return game_time.to_string();
    };
    format!("{year}-{month}-{day} {time}")
}

/// Session-wide totals over the filtered games, as the overview line reads
/// them left to right.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSummary {
    pub records: RecordBreakdown,
    pub total_frags: i64,
    /// Best single game, with the ship that did it. `None` with no games.
    pub best_frags: Option<(GameParamId, i64)>,
    pub best_damage: Option<(GameParamId, u64)>,
}

impl SessionSummary {
    pub fn from_games(games: &[&PerGameStat]) -> Self {
        Self {
            records: RecordBreakdown::from_games(games.iter().copied()),
            total_frags: games.iter().map(|game| game.frags).sum(),
            best_frags: games.iter().map(|game| (game.ship_id, game.frags)).max_by_key(|entry| entry.1),
            best_damage: games.iter().map(|game| (game.ship_id, game.damage)).max_by_key(|entry| entry.1),
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
    pub const SESSION_SCOPE: &str = "session_stats_scope";
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

/// Applies the filter bar with the recency limit counted per ship rather than
/// over the session.
///
/// What "last 25" means on the per-ship table and the charts: 25 battles in each
/// ship, not the 25 most recent overall. The egui app draws those two surfaces
/// from `per_ship_limited_games` and its summary line from the global count
/// (`data/session_stats.rs:325`, used at `ui/stats_tab.rs:406` and `:541`), so a
/// session filtered in one app reads the same in the other.
///
/// `games` is expected in `sort_key` order, and the result keeps that order: a
/// chart plots along it.
pub fn filter_games_per_ship<'a>(games: &'a [PerGameStat], filters: &StatsFilters) -> Vec<&'a PerGameStat> {
    let kept: Vec<&PerGameStat> = games.iter().filter(|game| filters.keeps(game)).collect();
    let GameLimit::Recent(count) = filters.limit else { return kept };

    // How many of each ship's battles have been passed over, counted from the
    // end: a battle is kept while its ship still has room.
    let mut seen: std::collections::HashMap<GameParamId, usize> = std::collections::HashMap::new();
    let mut keeping: Vec<bool> = vec![false; kept.len()];
    for (at, game) in kept.iter().enumerate().rev() {
        let seen = seen.entry(game.ship_id).or_default();
        if *seen < count {
            keeping[at] = true;
            *seen += 1;
        }
    }

    kept.into_iter().zip(keeping).filter(|(_, keeping)| *keeping).map(|(game, _)| game).collect()
}

/// Every match group present in `games`, for the filter bar's mode buttons.
pub fn all_match_groups(games: &[PerGameStat]) -> std::collections::BTreeSet<String> {
    games.iter().map(|game| game.match_group.clone()).collect()
}

/// Aggregate over a set of games, as the per-ship table row shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PerformanceInfo {
    ship_id: Option<GameParamId>,
    records: RecordBreakdown,
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

    /// The Random and Ranked games alone, which the rating is scored on.
    rated: RatedTotals,
}

impl PerformanceInfo {
    pub fn from_games(games: &[&PerGameStat]) -> Self {
        let mut info = PerformanceInfo::default();

        for game in games {
            if info.ship_id.is_none() {
                info.ship_id = Some(game.ship_id);
            }

            info.records.add(game);
            if game.rating_scope().is_rated() {
                info.rated.battles += 1;
                info.rated.damage += game.damage;
                info.rated.frags += game.frags;
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

    /// Every game's result, by the battle kinds the win rate is broken down by.
    pub fn records(&self) -> &RecordBreakdown {
        &self.records
    }

    /// Every game in view, rated or not, which is what the averages divide by.
    pub fn total_games(&self) -> usize {
        self.total_games
    }

    pub fn last_played(&self) -> &str {
        &self.last_played
    }

    /// Wins over the Random and Ranked battles that reached a result. `None`
    /// with none, rather than zero, which a ship played only in other modes
    /// would otherwise be sorted and coloured by.
    pub fn rated_win_rate(&self) -> Option<f64> {
        self.records.rated().win_rate()
    }

    /// Whether any game in view is one a rating applies to, whether or not
    /// it reached a result.
    pub fn is_rated(&self) -> bool {
        self.rated.battles > 0
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

    /// This ship's rating over its Random and Ranked games in view, from the
    /// summed battles rather than the mean of their individual ratings.
    ///
    /// `None` when the ship is unknown, which is what a zero-game aggregate
    /// carries, when it has no rated game, or when the table has no expected
    /// values for it.
    pub fn personal_rating(&self, table: &PersonalRatingData) -> Option<PersonalRatingResult> {
        let ship_id = self.ship_id?;
        if self.rated.battles == 0 {
            return None;
        }

        let stats = ShipBattleStats {
            ship_id,
            battles: self.rated.battles,
            damage: self.rated.damage,
            wins: self.records.rated().wins as u32,
            frags: self.rated.frags,
        };
        table.calculate_pr(&[stats])
    }
}

/// Totals over the games a rating applies to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RatedTotals {
    /// Every rated game, including one that reached no result.
    battles: u32,
    damage: u64,
    frags: i64,
}

/// The session's rating across every ship played in Random and Ranked.
///
/// Each ship contributes its own aggregate, which is what the rating formula
/// expects: it weights a ship against that ship's expected values, not against
/// a pooled average. `None` without a table or a rated game.
pub fn session_personal_rating(
    games: &[&PerGameStat],
    table: Option<&PersonalRatingData>,
) -> Option<PersonalRatingResult> {
    let table = table?;

    let mut per_ship: std::collections::HashMap<GameParamId, ShipBattleStats> = std::collections::HashMap::new();
    for game in games.iter().filter(|game| game.rating_scope().is_rated()) {
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
    /// Scored over the Random and Ranked games in `games` alone.
    ///
    /// `None` when no game in `games` could be rated, which is also what a
    /// missing expected-values table or a set of only unrated battle kinds
    /// produces, and when the games name more than one ship: the aggregate is
    /// scored against one ship's expected values, so pooling two ships into it
    /// would rate them both as the first.
    pub fn from_games(games: &[&PerGameStat], table: &PersonalRatingData) -> Option<Self> {
        let first = games.first()?;
        let ship_id = first.ship_id;
        if games.iter().any(|game| game.ship_id != ship_id) {
            return None;
        }

        let rated: Vec<&PerGameStat> = games.iter().copied().filter(|game| game.rating_scope().is_rated()).collect();
        let ratings: Vec<f64> = rated.iter().filter_map(|game| game.personal_rating(Some(table))).collect();
        if ratings.is_empty() {
            return None;
        }
        let min = ratings.iter().copied().fold(f64::INFINITY, f64::min);
        let max = ratings.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        let aggregate = ShipBattleStats {
            ship_id,
            battles: rated.len() as u32,
            damage: rated.iter().map(|game| game.damage).sum(),
            wins: rated.iter().filter(|game| game.is_win).count() as u32,
            frags: rated.iter().map(|game| game.frags).sum(),
        };
        let average = table.calculate_pr(&[aggregate])?;

        Some(Self { min: PersonalRatingResult::new(min), max: PersonalRatingResult::new(max), average })
    }
}

/// Match contribution across the games in view: the worst, the best and the mean
/// of the rated games. Games of a battle type MC does not apply to are left
/// out, not counted as nothing.
///
/// Unlike PR, MC is not recomputed from summed totals: it rates one whole
/// battle, so the mean of the battles is the session's figure. Ships may be
/// mixed, since every rating is already against its own ship.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McStats {
    pub min: f64,
    pub max: f64,
    pub average: f64,
    /// How many of the games carried a rating.
    pub rated_games: usize,
}

impl McStats {
    /// `None` when no game in `games` was a rated battle with an MC.
    pub fn from_games(games: &[&PerGameStat]) -> Option<Self> {
        Self::from_ratings(
            games.iter().filter(|game| game.rating_scope().is_rated()).filter_map(|game| game.match_contribution),
        )
    }

    /// `None` when `ratings` is empty.
    pub fn from_ratings(ratings: impl IntoIterator<Item = i64>) -> Option<Self> {
        let ratings: Vec<f64> = ratings.into_iter().map(|mc| mc as f64).collect();
        if ratings.is_empty() {
            return None;
        }

        Some(Self {
            min: ratings.iter().copied().fold(f64::INFINITY, f64::min),
            max: ratings.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            average: ratings.iter().sum::<f64>() / ratings.len() as f64,
            rated_games: ratings.len(),
        })
    }

    pub fn category(value: f64) -> PersonalRatingCategory {
        PersonalRatingCategory::from_pr(value)
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

/// Groups filtered games by ship id, newest-played ship first.
///
/// Display names are labels, not identities: two ships can have the same
/// localized name and must retain separate statistics and chart selection.
pub fn per_ship_performance_by_id(games: &[&PerGameStat]) -> Vec<(String, PerformanceInfo)> {
    let mut by_ship: std::collections::HashMap<GameParamId, Vec<&PerGameStat>> = std::collections::HashMap::new();
    for game in games {
        by_ship.entry(game.ship_id).or_default().push(game);
    }

    let mut rows: Vec<(GameParamId, String, PerformanceInfo)> = by_ship
        .into_iter()
        .filter_map(|(ship_id, games)| {
            let ship = games.first()?.ship_name.clone();
            Some((ship_id, ship, PerformanceInfo::from_games(&games)))
        })
        .collect();
    rows.sort_by(|left, right| {
        right.2.last_played().cmp(left.2.last_played()).then_with(|| left.0.raw().cmp(&right.0.raw()))
    });
    rows.into_iter().map(|(_, ship, info)| (ship, info)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::record::Record;

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
            match_contribution: None,
        }
    }

    /// The recency limit counts per ship on the surfaces that are about ships:
    /// "last one" keeps one battle in each, not the one most recent overall.
    #[test]
    fn the_limit_counts_per_ship_where_the_egui_tab_counts_per_ship() {
        let mut games = Vec::new();
        for (at, ship) in [1_u64, 2, 1, 2, 1].into_iter().enumerate() {
            let mut one = game("Ship", &format!("2026-01-0{at}"), 10_000, 1, true, false, "pvp");
            one.ship_id = ship.into();
            games.push(one);
        }

        let one_each = StatsFilters { limit: GameLimit::Recent(1), ..StatsFilters::default() };
        let kept = filter_games_per_ship(&games, &one_each);
        assert_eq!(kept.len(), 2, "one battle in each of the two ships");
        assert_eq!(kept[0].ship_id, 2u64.into(), "and in the order they were played");
        assert_eq!(kept[0].sort_key, "2026-01-03");
        assert_eq!(kept[1].ship_id, 1u64.into());
        assert_eq!(kept[1].sort_key, "2026-01-04");

        // The session-wide count is the other reading, and keeps one game.
        assert_eq!(filter_games(&games, &one_each).len(), 1);

        // With no limit the two agree.
        let all = StatsFilters::default();
        assert_eq!(filter_games_per_ship(&games, &all).len(), 5);
        assert_eq!(filter_games(&games, &all).len(), 5);
    }

    fn sample() -> Vec<PerGameStat> {
        vec![
            game("Yamato", "2026-01-01 10:00:00", 100_000, 2, true, false, "pvp"),
            game("Yamato", "2026-01-02 10:00:00", 50_000, 0, false, true, "ranked"),
            game("Shimakaze", "2026-01-03 10:00:00", 80_000, 3, true, false, "pvp"),
        ]
    }

    fn rated_game(mode: &str, mc: i64) -> PerGameStat {
        PerGameStat { match_contribution: Some(mc), ..game("Yamato", "2026-01-01 10:00:00", 1, 0, true, false, mode) }
    }

    /// A stored MC from a battle type MC does not apply to is left out of the
    /// figures, rather than counted.
    #[test]
    fn match_contribution_counts_only_random_and_ranked_games() {
        let games = [
            rated_game("pvp", 1_000),
            rated_game("ranked", 1_600),
            rated_game("cooperative", 3_000),
            rated_game("pve", 0),
        ];
        let games: Vec<&PerGameStat> = games.iter().collect();

        let stats = McStats::from_games(&games).expect("two games are rated");
        assert_eq!(stats, McStats { min: 1_000.0, max: 1_600.0, average: 1_300.0, rated_games: 2 });
    }

    #[test]
    fn match_contribution_is_absent_without_a_random_or_ranked_game() {
        let games = [rated_game("cooperative", 3_000), rated_game("clan", 1_200), rated_game("event", 900)];
        let games: Vec<&PerGameStat> = games.iter().collect();

        assert_eq!(McStats::from_games(&games), None);
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
        assert_eq!(info.records().rated(), Record { wins: 1, losses: 1, draws: 0 });
        assert_eq!(info.total_damage(), 150_000);
        assert_eq!(info.max_damage(), Some(100_000));
        assert_eq!(info.min_damage(), Some(50_000));
        assert_eq!(info.avg_damage(), Some(75_000.0));
        assert_eq!(info.rated_win_rate(), Some(50.0));
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

        assert_eq!(info.records().rated(), Record { wins: 0, losses: 0, draws: 1 });
        assert_eq!(info.total_games(), 1);
        assert_eq!(info.rated_win_rate(), Some(0.0), "a draw is a game played, not an absent one");
    }

    #[test]
    fn aggregating_no_games_reports_absent_minimums_rather_than_zero() {
        let info = PerformanceInfo::from_games(&[]);
        assert_eq!(info.total_games(), 0);
        assert_eq!(info.min_damage(), None);
        assert_eq!(info.min_frags(), None);
        assert_eq!(info.rated_win_rate(), None, "no games is not a zero win rate");
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
        assert_eq!(SessionSummary::from_games(&refs).records.rated().label(), "1W/1L");

        let with_draw = [result_of(true, false, false, 1, 10), result_of(false, false, true, 0, 5)];
        let refs: Vec<&PerGameStat> = with_draw.iter().collect();
        assert_eq!(SessionSummary::from_games(&refs).records.rated().label(), "1W/0L/1D");
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
        assert_eq!(summary.records.rated().battles(), 2, "the game with no result is not counted");
        assert_eq!(summary.records.rated().win_rate(), Some(50.0));
    }

    #[test]
    fn a_session_with_no_finished_games_has_no_win_rate_and_no_best_game() {
        let summary = SessionSummary::from_games(&[]);
        assert_eq!(summary.records.rated().win_rate(), None);
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
            match_contribution: None,
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

/// Win rate and rating count Random and Ranked games alone.
#[cfg(test)]
mod scope_tests {
    use super::PerGameStat;
    use super::PerformanceInfo;
    use super::PrStats;
    use super::SessionSummary;
    use super::chart::ChartableStat;
    use super::chart::bar_chart_series;
    use super::chart::line_chart_series;
    use super::per_ship_performance_by_id;
    use super::record::Record;
    use super::session_personal_rating;
    use super::table::ship_header;
    use crate::personal_rating::PersonalRatingData;
    use crate::personal_rating::ShipBattleStats;

    /// A ship the fixture carries expected values for.
    const RATED_SHIP: u64 = 3374266064;

    fn fixture_table() -> PersonalRatingData {
        let mut table = PersonalRatingData::new();
        table.load_from_bytes(crate::personal_rating::EXPECTED_VALUES_FIXTURE).expect("the fixture parses");
        table
    }

    fn played(at: usize, match_group: &str, damage: u64, frags: i64, win: bool) -> PerGameStat {
        let time = format!("2026-01-01 10:{at:02}:00");
        PerGameStat {
            ship_name: "Yamato".into(),
            ship_id: RATED_SHIP.into(),
            game_time: time.clone(),
            sort_key: time,
            player_id: 1,
            damage,
            spotting_damage: damage / 10,
            frags,
            raw_xp: 1_000,
            base_xp: 500,
            is_win: win,
            is_loss: !win,
            is_draw: false,
            is_div: false,
            match_group: match_group.into(),
            achievements: Vec::new(),

            match_contribution: None,
        }
    }

    /// Random 2W/1L, Ranked 0W/1L, and four strong games in other modes that
    /// would lift both the win rate and the rating if they counted.
    fn mixed() -> Vec<PerGameStat> {
        vec![
            played(0, "pvp", 120_000, 3, true),
            played(1, "cooperative", 150_000, 5, true),
            played(2, "pvp", 40_000, 0, false),
            played(3, "ranked", 60_000, 1, false),
            played(4, "clan", 20_000, 0, false),
            played(5, "pvp", 80_000, 1, true),
            played(6, "cooperative", 140_000, 4, true),
            played(7, "event", 90_000, 2, true),
        ]
    }

    fn rated_subset(games: &[PerGameStat]) -> Vec<&PerGameStat> {
        games.iter().filter(|game| matches!(game.match_group.as_str(), "pvp" | "ranked")).collect()
    }

    fn coop_only() -> Vec<PerGameStat> {
        vec![played(0, "cooperative", 150_000, 5, true), played(1, "pve", 30_000, 0, false)]
    }

    #[test]
    fn the_win_rate_counts_random_and_ranked_and_the_hover_breaks_it_down() {
        let games = mixed();
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let summary = SessionSummary::from_games(&refs);
        let rated = summary.records.rated();

        insta::assert_snapshot!(format!(
            "{:#?}\nrated: {} {:?}\n\n{}",
            summary.records,
            rated.label(),
            rated.win_rate(),
            summary.records.hover_text()
        ));
    }

    #[test]
    fn a_ships_record_and_header_count_its_rated_games() {
        let games = mixed();
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let info = PerformanceInfo::from_games(&refs);

        assert_eq!(info.total_games(), 8, "every game still counts toward the averages");
        assert_eq!(info.rated_win_rate(), Some(50.0));
        assert_eq!(info.records().rated(), Record { wins: 2, losses: 2, draws: 0 });
        assert_eq!(info.records().other, Record { wins: 3, losses: 1, draws: 0 });
        assert_eq!(ship_header("Yamato", &info, None, None), "Yamato 2W/2L (50%)");
    }

    /// Each rating aggregate over a mixed set is the one its rated games give
    /// alone, and differs from what every game together would give.
    #[test]
    fn ratings_are_computed_over_the_rated_games_alone() {
        let table = fixture_table();
        let games = mixed();
        let all: Vec<&PerGameStat> = games.iter().collect();
        let rated = rated_subset(&games);

        let session = session_personal_rating(&all, Some(&table)).expect("the rated games rate");
        assert_eq!(Some(session.clone()), session_personal_rating(&rated, Some(&table)));
        let every_game = table
            .calculate_pr(&[ShipBattleStats {
                ship_id: RATED_SHIP.into(),
                battles: all.len() as u32,
                damage: all.iter().map(|game| game.damage).sum(),
                wins: all.iter().filter(|game| game.is_win).count() as u32,
                frags: all.iter().map(|game| game.frags).sum(),
            }])
            .expect("the fixture rates this ship");
        assert!((session.pr - every_game.pr).abs() > 1.0, "the other modes are excluded, not counted");

        assert_eq!(PrStats::from_games(&all, &table), PrStats::from_games(&rated, &table));
        assert_eq!(
            PerformanceInfo::from_games(&all).personal_rating(&table),
            PerformanceInfo::from_games(&rated).personal_rating(&table)
        );
        assert_eq!(all[1].personal_rating(Some(&table)), None, "a co-op game carries no rating of its own");
    }

    #[test]
    fn charts_plot_rating_and_win_rate_over_the_rated_games_alone() {
        let table = fixture_table();
        let games = mixed();
        let all: Vec<&PerGameStat> = games.iter().collect();
        let rated = rated_subset(&games);
        let ships = [RATED_SHIP.into()];

        for stat in [ChartableStat::PersonalRating, ChartableStat::WinRate] {
            let mixed_lines = line_chart_series(&all, stat, &ships, Some(&table), true, true);
            let rated_lines = line_chart_series(&rated, stat, &ships, Some(&table), true, true);
            assert_eq!(mixed_lines, rated_lines, "{stat:?}");
            assert_eq!(mixed_lines[0].points.len(), rated.len(), "{stat:?}: one point per rated game");

            let bars = bar_chart_series(&per_ship_performance_by_id(&all), stat, &ships, Some(&table));
            let rated_bars = bar_chart_series(&per_ship_performance_by_id(&rated), stat, &ships, Some(&table));
            assert_eq!(bars, rated_bars, "{stat:?}");
        }
    }

    /// With no Random or Ranked game there is no win rate or rating to show,
    /// rather than a zero.
    #[test]
    fn a_set_of_only_other_modes_has_no_win_rate_and_no_rating() {
        let table = fixture_table();
        let games = coop_only();
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = [RATED_SHIP.into()];

        let summary = SessionSummary::from_games(&refs);
        assert_eq!(summary.records.rated().win_rate(), None);
        assert_eq!(session_personal_rating(&refs, Some(&table)), None);
        assert_eq!(PrStats::from_games(&refs, &table), None);

        let info = PerformanceInfo::from_games(&refs);
        assert_eq!(info.rated_win_rate(), None);
        assert_eq!(info.personal_rating(&table), None);
        assert_eq!(ship_header("Yamato", &info, None, None), "Yamato", "no record or rate for unrated battles");

        for stat in [ChartableStat::PersonalRating, ChartableStat::WinRate] {
            assert!(line_chart_series(&refs, stat, &ships, Some(&table), true, true).is_empty(), "{stat:?}");
            let bars = bar_chart_series(&per_ship_performance_by_id(&refs), stat, &ships, Some(&table));
            assert!(bars.is_empty(), "{stat:?}");
        }

        insta::assert_snapshot!(summary.records.hover_text());
    }
}
