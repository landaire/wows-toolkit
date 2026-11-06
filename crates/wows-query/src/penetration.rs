use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Millimeters;
use wowsunpack::ballistics::is_overmatch;

use crate::armor::zone_thickness;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::shells::Ammo;
use crate::shells::Battery;
use crate::shells::Shell;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipCounts;
use crate::ships::ShipFilter;
use crate::ships::Tier;
use crate::ships::TierRange;

const GROUP_LIMIT: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BatteryFilter {
    #[default]
    Main,
    Secondary,
    Any,
}

impl BatteryFilter {
    fn includes(self, battery: Battery) -> bool {
        match self {
            Self::Main => battery == Battery::Main,
            Self::Secondary => battery == Battery::Secondary,
            Self::Any => true,
        }
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

    /// Only this shell type. Omit to get separate HE penetration, SAP penetration and AP overmatch groups in one answer.
    pub ammo: Option<Ammo>,

    /// Apply Inertia Fuse for HE Shells to HE penetration. Defaults to false.
    pub ifhe: Option<bool>,

    /// Which guns to consider. Defaults to main.
    pub battery: Option<BatteryFilter>,

    /// Lowest tier to include. With a target ship and no tier bounds, the target's tier +-2 is used.
    pub min_tier: Option<Tier>,

    /// Highest tier to include. With a target ship and no tier bounds, the target's tier +-2 is used.
    pub max_tier: Option<Tier>,

    pub class: Option<ShipClass>,

    /// Nation as GameParams spells it, e.g. "USA".
    pub nation: Option<String>,

    /// Leave out ships in closed testing. Defaults to false: they are searched and counted under `test`. A test copy of a released ship is never searched.
    pub exclude_test_ships: Option<bool>,
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
    /// Names read as something they do not spell exactly.
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
    pub groups: Vec<PenetrationGroup>,
}

/// Ships that defeat the plate by one method, out of those that could.
#[derive(Debug, Clone, Serialize)]
pub struct PenetrationGroup {
    pub method: DefeatMethod,

    /// Ships matching the filters whose chosen battery fires this method's
    /// ammo with a known penetration or caliber.
    pub considered: ShipCounts,

    /// Ships among those considered that defeat the plate; listed in `ships`.
    pub qualifying: ShipCounts,

    /// "T10 Gearing (destroyer, USA): main 127 mm HE, 21 mm pen", best shell per ship.
    pub ships: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,
}

impl GameData {
    /// Ships whose shells defeat a plate outright: HE or SAP whose penetration
    /// is at least the thickness, or AP whose caliber overmatches it.
    pub fn penetration_search(&self, args: &PenetrationSearchArgs) -> Result<PenetrationSearchOutput, QueryError> {
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
        };

        let ifhe = args.ifhe.unwrap_or(false);
        let battery = args.battery.unwrap_or_default();
        let methods: Vec<DefeatMethod> = match args.ammo {
            Some(ammo) => vec![DefeatMethod::for_ammo(ammo)],
            None => vec![DefeatMethod::HePenetration, DefeatMethod::SapPenetration, DefeatMethod::ApOvermatch],
        };

        let mut considered: BTreeMap<DefeatMethod, Vec<&Ship>> = BTreeMap::new();
        let mut hits: BTreeMap<DefeatMethod, Vec<(&Ship, Shell)>> = BTreeMap::new();
        let mut ships: Vec<&Ship> = self.ships.iter().filter(|ship| filter.matches(ship)).collect();
        ships.sort_by(|left, right| right.tier.cmp(&left.tier).then_with(|| left.name.cmp(&right.name)));
        for ship in ships {
            let shells: Vec<Shell> =
                self.shells(ship).into_iter().filter(|shell| battery.includes(shell.battery)).collect();
            for &method in &methods {
                if shells.iter().any(|shell| rating(shell, method, ifhe).is_some()) {
                    considered.entry(method).or_default().push(ship);
                }

                let best = shells
                    .iter()
                    .filter_map(|shell| strength(shell, method, thickness, ifhe).map(|strength| (shell, strength)))
                    .max_by(|(_, left), (_, right)| left.value().total_cmp(&right.value()));
                if let Some((shell, _)) = best {
                    hits.entry(method).or_default().push((ship, shell.clone()));
                }
            }
        }

        let groups = methods
            .iter()
            .map(|method| {
                let candidates = considered.remove(method).unwrap_or_default();
                let found = hits.remove(method).unwrap_or_default();
                group(*method, &candidates, &found, ifhe)
            })
            .collect();

        Ok(PenetrationSearchOutput {
            interpretations,
            thickness_mm: thickness,
            target: target_label,
            plates,
            tiers,
            tier_source,
            ifhe,
            rule: "AP defeats a plate by overmatch when caliber > 14.3 x thickness. HE and SAP need penetration >= thickness. HE and SAP never overmatch.",
            groups,
        })
    }
}

/// What `method` compares against a plate for `shell`: caliber for AP
/// overmatch, penetration for HE and SAP. `None` when the shell is of another
/// ammo or the value is not known.
fn rating(shell: &Shell, method: DefeatMethod, ifhe: bool) -> Option<Millimeters> {
    match (method, shell.ammo) {
        (DefeatMethod::ApOvermatch, Ammo::Ap) => Some(shell.caliber_mm),
        (DefeatMethod::HePenetration, Ammo::He) => {
            if ifhe {
                shell.ifhe_penetration_mm
            } else {
                shell.penetration_mm
            }
        }
        (DefeatMethod::SapPenetration, Ammo::Sap) => shell.penetration_mm,
        _ => None,
    }
}

/// How strongly `shell` defeats `thickness` by `method`, for choosing the
/// best shell; `None` when it does not.
fn strength(shell: &Shell, method: DefeatMethod, thickness: Millimeters, ifhe: bool) -> Option<Millimeters> {
    let rating = rating(shell, method, ifhe)?;
    let defeats = match method {
        DefeatMethod::ApOvermatch => is_overmatch(shell.caliber_mm, thickness),
        DefeatMethod::HePenetration | DefeatMethod::SapPenetration => rating >= thickness,
    };

    defeats.then_some(rating)
}

fn group(method: DefeatMethod, considered: &[&Ship], hits: &[(&Ship, Shell)], ifhe: bool) -> PenetrationGroup {
    let ships: Vec<String> =
        hits.iter().take(GROUP_LIMIT).map(|(ship, shell)| describe_hit(ship, shell, ifhe)).collect();
    let omitted = (hits.len() > ships.len()).then(|| hits.len() - ships.len());

    PenetrationGroup {
        method,
        considered: ShipCounts::from_ships(considered.iter().copied()),
        qualifying: ShipCounts::from_ships(hits.iter().map(|(ship, _)| *ship)),
        ships,
        omitted,
    }
}

fn describe_hit(ship: &Ship, shell: &Shell, ifhe: bool) -> String {
    let battery = match shell.battery {
        Battery::Main => "main",
        Battery::Secondary => "secondary",
    };
    let penetration = match shell.ammo {
        Ammo::He if ifhe => shell.ifhe_penetration_mm,
        Ammo::He | Ammo::Sap => shell.penetration_mm,
        Ammo::Ap => None,
    };
    let penetration = penetration.map(|mm| format!(", {} mm pen", mm.value())).unwrap_or_default();

    format!("{}: {battery} {} mm {}{penetration}", ship.label(), shell.caliber_mm.value(), shell.ammo.label())
}
