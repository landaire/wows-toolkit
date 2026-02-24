//! Shared orchestration functions used by both the armor viewer tab
//! (`ui/armor_viewer.rs`) and the realtime armor viewer (`realtime_armor_viewer.rs`).

use std::collections::HashMap;

use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::types::Vehicle;

use super::state::{ArmorPane, LoadedShipArmor, VisibilitySnapshot};

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

impl Default for ShipLoadOptions {
    fn default() -> Self {
        Self {
            display_name: String::new(),
            lod: 0,
            selected_hull: None,
            module_overrides: HashMap::new(),
            include_splash_data: false,
            include_hit_locations: false,
            module_alternatives: Vec::new(),
            hull_upgrade_names: Vec::new(),
            dock_y_offset: None,
        }
    }
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

    let zone_part_plates: Vec<(String, Vec<(String, Vec<i32>)>)> = zone_parts
        .iter()
        .map(|(zone, parts)| {
            let parts_with_plates: Vec<(String, Vec<i32>)> = parts
                .iter()
                .map(|part| {
                    let plates = zone_part_plates_map
                        .get(zone)
                        .and_then(|m| m.get(part))
                        .map(|s| s.iter().copied().collect())
                        .unwrap_or_default();
                    (part.clone(), plates)
                })
                .collect();
            (zone.clone(), parts_with_plates)
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
            if let Some(dds_bytes) = wowsunpack::export::texture::load_base_albedo_bytes(ship_assets.vfs(), mfm) {
                if let Ok(dds) = image_dds::ddsfile::Dds::read(&mut std::io::Cursor::new(&dds_bytes)) {
                    if let Ok(img) = image_dds::image_from_dds(&dds, 0) {
                        let w = img.width();
                        let h = img.height();
                        hull_textures.insert(mfm.clone(), (w, h, img.into_raw()));
                    }
                }
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

    if let Some(rx) = &pane.load_receiver {
        if let Ok(result) = rx.try_recv() {
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
    }

    if let Some(rx) = &pane.hull_load_receiver {
        if let Ok(result) = rx.try_recv() {
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
    }

    ship_loaded
}
