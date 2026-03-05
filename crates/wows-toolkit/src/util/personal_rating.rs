use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use serde::Deserialize;
use serde::Serialize;
use tracing::instrument;
use wows_replays::types::GameParamId;

/// URL to fetch expected values from wows-numbers.com
const EXPECTED_VALUES_URL: &str = "https://api.wows-numbers.com/personal/rating/expected/json/";

/// How often to check for updates (7 days)
const UPDATE_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// File name for cached expected values
const EXPECTED_VALUES_FILENAME: &str = "pr_expected_values.json";

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

/// Personal Rating skill category
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PersonalRatingCategory {
    Bad,
    BelowAverage,
    Average,
    Good,
    VeryGood,
    Great,
    Unicum,
    SuperUnicum,
}

impl PersonalRatingCategory {
    /// Get the category for a given PR value
    pub fn from_pr(pr: f64) -> Self {
        match pr as u32 {
            0..750 => Self::Bad,
            750..1100 => Self::BelowAverage,
            1100..1350 => Self::Average,
            1350..1550 => Self::Good,
            1550..1750 => Self::VeryGood,
            1750..2100 => Self::Great,
            2100..2450 => Self::Unicum,
            _ => Self::SuperUnicum,
        }
    }

    /// Get the display name for this category
    pub fn name(&self) -> &'static str {
        match self {
            Self::Bad => "Bad",
            Self::BelowAverage => "Below Average",
            Self::Average => "Average",
            Self::Good => "Good",
            Self::VeryGood => "Very Good",
            Self::Great => "Great",
            Self::Unicum => "Unicum",
            Self::SuperUnicum => "Super Unicum",
        }
    }

    /// Get the color for this category (placeholder - to be filled in later)
    pub fn color(&self) -> egui::Color32 {
        match self {
            Self::Bad => egui::Color32::from_rgb(0xFF, 0x00, 0x00),
            Self::BelowAverage => egui::Color32::from_rgb(0xFE, 0x79, 0x03),
            Self::Average => egui::Color32::from_rgb(0xFF, 0xC7, 0x1F),
            Self::Good => egui::Color32::from_rgb(0x44, 0xB3, 0x00),
            Self::VeryGood => egui::Color32::from_rgb(0x31, 0x80, 0x00),
            Self::Great => egui::Color32::from_rgb(0x02, 0xC9, 0xB3),
            Self::Unicum => egui::Color32::from_rgb(0xD0, 0x42, 0xF3),
            Self::SuperUnicum => egui::Color32::from_rgb(0xA0, 0x0D, 0xC5),
        }
    }
}

/// Result of a PR calculation
#[derive(Debug, Clone)]
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
    #[allow(dead_code)]
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

/// Get the path for storing expected values
pub fn get_expected_values_path() -> PathBuf {
    let mut path = PathBuf::from(EXPECTED_VALUES_FILENAME);
    if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
        path = storage_dir.join(path);
    }
    path
}

/// Check if expected values need to be updated
pub fn needs_update() -> bool {
    let path = get_expected_values_path();

    if !path.exists() {
        return true;
    }

    // Check file modification time
    if let Ok(metadata) = fs::metadata(&path)
        && let Ok(modified) = metadata.modified()
        && let Ok(elapsed) = SystemTime::now().duration_since(modified)
    {
        return elapsed > UPDATE_INTERVAL;
    }

    // If we can't determine the age, assume it needs updating
    true
}

/// Fetch expected values from the API
#[instrument]
pub async fn fetch_expected_values() -> Result<Vec<u8>, reqwest::Error> {
    let response = reqwest::get(EXPECTED_VALUES_URL).await?;
    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}

/// Save expected values to disk
#[instrument(skip(data), fields(data_len = data.len()))]
pub fn save_expected_values(data: &[u8]) -> std::io::Result<()> {
    let path = get_expected_values_path();
    fs::write(path, data)
}

/// Load expected values from disk
#[allow(dead_code)]
pub fn load_expected_values_from_disk() -> std::io::Result<Vec<u8>> {
    let path = get_expected_values_path();
    fs::read(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Path to the checked-in expected values fixture.
    fn fixture_bytes() -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests")
            .join("fixtures")
            .join("pr_expected_values.json");
        fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()))
    }

    fn loaded_pr_data() -> PersonalRatingData {
        let mut pr = PersonalRatingData::new();
        pr.load_from_bytes(&fixture_bytes()).expect("should parse expected values JSON");
        pr
    }

    // -- Loading --

    #[test]
    fn load_from_bytes_parses_fixture() {
        let pr = loaded_pr_data();
        assert!(pr.is_loaded());
    }

    #[test]
    fn fixture_contains_ships() {
        let pr = loaded_pr_data();
        // The first ship ID in the fixture is 3374266064
        let ev = pr.get_ship_expected(GameParamId::from(3374266064u64));
        assert!(ev.is_some(), "fixture should contain ship 3374266064");
        let ev = ev.unwrap();
        assert!(ev.average_damage_dealt > 0.0);
        assert!(ev.average_frags > 0.0);
        assert!(ev.win_rate > 0.0);
    }

    #[test]
    fn empty_array_entries_return_none() {
        let pr = loaded_pr_data();
        // Ship 3330258928 is [] in the fixture
        let ev = pr.get_ship_expected(GameParamId::from(3330258928u64));
        assert!(ev.is_none(), "empty-array entries should return None");
    }

    #[test]
    fn missing_ship_returns_none() {
        let pr = loaded_pr_data();
        let ev = pr.get_ship_expected(GameParamId::from(9999999999u64));
        assert!(ev.is_none());
    }

    // -- PR calculation --

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
    fn calculate_pr_known_ship() {
        let pr = loaded_pr_data();
        let ev = pr.get_ship_expected(GameParamId::from(3374266064u64)).unwrap();

        // Play exactly at expected values -> should give ~1150 (average PR).
        // At ratio=1.0: nDmg=(1.0-0.4)/0.6=1.0, nFrags=(1.0-0.1)/0.9=1.0, nWins=(1.0-0.7)/0.3=1.0
        // PR = 700 + 300 + 150 = 1150
        let stats = [ShipBattleStats {
            ship_id: GameParamId::from(3374266064u64),
            battles: 100,
            damage: (ev.average_damage_dealt * 100.0) as u64,
            wins: (ev.win_rate / 100.0 * 100.0) as u32,
            frags: (ev.average_frags * 100.0) as i64,
        }];
        let result = pr.calculate_pr(&stats).expect("should calculate PR");
        // Allow tolerance for float->int truncation in test data
        assert!((result.pr - 1150.0).abs() < 20.0, "PR at expected values should be ~1150, got {}", result.pr);
        assert_eq!(result.category, PersonalRatingCategory::Average);
    }

    #[test]
    fn calculate_pr_double_expected_is_high() {
        let pr = loaded_pr_data();
        let ev = pr.get_ship_expected(GameParamId::from(3374266064u64)).unwrap();

        // Double all metrics -> high PR
        let stats = [ShipBattleStats {
            ship_id: GameParamId::from(3374266064u64),
            battles: 100,
            damage: (ev.average_damage_dealt * 200.0) as u64,
            wins: 100, // 100% WR
            frags: (ev.average_frags * 200.0) as i64,
        }];
        let result = pr.calculate_pr(&stats).expect("should calculate PR");
        assert!(result.pr > 2400.0, "double expected should give super unicum, got {}", result.pr);
        assert_eq!(result.category, PersonalRatingCategory::SuperUnicum);
    }

    #[test]
    fn calculate_pr_zero_damage_gives_low() {
        let pr = loaded_pr_data();

        let stats =
            [ShipBattleStats { ship_id: GameParamId::from(3374266064u64), battles: 100, damage: 0, wins: 0, frags: 0 }];
        let result = pr.calculate_pr(&stats).expect("should calculate PR");
        assert_eq!(result.pr, 0.0, "zero stats should give PR=0");
        assert_eq!(result.category, PersonalRatingCategory::Bad);
    }

    #[test]
    fn calculate_pr_multi_ship_aggregates() {
        let pr = loaded_pr_data();

        // Use two different ships that both exist in the fixture
        let ship_a = GameParamId::from(3374266064u64);
        let ship_b = GameParamId::from(3340645584u64);
        assert!(pr.get_ship_expected(ship_a).is_some());
        assert!(pr.get_ship_expected(ship_b).is_some());

        let ev_a = pr.get_ship_expected(ship_a).unwrap();
        let ev_b = pr.get_ship_expected(ship_b).unwrap();

        // Both at expected values -> combined should also be ~1150
        let stats = [
            ShipBattleStats {
                ship_id: ship_a,
                battles: 50,
                damage: (ev_a.average_damage_dealt * 50.0) as u64,
                wins: (ev_a.win_rate / 100.0 * 50.0) as u32,
                frags: (ev_a.average_frags * 50.0) as i64,
            },
            ShipBattleStats {
                ship_id: ship_b,
                battles: 50,
                damage: (ev_b.average_damage_dealt * 50.0) as u64,
                wins: (ev_b.win_rate / 100.0 * 50.0) as u32,
                frags: (ev_b.average_frags * 50.0) as i64,
            },
        ];
        let result = pr.calculate_pr(&stats).expect("should calculate PR");
        assert!((result.pr - 1150.0).abs() < 20.0, "multi-ship at expected should be ~1150, got {}", result.pr);
    }

    #[test]
    fn calculate_pr_skips_unknown_ships() {
        let pr = loaded_pr_data();
        let ev = pr.get_ship_expected(GameParamId::from(3374266064u64)).unwrap();

        // One real ship + one unknown ship -> should only use the real ship
        let stats = [
            ShipBattleStats {
                ship_id: GameParamId::from(3374266064u64),
                battles: 100,
                damage: (ev.average_damage_dealt * 100.0) as u64,
                wins: (ev.win_rate / 100.0 * 100.0) as u32,
                frags: (ev.average_frags * 100.0) as i64,
            },
            ShipBattleStats { ship_id: GameParamId::from(9999999999u64), battles: 100, damage: 0, wins: 0, frags: 0 },
        ];
        let result = pr.calculate_pr(&stats).expect("should calculate PR");
        assert!((result.pr - 1150.0).abs() < 20.0, "should ignore unknown ship, got {}", result.pr);
    }

    // -- PR categories --

    #[test]
    fn pr_category_boundaries() {
        assert_eq!(PersonalRatingCategory::from_pr(0.0), PersonalRatingCategory::Bad);
        assert_eq!(PersonalRatingCategory::from_pr(749.0), PersonalRatingCategory::Bad);
        assert_eq!(PersonalRatingCategory::from_pr(750.0), PersonalRatingCategory::BelowAverage);
        assert_eq!(PersonalRatingCategory::from_pr(1099.0), PersonalRatingCategory::BelowAverage);
        assert_eq!(PersonalRatingCategory::from_pr(1100.0), PersonalRatingCategory::Average);
        assert_eq!(PersonalRatingCategory::from_pr(1349.0), PersonalRatingCategory::Average);
        assert_eq!(PersonalRatingCategory::from_pr(1350.0), PersonalRatingCategory::Good);
        assert_eq!(PersonalRatingCategory::from_pr(1549.0), PersonalRatingCategory::Good);
        assert_eq!(PersonalRatingCategory::from_pr(1550.0), PersonalRatingCategory::VeryGood);
        assert_eq!(PersonalRatingCategory::from_pr(1749.0), PersonalRatingCategory::VeryGood);
        assert_eq!(PersonalRatingCategory::from_pr(1750.0), PersonalRatingCategory::Great);
        assert_eq!(PersonalRatingCategory::from_pr(2099.0), PersonalRatingCategory::Great);
        assert_eq!(PersonalRatingCategory::from_pr(2100.0), PersonalRatingCategory::Unicum);
        assert_eq!(PersonalRatingCategory::from_pr(2449.0), PersonalRatingCategory::Unicum);
        assert_eq!(PersonalRatingCategory::from_pr(2450.0), PersonalRatingCategory::SuperUnicum);
        assert_eq!(PersonalRatingCategory::from_pr(5000.0), PersonalRatingCategory::SuperUnicum);
    }

    #[test]
    fn pr_category_names() {
        assert_eq!(PersonalRatingCategory::Bad.name(), "Bad");
        assert_eq!(PersonalRatingCategory::BelowAverage.name(), "Below Average");
        assert_eq!(PersonalRatingCategory::Average.name(), "Average");
        assert_eq!(PersonalRatingCategory::Good.name(), "Good");
        assert_eq!(PersonalRatingCategory::VeryGood.name(), "Very Good");
        assert_eq!(PersonalRatingCategory::Great.name(), "Great");
        assert_eq!(PersonalRatingCategory::Unicum.name(), "Unicum");
        assert_eq!(PersonalRatingCategory::SuperUnicum.name(), "Super Unicum");
    }

    #[test]
    fn pr_category_ordering() {
        assert!(PersonalRatingCategory::Bad < PersonalRatingCategory::BelowAverage);
        assert!(PersonalRatingCategory::BelowAverage < PersonalRatingCategory::Average);
        assert!(PersonalRatingCategory::Average < PersonalRatingCategory::Good);
        assert!(PersonalRatingCategory::Good < PersonalRatingCategory::VeryGood);
        assert!(PersonalRatingCategory::VeryGood < PersonalRatingCategory::Great);
        assert!(PersonalRatingCategory::Great < PersonalRatingCategory::Unicum);
        assert!(PersonalRatingCategory::Unicum < PersonalRatingCategory::SuperUnicum);
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
