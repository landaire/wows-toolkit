use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Millimeters;
use wowsunpack::ballistics::is_overmatch;

use crate::aircraft::OrdnanceKind;
use crate::aircraft::Squadron;
use crate::aircraft::SquadronRole;
use crate::armor::zone_thickness;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::shells::Ammo;
use crate::shells::Battery;
use crate::shells::Shell;
use crate::ship_variants::listing_order;
use crate::ships::Shares;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipCounts;
use crate::ships::ShipFilter;
use crate::ships::Tier;
use crate::ships::TierRange;

const GROUP_LIMIT: usize = 40;

/// Weapons of a ship that the search tests, each reported in its own groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Armament {
    MainBattery,
    SecondaryBattery,
    Aircraft,
    Airstrike,
}

impl Armament {
    fn from_battery(battery: Battery) -> Self {
        match battery {
            Battery::Main => Self::MainBattery,
            Battery::Secondary => Self::SecondaryBattery,
        }
    }

    fn is_main_battery(&self) -> bool {
        *self == Self::MainBattery
    }

    fn is_ordnance(self) -> bool {
        matches!(self, Self::Aircraft | Self::Airstrike)
    }
}

/// Give either `thickness_mm`, or `target_ship` with `zone`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PenetrationSearchArgs {
    /// Plate thickness in mm to test against.
    pub thickness_mm: Option<Millimeters>,

    /// Ship whose armor is the target, e.g. "Des Moines".
    pub target_ship: Option<String>,

    /// Zone or plate of the target ship: "Bow", "Stern", "Cit", "Cas", "SS", or a plate name from ship_armor. A zone uses its thinnest plate above the waterline.
    pub zone: Option<String>,

    /// Only this shell, bomb or rocket type. Omit to get separate HE penetration, SAP penetration and AP overmatch groups in one answer.
    pub ammo: Option<Ammo>,

    /// Apply Inertia Fuse for HE Shells to HE shell penetration. It does not apply to bombs and rockets. Defaults to false.
    pub ifhe: Option<bool>,

    /// Weapons to test, each reported in its own groups: "main_battery", "secondary_battery", "aircraft" (bombs and rockets of carrier squadrons and of squadrons other ships launch) and "airstrike" (air strike consumables). Select aircraft for questions about carriers, planes, bombs or rockets. Omit to test every armament; groups no ship under the filters fires are then left out.
    pub armament: Option<Vec<Armament>>,

    /// Lowest tier to include. With a target ship and no tier bounds, the target's tier +-2 is used.
    pub min_tier: Option<Tier>,

    /// Highest tier to include. With a target ship and no tier bounds, the target's tier +-2 is used.
    pub max_tier: Option<Tier>,

    pub class: Option<ShipClass>,

    /// Nation as GameParams spells it, e.g. "USA".
    pub nation: Option<String>,

    /// Leave out ships in closed testing. Defaults to false: they are searched and counted under `test`. A test copy of a released ship is never searched.
    pub exclude_test_ships: Option<bool>,

    /// Search premium replicas of other ships (e.g. "BA Montana" of Montana, Xuan Wu of Iowa) as separate ships. Defaults to false: they are left out of the lists, counts and shares and named in their original's entry, unless a nation filter leaves out the original.
    pub include_replicas: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefeatMethod {
    HePenetration,
    SapPenetration,
    ApOvermatch,
}

impl DefeatMethod {
    fn for_ammo(ammo: Ammo) -> Self {
        match ammo {
            Ammo::He => Self::HePenetration,
            Ammo::Sap => Self::SapPenetration,
            Ammo::Ap => Self::ApOvermatch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TierSource {
    /// No tier bounds: every tier.
    All,
    Requested,
    /// The target ship's tier +-2, the matchmaking spread.
    TargetMatchmaking,
}

#[derive(Debug, Clone, Serialize)]
pub struct PenetrationSearchOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub thickness_mm: Millimeters,

    /// Target ship and zone, when given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,

    /// Plates of the target zone that have this thickness, by their game names.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plates: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<TierRange>,
    pub tier_source: TierSource,

    pub ifhe: bool,
    pub rule: &'static str,

    /// How bombs and rockets are tested; present with aircraft or airstrike groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordnance_rule: Option<&'static str>,

    pub groups: Vec<PenetrationGroup>,
}

/// Ships that defeat the plate with one armament by one method, out of those
/// that could.
#[derive(Debug, Clone, Serialize)]
pub struct PenetrationGroup {
    /// Absent for the main battery.
    #[serde(skip_serializing_if = "Armament::is_main_battery")]
    pub armament: Armament,

    pub method: DefeatMethod,

    /// Ships matching the filters whose armament fires this method's ammo
    /// with a known penetration or caliber.
    pub considered: ShipCounts,

    /// Ships among those considered that defeat the plate; listed in `ships`.
    pub qualifying: ShipCounts,

    #[serde(flatten)]
    pub shares: Shares,

    /// "T10 Gearing (destroyer, USA): main battery 127 mm HE, 21 mm pen" or
    /// "T10 Shinano (aircraft_carrier, Japan): B7A Ryusei skip bombers, 460 mm
    /// AP skip bomb (Type 99 No. 80 Mk. 7)", the best weapon per ship. A SAP
    /// projectile that also overmatches the plate, and so cannot ricochet off
    /// it, ends with ", overmatches".
    pub ships: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,
}

/// One shell, bomb or rocket as the search compares it against a plate.
#[derive(Debug, Clone)]
struct Munition {
    armament: Armament,
    ammo: Ammo,

    /// Known for shells and AP bombs and rockets. HE bombs and rockets list a
    /// placeholder `bulletDiametr` (40 mm), so aircraft HE and SAP have none.
    caliber: Option<Millimeters>,

    /// HE or SAP penetration; for HE shells with Inertia Fuse for HE Shells
    /// when requested.
    penetration: Option<Millimeters>,

    source: MunitionSource,
}

impl Munition {
    fn from_shell(shell: Shell, ifhe: bool) -> Self {
        let penetration = match shell.ammo {
            Ammo::He if ifhe => shell.ifhe_penetration_mm,
            Ammo::He | Ammo::Sap => shell.penetration_mm,
            Ammo::Ap => None,
        };

        Self {
            armament: Armament::from_battery(shell.battery),
            ammo: shell.ammo,
            caliber: Some(shell.caliber_mm),
            penetration,
            source: MunitionSource::Gun(shell.battery),
        }
    }

    /// The bomb or rocket of `squadron`; `None` for torpedoes, mines, depth
    /// charges and ordnance of no known shell type.
    fn from_squadron(squadron: Squadron) -> Option<Self> {
        let ordnance = squadron.ordnance?;
        let ammo = ordnance.ammo?;

        // Inertia Fuse for HE Shells does not apply to aircraft ordnance.
        let (penetration, caliber) = match ammo {
            Ammo::He => (ordnance.he_penetration_mm, None),
            Ammo::Sap => (ordnance.sap_penetration_mm, None),
            Ammo::Ap => (None, ordnance.ap.and_then(|ap| ap.caliber_mm)),
        };
        let armament = match squadron.role {
            SquadronRole::AirStrike => Armament::Airstrike,
            SquadronRole::RocketPlanes
            | SquadronRole::DiveBombers
            | SquadronRole::TorpedoBombers
            | SquadronRole::SkipBombers => Armament::Aircraft,
        };

        Some(Self {
            armament,
            ammo,
            caliber,
            penetration,
            source: MunitionSource::Squadron {
                aircraft: squadron.aircraft,
                role: squadron.role,
                kind: ordnance.kind,
                name: ordnance.name,
            },
        })
    }
}

#[derive(Debug, Clone)]
enum MunitionSource {
    Gun(Battery),
    Squadron { aircraft: String, role: SquadronRole, kind: OrdnanceKind, name: String },
}

impl GameData {
    /// Ships whose shells, bombs or rockets defeat a plate: HE or SAP whose
    /// penetration is at least the thickness, or AP whose caliber overmatches
    /// it.
    pub fn penetration_search(&self, args: &PenetrationSearchArgs) -> Result<PenetrationSearchOutput, QueryError> {
        const EVERY_ARMAMENT: [Armament; 4] =
            [Armament::MainBattery, Armament::SecondaryBattery, Armament::Aircraft, Armament::Airstrike];

        let mut interpretations = Interpretations::default();
        let target = match args.target_ship.as_deref() {
            Some(name) => Some(self.resolve_ship(name, &mut interpretations)?),
            None => None,
        };

        // A target zone is authoritative: models often add a thickness they
        // picked from ship_armor, and the zone rule is the one documented.
        let (thickness, plates, target_label) = match (target, args.zone.as_deref(), args.thickness_mm) {
            (Some(ship), Some(zone), _) => {
                let zone_armor = zone_thickness(self, ship, zone)?;
                (zone_armor.thickness, zone_armor.plates, Some(format!("{} {}", ship.label(), zone.trim())))
            }
            (_, _, Some(thickness)) if thickness.value() > 0.0 => (thickness, Vec::new(), None),
            _ => {
                return Err(QueryError::InvalidArguments {
                    reason: "give thickness_mm above 0, or target_ship together with zone",
                });
            }
        };

        let requested = TierRange::from_bounds(args.min_tier, args.max_tier);
        let (tiers, tier_source) = match (requested, target) {
            (Some(range), _) => (Some(range), TierSource::Requested),
            (None, Some(ship)) => (Some(TierRange::around(ship.tier)), TierSource::TargetMatchmaking),
            (None, None) => (None, TierSource::All),
        };
        let filter = ShipFilter {
            tiers,
            class: args.class,
            nation: args.nation.clone(),
            include_irregular: false,
            exclude_test: args.exclude_test_ships.unwrap_or(false),
            include_replicas: args.include_replicas.unwrap_or(false),
        };

        let ifhe = args.ifhe.unwrap_or(false);
        let mut armaments = args.armament.clone().unwrap_or_else(|| EVERY_ARMAMENT.to_vec());
        armaments.sort();
        armaments.dedup();
        if armaments.is_empty() {
            return Err(QueryError::InvalidArguments { reason: "armament must name at least one armament" });
        }
        let methods: Vec<DefeatMethod> = match args.ammo {
            Some(ammo) => vec![DefeatMethod::for_ammo(ammo)],
            None => vec![DefeatMethod::HePenetration, DefeatMethod::SapPenetration, DefeatMethod::ApOvermatch],
        };
        let keys: Vec<(Armament, DefeatMethod)> =
            armaments.iter().flat_map(|armament| methods.iter().map(move |method| (*armament, *method))).collect();

        let mut considered: BTreeMap<(Armament, DefeatMethod), Vec<&Ship>> = BTreeMap::new();
        let mut hits: BTreeMap<(Armament, DefeatMethod), Vec<(&Ship, Munition)>> = BTreeMap::new();
        let mut ships: Vec<&Ship> = self.ships.iter().filter(|ship| filter.matches(ship)).collect();
        ships.sort_by(|left, right| listing_order(left, right));
        for ship in ships {
            let munitions = self.munitions(ship, &armaments, ifhe);
            for &(armament, method) in &keys {
                let fitting = || munitions.iter().filter(move |munition| munition.armament == armament);
                if fitting().any(|munition| rating(munition, method).is_some()) {
                    considered.entry((armament, method)).or_default().push(ship);
                }

                let best = fitting()
                    .filter_map(|munition| strength(munition, method, thickness).map(|strength| (munition, strength)))
                    .max_by(|(_, left), (_, right)| left.value().total_cmp(&right.value()));
                if let Some((munition, _)) = best {
                    hits.entry((armament, method)).or_default().push((ship, munition.clone()));
                }
            }
        }

        let groups: Vec<PenetrationGroup> = keys
            .iter()
            .filter_map(|key| {
                let candidates = considered.remove(key).unwrap_or_default();
                // Unrequested armaments appear only where some ship fires the ammo.
                if candidates.is_empty() && args.armament.is_none() {
                    return None;
                }

                let found = hits.remove(key).unwrap_or_default();
                Some(group(key.0, key.1, &candidates, &found, thickness))
            })
            .collect();
        let ordnance_rule = groups.iter().any(|group| group.armament.is_ordnance()).then_some(
            "Bombs and rockets follow the same rules: on impact the game tests AP bombs and rockets with the AP shell mechanics, overmatch included. Inertia Fuse for HE Shells does not apply to them.",
        );

        Ok(PenetrationSearchOutput {
            interpretations,
            thickness_mm: thickness,
            target: target_label,
            plates,
            tiers,
            tier_source,
            ifhe,
            rule: "AP defeats a plate by overmatch when caliber > 14.3 x thickness. HE and SAP need penetration >= thickness. SAP also overmatches when caliber > 14.3 x thickness, and then cannot ricochet off the plate at any angle; without overmatch it ricochets at steep angles. HE never overmatches.",
            ordnance_rule,
            groups,
        })
    }

    /// The shells, bombs and rockets `ship` fires or drops with `armaments`.
    fn munitions(&self, ship: &Ship, armaments: &[Armament], ifhe: bool) -> Vec<Munition> {
        let mut munitions = Vec::new();
        if armaments.iter().any(|armament| !armament.is_ordnance()) {
            munitions.extend(self.shells(ship).into_iter().map(|shell| Munition::from_shell(shell, ifhe)));
        }
        if armaments.iter().any(|armament| armament.is_ordnance()) {
            munitions.extend(self.squadrons(ship).into_iter().filter_map(Munition::from_squadron));
        }
        munitions.retain(|munition| armaments.contains(&munition.armament));

        munitions
    }
}

/// What `method` compares against a plate for `munition`: caliber for AP
/// overmatch, penetration for HE and SAP. `None` when the munition is of
/// another ammo or the value is not known.
fn rating(munition: &Munition, method: DefeatMethod) -> Option<Millimeters> {
    match (method, munition.ammo) {
        (DefeatMethod::ApOvermatch, Ammo::Ap) => munition.caliber,
        (DefeatMethod::HePenetration, Ammo::He) | (DefeatMethod::SapPenetration, Ammo::Sap) => munition.penetration,
        _ => None,
    }
}

/// How strongly `munition` defeats `thickness` by `method`, for choosing the
/// best one; `None` when it does not.
fn strength(munition: &Munition, method: DefeatMethod, thickness: Millimeters) -> Option<Millimeters> {
    let rating = rating(munition, method)?;
    let defeats = match method {
        DefeatMethod::ApOvermatch => is_overmatch(rating, thickness),
        DefeatMethod::HePenetration | DefeatMethod::SapPenetration => rating >= thickness,
    };

    defeats.then_some(rating)
}

fn group(
    armament: Armament,
    method: DefeatMethod,
    considered: &[&Ship],
    hits: &[(&Ship, Munition)],
    thickness: Millimeters,
) -> PenetrationGroup {
    let ships: Vec<String> =
        hits.iter().take(GROUP_LIMIT).map(|(ship, munition)| describe_hit(ship, munition, thickness)).collect();
    let omitted = (hits.len() > ships.len()).then(|| hits.len() - ships.len());

    let considered = ShipCounts::from_ships(considered.iter().copied());
    let qualifying = ShipCounts::from_ships(hits.iter().map(|(ship, _)| *ship));

    PenetrationGroup {
        armament,
        method,
        shares: Shares::new(&qualifying.by_class, &considered),
        considered,
        qualifying,
        ships,
        omitted,
    }
}

fn describe_hit(ship: &Ship, munition: &Munition, thickness: Millimeters) -> String {
    let caliber = munition.caliber.map(|mm| format!("{} mm ", mm.value())).unwrap_or_default();
    let ammo = munition.ammo.label();
    let penetration = munition.penetration.map(|mm| format!(", {} mm pen", mm.value())).unwrap_or_default();
    let overmatch = if munition.ammo == Ammo::Sap && munition.caliber.is_some_and(|mm| is_overmatch(mm, thickness)) {
        ", overmatches"
    } else {
        ""
    };

    let weapon = match &munition.source {
        MunitionSource::Gun(battery) => format!("{} {caliber}{ammo}", battery.label()),
        MunitionSource::Squadron { aircraft, role, kind, name } => {
            format!("{aircraft} {}, {caliber}{ammo} {} ({name})", role.label(), kind.label())
        }
    };

    format!("{}: {weapon}{penetration}{overmatch}", ship.listing_label())
}
