//! The shape of a ship's citadel, how much of it sits above the waterline, and
//! the armor a projectile meets in front of it.
//!
//! Everything here is read from the armor model with grids of parallel rays.
//! Mesh space has the bow toward -Z and +Y up; the waterline is at Y = 0.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Degrees;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Millimeters;
use wows_core::units::Percent;
use wows_core::units::ShipModelDistance;
use wows_core::units::SquareMeters;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::effective_thickness;
use wowsunpack::ballistics::is_citadel_zone;
use wowsunpack::ballistics::is_overmatch;
use wowsunpack::ballistics::normalized_strike_angle;
use wowsunpack::export::gltf_export::InteractiveArmorMesh;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;

use crate::armor::PlateNames;
use crate::armor_trace::GridRay;
use crate::armor_trace::GunRequest;
use crate::armor_trace::RayPlate;
use crate::armor_trace::TargetHull;
use crate::armor_trace::WeaponModel;
use crate::armor_trace::bow_position;
use crate::armor_trace::cast_grid;
use crate::armor_trace::line_of_fire;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::shells::Ammo;
use crate::shells::Battery;

/// How rays with a glancing strike are treated in a [`CitadelApproach`].
pub(crate) const GLANCING_ASSUMPTION: &str = "Effective thickness spreads and stacks leave out glancing rays, counted in glancing_rays: a plate before or at the citadel is struck at or past the AP shell's always-ricochet angle and not overmatched, or, without an AP shell or on an overmatched plate, at 80 deg or more from its normal.";

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CitadelProfileArgs {
    /// Ship whose citadel is described, e.g. "Yamato".
    pub ship: String,

    /// Hull module, e.g. "Hull B". Defaults to the stock hull.
    pub hull: Option<String>,

    /// Ship whose main battery AP is fired at the citadel, e.g. "Des Moines". Needs range_km.
    pub shooter: Option<String>,

    /// Range in km for the shooter's plunging fire.
    pub range_km: Option<Km>,

    /// Distance between neighbouring rays in m, 0.25 to 10. Defaults to 0.5.
    pub spacing_m: Option<Meters>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CitadelProfileOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: String,
    pub hull: String,
    pub waterline: Waterline,
    pub exposure: CitadelExposure,
    pub shape: CitadelShape,

    /// Broadside with horizontal rays, then the shooter's fall angle when given.
    pub approaches: Vec<CitadelApproach>,

    pub verdict: String,
    pub assumptions: Vec<&'static str>,
}

/// Where the waterline is in the armor model.
#[derive(Debug, Clone, Serialize)]
pub struct Waterline {
    /// Deepest hull armor below model height 0, the waterline. Absent when the
    /// model has no hull armor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keel_below_waterline_m: Option<Meters>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CitadelExposure {
    /// Highest point of any citadel plate above the waterline.
    pub roof_above_waterline_m: Meters,

    /// Lowest point of any citadel plate, below the waterline when negative.
    pub floor_m: Meters,

    pub sides: Vec<SideExposure>,
}

/// The citadel face one side presents to horizontal rays.
#[derive(Debug, Clone, Serialize)]
pub struct SideExposure {
    pub side: Side,

    /// Area of the citadel seen from this side.
    pub face_area_m2: SquareMeters,

    /// Share of that area above the waterline. Absent when no ray from this
    /// side meets the citadel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub above_waterline_pct: Option<Percent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// The model's +X side.
    Starboard,
    Port,
}

#[derive(Debug, Clone, Serialize)]
pub struct CitadelShape {
    /// The citadel plates horizontal rays from either side meet first, by the
    /// height they are met at.
    pub bands: Vec<ShapeBand>,

    /// The most common kind of citadel plate met above the waterline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub above_waterline: Option<FaceSummary>,

    /// An armored deck slopes down behind a thicker belt to meet it near or
    /// below the waterline, and most horizontal rays meeting the citadel
    /// within 2 m of the waterline reach it through that deck.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turtleback: Option<Turtleback>,

    /// Top and bottom of the citadel along the ship.
    pub length_profile: Vec<Station>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FaceSummary {
    pub kind: FaceKind,
    pub share_pct: Percent,
    pub median_slope_deg: Degrees,
}

#[derive(Debug, Clone, Serialize)]
pub struct Turtleback {
    /// Median slope of the sloped deck from vertical.
    pub slope_deg: Degrees,
    pub median_deck_mm: Millimeters,
    pub median_belt_mm: Millimeters,

    /// Share of horizontal rays meeting the citadel within 2 m of the
    /// waterline that cross the sloped deck behind the belt.
    pub share_pct: Percent,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeBand {
    pub band: HeightBand,
    pub rays: usize,

    /// Median slope of the citadel plate from vertical: 0 is a vertical wall,
    /// 90 a flat deck.
    pub median_slope_deg: Degrees,

    /// Share of rays by the kind of citadel plate met, in percent.
    pub faces: BTreeMap<FaceKind, Percent>,
}

/// Height bands relative to the waterline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum HeightBand {
    #[serde(rename = "below -2 m")]
    Deep,
    #[serde(rename = "-2 to 0 m")]
    JustBelow,
    #[serde(rename = "0 to 2 m")]
    Waterline,
    #[serde(rename = "2 to 4 m")]
    Low,
    #[serde(rename = "above 4 m")]
    High,
}

impl HeightBand {
    fn of(height: ShipModelDistance) -> Self {
        let meters = height.to_meters().value();
        if meters < -2.0 {
            Self::Deep
        } else if meters < 0.0 {
            Self::JustBelow
        } else if meters < 2.0 {
            Self::Waterline
        } else if meters < 4.0 {
            Self::Low
        } else {
            Self::High
        }
    }
}

/// A plate by its slope from vertical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceKind {
    /// Under 10 degrees.
    Vertical,
    /// 10 to 25 degrees.
    InclinedBelt,
    /// 25 to 75 degrees.
    SlopedDeck,
    /// 75 degrees and more.
    FlatDeck,
}

impl FaceKind {
    fn of(slope: Degrees) -> Self {
        let slope = slope.value();
        if slope < 10.0 {
            Self::Vertical
        } else if slope < 25.0 {
            Self::InclinedBelt
        } else if slope < 75.0 {
            Self::SlopedDeck
        } else {
            Self::FlatDeck
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Station {
    pub from_bow_m: Meters,
    pub top_m: Meters,
    pub bottom_m: Meters,
}

/// What projectiles from one direction meet on the way to the citadel.
#[derive(Debug, Clone, Serialize)]
pub struct CitadelApproach {
    /// Below horizontal.
    pub fall_angle_deg: Degrees,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_km: Option<Km>,

    /// Rays whose first armor contact is above the waterline and that cross a
    /// citadel plate.
    pub rays_over_citadel: usize,

    /// Of `rays_over_citadel`, rays that strike a plate before or at the
    /// citadel at a glancing angle: at or past the AP shell's always-ricochet
    /// angle on a plate it does not overmatch, where it ricochets; without an
    /// AP shell or on an overmatched plate, at 80 degrees or more from the
    /// normal, where thickness / cos(strike angle) exceeds 5.7 times the
    /// plate. They are left out of the spreads and stacks, which they would
    /// otherwise dominate; `bands` counts every ray.
    pub glancing_rays: usize,

    /// Per ray that is not glancing, the sum of thickness / cos(strike angle)
    /// of the plates in front of the citadel. Absent when no such ray reaches it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armor_before_citadel_mm: Option<Spread>,

    /// Thickness / cos(strike angle) of the first citadel plate, per ray that
    /// is not glancing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citadel_plate_mm: Option<Spread>,

    /// By the height the citadel plate is met at.
    pub bands: Vec<ApproachBand>,

    /// The rays that are not glancing with the least, median and most armor
    /// in total.
    pub stacks: Vec<CitadelStack>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Spread {
    pub min: Millimeters,
    pub median: Millimeters,
    pub max: Millimeters,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApproachBand {
    pub band: HeightBand,
    pub rays: usize,

    /// Median strike angle on the citadel plate, from its normal.
    pub median_angle_deg: Degrees,

    /// Share of rays striking the citadel plate past 60 degrees, where its
    /// effective thickness is more than double.
    pub doubled_pct: Percent,

    /// Share of rays striking the citadel plate between the shell's ricochet
    /// angles, where the game rolls a ricochet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub may_ricochet_pct: Option<Percent>,

    /// Share of rays striking the citadel plate at or past the shell's
    /// always-ricochet angle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autobounce_pct: Option<Percent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CitadelStack {
    pub label: StackLabel,
    pub from_bow_m: Meters,

    /// Height the citadel plate is met at.
    pub height_m: Meters,

    /// In strike order, ending with the citadel plate.
    pub plates: Vec<StackPlate>,

    /// Plates in front of the citadel left out of `plates`.
    #[serde(skip_serializing_if = "is_zero")]
    pub more_plates: usize,

    pub total_effective_mm: Millimeters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StackLabel {
    Least,
    Median,
    Most,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackPlate {
    /// The game's name for the plate, led by its zone, e.g. "citadel: Armor Belt".
    pub plate: String,

    #[serde(skip)]
    pub material: ArmorMaterial,
    pub zone: String,

    pub thickness_mm: Millimeters,
    pub angle_deg: Degrees,
    pub effective_mm: Millimeters,

    /// What the shell does at this plate on its own, when a shooter is given.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shell: Vec<PlateEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlateEvent {
    /// Caliber > 14.3 x thickness: no ricochet, angle ignored.
    Overmatch,
    /// Between the ricochet start and always-ricochet angles.
    MayRicochet,
    /// At or past the always-ricochet angle.
    Autobounce,
    /// Thick enough after normalization to arm the fuse.
    ArmsFuse,
}

/// The shell flags in a citadel approach are read for.
pub(crate) struct ShellFlags<'a> {
    pub(crate) params: &'a ShellParams,
}

/// One ray that reaches the citadel, split at the first citadel plate.
struct CitadelRay<'a, 'b> {
    before: &'b [RayPlate<'a>],
    citadel: &'b RayPlate<'a>,
    before_mm: Millimeters,
    citadel_mm: Millimeters,
}

impl CitadelRay<'_, '_> {
    fn total_mm(&self) -> Millimeters {
        self.before_mm + self.citadel_mm
    }
}

/// A sloped deck and the thicker belt in front of it along one ray.
struct DeckBehindBelt<'a, 'b> {
    deck: &'b RayPlate<'a>,
    belt: &'b RayPlate<'a>,
}

/// The lowest and highest of a set of model heights.
#[derive(Debug, Clone, Copy)]
struct HeightRange {
    bottom: ShipModelDistance,
    top: ShipModelDistance,
}

impl HeightRange {
    fn at(height: ShipModelDistance) -> Self {
        Self { bottom: height, top: height }
    }

    fn extend(&mut self, height: ShipModelDistance) {
        self.bottom = ShipModelDistance::new(self.bottom.value().min(height.value()));
        self.top = ShipModelDistance::new(self.top.value().max(height.value()));
    }
}

impl GameData {
    /// Citadel exposure, shape and the armor in front of it, from the armor model.
    pub fn citadel_profile(&self, args: &CitadelProfileArgs) -> Result<CitadelProfileOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let spacing = args.spacing_m.unwrap_or(Meters::new(0.5));
        if !(0.25..=10.0).contains(&spacing.value()) {
            return Err(QueryError::InvalidArguments { reason: "spacing_m must be between 0.25 and 10" });
        }

        let weapon = match (&args.shooter, args.range_km) {
            (Some(shooter), Some(range_km)) => {
                let shooter = self.resolve_ship(shooter, &mut interpretations)?;
                let request =
                    GunRequest { battery: Battery::Main, ammo: Ammo::Ap, range_km: Some(range_km), ifhe: false };
                Some(self.gun_weapon(shooter, &request)?)
            }
            (None, None) => None,
            _ => return Err(QueryError::InvalidArguments { reason: "give shooter and range_km together" }),
        };
        let shell = weapon.as_ref().and_then(|weapon| match &weapon.model {
            WeaponModel::ArmorPiercing { params, .. } => Some(ShellFlags { params }),
            WeaponModel::FirstPlate { .. } => None,
        });

        let TargetHull { param: hull_param, name: hull_name } = self.target_hull(ship, args.hull.as_deref())?;
        let meshes = self.target_meshes(ship, &hull_param)?;
        let bow = bow_position(&meshes);
        if !has_citadel_plates(&meshes) {
            return Err(QueryError::NoCitadel { ship: ship.name.clone() });
        }
        let starboard = cast_grid(&meshes, line_of_fire(Degrees::new(0.0), Degrees::new(0.0)), spacing)?;
        let port = cast_grid(&meshes, line_of_fire(Degrees::new(180.0), Degrees::new(0.0)), spacing)?;
        let Some(CitadelGeometry { exposure, shape }) =
            citadel_geometry(&meshes, &starboard.rays, &port.rays, spacing, bow)
        else {
            return Err(QueryError::NoCitadel { ship: ship.name.clone() });
        };

        let materials = meshes.iter().flat_map(|mesh| mesh.triangle_info.iter().map(|info| info.material));
        let names = self.plate_names(ship, materials);
        let mut approaches = vec![approach(&starboard.rays, Degrees::new(0.0), None, shell.as_ref(), bow, &names)];
        if let Some(weapon) = &weapon {
            let fall = weapon.arrival.fall_angle_deg;
            let plunging = cast_grid(&meshes, line_of_fire(Degrees::new(0.0), fall), spacing)?;
            approaches.push(approach(&plunging.rays, fall, weapon.arrival.range_km, shell.as_ref(), bow, &names));
        }

        let verdict = verdict(&shape, &exposure, &approaches);

        Ok(CitadelProfileOutput {
            interpretations,
            ship: ship.label(),
            hull: hull_name,
            waterline: Waterline {
                keel_below_waterline_m: keel_depth(&meshes).map(|keel| round_m(Meters::new(-keel.to_meters().value()))),
            },
            exposure,
            shape,
            approaches,
            verdict,
            assumptions: vec![
                "The waterline is model height 0, where the armor viewer draws the water.",
                "Rays are parallel and evenly spaced, so shares are by area seen along the ray direction.",
                "Effective thickness is thickness / cos(strike angle) with no normalization. Shell events use the shell's normalization for the fuse and its ricochet angles; ricochets between the two angles are random in the game.",
                GLANCING_ASSUMPTION,
                "Approaches fire at the model's starboard side with the hull level, no heel or pitch, and turrets at their default heading.",
            ],
        })
    }
}

/// The citadel as horizontal rays from either side see it, `spacing` apart.
pub(crate) struct CitadelGeometry {
    pub(crate) exposure: CitadelExposure,
    pub(crate) shape: CitadelShape,
}

/// The citadel's extent against the waterline, the face each side presents
/// and the shape of its plates. `None` when the armor model has no citadel
/// plates.
pub(crate) fn citadel_geometry(
    meshes: &[InteractiveArmorMesh],
    starboard: &[GridRay],
    port: &[GridRay],
    spacing: Meters,
    bow: ShipModelDistance,
) -> Option<CitadelGeometry> {
    let extent = citadel_extent(meshes)?;

    let cell_area = SquareMeters::new(spacing.value() * spacing.value());
    let sides = vec![side_exposure(Side::Starboard, starboard, cell_area), side_exposure(Side::Port, port, cell_area)];

    let side_rays: Vec<&GridRay> = starboard.iter().chain(port).collect();
    let shape = CitadelShape {
        bands: shape_bands(&side_rays),
        above_waterline: above_waterline(&side_rays),
        turtleback: turtleback(&side_rays),
        length_profile: length_profile(&side_rays, bow),
    };

    Some(CitadelGeometry {
        exposure: CitadelExposure {
            roof_above_waterline_m: round_m(extent.top.to_meters()),
            floor_m: round_m(extent.bottom.to_meters()),
            sides,
        },
        shape,
    })
}

/// The armor in front of the citadel for rays along one direction. Only rays
/// whose first armor contact is above the waterline count, as in a trace.
pub(crate) fn approach(
    rays: &[GridRay],
    fall: Degrees,
    range: Option<Km>,
    shell: Option<&ShellFlags>,
    bow: ShipModelDistance,
    names: &PlateNames,
) -> CitadelApproach {
    let mut reaching: Vec<CitadelRay> =
        rays.iter().filter(|ray| !ray.underwater).filter_map(|ray| citadel_ray(&ray.plates)).collect();
    reaching.sort_by(|left, right| left.total_mm().value().total_cmp(&right.total_mm().value()));

    let mut by_band: BTreeMap<HeightBand, Vec<&CitadelRay>> = BTreeMap::new();
    for ray in &reaching {
        by_band.entry(HeightBand::of(height_of(ray.citadel))).or_default().push(ray);
    }
    let bands = by_band
        .into_iter()
        .filter_map(|(band, rays)| {
            let angles: Vec<f32> = rays.iter().map(|ray| ray.citadel.angle.value()).collect();
            let share = |predicate: &dyn Fn(&CitadelRay) -> bool| {
                percent(rays.iter().filter(|ray| predicate(ray)).count(), rays.len())
            };
            Some(ApproachBand {
                band,
                rays: rays.len(),
                median_angle_deg: round_deg(Degrees::new(median(angles)?)),
                doubled_pct: share(&|ray| ray.citadel.thickness.value() > 0.0 && ray.citadel.angle.value() > 60.0),
                may_ricochet_pct: shell
                    .map(|shell| share(&|ray| shell_events(shell, ray.citadel).contains(&PlateEvent::MayRicochet))),
                autobounce_pct: shell
                    .map(|shell| share(&|ray| shell_events(shell, ray.citadel).contains(&PlateEvent::Autobounce))),
            })
        })
        .collect();

    let clear: Vec<&CitadelRay> = reaching
        .iter()
        .filter(|ray| !ray.before.iter().chain(std::iter::once(ray.citadel)).any(|plate| is_glancing(plate, shell)))
        .collect();
    let stacks = if clear.is_empty() {
        Vec::new()
    } else {
        [(StackLabel::Least, 0), (StackLabel::Median, clear.len() / 2), (StackLabel::Most, clear.len() - 1)]
            .into_iter()
            .map(|(label, index)| stack(label, clear[index], shell, bow, names))
            .collect()
    };

    CitadelApproach {
        fall_angle_deg: fall,
        range_km: range,
        rays_over_citadel: reaching.len(),
        glancing_rays: reaching.len() - clear.len(),
        armor_before_citadel_mm: spread(clear.iter().map(|ray| ray.before_mm).collect()),
        citadel_plate_mm: spread(clear.iter().map(|ray| ray.citadel_mm).collect()),
        bands,
        stacks,
    }
}

/// Whether `plate` is struck at an angle that leaves its ray out of the
/// effective thickness spreads (see [`CitadelApproach::glancing_rays`]).
fn is_glancing(plate: &RayPlate, shell: Option<&ShellFlags>) -> bool {
    const GRAZING: Degrees = Degrees::new(80.0);

    if plate.thickness.value() <= 0.0 {
        return false;
    }

    match shell {
        Some(shell) if !is_overmatch(shell.params.caliber, plate.thickness) => {
            plate.angle.to_radians() >= shell.params.always_ricochet_angle
        }
        _ => plate.angle >= GRAZING,
    }
}

fn citadel_ray<'a, 'b>(plates: &'b [RayPlate<'a>]) -> Option<CitadelRay<'a, 'b>> {
    let index = plates.iter().position(|plate| is_citadel_zone(plate.zone))?;
    let before = &plates[..index];
    let citadel = &plates[index];

    Some(CitadelRay {
        before,
        citadel,
        before_mm: before.iter().map(effective_mm).sum(),
        citadel_mm: effective_mm(citadel),
    })
}

fn side_exposure(side: Side, rays: &[GridRay], cell_area: SquareMeters) -> SideExposure {
    let heights: Vec<ShipModelDistance> =
        rays.iter().filter_map(|ray| first_citadel(&ray.plates)).map(height_of).collect();
    let above = heights.iter().filter(|height| height.value() >= 0.0).count();

    SideExposure {
        side,
        face_area_m2: SquareMeters::new((heights.len() as f32 * cell_area.value()).round()),
        above_waterline_pct: (!heights.is_empty()).then(|| percent(above, heights.len())),
    }
}

fn shape_bands(rays: &[&GridRay]) -> Vec<ShapeBand> {
    let mut by_band: BTreeMap<HeightBand, Vec<Degrees>> = BTreeMap::new();
    for plate in rays.iter().filter_map(|ray| first_citadel(&ray.plates)) {
        by_band.entry(HeightBand::of(height_of(plate))).or_default().push(slope_from_vertical(plate));
    }

    by_band
        .into_iter()
        .filter_map(|(band, slopes)| {
            let mut counts: BTreeMap<FaceKind, usize> = BTreeMap::new();
            for slope in &slopes {
                *counts.entry(FaceKind::of(*slope)).or_default() += 1;
            }
            let faces = counts.into_iter().map(|(kind, count)| (kind, percent(count, slopes.len()))).collect();

            Some(ShapeBand { band, rays: slopes.len(), median_slope_deg: median_degrees(&slopes)?, faces })
        })
        .collect()
}

fn above_waterline(rays: &[&GridRay]) -> Option<FaceSummary> {
    let slopes: Vec<Degrees> = rays
        .iter()
        .filter_map(|ray| first_citadel(&ray.plates))
        .filter(|plate| height_of(plate).value() >= 0.0)
        .map(slope_from_vertical)
        .collect();

    let mut by_kind: BTreeMap<FaceKind, Vec<Degrees>> = BTreeMap::new();
    for slope in &slopes {
        by_kind.entry(FaceKind::of(*slope)).or_default().push(*slope);
    }
    let (kind, kind_slopes) = by_kind.into_iter().max_by_key(|(_, kind_slopes)| kind_slopes.len())?;

    Some(FaceSummary {
        kind,
        share_pct: percent(kind_slopes.len(), slopes.len()),
        median_slope_deg: median_degrees(&kind_slopes)?,
    })
}

/// Horizontal rays meeting the citadel within 2 m of the waterline mostly
/// cross a sloped armored deck (25 to 75 degrees from vertical) behind a
/// near-vertical belt at least half again as thick, and the deck's foot sits
/// no higher than a metre above the waterline.
fn turtleback(rays: &[&GridRay]) -> Option<Turtleback> {
    const LOWEST: Meters = Meters::new(-2.0);
    const HIGHEST: Meters = Meters::new(2.0);
    const FOOT_HEIGHT: Meters = Meters::new(1.0);
    const BELT_OVER_DECK: f32 = 1.5;

    let mut considered = 0;
    let mut decks: Vec<DeckBehindBelt> = Vec::new();
    for ray in rays {
        let Some(citadel) = citadel_ray(&ray.plates) else {
            continue;
        };
        let height = height_of(citadel.citadel).to_meters();
        if !(LOWEST..HIGHEST).contains(&height) {
            continue;
        }
        considered += 1;

        let path = &ray.plates[..=citadel.before.len()];
        let found = path.iter().enumerate().find_map(|(index, deck)| {
            if deck.thickness.value() <= 0.0 || FaceKind::of(slope_from_vertical(deck)) != FaceKind::SlopedDeck {
                return None;
            }
            path[..index]
                .iter()
                .find(|belt| {
                    slope_from_vertical(belt).value() < 25.0
                        && belt.thickness.value() >= deck.thickness.value() * BELT_OVER_DECK
                })
                .map(|belt| DeckBehindBelt { deck, belt })
        });
        if let Some(pair) = found {
            decks.push(pair);
        }
    }

    let foot = decks.iter().map(|pair| height_of(pair.deck).to_meters().value()).reduce(f32::min)?;
    if decks.len() * 2 < considered || foot > FOOT_HEIGHT.value() {
        return None;
    }

    let slopes: Vec<Degrees> = decks.iter().map(|pair| slope_from_vertical(pair.deck)).collect();
    Some(Turtleback {
        slope_deg: median_degrees(&slopes)?,
        median_deck_mm: Millimeters::new(median(decks.iter().map(|pair| pair.deck.thickness.value()).collect())?),
        median_belt_mm: Millimeters::new(median(decks.iter().map(|pair| pair.belt.thickness.value()).collect())?),
        share_pct: percent(decks.len(), considered),
    })
}

fn length_profile(rays: &[&GridRay], bow: ShipModelDistance) -> Vec<Station> {
    const STATION_COUNT: usize = 8;

    let points: Vec<[f32; 3]> = rays
        .iter()
        .flat_map(|ray| ray.plates.iter())
        .filter(|plate| is_citadel_zone(plate.zone))
        .map(|plate| plate.position)
        .collect();
    let Some(start) = points.iter().map(|point| point[2]).reduce(f32::min) else {
        return Vec::new();
    };
    let end = points.iter().map(|point| point[2]).fold(start, f32::max);
    let length = (end - start).max(f32::EPSILON);

    let mut stations: Vec<Option<HeightRange>> = vec![None; STATION_COUNT];
    for point in &points {
        let index = (((point[2] - start) / length) * STATION_COUNT as f32) as usize;
        let height = ShipModelDistance::new(point[1]);
        stations[index.min(STATION_COUNT - 1)].get_or_insert(HeightRange::at(height)).extend(height);
    }

    stations
        .into_iter()
        .enumerate()
        .filter_map(|(index, extent)| {
            let extent = extent?;
            let middle = start + length * (index as f32 + 0.5) / STATION_COUNT as f32;
            Some(Station {
                from_bow_m: round_m((ShipModelDistance::new(middle) - bow).to_meters()),
                top_m: round_m(extent.top.to_meters()),
                bottom_m: round_m(extent.bottom.to_meters()),
            })
        })
        .collect()
}

fn stack(
    label: StackLabel,
    ray: &CitadelRay,
    shell: Option<&ShellFlags>,
    bow: ShipModelDistance,
    names: &PlateNames,
) -> CitadelStack {
    const STACK_PLATE_LIMIT: usize = 8;

    // The plates nearest the citadel matter most, so the list keeps those.
    let kept = ray.before.len().min(STACK_PLATE_LIMIT - 1);
    let plates = ray.before[ray.before.len() - kept..]
        .iter()
        .chain(std::iter::once(ray.citadel))
        .map(|plate| StackPlate {
            plate: names.of(plate.material),
            material: plate.material,
            zone: plate.zone.to_owned(),
            thickness_mm: round_mm(plate.thickness),
            angle_deg: round_deg(plate.angle),
            effective_mm: round_mm(effective_mm(plate)),
            shell: shell.map(|shell| shell_events(shell, plate)).unwrap_or_default(),
        })
        .collect();

    CitadelStack {
        label,
        from_bow_m: round_m((ShipModelDistance::new(ray.citadel.position[2]) - bow).to_meters()),
        height_m: round_m(height_of(ray.citadel).to_meters()),
        plates,
        more_plates: ray.before.len() - kept,
        total_effective_mm: round_mm(ray.total_mm()),
    }
}

/// What the shell does at `plate` taken alone.
fn shell_events(shell: &ShellFlags, plate: &RayPlate) -> Vec<PlateEvent> {
    let params = shell.params;
    if plate.thickness.value() <= 0.0 {
        return Vec::new();
    }
    if is_overmatch(params.caliber, plate.thickness) {
        let mut events = vec![PlateEvent::Overmatch];
        if plate.thickness >= params.fuse_threshold {
            events.push(PlateEvent::ArmsFuse);
        }
        return events;
    }

    let angle = plate.angle.to_radians();
    let mut events = Vec::new();
    // The simulation does not arm the fuse on a plate the shell bounces off.
    if angle >= params.always_ricochet_angle {
        return vec![PlateEvent::Autobounce];
    }
    if angle >= params.ricochet_angle {
        events.push(PlateEvent::MayRicochet);
    }

    let effective =
        effective_thickness(plate.thickness, normalized_strike_angle(angle, params.effective_normalization()));
    if effective >= params.fuse_threshold {
        events.push(PlateEvent::ArmsFuse);
    }

    events
}

fn first_citadel<'a, 'b>(plates: &'b [RayPlate<'a>]) -> Option<&'b RayPlate<'a>> {
    plates.iter().find(|plate| is_citadel_zone(plate.zone))
}

pub(crate) fn has_citadel_plates(meshes: &[InteractiveArmorMesh]) -> bool {
    citadel_extent(meshes).is_some()
}

/// The lowest and highest model height of any citadel triangle.
fn citadel_extent(meshes: &[InteractiveArmorMesh]) -> Option<HeightRange> {
    let mut extent: Option<HeightRange> = None;
    for mesh in meshes.iter().filter(|mesh| mesh.transform.is_none()) {
        for (triangle, _) in mesh.triangle_info.iter().enumerate().filter(|(_, info)| is_citadel_zone(&info.zone)) {
            for &index in &mesh.indices[triangle * 3..triangle * 3 + 3] {
                let height = ShipModelDistance::new(mesh.positions[index as usize][1]);
                extent.get_or_insert(HeightRange::at(height)).extend(height);
            }
        }
    }

    extent
}

/// The lowest model height of the hull armor.
fn keel_depth(meshes: &[InteractiveArmorMesh]) -> Option<ShipModelDistance> {
    meshes
        .iter()
        .filter(|mesh| mesh.transform.is_none())
        .flat_map(|mesh| mesh.positions.iter())
        .map(|position| position[1])
        .reduce(f32::min)
        .map(ShipModelDistance::new)
}

fn height_of(plate: &RayPlate) -> ShipModelDistance {
    ShipModelDistance::new(plate.position[1])
}

/// Slope of a plate from vertical, from its normal: 0 is a wall, 90 a deck.
fn slope_from_vertical(plate: &RayPlate) -> Degrees {
    Degrees::new(plate.normal[1].abs().min(1.0).asin().to_degrees())
}

fn effective_mm(plate: &RayPlate) -> Millimeters {
    effective_thickness(plate.thickness, plate.angle.to_radians())
}

fn verdict(shape: &CitadelShape, exposure: &CitadelExposure, approaches: &[CitadelApproach]) -> String {
    let face = match &shape.above_waterline {
        Some(face) => {
            let kind = match face.kind {
                FaceKind::Vertical => "a vertical wall",
                FaceKind::InclinedBelt => "an inclined belt",
                FaceKind::SlopedDeck => "a sloped deck",
                FaceKind::FlatDeck => "only its deck edge",
            };
            format!(
                "Above the waterline the citadel shows {kind} ({} deg from vertical on {}% of it)",
                face.median_slope_deg.value(),
                face.share_pct.value()
            )
        }
        None => "The citadel side face is entirely below the waterline".to_owned(),
    };
    let turtleback = shape.turtleback.as_ref().map(|turtleback| {
        format!(
            "Turtleback: a {} mm deck sloped {} deg from vertical behind a {} mm belt, crossed by {}% of horizontal rays meeting the citadel within 2 m of the waterline. ",
            turtleback.median_deck_mm.value(),
            turtleback.slope_deg.value(),
            turtleback.median_belt_mm.value(),
            turtleback.share_pct.value()
        )
    });
    let roof = exposure.roof_above_waterline_m.value();
    let roof_text = if roof >= 0.0 {
        format!("the roof is {roof} m above the waterline")
    } else {
        format!("the roof is {} m below the waterline", -roof)
    };
    let above_share =
        shape.above_waterline.as_ref().and(exposure.sides.first()).and_then(|side| side.above_waterline_pct);
    let exposure_text = match above_share {
        Some(share) => format!("; {}% of the side face is above the waterline and {roof_text}", share.value()),
        None => format!("; {roof_text}"),
    };

    let effects: Vec<String> =
        approaches.iter().filter(|approach| approach.rays_over_citadel > 0).map(effect).collect();
    let effects = if effects.is_empty() { String::new() } else { format!(" {}", effects.join(" ")) };

    format!("{}{face}{exposure_text}.{effects}", turtleback.unwrap_or_default())
}

/// How steeply one approach strikes the citadel plate, as a sentence.
fn effect(approach: &CitadelApproach) -> String {
    let rays = approach.rays_over_citadel as f32;
    let weighted = |value: fn(&ApproachBand) -> Option<Percent>| -> Option<f32> {
        let total: Option<f32> =
            approach.bands.iter().map(|band| value(band).map(|share| share.value() * band.rays as f32)).sum();
        total.map(|total| round1(total / rays))
    };
    let doubled = weighted(|band| Some(band.doubled_pct)).unwrap_or(0.0);
    let at = match approach.range_km {
        Some(range) => format!("At {range}"),
        None => "At broadside with flat fire".to_owned(),
    };

    match weighted(|band| band.autobounce_pct) {
        Some(bounce) => format!(
            "{at}, {doubled}% of hits over the citadel strike its plate past 60 deg (effective thickness over double) and {bounce}% at or past the shell's always-ricochet angle."
        ),
        None => format!(
            "{at}, {doubled}% of hits over the citadel strike its plate past 60 deg (effective thickness over double)."
        ),
    }
}

fn spread(mut values: Vec<Millimeters>) -> Option<Spread> {
    values.sort_by(|left, right| left.value().total_cmp(&right.value()));
    let min = values.first().copied()?;

    Some(Spread {
        min: round_mm(min),
        median: round_mm(values[values.len() / 2]),
        max: round_mm(values[values.len() - 1]),
    })
}

/// The middle value, or the upper of the two middle values; `None` when empty.
fn median(mut values: Vec<f32>) -> Option<f32> {
    values.sort_by(f32::total_cmp);

    values.get(values.len() / 2).copied()
}

fn median_degrees(values: &[Degrees]) -> Option<Degrees> {
    let median = median(values.iter().map(|value| value.value()).collect())?;

    Some(round_deg(Degrees::new(median)))
}

fn round1(value: f32) -> f32 {
    // Adding zero turns a negative zero into zero.
    (value * 10.0).round() / 10.0 + 0.0
}

/// `part` as a share of `whole`, to one decimal.
fn percent(part: usize, whole: usize) -> Percent {
    Percent::new(round1(part as f32 * 100.0 / whole as f32))
}

fn round_m(value: Meters) -> Meters {
    Meters::new(round1(value.value()))
}

fn round_mm(value: Millimeters) -> Millimeters {
    Millimeters::new(round1(value.value()))
}

fn round_deg(value: Degrees) -> Degrees {
    Degrees::new(round1(value.value()))
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}
