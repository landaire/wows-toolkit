use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::types::GameParamId;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::personal_rating::PersonalRatingData;
use crate::personal_rating::PersonalRatingResult;
use crate::personal_rating::ShipBattleStats;
use crate::tab_state::ChartableStat;
use crate::ui::replay_parser::Replay;

/// Per-game statistics extracted from a single replay
#[derive(Clone)]
pub struct PerGameStat {
    pub ship_name: String,
    pub ship_id: GameParamId,
    #[allow(dead_code)]
    pub game_time: String,
    pub damage: u64,
    pub spotting_damage: u64,
    pub frags: i64,
    pub raw_xp: i64,
    pub base_xp: i64,
    pub is_win: bool,
    pub is_loss: bool,
}

impl PerGameStat {
    /// Create a PerGameStat from a replay
    pub fn from_replay(replay: &Replay, metadata_provider: &GameMetadataProvider) -> Option<Self> {
        let ui_report = replay.ui_report.as_ref()?;
        let self_report = ui_report.player_reports().iter().find(|r| r.relation().is_self())?;
        let ship_name = replay.vehicle_name(metadata_provider);
        let ship_id = replay.player_vehicle()?.shipId;
        let game_time = replay.game_time().to_string();
        let battle_result = replay.battle_result();

        Some(PerGameStat {
            ship_name,
            ship_id,
            game_time,
            damage: self_report.actual_damage().unwrap_or_default(),
            spotting_damage: self_report.spotting_damage().unwrap_or_default(),
            frags: self_report.kills().unwrap_or_default(),
            raw_xp: self_report.raw_xp().unwrap_or_default(),
            base_xp: self_report.base_xp().unwrap_or_default(),
            is_win: matches!(battle_result, Some(BattleResult::Win(_))),
            is_loss: matches!(battle_result, Some(BattleResult::Loss(_))),
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
        }

        info
    }

    pub fn wins(&self) -> usize {
        self.wins
    }

    pub fn losses(&self) -> usize {
        self.losses
    }

    pub fn win_rate(&self) -> Option<f64> {
        if self.wins + self.losses == 0 {
            return None;
        }

        Some(self.wins as f64 / (self.wins + self.losses) as f64 * 100.0)
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
#[derive(Default)]
pub struct SessionStats {
    pub session_replays: Vec<Arc<RwLock<Replay>>>,
    /// If set, stats only reflect the N most recent games.
    pub game_count_limit: Option<usize>,
}

impl SessionStats {
    pub fn clear(&mut self) {
        self.session_replays.clear();
    }

    pub fn add_replay(&mut self, replay: Arc<RwLock<Replay>>) {
        let new_replay = replay.read();
        let mut old_replay_index = None;
        for (idx, old_replay) in self.session_replays.iter().enumerate() {
            let old_replay = old_replay.read();
            if new_replay.replay_file.meta.dateTime == old_replay.replay_file.meta.dateTime
                && new_replay.replay_file.meta.playerID == old_replay.replay_file.meta.playerID
            {
                // Many of the stats are dependent on the UI report being
                // available. As such, we will only overwrite the old replay
                // when it has no UI report present.
                old_replay_index = Some(idx);
            }
        }

        drop(new_replay);

        if let Some(old_index) = old_replay_index {
            self.session_replays.remove(old_index);
        }

        self.session_replays.push(replay);
        self.session_replays.sort_by(|a, b| a.read().game_time().cmp(b.read().game_time()));
    }

    /// Return the most recent N replays (based on `game_count_limit`), or all if unset.
    pub fn recent_replays(&self) -> &[Arc<RwLock<Replay>>] {
        match self.game_count_limit {
            Some(n) if n < self.session_replays.len() => &self.session_replays[self.session_replays.len() - n..],
            _ => &self.session_replays,
        }
    }

    /// Get per-game statistics for replays in the session
    pub fn per_game_stats(&self, metadata_provider: &GameMetadataProvider) -> Vec<PerGameStat> {
        self.recent_replays()
            .iter()
            .filter_map(|replay| {
                let replay = replay.read();
                PerGameStat::from_replay(&replay, metadata_provider)
            })
            .collect()
    }

    /// Get aggregated ship statistics derived from per-game stats
    pub fn ship_stats(&self, metadata_provider: &GameMetadataProvider) -> HashMap<String, PerformanceInfo> {
        let per_game = self.per_game_stats(metadata_provider);

        // Group by ship name
        let mut by_ship: HashMap<String, Vec<&PerGameStat>> = HashMap::new();
        for game in &per_game {
            by_ship.entry(game.ship_name.clone()).or_default().push(game);
        }

        // Convert to PerformanceInfo
        by_ship.into_iter().map(|(name, games)| (name, PerformanceInfo::from_games(&games))).collect()
    }

    /// Returns the win rate percentage for this session. Will return `None`
    /// if no games have been played.
    pub fn win_rate(&self) -> Option<f64> {
        if self.recent_replays().is_empty() {
            return None;
        }

        Some((self.games_won() as f64 / self.games_played() as f64) * 100.0)
    }

    /// Total number of games played in the current session
    pub fn games_played(&self) -> usize {
        self.recent_replays().len()
    }

    /// Total number of games won in the current session
    pub fn games_won(&self) -> usize {
        self.recent_replays().iter().fold(0, |accum, replay| {
            if let Some(BattleResult::Win(_)) = replay.read().battle_result() { accum + 1 } else { accum }
        })
    }

    /// Total number of games lost in the current session
    pub fn games_lost(&self) -> usize {
        self.recent_replays().iter().fold(0, |accum, replay| {
            if let Some(BattleResult::Loss(_)) = replay.read().battle_result() { accum + 1 } else { accum }
        })
    }

    pub fn max_damage(&self, metadata_provider: &GameMetadataProvider) -> Option<(String, u64)> {
        self.per_game_stats(metadata_provider).into_iter().map(|g| (g.ship_name, g.damage)).max_by_key(|r| r.1)
    }

    pub fn max_frags(&self, metadata_provider: &GameMetadataProvider) -> Option<(String, i64)> {
        self.per_game_stats(metadata_provider).into_iter().map(|g| (g.ship_name, g.frags)).max_by_key(|r| r.1)
    }

    pub fn total_frags(&self, metadata_provider: &GameMetadataProvider) -> i64 {
        self.per_game_stats(metadata_provider).iter().map(|g| g.frags).sum()
    }

    /// Calculate overall Personal Rating for this session
    pub fn calculate_pr(&self, pr_data: &PersonalRatingData) -> Option<PersonalRatingResult> {
        let stats: Vec<_> = self.recent_replays().iter().filter_map(|replay| replay.read().to_battle_stats()).collect();
        pr_data.calculate_pr(&stats)
    }

    /// Calculate Personal Rating per ship for this session
    /// Returns a map of ship_id -> PR result
    #[allow(dead_code)]
    pub fn calculate_pr_per_ship(&self, pr_data: &PersonalRatingData) -> HashMap<GameParamId, PersonalRatingResult> {
        // Group stats by ship_id
        let mut ship_stats: HashMap<GameParamId, ShipBattleStats> = HashMap::new();

        for replay in self.recent_replays() {
            if let Some(stats) = replay.read().to_battle_stats() {
                let entry = ship_stats.entry(stats.ship_id).or_insert(ShipBattleStats {
                    ship_id: stats.ship_id,
                    battles: 0,
                    damage: 0,
                    wins: 0,
                    frags: 0,
                });
                entry.battles += stats.battles;
                entry.damage += stats.damage;
                entry.wins += stats.wins;
                entry.frags += stats.frags;
            }
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
