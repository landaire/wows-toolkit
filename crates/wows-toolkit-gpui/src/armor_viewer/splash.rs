//! Splash volumes: the box a high-explosive or semi-armour-piercing shell
//! bursts in, and the parts of the ship that box reaches.
//!
//! A shell that does not penetrate still damages what its burst overlaps. The
//! game sizes that burst from the shell's own calibre and tests it against
//! named boxes shipped with the hull, which is what this reproduces: the
//! boxes come from `wowsunpack::models::geometry::parse_splash_file` and the
//! zones they belong to from GameParams' own hit locations, so the egui
//! viewer and this one answer from the same data.

use std::collections::HashMap;
use std::collections::HashSet;

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

/// Half-width of a drawn wireframe edge, in model units.
const EDGE_HALF_WIDTH: f32 = 0.003;

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
    /// How many armour triangles fall inside the burst, and how many of those
    /// the shell gets through. Both zero until a shell is shaded against it.
    pub triangles_in_volume: usize,
    pub triangles_penetrated: usize,
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
            thickness: location.map(|found| Millimeters::new(found.thickness())),
            max_hp: location.map(|found| found.max_hp()),
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
        triangles_in_volume: 0,
        triangles_penetrated: 0,
    }
}

/// How much armour a shell's burst gets through.
///
/// `None` when the shell is neither high-explosive nor semi-armour-piercing,
/// and when the one it is carries no published figure: an absent figure is
/// not zero penetration, and reporting it as zero would say the shell bounces
/// off everything.
pub fn shell_pen_mm(shell: &ShellInfo, ifhe: bool) -> Option<Millimeters> {
    let raw = match shell.ammo_type {
        AmmoType::HE => shell.he_pen_mm.map(|base| if ifhe { base * 1.25 } else { base }),
        AmmoType::SAP => shell.sap_pen_mm,
        _ => None,
    }?;
    Some(Millimeters::new(raw))
}

/// Whether a shell's burst gets through `thickness`.
///
/// `None` when the shell's penetration is unknown, so a caller can say so
/// rather than show a verdict it cannot support.
pub fn shell_penetrates(shell: &ShellInfo, thickness: Millimeters, ifhe: bool) -> Option<bool> {
    Some(shell_pen_mm(shell, ifhe)?.value() >= thickness.value())
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

/// The outlines of every named box, with somewhere to write each name.
///
/// The label sits at the top face's middle, which is clear of the hull for
/// every box that is not buried in it.
pub fn build_splash_box_wireframes(boxes: &[&SplashBox]) -> (Vec<Vertex>, Vec<u32>, Vec<SplashBoxLabel>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut labels = Vec::new();
    for shape in boxes {
        let (shape_vertices, shape_indices) = build_box_wireframe(shape.min, shape.max, BOX_COLOR, EDGE_HALF_WIDTH);
        let base = vertices.len() as u32;
        vertices.extend(shape_vertices);
        indices.extend(shape_indices.into_iter().map(|index| index + base));
        labels.push(SplashBoxLabel {
            position: [(shape.min[0] + shape.max[0]) * 0.5, shape.max[1], (shape.min[2] + shape.max[2]) * 0.5],
            name: shape.name.clone(),
        });
    }
    (vertices, indices, labels)
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

/// How far a shaded triangle is lifted off its plate.
pub fn highlight_offset() -> f32 {
    HIGHLIGHT_OFFSET
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

        let result = compute_splash([0.0, 0.0, 0.0], ModelUnit::new(0.1), &ship, None);

        assert_eq!(result.direct_hit_box.as_deref(), Some("CM_SB_cit_1"));
        assert!(result.hit_zones[0].is_direct_hit);
    }

    #[test]
    fn a_burst_outside_a_box_still_reaches_it_when_it_overlaps() {
        let ship = data(vec![splash_box("CM_SB_cit_1", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])]);

        // Centred outside the box, but wide enough to reach into it.
        let result = compute_splash([-0.5, 0.5, 0.5], ModelUnit::new(1.0), &ship, None);

        assert!(result.direct_hit_box.is_none(), "the burst did not start inside");
        assert_eq!(result.hit_zones.len(), 1, "but it reached the box");
        assert!(!result.hit_zones[0].is_direct_hit);
    }

    #[test]
    fn a_burst_that_only_touches_a_face_does_not_reach_inside() {
        let ship = data(vec![splash_box("CM_SB_cit_1", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])]);

        // The burst ends exactly on the near face.
        let result = compute_splash([-1.0, 0.5, 0.5], ModelUnit::new(1.0), &ship, None);

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

        let result = compute_splash([0.5, 0.5, 0.5], ModelUnit::new(1.0), &ship, None);

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

        let result = compute_splash([0.5, 0.5, 0.5], ModelUnit::new(0.1), &ship, None);

        assert_eq!(result.direct_hit_box.as_deref(), Some("CM_SB_ss_1"));
    }

    #[test]
    fn a_zone_with_no_hit_location_reports_an_unknown_thickness_not_a_zero_one() {
        let ship = data(vec![splash_box("CM_SB_cit_1", [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0])]);

        let result = compute_splash([0.0, 0.0, 0.0], ModelUnit::new(0.1), &ship, None);

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

    #[test]
    fn every_boxs_label_sits_on_its_top_face() {
        let boxes = [splash_box("CM_SB_cit_1", [-2.0, -1.0, -3.0], [2.0, 4.0, 3.0])];
        let refs: Vec<&SplashBox> = boxes.iter().collect();

        let (_, _, labels) = build_splash_box_wireframes(&refs);

        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].position, [0.0, 4.0, 0.0]);
        // The raw name is the identity; prettifying is the reader's view of it.
        assert_eq!(labels[0].name, "CM_SB_cit_1");
    }

    #[test]
    fn wireframes_for_several_boxes_keep_their_indices_within_their_own_vertices() {
        let boxes = [splash_box("a", [0.0; 3], [1.0; 3]), splash_box("b", [2.0, 2.0, 2.0], [3.0, 3.0, 3.0])];
        let refs: Vec<&SplashBox> = boxes.iter().collect();

        let (vertices, indices, _) = build_splash_box_wireframes(&refs);

        // The second box's indices are offset past the first box's vertices;
        // getting that wrong draws one box's edges onto the other.
        assert!(indices.iter().all(|index| (*index as usize) < vertices.len()));
        assert_eq!(vertices.len(), 2 * 12 * 2 * 4);
    }

    #[test]
    fn an_unknown_penetration_is_not_painted_as_one_that_got_through() {
        assert_eq!(highlight_color(Some(true)), PEN_COLOR);
        assert_eq!(highlight_color(Some(false)), NO_PEN_COLOR);
        assert_eq!(highlight_color(None), NO_PEN_COLOR);
    }
}
