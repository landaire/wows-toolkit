//! Pure personal-rating (PR) computation, egui-free and IO-free. The toolkit
//! keeps the disk/network fetch helpers and the egui color mapping; this
//! module owns only the data model and the `calculate_pr` formula.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::GameParamId;

/// Re-exported from `wows-core`, which owns the band boundaries so a
/// consumer that only classifies a recorded PR does not need this crate.
pub use wows_core::game_types::PersonalRatingCategory;

/// Expected values for a single ship
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipExpectedValues {
    pub average_damage_dealt: f64,
    pub average_frags: f64,
    pub win_rate: f64,
}

/// Root structure for the expected values JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedValuesData {
    pub time: u64,
    pub data: HashMap<String, ShipExpectedValuesEntry>,
}

/// Entry in the expected values data - can be either actual values or an empty array
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShipExpectedValuesEntry {
    Values(ShipExpectedValues),
    Empty(Vec<()>),
}

impl ShipExpectedValuesEntry {
    pub fn as_values(&self) -> Option<&ShipExpectedValues> {
        match self {
            ShipExpectedValuesEntry::Values(v) => Some(v),
            ShipExpectedValuesEntry::Empty(_) => None,
        }
    }
}

/// Result of a PR calculation
#[derive(Debug, Clone, serde::Serialize)]
pub struct PersonalRatingResult {
    pub pr: f64,
    pub category: PersonalRatingCategory,
}

impl PersonalRatingResult {
    pub fn new(pr: f64) -> Self {
        Self { pr, category: PersonalRatingCategory::from_pr(pr) }
    }
}

/// Statistics for a single ship used in PR calculation
#[derive(Debug, Clone, Default)]
pub struct ShipBattleStats {
    pub ship_id: GameParamId,
    pub battles: u32,
    pub damage: u64,
    pub wins: u32,
    pub frags: i64,
}

/// Manager for PR expected values data
#[derive(Debug, Default)]
pub struct PersonalRatingData {
    data: Option<ExpectedValuesData>,
}

impl PersonalRatingData {
    pub fn new() -> Self {
        Self { data: None }
    }

    /// Load expected values from parsed data
    pub fn load(&mut self, data: ExpectedValuesData) {
        self.data = Some(data);
    }

    /// Load expected values from the given data
    pub fn load_from_bytes(&mut self, bytes: &[u8]) -> Result<(), serde_json::Error> {
        let data: ExpectedValuesData = serde_json::from_slice(bytes)?;
        self.data = Some(data);
        Ok(())
    }

    /// Check if data is loaded
    pub fn is_loaded(&self) -> bool {
        self.data.is_some()
    }

    /// Get expected values for a ship by its ID
    pub fn get_ship_expected(&self, ship_id: GameParamId) -> Option<&ShipExpectedValues> {
        self.data.as_ref()?.data.get(&ship_id.raw().to_string())?.as_values()
    }

    /// Calculate PR for a collection of ship battle stats
    ///
    /// This follows the formula from wows-numbers.com:
    /// 1. Calculate sums of actual and expected values for each ship
    /// 2. Calculate ratios: rDmg, rFrags, rWins
    /// 3. Normalize: nDmg, nFrags, nWins
    /// 4. PR = 700*nDmg + 300*nFrags + 150*nWins
    pub fn calculate_pr(&self, stats: &[ShipBattleStats]) -> Option<PersonalRatingResult> {
        if stats.is_empty() {
            return None;
        }

        let mut total_actual_damage: f64 = 0.0;
        let mut total_actual_frags: f64 = 0.0;
        let mut total_actual_wins: f64 = 0.0;

        let mut total_expected_damage: f64 = 0.0;
        let mut total_expected_frags: f64 = 0.0;
        let mut total_expected_wins: f64 = 0.0;

        let mut valid_battles = 0u32;

        for ship_stats in stats {
            let Some(expected) = self.get_ship_expected(ship_stats.ship_id) else {
                // Skip ships without expected values
                continue;
            };

            let battles = ship_stats.battles as f64;
            valid_battles += ship_stats.battles;

            // Actual values
            total_actual_damage += ship_stats.damage as f64;
            total_actual_frags += ship_stats.frags as f64;
            total_actual_wins += ship_stats.wins as f64;

            // Expected values (multiply expected per-battle values by number of battles)
            total_expected_damage += expected.average_damage_dealt * battles;
            total_expected_frags += expected.average_frags * battles;
            total_expected_wins += (expected.win_rate / 100.0) * battles;
        }

        if valid_battles == 0 || total_expected_damage == 0.0 {
            return None;
        }

        // Step 2: Calculate ratios
        let r_dmg = total_actual_damage / total_expected_damage;
        let r_frags = total_actual_frags / total_expected_frags;
        let r_wins = total_actual_wins / total_expected_wins;

        // Step 3: Normalize
        let n_dmg = f64::max(0.0, (r_dmg - 0.4) / (1.0 - 0.4));
        let n_frags = f64::max(0.0, (r_frags - 0.1) / (1.0 - 0.1));
        let n_wins = f64::max(0.0, (r_wins - 0.7) / (1.0 - 0.7));

        // Step 4: Calculate PR
        let pr = 700.0 * n_dmg + 300.0 * n_frags + 150.0 * n_wins;

        Some(PersonalRatingResult::new(pr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal, self-contained expected-values fixture: one ship with clean
    /// round-number stats so the PR-at-expected-values case lands on 1150
    /// exactly.
    fn fixture_data() -> ExpectedValuesData {
        let mut data = HashMap::new();
        data.insert(
            "3374266064".to_string(),
            ShipExpectedValuesEntry::Values(ShipExpectedValues {
                average_damage_dealt: 50000.0,
                average_frags: 1.0,
                win_rate: 50.0,
            }),
        );
        data.insert("3330258928".to_string(), ShipExpectedValuesEntry::Empty(Vec::new()));
        ExpectedValuesData { time: 0, data }
    }

    fn loaded_pr_data() -> PersonalRatingData {
        let mut pr = PersonalRatingData::new();
        pr.load(fixture_data());
        pr
    }

    #[test]
    fn fixture_contains_ship() {
        let pr = loaded_pr_data();
        let ev = pr.get_ship_expected(GameParamId::from(3374266064u64));
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert_eq!(ev.average_damage_dealt, 50000.0);
        assert_eq!(ev.average_frags, 1.0);
        assert_eq!(ev.win_rate, 50.0);
    }

    #[test]
    fn empty_array_entries_return_none() {
        let pr = loaded_pr_data();
        let ev = pr.get_ship_expected(GameParamId::from(3330258928u64));
        assert!(ev.is_none(), "empty-array entries should return None");
    }

    #[test]
    fn missing_ship_returns_none() {
        let pr = loaded_pr_data();
        let ev = pr.get_ship_expected(GameParamId::from(9999999999u64));
        assert!(ev.is_none());
    }

    #[test]
    fn calculate_pr_empty_stats_returns_none() {
        let pr = loaded_pr_data();
        assert!(pr.calculate_pr(&[]).is_none());
    }

    #[test]
    fn calculate_pr_all_missing_ships_returns_none() {
        let pr = loaded_pr_data();
        let stats = [ShipBattleStats {
            ship_id: GameParamId::from(9999999999u64),
            battles: 10,
            damage: 500000,
            wins: 5,
            frags: 10,
        }];
        assert!(pr.calculate_pr(&stats).is_none());
    }

    #[test]
    fn calculate_pr_known_ship_lands_on_average() {
        let pr = loaded_pr_data();

        // Play exactly at expected values -> PR = 700 + 300 + 150 = 1150.
        let stats = [ShipBattleStats {
            ship_id: GameParamId::from(3374266064u64),
            battles: 100,
            damage: 5_000_000,
            wins: 50,
            frags: 100,
        }];
        let result = pr.calculate_pr(&stats).expect("should calculate PR");
        assert!((result.pr - 1150.0).abs() < 1e-6, "PR at expected values should be 1150, got {}", result.pr);
        assert_eq!(result.category, PersonalRatingCategory::Average);
    }

    #[test]
    fn calculate_pr_zero_damage_gives_bad() {
        let pr = loaded_pr_data();

        let stats =
            [ShipBattleStats { ship_id: GameParamId::from(3374266064u64), battles: 100, damage: 0, wins: 0, frags: 0 }];
        let result = pr.calculate_pr(&stats).expect("should calculate PR");
        assert_eq!(result.pr, 0.0, "zero stats should give PR=0");
        assert_eq!(result.category, PersonalRatingCategory::Bad);
    }

    #[test]
    fn personal_rating_result_new() {
        let result = PersonalRatingResult::new(1500.0);
        assert_eq!(result.pr, 1500.0);
        assert_eq!(result.category, PersonalRatingCategory::Good);
    }

    #[test]
    fn unloaded_pr_data_returns_none() {
        let pr = PersonalRatingData::new();
        assert!(!pr.is_loaded());
        assert!(pr.get_ship_expected(GameParamId::from(3374266064u64)).is_none());
        let stats = [ShipBattleStats {
            ship_id: GameParamId::from(3374266064u64),
            battles: 10,
            damage: 100000,
            wins: 5,
            frags: 5,
        }];
        assert!(pr.calculate_pr(&stats).is_none());
    }
}
