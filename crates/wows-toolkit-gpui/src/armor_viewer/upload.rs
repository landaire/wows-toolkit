//! Uploads a [`LoadedShipArmor`] into a [`Viewport3D`]. Ports the egui app's
//! `upload_armor_to_viewport` (`armor_viewer/ui/tab.rs:1147-1335`), the
//! camera-framing half of `init_armor_viewport` (`tab.rs:1445-1495`), and (as
//! of Milestone 3 Task 6) the plain-triangle-visibility half of `pane.
//! plate_visibility` filtering the egui original applies at the same call
//! sites (`tab.rs:1178-1184`, `4025-4028`). [`upload_armor_to_viewport`] is
//! the initial-load path (uploads + frames the camera, matching
//! `init_armor_viewport`); [`reupload_armor_plates`] is the click-to-hide
//! re-upload path added by that same task -- it shares the mesh-building
//! logic via [`upload_armor_meshes`] but leaves the camera untouched, so
//! toggling a plate's visibility does not reset the user's view.
//!
//! **v1 scope.** Armor meshes only -- hull meshes are uploaded separately by
//! `upload_hull.rs` (Milestone 4 Task 8a), which shares this module's
//! `Viewport3D` but tracks its own mesh id list so a hull-only change never
//! runs this module's (comparatively expensive) armor rebuild. Camo and the
//! part-level display toggles are Milestone 4 Task 8b/8c. Camera-orbit-ellipse
//! and ship-center overlays are likewise out of scope (their toggles do not
//! exist yet). `show_hidden_only` is still a fixed v1 default (no pane UI for
//! it yet). [`DisplaySettings`] (Task 7b's display-settings popover,
//! `popover.rs`) makes `show_plate_edges`/`show_waterline`/`waterline_opacity`/
//! `show_zero_mm`/`armor_opacity` user-mutable, each through
//! `ViewportView::mutate_display_settings` (re-uploads armor); `hull_opaque`
//! (Task 8a) lives on the same struct for persistence symmetry with
//! `ArmorViewerDefaultsRow`, but is mutated through the hull-only
//! `ViewportView::set_hull_opaque` instead, so toggling it never triggers this
//! module's armor rebuild. `part_visibility` and `plate_visibility`
//! (Milestone 3 Task 7's armor-visibility popover) are likewise both
//! user-mutable.

use std::collections::HashMap;

use wowsunpack::export::gltf_export::ArmorTriangleInfo;

use wows_toolkit_config::queries::ArmorViewerDefaultsRow;

use crate::viewport::camera::ArcballCamera;
use crate::viewport::renderer::LAYER_DEFAULT;
use crate::viewport::renderer::LAYER_HULL;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::MeshId;
use crate::viewport::types::Vertex;

use super::load_ship::ArmorTriangleTooltip;
use super::load_ship::LoadedShipArmor;
use super::load_ship::PlateKey;
use super::load_ship::transform_normal;
use super::load_ship::transform_point;
use super::visibility::VisibilityFilter;
use super::visibility::part_on;

/// The v1 display-settings subset (Task 7b's display-settings popover,
/// `popover.rs`): plate edges, waterline (+ its own opacity), zero-mm plate
/// filtering, and per-vertex armor opacity. Threaded through every mesh
/// upload alongside [`VisibilityFilter`]; changing any field re-uploads the
/// armor since all of them affect geometry (edge outlines, the water plane,
/// per-vertex alpha, and which triangles are even included).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplaySettings {
    pub show_plate_edges: bool,
    pub show_waterline: bool,
    pub waterline_opacity: f32,
    pub show_zero_mm: bool,
    pub armor_opacity: f32,
    /// Whether hull meshes render fully opaque with depth writes (like armor
    /// plates) rather than semi-transparent behind them. Task 8's hull upload
    /// (`upload_hull.rs`) reads this for `hull_alpha`/`hull_layer`; mutated
    /// through `ViewportView::set_hull_opaque` rather than
    /// `mutate_display_settings`, since a hull-only change should re-upload
    /// only the hull, not the (potentially much larger) armor mesh set.
    pub hull_opaque: bool,
}

impl Default for DisplaySettings {
    /// `ArmorViewerDefaults::default()` (`armor_viewer/state.rs:96-112`).
    fn default() -> Self {
        Self {
            show_plate_edges: true,
            show_waterline: true,
            waterline_opacity: 0.3,
            show_zero_mm: false,
            armor_opacity: 1.0,
            hull_opaque: false,
        }
    }
}

impl DisplaySettings {
    /// Seeds from the persisted `armor_viewer_defaults` row, falling back to
    /// the egui defaults above when there is no row yet (fresh DB). Mirrors
    /// `legend::LegendState::from_defaults`'s same fallback shape.
    pub fn from_defaults(row: Option<&ArmorViewerDefaultsRow>) -> Self {
        let Some(row) = row else { return Self::default() };
        Self {
            show_plate_edges: row.show_plate_edges,
            show_waterline: row.show_waterline,
            waterline_opacity: row.waterline_opacity as f32,
            show_zero_mm: row.show_zero_mm,
            armor_opacity: row.armor_opacity as f32,
            hull_opaque: row.hull_opaque,
        }
    }
}

/// `armor_viewer::constants::PLATE_EDGE_HALF_WIDTH`.
const PLATE_EDGE_HALF_WIDTH: f32 = 0.003;
/// `armor_viewer::constants::PLATE_EDGE_NORMAL_OFFSET`.
const PLATE_EDGE_NORMAL_OFFSET: f32 = 0.005;
/// `armor_viewer::constants::EDGE_COLOR`.
const EDGE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Skip an armor triangle whose thickness rounds to (effectively) 0mm, unless
/// `show_zero_mm` is on. Matches the egui app's own `0.05` epsilon.
pub(crate) fn is_zero_mm(thickness_mm: f32) -> bool {
    thickness_mm.abs() < 0.05
}

/// Derives a triangle's [`PlateKey`] (zone, material_name, thickness rounded
/// to tenths of mm). Matches the egui app's own inline key derivation at
/// every `plate_visibility` check site (`tab.rs:1180`, `2911`, `4025`, ...).
pub(crate) fn derive_plate_key(info: &ArmorTriangleInfo) -> PlateKey {
    (info.zone.clone(), info.material_name.clone(), (info.thickness_mm * 10.0).round() as i32)
}

/// Whether triangle `info` should be included in an upload: not (effectively)
/// 0mm (unless `show_zero_mm`), its `(zone, material)` part is on in
/// `visibility.part` (absent means visible), and its plate is not in
/// `visibility.plate` (which stores only explicitly-hidden plate keys --
/// absent means visible; see `visibility.rs`'s module doc for why the two
/// maps use opposite boolean senses). Shared by the main mesh loop, plate
/// boundary edges, and the hover/sidebar highlights (`picking_ui.rs`) so all
/// of them treat a hidden part/plate identically.
pub(crate) fn plate_is_visible(info: &ArmorTriangleInfo, visibility: VisibilityFilter, show_zero_mm: bool) -> bool {
    if !show_zero_mm && is_zero_mm(info.thickness_mm) {
        return false;
    }
    if !part_on(visibility.part, &info.zone, &info.material_name) {
        return false;
    }
    !visibility.plate.get(&derive_plate_key(info)).copied().unwrap_or(false)
}

/// Clears `viewport` and uploads `armor`'s armor meshes (thickness-colored,
/// per the v1 fixed display defaults above, honoring `plate_visibility`),
/// plate boundary edges, and the waterline plane. Does not touch the camera;
/// shared by [`upload_armor_to_viewport`] (initial load, frames the camera
/// afterward) and [`reupload_armor_plates`] (visibility-toggle re-upload,
/// which must NOT move the camera). Returns the per-mesh triangle tooltip
/// data for hover/click picking (`picking_ui.rs`).
fn upload_armor_meshes(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    armor: &LoadedShipArmor,
    visibility: VisibilityFilter,
    display: DisplaySettings,
) -> Vec<(MeshId, Vec<ArmorTriangleTooltip>)> {
    viewport.clear();

    let mut mesh_triangle_info = Vec::new();
    for mesh in &armor.meshes {
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut tooltips: Vec<ArmorTriangleTooltip> = Vec::new();

        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            if !plate_is_visible(info, visibility, display.show_zero_mm) {
                continue;
            }

            let base_idx = tri_idx * 3;
            if base_idx + 2 >= mesh.indices.len() {
                continue;
            }

            let new_base = vertices.len() as u32;
            for k in 0..3 {
                let orig_idx = mesh.indices[base_idx + k] as usize;
                if orig_idx < mesh.positions.len() {
                    let mut pos = mesh.positions[orig_idx];
                    let mut norm = mesh.normals[orig_idx];

                    if let Some(t) = &mesh.transform {
                        pos = transform_point(t, pos);
                        norm = transform_normal(t, norm);
                    }

                    let mut color = mesh.colors[orig_idx];
                    color[3] = display.armor_opacity;
                    vertices.push(Vertex { position: pos, normal: norm, color, uv: [0.0, 0.0] });
                }
            }
            indices.extend_from_slice(&[new_base, new_base + 1, new_base + 2]);

            tooltips.push(ArmorTriangleTooltip {
                material_name: info.material_name.clone(),
                zone: info.zone.clone(),
                thickness_mm: info.thickness_mm,
                layers: info.layers.clone(),
                color: info.color,
            });
        }

        if !indices.is_empty() {
            let mesh_id = viewport.add_mesh(device, &vertices, &indices, LAYER_DEFAULT);
            mesh_triangle_info.push((mesh_id, tooltips));
        }
    }

    if display.show_plate_edges {
        upload_plate_boundary_edges(viewport, armor, device, visibility, display.show_zero_mm);
    }

    if display.show_waterline {
        let (verts, indices) = create_water_plane(0.0, armor.bounds, display.waterline_opacity);
        viewport.add_world_space_mesh(device, &verts, &indices, LAYER_HULL);
    }

    viewport.mark_dirty();
    mesh_triangle_info
}

/// Initial-load upload: builds `armor`'s meshes (see [`upload_armor_meshes`])
/// and frames the camera on `armor.bounds`, matching the egui app's
/// `init_armor_viewport` (`tab.rs:1445-1495`). Use [`reupload_armor_plates`]
/// instead when only `visibility`/`display` changed and the camera should
/// stay put.
pub fn upload_armor_to_viewport(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    armor: &LoadedShipArmor,
    visibility: VisibilityFilter,
    display: DisplaySettings,
) -> Vec<(MeshId, Vec<ArmorTriangleTooltip>)> {
    let mesh_triangle_info = upload_armor_meshes(viewport, device, armor, visibility, display);
    let (min, max) = armor.bounds;
    viewport.camera = ArcballCamera::from_bounds(min, max);
    viewport.mark_dirty();
    mesh_triangle_info
}

/// Re-upload after a `part_visibility`/`plate_visibility`/`display` change
/// (popover toggle, click-to-hide, context-menu toggle): rebuilds the meshes
/// but leaves the camera alone, so neither kind of change ever resets the
/// user's view.
pub fn reupload_armor_plates(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    armor: &LoadedShipArmor,
    visibility: VisibilityFilter,
    display: DisplaySettings,
) -> Vec<(MeshId, Vec<ArmorTriangleTooltip>)> {
    upload_armor_meshes(viewport, device, armor, visibility, display)
}

/// Create a water plane quad at the given Y height, extending beyond the hull
/// bounding box. Ports `armor_viewer::ui::tab::create_water_plane` verbatim.
fn create_water_plane(
    y: f32,
    bounds: (crate::viewport::types::Vec3, crate::viewport::types::Vec3),
    opacity: f32,
) -> (Vec<Vertex>, Vec<u32>) {
    let cx = (bounds.0.x + bounds.1.x) * 0.5;
    let cz = (bounds.0.z + bounds.1.z) * 0.5;
    let ex = (bounds.1.x - bounds.0.x) * 2.25;
    let ez = (bounds.1.z - bounds.0.z) * 2.25;

    let color = [0.1, 0.4, 0.8, opacity];
    let normal = [0.0, 1.0, 0.0];

    let vertices = vec![
        Vertex { position: [cx - ex, y, cz - ez], normal, color, uv: [0.0, 0.0] },
        Vertex { position: [cx + ex, y, cz - ez], normal, color, uv: [0.0, 0.0] },
        Vertex { position: [cx + ex, y, cz + ez], normal, color, uv: [0.0, 0.0] },
        Vertex { position: [cx - ex, y, cz + ez], normal, color, uv: [0.0, 0.0] },
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];

    (vertices, indices)
}

/// Quantize a float position to an integer key to avoid floating-point
/// comparison issues when matching up shared triangle edges.
fn quantize(v: [f32; 3]) -> [i32; 3] {
    [(v[0] * 10000.0).round() as i32, (v[1] * 10000.0).round() as i32, (v[2] * 10000.0).round() as i32]
}

type EdgeKey = ([i32; 3], [i32; 3]);

fn make_edge_key(a: [i32; 3], b: [i32; 3]) -> EdgeKey {
    if a < b { (a, b) } else { (b, a) }
}

/// One triangle edge's plate identity and face normal, for boundary-edge detection.
struct EdgeInfo {
    plate_key: PlateKey,
    normal: [f32; 3],
    p0: [f32; 3],
    p1: [f32; 3],
}

/// Upload plate boundary edge outlines where adjacent triangles have
/// different thickness values, differing plates, or a sharp crease. Ports
/// `armor_viewer::ui::tab::upload_plate_boundary_edges` verbatim (minus the
/// `show_hidden_only` filter -- v1 has no pane UI for it yet, see the module
/// doc; `part_visibility`/`plate_visibility` are applied like the main mesh
/// loop, via the shared `plate_is_visible`).
fn upload_plate_boundary_edges(
    viewport: &mut Viewport3D,
    armor: &LoadedShipArmor,
    device: &wgpu::Device,
    visibility: VisibilityFilter,
    show_zero_mm: bool,
) {
    let mut edge_map: HashMap<EdgeKey, Vec<EdgeInfo>> = HashMap::new();

    for mesh in &armor.meshes {
        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            if !plate_is_visible(info, visibility, show_zero_mm) {
                continue;
            }

            let base_idx = tri_idx * 3;
            if base_idx + 2 >= mesh.indices.len() {
                continue;
            }

            let plate_key: PlateKey = derive_plate_key(info);

            let mut tri_pos = [[0.0_f32; 3]; 3];
            for (k, vertex) in tri_pos.iter_mut().enumerate() {
                let orig_idx = mesh.indices[base_idx + k] as usize;
                if orig_idx >= mesh.positions.len() {
                    continue;
                }
                let mut pos = mesh.positions[orig_idx];
                if let Some(t) = &mesh.transform {
                    pos = transform_point(t, pos);
                }
                *vertex = pos;
            }

            let e1 = [tri_pos[1][0] - tri_pos[0][0], tri_pos[1][1] - tri_pos[0][1], tri_pos[1][2] - tri_pos[0][2]];
            let e2 = [tri_pos[2][0] - tri_pos[0][0], tri_pos[2][1] - tri_pos[0][1], tri_pos[2][2] - tri_pos[0][2]];
            let nx = e1[1] * e2[2] - e1[2] * e2[1];
            let ny = e1[2] * e2[0] - e1[0] * e2[2];
            let nz = e1[0] * e2[1] - e1[1] * e2[0];
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            let face_normal = if len > 1e-10 { [nx / len, ny / len, nz / len] } else { [0.0, 1.0, 0.0] };

            let edges = [(0, 1), (1, 2), (2, 0)];
            for (a, b) in edges {
                let qa = quantize(tri_pos[a]);
                let qb = quantize(tri_pos[b]);
                let edge_key = make_edge_key(qa, qb);

                edge_map.entry(edge_key).or_default().push(EdgeInfo {
                    plate_key: plate_key.clone(),
                    normal: face_normal,
                    p0: tri_pos[a],
                    p1: tri_pos[b],
                });
            }
        }
    }

    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for infos in edge_map.values() {
        let is_mesh_boundary = infos.len() == 1;
        let first_plate = &infos[0].plate_key;
        let is_plate_boundary = infos.len() >= 2 && infos.iter().any(|i| i.plate_key != *first_plate);
        let is_crease = infos.len() >= 2 && {
            let n0 = &infos[0].normal;
            infos[1..].iter().any(|i| {
                let dot = n0[0] * i.normal[0] + n0[1] * i.normal[1] + n0[2] * i.normal[2];
                dot < 0.7
            })
        };
        if !is_mesh_boundary && !is_plate_boundary && !is_crease {
            continue;
        }

        let p0 = infos[0].p0;
        let p1 = infos[0].p1;

        let mut avg_normal = [0.0_f32; 3];
        for info in infos {
            avg_normal[0] += info.normal[0];
            avg_normal[1] += info.normal[1];
            avg_normal[2] += info.normal[2];
        }
        let n_len =
            (avg_normal[0] * avg_normal[0] + avg_normal[1] * avg_normal[1] + avg_normal[2] * avg_normal[2]).sqrt();
        if n_len < 1e-10 {
            continue;
        }
        avg_normal[0] /= n_len;
        avg_normal[1] /= n_len;
        avg_normal[2] /= n_len;

        let edge_dir = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let edge_len = (edge_dir[0] * edge_dir[0] + edge_dir[1] * edge_dir[1] + edge_dir[2] * edge_dir[2]).sqrt();
        if edge_len < 1e-10 {
            continue;
        }

        let tx = edge_dir[1] * avg_normal[2] - edge_dir[2] * avg_normal[1];
        let ty = edge_dir[2] * avg_normal[0] - edge_dir[0] * avg_normal[2];
        let tz = edge_dir[0] * avg_normal[1] - edge_dir[1] * avg_normal[0];
        let t_len = (tx * tx + ty * ty + tz * tz).sqrt();
        if t_len < 1e-10 {
            continue;
        }
        let tangent = [tx / t_len, ty / t_len, tz / t_len];

        for &n_sign in &[1.0_f32, -1.0] {
            let base = vertices.len() as u32;
            let offset = [
                avg_normal[0] * PLATE_EDGE_NORMAL_OFFSET * n_sign,
                avg_normal[1] * PLATE_EDGE_NORMAL_OFFSET * n_sign,
                avg_normal[2] * PLATE_EDGE_NORMAL_OFFSET * n_sign,
            ];
            let vert_normal = [avg_normal[0] * n_sign, avg_normal[1] * n_sign, avg_normal[2] * n_sign];
            for &p in &[p0, p1] {
                for &sign in &[-1.0_f32, 1.0] {
                    vertices.push(Vertex {
                        position: [
                            p[0] + tangent[0] * PLATE_EDGE_HALF_WIDTH * sign + offset[0],
                            p[1] + tangent[1] * PLATE_EDGE_HALF_WIDTH * sign + offset[1],
                            p[2] + tangent[2] * PLATE_EDGE_HALF_WIDTH * sign + offset[2],
                        ],
                        normal: vert_normal,
                        color: EDGE_COLOR,
                        uv: [0.0, 0.0],
                    });
                }
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }

    if !indices.is_empty() {
        viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_DEFAULT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_zero_mm_matches_the_egui_epsilon() {
        assert!(is_zero_mm(0.0));
        assert!(is_zero_mm(0.04));
        assert!(!is_zero_mm(0.05));
        assert!(!is_zero_mm(1.0));
    }

    #[test]
    fn make_edge_key_is_order_independent() {
        let a = [1, 2, 3];
        let b = [4, 5, 6];
        assert_eq!(make_edge_key(a, b), make_edge_key(b, a));
    }

    #[test]
    fn quantize_rounds_to_the_nearest_tenth_of_a_millimeter_bucket() {
        assert_eq!(quantize([0.00001, 0.0, 0.0]), [0, 0, 0]);
        assert_eq!(quantize([0.12345, -0.12345, 1.0]), [1235, -1235, 10000]);
    }

    fn test_triangle_info(zone: &str, material: &str, thickness_mm: f32) -> ArmorTriangleInfo {
        ArmorTriangleInfo {
            model_index: 1,
            triangle_index: 0,
            material_id: 0,
            material_name: material.to_string(),
            zone: zone.to_string(),
            thickness_mm,
            layers: vec![thickness_mm],
            color: [0.0, 0.0, 0.0, 1.0],
            hidden: false,
        }
    }

    #[test]
    fn derive_plate_key_rounds_thickness_to_tenths_of_a_mm() {
        let info = test_triangle_info("Citadel", "Cit_Belt", 32.04);
        assert_eq!(derive_plate_key(&info), ("Citadel".to_string(), "Cit_Belt".to_string(), 320));
    }

    fn no_overrides() -> (HashMap<(String, String), bool>, HashMap<PlateKey, bool>) {
        (HashMap::new(), HashMap::new())
    }

    #[test]
    fn plate_is_visible_hides_zero_mm_triangles_by_default() {
        let info = test_triangle_info("Hull", "Trans", 0.0);
        let (part, plate) = no_overrides();
        assert!(!plate_is_visible(&info, VisibilityFilter { part: &part, plate: &plate }, false));
    }

    #[test]
    fn plate_is_visible_shows_zero_mm_triangles_when_show_zero_mm_is_on() {
        let info = test_triangle_info("Hull", "Trans", 0.0);
        let (part, plate) = no_overrides();
        assert!(plate_is_visible(&info, VisibilityFilter { part: &part, plate: &plate }, true));
    }

    #[test]
    fn plate_is_visible_respects_an_explicitly_hidden_plate() {
        let info = test_triangle_info("Citadel", "Cit_Belt", 32.0);
        let key = derive_plate_key(&info);
        let (part, empty) = no_overrides();
        assert!(plate_is_visible(&info, VisibilityFilter { part: &part, plate: &empty }, false));

        let mut hidden = HashMap::new();
        hidden.insert(key, true);
        assert!(!plate_is_visible(&info, VisibilityFilter { part: &part, plate: &hidden }, false));
    }

    #[test]
    fn plate_is_visible_ignores_other_plates_in_the_visibility_map() {
        let info = test_triangle_info("Citadel", "Cit_Belt", 32.0);
        let (part, _) = no_overrides();
        let mut hidden = HashMap::new();
        hidden.insert(("Bow".to_string(), "Bow_Bottom".to_string(), 16), true);
        assert!(plate_is_visible(&info, VisibilityFilter { part: &part, plate: &hidden }, false));
    }

    #[test]
    fn plate_is_visible_hides_a_triangle_whose_part_is_off() {
        let info = test_triangle_info("Citadel", "Cit_Belt", 32.0);
        let mut part = HashMap::new();
        part.insert(("Citadel".to_string(), "Cit_Belt".to_string()), false);
        let plate = HashMap::new();
        assert!(!plate_is_visible(&info, VisibilityFilter { part: &part, plate: &plate }, false));
    }

    #[test]
    fn display_settings_default_matches_the_egui_armor_viewer_defaults() {
        let d = DisplaySettings::default();
        assert!(d.show_plate_edges);
        assert!(d.show_waterline);
        assert!(!d.show_zero_mm);
        assert_eq!(d.armor_opacity, 1.0);
        assert_eq!(d.waterline_opacity, 0.3);
        assert!(!d.hull_opaque);
    }

    #[test]
    fn display_settings_from_defaults_falls_back_when_no_row() {
        assert_eq!(DisplaySettings::from_defaults(None), DisplaySettings::default());
    }

    #[test]
    fn display_settings_from_defaults_reads_a_persisted_row() {
        let row = ArmorViewerDefaultsRow {
            show_plate_edges: false,
            show_waterline: false,
            show_zero_mm: true,
            armor_opacity: 0.5,
            waterline_opacity: 0.75,
            hull_opaque: true,
            hull_all_visible: false,
            armor_all_visible: true,
            show_splash_boxes: false,
            show_legend: true,
            legend_collapsed: false,
            legend_pos_x: None,
            legend_pos_y: None,
        };
        let d = DisplaySettings::from_defaults(Some(&row));
        assert!(!d.show_plate_edges);
        assert!(!d.show_waterline);
        assert!(d.show_zero_mm);
        assert!(d.hull_opaque);
        assert_eq!(d.armor_opacity, 0.5);
        assert_eq!(d.waterline_opacity, 0.75);
    }

    /// Needs a local game install and a real GPU adapter: loads one real
    /// ship's armor (mirrors `load_ship.rs`'s own real-install test), uploads
    /// it into a fresh `Viewport3D` on an owned wgpu device (the same
    /// `GpuContext::new()` `viewport::device`'s risk-gate test uses), and
    /// renders it offscreen -- proving the whole T4 pipeline (catalog ->
    /// load -> upload -> camera framing -> render) end to end without a
    /// windowed app. Asserts the render actually shows thickness-colored
    /// geometry (not just the clear color) and that plate boundary edges
    /// (pure black) are present, matching `DisplaySettings::default()`'s
    /// `show_plate_edges = true`. Run with:
    ///
    /// ```text
    /// WOWS_ARMOR_VIEWER_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// cargo test -p wows-toolkit-gpui -- --ignored --nocapture upload_armor_to_viewport_against_a_real_install_renders_thickness_colored_geometry
    /// ```
    #[test]
    #[ignore = "needs a local game install and a real GPU adapter; see the doc comment for the run command"]
    fn upload_armor_to_viewport_against_a_real_install_renders_thickness_colored_geometry() {
        use crate::viewport::device::GpuContext;

        let wows_dir = std::env::var("WOWS_ARMOR_VIEWER_LOAD_TEST_DIR")
            .expect("set WOWS_ARMOR_VIEWER_LOAD_TEST_DIR to a WoWs install directory");
        let wows_dir = std::path::PathBuf::from(wows_dir);

        let available =
            wowsunpack::game_data::list_available_builds(&wows_dir).expect("failed to list installed builds");
        let build = *available.last().expect("expected at least one installed build");
        let vfs = wowsunpack::game_data::build_game_vfs_for_build(&wows_dir, build)
            .expect("failed to build the game VFS for the latest installed build");
        let metadata = std::sync::Arc::new(
            wowsunpack::game_params::provider::GameMetadataProvider::from_vfs(&vfs)
                .expect("failed to build GameMetadataProvider from the VFS"),
        );
        let ship_assets =
            wowsunpack::export::ship::ShipAssets::from_vfs_with_metadata(&vfs, std::sync::Arc::clone(&metadata))
                .expect("failed to load ShipAssets from the VFS");

        let catalog = crate::armor_viewer::catalog::ShipCatalog::build(&metadata);
        // A battleship tends to have the richest armor scheme (many distinct
        // plates/zones), which best exercises the plate-boundary-edge path.
        let ship = catalog
            .nations
            .iter()
            .flat_map(|n| &n.classes)
            .filter(|c| c.species == wowsunpack::game_params::types::Species::Battleship)
            .flat_map(|c| &c.ships)
            .next()
            .expect("expected at least one battleship in the real catalog");

        let armor = crate::armor_viewer::load_ship::load_ship_armor_by_param(
            &ship_assets,
            &ship.param_index,
            &ship.display_name,
        )
        .unwrap_or_else(|e| panic!("failed to load armor for {} ({}): {e}", ship.display_name, ship.param_index));

        let ctx = GpuContext::new().expect("owned wgpu device creation failed - no adapter available");
        let pipeline = ctx.pipeline();
        let mut viewport = Viewport3D::new();

        let empty_part = HashMap::new();
        let empty_plate = HashMap::new();
        let display = DisplaySettings::default();
        let mesh_triangle_info = upload_armor_to_viewport(
            &mut viewport,
            &ctx.device,
            &armor,
            VisibilityFilter { part: &empty_part, plate: &empty_plate },
            display,
        );
        assert!(!mesh_triangle_info.is_empty(), "expected at least one uploaded armor mesh");
        let total_triangles: usize = mesh_triangle_info.iter().map(|(_, tooltips)| tooltips.len()).sum();
        assert!(total_triangles > 0, "expected at least one uploaded armor triangle");

        // Hiding one real plate and re-uploading (the click-to-hide path,
        // `picking_ui.rs`) should drop exactly that plate's triangles and
        // nothing else, without moving the camera.
        let tooltip_key = |t: &ArmorTriangleTooltip| {
            (t.zone.clone(), t.material_name.clone(), (t.thickness_mm * 10.0).round() as i32)
        };
        let hidden_key = mesh_triangle_info[0]
            .1
            .first()
            .map(tooltip_key)
            .expect("expected at least one tooltip to derive a plate key from");
        let hidden_count_before =
            mesh_triangle_info.iter().flat_map(|(_, t)| t).filter(|t| tooltip_key(t) == hidden_key).count();
        let mut plate_visibility = HashMap::new();
        plate_visibility.insert(hidden_key.clone(), true);
        let (target_before, distance_before, azimuth_before, elevation_before) =
            (viewport.camera.target, viewport.camera.distance, viewport.camera.azimuth, viewport.camera.elevation);
        let reuploaded = reupload_armor_plates(
            &mut viewport,
            &ctx.device,
            &armor,
            VisibilityFilter { part: &empty_part, plate: &plate_visibility },
            display,
        );
        let total_after: usize = reuploaded.iter().map(|(_, tooltips)| tooltips.len()).sum();
        assert_eq!(
            total_triangles - total_after,
            hidden_count_before,
            "expected hiding one plate to drop exactly its own triangles"
        );
        assert!(
            reuploaded.iter().flat_map(|(_, t)| t).all(|t| tooltip_key(t) != hidden_key),
            "hidden plate's triangles should not reappear in the reuploaded tooltips"
        );
        assert_eq!(viewport.camera.target, target_before, "re-upload for a visibility toggle must not move the camera");
        assert_eq!(viewport.camera.distance, distance_before, "re-upload for a visibility toggle must not zoom");
        assert_eq!(viewport.camera.azimuth, azimuth_before, "re-upload for a visibility toggle must not rotate");
        assert_eq!(viewport.camera.elevation, elevation_before, "re-upload for a visibility toggle must not rotate");

        let (w, h, rgba) = viewport
            .render_offscreen_rgba(&ctx.device, &ctx.queue, &pipeline, (512, 512))
            .expect("offscreen render produced no pixels");
        assert_eq!(rgba.len(), (w * h * 4) as usize);

        let clear = viewport.clear_color;
        let clear_rgb =
            [(clear.r * 255.0).round() as i32, (clear.g * 255.0).round() as i32, (clear.b * 255.0).round() as i32];
        let mut non_clear_pixels = 0usize;
        let mut black_pixels = 0usize;
        for px in rgba.chunks_exact(4) {
            let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
            let dist = (r - clear_rgb[0]).abs() + (g - clear_rgb[1]).abs() + (b - clear_rgb[2]).abs();
            if dist > 20 {
                non_clear_pixels += 1;
            }
            if r < 20 && g < 20 && b < 20 {
                black_pixels += 1;
            }
        }
        assert!(
            non_clear_pixels > 1000,
            "expected a substantial number of non-clear-color pixels (thickness-colored armor), got {non_clear_pixels}"
        );
        assert!(
            black_pixels > 0,
            "expected some near-black pixels from plate boundary edges (show_plate_edges = true), got none"
        );

        // Task 7b: `show_plate_edges = false` must drop the boundary-edge
        // overlay -- the near-black pixel count should fall well below the
        // edges-on render (armor material colors are never solid black in
        // the thickness legend, so near-black pixels are edge outlines).
        let no_edges = DisplaySettings { show_plate_edges: false, ..display };
        reupload_armor_plates(
            &mut viewport,
            &ctx.device,
            &armor,
            VisibilityFilter { part: &empty_part, plate: &empty_plate },
            no_edges,
        );
        let (_, _, rgba_no_edges) = viewport
            .render_offscreen_rgba(&ctx.device, &ctx.queue, &pipeline, (512, 512))
            .expect("offscreen render produced no pixels");
        let black_pixels_no_edges =
            rgba_no_edges.chunks_exact(4).filter(|px| px[0] < 20 && px[1] < 20 && px[2] < 20).count();
        assert!(
            black_pixels_no_edges < black_pixels,
            "expected show_plate_edges = false to reduce near-black (edge) pixels: before={black_pixels} after={black_pixels_no_edges}"
        );

        // Task 7b: `armor_opacity` sets every uploaded vertex's alpha, and
        // the pipeline alpha-blends (`BlendState::ALPHA_BLENDING`), so a
        // lower opacity should blend the armor color toward the clear color
        // -- the average non-clear-color distance should shrink.
        let dim = DisplaySettings { armor_opacity: 0.3, ..no_edges };
        reupload_armor_plates(
            &mut viewport,
            &ctx.device,
            &armor,
            VisibilityFilter { part: &empty_part, plate: &empty_plate },
            dim,
        );
        let (_, _, rgba_dim) = viewport
            .render_offscreen_rgba(&ctx.device, &ctx.queue, &pipeline, (512, 512))
            .expect("offscreen render produced no pixels");
        let avg_clear_distance = |rgba: &[u8]| -> f64 {
            let mut total = 0i64;
            let mut n = 0i64;
            for px in rgba.chunks_exact(4) {
                let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
                total +=
                    (r - clear_rgb[0]).abs() as i64 + (g - clear_rgb[1]).abs() as i64 + (b - clear_rgb[2]).abs() as i64;
                n += 1;
            }
            total as f64 / n as f64
        };
        let dist_full = avg_clear_distance(&rgba_no_edges);
        let dist_dim = avg_clear_distance(&rgba_dim);
        assert!(
            dist_dim < dist_full,
            "expected a lower armor_opacity to blend pixels closer to the clear color: full={dist_full:.1} dim={dist_dim:.1}"
        );

        println!(
            "uploaded {} ({}, build {build}): {} meshes / {total_triangles} triangles; render {w}x{h}, {non_clear_pixels} non-clear pixels, {black_pixels} near-black (edge) pixels",
            armor.display_name,
            ship.param_index,
            mesh_triangle_info.len(),
        );
    }
}
