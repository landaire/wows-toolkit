//! Shared orchestration functions used by both the armor viewer tab
//! (`ui/armor_viewer.rs`) and the realtime armor viewer (`realtime_armor_viewer.rs`).

use std::collections::HashMap;

use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::types::Vehicle;

use super::state::ArmorPane;
use super::state::ArmorZone;
use super::state::LoadedShipArmor;
use super::state::VisibilitySnapshot;
use super::state::ZonePart;

/// Process undo/redo keyboard shortcuts (Ctrl+Z / Ctrl+Shift+Z / Ctrl+R).
/// Returns `true` if visibility was changed (caller should re-upload armor).
pub(crate) fn handle_undo_redo(ui: &egui::Ui, pane: &mut ArmorPane) -> bool {
    let wants_undo = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z) && !i.modifiers.shift);
    let wants_redo = ui.input(|i| {
        i.modifiers.command && (i.key_pressed(egui::Key::R) || (i.key_pressed(egui::Key::Z) && i.modifiers.shift))
    });
    if wants_undo {
        let current = VisibilitySnapshot {
            part_visibility: pane.part_visibility.clone(),
            plate_visibility: pane.plate_visibility.clone(),
        };
        if let Some(prev) = pane.undo_stack.undo(current) {
            pane.part_visibility = prev.part_visibility;
            pane.plate_visibility = prev.plate_visibility;
            return true;
        }
    } else if wants_redo {
        let current = VisibilitySnapshot {
            part_visibility: pane.part_visibility.clone(),
            plate_visibility: pane.plate_visibility.clone(),
        };
        if let Some(next) = pane.undo_stack.redo(current) {
            pane.part_visibility = next.part_visibility;
            pane.plate_visibility = next.plate_visibility;
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Ship loading helpers
// ---------------------------------------------------------------------------

/// Build sorted hull upgrade labels with diff-based suffixes.
///
/// Returns `Vec<(param_key, display_label)>` sorted alphabetically by key.
/// Each label is a letter (A, B, C, ...) optionally followed by the component
/// types that differ from the base (A) upgrade.
pub(crate) fn build_hull_upgrade_names(vehicle: &Vehicle) -> Vec<(String, String)> {
    vehicle
        .hull_upgrades()
        .map(|upgrades| {
            let mut sorted: Vec<_> = upgrades.iter().collect();
            sorted.sort_by_key(|(k, _)| (*k).clone());
            let base = &sorted[0].1;
            sorted
                .iter()
                .enumerate()
                .map(|(i, (k, config))| {
                    let letter = (b'A' + i as u8) as char;
                    let diffs: Vec<String> = ComponentType::ALL
                        .iter()
                        .filter(|&&ct| ct != ComponentType::Hull)
                        .filter(|&&ct| config.component_name(ct) != base.component_name(ct))
                        .map(|ct| ct.to_string())
                        .collect();
                    let label = if diffs.is_empty() || i == 0 {
                        format!("{letter}")
                    } else {
                        format!("{letter} ({})", diffs.join(", "))
                    };
                    ((*k).clone(), label)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Look up `dock_y_offset` for the selected hull upgrade (or the first upgrade
/// if `selected_hull` is `None`).
pub(crate) fn resolve_dock_y_offset(vehicle: &Vehicle, selected_hull: &Option<String>) -> Option<f32> {
    vehicle
        .hull_upgrades()
        .and_then(|upgrades| {
            if let Some(sel) = selected_hull {
                upgrades.get(sel)
            } else {
                let mut keys: Vec<&String> = upgrades.keys().collect();
                keys.sort();
                keys.first().and_then(|k| upgrades.get(*k))
            }
        })
        .and_then(|c| c.dock_y_offset())
}

/// Options for [`load_ship_armor`]. Callers populate the fields they care about;
/// others use sensible defaults via `..Default::default()`.
#[derive(Default)]
pub(crate) struct ShipLoadOptions {
    pub display_name: String,
    pub lod: usize,
    pub selected_hull: Option<String>,
    pub module_overrides: HashMap<ComponentType, String>,
    /// When `true`, parse splash box data from hull geometry.
    /// The realtime viewer skips this.
    pub include_splash_data: bool,
    /// When `true`, extract hit location data from the ship context.
    /// The realtime viewer skips this.
    pub include_hit_locations: bool,
    /// Pre-computed module alternatives. Pass `Vec::new()` if not needed.
    pub module_alternatives: Vec<(ComponentType, Vec<String>)>,
    /// Pre-computed hull upgrade names (from [`build_hull_upgrade_names`]).
    pub hull_upgrade_names: Vec<(String, String)>,
    /// Pre-computed dock Y offset (from [`resolve_dock_y_offset`]).
    pub dock_y_offset: Option<f32>,
}

/// Full re-upload sequence after a zone/visibility change.
///
/// `upload_armor_to_viewport` calls `viewport.clear()` which destroys all uploaded meshes,
/// so trajectories, splash overlays, and splash-box wireframes must be re-uploaded.
pub(crate) fn reupload_after_zone_change(
    pane: &mut ArmorPane,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &crate::viewport_3d::GpuPipeline,
    comparison_ships: &[super::penetration::ComparisonShip],
    ifhe_enabled: bool,
    traj_display_params: &[TrajectoryDisplayParams],
) {
    // 1. Re-upload armor meshes (calls viewport.clear() internally)
    if let Some(armor) = pane.loaded_armor.take() {
        crate::ui::armor_viewer::upload_armor_to_viewport(pane, &armor, device, queue, pipeline);
        pane.loaded_armor = Some(armor);
    }

    // 2. Re-upload trajectory visualizations (viewport.clear() destroyed them)
    reupload_trajectory_meshes(pane, device, traj_display_params, false);

    // 3. Re-upload splash overlays if active
    reupload_splash_overlays(pane, device, comparison_ships, ifhe_enabled);

    // 4. Re-upload splash box wireframes if enabled
    crate::ui::armor_viewer::upload_splash_box_wireframes(pane, device, None);
}

/// Load a ship's armor model on the current thread (intended to run inside
/// `std::thread::spawn`). Returns [`LoadedShipArmor`] on success.
///
/// This is the shared core of both `load_ship_for_pane_with_lod` and
/// `RealtimeArmorViewer::start_ship_load_with_lod`.
pub(crate) fn load_ship_armor(
    vehicle: &Vehicle,
    ship_assets: &ShipAssets,
    options: ShipLoadOptions,
) -> Result<LoadedShipArmor, String> {
    let export_options = wowsunpack::export::ship::ShipExportOptions {
        lod: options.lod,
        hull: options.selected_hull.clone(),
        textures: false,
        damaged: false,
        module_overrides: options.module_overrides,
    };
    let ctx = ship_assets.load_ship_from_vehicle(vehicle, &export_options).map_err(|e| format!("{e:?}"))?;

    // --- Armor meshes + bounding box ---
    let meshes = ctx.interactive_armor_meshes().map_err(|e| format!("{e:?}"))?;

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for mesh in &meshes {
        for pos in &mesh.positions {
            let p =
                if let Some(t) = &mesh.transform { crate::ui::armor_viewer::transform_point(t, *pos) } else { *pos };
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
    }

    // --- Zone / part / plate metadata ---
    let mut zone_parts_map: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    let mut zone_part_plates_map: std::collections::HashMap<
        String,
        std::collections::HashMap<String, std::collections::BTreeSet<i32>>,
    > = std::collections::HashMap::new();
    for mesh in &meshes {
        for info in &mesh.triangle_info {
            zone_parts_map.entry(info.zone.clone()).or_default().insert(info.material_name.clone());
            let thickness_key = (info.thickness_mm * 10.0).round() as i32;
            zone_part_plates_map
                .entry(info.zone.clone())
                .or_default()
                .entry(info.material_name.clone())
                .or_default()
                .insert(thickness_key);
        }
    }
    let mut zone_parts: Vec<(String, Vec<String>)> = zone_parts_map
        .into_iter()
        .map(|(zone, parts)| {
            let mut parts: Vec<String> = parts.into_iter().collect();
            parts.sort();
            (zone, parts)
        })
        .collect();
    zone_parts.sort_by(|a, b| a.0.cmp(&b.0));

    let zone_part_plates: Vec<ArmorZone> = zone_parts
        .iter()
        .map(|(zone, parts)| {
            let parts_with_plates = parts
                .iter()
                .map(|part| {
                    let plates = zone_part_plates_map
                        .get(zone)
                        .and_then(|m| m.get(part))
                        .map(|s| s.iter().copied().collect())
                        .unwrap_or_default();
                    ZonePart { name: part.clone(), plates }
                })
                .collect();
            ArmorZone { name: zone.clone(), parts: parts_with_plates }
        })
        .collect();

    let zones: Vec<String> = zone_parts.iter().map(|(z, _)| z.clone()).collect();

    // --- Hull meshes ---
    let hull_meshes = ctx.interactive_hull_meshes().map_err(|e| format!("{e:?}"))?;

    // Extend bounding box with hull meshes
    for mesh in &hull_meshes {
        for pos in &mesh.positions {
            let p =
                if let Some(t) = &mesh.transform { crate::ui::armor_viewer::transform_point(t, *pos) } else { *pos };
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
    }

    let hull_part_groups = crate::ui::armor_viewer::build_hull_part_groups(&hull_meshes);

    // --- Splash data (optional) ---
    let (splash_data, splash_box_groups, hit_locations) = if options.include_splash_data {
        let splash = crate::armor_viewer::splash::parse_ship_splash_data(ctx.hull_splash_bytes(), ctx.hit_locations());
        let groups = splash
            .as_ref()
            .map(|sd| crate::armor_viewer::splash::build_splash_box_groups(&sd.boxes))
            .unwrap_or_default();
        let hit_locs = if options.include_hit_locations { ctx.hit_locations().cloned() } else { None };
        (splash, groups, hit_locs)
    } else {
        (None, Vec::new(), None)
    };

    tracing::debug!("Ship loaded: dock_y_offset={:?}, bounds Y=[{:.4}, {:.4}]", options.dock_y_offset, min[1], max[1]);

    // --- Hull albedo textures (DDS -> RGBA8) ---
    let mut hull_textures = std::collections::HashMap::new();
    for mesh in &hull_meshes {
        if let Some(mfm) = &mesh.mfm_path {
            if hull_textures.contains_key(mfm) {
                continue;
            }
            if let Some(dds_bytes) = wowsunpack::export::texture::load_base_albedo_bytes(ship_assets.vfs(), mfm)
                && let Ok(dds) = image_dds::ddsfile::Dds::read(&mut std::io::Cursor::new(&dds_bytes))
                && let Ok(img) = image_dds::image_from_dds(&dds, 0)
            {
                let w = img.width();
                let h = img.height();
                hull_textures.insert(mfm.clone(), (w, h, img.into_raw()));
            }
        }
    }

    let hull_lod_count = ctx.hull_lod_count();

    let mut armor = LoadedShipArmor {
        display_name: options.display_name,
        meshes,
        bounds: (min, max),
        zones,
        zone_parts,
        zone_part_plates,
        hull_meshes,
        hull_part_groups,
        dock_y_offset: options.dock_y_offset,
        splash_data,
        splash_box_groups,
        hit_locations,
        waterline_dy: 0.0,
        hull_textures,
        hull_lod_count,
        hull_lod: options.lod,
        hull_upgrade_names: options.hull_upgrade_names,
        loaded_hull: options.selected_hull,
        module_alternatives: options.module_alternatives,
    };
    armor.apply_waterline_offset();
    Ok(armor)
}

// ---------------------------------------------------------------------------
// Sidebar highlight lifecycle
// ---------------------------------------------------------------------------

/// Update the sidebar hover highlight overlay mesh.
///
/// Compares `new_key` with the current sidebar highlight. If different, removes
/// the old overlay and uploads a new one. The caller must temporarily take
/// `armor` out of `pane.loaded_armor` before calling (and restore it after).
pub(crate) fn update_sidebar_highlight(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    new_key: Option<super::state::SidebarHighlightKey>,
    device: &wgpu::Device,
) {
    use super::state::SidebarHighlightKey;

    let current_key = pane.sidebar_highlight.as_ref().map(|(k, _)| k.clone());
    if new_key == current_key {
        return;
    }
    // Remove old highlight
    if let Some((_, old_id)) = pane.sidebar_highlight.take() {
        pane.viewport.remove_mesh(old_id);
        pane.viewport.mark_dirty();
    }
    if let Some(key) = new_key {
        let mesh_id = match &key {
            SidebarHighlightKey::Zone(z) => crate::ui::armor_viewer::upload_zone_highlight(pane, armor, z, device),
            SidebarHighlightKey::Part(z, p) => {
                crate::ui::armor_viewer::upload_part_highlight(pane, armor, z, p, device)
            }
            SidebarHighlightKey::Plate(pk) => crate::ui::armor_viewer::upload_plate_highlight(
                pane,
                armor,
                pk,
                device,
                crate::ui::armor_viewer::SIDEBAR_HIGHLIGHT_COLOR,
            ),
            SidebarHighlightKey::HullMeshes(names) => {
                let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                crate::ui::armor_viewer::upload_hull_highlight(pane, armor, &name_refs, device)
            }
            SidebarHighlightKey::SplashBoxes(names) => {
                crate::ui::armor_viewer::upload_splash_box_highlight(pane, armor, names, device)
            }
        };
        pane.sidebar_highlight = Some((key, mesh_id));
        pane.viewport.mark_dirty();
    }
}

// ---------------------------------------------------------------------------
// Poll load receivers
// ---------------------------------------------------------------------------

/// Poll `load_receiver` and `hull_load_receiver` on a single pane.
/// Returns `true` if the ship load completed (caller may want to set
/// additional flags like `ship_loaded` or request a repaint).
pub(crate) fn poll_pane_load_receivers(
    pane: &mut ArmorPane,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &crate::viewport_3d::GpuPipeline,
) -> bool {
    let mut ship_loaded = false;

    if let Some(rx) = &pane.load_receiver
        && let Ok(result) = rx.try_recv()
    {
        match result {
            Ok(armor) => {
                crate::ui::armor_viewer::init_armor_viewport(pane, &armor, device, queue, pipeline);
                pane.loaded_armor = Some(armor);
                ship_loaded = true;
            }
            Err(e) => {
                tracing::error!("Failed to load ship armor: {e}");
            }
        }
        pane.loading = false;
        pane.load_receiver = None;
    }

    if let Some(rx) = &pane.hull_load_receiver
        && let Ok(result) = rx.try_recv()
    {
        match result {
            Ok(data) => {
                crate::ui::armor_viewer::apply_hull_reload(pane, data, device, queue, pipeline);
            }
            Err(e) => {
                tracing::error!("Failed to reload hull LOD: {e}");
            }
        }
        pane.hull_load_receiver = None;
    }

    ship_loaded
}

// ---------------------------------------------------------------------------
// Trajectory building helpers
// ---------------------------------------------------------------------------

/// Build [`TrajectoryHit`] entries from ray-cast results against the armor mesh.
///
/// `all_hits` is the output of `viewport.pick_all_ray()`: pairs of (HitResult, surface_normal).
/// `mesh_triangle_info` is the per-mesh, per-triangle metadata from the pane.
pub(crate) fn build_traj_hits(
    all_hits: &[(crate::viewport_3d::types::HitResult, [f32; 3])],
    mesh_triangle_info: &[(crate::viewport_3d::MeshId, Vec<super::state::ArmorTriangleTooltip>)],
    shell_dir: &[f32; 3],
) -> Vec<super::penetration::TrajectoryHit> {
    let first_dist = all_hits.first().map(|h| h.0.distance).unwrap_or(0.0);
    let mut traj_hits = Vec::new();
    for (armor_hit, normal) in all_hits {
        let tooltip = mesh_triangle_info
            .iter()
            .find(|(id, _)| *id == armor_hit.mesh_id)
            .and_then(|(_, infos)| infos.get(armor_hit.triangle_index));
        if let Some(info) = tooltip {
            let angle = super::penetration::impact_angle_deg(shell_dir, normal);
            traj_hits.push(super::penetration::TrajectoryHit {
                position: armor_hit.world_position,
                thickness_mm: info.thickness_mm,
                zone: info.zone.clone(),
                material: info.material_name.clone(),
                angle_deg: angle,
                distance_from_start: armor_hit.distance - first_dist,
            });
        }
    }
    traj_hits
}

/// Result of AP shell simulation through armor hits.
pub(crate) struct ApSimResult {
    pub detonation_point: Option<[f32; 3]>,
    pub last_visible_hit: Option<usize>,
    pub sim: super::penetration::ShellSimResult,
}

/// Simulate a single AP shell through armor hits. Returns detonation point,
/// the earliest terminating event index, and the full simulation result.
pub(crate) fn simulate_ap_shell(
    params: &super::ballistics::ShellParams,
    impact: &super::ballistics::ImpactResult,
    traj_hits: &[super::penetration::TrajectoryHit],
    shell_dir: &[f32; 3],
) -> ApSimResult {
    let sim = super::penetration::simulate_shell_through_hits(params, impact, traj_hits, shell_dir);
    let detonation_point = sim.detonation.as_ref().map(|det| det.position);
    let shell_stop = match (sim.detonated_at, sim.stopped_at) {
        (Some(d), Some(s)) => Some(d.min(s)),
        (Some(d), None) => Some(d),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    };
    ApSimResult { detonation_point, last_visible_hit: shell_stop, sim }
}

/// Build a 3D ballistic arc for visualization.
///
/// `approach_xz` is the normalized XZ approach direction (shell_dir projected to horizontal).
/// `first_hit_pos` is the first armor hit position (arc end point).
/// `model_extent` is the max(dx, dz) of the ship bounding box.
pub(crate) fn build_ballistic_arc_3d(
    params: &super::ballistics::ShellParams,
    impact: &super::ballistics::ImpactResult,
    approach_xz: [f32; 3],
    first_hit_pos: [f32; 3],
    model_extent: f32,
) -> Vec<[f32; 3]> {
    let arc_horiz_extent = model_extent * 2.0;
    let (arc_2d, height_ratio) = super::ballistics::simulate_arc_points(params, impact.launch_angle, 60);
    let arc_height_extent = arc_horiz_extent * (height_ratio as f32).max(0.02);
    arc_2d
        .iter()
        .map(|(xf, yf)| {
            let xf = *xf as f32;
            let yf = *yf as f32;
            let along = (1.0 - xf) * arc_horiz_extent;
            [
                first_hit_pos[0] - approach_xz[0] * along,
                first_hit_pos[1] + yf * arc_height_extent,
                first_hit_pos[2] - approach_xz[2] * along,
            ]
        })
        .collect()
}

/// Normalize the XZ approach direction from a shell direction vector.
/// Returns `[1.0, 0.0, 0.0]` if the XZ component is too small.
pub(crate) fn approach_xz_from_shell_dir(shell_dir: &[f32; 3]) -> [f32; 3] {
    let approach_xz = [shell_dir[0], 0.0_f32, shell_dir[2]];
    let len = (approach_xz[0] * approach_xz[0] + approach_xz[2] * approach_xz[2]).sqrt();
    if len > 0.001 { [approach_xz[0] / len, 0.0, approach_xz[2] / len] } else { [1.0, 0.0, 0.0] }
}

// ---------------------------------------------------------------------------
// Trajectory re-upload helpers
// ---------------------------------------------------------------------------

/// Per-trajectory display parameters for re-upload.
pub(crate) struct TrajectoryDisplayParams {
    pub color: [f32; 4],
    pub line_width_mult: f32,
}

/// Build default display params (palette color, lw=1.0) for every trajectory.
pub(crate) fn default_trajectory_display_params(
    trajectories: &[super::state::StoredTrajectory],
) -> Vec<TrajectoryDisplayParams> {
    trajectories
        .iter()
        .map(|traj| {
            let color = super::constants::TRAJECTORY_PALETTE
                [traj.meta.color_index % super::constants::TRAJECTORY_PALETTE.len()];
            TrajectoryDisplayParams { color, line_width_mult: 1.0 }
        })
        .collect()
}

/// Re-upload all trajectory visualization meshes on a pane.
///
/// `display_params` must have the same length as `pane.trajectories`.
/// When `remove_old` is true, removes existing `mesh_id` before uploading
/// (needed when old meshes still exist). When false, assumes `viewport.clear()`
/// already removed them.
pub(crate) fn reupload_trajectory_meshes(
    pane: &mut ArmorPane,
    device: &wgpu::Device,
    display_params: &[TrajectoryDisplayParams],
    remove_old: bool,
) {
    let cam_dist = pane.viewport.camera.distance;
    let marker_opacity = pane.marker_opacity;
    for (i, traj) in pane.trajectories.iter_mut().enumerate() {
        if remove_old && let Some(old_mid) = traj.mesh_id.take() {
            pane.viewport.remove_mesh(old_mid);
        }
        let dp = &display_params[i];
        traj.mesh_id = Some(crate::ui::armor_viewer::upload_trajectory_visualization(
            &mut pane.viewport,
            &traj.result,
            device,
            dp.color,
            traj.last_visible_hit,
            cam_dist,
            marker_opacity,
            dp.line_width_mult,
        ));
        traj.marker_cam_dist = cam_dist;
    }
}

// ---------------------------------------------------------------------------
// Splash overlay re-upload
// ---------------------------------------------------------------------------

/// Re-upload splash visualization overlays (cube + penetration highlight).
///
/// No-op if `pane.splash_result` is `None` or no HE/SAP shell is found.
/// Call after `upload_armor_to_viewport` which destroys existing splash meshes.
pub(crate) fn reupload_splash_overlays(
    pane: &mut ArmorPane,
    device: &wgpu::Device,
    comparison_ships: &[super::penetration::ComparisonShip],
    ifhe_enabled: bool,
) {
    // Read splash data into locals before mutating pane
    let (impact_point, half_extent) = match pane.splash_result {
        Some(ref sr) => (sr.impact_point, sr.half_extent),
        None => return,
    };

    pane.splash_mesh_ids.clear();

    let shell = comparison_ships
        .iter()
        .flat_map(|s| s.shells.iter())
        .find(|s| s.ammo_type == wowsunpack::game_params::types::AmmoType::HE)
        .or_else(|| {
            comparison_ships
                .iter()
                .flat_map(|s| s.shells.iter())
                .find(|s| s.ammo_type == wowsunpack::game_params::types::AmmoType::SAP)
        });

    let Some(shell) = shell else { return };

    let (cube_verts, cube_indices) =
        super::splash::build_splash_cube_mesh(impact_point, half_extent, super::splash::SPLASH_CUBE_COLOR);
    if !cube_verts.is_empty() {
        let cube_mid = pane.viewport.add_overlay_mesh(device, &cube_verts, &cube_indices);
        pane.viewport.set_world_space(cube_mid, true);
        pane.splash_mesh_ids.push(cube_mid);
    }

    if let Some(ref armor) = pane.loaded_armor {
        let (hl_verts, hl_indices, _, _) =
            super::splash::build_splash_highlight_mesh(&armor.meshes, impact_point, half_extent, shell, ifhe_enabled);
        if !hl_verts.is_empty() {
            let hl_mid = pane.viewport.add_overlay_mesh(device, &hl_verts, &hl_indices);
            pane.viewport.set_world_space(hl_mid, true);
            pane.splash_mesh_ids.push(hl_mid);
        }
    }
}
