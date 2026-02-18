use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;

use wowsunpack::game_params::types::Species;

use crate::app::ToolkitTabViewer;
use crate::armor_viewer::legend::show_armor_legend;
use crate::armor_viewer::ship_selector::{ShipCatalog, species_name, tier_roman};
use crate::armor_viewer::split_pane::{CompareSettings, SplitAction};
use crate::armor_viewer::state::{ArmorPane, ArmorTriangleTooltip, LoadedShipArmor, ShipAssetsState};
use crate::icons;
use crate::viewport_3d::{GpuPipeline, Vertex};
use crate::wows_data::GameAsset;

impl ToolkitTabViewer<'_> {
    pub fn build_armor_viewer_tab(&mut self, ui: &mut egui::Ui) {
        let state = &mut self.tab_state.armor_viewer;
        let render_state = self.tab_state.wgpu_render_state.clone();
        let wows_data = self.tab_state.world_of_warships_data.clone();

        // Gate on game data being loaded
        let wows_data = match wows_data {
            Some(data) => data,
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("Load game data in Settings to use the Armor Viewer.");
                });
                return;
            }
        };

        let render_state = match render_state {
            Some(rs) => rs,
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("GPU render state not available.");
                });
                return;
            }
        };

        // Initialize GPU pipeline once
        if state.gpu_pipeline.is_none() {
            state.gpu_pipeline = Some(Arc::new(GpuPipeline::new(&render_state.device)));
        }

        // Initialize ShipAssets on background thread (catalog is built when loading completes)
        if matches!(&state.ship_assets, ShipAssetsState::NotLoaded) {
            let vfs = wows_data.read().vfs.clone();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let result =
                    wowsunpack::export::ship::ShipAssets::load(&vfs).map(Arc::new).map_err(|e| format!("{e:?}"));
                let _ = tx.send(result);
            });
            state.ship_assets = ShipAssetsState::Loading(rx);
        }

        // Poll for ShipAssets loading completion
        if let ShipAssetsState::Loading(rx) = &state.ship_assets {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(assets) => {
                        // Build catalog from wows_data.game_metadata which has translations loaded,
                        // not from assets.metadata() which has translations: None
                        let wd = wows_data.read();
                        if let Some(metadata) = wd.game_metadata.as_ref() {
                            state.ship_catalog = Some(Arc::new(ShipCatalog::build(metadata)));
                        }
                        state.ship_assets = ShipAssetsState::Loaded(assets);
                    }
                    Err(e) => {
                        state.ship_assets = ShipAssetsState::Failed(e);
                    }
                }
            }
        }

        // Show loading state
        let ship_assets_ready = matches!(&state.ship_assets, ShipAssetsState::Loaded(_));
        if !ship_assets_ready {
            match &state.ship_assets {
                ShipAssetsState::Loading(_) => {
                    ui.vertical_centered(|ui| {
                        let available = ui.available_height();
                        ui.add_space(available * 0.4);
                        ui.spinner();
                        ui.label("Loading ship data...");
                    });
                    ui.ctx().request_repaint();
                    return;
                }
                ShipAssetsState::Failed(e) => {
                    ui.vertical_centered(|ui| {
                        let available = ui.available_height();
                        ui.add_space(available * 0.4);
                        ui.colored_label(egui::Color32::RED, format!("Failed to load ship data: {e}"));
                    });
                    return;
                }
                _ => return,
            }
        }

        let ship_assets = match &state.ship_assets {
            ShipAssetsState::Loaded(a) => a.clone(),
            _ => return,
        };
        let gpu_pipeline = state.gpu_pipeline.clone().unwrap();
        let ship_catalog = state.ship_catalog.clone();
        let ship_icons = {
            let wd = wows_data.read();
            wd.ship_icons.clone()
        };

        // Poll per-pane ship loading receivers
        poll_pane_loads(&mut state.split_tree, &render_state.device, &gpu_pipeline);

        // Render split tree
        let action;
        let render_state_ref = &render_state;
        let gpu_pipeline_ref = &gpu_pipeline;
        let ship_catalog_ref = &ship_catalog;
        let ship_assets_ref = &ship_assets;
        let ship_icons_ref = &ship_icons;
        let pane_count = state.split_tree.pane_count();

        action = state.split_tree.show(ui, &mut |ui, pane| {
            render_armor_pane(
                ui,
                pane,
                render_state_ref,
                gpu_pipeline_ref,
                ship_catalog_ref.as_deref(),
                ship_assets_ref,
                ship_icons_ref,
                pane_count,
            )
        });

        // Apply deferred action
        if let Some(ref action) = action {
            let next_id = state.allocate_pane_id();

            // For Compare, create a pane with cloned settings and trigger ship load
            let compare_settings = if let SplitAction::Compare(_, settings) = action { Some(settings) } else { None };

            state.split_tree.apply_action(action, &mut || {
                let mut new_pane = ArmorPane::empty(next_id);
                if let Some(settings) = compare_settings {
                    new_pane.viewport.camera = settings.camera.clone();
                    new_pane.part_visibility = settings.part_visibility.clone();
                    new_pane.show_only_hidden = settings.show_only_hidden;
                    load_ship_for_pane(
                        &mut new_pane,
                        &settings.ship_param_index,
                        &settings.ship_display_name,
                        &ship_assets,
                    );
                }
                new_pane
            });
        }
    }
}

/// Poll all panes for completed ship loads.
fn poll_pane_loads(
    node: &mut crate::armor_viewer::split_pane::SplitNode,
    device: &wgpu::Device,
    pipeline: &GpuPipeline,
) {
    match node {
        crate::armor_viewer::split_pane::SplitNode::Leaf(pane) => {
            if let Some(rx) = &pane.load_receiver {
                if let Ok(result) = rx.try_recv() {
                    match result {
                        Ok(armor) => {
                            init_armor_viewport(pane, &armor, device);
                            pane.loaded_armor = Some(armor);
                        }
                        Err(e) => {
                            tracing::error!("Failed to load ship armor: {e}");
                        }
                    }
                    pane.loading = false;
                    pane.load_receiver = None;
                }
            }
        }
        crate::armor_viewer::split_pane::SplitNode::Split { first, second, .. } => {
            poll_pane_loads(first, device, pipeline);
            poll_pane_loads(second, device, pipeline);
        }
    }
}

/// Upload loaded armor meshes to the viewport's GPU buffers,
/// filtering out triangles belonging to hidden parts (by (zone, material_name)).
fn upload_armor_to_viewport(pane: &mut ArmorPane, armor: &LoadedShipArmor, device: &wgpu::Device) {
    pane.viewport.clear();
    pane.mesh_triangle_info.clear();

    for mesh in &armor.meshes {
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut tooltips: Vec<ArmorTriangleTooltip> = Vec::new();

        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            let key = (info.zone.clone(), info.material_name.clone());
            let part_visible = pane.part_visibility.get(&key).copied().unwrap_or(true);
            if !part_visible {
                continue;
            }

            // When "show only hidden" is on, skip plates with actual thickness
            if pane.show_only_hidden && info.thickness_mm > 0.0 {
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

                    // Apply mount transform for turret armor
                    if let Some(t) = &mesh.transform {
                        pos = transform_point(t, pos);
                        norm = transform_normal(t, norm);
                    }

                    vertices.push(Vertex { position: pos, normal: norm, color: mesh.colors[orig_idx] });
                }
            }
            indices.extend_from_slice(&[new_base, new_base + 1, new_base + 2]);

            tooltips.push(ArmorTriangleTooltip {
                material_name: info.material_name.clone(),
                zone: info.zone.clone(),
                thickness_mm: info.thickness_mm,
                color: info.color,
            });
        }

        if !indices.is_empty() {
            let mesh_id = pane.viewport.add_mesh(device, &vertices, &indices);
            pane.mesh_triangle_info.push((mesh_id, tooltips));
        }
    }

    pane.viewport.mark_dirty();
}

/// Initial upload when a ship is first loaded. Sets up part visibility and camera.
fn init_armor_viewport(pane: &mut ArmorPane, armor: &LoadedShipArmor, device: &wgpu::Device) {
    pane.part_visibility.clear();
    for (zone, parts) in &armor.zone_parts {
        for part in parts {
            pane.part_visibility.insert((zone.clone(), part.clone()), true);
        }
    }

    upload_armor_to_viewport(pane, armor, device);

    // Frame camera on the model
    pane.viewport.camera = crate::viewport_3d::ArcballCamera::from_bounds(armor.bounds.0, armor.bounds.1);
    pane.viewport.mark_dirty();
}

/// Render a single armor pane.
fn render_armor_pane(
    ui: &mut egui::Ui,
    pane: &mut ArmorPane,
    render_state: &eframe::egui_wgpu::RenderState,
    gpu_pipeline: &GpuPipeline,
    ship_catalog: Option<&ShipCatalog>,
    ship_assets: &Arc<wowsunpack::export::ship::ShipAssets>,
    ship_icons: &HashMap<Species, Arc<GameAsset>>,
    total_pane_count: usize,
) -> Option<SplitAction> {
    let mut action = None;

    let pane_id = pane.id;
    let ship_name = pane.loaded_armor.as_ref().map(|a| a.display_name.as_str()).unwrap_or("No ship selected");

    // Toolbar
    ui.horizontal(|ui| {
        ui.strong(ship_name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Close button (only if more than 1 pane)
            if total_pane_count > 1 && ui.small_button(icons::X).clicked() {
                action = Some(SplitAction::Close(pane_id));
            }
            if ui.small_button(icons::SPLIT_VERTICAL).on_hover_text("Split Vertical").clicked() {
                action = Some(SplitAction::SplitVertical(pane_id));
            }
            if ui.small_button(icons::SPLIT_HORIZONTAL).on_hover_text("Split Horizontal").clicked() {
                action = Some(SplitAction::SplitHorizontal(pane_id));
            }
        });
    });

    ui.separator();

    // Main content: sidebar + viewport (fixed-width sidebar, no scrolling)
    let available = ui.available_rect_before_wrap();
    let sidebar_width = 200.0_f32.min(available.width() * 0.3);
    let separator_width = 6.0;

    let sidebar_rect = egui::Rect::from_min_size(available.min, egui::vec2(sidebar_width, available.height()));
    let viewport_rect = egui::Rect::from_min_max(
        egui::pos2(available.left() + sidebar_width + separator_width, available.top()),
        available.max,
    );

    // Reserve the full area
    ui.allocate_rect(available, egui::Sense::hover());

    // Draw separator line
    ui.painter().vline(
        sidebar_rect.right() + separator_width * 0.5,
        sidebar_rect.y_range(),
        ui.visuals().widgets.noninteractive.bg_stroke,
    );

    // Ship selector sidebar
    {
        let mut sidebar_ui =
            ui.new_child(egui::UiBuilder::new().max_rect(sidebar_rect).id_salt(("armor_sidebar", pane_id)));
        // Search bar
        sidebar_ui.horizontal(|ui| {
            ui.label(icons::MAGNIFYING_GLASS);
            ui.text_edit_singleline(&mut pane.selector_search);
        });
        sidebar_ui.separator();

        // Ship tree
        egui::ScrollArea::vertical().id_salt(("armor_tree_scroll", pane_id)).show(&mut sidebar_ui, |ui| {
            if let Some(catalog) = ship_catalog {
                let search = pane.selector_search.to_lowercase();

                for nation in &catalog.nations {
                    let has_match = search.is_empty()
                        || nation
                            .classes
                            .iter()
                            .any(|c| c.ships.iter().any(|s| s.display_name.to_lowercase().contains(&search)));
                    if !has_match {
                        continue;
                    }

                    let nation_id = ui.make_persistent_id(("nation", pane_id, &nation.nation));
                    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), nation_id, false)
                        .show_header(ui, |ui| {
                            ui.label(&nation.nation);
                        })
                        .body(|ui| {
                            for class in &nation.classes {
                                let has_class_match = search.is_empty()
                                    || class.ships.iter().any(|s| s.display_name.to_lowercase().contains(&search));
                                if !has_class_match {
                                    continue;
                                }

                                let class_id = ui.make_persistent_id((
                                    "class",
                                    pane_id,
                                    &nation.nation,
                                    species_name(&class.species),
                                ));
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    class_id,
                                    false,
                                )
                                .show_header(ui, |ui| {
                                    if let Some(icon) = ship_icons.get(&class.species) {
                                        ui.add(
                                            egui::Image::new(egui::ImageSource::Bytes {
                                                uri: icon.path.clone().into(),
                                                bytes: icon.data.clone().into(),
                                            })
                                            .fit_to_exact_size(egui::vec2(16.0, 16.0))
                                            .rotate(90.0_f32.to_radians(), egui::Vec2::splat(0.5)),
                                        );
                                    }
                                    ui.label(species_name(&class.species));
                                })
                                .body(|ui| {
                                    for ship in &class.ships {
                                        if !search.is_empty() && !ship.display_name.to_lowercase().contains(&search) {
                                            continue;
                                        }

                                        let label = format!("{} {}", tier_roman(ship.tier), ship.display_name);
                                        let is_selected =
                                            pane.selected_ship.as_deref() == Some(ship.param_index.as_str());

                                        let response = ui.selectable_label(is_selected, &label);
                                        if response.clicked() && !is_selected {
                                            load_ship_for_pane(
                                                pane,
                                                &ship.param_index,
                                                &ship.display_name,
                                                ship_assets,
                                            );
                                        }
                                        // Right-click context menu: Compare
                                        response.context_menu(|ui| {
                                            if ui.button("Compare in new split").clicked() {
                                                action = Some(SplitAction::Compare(
                                                    pane.id,
                                                    CompareSettings {
                                                        ship_param_index: ship.param_index.clone(),
                                                        ship_display_name: ship.display_name.clone(),
                                                        camera: pane.viewport.camera.clone(),
                                                        part_visibility: pane.part_visibility.clone(),
                                                        show_only_hidden: pane.show_only_hidden,
                                                    },
                                                ));
                                                ui.close();
                                            }
                                        });
                                    }
                                });
                            }
                        });
                }
            }
        });
    }

    // 3D viewport
    {
        let mut vp_ui =
            ui.new_child(egui::UiBuilder::new().max_rect(viewport_rect).id_salt(("armor_viewport", pane_id)));

        // Zone toggles (top bar, before the 3D image)
        let mut zone_changed = false;
        if let Some(armor) = &pane.loaded_armor {
            if !armor.zone_parts.is_empty() {
                vp_ui.horizontal_wrapped(|ui| {
                    if ui.toggle_value(&mut pane.show_only_hidden, "Hidden only").changed() {
                        zone_changed = true;
                    }
                    ui.separator();
                    for (zone, parts) in &armor.zone_parts {
                        let all_on = parts
                            .iter()
                            .all(|p| pane.part_visibility.get(&(zone.clone(), p.clone())).copied().unwrap_or(true));
                        let any_on = parts
                            .iter()
                            .any(|p| pane.part_visibility.get(&(zone.clone(), p.clone())).copied().unwrap_or(true));

                        // Zone checkbox (toggles all parts in zone)
                        let mut checked = all_on;
                        let cb_response = ui.checkbox(&mut checked, "");
                        if any_on && !all_on {
                            // Draw indeterminate dash over the checkbox
                            let c = cb_response.rect.center();
                            ui.painter().line_segment(
                                [egui::pos2(c.x - 3.5, c.y), egui::pos2(c.x + 3.5, c.y)],
                                egui::Stroke::new(2.0, ui.visuals().warn_fg_color),
                            );
                        }
                        if cb_response.changed() {
                            for part in parts {
                                pane.part_visibility.insert((zone.clone(), part.clone()), checked);
                            }
                            zone_changed = true;
                        }

                        // Zone name as clickable label that opens a popup for per-part control
                        let label_response = ui.selectable_label(false, zone);
                        egui::Popup::from_toggle_button_response(&label_response)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.horizontal(|ui| {
                                    if ui.small_button("All").clicked() {
                                        for part in parts {
                                            pane.part_visibility.insert((zone.clone(), part.clone()), true);
                                        }
                                        zone_changed = true;
                                    }
                                    if ui.small_button("None").clicked() {
                                        for part in parts {
                                            pane.part_visibility.insert((zone.clone(), part.clone()), false);
                                        }
                                        zone_changed = true;
                                    }
                                });
                                ui.separator();
                                for part in parts {
                                    let key = (zone.clone(), part.clone());
                                    let mut v = pane.part_visibility.get(&key).copied().unwrap_or(true);
                                    if ui.checkbox(&mut v, part).changed() {
                                        pane.part_visibility.insert(key, v);
                                        zone_changed = true;
                                    }
                                }
                            });

                        ui.add_space(4.0);
                    }
                });
                vp_ui.separator();
            }
        }
        if zone_changed {
            if let Some(armor) = pane.loaded_armor.take() {
                upload_armor_to_viewport(pane, &armor, &render_state.device);
                pane.loaded_armor = Some(armor);
            }
        }

        if pane.loading {
            vp_ui.vertical_centered(|ui| {
                let available = ui.available_height();
                ui.add_space(available * 0.4);
                ui.spinner();
                ui.label("Loading ship...");
            });
            vp_ui.ctx().request_repaint();
        } else if pane.loaded_armor.is_some() {
            let available_size = vp_ui.available_size();
            let pixel_size = (
                (available_size.x * vp_ui.ctx().pixels_per_point()) as u32,
                (available_size.y * vp_ui.ctx().pixels_per_point()) as u32,
            );

            let bounds = pane.loaded_armor.as_ref().map(|a| a.bounds);

            // Render to offscreen texture
            if let Some(tex_id) = pane.viewport.render(render_state, gpu_pipeline, pixel_size) {
                let response = vp_ui.add(
                    egui::Image::new(egui::load::SizedTexture::new(tex_id, available_size))
                        .sense(egui::Sense::click_and_drag()),
                );

                // Camera interaction
                if pane.viewport.handle_input(&response, bounds) {
                    vp_ui.ctx().request_repaint();
                }

                // Picking on hover
                if response.hovered() {
                    if let Some(hover_pos) = response.hover_pos() {
                        if let Some(hit) = pane.viewport.pick(hover_pos, response.rect) {
                            let tooltip = pane
                                .mesh_triangle_info
                                .iter()
                                .find(|(id, _)| *id == hit.mesh_id)
                                .and_then(|(_, infos)| infos.get(hit.triangle_index));

                            if let Some(tooltip) = tooltip {
                                pane.hovered_info = Some(tooltip.clone());
                                egui::containers::Tooltip::for_widget(&response).at_pointer().show(|ui| {
                                    show_armor_tooltip(ui, tooltip);
                                });
                            } else {
                                pane.hovered_info = None;
                            }
                        } else {
                            pane.hovered_info = None;
                        }
                    }
                }
            }

            // Overlay: legend (bottom-right of viewport, draggable window)
            let legend_pos = egui::pos2(viewport_rect.right() - 140.0, viewport_rect.bottom() - 220.0);
            egui::Window::new("Armor Thickness")
                .id(egui::Id::new(("armor_legend", pane.id)))
                .default_pos(legend_pos)
                .collapsible(true)
                .resizable(false)
                .title_bar(true)
                .show(vp_ui.ctx(), |ui| {
                    show_armor_legend(ui);
                });
        } else {
            vp_ui.vertical_centered(|ui| {
                let available = ui.available_height();
                ui.add_space(available * 0.4);
                ui.label("Select a ship from the list");
            });
        }
    }

    action
}

/// Spawn a background thread to load a ship's armor data.
fn load_ship_for_pane(
    pane: &mut ArmorPane,
    param_index: &str,
    display_name: &str,
    ship_assets: &Arc<wowsunpack::export::ship::ShipAssets>,
) {
    pane.selected_ship = Some(param_index.to_string());
    pane.loading = true;
    pane.loaded_armor = None;
    pane.viewport.clear();
    pane.mesh_triangle_info.clear();
    pane.hovered_info = None;

    let assets = ship_assets.clone();
    let index = param_index.to_string();
    let ship_display_name = display_name.to_string();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let result = (|| {
            let options =
                wowsunpack::export::ship::ShipExportOptions { lod: 0, hull: None, textures: false, damaged: false };

            let ctx = assets.load_ship(&index, &options).map_err(|e| format!("{e:?}"))?;

            let meshes = ctx.interactive_armor_meshes().map_err(|e| format!("{e:?}"))?;

            // Compute bounding box (applying mount transforms for turrets)
            let mut min = [f32::MAX; 3];
            let mut max = [f32::MIN; 3];
            for mesh in &meshes {
                for pos in &mesh.positions {
                    let p = if let Some(t) = &mesh.transform { transform_point(t, *pos) } else { *pos };
                    for i in 0..3 {
                        min[i] = min[i].min(p[i]);
                        max[i] = max[i].max(p[i]);
                    }
                }
            }

            // Build zone -> parts mapping
            let mut zone_parts_map: std::collections::HashMap<String, std::collections::HashSet<String>> =
                std::collections::HashMap::new();
            for mesh in &meshes {
                for info in &mesh.triangle_info {
                    zone_parts_map.entry(info.zone.clone()).or_default().insert(info.material_name.clone());
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

            let zones: Vec<String> = zone_parts.iter().map(|(z, _)| z.clone()).collect();

            Ok(LoadedShipArmor {
                ship_name: index,
                display_name: ship_display_name,
                meshes,
                bounds: (min, max),
                zones,
                zone_parts,
            })
        })();

        let _ = tx.send(result);
    });

    pane.load_receiver = Some(rx);
}

/// Show tooltip for a hovered armor triangle.
fn show_armor_tooltip(ui: &mut egui::Ui, info: &ArmorTriangleTooltip) {
    ui.horizontal(|ui| {
        let color = egui::Color32::from_rgba_unmultiplied(
            (info.color[0] * 255.0) as u8,
            (info.color[1] * 255.0) as u8,
            (info.color[2] * 255.0) as u8,
            255,
        );
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, color);
        ui.label(format!("{:.0} mm", info.thickness_mm));
    });
    ui.label(format!("Zone: {}", info.zone));
    ui.label(format!("Part: {}", info.material_name));
}

/// Apply a column-major 4x4 transform to a point (position).
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
