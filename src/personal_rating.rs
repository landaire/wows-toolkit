//! Personal Rating (PR) calculation module for World of Warships
//!
//! This module provides functionality to calculate Personal Rating based on
//! the wows-numbers.com expected values.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use serde::Deserialize;
use serde::Serialize;
use tracing::info;

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
    pub ship_id: u64,
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
    pub fn get_ship_expected(&self, ship_id: u64) -> Option<&ShipExpectedValues> {
        self.data.as_ref()?.data.get(&ship_id.to_string())?.as_values()
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
            && let Ok(elapsed) = SystemTime::now().duration_since(modified) {
                return elapsed > UPDATE_INTERVAL;
            }

    // If we can't determine the age, assume it needs updating
    true
}

/// Fetch expected values from the API
pub async fn fetch_expected_values() -> Result<Vec<u8>, reqwest::Error> {
    info!("Fetching PR expected values from {}", EXPECTED_VALUES_URL);
    let response = reqwest::get(EXPECTED_VALUES_URL).await?;
    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}

/// Save expected values to disk
pub fn save_expected_values(data: &[u8]) -> std::io::Result<()> {
    let path = get_expected_values_path();
    info!("Saving PR expected values to {:?}", path);
    fs::write(path, data)
}

/// Load expected values from disk
#[allow(dead_code)]
pub fn load_expected_values_from_disk() -> std::io::Result<Vec<u8>> {
    let path = get_expected_values_path();
    fs::read(path)
}
