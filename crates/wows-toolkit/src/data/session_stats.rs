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
    /// The same game as the shared viewmodel records it, so logic that both
    /// front ends run reads one type.
    pub fn to_shared(&self) -> wows_toolkit_viewmodel::stats::PerGameStat {
        wows_toolkit_viewmodel::stats::PerGameStat {
            ship_name: self.ship_name.clone(),
            ship_id: self.ship_id,
            game_time: self.game_time.clone(),
            sort_key: self.sort_key.clone(),
            player_id: self.player_id,
            damage: self.damage,
            spotting_damage: self.spotting_damage,
            frags: self.frags,
            raw_xp: self.raw_xp,
            base_xp: self.base_xp,
            is_win: self.is_win,
            is_loss: self.is_loss,
            is_draw: self.is_draw,
            is_div: self.is_div,
            match_group: self.match_group.clone(),
            achievements: self
                .achievements
                .iter()
                .map(|achievement| wows_toolkit_viewmodel::stats::SerializableAchievement {
                    game_param_id: achievement.game_param_id,
                    display_name: achievement.display_name.clone(),
                    description: achievement.description.clone(),
                    icon_key: achievement.icon_key.clone(),
                    count: achievement.count,
                })
                .collect(),
        }
    }

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
        let match_group = replay.replay_file.meta.matchGroup.clone().unwrap_or_default();

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

/// One ship aggregated over the games in view. Both front ends compute it,
/// so it lives in the shared viewmodel.
pub use wows_toolkit_viewmodel::stats::PerformanceInfo;

/// A ship's localized display name from the provider, or `None` when there is
/// no provider or it cannot name the ship. Callers that have another source to
/// fall back on need to tell those two cases apart.
pub fn try_resolve_ship_name(ship_id: GameParamId, provider: Option<&GameMetadataProvider>) -> Option<String> {
    wows_toolkit_viewmodel::search::try_resolve_ship_name(ship_id, provider)
}

/// Resolve a ship's display name from the provider, falling back to ID.
pub fn resolve_ship_name(ship_id: GameParamId, provider: Option<&GameMetadataProvider>) -> String {
    try_resolve_ship_name(ship_id, provider).unwrap_or_else(|| format!("[{ship_id}]"))
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
        let per_game: Vec<wows_toolkit_viewmodel::stats::PerGameStat> =
            self.per_ship_limited_games().into_iter().map(PerGameStat::to_shared).collect();

        let mut by_ship: HashMap<GameParamId, Vec<&wows_toolkit_viewmodel::stats::PerGameStat>> = HashMap::new();
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
    #[allow(clippy::too_many_arguments)]
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
}
