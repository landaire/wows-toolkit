use std::collections::HashMap;

use wowsunpack::export::gltf_export::InteractiveArmorMesh;
use wowsunpack::game_params::types::{AmmoType, HitLocation, Millimeters, ShellInfo};
use wowsunpack::models::geometry::SplashBox;

use crate::viewport_3d::types::Vertex;

// ─── Model-Space Unit ────────────────────────────────────────────────────────

/// A scalar distance in the 3D model's local coordinate system.
///
/// Model-space coordinates are used by armor meshes, splash boxes, and the
/// viewport picking system. They do **not** have a fixed relationship to
/// real-world meters — the scale depends on the asset pipeline. The game's
/// splash functions receive values derived from `bulletDiametr` (meters)
/// directly in this space (e.g. `bulletDiametr / 6.0`), so the numeric
/// value of a `Millimeters` quantity divided by 1000 can be used as-is.
///
/// This type exists to prevent accidental mixing with [`Meters`],
/// [`BigWorldDistance`], or [`Km`].
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct ModelUnit(f32);

impl ModelUnit {
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

impl From<f32> for ModelUnit {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

impl std::ops::Mul<f32> for ModelUnit {
    type Output = ModelUnit;
    fn mul(self, rhs: f32) -> ModelUnit {
        ModelUnit(self.0 * rhs)
    }
}

impl std::ops::Div<f32> for ModelUnit {
    type Output = ModelUnit;
    fn div(self, rhs: f32) -> ModelUnit {
        ModelUnit(self.0 / rhs)
    }
}

// ─── Transform Helpers ───────────────────────────────────────────────────────

/// Apply a column-major 4x4 transform to a position.
fn transform_point(t: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        t[0] * p[0] + t[4] * p[1] + t[8] * p[2] + t[12],
        t[1] * p[0] + t[5] * p[1] + t[9] * p[2] + t[13],
        t[2] * p[0] + t[6] * p[1] + t[10] * p[2] + t[14],
    ]
}

/// Apply the upper-left 3x3 of a column-major 4x4 transform to a normal and renormalize.
fn transform_normal(t: &[f32; 16], n: [f32; 3]) -> [f32; 3] {
    let x = t[0] * n[0] + t[4] * n[1] + t[8] * n[2];
    let y = t[1] * n[0] + t[5] * n[1] + t[9] * n[2];
    let z = t[2] * n[0] + t[6] * n[1] + t[10] * n[2];
    let len = (x * x + y * y + z * z).sqrt();
    if len < 1e-10 {
        return [0.0, 0.0, 0.0];
    }
    [x / len, y / len, z / len]
}

// ─── Splash Colors ───────────────────────────────────────────────────────────

/// Color for armor triangles the HE shell can penetrate (green, semi-transparent).
pub const SPLASH_PEN_COLOR: [f32; 4] = [0.2, 0.9, 0.2, 0.55];

/// Color for armor triangles the HE shell cannot penetrate (red, semi-transparent).
pub const SPLASH_NO_PEN_COLOR: [f32; 4] = [0.9, 0.2, 0.2, 0.55];

/// Color for the splash cube wireframe.
pub const SPLASH_CUBE_COLOR: [f32; 4] = [1.0, 0.7, 0.1, 0.7];

/// Half-width of wireframe cube edges in world-space units.
const CUBE_EDGE_HALF_WIDTH: f32 = 0.003;

// ─── Data Structures ─────────────────────────────────────────────────────────

/// Parsed splash box data for a loaded ship.
pub struct ShipSplashData {
    /// Named AABBs from the `.splash` file.
    pub boxes: Vec<SplashBox>,
    /// Zone name → list of splash box names that belong to it.
    pub zone_box_mapping: HashMap<String, Vec<String>>,
    /// Reverse: splash box name → zone name.
    pub box_to_zone: HashMap<String, String>,
}

/// Result of a splash analysis at a given impact point.
///
/// This is shell-independent: it records which zones the splash volume
/// overlaps. Penetration checks are done per-shell in the UI layer.
pub struct SplashResult {
    pub impact_point: [f32; 3],
    /// Splash cube half-extent in model-space units (uniform on all axes).
    pub half_extent: ModelUnit,
    /// The splash box that directly contains the impact point (if any).
    pub direct_hit_box: Option<String>,
    /// All zones whose splash boxes overlap the splash cube or contain the point.
    pub hit_zones: Vec<SplashZoneHit>,
    /// Number of armor triangles inside the splash cube.
    pub triangles_in_volume: usize,
    /// Number of those triangles that are penetrated (set per-shell later).
    pub triangles_penetrated: usize,
}

/// A zone/component hit by the splash cube.
#[derive(Clone, Debug)]
pub struct SplashZoneHit {
    /// Human-readable zone name (from HitLocation, or prettified box name).
    pub zone_name: String,
    /// Raw splash box name from the `.splash` file.
    pub box_name: String,
    /// Default zone plating thickness from HitLocation.
    pub thickness: Millimeters,
    /// Zone max HP.
    pub max_hp: f32,
    /// Whether this is the box that directly contains the impact point.
    pub is_direct_hit: bool,
}

// ─── Splash Data Loading ─────────────────────────────────────────────────────

/// Parse splash box data from a ShipModelContext during ship loading.
///
/// Returns `None` if no splash file is available.
pub fn parse_ship_splash_data(
    splash_bytes: Option<&[u8]>,
    hit_locations: Option<&HashMap<String, HitLocation>>,
) -> Option<ShipSplashData> {
    let bytes = splash_bytes?;
    let boxes = wowsunpack::models::geometry::parse_splash_file(bytes).ok()?;

    let mut zone_box_mapping: HashMap<String, Vec<String>> = HashMap::new();
    let mut box_to_zone: HashMap<String, String> = HashMap::new();

    if let Some(hit_locs) = hit_locations {
        for (zone_name, hl) in hit_locs {
            let box_names: Vec<String> = hl.splash_boxes().iter().cloned().collect();
            for bname in &box_names {
                box_to_zone.insert(bname.clone(), zone_name.clone());
            }
            zone_box_mapping.insert(zone_name.clone(), box_names);
        }
    }

    Some(ShipSplashData { boxes, zone_box_mapping, box_to_zone })
}

// ─── Splash Computation ──────────────────────────────────────────────────────

/// Compute the HE splash cube half-extent from shell caliber.
///
/// The game passes `bulletDiametr / 6.0` (in meters) as the splash half-extent
/// to `getSplashEffectiveArmor`. The splash boxes are in the same model-local
/// coordinate system, and empirically the numeric value of
/// `bulletDiametr_m / 6.0` maps directly into model-space coordinates.
pub fn splash_half_extent(caliber: Millimeters) -> ModelUnit {
    ModelUnit::new((caliber / 6.0).to_meters().value())
}

/// Produce a human-readable label from a splash box name.
///
/// Game box names follow the pattern `XX_SB_<type>_<index>_<sub>`.
/// We strip the prefix and map known abbreviations to readable names.
pub fn prettify_box_name(box_name: &str) -> String {
    // Strip the "XX_SB_" prefix (e.g. "CM_SB_gk_3_1" → "gk_3_1")
    let stripped = box_name.find("_SB_").map(|i| &box_name[i + 4..]).unwrap_or(box_name);

    // Extract the type part (before the first digit segment)
    let type_part =
        stripped.split('_').take_while(|s| s.chars().all(|c| c.is_alphabetic())).collect::<Vec<_>>().join("_");

    let label = match type_part.as_str() {
        "gk" => "Turret",
        "engine" => "Engine",
        "bow" => "Bow",
        "stern" => "Stern",
        "cit" => "Citadel",
        "ss" => "Superstructure",
        "ssc" => "Superstructure (casemate)",
        "ruder" => "Steering Gear",
        other => other,
    };

    // Append the index if present
    let rest: Vec<&str> = stripped.split('_').skip_while(|s| s.chars().all(|c| c.is_alphabetic())).collect();
    if rest.is_empty() { label.to_string() } else { format!("{} {}", label, rest.join(".")) }
}

/// Test whether two AABBs overlap (strictly).
fn aabb_overlap(a_min: [f32; 3], a_max: [f32; 3], b_min: [f32; 3], b_max: [f32; 3]) -> bool {
    a_max[0] > b_min[0]
        && a_min[0] < b_max[0]
        && a_max[1] > b_min[1]
        && a_min[1] < b_max[1]
        && a_max[2] > b_min[2]
        && a_min[2] < b_max[2]
}

/// Test whether a point is inside an AABB.
fn point_in_aabb(p: [f32; 3], aabb_min: [f32; 3], aabb_max: [f32; 3]) -> bool {
    p[0] >= aabb_min[0]
        && p[0] <= aabb_max[0]
        && p[1] >= aabb_min[1]
        && p[1] <= aabb_max[1]
        && p[2] >= aabb_min[2]
        && p[2] <= aabb_max[2]
}

/// Compute which splash boxes and zones are hit by the splash volume.
///
/// This is **shell-independent**: it identifies the containing box (direct hit)
/// and all overlapping boxes, recording their zone thickness and HP. Penetration
/// checks are done per-shell in the UI.
pub fn compute_splash(
    impact_point: [f32; 3],
    half_extent: ModelUnit,
    splash_data: &ShipSplashData,
    hit_locations: Option<&HashMap<String, HitLocation>>,
) -> SplashResult {
    let he = half_extent.value();
    let splash_min = [impact_point[0] - he, impact_point[1] - he, impact_point[2] - he];
    let splash_max = [impact_point[0] + he, impact_point[1] + he, impact_point[2] + he];

    // First, find which box directly contains the impact point
    let direct_hit_box = splash_data
        .boxes
        .iter()
        .find(|sbox| point_in_aabb(impact_point, sbox.min, sbox.max))
        .map(|sbox| sbox.name.clone());

    let mut hit_zones = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();

    for sbox in &splash_data.boxes {
        // Include boxes that either contain the point OR overlap the splash cube
        let contains_point = point_in_aabb(impact_point, sbox.min, sbox.max);
        let overlaps_cube = aabb_overlap(splash_min, splash_max, sbox.min, sbox.max);

        if contains_point || overlaps_cube {
            if !seen.insert(sbox.name.clone()) {
                continue;
            }

            // Resolve zone name: HitLocation mapping first, then prettified box name
            let zone_name =
                splash_data.box_to_zone.get(&sbox.name).cloned().unwrap_or_else(|| prettify_box_name(&sbox.name));

            let (thickness, max_hp) = hit_locations
                .and_then(|hls| {
                    hls.get(&zone_name).or_else(|| {
                        // Also try the raw zone name from box_to_zone
                        splash_data.box_to_zone.get(&sbox.name).and_then(|zn| hls.get(zn))
                    })
                })
                .map(|hl| (Millimeters::new(hl.thickness()), hl.max_hp()))
                .unwrap_or((Millimeters::new(0.0), 0.0));

            let is_direct_hit = direct_hit_box.as_ref() == Some(&sbox.name);

            hit_zones.push(SplashZoneHit { zone_name, box_name: sbox.name.clone(), thickness, max_hp, is_direct_hit });
        }
    }

    // Sort: direct hit first, then by zone name
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

/// Check whether a shell penetrates a given thickness.
pub fn shell_penetrates(shell: &ShellInfo, thickness: Millimeters, ifhe: bool) -> bool {
    let pen = match shell.ammo_type {
        AmmoType::HE => {
            let base = shell.he_pen_mm.unwrap_or(0.0);
            if ifhe { base * 1.25 } else { base }
        }
        AmmoType::SAP => shell.sap_pen_mm.unwrap_or(0.0),
        _ => return false,
    };
    pen >= thickness.value()
}

/// Get the effective penetration value for a shell.
pub fn shell_pen_mm(shell: &ShellInfo, ifhe: bool) -> f32 {
    match shell.ammo_type {
        AmmoType::HE => {
            let base = shell.he_pen_mm.unwrap_or(0.0);
            if ifhe { base * 1.25 } else { base }
        }
        AmmoType::SAP => shell.sap_pen_mm.unwrap_or(0.0),
        _ => 0.0,
    }
}

// ─── Mesh Generation ─────────────────────────────────────────────────────────

/// Build a wireframe cube mesh for the splash volume visualization.
///
/// Generates 12 edges as thin quads (24 triangles total).
pub fn build_splash_cube_mesh(center: [f32; 3], half_extent: ModelUnit, color: [f32; 4]) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    let he = half_extent.value();
    let lo = [center[0] - he, center[1] - he, center[2] - he];
    let hi = [center[0] + he, center[1] + he, center[2] + he];

    // 8 corners of the cube
    let corners: [[f32; 3]; 8] = [
        [lo[0], lo[1], lo[2]], // 0: ---
        [hi[0], lo[1], lo[2]], // 1: +--
        [hi[0], hi[1], lo[2]], // 2: ++-
        [lo[0], hi[1], lo[2]], // 3: -+-
        [lo[0], lo[1], hi[2]], // 4: --+
        [hi[0], lo[1], hi[2]], // 5: +-+
        [hi[0], hi[1], hi[2]], // 6: +++
        [lo[0], hi[1], hi[2]], // 7: -++
    ];

    // 12 edges of the cube (pairs of corner indices)
    let edges: [(usize, usize); 12] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0), // bottom face
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4), // top face
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7), // verticals
    ];

    let w = CUBE_EDGE_HALF_WIDTH;

    for &(a, b) in &edges {
        let pa = corners[a];
        let pb = corners[b];

        // Edge direction
        let dx = pb[0] - pa[0];
        let dy = pb[1] - pa[1];
        let dz = pb[2] - pa[2];

        // Pick a perpendicular direction for quad width
        // Use the axis least aligned with the edge direction
        let (perp_x, perp_y, perp_z) = {
            let ax = dx.abs();
            let ay = dy.abs();
            let az = dz.abs();
            if ax <= ay && ax <= az {
                let cy = -dz;
                let cz = dy;
                let len = (cy * cy + cz * cz).sqrt().max(1e-10);
                (0.0, cy / len * w, cz / len * w)
            } else if ay <= az {
                let cx = dz;
                let cz = -dx;
                let len = (cx * cx + cz * cz).sqrt().max(1e-10);
                (cx / len * w, 0.0, cz / len * w)
            } else {
                let cx = -dy;
                let cy = dx;
                let len = (cx * cx + cy * cy).sqrt().max(1e-10);
                (cx / len * w, cy / len * w, 0.0)
            }
        };

        let normal = [0.0, 1.0, 0.0]; // dummy normal for overlay

        let base = vertices.len() as u32;
        vertices.push(Vertex { position: [pa[0] - perp_x, pa[1] - perp_y, pa[2] - perp_z], normal, color });
        vertices.push(Vertex { position: [pa[0] + perp_x, pa[1] + perp_y, pa[2] + perp_z], normal, color });
        vertices.push(Vertex { position: [pb[0] - perp_x, pb[1] - perp_y, pb[2] - perp_z], normal, color });
        vertices.push(Vertex { position: [pb[0] + perp_x, pb[1] + perp_y, pb[2] + perp_z], normal, color });

        indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }

    (vertices, indices)
}

/// Build a highlight mesh for armor triangles inside the splash volume.
///
/// Each triangle is colored by whether the HE shell penetrates its thickness:
/// green = penetrates, red = does not penetrate.
///
/// Returns `(vertices, indices, triangles_total, triangles_penetrated)`.
pub fn build_splash_highlight_mesh(
    armor_meshes: &[InteractiveArmorMesh],
    impact_point: [f32; 3],
    half_extent: ModelUnit,
    shell: &ShellInfo,
    ifhe: bool,
) -> (Vec<Vertex>, Vec<u32>, usize, usize) {
    let he = half_extent.value();
    let splash_min = [impact_point[0] - he, impact_point[1] - he, impact_point[2] - he];
    let splash_max = [impact_point[0] + he, impact_point[1] + he, impact_point[2] + he];

    let pen_mm = shell_pen_mm(shell, ifhe);

    let normal_offset = 0.006; // slight offset to avoid z-fighting
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut total = 0usize;
    let mut penetrated = 0usize;

    for mesh in armor_meshes {
        let tri_count = mesh.indices.len() / 3;
        for tri_idx in 0..tri_count {
            let i0 = mesh.indices[tri_idx * 3] as usize;
            let i1 = mesh.indices[tri_idx * 3 + 1] as usize;
            let i2 = mesh.indices[tri_idx * 3 + 2] as usize;

            let mut p0 = mesh.positions[i0];
            let mut p1 = mesh.positions[i1];
            let mut p2 = mesh.positions[i2];
            let mut n0 = mesh.normals[i0];
            let mut n1 = mesh.normals[i1];
            let mut n2 = mesh.normals[i2];

            // Apply turret transform if present
            if let Some(t) = &mesh.transform {
                p0 = transform_point(t, p0);
                p1 = transform_point(t, p1);
                p2 = transform_point(t, p2);
                n0 = transform_normal(t, n0);
                n1 = transform_normal(t, n1);
                n2 = transform_normal(t, n2);
            }

            // Compute centroid
            let centroid =
                [(p0[0] + p1[0] + p2[0]) / 3.0, (p0[1] + p1[1] + p2[1]) / 3.0, (p0[2] + p1[2] + p2[2]) / 3.0];

            if !point_in_aabb(centroid, splash_min, splash_max) {
                continue;
            }

            total += 1;

            // Get this triangle's thickness
            let thickness_mm = mesh.triangle_info.get(tri_idx).map(|ti| ti.thickness_mm).unwrap_or(0.0);
            let pen = pen_mm >= thickness_mm;
            if pen {
                penetrated += 1;
            }

            let color = if pen { SPLASH_PEN_COLOR } else { SPLASH_NO_PEN_COLOR };

            // Offset vertices slightly along their normals
            let base = vertices.len() as u32;
            vertices.push(Vertex {
                position: [p0[0] + n0[0] * normal_offset, p0[1] + n0[1] * normal_offset, p0[2] + n0[2] * normal_offset],
                normal: n0,
                color,
            });
            vertices.push(Vertex {
                position: [p1[0] + n1[0] * normal_offset, p1[1] + n1[1] * normal_offset, p1[2] + n1[2] * normal_offset],
                normal: n1,
                color,
            });
            vertices.push(Vertex {
                position: [p2[0] + n2[0] * normal_offset, p2[1] + n2[1] * normal_offset, p2[2] + n2[2] * normal_offset],
                normal: n2,
                color,
            });
            indices.extend_from_slice(&[base, base + 1, base + 2]);
        }
    }

    (vertices, indices, total, penetrated)
}
