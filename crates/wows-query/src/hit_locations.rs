//! Hull sections of a ship with their own HP pools, and how they saturate.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Percent;
use wowsunpack::game_params::ttx::components::HullHitLocation;
use wowsunpack::game_params::ttx::components::HullSectionType;
use wowsunpack::game_params::ttx::model::Hp;

use crate::data::GameData;
use crate::error::QueryError;
use crate::firing_arcs::round_to;
use crate::modules::ModuleKind;
use crate::names::Interpretations;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipSummary;

// Rules beyond the game data follow the Wargaming wiki article
// "Ship:Damage Saturation" (2021-03-31), whose 75% hull pool and 16.5% shares
// match every hull's `maxHP` and `volumeCoeff`.
const EXPLANATION: &str = "Damage saturation: a ship's HP is split among hull sections (bow, stern, casemate or midsection, superstructure, auxiliary rooms, citadel), each with its own HP pool, plus a hull pool of 75% of ship HP that has no geometry of its own. A penetrating hit drains the pool of the section it damages and, for a section that lists drains, the hull pool too; overpenetrations, fire and flooding drain only ship HP. penetration_damage_pct is the share of an AP shell's listed damage a penetrating hit deals there while the pools it drains have HP: 33% on most sections, half from each pool, and 100% on the citadel. Once a section's own pool is spent it is saturated (black in the damage model) and penetrations there deal only saturated_damage_pct, half as much. Once the hull pool is spent, every section that drains it counts as saturated and penetrations deal 10% (Wargaming wiki). The citadel never saturates, and neither does the midsection of most destroyers, which always takes its full 33%. Repair Party restores up to repairable_pct of the damage a section has taken (Halland's unique upgrade raises it on every section but the citadel), and its healing refills every section's pool in proportion to what that section has lost, so a healed section takes its full share again. French battleships and cruisers have no special saturation: their sections follow the same rules as those of other nations. \"French saturation\" is a destroyer trait: the midsection of most French destroyers (not Aigle or Kleber TE) and of a few others, such as Shigure and Blyskawica '52, has a pool that saturates, so past that much damage there they take half damage instead of full damage.";

/// Arguments for `ship_hit_locations`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipHitLocationsArgs {
    /// Ship to list the hull sections of, e.g. "Republique".
    pub ship: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipHitLocationsOutput {
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

    /// How saturation works, for every ship.
    pub explanation: &'static str,

    /// One entry per hull module, stock first.
    pub hulls: Vec<HullSections>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HullSections {
    /// English module name, e.g. "Hull A".
    pub hull: String,

    pub stock: bool,

    /// HP of the ship with this hull, before upgrades and skills.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ship_hp: Option<Hp>,

    pub sections: Vec<Section>,

    /// What sets this hull apart from the rules in `explanation`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    /// English name, e.g. "bow" or "casemate (midsection)".
    pub section: String,

    /// Key of the section in GameParams, e.g. "Bow", "Cas", "SSC".
    pub game_key: String,

    /// Whether the pool can run out before the ship sinks.
    pub saturates: bool,

    /// HP of the pool. Present when it saturates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_hp: Option<Hp>,

    /// Pool HP as a share of ship HP. Present when it saturates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_of_ship_hp_pct: Option<Percent>,

    /// Share of an AP shell's listed damage a penetrating hit here deals
    /// while the pools it drains have HP. Absent for the hull pool, which has
    /// no geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_damage_pct: Option<Percent>,

    /// Share once this section's pool is spent while the hull pool has HP.
    /// Present for saturating sections that drain the hull pool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturated_damage_pct: Option<Percent>,

    /// The pool that damage to this section also drains.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drains: Option<String>,

    /// Share of the damage taken here that Repair Party can restore, before
    /// upgrades.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repairable_pct: Option<Percent>,

    /// Larger share Repair Party can restore of damage from submarine and
    /// aircraft torpedoes. Present when it differs from `repairable_pct`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repairable_pct_from_torpedoes: Option<Percent>,
}

impl GameData {
    /// The hull sections of `args.ship` for each of its hulls.
    pub fn ship_hit_locations(&self, args: &ShipHitLocationsArgs) -> Result<ShipHitLocationsOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;

        let components = ship.param.vehicle().and_then(|vehicle| vehicle.ttx_components());
        let mut hull_modules: Vec<_> =
            self.ship_modules(ship).into_iter().filter(|module| module.kind == ModuleKind::Hull).collect();
        hull_modules.sort_by_key(|module| !module.stock);

        let mut hulls = Vec::new();
        for module in hull_modules {
            let Some(hull) = components.and_then(|components| components.hull(&module.param)) else {
                continue;
            };
            if hull.hit_locations.is_empty() {
                continue;
            }

            let mut ordered: Vec<&HullHitLocation> = hull.hit_locations.iter().collect();
            ordered.sort_by(|left, right| section_order(left).cmp(&section_order(right)));
            let sections: Vec<Section> =
                ordered.into_iter().map(|location| section(location, &hull.hit_locations, hull.health)).collect();

            hulls.push(HullSections {
                hull: module.name,
                stock: module.stock,
                ship_hp: hull.health,
                notes: notes(ship, &sections),
                sections,
            });
        }

        if hulls.is_empty() {
            return Err(QueryError::HitLocationsMissing { ship: ship.name.clone() });
        }

        Ok(ShipHitLocationsOutput { interpretations, ship: ship.summary(), explanation: EXPLANATION, hulls })
    }
}

/// Display order: the extremities, the midsection and what sits on it, the
/// citadel, then the hull pool. Other keys follow by name.
fn section_order(location: &HullHitLocation) -> (u8, &str) {
    let rank = match location.section_type {
        HullSectionType::Hull => 7,
        HullSectionType::Citadel => 5,
        HullSectionType::Superstructure => 4,
        HullSectionType::Casemate => 2,
        HullSectionType::Simple => match location.name.as_str() {
            "Bow" => 0,
            "St" => 1,
            "Cas" => 2,
            "SSC" => 3,
            "SS" => 4,
            _ => 6,
        },
    };

    (rank, location.name.as_str())
}

/// `location` of a hull whose sections are `all` and whose HP is `ship_hp`.
fn section(location: &HullHitLocation, all: &[HullHitLocation], ship_hp: Option<Hp>) -> Section {
    // A pool larger than the ship's HP cannot run out: game data gives
    // sections that never saturate 165000 or 165500 HP.
    let saturates = match (location.max_hp, ship_hp) {
        (Some(pool), Some(ship)) => pool.value() < ship.value(),
        _ => false,
    };
    let pool_hp = location.max_hp.filter(|_| saturates);
    let share_of_ship_hp_pct =
        pool_hp.zip(ship_hp).map(|(pool, ship)| Percent::new(round_to(pool.value() / ship.value() * 100.0, 10.0)));

    // A penetration splits its damage between the section and the pool it
    // drains, each taking its own `volumeCoeff` while it has HP.
    let parent = location.parent.as_deref().map(|name| all.iter().find(|other| other.name == name));
    let parent_share = match parent {
        None => Some(0.0),
        Some(found) => found.and_then(|parent| parent.volume_coeff),
    };
    let has_geometry = location.section_type != HullSectionType::Hull;
    let penetration_damage_pct = location
        .volume_coeff
        .zip(parent_share)
        .filter(|_| has_geometry)
        .map(|(own, parent)| fraction_pct(own + parent));
    let saturated_damage_pct =
        parent.flatten().and_then(|parent| parent.volume_coeff).filter(|_| saturates && has_geometry).map(fraction_pct);

    let repairable = location.regenerated_hp_part;
    let repairable_from_torpedoes = location
        .enhanced_regenerated_hp_part
        .filter(|enhanced| repairable.is_none_or(|part| *enhanced > part))
        .map(fraction_pct);

    Section {
        section: section_name(location),
        game_key: location.name.clone(),
        saturates,
        pool_hp,
        share_of_ship_hp_pct,
        penetration_damage_pct,
        saturated_damage_pct,
        drains: parent.flatten().map(section_name),
        repairable_pct: repairable.map(fraction_pct),
        repairable_pct_from_torpedoes: repairable_from_torpedoes,
    }
}

fn section_name(location: &HullHitLocation) -> String {
    let name = match location.section_type {
        HullSectionType::Hull => "hull pool",
        HullSectionType::Citadel => "citadel",
        HullSectionType::Superstructure => "superstructure",
        HullSectionType::Casemate => "casemate (midsection)",
        HullSectionType::Simple => match location.name.as_str() {
            "Bow" => "bow",
            "St" => "stern",
            "Cas" => "casemate (midsection)",
            "SSC" => "auxiliary rooms",
            "SS" => "superstructure",
            other => other,
        },
    };

    name.to_owned()
}

/// A 0 to 1 fraction as a percent rounded to 0.1, so 0.165 reads 16.5.
fn fraction_pct(fraction: f32) -> Percent {
    Percent::new(round_to(fraction * 100.0, 10.0))
}

fn notes(ship: &Ship, sections: &[Section]) -> Vec<String> {
    let mut notes = Vec::new();
    if ship.class != ShipClass::Destroyer {
        return notes;
    }

    let Some(midsection) = sections.iter().find(|section| section.game_key == "Cas") else {
        return notes;
    };

    if let (Some(pool), Some(share)) = (midsection.pool_hp, midsection.share_of_ship_hp_pct) {
        notes.push(format!(
            "French destroyer saturation: the midsection has a pool of {pool} HP ({}% of ship HP); past that much damage there, penetrations deal half damage. The midsection of most destroyers never saturates.",
            share.value()
        ));
    } else {
        notes.push(
            "The midsection never saturates: penetrations there always deal their full share, never half of it. Destroyers with French destroyer saturation have a midsection pool instead."
                .to_owned(),
        );
    }

    notes
}
