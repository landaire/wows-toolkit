//! Egui-free numeric result types: per-type damage/hit breakdowns and the
//! server- and observed-results holders built from resolved battle results.

use std::collections::HashMap;

use wows_replays::types::AccountId;

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
#[derive(Clone, Debug, serde::Serialize)]
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
#[derive(Clone, Debug, serde::Serialize)]
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
    pub damage_dealt_percentage: f64,
    pub damage_dealt_inverse_percentage: f64,
    pub damage_received: u64,
    pub damage_received_by_type: Damage,
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
    pub xp: i64,
    pub raw_xp: i64,
    pub damage: u64,
    pub damage_details: Damage,
    pub hits_details: Hits,
    pub spotting_damage: u64,
    pub potential_damage: u64,
    pub potential_damage_details: PotentialDamage,
    pub received_damage: u64,
    pub received_damage_details: Damage,
    pub fires_dealt: u64,
    pub floods_dealt: u64,
    pub citadels_dealt: u64,
    pub crits_dealt: u64,
    pub distance_traveled: f64,
    pub kills: i64,
    pub damage_interactions: HashMap<AccountId, DamageInteraction>,
}
