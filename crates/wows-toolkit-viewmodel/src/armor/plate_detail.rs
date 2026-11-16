//! Each plate a shell struck and what the shell did there, as the trajectory editor and the
//! incoming-fire log list them.

use wowsunpack::ballistics::ImpactResult;
use wowsunpack::ballistics::PlateIndex;
use wowsunpack::ballistics::PlateOutcome;
use wowsunpack::ballistics::PlateResult;
use wowsunpack::ballistics::ShellSimResult;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Degrees;
use wowsunpack::game_params::types::Meters;
use wowsunpack::game_params::types::MetersPerSecond;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::Seconds;
use wowsunpack::game_params::types::ShellInfo;

use crate::armor::arc::ArcPlate;
use crate::armor::penetration::Ifhe;
use crate::armor::penetration::he_penetration;
use crate::armor::penetration::sap_penetration;

/// How squarely a plate was struck, in the three bands the viewers colour a strike by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngleBand {
    Good,
    Mid,
    Bad,
}

impl AngleBand {
    /// The band a strike `angle` from the plate's normal falls in. Each band starts at its own
    /// boundary, so the three meet without a gap.
    pub fn of(angle: Degrees) -> Self {
        const SQUARE: f32 = 30.0;
        const GLANCING: f32 = 45.0;

        let degrees = angle.value();
        if degrees < SQUARE {
            Self::Good
        } else if degrees < GLANCING {
            Self::Mid
        } else {
            Self::Bad
        }
    }
}

/// How a shell arrived, read off its flight solve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arrival {
    pub velocity: MetersPerSecond,
    /// Time of flight on the in-game clock.
    pub flight_time: Seconds,
    /// The fall angle, from horizontal.
    pub fall: Degrees,
}

impl Arrival {
    pub fn from_impact(impact: &ImpactResult) -> Self {
        Self {
            velocity: impact.impact_velocity,
            flight_time: impact.time_to_target,
            fall: impact.impact_angle_horizontal.to_degrees(),
        }
    }
}

/// What an armour-piercing shell did at one plate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlateVerdict {
    Overmatch { penetration: Millimeters, velocity: MetersPerSecond },
    Penetrated { penetration: Millimeters, effective_thickness: Millimeters, velocity: MetersPerSecond },
    Ricocheted,
    Shattered { penetration: Millimeters, effective_thickness: Millimeters },
}

impl PlateVerdict {
    fn from_result(result: &PlateResult) -> Self {
        let penetration = result.raw_penetration;
        let effective_thickness = result.effective_thickness;
        let velocity = result.velocity_before;
        match result.outcome {
            PlateOutcome::Overmatch => Self::Overmatch { penetration, velocity },
            PlateOutcome::Penetrate => Self::Penetrated { penetration, effective_thickness, velocity },
            PlateOutcome::Ricochet => Self::Ricocheted,
            PlateOutcome::Shatter => Self::Shattered { penetration, effective_thickness },
        }
    }
}

/// The simulation's result at one plate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimulatedPlate {
    pub verdict: PlateVerdict,
    /// Whether this plate armed the fuse.
    pub fuse_armed: bool,
}

/// Where the fuse went off, listed under the last plate the shell crossed before it did.
#[derive(Clone, Debug, PartialEq)]
pub struct Detonation {
    /// The volume it went off inside. `None` once the shell was clear of every zone.
    pub zone: Option<String>,
    /// How far the shell travelled after its fuse armed.
    pub travel: Meters,
    pub armed_at: PlateIndex,
}

/// One plate a shell struck, in the order it struck them.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateDetail {
    pub index: PlateIndex,
    pub thickness: Millimeters,
    /// The strike angle from the plate's normal.
    pub angle: Degrees,
    pub band: AngleBand,
    /// The armor zone the plate belongs to, as the model names it.
    pub zone: String,
    pub material: ArmorMaterial,
    /// `None` when nothing was simulated through the plates.
    pub simulated: Option<SimulatedPlate>,
    pub detonation: Option<Detonation>,
}

/// Every plate a shell struck, with how it arrived.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArcDetail {
    /// `None` without a shell whose flight solves.
    pub arrival: Option<Arrival>,
    pub plates: Vec<PlateDetail>,
}

impl ArcDetail {
    /// Lists `plates`, with what `sim` made of each.
    ///
    /// The plate chain models an armour-piercing shell, so for an HE or SAP shell `sim` is not
    /// read and [`flat_check`] answers for the first plate instead. Past the plate a simulated
    /// run ended at the shell no longer travels, so the plates beyond it are left out.
    pub fn new(
        ammo: Option<&AmmoType>,
        impact: Option<&ImpactResult>,
        sim: Option<&ShellSimResult>,
        plates: &[ArcPlate],
    ) -> Self {
        let sim = sim.filter(|_| !matches!(ammo, Some(AmmoType::HE | AmmoType::SAP)));

        let reached = match sim.and_then(ShellSimResult::last_reached_plate) {
            Some(last) => last.number().min(plates.len()),
            None => plates.len(),
        };

        let zones: Vec<&str> = plates.iter().map(|plate| plate.zone.as_str()).collect();
        let detonation = sim.and_then(|sim| {
            let at = sim.detonated_at?;
            let fuse = sim.detonation.as_ref()?;
            let zone = sim.detonation_zone(&zones).map(str::to_owned);
            Some((at, Detonation { zone, travel: fuse.travel_distance, armed_at: fuse.armed_at }))
        });

        let plates = plates[..reached]
            .iter()
            .enumerate()
            .map(|(position, plate)| {
                let index = PlateIndex::new(position);
                let simulated = sim.and_then(|sim| sim.plates.get(position)).map(|result| SimulatedPlate {
                    verdict: PlateVerdict::from_result(result),
                    fuse_armed: result.fuse_armed_here,
                });
                PlateDetail {
                    index,
                    thickness: plate.thickness,
                    angle: plate.angle,
                    band: AngleBand::of(plate.angle),
                    zone: plate.zone.clone(),
                    material: plate.material,
                    simulated,
                    detonation: detonation.as_ref().filter(|(at, _)| *at == index).map(|(_, fuse)| fuse.clone()),
                }
            })
            .collect();

        Self { arrival: impact.map(Arrival::from_impact), plates }
    }
}

/// A shell read against a plate's flat thickness, which is how HE and SAP are decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatAmmo {
    He,
    Sap,
}

impl FlatAmmo {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::He => "HE",
            Self::Sap => "SAP",
        }
    }
}

/// An HE or SAP shell held against the first plate it struck.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatCheck {
    pub ammo: FlatAmmo,
    pub penetration: Millimeters,
    pub thickness: Millimeters,
}

impl FlatCheck {
    pub fn penetrates(&self) -> bool {
        self.penetration >= self.thickness
    }
}

/// `shell` against a plate `thickness` thick. `None` for a shell that is neither HE nor SAP, or
/// that carries no penetration value for its kind.
pub fn flat_check(shell: &ShellInfo, thickness: Millimeters, ifhe: Ifhe) -> Option<FlatCheck> {
    let (ammo, penetration) = match shell.ammo_type {
        AmmoType::HE => (FlatAmmo::He, he_penetration(shell, ifhe)?),
        AmmoType::SAP => (FlatAmmo::Sap, sap_penetration(shell)?),
        AmmoType::AP | AmmoType::Unknown(_) => return None,
    };

    Some(FlatCheck { ammo, penetration, thickness })
}

#[cfg(test)]
mod tests {
    use wowsunpack::ballistics::FuseDetonation;
    use wowsunpack::ballistics::PlateHit;
    use wowsunpack::ballistics::ShellParams;
    use wowsunpack::ballistics::simulate_shell_through_plates;
    use wowsunpack::game_params::types::ShipModelDistance;

    use super::*;

    fn plate(zone: &str, thickness: f32, angle: f32) -> ArcPlate {
        ArcPlate {
            zone: zone.to_owned(),
            thickness: Millimeters::from(thickness),
            material: ArmorMaterial::parse("Belt").expect("Belt is a table key"),
            angle: Degrees::from(angle),
        }
    }

    fn result(outcome: PlateOutcome, fuse_armed_here: bool) -> PlateResult {
        PlateResult {
            outcome,
            effective_thickness: Millimeters::from(40.0),
            raw_penetration: Millimeters::from(500.0),
            velocity_before: MetersPerSecond::from(600.0),
            velocity_after: Some(MetersPerSecond::from(550.0)),
            fuse_armed_here,
        }
    }

    /// Iowa's 406 mm armour-piercing shell, as GameParams states it.
    fn iowa_ap() -> ShellInfo {
        ShellInfo {
            name: "PAPA001".to_owned(),
            ammo_type: AmmoType::AP,
            caliber: Millimeters::from(406.0),
            he_pen_mm: None,
            sap_pen_mm: None,
            alpha_damage: 10_500.0,
            muzzle_velocity: 762.0,
            mass_kg: 1225.0,
            krupp: 2574.0,
            ricochet_angle: Some(45.0),
            always_ricochet_angle: Some(60.0),
            fuse_time: Some(0.033),
            fuse_threshold: Some(68.0),
            burn_prob: None,
            air_drag: 0.321,
            normalization: Some(6.0),
            cap: true,
        }
    }

    #[test]
    fn the_bands_meet_at_their_own_boundaries() {
        assert_eq!(AngleBand::of(Degrees::from(10.0)), AngleBand::Good);
        assert_eq!(AngleBand::of(Degrees::from(30.0)), AngleBand::Mid);
        assert_eq!(AngleBand::of(Degrees::from(45.0)), AngleBand::Bad);
    }

    /// A plate too thin to stand against the calibre is overmatched; the belt behind it is
    /// penetrated and arms the fuse. Each plate reads what the simulation made of it.
    #[test]
    fn each_plate_reads_the_simulated_result_against_it() {
        let shell = iowa_ap();
        let params = ShellParams::from_shell_info(&shell).expect("the shell has every ballistic value");
        let plates = [plate("Bow", 25.0, 10.0), plate("Citadel", 300.0, 20.0)];
        let hits: Vec<PlateHit> = plates
            .iter()
            .enumerate()
            .map(|(position, plate)| PlateHit {
                thickness: plate.thickness,
                angle_from_normal: plate.angle,
                distance_along_ray: ShipModelDistance::from(position as f32 * 0.5),
            })
            .collect();
        let sim = simulate_shell_through_plates(&params, MetersPerSecond::from(600.0), &hits, false);

        let detail = ArcDetail::new(Some(&shell.ammo_type), None, Some(&sim), &plates);

        assert_eq!(detail.plates.len(), 2);
        let verdicts: Vec<PlateVerdict> =
            detail.plates.iter().map(|plate| plate.simulated.expect("both plates were simulated").verdict).collect();
        assert!(matches!(verdicts[0], PlateVerdict::Overmatch { .. }), "{verdicts:?}");
        let PlateVerdict::Penetrated { penetration, effective_thickness, .. } = verdicts[1] else {
            panic!("the belt is penetrated: {verdicts:?}");
        };
        assert!(penetration > effective_thickness && effective_thickness > Millimeters::from(300.0));
        let armed: Vec<bool> =
            detail.plates.iter().map(|plate| plate.simulated.is_some_and(|sim| sim.fuse_armed)).collect();
        assert_eq!(armed, [false, true], "only the belt is thick enough to arm the fuse");
        assert_eq!(detail.plates[1].band, AngleBand::Good);
    }

    /// The plates past the one a run ended at are not listed, and the detonation is listed under
    /// the plate it went off after, with the zone it went off in.
    #[test]
    fn a_run_ends_at_the_plate_it_detonated_after() {
        let plates = [plate("Hull", 25.0, 50.0), plate("Citadel", 300.0, 20.0), plate("Hull", 25.0, 50.0)];
        let sim = ShellSimResult {
            plates: vec![result(PlateOutcome::Penetrate, false), result(PlateOutcome::Penetrate, true)],
            detonation: Some(FuseDetonation {
                distance_along_ray: ShipModelDistance::from(1.0),
                armed_at: PlateIndex::new(1),
                travel_distance: Meters::from(4.5),
            }),
            stopped_at: None,
            detonated_at: Some(PlateIndex::new(1)),
        };

        let detail = ArcDetail::new(Some(&AmmoType::AP), None, Some(&sim), &plates);

        assert_eq!(detail.plates.len(), 2, "the plate past the detonation is not listed");
        assert_eq!(detail.plates[0].detonation, None);
        assert_eq!(detail.plates[0].band, AngleBand::Bad);
        let fuse = detail.plates[1].detonation.as_ref().expect("the detonation is under the second plate");
        assert_eq!(fuse.armed_at, PlateIndex::new(1));
        assert_eq!(fuse.travel, Meters::from(4.5));
        assert_eq!(fuse.zone.as_deref(), sim.detonation_zone(&["Hull", "Citadel", "Hull"]));
    }

    /// An HE shell is not run through the armour-piercing plate chain: every plate the ray crossed
    /// is listed bare, and the first is read against the shell's flat penetration.
    #[test]
    fn an_he_shell_lists_every_plate_and_is_read_flat() {
        let plates = [plate("Hull", 25.0, 50.0), plate("Citadel", 300.0, 20.0)];
        let sim = ShellSimResult {
            plates: vec![result(PlateOutcome::Ricochet, false)],
            detonation: None,
            stopped_at: Some(PlateIndex::new(0)),
            detonated_at: None,
        };

        let detail = ArcDetail::new(Some(&AmmoType::HE), None, Some(&sim), &plates);

        assert_eq!(detail.plates.len(), 2);
        assert!(detail.plates.iter().all(|plate| plate.simulated.is_none()));

        let mut he = iowa_ap();
        he.ammo_type = AmmoType::HE;
        he.he_pen_mm = Some(68.0);
        let plain = flat_check(&he, Millimeters::from(76.0), Ifhe::NotApplied).expect("an HE shell is read flat");
        assert!(!plain.penetrates());
        let with_ifhe = flat_check(&he, Millimeters::from(76.0), Ifhe::Applied).expect("an HE shell is read flat");
        assert!(with_ifhe.penetrates(), "IFHE lifts 68 mm past 76 mm");
        assert_eq!(flat_check(&iowa_ap(), Millimeters::from(76.0), Ifhe::Applied), None);
    }
}
