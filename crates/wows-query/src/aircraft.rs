//! Squadrons of carriers, hybrid ships and ships with air strikes, and whether
//! their ordnance defeats a plate.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Degrees;
use wows_core::units::Kilograms;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::MetersPerSecond;
use wows_core::units::Millimeters;
use wows_core::units::Percent;
use wows_core::units::Seconds;
use wows_ship_stats::FireArmament;
use wows_ship_stats::FireInputs;
use wowsunpack::ballistics::is_overmatch;
use wowsunpack::game_params::translations::translate_unit;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::components::PlaneSlot;
use wowsunpack::game_params::ttx::constants::TORPEDO_DAMAGE_CONSTANT;
use wowsunpack::game_params::ttx::model::Hp;
use wowsunpack::game_params::ttx::model::Knots;
use wowsunpack::game_params::types::Aircraft;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Projectile;

use crate::builds::Better;
use crate::builds::BuildStat;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatPlan;
use crate::data::GameData;
use crate::error::QueryError;
use crate::firing_arcs::round_to;
use crate::names::Interpretations;
use crate::shells::Ammo;
use crate::shells::ricochet_angles;
use crate::shells::shell_ammo;
use crate::ships::Ship;
use crate::ships::ShipSummary;
use crate::text::clean;

/// Arguments for `ship_aircraft`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipAircraftArgs {
    /// Ship name, e.g. "Midway", "Ise" or "Gouden Leeuw".
    pub ship: String,

    /// Plate thickness in mm to test each squadron's ordnance against.
    pub thickness_mm: Option<Millimeters>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipAircraftOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,
    pub squadron_count: usize,
    pub squadrons: Vec<Squadron>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thickness_mm: Option<Millimeters>,

    /// How `vs_plate` is decided; present with `thickness_mm`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<&'static str>,
}

/// What a squadron is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SquadronRole {
    RocketPlanes,
    DiveBombers,
    TorpedoBombers,
    SkipBombers,
    AirStrike,
}

impl SquadronRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::RocketPlanes => "rocket planes",
            Self::DiveBombers => "dive bombers",
            Self::TorpedoBombers => "torpedo bombers",
            Self::SkipBombers => "skip bombers",
            Self::AirStrike => "air strike",
        }
    }

    fn from_slot(slot: PlaneSlot) -> Self {
        match slot {
            PlaneSlot::Fighter => Self::RocketPlanes,
            PlaneSlot::DiveBomber => Self::DiveBombers,
            PlaneSlot::TorpedoBomber => Self::TorpedoBombers,
            PlaneSlot::SkipBomber => Self::SkipBombers,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Squadron {
    pub role: SquadronRole,

    /// A jet or turboprop squadron flown instead of the regular one of its
    /// slot (`PlaneSubtypes.ALTERNATIVE_TYPES`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub alternative: bool,

    /// Carried by a researched plane module rather than the stock one.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub module_upgrade: bool,

    pub aircraft: String,

    /// GameParams name of the aircraft.
    pub aircraft_param: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub squadron_size: Option<u32>,

    /// Planes that attack together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_flight_size: Option<u32>,

    /// Bombs, rockets or torpedoes each attacking plane releases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_per_plane: Option<u32>,

    /// Cruise speed while carrying ordnance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_with_ordnance_knots: Option<Knots>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordnance: Option<Ordnance>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub air_strike: Option<AirStrike>,

    /// Fire chance of one projectile: base and best-fit build, with the
    /// skills that raise it. Absent when no module combination gives it a
    /// fire chance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_chance: Option<BuildStat<Percent>>,

    /// The ordnance against `thickness_mm`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_plate: Option<PlateVerdict>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AirStrike {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charges: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload_s: Option<Seconds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_km: Option<Km>,
}

/// The kind of projectile an aircraft releases (`planeAmmoType`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrdnanceKind {
    Bomb,
    SkipBomb,
    Rocket,
    Torpedo,
    DepthCharge,
    Mine,
    /// A `planeAmmoType` this crate does not recognize.
    Other(String),
}

impl OrdnanceKind {
    fn from_plane_ammo_type(kind: &str) -> Self {
        match kind {
            "bomb" => Self::Bomb,
            "skip" => Self::SkipBomb,
            "rocket" => Self::Rocket,
            "torpedo" => Self::Torpedo,
            "depthcharge" => Self::DepthCharge,
            "mine" => Self::Mine,
            other => Self::Other(other.to_owned()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Bomb => "bomb",
            Self::SkipBomb => "skip bomb",
            Self::Rocket => "rocket",
            Self::Torpedo => "torpedo",
            Self::DepthCharge => "depth charge",
            Self::Mine => "mine",
            Self::Other(kind) => kind,
        }
    }

    /// Weapons that strike below the waterline and never test a plate.
    fn is_underwater(&self) -> bool {
        matches!(self, Self::Torpedo | Self::DepthCharge | Self::Mine)
    }
}

/// One aircraft's ordnance.
#[derive(Debug, Clone, Serialize)]
pub struct Ordnance {
    pub kind: OrdnanceKind,

    /// Shell type of bombs and rockets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ammo: Option<Ammo>,

    /// English name of the projectile.
    pub name: String,

    /// GameParams name of the projectile.
    pub param: String,

    /// Damage per projectile as the port shows it, rounded to a whole
    /// number. For torpedoes this is `alphaDamage / TORPEDO_DAMAGE_CONSTANT
    /// + damage` (`getTorpedoDamage`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage: Option<Hp>,

    /// Chance one bomb, rocket or other projectile starts a fire on a hit,
    /// without skills, as the port shows it. 0 when it cannot start fires.
    pub fire_chance_pct: Percent,

    /// `fallingTimeCoef`, the multiplier on the bomb's fall time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fall_time_coef: Option<f32>,

    /// HE penetration as the port shows it, `floor(alphaPiercingHE)`. No
    /// skill or upgrade changes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub he_penetration_mm: Option<Millimeters>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sap_penetration_mm: Option<Millimeters>,

    /// Inputs of the server-side AP penetration check, for AP bombs and rockets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap: Option<ApOrdnance>,
}

/// The fields that decide whether an AP bomb or rocket penetrates a plate it
/// does not overmatch. The client has no formula combining them; the server
/// decides.
#[derive(Debug, Clone, Serialize)]
pub struct ApOrdnance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caliber_mm: Option<Millimeters>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass_kg: Option<Kilograms>,

    /// `bulletSpeed`. The client uses drop kinematics only for fall time and
    /// aiming, so the impact speed the server uses is not known here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_mps: Option<MetersPerSecond>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub krupp: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalization_deg: Option<Degrees>,

    /// Impact angle from the plate normal where ricochets start, no later than
    /// `ricochet_always_from_deg`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ricochet_from_deg: Option<Degrees>,

    /// Impact angle from the plate normal where every hit ricochets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ricochet_always_from_deg: Option<Degrees>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_time_s: Option<Seconds>,

    /// Plate thickness that arms the fuse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_threshold_mm: Option<Millimeters>,

    /// The aircraft's dive angle, negative when diving.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dive_angle_deg: Option<Degrees>,
}

/// Whether ordnance defeats a plate.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum PlateVerdict {
    Penetrates {
        penetration_mm: Millimeters,
    },
    Fails {
        penetration_mm: Millimeters,
    },

    /// AP ordnance whose caliber exceeds 14.3 x the thickness. On impact the
    /// game tests AP bombs and rockets with the AP shell mechanics, so the
    /// plate cannot ricochet it.
    Overmatches {
        caliber_mm: Millimeters,
    },

    /// Game data alone cannot decide; `reason` says why.
    Undetermined {
        reason: &'static str,
    },

    /// The weapon does not test plates.
    NotApplicable {
        reason: &'static str,
    },
}

impl GameData {
    /// Every squadron a ship can launch, carrier and hybrid squadrons first,
    /// then air strikes, optionally tested against a plate.
    pub fn ship_aircraft(&self, args: &ShipAircraftArgs) -> Result<ShipAircraftOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let thickness = match args.thickness_mm {
            Some(thickness) if thickness.value() > 0.0 => Some(thickness),
            Some(_) => return Err(QueryError::InvalidArguments { reason: "thickness_mm must be above 0" }),
            None => None,
        };

        let mut squadrons = self.squadrons(ship);
        let planner = Planner::new(self, ship);
        for squadron in &mut squadrons {
            squadron.fire_chance = self.ordnance_fire_chance(&planner, squadron)?;
        }

        if let Some(thickness) = thickness {
            for squadron in &mut squadrons {
                squadron.vs_plate = squadron.ordnance.as_ref().map(|ordnance| ordnance_vs_plate(ordnance, thickness));
            }
        }

        Ok(ShipAircraftOutput {
            interpretations,
            ship: ship.summary(),
            squadron_count: squadrons.len(),
            squadrons,
            thickness_mm: thickness,
            rule: thickness.map(|_| {
                "HE and SAP ordnance defeats the plate when its penetration is at least the thickness. Inertia Fuse for HE Shells does not apply to aircraft ordnance. AP bombs and rockets overmatch the plate when caliber > 14.3 x thickness; otherwise the server decides and ordnance.ap lists the inputs. Torpedoes, depth charges and mines do not test plates."
            }),
        })
    }

    pub(crate) fn squadrons(&self, ship: &Ship) -> Vec<Squadron> {
        let Some(components) = ship.param.vehicle().and_then(|vehicle| vehicle.ttx_components()) else {
            return Vec::new();
        };

        let mut squadrons = self.carrier_squadrons(ship);

        // Hulls usually share one air strike; list each distinct one once,
        // the stock hull's first.
        let stock_hull = components.stock_selection().hull.as_deref();
        let mut strikes: Vec<(&String, &wowsunpack::game_params::ttx::components::AirSupportStats)> =
            components.air_supports.iter().collect();
        strikes.sort_by(|(left, _), (right, _)| {
            (Some(right.as_str()) == stock_hull).cmp(&(Some(left.as_str()) == stock_hull)).then_with(|| left.cmp(right))
        });
        let mut seen: Vec<&Vec<String>> = Vec::new();
        for (_, strike) in strikes {
            if seen.contains(&&strike.planes) {
                continue;
            }
            seen.push(&strike.planes);

            for plane in &strike.planes {
                let Some(squadron) = self.squadron(plane, SquadronRole::AirStrike) else {
                    continue;
                };
                squadrons.push(Squadron {
                    air_strike: Some(AirStrike {
                        charges: strike.charges,
                        reload_s: strike.reload_time.map(|reload| Seconds::new(reload.value())),
                        range_km: strike.max_dist.map(|meters| Km::new(meters.value() / 1000.0)),
                    }),
                    ..squadron
                });
            }
        }

        squadrons
    }

    /// Carrier and hybrid squadrons of every plane module, stock modules first.
    pub(crate) fn carrier_squadrons(&self, ship: &Ship) -> Vec<Squadron> {
        let Some(components) = ship.param.vehicle().and_then(|vehicle| vehicle.ttx_components()) else {
            return Vec::new();
        };

        let mut carried: Vec<(&String, &wowsunpack::game_params::ttx::components::SquadronUpgrade)> =
            components.squadrons.iter().collect();
        carried.sort_by(|(left_name, left), (right_name, right)| {
            SquadronRole::from_slot(left.slot)
                .cmp(&SquadronRole::from_slot(right.slot))
                .then_with(|| right.stock.cmp(&left.stock))
                .then_with(|| left_name.cmp(right_name))
        });

        let mut squadrons = Vec::new();
        for (_, upgrade) in carried {
            for plane in &upgrade.planes {
                let Some(squadron) = self.squadron(plane, SquadronRole::from_slot(upgrade.slot)) else {
                    continue;
                };
                squadrons.push(Squadron { module_upgrade: !upgrade.stock, ..squadron });
            }
        }

        squadrons
    }

    pub(crate) fn squadron(&self, plane_name: &str, role: SquadronRole) -> Option<Squadron> {
        let param = self.provider().game_param_by_name(plane_name)?;
        let aircraft = param.aircraft()?;
        let ordnance = aircraft
            .ordnance()
            .and_then(|name| self.provider().game_param_by_name(name))
            .and_then(|projectile| ordnance(&projectile, self.projectile_name(projectile.name()), aircraft));

        Some(Squadron {
            role,
            alternative: aircraft.subtypes().iter().any(|subtype| matches!(subtype.as_str(), "jet" | "turboprop")),
            module_upgrade: false,
            aircraft: self.aircraft_name(&param),
            aircraft_param: plane_name.to_owned(),
            squadron_size: aircraft.squadron_size(),
            attack_flight_size: aircraft.attack_flight_size(),
            payload_per_plane: aircraft.payload_per_plane(),
            speed_with_ordnance_knots: aircraft.speed_with_ordnance(),
            ordnance,
            air_strike: None,
            fire_chance: None,
            vs_plate: None,
        })
    }

    /// Aircraft localize as units, `IDS_<NAME>`.
    pub(crate) fn aircraft_name(&self, param: &Param) -> String {
        translate_unit(param.name(), self.provider())
            .map(|name| clean(&name))
            .unwrap_or_else(|| param.name().to_owned())
    }

    /// Fire chance of one projectile of `squadron` on its best-fit build,
    /// read from the fire sources the build gives the ship. `None` when no
    /// module combination gives the squadron a fire chance.
    fn ordnance_fire_chance(
        &self,
        planner: &Planner<'_>,
        squadron: &Squadron,
    ) -> Result<Option<BuildStat<Percent>>, QueryError> {
        let selections: Vec<&ShipUpgradeSelection> = planner.selections().collect();
        let chance = |outcome: &Outcome| {
            FireInputs {
                ship: &planner.ship().param,
                selection: selections[outcome.selection],
                stats: &outcome.stats,
                modifiers: &outcome.bundle,
                provider: self.provider(),
            }
            .sources()
            .into_iter()
            .find(|source| {
                matches!(source.armament, FireArmament::Squadron | FireArmament::AirStrike)
                    && source.qualifier == squadron.aircraft_param
            })
            .map(|source| Percent::new(round_to(source.fire_chance.value(), 100.0)))
        };

        let mut carrying = None;
        for selection in 0..selections.len() {
            if chance(&*planner.bare(selection)?).is_some() {
                carrying = Some(selection);
                break;
            }
        }

        let Some(selection) = carrying else {
            return Ok(None);
        };

        let label = format!("{} fire chance per projectile", squadron.aircraft);
        planner.plan(&StatPlan::new(label, Better::Higher, &chance).on_selection(selection))
    }
}

/// Whether `ordnance` defeats a plate of `thickness`. HE and SAP compare
/// their fixed penetration; no skill raises it for aircraft ordnance
/// (`calculateHECSPenetration`, ModifiersApply.py:400-425, scales only main
/// and secondary battery shells). AP bombs and rockets overmatch by the shell
/// rule; whether they penetrate a thicker plate is undetermined, as the
/// client holds no formula for them and the server decides.
pub fn ordnance_vs_plate(ordnance: &Ordnance, thickness: Millimeters) -> PlateVerdict {
    if ordnance.kind.is_underwater() {
        return PlateVerdict::NotApplicable { reason: "strikes below the waterline and does not test plates" };
    }

    let penetration = match ordnance.ammo {
        Some(Ammo::He) => ordnance.he_penetration_mm,
        Some(Ammo::Sap) => ordnance.sap_penetration_mm,
        Some(Ammo::Ap) => {
            let caliber = ordnance.ap.as_ref().and_then(|ap| ap.caliber_mm);
            return match caliber {
                Some(caliber) if is_overmatch(caliber, thickness) => PlateVerdict::Overmatches { caliber_mm: caliber },
                _ => PlateVerdict::Undetermined {
                    reason: "the plate is not overmatched and the client has no AP bomb or rocket penetration formula; the server decides",
                },
            };
        }
        None => return PlateVerdict::Undetermined { reason: "the ordnance has no known shell type" },
    };
    let Some(penetration) = penetration else {
        return PlateVerdict::Undetermined { reason: "the ordnance lists no penetration" };
    };

    if penetration.value() >= thickness.value() {
        PlateVerdict::Penetrates { penetration_mm: penetration }
    } else {
        PlateVerdict::Fails { penetration_mm: penetration }
    }
}

fn ordnance(param: &Param, name: String, aircraft: &Aircraft) -> Option<Ordnance> {
    let projectile = param.projectile()?;
    let kind = OrdnanceKind::from_plane_ammo_type(projectile.plane_ammo_type()?);
    let ammo = match kind {
        OrdnanceKind::Bomb | OrdnanceKind::SkipBomb | OrdnanceKind::Rocket => shell_ammo(projectile),
        _ => None,
    };

    // FactorySquadrons.py:137-154 shows `floor(alphaPiercingHE)` for HE payloads.
    let he_penetration_mm = (ammo == Some(Ammo::He))
        .then(|| projectile.alpha_piercing_he())
        .flatten()
        .map(|penetration| Millimeters::new(penetration.floor()));
    let sap_penetration_mm = (ammo == Some(Ammo::Sap))
        .then(|| projectile.alpha_piercing_cs())
        .flatten()
        .map(|penetration| Millimeters::new(penetration.floor()));
    // getTorpedoDamage (ModifiersApply.py:504-517).
    let damage = if kind == OrdnanceKind::Torpedo {
        projectile.alpha_damage().zip(projectile.damage()).map(|(alpha, flood)| alpha / TORPEDO_DAMAGE_CONSTANT + flood)
    } else {
        projectile.alpha_damage()
    }
    .map(f32::round);

    Some(Ordnance {
        kind,
        ammo,
        name,
        param: param.name().to_owned(),
        damage: damage.map(Hp::from),
        // A projectile without `burnProb` takes the client's ammo default of
        // 0 (m65e88a3f.py); AP ordnance carries a negative one, which
        // `calculateBurnChance` floors at 0 (`max(initialBurnProb, 0)`).
        fire_chance_pct: Percent::new(round_to(projectile.burn_prob().unwrap_or(0.0).max(0.0) * 100.0, 100.0)),
        fall_time_coef: projectile.falling_time_coef(),
        he_penetration_mm,
        sap_penetration_mm,
        ap: (ammo == Some(Ammo::Ap)).then(|| ap_inputs(projectile, aircraft)),
    })
}

fn ap_inputs(projectile: &Projectile, aircraft: &Aircraft) -> ApOrdnance {
    let ricochet = ricochet_angles(Ammo::Ap, projectile);

    ApOrdnance {
        caliber_mm: projectile
            .bullet_diametr()
            .map(|meters| Millimeters::new(Meters::new(meters).to_mm().value().round())),
        mass_kg: projectile.bullet_mass().map(Kilograms::new),
        speed_mps: projectile.bullet_speed().map(MetersPerSecond::new),
        krupp: projectile.bullet_krupp(),
        normalization_deg: projectile.bullet_cap_normalize_max_angle().map(Degrees::new),
        ricochet_from_deg: ricochet.map(|angles| angles.from_deg),
        ricochet_always_from_deg: ricochet.map(|angles| angles.always_from_deg),
        fuse_time_s: projectile.bullet_detonator().map(Seconds::new),
        fuse_threshold_mm: projectile.bullet_detonator_threshold().map(Millimeters::new),
        dive_angle_deg: aircraft.angle_of_dive(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordnance(kind: OrdnanceKind, ammo: Option<Ammo>, he: Option<Millimeters>) -> Ordnance {
        Ordnance {
            kind,
            ammo,
            name: "Mk. 1".to_owned(),
            param: "PAPB110_Midway_stock".to_owned(),
            damage: Some(Hp::from(11200.0)),
            fire_chance_pct: Percent::new(0.0),
            fall_time_coef: None,
            he_penetration_mm: he,
            sap_penetration_mm: None,
            ap: None,
        }
    }

    #[test]
    fn he_ordnance_penetrates_plates_up_to_its_penetration() {
        let bomb = ordnance(OrdnanceKind::Bomb, Some(Ammo::He), Some(Millimeters::new(67.0)));

        assert_eq!(
            ordnance_vs_plate(&bomb, Millimeters::new(67.0)),
            PlateVerdict::Penetrates { penetration_mm: Millimeters::new(67.0) }
        );
        assert_eq!(
            ordnance_vs_plate(&bomb, Millimeters::new(68.0)),
            PlateVerdict::Fails { penetration_mm: Millimeters::new(67.0) }
        );
    }

    #[test]
    fn ap_ordnance_overmatches_thin_plates_and_is_otherwise_undetermined() {
        let mut bomb = ordnance(OrdnanceKind::SkipBomb, Some(Ammo::Ap), None);
        bomb.ap = Some(ApOrdnance {
            caliber_mm: Some(Millimeters::new(460.0)),
            mass_kg: None,
            speed_mps: None,
            krupp: None,
            normalization_deg: None,
            ricochet_from_deg: None,
            ricochet_always_from_deg: None,
            fuse_time_s: None,
            fuse_threshold_mm: None,
            dive_angle_deg: None,
        });

        assert_eq!(
            ordnance_vs_plate(&bomb, Millimeters::new(32.0)),
            PlateVerdict::Overmatches { caliber_mm: Millimeters::new(460.0) }
        );
        assert!(matches!(ordnance_vs_plate(&bomb, Millimeters::new(33.0)), PlateVerdict::Undetermined { .. }));
    }

    #[test]
    fn underwater_ordnance_does_not_test_plates() {
        let torpedo = ordnance(OrdnanceKind::Torpedo, None, None);
        assert!(matches!(ordnance_vs_plate(&torpedo, Millimeters::new(32.0)), PlateVerdict::NotApplicable { .. }));
    }
}
