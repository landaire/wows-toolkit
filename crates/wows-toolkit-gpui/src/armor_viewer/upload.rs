//! Uploads a [`LoadedShipArmor`] into a [`Viewport3D`]. Ports the egui app's
//! `upload_armor_to_viewport` (`armor_viewer/ui/tab.rs:1147-1335`) plus the
//! camera-framing half of `init_armor_viewport` (`tab.rs:1445-1495`) -- this
//! milestone has no separate "reupload without recentering the camera" path
//! (that only exists once visibility toggles exist, Milestone 3), so both are
//! one function here.
//!
//! **v1 scope.** Armor meshes only. Hull meshes are loaded into
//! [`LoadedShipArmor`] (see `load_ship.rs`) but never uploaded here -- hull
//! display, camo, and the visibility toggles that gate them are Milestone 4.
//! Camera-orbit-ellipse and ship-center overlays are likewise out of scope
//! (their toggles do not exist yet). Every display setting this module
//! applies is a fixed v1 default matching the egui app's own out-of-the-box
//! defaults (`ArmorViewerDefaults::default()`, `armor_viewer/state.rs:95-113`;
//! `PLATE_EDGE_*`/`EDGE_COLOR`, `armor_viewer/constants.rs:44-50`) -- there is
//! no per-pane override yet (Milestone 3).

use std::collections::HashMap;

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

/// `ArmorViewerDefaults::default().show_zero_mm` (`armor_viewer/state.rs:100`):
/// 0mm-thickness triangles are hidden by default.
const SHOW_ZERO_MM: bool = false;
/// `ArmorViewerDefaults::default().show_plate_edges` (`state.rs:98`).
const SHOW_PLATE_EDGES: bool = true;
/// `ArmorViewerDefaults::default().show_waterline` (`state.rs:99`).
const SHOW_WATERLINE: bool = true;
/// `ArmorViewerDefaults::default().armor_opacity` (`state.rs:101`).
const ARMOR_OPACITY: f32 = 1.0;
/// `ArmorViewerDefaults::default().waterline_opacity` (`state.rs:102`).
const WATERLINE_OPACITY: f32 = 0.3;

/// `armor_viewer::constants::PLATE_EDGE_HALF_WIDTH`.
const PLATE_EDGE_HALF_WIDTH: f32 = 0.003;
/// `armor_viewer::constants::PLATE_EDGE_NORMAL_OFFSET`.
const PLATE_EDGE_NORMAL_OFFSET: f32 = 0.005;
/// `armor_viewer::constants::EDGE_COLOR`.
const EDGE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Skip an armor triangle whose thickness rounds to (effectively) 0mm, unless
/// `SHOW_ZERO_MM` is on. Matches the egui app's own `0.05` epsilon.
fn is_zero_mm(thickness_mm: f32) -> bool {
    thickness_mm.abs() < 0.05
}

/// Clears `viewport` and uploads `armor`'s armor meshes (thickness-colored,
/// per the v1 fixed display defaults above), plate boundary edges, and the
/// waterline plane, then frames the camera on `armor.bounds` and marks the
/// viewport dirty. Returns the per-mesh triangle tooltip data for later
/// picking (nothing reads it yet in this milestone; see the module doc).
pub fn upload_armor_to_viewport(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    armor: &LoadedShipArmor,
) -> Vec<(MeshId, Vec<ArmorTriangleTooltip>)> {
    viewport.clear();

    let mut mesh_triangle_info = Vec::new();
    for mesh in &armor.meshes {
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut tooltips: Vec<ArmorTriangleTooltip> = Vec::new();

        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            if !SHOW_ZERO_MM && is_zero_mm(info.thickness_mm) {
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
                    color[3] = ARMOR_OPACITY;
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

    if SHOW_PLATE_EDGES {
        upload_plate_boundary_edges(viewport, armor, device);
    }

    if SHOW_WATERLINE {
        let (verts, indices) = create_water_plane(0.0, armor.bounds, WATERLINE_OPACITY);
        viewport.add_world_space_mesh(device, &verts, &indices, LAYER_HULL);
    }

    let (min, max) = armor.bounds;
    viewport.camera = crate::viewport::camera::ArcballCamera::from_bounds(min, max);
    viewport.mark_dirty();

    mesh_triangle_info
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
/// `show_zero_mm`/hidden-plate/part-visibility filters -- v1 has no
/// visibility state yet, see the module doc, so every triangle
/// `upload_armor_to_viewport` itself uploaded is eligible here too).
fn upload_plate_boundary_edges(viewport: &mut Viewport3D, armor: &LoadedShipArmor, device: &wgpu::Device) {
    let mut edge_map: HashMap<EdgeKey, Vec<EdgeInfo>> = HashMap::new();

    for mesh in &armor.meshes {
        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            if !SHOW_ZERO_MM && is_zero_mm(info.thickness_mm) {
                continue;
            }

            let base_idx = tri_idx * 3;
            if base_idx + 2 >= mesh.indices.len() {
                continue;
            }

            let plate_key: PlateKey =
                (info.zone.clone(), info.material_name.clone(), (info.thickness_mm * 10.0).round() as i32);

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

    /// Needs a local game install and a real GPU adapter: loads one real
    /// ship's armor (mirrors `load_ship.rs`'s own real-install test), uploads
    /// it into a fresh `Viewport3D` on an owned wgpu device (the same
    /// `GpuContext::new()` `viewport::device`'s risk-gate test uses), and
    /// renders it offscreen -- proving the whole T4 pipeline (catalog ->
    /// load -> upload -> camera framing -> render) end to end without a
    /// windowed app. Asserts the render actually shows thickness-colored
    /// geometry (not just the clear color) and that plate boundary edges
    /// (pure black) are present, matching the v1 fixed defaults
    /// (`SHOW_PLATE_EDGES = true`). Run with:
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

        let mesh_triangle_info = upload_armor_to_viewport(&mut viewport, &ctx.device, &armor);
        assert!(!mesh_triangle_info.is_empty(), "expected at least one uploaded armor mesh");
        let total_triangles: usize = mesh_triangle_info.iter().map(|(_, tooltips)| tooltips.len()).sum();
        assert!(total_triangles > 0, "expected at least one uploaded armor triangle");

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
            "expected some near-black pixels from plate boundary edges (SHOW_PLATE_EDGES = true), got none"
        );

        println!(
            "uploaded {} ({}, build {build}): {} meshes / {total_triangles} triangles; render {w}x{h}, {non_clear_pixels} non-clear pixels, {black_pixels} near-black (edge) pixels",
            armor.display_name,
            ship.param_index,
            mesh_triangle_info.len(),
        );
    }
}
