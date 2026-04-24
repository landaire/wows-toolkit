use std::collections::HashMap;
use std::collections::HashSet;

use serde::Deserialize;
use serde::Serialize;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::types::GameParamId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::tab_state::ChartableStat;
use crate::ui::replay_parser::Replay;
use crate::util::personal_rating::PersonalRatingData;
use crate::util::personal_rating::PersonalRatingResult;
use crate::util::personal_rating::ShipBattleStats;

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

impl SerializableAchievement {
    /// Resolve display name dynamically from the current locale, falling back
    /// to the persisted `display_name`.
    pub fn resolved_name(&self, provider: Option<&dyn ResourceLoader>) -> String {
        if let Some(provider) = provider
            && let Some(name) =
                wowsunpack::game_params::translations::translate_achievement_name(&self.icon_key, provider)
        {
            return name;
        }
        self.display_name.clone()
    }

    /// Resolve description dynamically from the current locale, falling back
    /// to the persisted `description`.
    pub fn resolved_description(&self, provider: Option<&dyn ResourceLoader>) -> String {
        if let Some(provider) = provider
            && let Some(desc) =
                wowsunpack::game_params::translations::translate_achievement_description(&self.icon_key, provider)
        {
            return desc;
        }
        self.description.clone()
    }
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

/// Stats for a single allied (non-bot) player in one game.
#[derive(Clone, Serialize, Deserialize)]
pub struct TeamMemberGameStat {
    pub username: String,
    pub ship_name: String,
    pub ship_id: GameParamId,
    pub damage: Option<u64>,
    pub spotting_damage: Option<u64>,
    pub frags: Option<i64>,
    pub raw_xp: Option<i64>,
    pub base_xp: Option<i64>,
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
    /// Stats for all non-bot allied players in this game.
    #[serde(default)]
    pub team_members: Vec<TeamMemberGameStat>,
    /// Username of the local player (empty on data loaded before this field was added).
    #[serde(default)]
    pub player_name: String,
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

        let player_name = self_report.player().initial_state().username().to_string();
        let team_members: Vec<TeamMemberGameStat> = ui_report
            .player_reports()
            .iter()
            .filter(|r| r.relation().is_ally() && !r.player().is_bot())
            .map(|r| TeamMemberGameStat {
                username: r.player().initial_state().username().to_string(),
                ship_name: r.ship_name().to_string(),
                ship_id: r.player().vehicle().id(),
                damage: r.actual_damage(),
                spotting_damage: r.spotting_damage(),
                frags: r.kills(),
                raw_xp: r.raw_xp(),
                base_xp: r.base_xp(),
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
            team_members,
            player_name,
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
    min_frags: i64,
    max_spotting_damage: u64,
    min_spotting_damage: u64,
    total_spotting_damage: u64,
    min_xp: i64,
    min_win_adjusted_xp: i64,
    min_damage: u64,
    /// The `game_time` of the most recent game for this ship.
    last_played: String,
}

impl PerformanceInfo {
    /// Create a PerformanceInfo by aggregating multiple PerGameStat instances
    pub fn from_games(games: &[&PerGameStat]) -> Self {
        let mut info = PerformanceInfo {
            min_frags: i64::MAX,
            min_damage: u64::MAX,
            min_spotting_damage: u64::MAX,
            min_xp: i64::MAX,
            min_win_adjusted_xp: i64::MAX,
            ..Default::default()
        };

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
            info.min_frags = info.min_frags.min(game.frags);

            info.total_damage += game.damage;
            info.max_damage = info.max_damage.max(game.damage);
            info.min_damage = info.min_damage.min(game.damage);

            info.total_spotting_damage += game.spotting_damage;
            info.max_spotting_damage = info.max_spotting_damage.max(game.spotting_damage);
            info.min_spotting_damage = info.min_spotting_damage.min(game.spotting_damage);

            info.total_xp += game.raw_xp;
            info.max_xp = info.max_xp.max(game.raw_xp);
            info.min_xp = info.min_xp.min(game.raw_xp);

            info.total_win_adjusted_xp += game.base_xp;
            info.max_win_adjusted_xp = info.max_win_adjusted_xp.max(game.base_xp);
            info.min_win_adjusted_xp = info.min_win_adjusted_xp.min(game.base_xp);

            info.total_games += 1;

            if game.sort_key > info.last_played {
                info.last_played = game.sort_key.clone();
            }
        }

        // Reset mins to 0 if no games were processed
        if info.total_games == 0 {
            info.min_frags = 0;
            info.min_damage = 0;
            info.min_spotting_damage = 0;
            info.min_xp = 0;
            info.min_win_adjusted_xp = 0;
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

    pub fn min_damage(&self) -> u64 {
        self.min_damage
    }

    pub fn total_damage(&self) -> u64 {
        self.total_damage
    }

    pub fn min_spotting_damage(&self) -> u64 {
        self.min_spotting_damage
    }

    pub fn total_spotting_damage(&self) -> u64 {
        self.total_spotting_damage
    }

    pub fn min_frags(&self) -> i64 {
        self.min_frags
    }

    pub fn min_xp(&self) -> i64 {
        self.min_xp
    }

    pub fn total_xp(&self) -> i64 {
        self.total_xp
    }

    pub fn min_win_adjusted_xp(&self) -> i64 {
        self.min_win_adjusted_xp
    }

    pub fn total_win_adjusted_xp(&self) -> i64 {
        self.total_win_adjusted_xp
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

/// Min/max/average Personal Rating computed from individual games.
pub struct PrStats {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
}

impl PrStats {
    /// Compute PR stats from a set of per-game stats.
    /// `avg` is the aggregate PR (from totals), matching the header PR formula.
    pub fn from_games(games: &[&PerGameStat], pr_data: &PersonalRatingData) -> Option<Self> {
        let prs: Vec<f64> = games.iter().filter_map(|g| g.calculate_pr(Some(pr_data))).collect();
        if prs.is_empty() {
            return None;
        }
        let min = prs.iter().copied().fold(f64::INFINITY, f64::min);
        let max = prs.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        // Aggregate PR from totals (same formula as PerformanceInfo::calculate_pr)
        let first = games.first()?;
        let stats = ShipBattleStats {
            ship_id: first.ship_id,
            battles: games.len() as u32,
            damage: games.iter().map(|g| g.damage).sum(),
            wins: games.iter().filter(|g| g.is_win).count() as u32,
            frags: games.iter().map(|g| g.frags).sum(),
        };
        let avg = pr_data.calculate_pr(&[stats])?.pr;

        Some(PrStats { min, max, avg })
    }
}

/// Resolve a ship's display name from the provider, falling back to ID.
pub fn resolve_ship_name(ship_id: GameParamId, provider: Option<&GameMetadataProvider>) -> String {
    if let Some(provider) = provider
        && let Some(param) = provider.game_param_by_id(ship_id)
        && let Some(name) = provider.localized_name_from_param(&param)
    {
        return name;
    }
    format!("[{ship_id}]")
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

    /// Remove all games for a specific ship by ID.
    pub fn clear_ship(&mut self, ship_id: GameParamId) {
        self.games.retain(|g| g.ship_id != ship_id);
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
        let mut by_ship: HashMap<GameParamId, Vec<&PerGameStat>> = HashMap::new();
        for game in &all_filtered {
            by_ship.entry(game.ship_id).or_default().push(game);
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
    pub fn ship_stats_per_ship_limited(&self) -> HashMap<GameParamId, PerformanceInfo> {
        let per_game = self.per_ship_limited_games();

        let mut by_ship: HashMap<GameParamId, Vec<&PerGameStat>> = HashMap::new();
        for game in &per_game {
            by_ship.entry(game.ship_id).or_default().push(game);
        }

        by_ship.into_iter().map(|(id, games)| (id, PerformanceInfo::from_games(&games))).collect()
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

    pub fn max_damage(&self) -> Option<(GameParamId, u64)> {
        self.filtered_games().into_iter().map(|g| (g.ship_id, g.damage)).max_by_key(|r| r.1)
    }

    pub fn max_frags(&self) -> Option<(GameParamId, i64)> {
        self.filtered_games().into_iter().map(|g| (g.ship_id, g.frags)).max_by_key(|r| r.1)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// Helper: create a PerGameStat with the given parameters.
    fn make_game(
        ship_name: &str,
        ship_id: u64,
        game_time: &str,
        player_id: i64,
        damage: u64,
        frags: i64,
        raw_xp: i64,
        base_xp: i64,
        is_win: bool,
        is_loss: bool,
        is_draw: bool,
        is_div: bool,
        match_group: &str,
    ) -> PerGameStat {
        let sort_key = sortable_game_time(game_time);
        PerGameStat {
            ship_name: ship_name.to_string(),
            ship_id: GameParamId::from(ship_id),
            game_time: game_time.to_string(),
            sort_key,
            player_id,
            damage,
            spotting_damage: 0,
            frags,
            raw_xp,
            base_xp,
            is_win,
            is_loss,
            is_draw,
            is_div,
            match_group: match_group.to_string(),
            achievements: Vec::new(),
            team_members: Vec::new(),
            player_name: String::new(),
        }
    }

    /// Shorthand for a PvP win.
    fn pvp_win(ship: &str, ship_id: u64, time: &str, damage: u64, frags: i64, xp: i64) -> PerGameStat {
        make_game(ship, ship_id, time, 1, damage, frags, xp, xp, true, false, false, false, "pvp")
    }

    /// Shorthand for a PvP loss.
    fn pvp_loss(ship: &str, ship_id: u64, time: &str, damage: u64, frags: i64, xp: i64) -> PerGameStat {
        make_game(ship, ship_id, time, 1, damage, frags, xp, xp, false, true, false, false, "pvp")
    }

    fn fixture_pr_data() -> PersonalRatingData {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests")
            .join("fixtures")
            .join("pr_expected_values.json");
        let bytes = std::fs::read(&path).expect("fixture must exist");
        let mut pr = PersonalRatingData::new();
        pr.load_from_bytes(&bytes).unwrap();
        pr
    }

    // -- sortable_game_time --

    #[test]
    fn sortable_game_time_standard_format() {
        assert_eq!(sortable_game_time("13.02.2026 14:35:18"), "2026-02-13 14:35:18");
    }

    #[test]
    fn sortable_game_time_single_digit_day() {
        assert_eq!(sortable_game_time("01.01.2025 00:00:00"), "2025-01-01 00:00:00");
    }

    #[test]
    fn sortable_game_time_invalid_format_passthrough() {
        let bad = "not a date";
        assert_eq!(sortable_game_time(bad), bad);
    }

    #[test]
    fn sortable_game_time_sorts_correctly() {
        let early = sortable_game_time("01.01.2025 08:00:00");
        let late = sortable_game_time("13.02.2026 14:35:18");
        assert!(early < late);
    }

    // -- match_group_display_name --

    #[test]
    fn match_group_display_names() {
        assert_eq!(match_group_display_name("pvp"), "Random");
        assert_eq!(match_group_display_name("ranked"), "Ranked");
        assert_eq!(match_group_display_name("cooperative"), "Co-op");
        assert_eq!(match_group_display_name("clan"), "Clan Battle");
        assert_eq!(match_group_display_name("brawl"), "Brawl");
        assert_eq!(match_group_display_name("event"), "Event");
        assert_eq!(match_group_display_name("pve"), "PvE");
        assert_eq!(match_group_display_name(""), "Unknown");
        assert_eq!(match_group_display_name("some_future_mode"), "some_future_mode");
    }

    // -- PerformanceInfo --

    #[test]
    fn performance_info_from_empty_games() {
        let info = PerformanceInfo::from_games(&[]);
        assert_eq!(info.wins(), 0);
        assert_eq!(info.losses(), 0);
        assert_eq!(info.draws(), 0);
        assert_eq!(info.total_damage(), 0);
        assert_eq!(info.min_damage(), 0);
        assert!(info.win_rate().is_none());
        assert!(info.avg_damage().is_none());
    }

    #[test]
    fn performance_info_single_game() {
        let game = pvp_win("Vermont", 3374266064, "13.02.2026 14:00:00", 120000, 3, 2500);
        let info = PerformanceInfo::from_games(&[&game]);

        assert_eq!(info.wins(), 1);
        assert_eq!(info.losses(), 0);
        assert_eq!(info.draws(), 0);
        assert_eq!(info.total_damage(), 120000);
        assert_eq!(info.max_damage(), 120000);
        assert_eq!(info.min_damage(), 120000);
        assert_eq!(info.total_frags(), 3);
        assert_eq!(info.max_frags(), 3);
        assert_eq!(info.min_frags(), 3);
        assert_eq!(info.max_xp(), 2500);
        assert_eq!(info.min_xp(), 2500);
        assert!((info.win_rate().unwrap() - 100.0).abs() < f64::EPSILON);
        assert!((info.avg_damage().unwrap() - 120000.0).abs() < f64::EPSILON);
        assert!((info.avg_frags().unwrap() - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn performance_info_multi_game_aggregation() {
        let g1 = pvp_win("Vermont", 3374266064, "13.02.2026 14:00:00", 100000, 2, 2000);
        let g2 = pvp_loss("Vermont", 3374266064, "13.02.2026 15:00:00", 50000, 0, 1000);
        let g3 = pvp_win("Vermont", 3374266064, "13.02.2026 16:00:00", 200000, 5, 3000);
        let info = PerformanceInfo::from_games(&[&g1, &g2, &g3]);

        assert_eq!(info.wins(), 2);
        assert_eq!(info.losses(), 1);
        assert_eq!(info.total_damage(), 350000);
        assert_eq!(info.max_damage(), 200000);
        assert_eq!(info.min_damage(), 50000);
        assert_eq!(info.total_frags(), 7);
        assert_eq!(info.max_frags(), 5);
        assert_eq!(info.min_frags(), 0);
        assert_eq!(info.max_xp(), 3000);
        assert_eq!(info.min_xp(), 1000);
        assert!((info.win_rate().unwrap() - 66.66666666666667).abs() < 0.01);
        assert!((info.avg_damage().unwrap() - 116666.666666).abs() < 1.0);
    }

    #[test]
    fn performance_info_last_played() {
        let g1 = pvp_win("Vermont", 1, "01.01.2025 10:00:00", 100000, 1, 1000);
        let g2 = pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 1, 1000);
        let info = PerformanceInfo::from_games(&[&g1, &g2]);
        assert_eq!(info.last_played(), "2026-02-13 14:00:00");
    }

    #[test]
    fn performance_info_draw_counted() {
        let game =
            make_game("Vermont", 1, "01.01.2025 10:00:00", 1, 50000, 1, 1000, 1000, false, false, true, false, "pvp");
        let info = PerformanceInfo::from_games(&[&game]);
        assert_eq!(info.draws(), 1);
        assert_eq!(info.wins(), 0);
        assert_eq!(info.losses(), 0);
        assert!((info.win_rate().unwrap() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn performance_info_calculate_pr() {
        let pr = fixture_pr_data();
        let ev = pr.get_ship_expected(GameParamId::from(3374266064u64)).unwrap();

        let game = make_game(
            "TestShip",
            3374266064,
            "13.02.2026 14:00:00",
            1,
            ev.average_damage_dealt as u64,
            ev.average_frags as i64,
            2000,
            2000,
            true,
            false,
            false,
            false,
            "pvp",
        );
        let info = PerformanceInfo::from_games(&[&game]);
        let result = info.calculate_pr(&pr);
        assert!(result.is_some(), "should calculate PR for performance info");
    }

    // -- SessionStats --

    #[test]
    fn session_stats_add_game_and_count() {
        let mut ss = SessionStats::default();
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 2, 2000));
        ss.add_game(pvp_loss("Marceau", 2, "13.02.2026 15:00:00", 80000, 1, 1500));
        assert_eq!(ss.games.len(), 2);
        assert_eq!(ss.games_played(), 2);
        assert_eq!(ss.games_won(), 1);
        assert_eq!(ss.games_lost(), 1);
    }

    #[test]
    fn session_stats_dedup_by_time_and_player() {
        let mut ss = SessionStats::default();
        let g1 =
            make_game("Vermont", 1, "13.02.2026 14:00:00", 42, 100000, 2, 2000, 2000, true, false, false, false, "pvp");
        let g2 =
            make_game("Vermont", 1, "13.02.2026 14:00:00", 42, 150000, 3, 2500, 2500, true, false, false, false, "pvp");
        ss.add_game(g1);
        ss.add_game(g2);
        assert_eq!(ss.games.len(), 1, "duplicate game_time+player_id should deduplicate");
        assert_eq!(ss.games[0].damage, 150000, "should keep the newer entry");
    }

    #[test]
    fn session_stats_different_players_not_deduped() {
        let mut ss = SessionStats::default();
        let g1 =
            make_game("Vermont", 1, "13.02.2026 14:00:00", 1, 100000, 2, 2000, 2000, true, false, false, false, "pvp");
        let g2 =
            make_game("Vermont", 1, "13.02.2026 14:00:00", 2, 100000, 2, 2000, 2000, true, false, false, false, "pvp");
        ss.add_game(g1);
        ss.add_game(g2);
        assert_eq!(ss.games.len(), 2, "different player_ids should not be deduped");
    }

    #[test]
    fn session_stats_win_rate() {
        let mut ss = SessionStats::default();
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 2, 2000));
        ss.add_game(pvp_loss("Vermont", 1, "13.02.2026 15:00:00", 80000, 0, 1000));
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 16:00:00", 120000, 3, 2500));
        assert!((ss.win_rate().unwrap() - 66.66666666666667).abs() < 0.01);
    }

    #[test]
    fn session_stats_win_rate_empty() {
        let ss = SessionStats::default();
        assert!(ss.win_rate().is_none());
    }

    #[test]
    fn session_stats_max_damage() {
        let mut ss = SessionStats::default();
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 2, 2000));
        ss.add_game(pvp_loss("Marceau", 2, "13.02.2026 15:00:00", 200000, 1, 1500));
        let (ship, dmg) = ss.max_damage().unwrap();
        assert_eq!(ship, GameParamId::from(2u64));
        assert_eq!(dmg, 200000);
    }

    #[test]
    fn session_stats_max_frags() {
        let mut ss = SessionStats::default();
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 5, 2000));
        ss.add_game(pvp_loss("Marceau", 2, "13.02.2026 15:00:00", 80000, 2, 1000));
        let (ship, frags) = ss.max_frags().unwrap();
        assert_eq!(ship, GameParamId::from(1u64));
        assert_eq!(frags, 5);
    }

    #[test]
    fn session_stats_total_frags() {
        let mut ss = SessionStats::default();
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 3, 2000));
        ss.add_game(pvp_loss("Marceau", 2, "13.02.2026 15:00:00", 80000, 2, 1000));
        assert_eq!(ss.total_frags(), 5);
    }

    #[test]
    fn session_stats_recent_games_limit() {
        let mut ss = SessionStats::default();
        for i in 0..10 {
            ss.add_game(pvp_win("Vermont", 1, &format!("13.02.2026 {:02}:00:00", i), 100000, 1, 1000));
        }
        assert_eq!(ss.recent_games().len(), 10);

        ss.game_count_limit = Some(3);
        let recent = ss.recent_games();
        assert_eq!(recent.len(), 3);
        // Should be the last 3 games (07, 08, 09)
        assert!(recent[0].game_time.contains("07:"));
        assert!(recent[1].game_time.contains("08:"));
        assert!(recent[2].game_time.contains("09:"));
    }

    #[test]
    fn session_stats_division_filter() {
        let mut ss = SessionStats::default();
        let solo =
            make_game("Vermont", 1, "13.02.2026 14:00:00", 1, 100000, 2, 2000, 2000, true, false, false, false, "pvp");
        let div =
            make_game("Marceau", 2, "13.02.2026 15:00:00", 1, 80000, 1, 1500, 1500, false, true, false, true, "pvp");
        ss.add_game(solo);
        ss.add_game(div);

        ss.division_filter = DivisionFilter::All;
        assert_eq!(ss.filtered_games().len(), 2);

        ss.division_filter = DivisionFilter::SoloOnly;
        let filtered = ss.filtered_games();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].ship_name, "Vermont");

        ss.division_filter = DivisionFilter::DivOnly;
        let filtered = ss.filtered_games();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].ship_name, "Marceau");
    }

    #[test]
    fn session_stats_game_mode_filter() {
        let mut ss = SessionStats::default();
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 2, 2000));
        ss.add_game(make_game(
            "Marceau",
            2,
            "13.02.2026 15:00:00",
            1,
            80000,
            1,
            1500,
            1500,
            false,
            true,
            false,
            false,
            "ranked",
        ));

        assert_eq!(ss.filtered_games().len(), 2);

        ss.game_mode_filter = HashSet::from(["pvp".to_string()]);
        let filtered = ss.filtered_games();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].ship_name, "Vermont");

        ss.game_mode_filter = HashSet::from(["ranked".to_string()]);
        let filtered = ss.filtered_games();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].ship_name, "Marceau");

        ss.game_mode_filter = HashSet::from(["pvp".to_string(), "ranked".to_string()]);
        assert_eq!(ss.filtered_games().len(), 2);
    }

    #[test]
    fn session_stats_all_match_groups() {
        let mut ss = SessionStats::default();
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 2, 2000));
        ss.add_game(make_game(
            "Marceau",
            2,
            "13.02.2026 15:00:00",
            1,
            80000,
            1,
            1500,
            1500,
            true,
            false,
            false,
            false,
            "ranked",
        ));
        ss.add_game(pvp_loss("Shimakaze", 3, "13.02.2026 16:00:00", 60000, 0, 800));

        let groups = ss.all_match_groups();
        assert!(groups.contains(&"pvp".to_string()));
        assert!(groups.contains(&"ranked".to_string()));
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn session_stats_clear() {
        let mut ss = SessionStats::default();
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 2, 2000));
        assert_eq!(ss.games.len(), 1);
        ss.clear();
        assert_eq!(ss.games.len(), 0);
    }

    #[test]
    fn session_stats_clear_ship() {
        let mut ss = SessionStats::default();
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 2, 2000));
        ss.add_game(pvp_win("Marceau", 2, "13.02.2026 15:00:00", 80000, 1, 1500));
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 16:00:00", 120000, 3, 2500));

        ss.clear_ship(GameParamId::from(1u64));
        assert_eq!(ss.games.len(), 1);
        assert_eq!(ss.games[0].ship_name, "Marceau");
    }

    #[test]
    fn session_stats_sort_backfills_sort_key() {
        let mut ss = SessionStats::default();
        ss.games.push(PerGameStat {
            ship_name: "Vermont".to_string(),
            ship_id: GameParamId::from(1u64),
            game_time: "13.02.2026 14:00:00".to_string(),
            sort_key: String::new(),
            player_id: 1,
            damage: 100000,
            spotting_damage: 0,
            frags: 2,
            raw_xp: 2000,
            base_xp: 2000,
            is_win: true,
            is_loss: false,
            is_draw: false,
            is_div: false,
            match_group: "pvp".to_string(),
            achievements: Vec::new(),
            team_members: Vec::new(),
            player_name: String::new(),
        });
        ss.sort_games();
        assert_eq!(ss.games[0].sort_key, "2026-02-13 14:00:00");
    }

    #[test]
    fn session_stats_per_ship_limited_games() {
        let mut ss = SessionStats::default();
        for i in 0..5 {
            ss.add_game(pvp_win("Vermont", 1, &format!("13.02.2026 {:02}:00:00", i), 100000, 1, 1000));
        }
        for i in 5..8 {
            ss.add_game(pvp_win("Marceau", 2, &format!("13.02.2026 {:02}:00:00", i), 80000, 1, 1000));
        }

        assert_eq!(ss.per_ship_limited_games().len(), 8);

        ss.game_count_limit = Some(2);
        let limited = ss.per_ship_limited_games();
        assert_eq!(limited.len(), 4);

        let vermont_games: Vec<_> = limited.iter().filter(|g| g.ship_name == "Vermont").collect();
        let marceau_games: Vec<_> = limited.iter().filter(|g| g.ship_name == "Marceau").collect();
        assert_eq!(vermont_games.len(), 2);
        assert_eq!(marceau_games.len(), 2);
    }

    #[test]
    fn session_stats_ship_stats_per_ship_limited() {
        let mut ss = SessionStats::default();
        ss.add_game(pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 2, 2000));
        ss.add_game(pvp_loss("Vermont", 1, "13.02.2026 15:00:00", 50000, 0, 1000));
        ss.add_game(pvp_win("Marceau", 2, "13.02.2026 16:00:00", 80000, 1, 1500));

        let stats = ss.ship_stats_per_ship_limited();
        assert_eq!(stats.len(), 2);
        let vermont_id = GameParamId::from(1u64);
        let marceau_id = GameParamId::from(2u64);
        assert!(stats.contains_key(&vermont_id));
        assert!(stats.contains_key(&marceau_id));

        let vermont = &stats[&vermont_id];
        assert_eq!(vermont.wins(), 1);
        assert_eq!(vermont.losses(), 1);
        assert_eq!(vermont.total_damage(), 150000);
    }

    #[test]
    fn session_stats_games_drawn() {
        let mut ss = SessionStats::default();
        ss.add_game(make_game(
            "Vermont",
            1,
            "13.02.2026 14:00:00",
            1,
            100000,
            0,
            1000,
            1000,
            false,
            false,
            true,
            false,
            "pvp",
        ));
        assert_eq!(ss.games_drawn(), 1);
        assert_eq!(ss.games_won(), 0);
        assert_eq!(ss.games_lost(), 0);
        assert_eq!(ss.games_played(), 1);
    }

    // -- Per-game PR --

    #[test]
    fn per_game_stat_calculate_pr() {
        let pr_data = fixture_pr_data();
        let ev = pr_data.get_ship_expected(GameParamId::from(3374266064u64)).unwrap();

        let game = make_game(
            "TestShip",
            3374266064,
            "13.02.2026 14:00:00",
            1,
            ev.average_damage_dealt as u64,
            ev.average_frags as i64,
            2000,
            2000,
            true,
            false,
            false,
            false,
            "pvp",
        );

        let pr = game.calculate_pr(Some(&pr_data));
        assert!(pr.is_some(), "should calculate per-game PR");
    }

    #[test]
    fn per_game_stat_calculate_pr_no_data() {
        let game = pvp_win("Vermont", 1, "13.02.2026 14:00:00", 100000, 2, 2000);
        assert!(game.calculate_pr(None).is_none());
    }

    // -- SessionStats PR --

    #[test]
    fn session_stats_calculate_pr() {
        let pr_data = fixture_pr_data();
        let ev = pr_data.get_ship_expected(GameParamId::from(3374266064u64)).unwrap();

        let mut ss = SessionStats::default();
        for i in 0..10 {
            let game = make_game(
                "TestShip",
                3374266064,
                &format!("13.02.2026 {:02}:00:00", i),
                1,
                ev.average_damage_dealt as u64,
                ev.average_frags as i64,
                2000,
                2000,
                i % 2 == 0,
                i % 2 != 0,
                false,
                false,
                "pvp",
            );
            ss.add_game(game);
        }

        let result = ss.calculate_pr(&pr_data);
        assert!(result.is_some(), "session PR should be calculable");
    }

    // -- PrStats --

    #[test]
    fn pr_stats_from_games() {
        let pr_data = fixture_pr_data();
        let ev = pr_data.get_ship_expected(GameParamId::from(3374266064u64)).unwrap();

        let g1 = make_game(
            "TestShip",
            3374266064,
            "13.02.2026 14:00:00",
            1,
            (ev.average_damage_dealt * 2.0) as u64,
            (ev.average_frags * 2.0) as i64,
            3000,
            3000,
            true,
            false,
            false,
            false,
            "pvp",
        );
        let g2 = make_game(
            "TestShip",
            3374266064,
            "13.02.2026 15:00:00",
            1,
            (ev.average_damage_dealt * 0.5) as u64,
            0,
            1000,
            1000,
            false,
            true,
            false,
            false,
            "pvp",
        );

        let refs: Vec<&PerGameStat> = vec![&g1, &g2];
        let stats = PrStats::from_games(&refs, &pr_data).expect("should compute PR stats");
        assert!(stats.max > stats.min, "high-damage game should have higher PR");
        assert!(stats.avg > 0.0, "average PR should be positive");
    }

    #[test]
    fn pr_stats_no_expected_values_returns_none() {
        let pr_data = PersonalRatingData::new();
        let game = pvp_win("Unknown", 9999999999, "13.02.2026 14:00:00", 100000, 2, 2000);
        let refs: Vec<&PerGameStat> = vec![&game];
        assert!(PrStats::from_games(&refs, &pr_data).is_none());
    }
}
