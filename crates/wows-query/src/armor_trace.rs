//! Grids of parallel rays cast through a target ship's armor model, each ray's
//! shell or bomb run through the toolkit's plate-by-plate simulation.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Degrees;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::MetersPerSecond;
use wows_core::units::Millimeters;
use wows_core::units::ShipModelDistance;
use wowsunpack::ballistics::PlateHit;
use wowsunpack::ballistics::PlateOutcome;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::ShellSimResult;
use wowsunpack::ballistics::is_citadel_zone;
use wowsunpack::ballistics::is_overmatch;
use wowsunpack::ballistics::simulate_shell_through_plates;
use wowsunpack::ballistics::solve_for_range;
use wowsunpack::export::armor_extract::MountModelArmor;
use wowsunpack::export::armor_ray::ArmorRayHit;
use wowsunpack::export::armor_ray::ParallelRays;
use wowsunpack::export::armor_ray::RayOffset;
use wowsunpack::export::armor_ray::angle_from_normal;
use wowsunpack::export::gltf_export::InteractiveArmorMesh;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Projectile;

use crate::aircraft::Ordnance;
use crate::aircraft::OrdnanceKind;
use crate::aircraft::Squadron;
use crate::aircraft::SquadronRole;
use crate::armor::PlateNames;
use crate::citadel;
use crate::citadel::CitadelApproach;
use crate::citadel::GLANCING_ASSUMPTION;
use crate::citadel::ShellFlags;
use crate::data::GameData;
use crate::error::ArmorReadError;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::names::search_key;
use crate::shells::Ammo;
use crate::shells::Battery;
use crate::shells::Shell;
use crate::ships::Ship;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TraceWeapon {
    #[default]
    MainBattery,
    SecondaryBattery,
    /// AP, HE or SAP bombs of the shooter's dive bombers or air strike.
    Bombs,
}

impl TraceWeapon {
    pub fn label(self) -> &'static str {
        match self {
            Self::MainBattery => "main battery",
            Self::SecondaryBattery => "secondary battery",
            Self::Bombs => "bombs",
        }
    }
}

/// A weapon of the shooter that armor_trace can trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmmoSource {
    MainBattery,
    SecondaryBattery,
    /// A squadron that drops bombs.
    Squadron { aircraft: String, role: SquadronRole },
}

impl AmmoSource {
    pub fn weapon(&self) -> TraceWeapon {
        match self {
            Self::MainBattery => TraceWeapon::MainBattery,
            Self::SecondaryBattery => TraceWeapon::SecondaryBattery,
            Self::Squadron { .. } => TraceWeapon::Bombs,
        }
    }
}

/// The ammo types one weapon fires or carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponAmmo {
    pub source: AmmoSource,
    pub ammo: Vec<Ammo>,
}

impl WeaponAmmo {
    /// "its main battery fires AP and HE", "its Suisei dive bombers carry HE bombs".
    pub(crate) fn describe(&self) -> String {
        let ammo = self.ammo.iter().map(|ammo| ammo.label()).collect::<Vec<_>>().join(" and ");

        match &self.source {
            AmmoSource::MainBattery => format!("its main battery fires {ammo}"),
            AmmoSource::SecondaryBattery => format!("its secondary battery fires {ammo}"),
            AmmoSource::Squadron { aircraft, role: SquadronRole::AirStrike } => {
                format!("its {aircraft} air strike carries {ammo} bombs")
            }
            AmmoSource::Squadron { aircraft, role } => format!("its {aircraft} {} carry {ammo} bombs", role.label()),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArmorTraceArgs {
    /// Firing ship, e.g. "Des Moines" or "Sekiryu".
    pub shooter: String,

    /// Target ship, e.g. "Yamato".
    pub target_ship: String,

    /// Defaults to main_battery.
    pub weapon: Option<TraceWeapon>,

    /// Shell or bomb type. Omit to trace each type the weapon fires or carries.
    pub ammo: Option<Ammo>,

    /// Gun range in km, 0 or more; 0 is point blank, flat at muzzle velocity. Required for guns, not used for bombs.
    pub range_km: Option<Km>,

    /// Target heading away from broadside in degrees: 0 is broadside, 30 angled, 90 bow-on with the bow toward the shooter. For bombs, 0 is a dive across the ship and 90 a dive along it from bow to stern. Defaults to 0.
    pub target_angle_deg: Option<Degrees>,

    /// Target hull module, e.g. "Hull B". Defaults to the stock hull.
    pub target_hull: Option<String>,

    /// Apply Inertia Fuse for HE Shells to gun HE penetration. Defaults to false.
    pub ifhe: Option<bool>,

    /// Distance between neighbouring rays in m, 0.25 to 10. Defaults to 1.
    pub spacing_m: Option<Meters>,

    /// Aircraft name when the shooter carries several bomb-carrying squadrons, e.g. "Ryusei". Without it, each bomb type comes from stock dive bombers before researched or jet dive bombers, and those before the air strike.
    pub aircraft: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArmorTraceOutput {
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub shooter: String,
    pub target: String,
    pub target_hull: String,
    pub target_angle_deg: Degrees,

    /// One trace per ammo type: the one asked for, or each the weapon fires or carries.
    pub traces: Vec<AmmoTrace>,

    /// Ammo types the weapon has that could not be traced, when ammo was not given.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<SkippedAmmo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedAmmo {
    pub ammo: Ammo,
    pub reason: String,
}

/// The rays of one ammo type through the target.
#[derive(Debug, Clone, Serialize)]
pub struct AmmoTrace {
    pub weapon: TracedWeapon,
    pub arrival: Arrival,
    pub rays: RayCounts,

    /// Rays whose path crosses a citadel plate, and how many of them reach it:
    /// AP that detonates inside it, HE or SAP that defeats citadel armor struck first.
    pub citadel: CitadelSummary,

    /// The armor in front of the citadel along this line of fire, and how
    /// steeply the citadel plate itself is struck.
    pub citadel_face: CitadelApproach,

    /// Per zone of the first plate struck, what became of the rays. Rays that
    /// entered the water first are not included.
    pub zones: Vec<ZoneSummary>,

    /// One ray per outcome, most common outcome first. AP examples are drawn
    /// only from rays over the citadel when any ray crosses it; HE and SAP
    /// examples from every modelled ray.
    pub examples: Vec<RayExample>,
    pub verdict: String,
    pub assumptions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TracedWeapon {
    /// "main battery", "secondary battery" or the aircraft name.
    pub source: String,

    /// GameParams name of the projectile.
    pub projectile: String,
    pub ammo: Ammo,
    pub caliber_mm: Millimeters,

    /// HE or SAP penetration used against the first plate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_mm: Option<Millimeters>,
}

/// How the projectile reaches the target.
#[derive(Debug, Clone, Serialize)]
pub struct Arrival {
    pub velocity_mps: MetersPerSecond,

    /// Below horizontal.
    pub fall_angle_deg: Degrees,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_km: Option<Km>,

    /// AP penetration against a plate struck head-on at this velocity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_penetration_mm: Option<Millimeters>,

    pub basis: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct RayCounts {
    pub spacing_m: Meters,
    pub cast: usize,

    /// Rays that struck armor, including those below the waterline.
    pub struck_ship: usize,

    /// Rays whose first armor contact is below the waterline: the projectile
    /// entered the water first, which is not modelled.
    pub entered_water_first: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CitadelSummary {
    /// Modelled rays whose path crosses a citadel plate.
    pub rays_over_citadel: usize,

    /// Of those, rays that detonate inside the citadel.
    pub detonating_inside: usize,

    /// `detonating_inside / rays_over_citadel`, by area seen along the line of
    /// fire; absent when no ray crosses the citadel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f32>,

    /// Citadel detonations that crossed a plate in the band where a ricochet
    /// is random in the game, which the simulation does not roll.
    pub through_random_ricochet_band: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZoneSummary {
    /// Zone of the first plate struck.
    pub zone: String,
    pub rays: usize,
    pub outcomes: BTreeMap<RayOutcome, usize>,

    /// AP: zones the fuse went off in, by count.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub detonated_in: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RayOutcome {
    /// AP detonated inside the citadel; HE or SAP defeated citadel armor struck first.
    Citadel,

    /// AP detonated inside the ship outside the citadel; HE or SAP defeated the
    /// first plate.
    Penetration,

    /// AP left the ship without detonating.
    Overpenetration,
    Ricochet,

    /// The plate stopped the projectile, or HE or SAP failed to defeat it.
    Shatter,

    /// AP spent its velocity inside the armor without detonating.
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
pub struct RayExample {
    pub outcome: RayOutcome,

    /// Where the ray first strikes armor.
    pub from_bow_m: Meters,
    pub above_waterline_m: Meters,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub detonated_in: Option<String>,

    /// Plates in strike order, up to the one the run ended at.
    pub plates: Vec<PlateStep>,

    /// Plates beyond those listed that the ray crosses.
    #[serde(skip_serializing_if = "is_zero")]
    pub more_plates: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlateStep {
    /// The game's name for the plate, led by its zone, e.g. "citadel: Armor Belt".
    pub plate: String,

    #[serde(skip)]
    pub material: ArmorMaterial,
    pub zone: String,
    pub thickness_mm: Millimeters,

    /// From the plate normal: 0 is head-on.
    pub angle_deg: Degrees,
    pub result: PlateResultKind,

    /// AP: thickness presented after normalization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_mm: Option<Millimeters>,

    /// AP: penetration on arrival at this plate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_mm: Option<Millimeters>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlateResultKind {
    Penetrate,
    Overmatch,
    Ricochet,
    Shatter,
    /// The fuse went off before this plate.
    NotReached,
}

/// What the trace fires and how it arrives.
pub(crate) struct Weapon {
    traced: TracedWeapon,
    pub(crate) arrival: Arrival,
    pub(crate) model: WeaponModel,

    /// Assumptions that hold for this weapon only.
    assumptions: Vec<String>,
}

pub(crate) enum WeaponModel {
    /// AP through every plate.
    ArmorPiercing { params: ShellParams, impact_velocity: MetersPerSecond },

    /// HE or SAP against the first plate.
    FirstPlate { penetration: Millimeters, ricochet: Option<FirstPlateRicochet> },
}

/// When SAP ricochets off its first plate: struck at or past `always_at`,
/// unless its caliber overmatches the plate.
pub(crate) struct FirstPlateRicochet {
    always_at: Degrees,
    caliber: Millimeters,
}

impl FirstPlateRicochet {
    fn overmatches(&self, plate: &RayPlate) -> bool {
        is_overmatch(self.caliber, plate.thickness)
    }

    fn ricochets(&self, plate: &RayPlate) -> bool {
        plate.angle.value() >= self.always_at.value() && !self.overmatches(plate)
    }
}

/// The gun a trace fires and from how far.
pub(crate) struct GunRequest {
    pub(crate) battery: Battery,
    pub(crate) ammo: Ammo,
    pub(crate) range_km: Option<Km>,
    pub(crate) ifhe: bool,
}

/// The hull upgrade a target's armor is read for.
pub(crate) struct TargetHull {
    /// GameParams key.
    pub(crate) param: String,
    pub(crate) name: String,
}

/// One plate along a ray, borrowing its names from the meshes.
pub(crate) struct RayPlate<'a> {
    pub(crate) zone: &'a str,
    pub(crate) material: ArmorMaterial,
    pub(crate) thickness: Millimeters,

    /// Strike angle from the plate normal.
    pub(crate) angle: Degrees,

    /// From the first plate along the ray.
    pub(crate) distance: ShipModelDistance,
    pub(crate) position: [f32; 3],
    pub(crate) normal: [f32; 3],
}

/// Parallel rays cast over a whole target.
pub(crate) struct RayGrid<'a> {
    pub(crate) cast: usize,
    pub(crate) rays: Vec<GridRay<'a>>,
}

/// A grid ray that struck armor.
pub(crate) struct GridRay<'a> {
    pub(crate) plates: Vec<RayPlate<'a>>,

    /// The first plate is below the waterline: a projectile from the air
    /// reaches it only through the water.
    pub(crate) underwater: bool,
}

/// What one modelled ray did.
pub(crate) struct TracedRay<'a> {
    pub(crate) plates: Vec<RayPlate<'a>>,
    pub(crate) outcome: RayOutcome,
    over_citadel: bool,
    random_ricochet_band: bool,
    detonated_in: Option<&'a str>,
    pub(crate) sim: Option<ShellSimResult>,
}

/// A plate [`ray_plates`] kept, to recognize a second crossing of it.
struct KeptPlate {
    mesh_index: usize,
    model_index: u32,
    material: ArmorMaterial,
    distance: ShipModelDistance,
}

/// The target side of a trace, shared by every ammo type traced.
struct TraceTarget<'a> {
    ship: &'a Ship,
    meshes: &'a [InteractiveArmorMesh],
    angle: Degrees,
    spacing: Meters,
    bow: ShipModelDistance,
    names: &'a PlateNames<'a>,
}

impl GameData {
    /// Casts a grid of parallel rays at `target` along the line of fire and
    /// runs `shooter`'s projectile through the armor each ray crosses.
    pub fn armor_trace(&self, args: &ArmorTraceArgs) -> Result<ArmorTraceOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let shooter = self.resolve_ship(&args.shooter, &mut interpretations)?;
        let target = self.resolve_ship(&args.target_ship, &mut interpretations)?;

        let spacing = args.spacing_m.unwrap_or(Meters::new(1.0));
        if !(0.25..=10.0).contains(&spacing.value()) {
            return Err(QueryError::InvalidArguments { reason: "spacing_m must be between 0.25 and 10" });
        }
        let target_angle = args.target_angle_deg.unwrap_or(Degrees::new(0.0));
        if !(-180.0..=180.0).contains(&target_angle.value()) {
            return Err(QueryError::InvalidArguments { reason: "target_angle_deg must be between -180 and 180" });
        }

        let weapon = args.weapon.unwrap_or_default();
        let mut weapons = Vec::new();
        let mut failures = Vec::new();
        for ammo in self.trace_ammo(shooter, weapon, args)? {
            let built = match weapon {
                TraceWeapon::MainBattery => self.gun_weapon(shooter, &gun_request(Battery::Main, ammo, args)),
                TraceWeapon::SecondaryBattery => self.gun_weapon(shooter, &gun_request(Battery::Secondary, ammo, args)),
                TraceWeapon::Bombs => self.bomb_weapon(shooter, ammo, args),
            };
            match built {
                Ok(built) => weapons.push(built),
                Err(error) => failures.push((ammo, error)),
            }
        }
        // Without traces, the failure of the first type is the answer.
        if weapons.is_empty() && !failures.is_empty() {
            return Err(failures.swap_remove(0).1);
        }
        let skipped =
            failures.into_iter().map(|(ammo, error)| SkippedAmmo { ammo, reason: error.to_string() }).collect();

        let TargetHull { param: hull_param, name: hull_name } =
            self.target_hull(target, args.target_hull.as_deref())?;
        let meshes = self.target_meshes(target, &hull_param)?;
        let materials = meshes.iter().flat_map(|mesh| mesh.triangle_info.iter().map(|info| info.material));
        let names = self.plate_names(target, materials);
        let trace_target = TraceTarget {
            ship: target,
            meshes: &meshes,
            angle: target_angle,
            spacing,
            bow: bow_position(&meshes),
            names: &names,
        };
        let traces =
            weapons.into_iter().map(|weapon| trace_weapon(weapon, &trace_target)).collect::<Result<Vec<_>, _>>()?;

        Ok(ArmorTraceOutput {
            interpretations,
            shooter: shooter.label(),
            target: target.label(),
            target_hull: hull_name,
            target_angle_deg: target_angle,
            traces,
            skipped,
        })
    }

    /// The ammo types to trace: `args.ammo` when `weapon` fires or carries
    /// it, else every type it does.
    fn trace_ammo(&self, shooter: &Ship, weapon: TraceWeapon, args: &ArmorTraceArgs) -> Result<Vec<Ammo>, QueryError> {
        let squadrons = self.squadrons(shooter);
        let available = self.weapon_ammo(shooter, &squadrons);
        let wanted = args.aircraft.as_deref().map(search_key);
        let fitting: BTreeSet<Ammo> = available
            .iter()
            .filter(|weapon_ammo| weapon_ammo.source.weapon() == weapon)
            .filter(|weapon_ammo| match (&weapon_ammo.source, wanted.as_deref()) {
                (AmmoSource::Squadron { aircraft, .. }, Some(wanted)) => search_key(aircraft).contains(wanted),
                _ => true,
            })
            .flat_map(|weapon_ammo| weapon_ammo.ammo.iter().copied())
            .collect();
        if fitting.is_empty() {
            let untyped = bomb_squadrons(&squadrons)
                .filter(|(squadron, _)| {
                    wanted.as_deref().is_none_or(|wanted| search_key(&squadron.aircraft).contains(wanted))
                })
                .find(|(_, ordnance)| ordnance.ammo.is_none());
            if let Some((squadron, _)) = untyped.filter(|_| weapon == TraceWeapon::Bombs) {
                return Err(QueryError::OrdnanceNotTraceable {
                    aircraft: squadron.aircraft.clone(),
                    reason: "the bomb has no known shell type",
                });
            }

            return Err(QueryError::WeaponNotFound { ship: shooter.name.clone(), weapon: weapon.label().to_owned() });
        }

        match args.ammo {
            None => Ok(fitting.into_iter().collect()),
            Some(ammo) if fitting.contains(&ammo) => Ok(vec![ammo]),
            Some(ammo) => Err(QueryError::AmmoNotCarried {
                ship: shooter.name.clone(),
                weapon,
                aircraft: args.aircraft.clone().filter(|_| weapon == TraceWeapon::Bombs),
                ammo,
                fitting: fitting.into_iter().collect(),
                available,
            }),
        }
    }

    /// Every weapon of `shooter` that armor_trace can trace, with its ammo.
    fn weapon_ammo(&self, shooter: &Ship, squadrons: &[Squadron]) -> Vec<WeaponAmmo> {
        let shells = self.shells(shooter);
        let mut weapons = Vec::new();

        for (battery, source) in [(Battery::Main, AmmoSource::MainBattery), (Battery::Secondary, AmmoSource::SecondaryBattery)]
        {
            let ammo: BTreeSet<Ammo> =
                shells.iter().filter(|shell| shell.battery == battery).map(|shell| shell.ammo).collect();
            if !ammo.is_empty() {
                weapons.push(WeaponAmmo { source, ammo: ammo.into_iter().collect() });
            }
        }

        for (squadron, ordnance) in bomb_squadrons(squadrons) {
            let Some(ammo) = ordnance.ammo else {
                continue;
            };

            weapons.push(WeaponAmmo {
                source: AmmoSource::Squadron { aircraft: squadron.aircraft.clone(), role: squadron.role },
                ammo: vec![ammo],
            });
        }

        weapons
    }

    pub(crate) fn gun_weapon(&self, shooter: &Ship, request: &GunRequest) -> Result<Weapon, QueryError> {
        let battery = request.battery;
        let shells: Vec<_> = self.shells(shooter).into_iter().filter(|shell| shell.battery == battery).collect();
        let source = battery.label();
        let ammo = request.ammo;

        let Some(shell) = largest_shell(&shells, ammo) else {
            return Err(QueryError::WeaponNotFound {
                ship: shooter.name.clone(),
                weapon: format!("{source} {}", ammo.label()),
            });
        };
        let projectile = self.projectile(&shell.param)?;

        let Some(range) = request.range_km else {
            return Err(QueryError::InvalidArguments { reason: "range_km is required for guns" });
        };
        if range.value().is_nan() || range.value() < 0.0 {
            return Err(QueryError::InvalidArguments { reason: "range_km must be 0 or more" });
        }

        let ifhe = request.ifhe;
        let mut assumptions = vec![format!(
            "Impact velocity and fall angle come from the toolkit's trajectory solution at {range} (client drag model, no target movement)."
        )];

        let Some(params) = ShellParams::from_shell_info(&projectile.to_shell_info(shell.param.clone())) else {
            return Err(QueryError::ProjectileIncomplete { projectile: shell.param.clone() });
        };
        let Some(impact) = solve_for_range(&params, range.to_meters()) else {
            return Err(QueryError::OutOfRange { projectile: shell.param.clone(), range });
        };

        let (model, penetration) = match ammo {
            Ammo::Ap => (WeaponModel::ArmorPiercing { params, impact_velocity: impact.impact_velocity }, None),
            Ammo::He | Ammo::Sap => {
                // Inertia Fuse for HE Shells changes HE only.
                let penetration = match (ammo, ifhe) {
                    (Ammo::He, true) => shell.ifhe_penetration_mm,
                    _ => shell.penetration_mm,
                };
                let Some(penetration) = penetration else {
                    return Err(QueryError::WeaponNotFound { ship: shooter.name.clone(), weapon: shell.param.clone() });
                };
                if ifhe && ammo == Ammo::He {
                    assumptions.push("Inertia Fuse for HE Shells is applied to the HE penetration.".to_owned());
                }
                let ricochet = first_plate_ricochet(ammo, &projectile, shell.caliber_mm);
                (WeaponModel::FirstPlate { penetration, ricochet }, Some(penetration))
            }
        };

        let raw_penetration =
            matches!(model, WeaponModel::ArmorPiercing { .. }).then_some(round_mm(impact.raw_penetration));

        Ok(Weapon {
            traced: TracedWeapon {
                source: source.to_owned(),
                projectile: shell.param.clone(),
                ammo,
                caliber_mm: shell.caliber_mm,
                penetration_mm: penetration,
            },
            arrival: Arrival {
                velocity_mps: MetersPerSecond::new(round1(impact.impact_velocity.value())),
                fall_angle_deg: Degrees::new(round1(impact.impact_angle_horizontal.to_degrees().value())),
                range_km: Some(range),
                raw_penetration_mm: raw_penetration,
                basis: "ballistic solution at the given range",
            },
            model,
            assumptions,
        })
    }

    fn bomb_weapon(&self, shooter: &Ship, ammo: Ammo, args: &ArmorTraceArgs) -> Result<Weapon, QueryError> {
        let wanted = args.aircraft.as_deref().map(search_key);
        let squadrons = self.squadrons(shooter);
        // The squadron a stock ship flies is the expected one.
        let chosen = bomb_squadrons(&squadrons)
            .filter(|(_, ordnance)| ordnance.ammo == Some(ammo))
            .filter(|(squadron, _)| {
                wanted.as_deref().is_none_or(|wanted| search_key(&squadron.aircraft).contains(wanted))
            })
            .min_by_key(|(squadron, _)| {
                (squadron.role == SquadronRole::AirStrike, squadron.module_upgrade, squadron.alternative)
            });
        let Some((squadron, ordnance)) = chosen else {
            return Err(QueryError::WeaponNotFound {
                ship: shooter.name.clone(),
                weapon: format!("{} bombs", ammo.label()),
            });
        };

        let not_traceable =
            |reason: &'static str| QueryError::OrdnanceNotTraceable { aircraft: squadron.aircraft.clone(), reason };
        let projectile = self.projectile(&ordnance.projectile)?;
        let dive = self
            .provider()
            .game_param_by_name(&squadron.aircraft_param)
            .and_then(|param| param.aircraft().and_then(|aircraft| aircraft.angle_of_dive()))
            .map(Degrees::abs)
            .filter(|dive| dive.value() > 0.0)
            .ok_or_else(|| not_traceable("the aircraft has no dive angle"))?;
        let speed = projectile
            .bullet_speed()
            .map(MetersPerSecond::new)
            .ok_or_else(|| not_traceable("the bomb has no bulletSpeed"))?;
        let caliber = projectile
            .bullet_diametr()
            .map(|meters| round_mm(Meters::new(meters).to_mm()))
            .ok_or_else(|| not_traceable("the bomb has no caliber"))?;

        let mut assumptions = vec![format!(
            "Bomb impact speed is assumed to be its bulletSpeed ({} m/s) and its fall angle the aircraft's angleOfDive ({} deg). The client does not define bomb impact kinematics; the server does.",
            speed.value(),
            dive.value()
        )];
        if args.ifhe == Some(true) {
            assumptions.push("Inertia Fuse for HE Shells does not apply to aircraft ordnance.".to_owned());
        }

        let (model, penetration, raw_penetration) = match ammo {
            Ammo::Ap => {
                let params = ShellParams::from_shell_info(&projectile.to_shell_info(ordnance.projectile.clone()))
                    .ok_or_else(|| not_traceable("the bomb lacks a normalization angle or fuse threshold"))?;
                let raw = round_mm(params.raw_penetration(speed));
                assumptions.push(
                    "AP bombs are simulated with the gun shell formulas; the game may treat bombs differently on the server."
                        .to_owned(),
                );
                (WeaponModel::ArmorPiercing { params, impact_velocity: speed }, None, Some(raw))
            }
            Ammo::He | Ammo::Sap => {
                let penetration = match ammo {
                    Ammo::Sap => ordnance.sap_penetration_mm,
                    _ => ordnance.he_penetration_mm,
                };
                let penetration = penetration.ok_or_else(|| not_traceable("the bomb lists no penetration"))?;
                let ricochet = first_plate_ricochet(ammo, &projectile, caliber);
                (WeaponModel::FirstPlate { penetration, ricochet }, Some(penetration), None)
            }
        };

        Ok(Weapon {
            traced: TracedWeapon {
                source: squadron.aircraft.clone(),
                projectile: ordnance.projectile.clone(),
                ammo,
                caliber_mm: caliber,
                penetration_mm: penetration,
            },
            arrival: Arrival {
                velocity_mps: speed,
                fall_angle_deg: dive,
                range_km: None,
                raw_penetration_mm: raw_penetration,
                basis: "assumed: bulletSpeed and the aircraft's angleOfDive",
            },
            model,
            assumptions,
        })
    }

    pub(crate) fn projectile(&self, name: &str) -> Result<Projectile, QueryError> {
        self.provider()
            .game_param_by_name(name)
            .and_then(|param| param.projectile().cloned())
            .ok_or_else(|| QueryError::ProjectileNotFound { projectile: name.to_owned() })
    }

    /// The hull upgrade the target's armor is read for.
    pub(crate) fn target_hull(&self, target: &Ship, query: Option<&str>) -> Result<TargetHull, QueryError> {
        let queries: Vec<String> = query.map(str::to_owned).into_iter().collect();
        let selection = self.resolve_modules(target, &queries)?;
        let Some(param) = selection.selection.hull else {
            return Err(QueryError::ArmorMissing { ship: target.name.clone() });
        };
        let name = self
            .ship_modules(target)
            .into_iter()
            .find(|module| module.param == param)
            .map_or_else(|| param.clone(), |module| module.name);

        Ok(TargetHull { param, name })
    }

    pub(crate) fn target_meshes(
        &self,
        target: &Ship,
        hull_param: &str,
    ) -> Result<Vec<InteractiveArmorMesh>, QueryError> {
        let Some(armor) = self.vehicle_armor(target)? else {
            return Err(QueryError::ArmorMissing { ship: target.name.clone() });
        };
        let Some(hull) = armor.hull_upgrade(Some(hull_param)) else {
            return Err(QueryError::ArmorMissing { ship: target.name.clone() });
        };
        let Some(vehicle) = target.param.vehicle() else {
            return Err(QueryError::ArmorMissing { ship: target.name.clone() });
        };

        let mut mount_models: Vec<MountModelArmor> = Vec::new();
        for mount in &hull.mounts {
            if mount_models.iter().any(|model| model.model_path == mount.model_path) {
                continue;
            }
            if let Some(model) = self.mount_model_armor(target, &mount.model_path)? {
                mount_models.push(model);
            }
        }

        armor
            .interactive_meshes(hull, vehicle, |path| mount_models.iter().find(|model| model.model_path == path))
            .map_err(|source| QueryError::ArmorUnreadable {
                ship: target.name.clone(),
                source: ArmorReadError::Format(source),
            })
    }
}

/// Unit direction of travel in armor mesh space (bow toward -Z, +Y up). The
/// shooter sits on the +X side; a positive angle swings the bow toward it.
pub(crate) fn line_of_fire(target_angle: Degrees, fall: Degrees) -> [f32; 3] {
    let heading = target_angle.to_radians();
    let fall = fall.to_radians();

    [-heading.cos() * fall.cos(), -fall.sin(), heading.sin() * fall.cos()]
}

/// Casts parallel rays `spacing` apart over every part of `meshes` seen along
/// `direction`, keeping the rays that strike armor.
pub(crate) fn cast_grid(
    meshes: &[InteractiveArmorMesh],
    direction: [f32; 3],
    spacing: Meters,
) -> Result<RayGrid<'_>, QueryError> {
    // More rays than this is a spacing too fine for the target.
    const RAY_LIMIT: usize = 250_000;

    let rays = ParallelRays::new(meshes, direction);
    let footprint = rays.footprint();
    let step = spacing.to_ship_model().value();
    let columns = ((footprint.max.across.value() - footprint.min.across.value()) / step).ceil().max(1.0) as usize;
    let rows = ((footprint.max.up.value() - footprint.min.up.value()) / step).ceil().max(1.0) as usize;
    if columns.saturating_mul(rows) > RAY_LIMIT {
        return Err(QueryError::InvalidArguments { reason: "spacing_m is too fine for this target; use a larger one" });
    }

    let mut grid = RayGrid { cast: columns * rows, rays: Vec::new() };
    for row in 0..rows {
        for column in 0..columns {
            let offset = RayOffset {
                across: ShipModelDistance::new(footprint.min.across.value() + (column as f32 + 0.5) * step),
                up: ShipModelDistance::new(footprint.min.up.value() + (row as f32 + 0.5) * step),
            };
            let hits = rays.cast(offset);
            let plates = ray_plates(meshes, &hits, rays.direction());
            let Some(first) = plates.first() else {
                continue;
            };

            let underwater = first.position[1] < 0.0;
            grid.rays.push(GridRay { plates, underwater });
        }
    }

    Ok(grid)
}

/// The largest-caliber shell of `ammo` among `shells`. Gun modules of one
/// ship can fire different shells of one type; the largest caliber is the one
/// a trace is usually asked about.
pub(crate) fn largest_shell(shells: &[Shell], ammo: Ammo) -> Option<&Shell> {
    shells
        .iter()
        .filter(|shell| shell.ammo == ammo)
        .max_by(|left, right| left.caliber_mm.value().total_cmp(&right.caliber_mm.value()))
}

fn gun_request(battery: Battery, ammo: Ammo, args: &ArmorTraceArgs) -> GunRequest {
    GunRequest { battery, ammo, range_km: args.range_km, ifhe: args.ifhe.unwrap_or(false) }
}

/// Squadrons that drop bombs, with their bombs.
fn bomb_squadrons(squadrons: &[Squadron]) -> impl Iterator<Item = (&Squadron, &Ordnance)> {
    squadrons
        .iter()
        .filter_map(|squadron| squadron.ordnance.as_ref().map(|ordnance| (squadron, ordnance)))
        .filter(|(_, ordnance)| ordnance.kind == OrdnanceKind::Bomb)
}

/// Casts the grid along `weapon`'s line of fire and runs its projectile
/// through each ray.
fn trace_weapon(weapon: Weapon, target: &TraceTarget) -> Result<AmmoTrace, QueryError> {
    let direction = line_of_fire(target.angle, weapon.arrival.fall_angle_deg);
    let grid = cast_grid(target.meshes, direction, target.spacing)?;
    let shell = match &weapon.model {
        WeaponModel::ArmorPiercing { params, .. } => Some(ShellFlags { params }),
        WeaponModel::FirstPlate { .. } => None,
    };
    let citadel_face = citadel::approach(
        &grid.rays,
        weapon.arrival.fall_angle_deg,
        weapon.arrival.range_km,
        shell.as_ref(),
        target.bow,
        target.names,
    );

    let struck = grid.rays.len();
    let underwater = grid.rays.iter().filter(|ray| ray.underwater).count();
    let traced: Vec<TracedRay> =
        grid.rays.into_iter().filter(|ray| !ray.underwater).map(|ray| trace_ray(&weapon.model, ray.plates)).collect();

    let citadel = citadel_summary(&traced);
    let zones = zone_summaries(&traced);
    let examples = examples(&traced, &weapon.model, target.bow, target.names);
    let verdict = verdict(&weapon, &citadel, &zones, target.ship);

    let mut assumptions = weapon.assumptions;
    assumptions.extend(common_assumptions(&weapon.model).into_iter().map(str::to_owned));

    Ok(AmmoTrace {
        weapon: weapon.traced,
        arrival: weapon.arrival,
        rays: RayCounts { spacing_m: target.spacing, cast: grid.cast, struck_ship: struck, entered_water_first: underwater },
        citadel,
        citadel_face,
        zones,
        examples,
        verdict,
        assumptions,
    })
}

/// The plates a ray crosses, in strike order.
///
/// A ray through a shared edge, or through a plate modelled by two coincident
/// faces, crosses the same plate twice at one point; the second crossing is
/// dropped so its thickness is not counted twice. Coincident triangles of different armor models are separate layers and
/// are all kept.
fn ray_plates<'a>(meshes: &'a [InteractiveArmorMesh], hits: &[ArmorRayHit], direction: [f32; 3]) -> Vec<RayPlate<'a>> {
    const SAME_POINT: ShipModelDistance = ShipModelDistance::new(1e-4);

    let first_distance = hits.first().map_or(ShipModelDistance::new(0.0), |hit| hit.distance);
    let mut plates: Vec<RayPlate<'a>> = Vec::with_capacity(hits.len());
    let mut kept: Vec<KeptPlate> = Vec::with_capacity(hits.len());
    for hit in hits {
        let Some(info) = meshes.get(hit.mesh_index).and_then(|mesh| mesh.triangle_info.get(hit.triangle_index)) else {
            continue;
        };
        let distance = hit.distance - first_distance;
        let crossed_already =
            kept.iter().rev().take_while(|plate| distance - plate.distance < SAME_POINT).any(|plate| {
                plate.mesh_index == hit.mesh_index
                    && plate.model_index == info.model_index
                    && plate.material == info.material
            });
        if crossed_already {
            continue;
        }
        kept.push(KeptPlate {
            mesh_index: hit.mesh_index,
            model_index: info.model_index,
            material: info.material,
            distance,
        });

        plates.push(RayPlate {
            zone: &info.zone,
            material: info.material,
            thickness: Millimeters::new(info.thickness_mm),
            angle: angle_from_normal(direction, hit.normal),
            distance,
            position: hit.position,
            normal: hit.normal,
        });
    }

    plates
}

pub(crate) fn trace_ray<'a>(model: &WeaponModel, plates: Vec<RayPlate<'a>>) -> TracedRay<'a> {
    let over_citadel = plates.iter().any(|plate| is_citadel_zone(plate.zone));

    match model {
        WeaponModel::ArmorPiercing { params, impact_velocity } => {
            let sim = simulate_shell_through_plates(params, *impact_velocity, &plate_hits(&plates), false);
            let zones: Vec<&'a str> = plates.iter().map(|plate| plate.zone).collect();
            let (outcome, detonated_in) = ap_outcome(&sim, &zones);
            let reached = sim.plates.len();
            let random_ricochet_band = plates[..reached].iter().any(|plate| {
                let angle = plate.angle.to_radians();
                !is_overmatch(params.caliber, plate.thickness)
                    && angle >= params.ricochet_angle
                    && angle < params.always_ricochet_angle
            });

            TracedRay { plates, outcome, over_citadel, random_ricochet_band, detonated_in, sim: Some(sim) }
        }
        WeaponModel::FirstPlate { penetration, ricochet } => {
            let first = &plates[0];
            let outcome = if ricochet.as_ref().is_some_and(|ricochet| ricochet.ricochets(first)) {
                RayOutcome::Ricochet
            } else if penetration.value() >= first.thickness.value() && is_citadel_zone(first.zone) {
                RayOutcome::Citadel
            } else if penetration.value() >= first.thickness.value() {
                RayOutcome::Penetration
            } else {
                RayOutcome::Shatter
            };

            TracedRay { plates, outcome, over_citadel, random_ricochet_band: false, detonated_in: None, sim: None }
        }
    }
}

/// `plates` as the shell simulation takes them.
pub(crate) fn plate_hits(plates: &[RayPlate]) -> Vec<PlateHit> {
    plates
        .iter()
        .map(|plate| PlateHit {
            thickness: plate.thickness,
            angle_from_normal: plate.angle,
            distance_along_ray: plate.distance,
        })
        .collect()
}

/// The armor viewer's reading of a simulated run
/// (`wows_toolkit_viewmodel::armor::arc::sim_outcome`), with the zone the
/// fuse went off in.
pub(crate) fn ap_outcome<'a>(sim: &ShellSimResult, zones: &[&'a str]) -> (RayOutcome, Option<&'a str>) {
    if sim.detonation.is_some() {
        if sim.detonated_at.is_none() {
            return (RayOutcome::Overpenetration, None);
        }
        let zone = sim.detonation_zone(zones);
        let outcome = if zone.is_some_and(is_citadel_zone) { RayOutcome::Citadel } else { RayOutcome::Penetration };
        return (outcome, zone);
    }

    let Some(stopped_at) = sim.stopped_at else {
        return (RayOutcome::Overpenetration, None);
    };
    let outcome = match sim.plates.get(stopped_at.value()).map(|plate| plate.outcome) {
        Some(PlateOutcome::Ricochet) => RayOutcome::Ricochet,
        Some(PlateOutcome::Shatter) => RayOutcome::Shatter,
        _ => RayOutcome::Stopped,
    };

    (outcome, None)
}

/// How SAP ricochets off its first plate; HE does not ricochet.
fn first_plate_ricochet(ammo: Ammo, projectile: &Projectile, caliber: Millimeters) -> Option<FirstPlateRicochet> {
    match ammo {
        Ammo::Sap => projectile
            .bullet_always_ricochet_at()
            .map(|always_at| FirstPlateRicochet { always_at: Degrees::new(always_at), caliber }),
        Ammo::He | Ammo::Ap => None,
    }
}

fn citadel_summary(rays: &[TracedRay]) -> CitadelSummary {
    let over: Vec<&TracedRay> = rays.iter().filter(|ray| ray.over_citadel).collect();
    let inside: Vec<&&TracedRay> = over.iter().filter(|ray| ray.outcome == RayOutcome::Citadel).collect();
    let fraction = (!over.is_empty()).then(|| (inside.len() as f32 / over.len() as f32 * 1000.0).round() / 1000.0);

    CitadelSummary {
        rays_over_citadel: over.len(),
        detonating_inside: inside.len(),
        fraction,
        through_random_ricochet_band: inside.iter().filter(|ray| ray.random_ricochet_band).count(),
    }
}

fn zone_summaries(rays: &[TracedRay]) -> Vec<ZoneSummary> {
    let mut zones: BTreeMap<&str, ZoneSummary> = BTreeMap::new();
    for ray in rays {
        let zone = ray.plates[0].zone;
        let summary = zones.entry(zone).or_insert_with(|| ZoneSummary {
            zone: zone.to_owned(),
            rays: 0,
            outcomes: BTreeMap::new(),
            detonated_in: BTreeMap::new(),
        });
        summary.rays += 1;
        *summary.outcomes.entry(ray.outcome).or_default() += 1;
        if let Some(detonated_in) = ray.detonated_in {
            *summary.detonated_in.entry(detonated_in.to_owned()).or_default() += 1;
        }
    }

    let mut zones: Vec<ZoneSummary> = zones.into_values().collect();
    zones.sort_by(|left, right| right.rays.cmp(&left.rays).then_with(|| left.zone.cmp(&right.zone)));

    zones
}

/// One ray per outcome, most common outcome first, taken from the middle of
/// that outcome's rays. Rays over the citadel are preferred when there are any.
fn examples(rays: &[TracedRay], model: &WeaponModel, bow: ShipModelDistance, names: &PlateNames) -> Vec<RayExample> {
    const EXAMPLE_LIMIT: usize = 4;

    let over_citadel: Vec<&TracedRay> = rays.iter().filter(|ray| ray.over_citadel).collect();
    let pool: Vec<&TracedRay> = if over_citadel.is_empty() || matches!(model, WeaponModel::FirstPlate { .. }) {
        rays.iter().collect()
    } else {
        over_citadel
    };

    let mut by_outcome: BTreeMap<RayOutcome, Vec<&TracedRay>> = BTreeMap::new();
    for ray in pool {
        by_outcome.entry(ray.outcome).or_default().push(ray);
    }
    let mut groups: Vec<(RayOutcome, Vec<&TracedRay>)> = by_outcome.into_iter().collect();
    groups.sort_by(|left, right| right.1.len().cmp(&left.1.len()).then_with(|| left.0.cmp(&right.0)));

    groups.into_iter().take(EXAMPLE_LIMIT).map(|(_, rays)| example(rays[rays.len() / 2], model, bow, names)).collect()
}

fn example(ray: &TracedRay, model: &WeaponModel, bow: ShipModelDistance, names: &PlateNames) -> RayExample {
    const EXAMPLE_PLATE_LIMIT: usize = 10;

    // A fuse that fired between plates leaves the next one unreached, which is
    // what shows where it fired. A shell that stopped or left the ship ends at
    // its last plate. HE and SAP are tested on the first plate only.
    let listed = match &ray.sim {
        Some(sim) if sim.detonated_at.is_some() && sim.stopped_at.is_none() => sim.plates.len() + 1,
        Some(sim) => sim.plates.len(),
        None => 1,
    };
    let listed = listed.min(ray.plates.len()).min(EXAMPLE_PLATE_LIMIT);

    let plates = ray.plates[..listed]
        .iter()
        .enumerate()
        .map(|(index, plate)| {
            let simulated = ray.sim.as_ref().and_then(|sim| sim.plates.get(index));
            let result = match (simulated, &ray.sim) {
                (Some(result), _) => match result.outcome {
                    PlateOutcome::Overmatch => PlateResultKind::Overmatch,
                    PlateOutcome::Penetrate => PlateResultKind::Penetrate,
                    PlateOutcome::Ricochet => PlateResultKind::Ricochet,
                    PlateOutcome::Shatter => PlateResultKind::Shatter,
                },
                (None, Some(_)) => PlateResultKind::NotReached,
                (None, None) => match (ray.outcome, model) {
                    (RayOutcome::Ricochet, _) => PlateResultKind::Ricochet,
                    (RayOutcome::Shatter, _) => PlateResultKind::Shatter,
                    (_, WeaponModel::FirstPlate { ricochet: Some(ricochet), .. }) if ricochet.overmatches(plate) => {
                        PlateResultKind::Overmatch
                    }
                    _ => PlateResultKind::Penetrate,
                },
            };

            PlateStep {
                plate: names.of(plate.material),
                material: plate.material,
                zone: plate.zone.to_owned(),
                thickness_mm: round_mm(plate.thickness),
                angle_deg: Degrees::new(round1(plate.angle.value())),
                result,
                effective_mm: simulated.map(|result| round_mm(result.effective_thickness)),
                penetration_mm: simulated.map(|result| round_mm(result.raw_penetration)),
            }
        })
        .collect();

    let [_, up, forward] = ray.plates[0].position;
    RayExample {
        outcome: ray.outcome,
        from_bow_m: round_m((ShipModelDistance::new(forward) - bow).to_meters()),
        above_waterline_m: round_m(ShipModelDistance::new(up).to_meters()),
        detonated_in: ray.detonated_in.map(str::to_owned),
        plates,
        more_plates: ray.plates.len() - listed,
    }
}

/// The bow's position along the mesh Z axis: the hull's smallest Z.
pub(crate) fn bow_position(meshes: &[InteractiveArmorMesh]) -> ShipModelDistance {
    let bow = meshes
        .iter()
        .filter(|mesh| mesh.transform.is_none())
        .flat_map(|mesh| mesh.positions.iter())
        .map(|position| position[2])
        .fold(f32::INFINITY, f32::min);

    ShipModelDistance::new(bow)
}

fn verdict(weapon: &Weapon, citadel: &CitadelSummary, zones: &[ZoneSummary], target: &Ship) -> String {
    let projectile = format!("{} {}", weapon.traced.source, weapon.traced.ammo.label());

    if let WeaponModel::FirstPlate { .. } = weapon.model {
        let total: usize = zones.iter().map(|zone| zone.rays).sum();
        let defeated = |outcome: RayOutcome| -> usize {
            zones.iter().map(|zone| zone.outcomes.get(&outcome).copied().unwrap_or(0)).sum()
        };
        let penetrating = defeated(RayOutcome::Penetration) + defeated(RayOutcome::Citadel);
        let damaged: Vec<&str> = zones
            .iter()
            .filter(|zone| zone.outcomes.contains_key(&RayOutcome::Penetration))
            .map(|zone| zone.zone.as_str())
            .collect();
        let citadel_text = match defeated(RayOutcome::Citadel) {
            0 => format!(
                "It reaches {}'s citadel only where citadel armor is the first plate struck, and defeats none.",
                target.name
            ),
            hits => format!(
                "{hits} rays ({}) strike {}'s citadel armor first and defeat it.",
                percent(hits, total),
                target.name
            ),
        };
        let verdict = format!(
            "{projectile} damages only behind the first plate it defeats. It defeats the first plate on {penetrating} of {total} rays ({}); zones it damages: {}. {citadel_text}",
            percent(penetrating, total),
            if damaged.is_empty() { "none".to_owned() } else { damaged.join(", ") },
        );
        return capitalize_first(&verdict);
    }

    if citadel.rays_over_citadel == 0 {
        return format!(
            "No modelled ray of {projectile} crosses a citadel plate of {} from this direction.",
            target.name
        );
    }

    let percentage = percent(citadel.detonating_inside, citadel.rays_over_citadel);
    if citadel.detonating_inside == 0 {
        return format!(
            "No: none of the {} rays over {}'s citadel detonates inside it.",
            citadel.rays_over_citadel, target.name
        );
    }
    let qualifier = match citadel.fraction {
        Some(fraction) if fraction >= 0.25 => "Yes",
        Some(fraction) if fraction >= 0.05 => "Sometimes",
        _ => "Rarely",
    };
    let random = match citadel.through_random_ricochet_band {
        0 => String::new(),
        rays => format!(" {rays} of them cross a plate at an angle where the game rolls a ricochet."),
    };

    format!(
        "{qualifier}: {} of {} rays over {}'s citadel ({percentage}) detonate inside it.{random}",
        citadel.detonating_inside, citadel.rays_over_citadel, target.name
    )
}

fn common_assumptions(model: &WeaponModel) -> Vec<&'static str> {
    let mut assumptions = vec![
        "Each ray stands for an equal area of the target seen along the line of fire; fractions are by area, not weighted by dispersion or aim.",
        "Rays whose first armor contact is below the waterline entered the water first and are not modelled.",
        "Target hull level and stationary, turrets at their default heading, and the projectile travels straight through the ship.",
        "Penetration, ricochet and fuse behaviour are resolved by the game server; the client data holds only the inputs.",
        GLANCING_ASSUMPTION,
    ];
    match model {
        WeaponModel::ArmorPiercing { .. } => assumptions.extend([
            "AP uses the toolkit armor viewer's plate-by-plate simulation with community formulas from wows_shell (jcw780): penetration = krupp-scaled mass and caliber x velocity^1.38, normalization, overmatch when caliber > 14.3 x thickness, velocity loss after each plate, fuse armed by a plate at least the fuse threshold thick and fired after the fuse time.",
            "A plate struck at or past the always-ricochet angle ricochets. Strikes between the ricochet start and always-ricochet angles ricochet at random in the game and are treated as no ricochet.",
            "A citadel hit is a detonation inside the citadel volume, judged by how many citadel plates the ray crossed before the detonation (armor viewer rule).",
        ]),
        WeaponModel::FirstPlate { .. } => assumptions.extend([
            "HE and SAP defeat the first plate when penetration >= its nominal thickness; the strike angle does not change penetration. SAP ricochets off a plate struck at or past its always-ricochet angle unless its caliber > 14.3 x thickness overmatches the plate. HE does not ricochet or overmatch.",
            "Damage behind the first plate is not modelled.",
        ]),
    }

    assumptions
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "0%".to_owned();
    }

    format!("{:.0}%", part as f32 * 100.0 / whole as f32)
}

/// `source` is "main battery" or an aircraft name, so a sentence opening with
/// it needs its first letter raised.
fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn round1(value: f32) -> f32 {
    // Adding zero turns a negative zero into zero.
    (value * 10.0).round() / 10.0 + 0.0
}

fn round_mm(value: Millimeters) -> Millimeters {
    Millimeters::new(round1(value.value()))
}

fn round_m(value: Meters) -> Meters {
    Meters::new(round1(value.value()))
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use wowsunpack::export::armor_ray::cast_ray;

    use super::*;

    /// The armor viewer casts one ray at a time through every triangle and
    /// simulates every plate it crosses. Grid rays must find the same
    /// triangles, and the plates they keep must reach the same outcome.
    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn grid_rays_match_single_casts_of_the_armor_viewer() {
        let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"));
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let cache = std::env::temp_dir().join("wows-query-test-cache");
        let game = GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads");

        let target = game.resolve_ship("Hindenburg", &mut Interpretations::default()).expect("Hindenburg exists");
        let hull = game.target_hull(target, None).expect("a stock hull");
        let meshes = game.target_meshes(target, &hull.param).expect("the dump carries Hindenburg's armor");
        let shooter = game.resolve_ship("Des Moines", &mut Interpretations::default()).expect("Des Moines exists");
        let request = GunRequest { battery: Battery::Main, ammo: Ammo::Ap, range_km: Some(Km::new(10.0)), ifhe: false };
        let weapon = game.gun_weapon(shooter, &request).expect("Des Moines fires AP at 10 km");
        let WeaponModel::ArmorPiercing { params, impact_velocity } = &weapon.model else {
            panic!("AP is simulated plate by plate");
        };

        let rays = ParallelRays::new(&meshes, line_of_fire(Degrees::new(0.0), weapon.arrival.fall_angle_deg));
        let footprint = rays.footprint();
        let lerp = |min: ShipModelDistance, max: ShipModelDistance, fraction: f32| {
            ShipModelDistance::new(min.value() + (max.value() - min.value()) * fraction)
        };

        let mut compared = 0;
        let mut edge_crossings = 0;
        let mut outcomes: BTreeMap<RayOutcome, usize> = BTreeMap::new();
        for column in 0..60 {
            for row in 0..12 {
                let offset = RayOffset {
                    across: lerp(footprint.min.across, footprint.max.across, (column as f32 + 0.5) / 60.0),
                    up: lerp(footprint.min.up, footprint.max.up, (row as f32 + 0.5) / 12.0),
                };
                let grid_hits = rays.cast(offset);
                let viewer_hits = cast_ray(&meshes, rays.origin(offset), rays.direction());
                assert_eq!(grid_hits, viewer_hits, "ray at {offset:?}");

                let Some(first) = viewer_hits.first() else {
                    continue;
                };
                if first.position[1] < 0.0 {
                    continue;
                }

                let viewer_plates: Vec<PlateHit> = viewer_hits
                    .iter()
                    .map(|hit| PlateHit {
                        thickness: Millimeters::new(
                            meshes[hit.mesh_index].triangle_info[hit.triangle_index].thickness_mm,
                        ),
                        angle_from_normal: angle_from_normal(rays.direction(), hit.normal),
                        distance_along_ray: hit.distance - first.distance,
                    })
                    .collect();
                let viewer_zones: Vec<&str> = viewer_hits
                    .iter()
                    .map(|hit| meshes[hit.mesh_index].triangle_info[hit.triangle_index].zone.as_str())
                    .collect();
                let viewer_sim = simulate_shell_through_plates(params, *impact_velocity, &viewer_plates, false);
                let (viewer_outcome, _) = ap_outcome(&viewer_sim, &viewer_zones);

                // The viewer counts a plate crossed twice at one point (a
                // shared edge or a doubled face) twice; the trace counts it once.
                let traced = trace_ray(&weapon.model, ray_plates(&meshes, &grid_hits, rays.direction()));
                if traced.plates.len() != viewer_plates.len() {
                    edge_crossings += 1;
                    continue;
                }
                assert_eq!(traced.outcome, viewer_outcome, "ray at {offset:?}");
                *outcomes.entry(traced.outcome).or_default() += 1;
                compared += 1;
            }
        }

        assert!(compared > 100, "only {compared} rays struck armor above the waterline");
        assert!(edge_crossings * 20 < compared, "{edge_crossings} edge crossings in {compared} rays");
        assert!(outcomes.contains_key(&RayOutcome::Citadel), "{outcomes:?}");
    }
}
