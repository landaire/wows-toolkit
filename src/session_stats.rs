use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::personal_rating::PersonalRatingData;
use crate::personal_rating::PersonalRatingResult;
use crate::personal_rating::ShipBattleStats;
use crate::ui::replay_parser::Replay;

/// Performance statistics for a single ship across multiple games
#[derive(Default)]
pub struct PerformanceInfo {
    ship_id: Option<u64>,
    wins: usize,
    losses: usize,
    /// Total frags
    total_frags: i64,
    /// Highest frags in a single match
    max_frags: i64,
    total_damage: u64,
    max_damage: u64,
    total_games: usize,
    max_xp: i64,
    max_win_adjusted_xp: i64,
    total_xp: usize,
    total_win_adjusted_xp: usize,
    max_spotting_damage: u64,
    total_spotting_damage: u64,
}

impl PerformanceInfo {
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
    }

    /// Returns the win rate percentage for this session. Will return `None`
    /// if no games have been played.
    pub fn win_rate(&self) -> Option<f64> {
        if self.session_replays.is_empty() {
            return None;
        }

        Some((self.games_won() as f64 / self.games_played() as f64) * 100.0)
    }

    /// Total number of games played in the current session
    pub fn games_played(&self) -> usize {
        self.session_replays.len()
    }

    /// Total number of games won in the current session
    pub fn games_won(&self) -> usize {
        self.session_replays.iter().fold(0, |accum, replay| {
            if let Some(wows_replays::analyzer::battle_controller::BattleResult::Win(_)) = replay.read().battle_result()
            {
                accum + 1
            } else {
                accum
            }
        })
    }

    /// Total number of games lost in the current session
    pub fn games_lost(&self) -> usize {
        self.session_replays.iter().fold(0, |accum, replay| {
            if let Some(wows_replays::analyzer::battle_controller::BattleResult::Loss(_)) =
                replay.read().battle_result()
            {
                accum + 1
            } else {
                accum
            }
        })
    }

    pub fn ship_stats(&self, metadata_provider: &GameMetadataProvider) -> HashMap<String, PerformanceInfo> {
        let mut results: HashMap<String, PerformanceInfo> = HashMap::new();

        for replay in &self.session_replays {
            let replay = replay.read();
            let Some(battle_result) = replay.battle_result() else {
                continue;
            };

            let ship_name = replay.vehicle_name(metadata_provider);
            let ship_id = replay.player_vehicle().map(|v| v.shipId);
            let performance_info = results.entry(ship_name).or_default();

            // Set ship_id if not already set
            if performance_info.ship_id.is_none() {
                performance_info.ship_id = ship_id;
            }

            match battle_result {
                wows_replays::analyzer::battle_controller::BattleResult::Win(_) => {
                    performance_info.wins += 1;
                }
                wows_replays::analyzer::battle_controller::BattleResult::Loss(_) => {
                    performance_info.losses += 1;
                }
                wows_replays::analyzer::battle_controller::BattleResult::Draw => {
                    // do nothing for draws at the moment
                }
            }

            let Some(ui_report) = replay.ui_report.as_ref() else {
                continue;
            };
            let Some(self_report) = ui_report.player_reports().iter().find(|report| report.relation().is_self()) else {
                continue;
            };

            performance_info.total_frags += self_report.kills().unwrap_or_default();
            performance_info.max_frags = performance_info.max_frags.max(self_report.kills().unwrap_or_default());

            performance_info.total_damage += self_report.actual_damage().unwrap_or_default();
            performance_info.max_damage =
                performance_info.max_damage.max(self_report.actual_damage().unwrap_or_default());

            performance_info.total_spotting_damage += self_report.spotting_damage().unwrap_or_default();
            performance_info.max_spotting_damage =
                performance_info.max_spotting_damage.max(self_report.spotting_damage().unwrap_or_default());

            performance_info.total_xp += self_report.raw_xp().unwrap_or_default() as usize;
            performance_info.max_xp = performance_info.max_xp.max(self_report.raw_xp().unwrap_or_default());

            performance_info.total_win_adjusted_xp += self_report.base_xp().unwrap_or_default() as usize;
            performance_info.max_win_adjusted_xp =
                performance_info.max_win_adjusted_xp.max(self_report.base_xp().unwrap_or_default());

            performance_info.total_games += 1;
        }

        results
    }

    pub fn max_damage(&self, metadata_provider: &GameMetadataProvider) -> Option<(String, u64)> {
        self.session_replays
            .iter()
            .filter_map(|replay| {
                let replay = replay.read();

                let ui_report = replay.ui_report.as_ref()?;
                let self_report = ui_report.player_reports().iter().find(|report| report.relation().is_self())?;

                Some((replay.vehicle_name(metadata_provider), self_report.actual_damage()?))
            })
            .max_by_key(|result| result.1)
    }

    pub fn max_frags(&self, metadata_provider: &GameMetadataProvider) -> Option<(String, i64)> {
        self.session_replays
            .iter()
            .filter_map(|replay| {
                let replay = replay.read();

                let ui_report = replay.ui_report.as_ref()?;
                let self_report = ui_report.player_reports().iter().find(|report| report.relation().is_self())?;

                Some((replay.vehicle_name(metadata_provider), self_report.kills()?))
            })
            .max_by_key(|result| result.1)
    }

    pub fn total_frags(&self) -> i64 {
        self.session_replays.iter().fold(0, |accum, replay| {
            let replay = replay.read();

            let Some(ui_report) = replay.ui_report.as_ref() else {
                return accum;
            };

            let Some(self_report) = ui_report.player_reports().iter().find(|report| report.relation().is_self()) else {
                return accum;
            };

            accum + self_report.kills().unwrap_or_default()
        })
    }

    /// Calculate overall Personal Rating for this session
    pub fn calculate_pr(&self, pr_data: &PersonalRatingData) -> Option<PersonalRatingResult> {
        let stats: Vec<_> = self.session_replays.iter().filter_map(|replay| replay.read().to_battle_stats()).collect();
        pr_data.calculate_pr(&stats)
    }

    /// Calculate Personal Rating per ship for this session
    /// Returns a map of ship_id -> PR result
    #[allow(dead_code)]
    pub fn calculate_pr_per_ship(&self, pr_data: &PersonalRatingData) -> HashMap<u64, PersonalRatingResult> {
        // Group stats by ship_id
        let mut ship_stats: HashMap<u64, ShipBattleStats> = HashMap::new();

        for replay in &self.session_replays {
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
