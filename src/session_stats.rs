use std::collections::HashMap;
use std::collections::HashSet;

use serde::Deserialize;
use serde::Serialize;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::types::GameParamId;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::personal_rating::PersonalRatingData;
use crate::personal_rating::PersonalRatingResult;
use crate::personal_rating::ShipBattleStats;
use crate::tab_state::ChartableStat;
use crate::ui::replay_parser::Replay;

/// Division filter for session stats.
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DivisionFilter {
    #[default]
    All,
    SoloOnly,
    DivOnly,
}

/// A serializable achievement snapshot for session persistence.
#[derive(Clone, Serialize, Deserialize)]
pub struct SerializableAchievement {
    pub game_param_id: GameParamId,
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub count: usize,
}

/// Human-readable display name for a match group string.
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

/// Parse the replay `dateTime` format (`DD.MM.YYYY HH:MM:SS`) into a
/// lexicographically sortable string (`YYYY-MM-DD HH:MM:SS`).
/// Falls back to the original string if parsing fails.
fn sortable_game_time(game_time: &str) -> String {
    // Expected format: "DD.MM.YYYY HH:MM:SS"
    let parts: Vec<&str> = game_time.splitn(2, ' ').collect();
    if parts.len() == 2 {
        let date_parts: Vec<&str> = parts[0].split('.').collect();
        if date_parts.len() == 3 {
            return format!("{}-{}-{} {}", date_parts[2], date_parts[1], date_parts[0], parts[1]);
        }
    }
    game_time.to_string()
}

/// Per-game statistics extracted from a single replay
#[derive(Clone, Serialize, Deserialize)]
pub struct PerGameStat {
    pub ship_name: String,
    pub ship_id: GameParamId,
    pub game_time: String,
    /// Lexicographically sortable version of `game_time` (YYYY-MM-DD HH:MM:SS).
    #[serde(default)]
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
    /// The match group string from the replay metadata (e.g. "pvp", "ranked", "cooperative").
    #[serde(default)]
    pub match_group: String,
    #[serde(default)]
    pub achievements: Vec<SerializableAchievement>,
}

impl PerGameStat {
    /// Create a PerGameStat from a replay
    pub fn from_replay(replay: &Replay, metadata_provider: &GameMetadataProvider) -> Option<Self> {
        if replay.battle_results_are_pending() {
            return None;
        }

        let ui_report = replay.ui_report.as_ref()?;
        let self_report = ui_report.player_reports().iter().find(|r| r.relation().is_self())?;
        let ship_name = replay.vehicle_name(metadata_provider);
        let ship_id = replay.player_vehicle()?.shipId;
        let game_time = replay.game_time().to_string();
        let sort_key = sortable_game_time(&game_time);
        let player_id = replay.replay_file.meta.playerID.raw();
        let battle_result = replay.battle_result();
        let is_div = self_report.division_label().is_some();
        let match_group = replay.replay_file.meta.matchGroup.clone();

        let achievements = self_report
            .achievements
            .iter()
            .map(|a| SerializableAchievement {
                game_param_id: a.game_param.id(),
                display_name: a.display_name.clone(),
                description: a.description.clone(),
                icon_key: a.icon_key.clone(),
                count: a.count,
            })
            .collect();

        Some(PerGameStat {
            ship_name,
            ship_id,
            game_time,
            sort_key,
            player_id,
            damage: self_report.actual_damage().unwrap_or_default(),
            spotting_damage: self_report.spotting_damage().unwrap_or_default(),
            frags: self_report.kills().unwrap_or_default(),
            raw_xp: self_report.raw_xp().unwrap_or_default(),
            base_xp: self_report.base_xp().unwrap_or_default(),
            is_win: matches!(battle_result, Some(BattleResult::Win(_))),
            is_loss: matches!(battle_result, Some(BattleResult::Loss(_))),
            is_draw: matches!(battle_result, Some(BattleResult::Draw)),
            is_div,
            match_group,
            achievements,
        })
    }

    /// Get the value of a specific stat for charting
    pub fn get_stat(&self, stat: ChartableStat, pr_data: Option<&PersonalRatingData>) -> f64 {
        match stat {
            ChartableStat::Damage => self.damage as f64,
            ChartableStat::SpottingDamage => self.spotting_damage as f64,
            ChartableStat::Frags => self.frags as f64,
            ChartableStat::RawXp => self.raw_xp as f64,
            ChartableStat::BaseXp => self.base_xp as f64,
            ChartableStat::WinRate => 0.0, // Win rate doesn't make sense per-game
            ChartableStat::PersonalRating => self.calculate_pr(pr_data).unwrap_or(0.0),
        }
    }

    /// Calculate Personal Rating for this single game
    pub fn calculate_pr(&self, pr_data: Option<&PersonalRatingData>) -> Option<f64> {
        let pr_data = pr_data?;
        let stats = ShipBattleStats {
            ship_id: self.ship_id,
            battles: 1,
            damage: self.damage,
            wins: if self.is_win { 1 } else { 0 },
            frags: self.frags,
        };
        pr_data.calculate_pr(&[stats]).map(|r| r.pr)
    }
}

/// Performance statistics for a single ship aggregated from multiple games
#[derive(Default)]
pub struct PerformanceInfo {
    ship_id: Option<GameParamId>,
    wins: usize,
    losses: usize,
    draws: usize,
    total_frags: i64,
    max_frags: i64,
    total_damage: u64,
    max_damage: u64,
    total_games: usize,
    max_xp: i64,
    max_win_adjusted_xp: i64,
    total_xp: i64,
    total_win_adjusted_xp: i64,
    max_spotting_damage: u64,
    total_spotting_damage: u64,
    /// The `game_time` of the most recent game for this ship.
    last_played: String,
}

impl PerformanceInfo {
    /// Create a PerformanceInfo by aggregating multiple PerGameStat instances
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
            info.max_frags = info.max_frags.max(game.frags);

            info.total_damage += game.damage;
            info.max_damage = info.max_damage.max(game.damage);

            info.total_spotting_damage += game.spotting_damage;
            info.max_spotting_damage = info.max_spotting_damage.max(game.spotting_damage);

            info.total_xp += game.raw_xp;
            info.max_xp = info.max_xp.max(game.raw_xp);

            info.total_win_adjusted_xp += game.base_xp;
            info.max_win_adjusted_xp = info.max_win_adjusted_xp.max(game.base_xp);

            info.total_games += 1;

            if game.sort_key > info.last_played {
                info.last_played = game.sort_key.clone();
            }
        }

        info
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

    pub fn last_played(&self) -> &str {
        &self.last_played
    }

    pub fn win_rate(&self) -> Option<f64> {
        if self.total_games == 0 {
            return None;
        }

        Some(self.wins as f64 / self.total_games as f64 * 100.0)
    }

    pub fn total_frags(&self) -> i64 {
        self.total_frags
    }

    pub fn max_frags(&self) -> i64 {
        self.max_frags
    }

    pub fn avg_frags(&self) -> Option<f64> {
        if self.total_games == 0 {
            return None;
        }
        Some(self.total_frags as f64 / self.total_games as f64)
    }

    pub fn max_damage(&self) -> u64 {
        self.max_damage
    }

    pub fn avg_damage(&self) -> Option<f64> {
        if self.total_games == 0 {
            return None;
        }
        Some(self.total_damage as f64 / self.total_games as f64)
    }

    pub fn max_spotting_damage(&self) -> u64 {
        self.max_spotting_damage
    }

    pub fn avg_spotting_damage(&self) -> Option<f64> {
        if self.total_games == 0 {
            return None;
        }
        Some(self.total_spotting_damage as f64 / self.total_games as f64)
    }

    pub fn max_xp(&self) -> i64 {
        self.max_xp
    }

    pub fn avg_xp(&self) -> Option<f64> {
        if self.total_games == 0 {
            return None;
        }
        Some(self.total_xp as f64 / self.total_games as f64)
    }

    pub fn max_win_adjusted_xp(&self) -> i64 {
        self.max_win_adjusted_xp
    }

    pub fn avg_win_adjusted_xp(&self) -> Option<f64> {
        if self.total_games == 0 {
            return None;
        }
        Some(self.total_win_adjusted_xp as f64 / self.total_games as f64)
    }

    /// Calculate Personal Rating for this ship's performance
    pub fn calculate_pr(&self, pr_data: &PersonalRatingData) -> Option<PersonalRatingResult> {
        let ship_id = self.ship_id?;
        let stats = ShipBattleStats {
            ship_id,
            battles: self.total_games as u32,
            damage: self.total_damage,
            wins: self.wins as u32,
            frags: self.total_frags,
        };
        pr_data.calculate_pr(&[stats])
    }
}

/// Aggregated session statistics across multiple replays
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub games: Vec<PerGameStat>,
    /// If set, stats only reflect the N most recent games.
    #[serde(skip)]
    pub game_count_limit: Option<usize>,
    /// Division filter for stats display.
    #[serde(skip)]
    pub division_filter: DivisionFilter,
    /// Game mode filter — if set, only show games whose `match_group` is in this set.
    /// Empty means no filter (show all).
    #[serde(skip)]
    pub game_mode_filter: HashSet<String>,
}

impl SessionStats {
    pub fn clear(&mut self) {
        self.games.clear();
    }

    /// Get all unique match group strings from the session's games.
    pub fn all_match_groups(&self) -> Vec<String> {
        let mut groups: Vec<String> =
            self.games.iter().map(|g| g.match_group.clone()).collect::<HashSet<_>>().into_iter().collect();
        groups.sort();
        groups
    }

    /// Add a game to the session. Deduplicates on game_time + player_id,
    /// always preferring the newer entry (which may have battle results).
    pub fn add_game(&mut self, mut stat: PerGameStat) {
        // Backfill sort_key for legacy data missing it
        if stat.sort_key.is_empty() {
            stat.sort_key = sortable_game_time(&stat.game_time);
        }

        let old_index = self
            .games
            .iter()
            .position(|existing| existing.game_time == stat.game_time && existing.player_id == stat.player_id);

        if let Some(old_index) = old_index {
            self.games.remove(old_index);
        }

        self.games.push(stat);
        self.sort_games();
    }

    /// Ensure all games are sorted chronologically and have valid sort keys.
    /// Should be called after deserialization to fix legacy data with missing sort keys.
    pub fn sort_games(&mut self) {
        // Backfill any empty sort_keys (from legacy serialized data)
        for game in &mut self.games {
            if game.sort_key.is_empty() {
                game.sort_key = sortable_game_time(&game.game_time);
            }
        }
        self.games.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    }

    /// Return the most recent N games (based on `game_count_limit`), or all if unset.
    pub fn recent_games(&self) -> &[PerGameStat] {
        match self.game_count_limit {
            Some(n) if n < self.games.len() => &self.games[self.games.len() - n..],
            _ => &self.games,
        }
    }

    /// Return recent games filtered by division filter and game mode filter.
    pub fn filtered_games(&self) -> Vec<&PerGameStat> {
        self.recent_games()
            .iter()
            .filter(|g| match self.division_filter {
                DivisionFilter::All => true,
                DivisionFilter::SoloOnly => !g.is_div,
                DivisionFilter::DivOnly => g.is_div,
            })
            .filter(|g| self.game_mode_filter.is_empty() || self.game_mode_filter.contains(&g.match_group))
            .collect()
    }

    /// Get per-game stats with the count limit applied per-ship rather than overall.
    /// For each ship, takes the last N games (where N = game_count_limit) from the
    /// filtered set. If no limit is set, returns all filtered games.
    pub fn per_ship_limited_games(&self) -> Vec<&PerGameStat> {
        // Apply division + game mode filters on ALL games (no global count limit)
        let all_filtered: Vec<&PerGameStat> = self
            .games
            .iter()
            .filter(|g| match self.division_filter {
                DivisionFilter::All => true,
                DivisionFilter::SoloOnly => !g.is_div,
                DivisionFilter::DivOnly => g.is_div,
            })
            .filter(|g| self.game_mode_filter.is_empty() || self.game_mode_filter.contains(&g.match_group))
            .collect();

        let Some(limit) = self.game_count_limit else {
            return all_filtered;
        };

        // Group by ship, take last N per ship, then merge back in chronological order
        let mut by_ship: HashMap<&str, Vec<&PerGameStat>> = HashMap::new();
        for game in &all_filtered {
            by_ship.entry(game.ship_name.as_str()).or_default().push(game);
        }

        // Each ship's games are already in chronological order; take the tail
        let mut kept: HashSet<*const PerGameStat> = HashSet::new();
        for games in by_ship.values() {
            let start = games.len().saturating_sub(limit);
            for game in &games[start..] {
                kept.insert(*game as *const PerGameStat);
            }
        }

        // Return in original chronological order
        all_filtered.into_iter().filter(|g| kept.contains(&(*g as *const PerGameStat))).collect()
    }

    /// Get aggregated ship statistics using per-ship count limits.
    pub fn ship_stats_per_ship_limited(&self) -> HashMap<String, PerformanceInfo> {
        let per_game = self.per_ship_limited_games();

        let mut by_ship: HashMap<String, Vec<&PerGameStat>> = HashMap::new();
        for game in &per_game {
            by_ship.entry(game.ship_name.clone()).or_default().push(game);
        }

        by_ship.into_iter().map(|(name, games)| (name, PerformanceInfo::from_games(&games))).collect()
    }

    /// Returns the win rate percentage for this session. Will return `None`
    /// if no games with results have been played.
    pub fn win_rate(&self) -> Option<f64> {
        let played = self.games_played();
        if played == 0 {
            return None;
        }

        Some((self.games_won() as f64 / played as f64) * 100.0)
    }

    /// Total number of games with a result (win, loss, or draw) in the current session
    pub fn games_played(&self) -> usize {
        self.filtered_games().iter().filter(|g| g.is_win || g.is_loss || g.is_draw).count()
    }

    /// Total number of games won in the current session
    pub fn games_won(&self) -> usize {
        self.filtered_games().iter().filter(|g| g.is_win).count()
    }

    /// Total number of games lost in the current session
    pub fn games_lost(&self) -> usize {
        self.filtered_games().iter().filter(|g| g.is_loss).count()
    }

    /// Total number of games drawn in the current session
    pub fn games_drawn(&self) -> usize {
        self.filtered_games().iter().filter(|g| g.is_draw).count()
    }

    pub fn max_damage(&self) -> Option<(String, u64)> {
        self.filtered_games().into_iter().map(|g| (g.ship_name.clone(), g.damage)).max_by_key(|r| r.1)
    }

    pub fn max_frags(&self) -> Option<(String, i64)> {
        self.filtered_games().into_iter().map(|g| (g.ship_name.clone(), g.frags)).max_by_key(|r| r.1)
    }

    pub fn total_frags(&self) -> i64 {
        self.filtered_games().iter().map(|g| g.frags).sum()
    }

    /// Calculate overall Personal Rating for this session
    pub fn calculate_pr(&self, pr_data: &PersonalRatingData) -> Option<PersonalRatingResult> {
        // Group stats by ship_id for proper PR calculation
        let mut ship_stats: HashMap<GameParamId, ShipBattleStats> = HashMap::new();

        for game in self.filtered_games() {
            let entry = ship_stats.entry(game.ship_id).or_insert(ShipBattleStats {
                ship_id: game.ship_id,
                battles: 0,
                damage: 0,
                wins: 0,
                frags: 0,
            });
            entry.battles += 1;
            entry.damage += game.damage;
            entry.wins += if game.is_win { 1 } else { 0 };
            entry.frags += game.frags;
        }

        let stats: Vec<_> = ship_stats.into_values().collect();
        pr_data.calculate_pr(&stats)
    }

    /// Calculate Personal Rating per ship for this session
    /// Returns a map of ship_id -> PR result
    #[allow(dead_code)]
    pub fn calculate_pr_per_ship(&self, pr_data: &PersonalRatingData) -> HashMap<GameParamId, PersonalRatingResult> {
        // Group stats by ship_id
        let mut ship_stats: HashMap<GameParamId, ShipBattleStats> = HashMap::new();

        for game in self.filtered_games() {
            let entry = ship_stats.entry(game.ship_id).or_insert(ShipBattleStats {
                ship_id: game.ship_id,
                battles: 0,
                damage: 0,
                wins: 0,
                frags: 0,
            });
            entry.battles += 1;
            entry.damage += game.damage;
            entry.wins += if game.is_win { 1 } else { 0 };
            entry.frags += game.frags;
        }

        // Calculate PR for each ship
        ship_stats
            .into_iter()
            .filter_map(|(ship_id, stats)| {
                let pr = pr_data.calculate_pr(&[stats])?;
                Some((ship_id, pr))
            })
            .collect()
    }
}
