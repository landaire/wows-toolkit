//! Splash volumes: the box a high-explosive or semi-armour-piercing shell
//! bursts in, and the parts of the ship that box reaches.
//!
//! A shell that does not penetrate still damages what its burst overlaps. The
//! game sizes that burst from the shell's own calibre and tests it against
//! named boxes shipped with the hull, which is what this reproduces: the
//! boxes come from `wowsunpack::models::geometry::parse_splash_file` and the
//! zones they belong to from GameParams' own hit locations, so the egui
//! viewer and this one answer from the same data.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::HashSet;

use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wows_toolkit_viewmodel::armor::penetration::Ifhe;
use wows_toolkit_viewmodel::armor::penetration::PenResult;
use wows_toolkit_viewmodel::armor::penetration::check_penetration;
use wows_toolkit_viewmodel::armor::penetration::he_penetration;
use wows_toolkit_viewmodel::armor::penetration::sap_penetration;
use wowsunpack::export::armor_ray::transform_point;
use wowsunpack::export::gltf_export::InteractiveArmorMesh;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::HitLocation;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;
use wowsunpack::models::geometry::SplashBox;

use crate::viewport::types::Vertex;

/// A length in the ship model's own space.
///
/// Distinct from [`Millimeters`] and from a real-world distance because the
/// splash box coordinates are neither: mixing them silently would size a
/// burst wrongly rather than fail to compile.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct ModelUnit(f32);

impl ModelUnit {
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

/// Colour for armour a shell's burst can get through.
const PEN_COLOR: [f32; 4] = [0.2, 0.9, 0.2, 0.55];
/// Colour for armour it cannot.
const NO_PEN_COLOR: [f32; 4] = [0.9, 0.2, 0.2, 0.55];
/// Colour of the burst volume itself.
pub const CUBE_COLOR: [f32; 4] = [1.0, 0.7, 0.1, 0.7];
/// Colour of the box outlines the reader can switch on.
pub const BOX_COLOR: [f32; 4] = [0.3, 0.7, 1.0, 0.6];
/// Colour of a box outline while its row in the splash zone list is hovered.
const BOX_HIGHLIGHT_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.9];

/// Half-width of a drawn wireframe edge, in model units.
const EDGE_HALF_WIDTH: f32 = 0.003;
/// Half-width of a highlighted box's edges, wide enough to stand out over its plain outline.
const HIGHLIGHT_EDGE_HALF_WIDTH: f32 = 0.006;

/// How far a shaded triangle is lifted off the plate it belongs to, so it
/// does not fight with it for the same depth.
const HIGHLIGHT_OFFSET: f32 = 0.006;

/// The splash boxes a loaded ship carries, and the zones they belong to.
pub struct ShipSplashData {
    /// The named boxes the hull ships with.
    pub boxes: Vec<SplashBox>,
    /// Which box belongs to which zone, as GameParams states it. Empty when
    /// the ship names no hit locations, in which case a box is known only by
    /// its own name.
    pub box_to_zone: HashMap<String, String>,
}

/// One zone a burst reached.
#[derive(Clone, Debug)]
pub struct SplashZoneHit {
    /// What to call the zone: GameParams' own name where there is one, the
    /// box's own name made readable where there is not.
    pub zone_name: String,
    pub box_name: String,
    /// The zone's plating. `None` when GameParams names no hit location for
    /// it, which is not zero-thickness armour but an unknown one.
    pub thickness: Option<Millimeters>,
    /// The zone's health. `None` for the same reason.
    pub max_hp: Option<f32>,
    /// Whether the burst started inside this box rather than merely reaching
    /// it.
    pub is_direct_hit: bool,
}

/// What a burst at one point reached.
pub struct SplashResult {
    pub impact_point: [f32; 3],
    pub half_extent: ModelUnit,
    /// The box the burst started inside, when it started inside one.
    pub direct_hit_box: Option<String>,
    /// Every zone the burst reached, the one it started in first.
    pub hit_zones: Vec<SplashZoneHit>,
    /// The plating of each armour triangle whose centre falls inside the
    /// burst, which a shell is counted against afterwards.
    pub plates_in_volume: Vec<Millimeters>,
}

impl SplashResult {
    /// Whether the burst reached anything at all.
    pub fn reached_anything(&self) -> bool {
        !self.hit_zones.is_empty()
    }
}

/// Reads the splash boxes a hull ships with.
///
/// `None` when the hull carries no splash file, which is a real state: not
/// every model has one, and the mode says so rather than drawing an empty
/// burst.
pub fn parse_ship_splash_data(
    splash_bytes: Option<&[u8]>,
    hit_locations: Option<&HashMap<String, HitLocation>>,
) -> Option<ShipSplashData> {
    let bytes = splash_bytes?;
    // The armor meshes have their Z negated to get from the game's
    // left-handed space to a right-handed one; the boxes have to follow or
    // they would sit mirrored through the hull.
    let boxes: Vec<SplashBox> = wowsunpack::models::geometry::parse_splash_file(bytes)
        .ok()?
        .into_iter()
        .map(|mut shape| {
            let (near, far) = (-shape.min[2], -shape.max[2]);
            shape.min[2] = near.min(far);
            shape.max[2] = near.max(far);
            shape
        })
        .collect();

    let mut box_to_zone = HashMap::new();
    if let Some(locations) = hit_locations {
        for (zone, location) in locations {
            for name in location.splash_boxes() {
                box_to_zone.insert(name.clone(), zone.clone());
            }
        }
    }

    Some(ShipSplashData { boxes, box_to_zone })
}

/// How far a shell's burst reaches, from its calibre.
///
/// The game passes `bulletDiametr / 6` to its own splash routine, and the
/// boxes are in the same space as that figure.
pub fn splash_half_extent(caliber: Millimeters) -> ModelUnit {
    ModelUnit::new((caliber / 6.0).to_meters().value())
}

/// A readable name for a splash box.
///
/// The game names them `XX_SB_<part>_<index>`; the prefix says nothing to a
/// reader and the part is an abbreviation.
pub fn prettify_box_name(box_name: &str) -> String {
    let stripped = box_name.find("_SB_").map(|at| &box_name[at + 4..]).unwrap_or(box_name);
    let part: String =
        stripped.split('_').take_while(|piece| piece.chars().all(char::is_alphabetic)).collect::<Vec<_>>().join("_");

    let label = match part.as_str() {
        "gk" => "Turret",
        "engine" => "Engine",
        "bow" => "Bow",
        "stern" => "Stern",
        "cit" => "Citadel",
        "ss" => "Superstructure",
        "ssc" => "Superstructure (casemate)",
        "ruder" => "Steering Gear",
        "cit_ammo" => "Magazine",
        "cas" => "Casemate",
        other => other,
    };

    let index: Vec<&str> = stripped.split('_').skip_while(|piece| piece.chars().all(char::is_alphabetic)).collect();
    if index.is_empty() { label.to_string() } else { format!("{} {}", label, index.join(".")) }
}

/// Groups the boxes by the part of the ship they belong to, for a list that
/// can be folded rather than one row per box.
pub fn build_splash_box_groups(boxes: &[SplashBox]) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for shape in boxes {
        let label = prettify_box_name(&shape.name);
        // The group is the part, without the index that distinguishes one
        // turret from another.
        let group = label.split(' ').next().unwrap_or(&label).to_string();
        match groups.iter_mut().find(|(name, _)| *name == group) {
            Some((_, members)) => members.push(shape.name.clone()),
            None => groups.push((group, vec![shape.name.clone()])),
        }
    }
    groups
}

/// Whether two boxes overlap. Touching faces do not count: a burst that ends
/// exactly on a boundary has not reached past it.
fn boxes_overlap(a_min: [f32; 3], a_max: [f32; 3], b_min: [f32; 3], b_max: [f32; 3]) -> bool {
    (0..3).all(|axis| a_max[axis] > b_min[axis] && a_min[axis] < b_max[axis])
}

/// Whether a point is inside a box, boundary included.
fn point_in_box(point: [f32; 3], min: [f32; 3], max: [f32; 3]) -> bool {
    (0..3).all(|axis| point[axis] >= min[axis] && point[axis] <= max[axis])
}

/// What a burst of the given reach, centred at `impact_point`, gets to.
///
/// Shell-independent: this says what the burst overlaps, and a shell is held
/// against it afterwards.
pub fn compute_splash(
    impact_point: [f32; 3],
    half_extent: ModelUnit,
    splash_data: &ShipSplashData,
    hit_locations: Option<&HashMap<String, HitLocation>>,
    armor_meshes: &[InteractiveArmorMesh],
) -> SplashResult {
    let reach = half_extent.value();
    let burst_min = [impact_point[0] - reach, impact_point[1] - reach, impact_point[2] - reach];
    let burst_max = [impact_point[0] + reach, impact_point[1] + reach, impact_point[2] + reach];

    let direct_hit_box = splash_data
        .boxes
        .iter()
        .find(|shape| point_in_box(impact_point, shape.min, shape.max))
        .map(|shape| shape.name.clone());

    let mut hit_zones = Vec::new();
    let mut seen = HashSet::new();
    for shape in &splash_data.boxes {
        let inside = point_in_box(impact_point, shape.min, shape.max);
        let reached = boxes_overlap(burst_min, burst_max, shape.min, shape.max);
        if !(inside || reached) || !seen.insert(shape.name.clone()) {
            continue;
        }

        let zone = splash_data.box_to_zone.get(&shape.name);
        let zone_name = zone.cloned().unwrap_or_else(|| prettify_box_name(&shape.name));
        let location = hit_locations.and_then(|locations| zone.and_then(|zone| locations.get(zone)));
        hit_zones.push(SplashZoneHit {
            zone_name,
            box_name: shape.name.clone(),
            thickness: location.and_then(|found| found.thickness()).map(Millimeters::new),
            max_hp: location.and_then(|found| found.max_hp()),
            is_direct_hit: direct_hit_box.as_ref() == Some(&shape.name),
        });
    }

    // What the burst started inside is what it did the most to, so it leads.
    hit_zones.sort_by(|a, b| b.is_direct_hit.cmp(&a.is_direct_hit).then_with(|| a.zone_name.cmp(&b.zone_name)));

    SplashResult {
        impact_point,
        half_extent,
        direct_hit_box,
        hit_zones,
        plates_in_volume: plates_in_burst(armor_meshes, burst_min, burst_max),
    }
}

/// The plating of every armour triangle whose centre lies inside the burst.
///
/// A triangle the mesh carries no plate record for is left out: its plating
/// is unknown, and counting it as zero would claim it is penetrated.
fn plates_in_burst(meshes: &[InteractiveArmorMesh], burst_min: [f32; 3], burst_max: [f32; 3]) -> Vec<Millimeters> {
    let mut plates = Vec::new();
    for mesh in meshes {
        for (triangle, corners) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
            let Some(info) = mesh.triangle_info.get(triangle) else { continue };
            let Some(centre) = triangle_centre(mesh, corners) else { continue };

            if point_in_box(centre, burst_min, burst_max) {
                plates.push(Millimeters::new(info.thickness_mm));
            }
        }
    }

    plates
}

/// A triangle's centre in ship space. `None` when the mesh's own indices do
/// not reach its corners, which a truncated model can produce.
fn triangle_centre(mesh: &InteractiveArmorMesh, corners: &[u32; 3]) -> Option<[f32; 3]> {
    let mut centre = [0.0_f32; 3];
    for &corner in corners {
        let position = *mesh.positions.get(corner as usize)?;
        let position = match &mesh.transform {
            Some(transform) => transform_point(transform, position),
            None => position,
        };
        for (sum, coordinate) in centre.iter_mut().zip(position) {
            *sum += coordinate / 3.0;
        }
    }

    Some(centre)
}

/// The shell whose burst a ship is asked about: its largest high-explosive or
/// semi-armour-piercing one. An armour-piercing round that fails to
/// penetrate does not burst outside the hull.
pub fn bursting_shell(shells: &[ShellInfo]) -> Option<&ShellInfo> {
    shells
        .iter()
        .filter(|shell| matches!(shell.ammo_type, AmmoType::HE | AmmoType::SAP))
        .max_by(|a, b| a.caliber.value().total_cmp(&b.caliber.value()))
}

/// The flat penetration an HE or SAP shell brings. `None` for any other shell
/// and for one that carries no published figure: an absent figure is not zero
/// penetration.
fn flat_penetration(shell: &ShellInfo, ifhe: Ifhe) -> Option<Millimeters> {
    match shell.ammo_type {
        AmmoType::HE => he_penetration(shell, ifhe),
        AmmoType::SAP => sap_penetration(shell),
        AmmoType::AP | AmmoType::Unknown(_) => None,
    }
}

/// One HE or SAP shell of a compared ship held against a zone's plating.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellVerdict {
    pub ship_name: String,
    pub ammo_type: AmmoType,
    pub caliber: Millimeters,
    /// `None` when the shell carries no published figure.
    pub penetration: Option<Millimeters>,
    /// Whether Inertia Fuse for HE Shells raised `penetration`.
    pub ifhe_applied: bool,
    /// `None` when the penetration is unknown.
    pub result: Option<PenResult>,
}

/// One zone a burst reached, with what each compared shell does to it.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneReadout {
    pub zone_name: String,
    pub is_direct_hit: bool,
    pub thickness: Option<Millimeters>,
    pub max_hp: Option<f32>,
    /// Empty when the zone's plating is unknown, since no verdict can be
    /// reached without it.
    pub verdicts: Vec<ShellVerdict>,
}

/// How many armour triangles a burst holds, and how many of those the shell
/// that sized it gets through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BurstTriangles {
    pub in_volume: usize,
    pub penetrated: usize,
}

/// What the splash readout lists for one burst.
#[derive(Clone, Debug, PartialEq)]
pub struct SplashReadout {
    pub half_extent: ModelUnit,
    /// `None` when the burst reached no zone, when no shell sized it, when
    /// that shell has no published penetration, or when no armour triangle
    /// lies inside it: shading plates no zone holds would claim damage the
    /// game does not deal.
    pub triangles: Option<BurstTriangles>,
    pub zones: Vec<ZoneReadout>,
}

/// The readout for `result`: each zone with every HE and SAP shell of
/// `ships` held against its plating, and the burst's triangles counted
/// against `burst_shell`, the shell that sized it.
pub fn splash_readout(
    result: &SplashResult,
    burst_shell: Option<&ShellInfo>,
    ships: &[ComparisonShip],
    ifhe: Ifhe,
) -> SplashReadout {
    let shells: Vec<(&str, &ShellInfo)> = ships
        .iter()
        .flat_map(|ship| {
            ship.shells
                .iter()
                .filter(|shell| matches!(shell.ammo_type, AmmoType::HE | AmmoType::SAP))
                .map(|shell| (ship.display_name.as_str(), shell))
        })
        .collect();

    let zones = result
        .hit_zones
        .iter()
        .map(|zone| {
            let verdicts = match zone.thickness {
                Some(thickness) => shells
                    .iter()
                    .map(|(ship_name, shell)| {
                        let penetration = flat_penetration(shell, ifhe);
                        ShellVerdict {
                            ship_name: (*ship_name).to_owned(),
                            ammo_type: shell.ammo_type.clone(),
                            caliber: shell.caliber,
                            ifhe_applied: penetration.is_some()
                                && ifhe == Ifhe::Applied
                                && shell.ammo_type == AmmoType::HE,
                            penetration,
                            result: check_penetration(shell, thickness, ifhe),
                        }
                    })
                    .collect(),
                None => Vec::new(),
            };
            ZoneReadout {
                zone_name: zone.zone_name.clone(),
                is_direct_hit: zone.is_direct_hit,
                thickness: zone.thickness,
                max_hp: zone.max_hp,
                verdicts,
            }
        })
        .collect();

    let triangles = burst_shell
        .filter(|_| result.reached_anything() && !result.plates_in_volume.is_empty())
        .and_then(|shell| flat_penetration(shell, ifhe))
        .map(|penetration| BurstTriangles {
            in_volume: result.plates_in_volume.len(),
            penetrated: result.plates_in_volume.iter().filter(|plate| penetration >= **plate).count(),
        });

    SplashReadout { half_extent: result.half_extent, triangles, zones }
}

/// The burst volume, drawn as the twelve edges of its box.
pub fn build_splash_cube_mesh(center: [f32; 3], half_extent: ModelUnit, color: [f32; 4]) -> (Vec<Vertex>, Vec<u32>) {
    let reach = half_extent.value();
    let min = [center[0] - reach, center[1] - reach, center[2] - reach];
    let max = [center[0] + reach, center[1] + reach, center[2] + reach];
    build_box_wireframe(min, max, color, EDGE_HALF_WIDTH)
}

/// Where a box's name is drawn, and what it says.
#[derive(Clone, Debug)]
pub struct SplashBoxLabel {
    pub position: [f32; 3],
    pub name: String,
}

/// The outlines of every named box.
pub fn build_splash_box_wireframes<B>(boxes: &[B]) -> (Vec<Vertex>, Vec<u32>)
where
    B: Borrow<SplashBox>,
{
    build_box_wireframes(boxes, BOX_COLOR, EDGE_HALF_WIDTH)
}

/// The outlines of the boxes named in `names`, wider and brighter than their plain outlines, for
/// a hovered row in the splash zone list.
pub fn build_splash_box_highlight(boxes: &[SplashBox], names: &[String]) -> (Vec<Vertex>, Vec<u32>) {
    let named: Vec<&SplashBox> = boxes.iter().filter(|shape| names.contains(&shape.name)).collect();

    build_box_wireframes(&named, BOX_HIGHLIGHT_COLOR, HIGHLIGHT_EDGE_HALF_WIDTH)
}

/// Each box's name as the reader reads it, written at the middle of its top
/// face, which is clear of the hull for every box that is not buried in it.
pub fn box_labels<B>(boxes: &[B]) -> Vec<SplashBoxLabel>
where
    B: Borrow<SplashBox>,
{
    boxes
        .iter()
        .map(|shape| {
            let shape = shape.borrow();
            SplashBoxLabel {
                position: [(shape.min[0] + shape.max[0]) * 0.5, shape.max[1], (shape.min[2] + shape.max[2]) * 0.5],
                name: prettify_box_name(&shape.name),
            }
        })
        .collect()
}

/// Every box's outline in one mesh.
fn build_box_wireframes<B>(boxes: &[B], color: [f32; 4], half_width: f32) -> (Vec<Vertex>, Vec<u32>)
where
    B: Borrow<SplashBox>,
{
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for shape in boxes {
        let shape = shape.borrow();
        let (shape_vertices, shape_indices) = build_box_wireframe(shape.min, shape.max, color, half_width);
        let base = vertices.len() as u32;
        vertices.extend(shape_vertices);
        indices.extend(shape_indices.into_iter().map(|index| index + base));
    }

    (vertices, indices)
}

/// A box drawn as twelve thin quads, one per edge.
fn build_box_wireframe(min: [f32; 3], max: [f32; 3], color: [f32; 4], half_width: f32) -> (Vec<Vertex>, Vec<u32>) {
    let corner = |x: usize, y: usize, z: usize| [[min[0], max[0]][x], [min[1], max[1]][y], [min[2], max[2]][z]];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    // Each edge joins two corners differing in exactly one coordinate.
    for (from, to) in [
        ((0, 0, 0), (1, 0, 0)),
        ((0, 1, 0), (1, 1, 0)),
        ((0, 0, 1), (1, 0, 1)),
        ((0, 1, 1), (1, 1, 1)),
        ((0, 0, 0), (0, 1, 0)),
        ((1, 0, 0), (1, 1, 0)),
        ((0, 0, 1), (0, 1, 1)),
        ((1, 0, 1), (1, 1, 1)),
        ((0, 0, 0), (0, 0, 1)),
        ((1, 0, 0), (1, 0, 1)),
        ((0, 1, 0), (0, 1, 1)),
        ((1, 1, 0), (1, 1, 1)),
    ] {
        push_edge(
            &mut vertices,
            &mut indices,
            corner(from.0, from.1, from.2),
            corner(to.0, to.1, to.2),
            half_width,
            color,
        );
    }
    (vertices, indices)
}

/// One edge, as two crossed quads so it reads from any angle.
fn push_edge(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    from: [f32; 3],
    to: [f32; 3],
    half_width: f32,
    color: [f32; 4],
) {
    let along = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let length = (along[0] * along[0] + along[1] * along[1] + along[2] * along[2]).sqrt();
    if length < 1e-9 {
        return;
    }
    let direction = [along[0] / length, along[1] / length, along[2] / length];
    // Any axis not parallel to the edge will do to get a perpendicular from.
    let reference = if direction[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let across = normalize(cross(direction, reference));
    let up = normalize(cross(direction, across));

    for perpendicular in [across, up] {
        let offset = [perpendicular[0] * half_width, perpendicular[1] * half_width, perpendicular[2] * half_width];
        let base = vertices.len() as u32;
        for (point, sign) in [(from, -1.0), (from, 1.0), (to, 1.0), (to, -1.0)] {
            vertices.push(Vertex {
                position: [point[0] + offset[0] * sign, point[1] + offset[1] * sign, point[2] + offset[2] * sign],
                normal: across,
                color,
                uv: [0.0, 0.0],
                tangent: [0.0; 4],
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length < 1e-9 { [0.0, 0.0, 0.0] } else { [v[0] / length, v[1] / length, v[2] / length] }
}

/// The colour a triangle inside the burst takes.
///
/// An unknown penetration reads as no penetration for colouring: the list
/// beside the viewport is where the uncertainty is stated, and painting it
/// green would claim damage that cannot be shown.
pub fn highlight_color(penetrates: Option<bool>) -> [f32; 4] {
    if penetrates == Some(true) { PEN_COLOR } else { NO_PEN_COLOR }
}

/// The armour triangles inside a burst, shaded by whether `shell` gets through each, lifted off
/// their plates so they do not fight them for depth.
///
/// Empty when the burst reached no splash box, since shading plates the game would deal no damage
/// through would claim damage it does not deal, and when no bursting shell was chosen to hold
/// against them.
pub fn build_burst_highlight(
    meshes: &[InteractiveArmorMesh],
    result: &SplashResult,
    shell: Option<&ShellInfo>,
    ifhe: Ifhe,
) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let Some(shell) = shell.filter(|_| !result.hit_zones.is_empty()) else { return (vertices, indices) };

    let penetration = flat_penetration(shell, ifhe);
    let reach = result.half_extent.value();
    let centre = result.impact_point;
    let burst_min = [centre[0] - reach, centre[1] - reach, centre[2] - reach];
    let burst_max = [centre[0] + reach, centre[1] + reach, centre[2] + reach];
    for mesh in meshes {
        for (triangle, corners) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
            let Some(info) = mesh.triangle_info.get(triangle) else { continue };
            let Some(points) = triangle_corners(mesh, corners) else { continue };
            let middle: [f32; 3] =
                std::array::from_fn(|axis| points.iter().map(|point| point[axis]).sum::<f32>() / 3.0);
            if !point_in_box(middle, burst_min, burst_max) {
                continue;
            }

            let penetrates = penetration.map(|penetration| penetration >= Millimeters::new(info.thickness_mm));
            let color = highlight_color(penetrates);
            let edge = |to: usize| std::array::from_fn::<f32, 3, _>(|axis| points[to][axis] - points[0][axis]);
            let normal = normalize(cross(edge(1), edge(2)));
            let base = vertices.len() as u32;
            for point in points {
                let position = std::array::from_fn(|axis| point[axis] + normal[axis] * HIGHLIGHT_OFFSET);
                vertices.push(Vertex { position, normal, color, uv: [0.0, 0.0], tangent: [0.0; 4] });
            }
            indices.extend_from_slice(&[base, base + 1, base + 2]);
        }
    }

    (vertices, indices)
}

/// A triangle's corners in ship space. `None` when the mesh's own indices do not reach them.
fn triangle_corners(mesh: &InteractiveArmorMesh, corners: &[u32; 3]) -> Option<[[f32; 3]; 3]> {
    let mut points = [[0.0_f32; 3]; 3];
    for (point, &corner) in points.iter_mut().zip(corners) {
        let position = *mesh.positions.get(corner as usize)?;
        *point = match &mesh.transform {
            Some(transform) => transform_point(transform, position),
            None => position,
        };
    }

    Some(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn splash_box(name: &str, min: [f32; 3], max: [f32; 3]) -> SplashBox {
        SplashBox { name: name.to_string(), min, max }
    }

    fn data(boxes: Vec<SplashBox>) -> ShipSplashData {
        ShipSplashData { boxes, box_to_zone: HashMap::new() }
    }

    #[test]
    fn a_burst_inside_a_box_records_it_as_a_direct_hit() {
        let ship = data(vec![splash_box("CM_SB_cit_1", [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0])]);

        let result = compute_splash([0.0, 0.0, 0.0], ModelUnit::new(0.1), &ship, None, &[]);

        assert_eq!(result.direct_hit_box.as_deref(), Some("CM_SB_cit_1"));
        assert!(result.hit_zones[0].is_direct_hit);
    }

    #[test]
    fn a_burst_outside_a_box_still_reaches_it_when_it_overlaps() {
        let ship = data(vec![splash_box("CM_SB_cit_1", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])]);

        // Centred outside the box, but wide enough to reach into it.
        let result = compute_splash([-0.5, 0.5, 0.5], ModelUnit::new(1.0), &ship, None, &[]);

        assert!(result.direct_hit_box.is_none(), "the burst did not start inside");
        assert_eq!(result.hit_zones.len(), 1, "but it reached the box");
        assert!(!result.hit_zones[0].is_direct_hit);
    }

    #[test]
    fn a_burst_that_only_touches_a_face_does_not_reach_inside() {
        let ship = data(vec![splash_box("CM_SB_cit_1", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])]);

        // The burst ends exactly on the near face.
        let result = compute_splash([-1.0, 0.5, 0.5], ModelUnit::new(1.0), &ship, None, &[]);

        assert!(!result.reached_anything());
    }

    #[test]
    fn the_box_a_burst_started_in_is_listed_before_one_it_only_reached() {
        let ship = data(vec![
            // Reached but not entered, and its name sorts first.
            splash_box("CM_SB_aaa_1", [-5.0, -5.0, -5.0], [0.0, 5.0, 5.0]),
            // Entered, and its name sorts last.
            splash_box("CM_SB_zzz_1", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
        ]);

        let result = compute_splash([0.5, 0.5, 0.5], ModelUnit::new(1.0), &ship, None, &[]);

        assert_eq!(result.hit_zones.len(), 2, "one entered, one reached");
        assert_eq!(result.hit_zones[0].box_name, "CM_SB_zzz_1", "the one entered leads despite sorting last");
        assert!(result.hit_zones[0].is_direct_hit);
        assert!(!result.hit_zones[1].is_direct_hit);
    }

    #[test]
    fn the_first_box_in_file_order_wins_when_several_contain_the_point() {
        // Splash boxes nest: a turret sits inside the superstructure. The
        // game takes the first that contains the point, so the order the hull
        // lists them in is what decides, not their size.
        let ship = data(vec![
            splash_box("CM_SB_ss_1", [-5.0, -5.0, -5.0], [5.0, 5.0, 5.0]),
            splash_box("CM_SB_gk_1_1", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
        ]);

        let result = compute_splash([0.5, 0.5, 0.5], ModelUnit::new(0.1), &ship, None, &[]);

        assert_eq!(result.direct_hit_box.as_deref(), Some("CM_SB_ss_1"));
    }

    #[test]
    fn a_zone_with_no_hit_location_reports_an_unknown_thickness_not_a_zero_one() {
        let ship = data(vec![splash_box("CM_SB_cit_1", [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0])]);

        let result = compute_splash([0.0, 0.0, 0.0], ModelUnit::new(0.1), &ship, None, &[]);

        assert!(result.hit_zones[0].thickness.is_none(), "an unnamed zone's plating is unknown, not absent");
        assert!(result.hit_zones[0].max_hp.is_none());
    }

    #[test]
    fn a_box_name_reads_as_its_part_and_index() {
        assert_eq!(prettify_box_name("CM_SB_gk_3_1"), "Turret 3.1");
        assert_eq!(prettify_box_name("CM_SB_cit"), "Citadel");
        assert_eq!(prettify_box_name("CM_SB_ruder_1"), "Steering Gear 1");
        // A part the table does not name keeps its own.
        assert_eq!(prettify_box_name("CM_SB_wibble_2"), "wibble 2");
        // A name with no prefix at all is still read.
        assert_eq!(prettify_box_name("bow"), "Bow");
    }

    #[test]
    fn boxes_group_by_part_and_keep_every_member() {
        let boxes = vec![
            splash_box("CM_SB_gk_1_1", [0.0; 3], [1.0; 3]),
            splash_box("CM_SB_gk_2_1", [0.0; 3], [1.0; 3]),
            splash_box("CM_SB_engine_1", [0.0; 3], [1.0; 3]),
        ];

        let groups = build_splash_box_groups(&boxes);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "Turret");
        assert_eq!(groups[0].1.len(), 2);
        assert_eq!(groups[1].0, "Engine");
    }

    #[test]
    fn a_bigger_shell_bursts_further() {
        let small = splash_half_extent(Millimeters::new(152.0));
        let large = splash_half_extent(Millimeters::new(460.0));

        assert!(large.value() > small.value());
        // The game's own figure: calibre in metres over six.
        assert!((small.value() - 0.152 / 6.0).abs() < 1e-6, "was {}", small.value());
    }

    #[test]
    fn a_box_wireframe_emits_twelve_edges_and_valid_indices() {
        let (vertices, indices) = build_splash_cube_mesh([0.0; 3], ModelUnit::new(1.0), CUBE_COLOR);

        // Twelve edges, two crossed quads each, four vertices per quad.
        assert_eq!(vertices.len(), 12 * 2 * 4);
        assert_eq!(indices.len(), 12 * 2 * 6);
        assert!(indices.iter().all(|index| (*index as usize) < vertices.len()));
    }

    /// The outlines are drawn by the GPU and the names over them by the
    /// overlay, so each box needs a label of its own to be named at all.
    #[test]
    fn every_box_is_named_on_its_top_face_as_the_reader_reads_it() {
        let boxes = [
            splash_box("CM_SB_cit_1", [-2.0, -1.0, -3.0], [2.0, 4.0, 3.0]),
            splash_box("CM_SB_gk_2_1", [0.0; 3], [1.0; 3]),
        ];

        let labels = box_labels(&boxes);

        let named: Vec<(&str, [f32; 3])> = labels.iter().map(|label| (label.name.as_str(), label.position)).collect();
        assert_eq!(named, vec![("Citadel 1", [0.0, 4.0, 0.0]), ("Turret 2.1", [0.5, 1.0, 0.5])]);
    }

    #[test]
    fn wireframes_for_several_boxes_keep_their_indices_within_their_own_vertices() {
        let boxes = [splash_box("a", [0.0; 3], [1.0; 3]), splash_box("b", [2.0, 2.0, 2.0], [3.0, 3.0, 3.0])];

        let (vertices, indices) = build_splash_box_wireframes(&boxes);

        // The second box's indices are offset past the first box's vertices;
        // getting that wrong draws one box's edges onto the other.
        assert!(indices.iter().all(|index| (*index as usize) < vertices.len()));
        assert_eq!(vertices.len(), 2 * 12 * 2 * 4);
    }

    /// A hovered row outlines only the boxes it names, with edges standing further off the box
    /// than its plain outline's so it reads over it.
    #[test]
    fn a_highlight_outlines_only_the_named_boxes_and_stands_out_from_their_outline() {
        let boxes = [splash_box("a", [0.0; 3], [1.0; 3]), splash_box("b", [2.0, 2.0, 2.0], [3.0, 3.0, 3.0])];
        // How far a vertex sits outside box `b`.
        let outside_b = |vertex: &Vertex| {
            vertex.position.iter().map(|axis| (2.0 - axis).max(axis - 3.0).max(0.0)).fold(0.0_f32, f32::max)
        };

        let (highlight, indices) = build_splash_box_highlight(&boxes, &["b".to_string()]);
        let (outline, _) = build_splash_box_wireframes(&boxes[1..]);

        assert_eq!(highlight.len(), outline.len(), "one box's worth of edges");
        assert!(indices.iter().all(|index| (*index as usize) < highlight.len()));
        let reach = |vertices: &[Vertex]| vertices.iter().map(outside_b).fold(0.0_f32, f32::max);
        assert!(reach(&highlight) < 0.5, "nothing is drawn around box a");
        assert!(reach(&highlight) > reach(&outline), "the highlight's edges are wider");
        assert_ne!(highlight[0].color, outline[0].color);
        assert!(build_splash_box_highlight(&boxes, &["c".to_string()]).0.is_empty(), "an unknown name draws nothing");
    }

    #[test]
    fn an_unknown_penetration_is_not_painted_as_one_that_got_through() {
        assert_eq!(highlight_color(Some(true)), PEN_COLOR);
        assert_eq!(highlight_color(Some(false)), NO_PEN_COLOR);
        assert_eq!(highlight_color(None), NO_PEN_COLOR);
    }

    fn shell(ammo_type: AmmoType, caliber: f32, he_pen_mm: Option<f32>, sap_pen_mm: Option<f32>) -> ShellInfo {
        ShellInfo {
            name: "PTEST".to_string(),
            ammo_type,
            caliber: Millimeters::new(caliber),
            he_pen_mm,
            sap_pen_mm,
            alpha_damage: 0.0,
            muzzle_velocity: 0.0,
            mass_kg: 0.0,
            krupp: 0.0,
            ricochet_angle: None,
            always_ricochet_angle: None,
            fuse_time: None,
            fuse_threshold: None,
            burn_prob: None,
            air_drag: 0.0,
            normalization: None,
            cap: true,
        }
    }

    fn ship(name: &str, shells: Vec<ShellInfo>) -> ComparisonShip {
        ComparisonShip {
            param_index: name.to_string(),
            display_name: name.to_string(),
            tier: 10,
            nation: "usa".to_string(),
            species: wowsunpack::game_params::types::Species::Cruiser,
            shells,
        }
    }

    fn hit_location(thickness: f32, max_hp: f32, box_name: &str) -> HitLocation {
        HitLocation::builder()
            .max_hp(max_hp)
            .hl_type("test".to_string())
            .thickness(thickness)
            .splash_boxes(vec![box_name.to_string()])
            .build()
    }

    /// One small triangle per `(centre, thickness)`, the triangle at index
    /// `missing_info` left without a plate record.
    fn plate_mesh(
        plates: &[([f32; 3], f32)],
        transform: Option<[f32; 16]>,
        missing_info: Option<usize>,
    ) -> InteractiveArmorMesh {
        let mut positions = Vec::new();
        let mut triangle_info = Vec::new();
        for (index, &(centre, thickness)) in plates.iter().enumerate() {
            positions.push([centre[0] - 0.01, centre[1] - 0.01, centre[2]]);
            positions.push([centre[0] + 0.02, centre[1] - 0.01, centre[2]]);
            positions.push([centre[0] - 0.01, centre[1] + 0.02, centre[2]]);
            if missing_info != Some(index) {
                triangle_info.push(wowsunpack::export::gltf_export::ArmorTriangleInfo {
                    model_index: 1,
                    triangle_index: index as u32,
                    material_id: 0,
                    material: wowsunpack::game_params::ttx::armor_material::ArmorMaterial::parse("Cit_Belt")
                        .expect("a table key"),
                    zone: "Citadel".to_string(),
                    thickness_mm: thickness,
                    layers: vec![thickness],
                    color: [0.0, 0.0, 0.0, 1.0],
                    hidden: false,
                });
            }
        }
        let count = positions.len();

        InteractiveArmorMesh {
            name: "CM_PA_test".to_string(),
            positions,
            normals: vec![[0.0, 1.0, 0.0]; count],
            indices: (0..count as u32).collect(),
            colors: vec![[0.0, 0.0, 0.0, 1.0]; count],
            triangle_info,
            transform,
        }
    }

    #[test]
    fn bursting_shell_is_the_largest_he_or_sap_shell() {
        let shells = vec![
            shell(AmmoType::AP, 406.0, None, None),
            shell(AmmoType::HE, 127.0, Some(21.0), None),
            shell(AmmoType::SAP, 203.0, None, Some(35.0)),
        ];

        let chosen = bursting_shell(&shells).expect("a bursting shell is carried");

        assert_eq!(chosen.ammo_type, AmmoType::SAP, "the larger AP shell does not burst outside the hull");
        assert!(bursting_shell(&shells[..1]).is_none());
    }

    #[test]
    fn a_turret_plate_is_placed_before_it_is_tested_against_the_burst() {
        let mut moved_by_ten = [0.0_f32; 16];
        for axis in 0..4 {
            moved_by_ten[axis * 5] = 1.0;
        }
        moved_by_ten[12] = 10.0;
        let turret = plate_mesh(&[([0.0, 0.0, 0.0], 76.0)], Some(moved_by_ten), None);
        let hull = plate_mesh(&[([10.0, 0.0, 0.0], 25.0), ([10.0, 0.1, 0.0], 32.0)], None, Some(1));
        let ship = data(vec![splash_box("CM_SB_gk_1_1", [9.0, -1.0, -1.0], [11.0, 1.0, 1.0])]);

        let result = compute_splash([10.0, 0.0, 0.0], ModelUnit::new(0.5), &ship, None, &[turret, hull]);

        // The turret plate counts once moved into the burst; the hull triangle with no plate record
        // does not count at all.
        assert_eq!(result.plates_in_volume, vec![Millimeters::new(76.0), Millimeters::new(25.0)]);
    }

    #[test]
    fn the_readout_holds_each_compared_shell_against_each_zone() {
        let mut ship_data = data(vec![
            splash_box("CM_SB_cit_1", [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]),
            splash_box("CM_SB_ss_1", [1.0, -1.0, -1.0], [3.0, 1.0, 1.0]),
            splash_box("CM_SB_bow_1", [-1.0, 1.0, -1.0], [1.0, 3.0, 1.0]),
        ]);
        let locations = HashMap::from([
            ("Cit".to_string(), hit_location(32.0, 1000.0, "CM_SB_cit_1")),
            ("SS".to_string(), hit_location(13.0, 500.0, "CM_SB_ss_1")),
        ]);
        ship_data.box_to_zone = HashMap::from([
            ("CM_SB_cit_1".to_string(), "Cit".to_string()),
            ("CM_SB_ss_1".to_string(), "SS".to_string()),
        ]);
        let meshes =
            [plate_mesh(&[([0.8, 0.5, 0.0], 19.0), ([1.2, 0.6, 0.0], 50.0), ([5.0, 0.0, 0.0], 10.0)], None, None)];
        let atlanta =
            ship("Atlanta", vec![shell(AmmoType::AP, 127.0, None, None), shell(AmmoType::HE, 127.0, Some(21.0), None)]);
        let mogami = ship("Mogami", vec![shell(AmmoType::SAP, 203.0, None, Some(35.0))]);
        let unknown = ship("Unknown", vec![shell(AmmoType::HE, 152.0, None, None)]);
        let burst_shell = bursting_shell(&atlanta.shells).cloned();
        // Wide enough to enter the citadel box, reach the superstructure and bow boxes beside it, and
        // hold two of the three plates.
        let result = compute_splash([0.8, 0.6, 0.0], ModelUnit::new(0.5), &ship_data, Some(&locations), &meshes);

        let readout = splash_readout(&result, burst_shell.as_ref(), &[atlanta, mogami, unknown], Ifhe::Applied);

        insta::assert_debug_snapshot!(readout);
    }

    #[test]
    fn no_triangles_are_counted_without_a_shell_to_count_them_against() {
        let ship_data = data(vec![splash_box("CM_SB_cit_1", [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0])]);
        let meshes = [plate_mesh(&[([0.0, 0.0, 0.0], 19.0)], None, None)];
        let result = compute_splash([0.0, 0.0, 0.0], ModelUnit::new(0.5), &ship_data, None, &meshes);
        let no_figure = shell(AmmoType::HE, 152.0, None, None);

        assert_eq!(splash_readout(&result, None, &[], Ifhe::NotApplied).triangles, None);
        assert_eq!(splash_readout(&result, Some(&no_figure), &[], Ifhe::NotApplied).triangles, None);
    }

    /// Inside a burst that reached a box, each plate is shaded by whether the shell gets through
    /// it; plates outside the burst, and every plate of a burst that reached no box, are not.
    #[test]
    fn a_burst_shades_the_plates_it_holds_by_whether_the_shell_gets_through() {
        let he = shell(AmmoType::HE, 152.0, Some(25.0), None);
        let meshes =
            [plate_mesh(&[([0.0, 0.0, 0.0], 19.0), ([0.1, 0.0, 0.0], 32.0), ([5.0, 0.0, 0.0], 10.0)], None, None)];
        let boxed = data(vec![splash_box("CM_SB_cit_1", [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0])]);
        let result = compute_splash([0.0, 0.0, 0.0], ModelUnit::new(0.5), &boxed, None, &meshes);

        let (vertices, indices) = build_burst_highlight(&meshes, &result, Some(&he), Ifhe::NotApplied);

        let colors: Vec<[f32; 4]> = vertices.chunks(3).map(|corners| corners[0].color).collect();
        assert_eq!(colors, [PEN_COLOR, NO_PEN_COLOR], "19 mm is penetrated, 32 mm is not, 10 mm is outside");
        assert_eq!(indices.len(), 6);

        let unboxed = compute_splash([0.0, 0.0, 0.0], ModelUnit::new(0.5), &data(Vec::new()), None, &meshes);
        assert!(build_burst_highlight(&meshes, &unboxed, Some(&he), Ifhe::NotApplied).0.is_empty());
        assert!(build_burst_highlight(&meshes, &result, None, Ifhe::NotApplied).0.is_empty());
    }
}
