//! Egui-free numeric result types: per-type damage/hit breakdowns and the
//! server- and observed-results holders built from resolved battle results.
//!
//! The `DAMAGE_*`/`HITS_*` string constants and the `*_DESCRIPTIONS` tables are
//! the resolved-results key names shared by the builder and the toolkit UI (the
//! UI re-imports them to rebuild hover text). `ControllerWeaponGroup` names
//! the weapon groups the controller damage stats are summed into.

use std::collections::BTreeMap;
use std::collections::HashMap;

use wows_replays::types::AccountId;
use wowsunpack::game_types::DamageStatWeapon;
use wowsunpack::recognized::Recognized;

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
pub const DAMAGE_BOMB_AVIA: &str = "damage_bomb_avia";
pub const DAMAGE_BOMB_ALT: &str = "damage_bomb_alt";
pub const DAMAGE_BOMB_AIRSUPPORT: &str = "damage_bomb_airsupport";
pub const DAMAGE_DBOMB_AIRSUPPORT: &str = "damage_dbomb_airsupport";
pub const DAMAGE_TBOMB_AVIA: &str = "damage_tbomb_avia";
pub const DAMAGE_TBOMB_ALT: &str = "damage_tbomb_alt";
pub const DAMAGE_TBOMB_AIRSUPPORT: &str = "damage_tbomb_airsupport";
pub const DAMAGE_FIRE: &str = "damage_fire";
pub const DAMAGE_RAM: &str = "damage_ram";
pub const DAMAGE_FLOOD: &str = "damage_flood";
pub const DAMAGE_DBOMB: &str = "damage_dbomb";
pub const DAMAGE_DBOMB_DIRECT: &str = "damage_dbomb_direct";
pub const DAMAGE_DBOMB_SPLASH: &str = "damage_dbomb_splash";
pub const DAMAGE_SEA_MINE: &str = "damage_sea_mine";
pub const DAMAGE_ROCKET_AVIA: &str = "damage_rocket_avia";
pub const DAMAGE_ROCKET_ALT: &str = "damage_rocket_alt";
pub const DAMAGE_ROCKET_AIRSUPPORT: &str = "damage_rocket_airsupport";
pub const DAMAGE_SKIP_AVIA: &str = "damage_skip_avia";
pub const DAMAGE_SKIP_ALT: &str = "damage_skip_alt";
pub const DAMAGE_SKIP_AIRSUPPORT: &str = "damage_skip_airsupport";
pub const DAMAGE_MISSILE: &str = "damage_missile";
pub const DAMAGE_WAVE: &str = "damage_wave";
pub const DAMAGE_CHARGE_LASER: &str = "damage_charge_laser";
pub const DAMAGE_PULSE_LASER: &str = "damage_pulse_laser";
pub const DAMAGE_AXIS_LASER: &str = "damage_axis_laser";
pub const DAMAGE_PHASER_LASER: &str = "damage_phaser_laser";
pub const DAMAGE_PHASER_LASER_SPLASH: &str = "damage_phaser_laser_splash";

// Server sums of the keys above, left out of the description tables:
// `damage_bomb`, `damage_tbomb`, `damage_rocket` and `damage_skip` are the sums
// of their `_avia`, `_alt` and `_airsupport` keys, and `damage_adbomb` equals
// `damage_dbomb_airsupport`.
pub const DAMAGE_BOMB: &str = "damage_bomb";
pub const DAMAGE_TBOMB: &str = "damage_tbomb";
pub const DAMAGE_ROCKET: &str = "damage_rocket";
pub const DAMAGE_SKIP: &str = "damage_skip";
pub const DAMAGE_ADBOMB: &str = "damage_adbomb";

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
pub const HITS_BOMB_AVIA: &str = "hits_bomb_avia";
pub const HITS_BOMB_ALT: &str = "hits_bomb_alt";
pub const HITS_BOMB_AIRSUPPORT: &str = "hits_bomb_airsupport";
pub const HITS_DBOMB_AIRSUPPORT: &str = "hits_dbomb_airsupport";
pub const HITS_TBOMB_AVIA: &str = "hits_tbomb_avia";
pub const HITS_TBOMB_ALT: &str = "hits_tbomb_alt";
pub const HITS_TBOMB_AIRSUPPORT: &str = "hits_tbomb_airsupport";
pub const HITS_RAM: &str = "hits_ram";
pub const HITS_DBOMB_DIRECT: &str = "hits_dbomb_direct";
pub const HITS_DBOMB_SPLASH: &str = "hits_dbomb_splash";
pub const HITS_SEA_MINE: &str = "hits_sea_mine";
pub const HITS_ROCKET_AVIA: &str = "hits_rocket_avia";
pub const HITS_ROCKET_ALT: &str = "hits_rocket_alt";
pub const HITS_ROCKET_AIRSUPPORT: &str = "hits_rocket_airsupport";
pub const HITS_SKIP_AVIA: &str = "hits_skip_avia";
pub const HITS_SKIP_ALT: &str = "hits_skip_alt";
pub const HITS_SKIP_AIRSUPPORT: &str = "hits_skip_airsupport";
pub const HITS_MISSILE: &str = "hits_missile";
pub const HITS_WAVE: &str = "hits_wave";
pub const HITS_CHARGE_LASER: &str = "hits_charge_laser";
pub const HITS_PULSE_LASER: &str = "hits_pulse_laser";
pub const HITS_AXIS_LASER: &str = "hits_axis_laser";
pub const HITS_PHASER_LASER: &str = "hits_phaser_laser";
pub const HITS_PHASER_LASER_SPLASH: &str = "hits_phaser_laser_splash";

// Server sums of the keys above, left out of `HITS_DESCRIPTIONS`.
pub const HITS_BOMB: &str = "hits_bomb";
pub const HITS_TBOMB: &str = "hits_tbomb";
pub const HITS_ROCKET: &str = "hits_rocket";
pub const HITS_SKIP: &str = "hits_skip";

/// Lists only the parts of the summed keys (`DAMAGE_BOMB` and the others
/// beside it) on the dealt, per-victim and received sides alike, so each point
/// of damage appears once and the lines sum to the `damage` total.
/// `damage_airdefense`, `damage_planes_by_plane` and `damage_shields` are
/// damage to aircraft and shields, which `damage` leaves out.
pub static DAMAGE_DESCRIPTIONS: [(&str, &str); 40] = [
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
    (DAMAGE_BOMB_AVIA, "HE Bomb"),
    (DAMAGE_BOMB_ALT, "Alt Bomb"),
    (DAMAGE_BOMB_AIRSUPPORT, "Air Support Bomb"),
    (DAMAGE_DBOMB_AIRSUPPORT, "Air Support Depth Charge"),
    (DAMAGE_TBOMB_AVIA, "Torpedo Bomber"),
    (DAMAGE_TBOMB_ALT, "Torpedo Bomber (Alt)"),
    (DAMAGE_TBOMB_AIRSUPPORT, "Torpedo Bomber Air Support"),
    (DAMAGE_FIRE, "Fire"),
    (DAMAGE_RAM, "Ram"),
    (DAMAGE_FLOOD, "Flood"),
    (DAMAGE_DBOMB, "Depth Charge"),
    (DAMAGE_DBOMB_DIRECT, "Depth Charge (Direct)"),
    (DAMAGE_DBOMB_SPLASH, "Depth Charge (Splash)"),
    (DAMAGE_SEA_MINE, "Sea Mine"),
    (DAMAGE_ROCKET_AVIA, "Rocket"),
    (DAMAGE_ROCKET_ALT, "Alt Rocket"),
    (DAMAGE_ROCKET_AIRSUPPORT, "Air Supp Rocket"),
    (DAMAGE_SKIP_AVIA, "Skip Bomb"),
    (DAMAGE_SKIP_ALT, "Alt Skip Bomb"),
    (DAMAGE_SKIP_AIRSUPPORT, "Air Supp Skip Bomb"),
    (DAMAGE_MISSILE, "Missile"),
    (DAMAGE_WAVE, "Wave"),
    (DAMAGE_CHARGE_LASER, "Charge Laser"),
    (DAMAGE_PULSE_LASER, "Pulse Laser"),
    (DAMAGE_AXIS_LASER, "Axis Laser"),
    (DAMAGE_PHASER_LASER, "Phaser Laser"),
    (DAMAGE_PHASER_LASER_SPLASH, "Phaser Laser (Splash)"),
];

/// Lists the parts of the summed hit keys, as `DAMAGE_DESCRIPTIONS` does.
pub static HITS_DESCRIPTIONS: [(&str, &str); 34] = [
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
    (HITS_BOMB_AVIA, "HE Bomb"),
    (HITS_BOMB_ALT, "Alt Bomb"),
    (HITS_BOMB_AIRSUPPORT, "Air Support Bomb"),
    (HITS_DBOMB_AIRSUPPORT, "Air Support Depth Charge"),
    (HITS_TBOMB_AVIA, "Torpedo Bomber"),
    (HITS_TBOMB_ALT, "Torpedo Bomber (Alt)"),
    (HITS_TBOMB_AIRSUPPORT, "Torpedo Bomber Air Support"),
    (HITS_RAM, "Ram"),
    (HITS_DBOMB_DIRECT, "Depth Charge (Direct)"),
    (HITS_DBOMB_SPLASH, "Depth Charge (Splash)"),
    (HITS_SEA_MINE, "Sea Mine"),
    (HITS_ROCKET_AVIA, "Rocket"),
    (HITS_ROCKET_ALT, "Alt Rocket"),
    (HITS_ROCKET_AIRSUPPORT, "Air Supp Rocket"),
    (HITS_SKIP_AVIA, "Skip Bomb"),
    (HITS_SKIP_ALT, "Alt Skip Bomb"),
    (HITS_SKIP_AIRSUPPORT, "Air Supp Skip Bomb"),
    (HITS_MISSILE, "Missile"),
    (HITS_WAVE, "Wave"),
    (HITS_CHARGE_LASER, "Charge Laser"),
    (HITS_PULSE_LASER, "Pulse Laser"),
    (HITS_AXIS_LASER, "Axis Laser"),
    (HITS_PHASER_LASER, "Phaser Laser"),
    (HITS_PHASER_LASER_SPLASH, "Phaser Laser (Splash)"),
];

/// Keys for received damage lookups. The server uses different key names on the
/// received side (e.g. combined `damage_dbomb` instead of split direct/splash).
/// Lists the parts of the summed keys, as `DAMAGE_DESCRIPTIONS` does.
pub static RECEIVED_DAMAGE_DESCRIPTIONS: [(&str, &str); 34] = [
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
    (DAMAGE_BOMB_AVIA, "HE Bomb"),
    (DAMAGE_BOMB_ALT, "Alt Bomb"),
    (DAMAGE_BOMB_AIRSUPPORT, "Air Support Bomb"),
    (DAMAGE_DBOMB, "Depth Charge"),
    (DAMAGE_DBOMB_AIRSUPPORT, "Air Support Depth Charge"),
    (DAMAGE_TBOMB_AVIA, "Torpedo Bomber"),
    (DAMAGE_TBOMB_ALT, "Torpedo Bomber (Alt)"),
    (DAMAGE_TBOMB_AIRSUPPORT, "Torpedo Bomber Air Support"),
    (DAMAGE_FIRE, "Fire"),
    (DAMAGE_RAM, "Ram"),
    (DAMAGE_FLOOD, "Flood"),
    (DAMAGE_SEA_MINE, "Sea Mine"),
    (DAMAGE_ROCKET_AVIA, "Rocket"),
    (DAMAGE_ROCKET_ALT, "Alt Rocket"),
    (DAMAGE_ROCKET_AIRSUPPORT, "Air Supp Rocket"),
    (DAMAGE_SKIP_AVIA, "Skip Bomb"),
    (DAMAGE_SKIP_ALT, "Alt Skip Bomb"),
    (DAMAGE_SKIP_AIRSUPPORT, "Air Supp Skip Bomb"),
    (DAMAGE_MISSILE, "Missile"),
    (DAMAGE_PHASER_LASER, "Phaser Laser"),
    (DAMAGE_PHASER_LASER_SPLASH, "Phaser Laser (Splash)"),
];

pub static POTENTIAL_DAMAGE_DESCRIPTIONS: [(&str, &str); 4] =
    [("agro_art", "Artillery"), ("agro_tpd", "Torpedo"), ("agro_air", "Planes"), ("agro_dbomb", "Depth Charge")];

/// Weapon group for the recording player's controller damage stats. The
/// controller reports one entry per weapon variant; these fold the variants
/// into the groups the results tables also distinguish. Declaration order is
/// listing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ControllerWeaponGroup {
    MainAp,
    MainCs,
    MainHe,
    Secondary,
    Torpedo,
    TorpedoBomber,
    Bomb,
    Rocket,
    DepthCharge,
    Fire,
    Flood,
    Ram,
    Missile,
    Other,
}

impl ControllerWeaponGroup {
    /// Serialized name.
    pub const fn key(self) -> &'static str {
        match self {
            Self::MainAp => "main_ap",
            Self::MainCs => "main_cs",
            Self::MainHe => "main_he",
            Self::Secondary => "secondary",
            Self::Torpedo => "torpedo",
            Self::TorpedoBomber => "torpedo_bomber",
            Self::Bomb => "bomb",
            Self::Rocket => "rocket",
            Self::DepthCharge => "depth_charge",
            Self::Fire => "fire",
            Self::Flood => "flood",
            Self::Ram => "ram",
            Self::Missile => "missile",
            Self::Other => "other",
        }
    }

    /// Breakdown label, in the style of the results description tables.
    pub const fn label(self) -> &'static str {
        match self {
            Self::MainAp => "AP",
            Self::MainCs => "SAP",
            Self::MainHe => "HE",
            Self::Secondary => "Secondaries",
            Self::Torpedo => "Torps",
            Self::TorpedoBomber => "Torpedo Bomber",
            Self::Bomb => "Bomb",
            Self::Rocket => "Rocket",
            Self::DepthCharge => "Depth Charge",
            Self::Fire => "Fire",
            Self::Flood => "Flood",
            Self::Ram => "Ram",
            Self::Missile => "Missile",
            Self::Other => "Other",
        }
    }

    /// The group a controller weapon is summed under.
    pub fn of(weapon: &Recognized<DamageStatWeapon>) -> Self {
        match weapon.known() {
            Some(DamageStatWeapon::MainAp | DamageStatWeapon::MainAiAp) => Self::MainAp,
            Some(DamageStatWeapon::MainCs) => Self::MainCs,
            Some(DamageStatWeapon::MainHe | DamageStatWeapon::MainAiHe) => Self::MainHe,
            Some(DamageStatWeapon::AtbaAp | DamageStatWeapon::AtbaHe | DamageStatWeapon::AtbaCs) => Self::Secondary,
            Some(
                DamageStatWeapon::Torpedo
                | DamageStatWeapon::TorpedoAcc
                | DamageStatWeapon::TorpedoDeep
                | DamageStatWeapon::TorpedoAlter
                | DamageStatWeapon::TorpedoMag
                | DamageStatWeapon::TorpedoAccOff
                | DamageStatWeapon::TorpedoPhoton,
            ) => Self::Torpedo,
            Some(
                DamageStatWeapon::TBomber
                | DamageStatWeapon::TBomberAsup
                | DamageStatWeapon::TBomberAlter
                | DamageStatWeapon::TBomberTc,
            ) => Self::TorpedoBomber,
            Some(DamageStatWeapon::Burn) => Self::Fire,
            Some(DamageStatWeapon::Flood) => Self::Flood,
            Some(
                DamageStatWeapon::BomberAp
                | DamageStatWeapon::BomberHe
                | DamageStatWeapon::SkipHe
                | DamageStatWeapon::SkipAp
                | DamageStatWeapon::BomberApAsup
                | DamageStatWeapon::BomberHeAsup
                | DamageStatWeapon::SkipHeAsup
                | DamageStatWeapon::SkipApAsup
                | DamageStatWeapon::BomberApAlter
                | DamageStatWeapon::BomberHeAlter
                | DamageStatWeapon::SkipHeAlter
                | DamageStatWeapon::SkipApAlter
                | DamageStatWeapon::BomberApTc
                | DamageStatWeapon::BomberHeTc
                | DamageStatWeapon::SkipHeTc
                | DamageStatWeapon::SkipApTc,
            ) => Self::Bomb,
            Some(
                DamageStatWeapon::RocketHe
                | DamageStatWeapon::RocketAp
                | DamageStatWeapon::RocketHeAsup
                | DamageStatWeapon::RocketApAsup
                | DamageStatWeapon::RocketHeAlter
                | DamageStatWeapon::RocketApAlter
                | DamageStatWeapon::RocketHeTc
                | DamageStatWeapon::RocketApTc,
            ) => Self::Rocket,
            Some(
                DamageStatWeapon::DepthCharge
                | DamageStatWeapon::DepthChargeAsup
                | DamageStatWeapon::DepthChargeAlter
                | DamageStatWeapon::DepthChargeTc,
            ) => Self::DepthCharge,
            Some(DamageStatWeapon::Ram) => Self::Ram,
            Some(DamageStatWeapon::Missile) => Self::Missile,
            _ => Self::Other,
        }
    }
}

impl serde::Serialize for ControllerWeaponGroup {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.key())
    }
}

/// One line of a per-type damage split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageBreakdownLine {
    pub label: &'static str,
    pub amount: u64,
}

/// A per-type damage split, so a front end lists it without knowing which
/// source it came from.
#[derive(Clone, Copy, Debug)]
pub enum DamageBreakdown<'a> {
    /// A results map keyed by the key column of `descriptions`.
    Results {
        descriptions: &'static [(&'static str, &'static str)],
        by_type: &'a BTreeMap<String, u64>,
    },
    Controller(&'a BTreeMap<ControllerWeaponGroup, u64>),
}

impl DamageBreakdown<'_> {
    /// The nonzero lines, in description-table or group declaration order.
    pub fn lines(&self) -> Vec<DamageBreakdownLine> {
        match self {
            Self::Results { descriptions, by_type } => descriptions
                .iter()
                .filter_map(|(key, label)| {
                    let amount = by_type.get(*key).copied()?;
                    (amount > 0).then_some(DamageBreakdownLine { label, amount })
                })
                .collect(),
            Self::Controller(by_type) => by_type
                .iter()
                .filter(|(_, amount)| **amount > 0)
                .map(|(group, amount)| DamageBreakdownLine { label: group.label(), amount: *amount })
                .collect(),
        }
    }
}

/// The recording player's damage for one controller `DamageStatCategory`,
/// from the cumulative `receiveDamageStat` entries.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ControllerDamage {
    pub total: u64,
    /// Only entries > 0.
    pub by_type: BTreeMap<ControllerWeaponGroup, u64>,
}

impl ControllerDamage {
    pub fn breakdown(&self) -> DamageBreakdown<'_> {
        DamageBreakdown::Controller(&self.by_type)
    }
}

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
    /// Damage allies dealt to this victim while this player spotted it
    /// (`scouting_damage`).
    pub spotting_damage: u64,
    /// Share of this player's total spotting damage. `None` when the results
    /// carry no nonzero total to divide by.
    pub spotting_damage_percentage: Option<f64>,
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
    /// self-player controller fallback lives on `NormalizedPlayer`. The
    /// results carry no per-type split of it, only the per-victim one in
    /// `damage_interactions`.
    pub spotting_damage: Option<u64>,
    pub potential_damage: u64,
    pub potential_damage_details: PotentialDamage,
    /// Potential breakdown keyed by the `POTENTIAL_DAMAGE_DESCRIPTIONS` key,
    /// only entries > 0.
    pub potential_damage_by_type: BTreeMap<String, u64>,
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
