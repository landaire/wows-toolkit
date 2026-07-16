//! Egui-free numeric result types: per-type damage/hit breakdowns and the
//! server- and observed-results holders built from resolved battle results.
//!
//! The `DAMAGE_*`/`HITS_*` string constants and the `*_DESCRIPTIONS` tables are
//! the resolved-results key names shared by the builder and the toolkit UI (the
//! UI re-imports them to rebuild hover text).

use std::collections::BTreeMap;
use std::collections::HashMap;

use wows_replays::types::AccountId;

pub const DAMAGE_MAIN_AP: &str = "damage_main_ap";
pub const DAMAGE_MAIN_CS: &str = "damage_main_cs";
pub const DAMAGE_MAIN_HE: &str = "damage_main_he";
pub const DAMAGE_ATBA_AP: &str = "damage_atba_ap";
pub const DAMAGE_ATBA_CS: &str = "damage_atba_cs";
pub const DAMAGE_ATBA_HE: &str = "damage_atba_he";
pub const DAMAGE_ATBA_AP_MANUAL: &str = "damage_atba_ap_manual";
pub const DAMAGE_ATBA_CS_MANUAL: &str = "damage_atba_cs_manual";
pub const DAMAGE_ATBA_HE_MANUAL: &str = "damage_atba_he_manual";
pub const DAMAGE_TPD_NORMAL: &str = "damage_tpd_normal";
pub const DAMAGE_TPD_DEEP: &str = "damage_tpd_deep";
pub const DAMAGE_TPD_ALTER: &str = "damage_tpd_alter";
pub const DAMAGE_TPD_PHOTON: &str = "damage_tpd_photon";
pub const DAMAGE_BOMB: &str = "damage_bomb";
pub const DAMAGE_BOMB_ALT: &str = "damage_bomb_alt";
pub const DAMAGE_DBOMB_AIRSUPPORT: &str = "damage_dbomb_airsupport";
pub const DAMAGE_ADBOMB: &str = "damage_adbomb";
pub const DAMAGE_TBOMB: &str = "damage_tbomb";
pub const DAMAGE_TBOMB_ALT: &str = "damage_tbomb_alt";
pub const DAMAGE_TBOMB_AIRSUPPORT: &str = "damage_tbomb_airsupport";
pub const DAMAGE_FIRE: &str = "damage_fire";
pub const DAMAGE_RAM: &str = "damage_ram";
pub const DAMAGE_FLOOD: &str = "damage_flood";
pub const DAMAGE_DBOMB: &str = "damage_dbomb";
pub const DAMAGE_DBOMB_DIRECT: &str = "damage_dbomb_direct";
pub const DAMAGE_DBOMB_SPLASH: &str = "damage_dbomb_splash";
pub const DAMAGE_SEA_MINE: &str = "damage_sea_mine";
pub const DAMAGE_ROCKET: &str = "damage_rocket";
pub const DAMAGE_ROCKET_AIRSUPPORT: &str = "damage_rocket_airsupport";
pub const DAMAGE_SKIP: &str = "damage_skip";
pub const DAMAGE_SKIP_ALT: &str = "damage_skip_alt";
pub const DAMAGE_SKIP_AIRSUPPORT: &str = "damage_skip_airsupport";
pub const DAMAGE_WAVE: &str = "damage_wave";
pub const DAMAGE_CHARGE_LASER: &str = "damage_charge_laser";
pub const DAMAGE_PULSE_LASER: &str = "damage_pulse_laser";
pub const DAMAGE_AXIS_LASER: &str = "damage_axis_laser";
pub const DAMAGE_PHASER_LASER: &str = "damage_phaser_laser";

pub const HITS_MAIN_AP: &str = "hits_main_ap";
pub const HITS_MAIN_CS: &str = "hits_main_cs";
pub const HITS_MAIN_HE: &str = "hits_main_he";
pub const HITS_ATBA_AP: &str = "hits_atba_ap";
pub const HITS_ATBA_CS: &str = "hits_atba_cs";
pub const HITS_ATBA_HE: &str = "hits_atba_he";
pub const HITS_ATBA_AP_MANUAL: &str = "hits_atba_ap_manual";
pub const HITS_ATBA_CS_MANUAL: &str = "hits_atba_cs_manual";
pub const HITS_ATBA_HE_MANUAL: &str = "hits_atba_he_manual";
pub const HITS_TPD_NORMAL: &str = "hits_tpd";
pub const HITS_BOMB: &str = "hits_bomb";
pub const HITS_BOMB_ALT: &str = "hits_bomb_alt";
pub const HITS_BOMB_AIRSUPPORT: &str = "hits_bomb_airsupport";
pub const HITS_DBOMB_AIRSUPPORT: &str = "hits_dbomb_airsupport";
pub const HITS_TBOMB: &str = "hits_tbomb";
pub const HITS_TBOMB_ALT: &str = "hits_tbomb_alt";
pub const HITS_TBOMB_AIRSUPPORT: &str = "hits_tbomb_airsupport";
pub const HITS_RAM: &str = "hits_ram";
pub const HITS_DBOMB_DIRECT: &str = "hits_dbomb_direct";
pub const HITS_DBOMB_SPLASH: &str = "hits_dbomb_splash";
pub const HITS_SEA_MINE: &str = "hits_sea_mine";
pub const HITS_ROCKET: &str = "hits_rocket";
pub const HITS_ROCKET_AIRSUPPORT: &str = "hits_rocket_airsupport";
pub const HITS_SKIP: &str = "hits_skip";
pub const HITS_SKIP_ALT: &str = "hits_skip_alt";
pub const HITS_SKIP_AIRSUPPORT: &str = "hits_skip_airsupport";
pub const HITS_WAVE: &str = "hits_wave";
pub const HITS_CHARGE_LASER: &str = "hits_charge_laser";
pub const HITS_PULSE_LASER: &str = "hits_pulse_laser";
pub const HITS_AXIS_LASER: &str = "hits_axis_laser";
pub const HITS_PHASER_LASER: &str = "hits_phaser_laser";

pub static DAMAGE_DESCRIPTIONS: [(&str, &str); 36] = [
    (DAMAGE_MAIN_AP, "AP"),
    (DAMAGE_MAIN_CS, "SAP"),
    (DAMAGE_MAIN_HE, "HE"),
    (DAMAGE_ATBA_AP, "AP Sec"),
    (DAMAGE_ATBA_AP_MANUAL, "AP Sec (Manual)"),
    (DAMAGE_ATBA_CS, "SAP Sec"),
    (DAMAGE_ATBA_CS_MANUAL, "SAP Sec (Manual)"),
    (DAMAGE_ATBA_HE, "HE Sec"),
    (DAMAGE_ATBA_HE_MANUAL, "HE Sec (Manual)"),
    (DAMAGE_TPD_NORMAL, "Torps"),
    (DAMAGE_TPD_DEEP, "Deep Water Torps"),
    (DAMAGE_TPD_ALTER, "Alt Torps"),
    (DAMAGE_TPD_PHOTON, "Photon Torps"),
    (DAMAGE_BOMB, "HE Bomb"),
    (DAMAGE_BOMB_ALT, "Alt Bomb"),
    (DAMAGE_DBOMB_AIRSUPPORT, "Air Support Depth Charge"),
    (DAMAGE_TBOMB, "Torpedo Bomber"),
    (DAMAGE_TBOMB_ALT, "Torpedo Bomber (Alt)"),
    (DAMAGE_TBOMB_AIRSUPPORT, "Torpedo Bomber Air Support"),
    (DAMAGE_FIRE, "Fire"),
    (DAMAGE_RAM, "Ram"),
    (DAMAGE_FLOOD, "Flood"),
    (DAMAGE_DBOMB, "Depth Charge"),
    (DAMAGE_DBOMB_DIRECT, "Depth Charge (Direct)"),
    (DAMAGE_DBOMB_SPLASH, "Depth Charge (Splash)"),
    (DAMAGE_SEA_MINE, "Sea Mine"),
    (DAMAGE_ROCKET, "Rocket"),
    (DAMAGE_ROCKET_AIRSUPPORT, "Air Supp Rocket"),
    (DAMAGE_SKIP, "Skip Bomb"),
    (DAMAGE_SKIP_ALT, "Alt Skip Bomb"),
    (DAMAGE_SKIP_AIRSUPPORT, "Air Supp Skip Bomb"),
    (DAMAGE_WAVE, "Wave"),
    (DAMAGE_CHARGE_LASER, "Charge Laser"),
    (DAMAGE_PULSE_LASER, "Pulse Laser"),
    (DAMAGE_AXIS_LASER, "Axis Laser"),
    (DAMAGE_PHASER_LASER, "Phaser Laser"),
];

pub static HITS_DESCRIPTIONS: [(&str, &str); 31] = [
    (HITS_MAIN_AP, "AP"),
    (HITS_MAIN_CS, "SAP"),
    (HITS_MAIN_HE, "HE"),
    (HITS_ATBA_AP, "AP Sec"),
    (HITS_ATBA_AP_MANUAL, "AP Sec (Manual)"),
    (HITS_ATBA_CS, "SAP Sec"),
    (HITS_ATBA_CS_MANUAL, "SAP Sec (Manual)"),
    (HITS_ATBA_HE, "HE Sec"),
    (HITS_ATBA_HE_MANUAL, "HE Sec (Manual)"),
    (HITS_TPD_NORMAL, "Torps"),
    (HITS_BOMB, "HE Bomb"),
    (HITS_BOMB_ALT, "Alt Bomb"),
    (HITS_BOMB_AIRSUPPORT, "Air Support Bomb"),
    (HITS_DBOMB_AIRSUPPORT, "Air Support Depth Charge"),
    (HITS_TBOMB, "Torpedo Bomber"),
    (HITS_TBOMB_ALT, "Torpedo Bomber (Alt)"),
    (HITS_TBOMB_AIRSUPPORT, "Torpedo Bomber Air Support"),
    (HITS_RAM, "Ram"),
    (HITS_DBOMB_DIRECT, "Depth Charge (Direct)"),
    (HITS_DBOMB_SPLASH, "Depth Charge (Splash)"),
    (HITS_SEA_MINE, "Sea Mine"),
    (HITS_ROCKET, "Rocket"),
    (HITS_ROCKET_AIRSUPPORT, "Air Supp Rocket"),
    (HITS_SKIP, "Skip Bomb"),
    (HITS_SKIP_ALT, "Alt Skip Bomb"),
    (HITS_SKIP_AIRSUPPORT, "Air Supp Skip Bomb"),
    (HITS_WAVE, "Wave"),
    (HITS_CHARGE_LASER, "Charge Laser"),
    (HITS_PULSE_LASER, "Pulse Laser"),
    (HITS_AXIS_LASER, "Axis Laser"),
    (HITS_PHASER_LASER, "Phaser Laser"),
];

/// Keys for received damage lookups. The server uses different key names on the
/// received side (e.g. combined `damage_dbomb` instead of split direct/splash).
pub static RECEIVED_DAMAGE_DESCRIPTIONS: [(&str, &str); 27] = [
    (DAMAGE_MAIN_AP, "AP"),
    (DAMAGE_MAIN_CS, "SAP"),
    (DAMAGE_MAIN_HE, "HE"),
    (DAMAGE_ATBA_AP, "AP Sec"),
    (DAMAGE_ATBA_AP_MANUAL, "AP Sec (Manual)"),
    (DAMAGE_ATBA_CS, "SAP Sec"),
    (DAMAGE_ATBA_CS_MANUAL, "SAP Sec (Manual)"),
    (DAMAGE_ATBA_HE, "HE Sec"),
    (DAMAGE_ATBA_HE_MANUAL, "HE Sec (Manual)"),
    (DAMAGE_TPD_NORMAL, "Torps"),
    (DAMAGE_TPD_DEEP, "Deep Water Torps"),
    (DAMAGE_TPD_ALTER, "Alt Torps"),
    (DAMAGE_BOMB, "HE Bomb"),
    (DAMAGE_BOMB_ALT, "Alt Bomb"),
    (DAMAGE_DBOMB, "Depth Charge"),
    (DAMAGE_DBOMB_AIRSUPPORT, "Air Support Depth Charge"),
    (DAMAGE_ADBOMB, "Airstrike Depth Charge"),
    (DAMAGE_TBOMB, "Torpedo Bomber"),
    (DAMAGE_TBOMB_ALT, "Torpedo Bomber (Alt)"),
    (DAMAGE_TBOMB_AIRSUPPORT, "Torpedo Bomber Air Support"),
    (DAMAGE_FIRE, "Fire"),
    (DAMAGE_RAM, "Ram"),
    (DAMAGE_FLOOD, "Flood"),
    (DAMAGE_SEA_MINE, "Sea Mine"),
    (DAMAGE_ROCKET, "Rocket"),
    (DAMAGE_ROCKET_AIRSUPPORT, "Air Supp Rocket"),
    (DAMAGE_SKIP, "Skip Bomb"),
];

pub static POTENTIAL_DAMAGE_DESCRIPTIONS: [(&str, &str); 4] =
    [("agro_art", "Artillery"), ("agro_tpd", "Torpedo"), ("agro_air", "Planes"), ("agro_dbomb", "Depth Charge")];

/// Damage breakdown by type.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Damage {
    pub ap: Option<u64>,
    pub sap: Option<u64>,
    pub he: Option<u64>,
    pub he_secondaries: Option<u64>,
    pub sap_secondaries: Option<u64>,
    pub torps: Option<u64>,
    pub deep_water_torps: Option<u64>,
    pub fire: Option<u64>,
    pub flooding: Option<u64>,
}

/// Hit counts by weapon type.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Hits {
    pub ap: Option<u64>,
    pub sap: Option<u64>,
    pub he: Option<u64>,
    pub he_secondaries: Option<u64>,
    pub sap_secondaries: Option<u64>,
    pub ap_secondaries_manual: Option<u64>,
    pub he_secondaries_manual: Option<u64>,
    pub sap_secondaries_manual: Option<u64>,
    pub torps: Option<u64>,
}

/// Potential damage breakdown by source type.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct PotentialDamage {
    pub artillery: u64,
    pub torpedoes: u64,
    pub planes: u64,
}

/// Per-victim damage interaction. Totals + percentages for tables, plus the
/// per-type breakdown the UI hover needs (the export projects this down to
/// totals only).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct DamageInteraction {
    pub damage_dealt: u64,
    pub damage_dealt_by_type: Damage,
    /// Full per-type dealt breakdown keyed by the `DAMAGE_*` constant, only
    /// entries > 0. Carries the types the 9-field `Damage` projection drops.
    pub damage_dealt_by_type_full: BTreeMap<String, u64>,
    pub damage_dealt_percentage: f64,
    pub damage_dealt_inverse_percentage: f64,
    pub damage_received: u64,
    pub damage_received_by_type: Damage,
    /// Full per-type received breakdown keyed by the `DAMAGE_*` constant,
    /// attributed from the attacker's dealt breakdown.
    pub damage_received_by_type_full: BTreeMap<String, u64>,
    pub damage_received_percentage: f64,
    pub damage_received_inverse_percentage: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ObservedResults {
    pub damage: u64,
    pub kills: i64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ServerResults {
    /// Base XP. `None` when the resolved object omits the `exp` key.
    pub xp: Option<i64>,
    pub raw_xp: Option<i64>,
    /// Total damage dealt. `None` when the resolved object omits the `damage`
    /// key (old-format results still carry hits/received/interactions).
    pub damage: Option<u64>,
    pub damage_details: Damage,
    /// Full dealt breakdown keyed by the `DAMAGE_*` constant, only entries > 0.
    /// Empty when `damage` is absent, mirroring the original hover gate.
    pub damage_by_type: BTreeMap<String, u64>,
    pub hits_details: Hits,
    /// Species-aware relevant-hits scalar: rocket/skip hits for carriers,
    /// otherwise main-battery AP+SAP+HE. Matches the UI hits column.
    pub hits: Option<u64>,
    /// Full hit breakdown keyed by the `HITS_*` constant, only entries > 0.
    pub hits_by_type: BTreeMap<String, u64>,
    /// Server-reported spotting (`scouting_damage`). `None` when absent; the
    /// self-player controller fallback lives on `NormalizedPlayer`.
    pub spotting_damage: Option<u64>,
    pub potential_damage: u64,
    pub potential_damage_details: PotentialDamage,
    pub received_damage: u64,
    pub received_damage_details: Damage,
    /// Full received breakdown keyed by the `DAMAGE_*` constant, only entries
    /// > 0 (values read from the `received_*` keys).
    pub received_damage_by_type: BTreeMap<String, u64>,
    /// `None` when the resolved object omits the `interactions` key.
    pub fires_dealt: Option<u64>,
    /// `None` when the resolved object omits the `interactions` key.
    pub floods_dealt: Option<u64>,
    /// `None` when the resolved object omits the `interactions` key.
    pub citadels_dealt: Option<u64>,
    /// `None` when the resolved object omits the `interactions` key.
    pub crits_dealt: Option<u64>,
    pub distance_traveled: Option<f64>,
    pub kills: Option<i64>,
    pub damage_interactions: HashMap<AccountId, DamageInteraction>,
}
