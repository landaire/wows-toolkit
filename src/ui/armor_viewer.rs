use std::sync::Arc;
use std::sync::mpsc;

use egui_dock::{DockArea, DockState, TabViewer};

use crate::app::ToolkitTabViewer;
use crate::armor_viewer::legend::show_armor_legend;
use crate::armor_viewer::ship_selector::{ShipCatalog, species_name, tier_roman};
use crate::armor_viewer::state::{ArmorPane, ArmorTriangleTooltip, CompareSettings, LoadedShipArmor, ShipAssetsState};
use crate::icons;
use crate::viewport_3d::{GpuPipeline, LAYER_DEFAULT, LAYER_HULL, MeshId, Vertex};

/// Per-frame viewer struct implementing `egui_dock::TabViewer` for armor panes.
struct ArmorPaneViewer<'a> {
    render_state: &'a eframe::egui_wgpu::RenderState,
    gpu_pipeline: &'a GpuPipeline,
    mirror_camera_signal: &'a std::cell::Cell<Option<u64>>,
    active_pane_signal: &'a std::cell::Cell<Option<u64>>,
    translate_part: &'a dyn Fn(&str) -> String,
    tab_count: usize,
}

impl TabViewer for ArmorPaneViewer<'_> {
    type Tab = ArmorPane;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("armor_pane", tab.id))
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.loaded_armor.as_ref().map(|a| a.display_name.as_str()).unwrap_or("Empty").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        render_armor_pane(
            ui,
            tab,
            self.render_state,
            self.gpu_pipeline,
            self.mirror_camera_signal,
            self.active_pane_signal,
            self.translate_part,
        );
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        self.tab_count > 1
    }

    fn clear_background(&self, _tab: &Self::Tab) -> bool {
        false
    }
}

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
                            let catalog = ShipCatalog::build(metadata);
                            // Load nation flags for each nation in the catalog.
                            for nation_group in &catalog.nations {
                                if !state.nation_flag_textures.contains_key(&nation_group.nation) {
                                    if let Some(asset) = crate::task::load_nation_flag(&wd.vfs, &nation_group.nation) {
                                        state.nation_flag_textures.insert(nation_group.nation.clone(), asset);
                                    }
                                }
                            }
                            state.ship_catalog = Some(Arc::new(catalog));
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
        let nation_flags = &state.nation_flag_textures;

        // Poll per-pane ship loading receivers
        poll_pane_loads(&mut state.dock_state, &render_state.device, &gpu_pipeline);

        // Multi-pane toolbar (only shown when multiple panes exist)
        let pane_count = state.dock_state.main_surface().num_tabs();
        if pane_count > 1 {
            ui.horizontal(|ui| {
                ui.toggle_value(&mut state.mirror_cameras, "Mirror cameras");
                ui.toggle_value(&mut state.sync_options, "Sync options")
                    .on_hover_text("Keep armor/hull visibility in sync across all panes");
            });
        }

        // Track which pane's camera was interacted with during rendering.
        let active_camera_pane: std::cell::Cell<Option<u64>> = std::cell::Cell::new(None);
        let active_camera_ref = &active_camera_pane;
        let mirror_cameras = state.mirror_cameras;

        // Track which pane was interacted with (becomes the active pane for sidebar selection).
        let active_pane_cell: std::cell::Cell<Option<u64>> = std::cell::Cell::new(None);
        let active_pane_ref = &active_pane_cell;

        // Build a translation helper from game metadata.
        let wd = wows_data.read();
        let translate_part = |name: &str| -> String {
            let id = format!("IDS_{}", name.to_uppercase());
            wd.game_metadata
                .as_ref()
                .and_then(|m| {
                    use wowsunpack::data::ResourceLoader;
                    m.localized_name_from_id(&id)
                })
                .unwrap_or_else(|| name.to_string())
        };
        let translate_part_ref = &translate_part;

        // Layout: sidebar (left) | separator | split_tree (right)
        let available = ui.available_rect_before_wrap();
        let sidebar_width = 200.0_f32.min(available.width() * 0.3);
        let separator_width = 6.0;

        let sidebar_rect = egui::Rect::from_min_size(available.min, egui::vec2(sidebar_width, available.height()));
        let content_rect = egui::Rect::from_min_max(
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

        // Determine which pane is "active" for ship selection.
        // Default to the first pane if the stored active_pane_id doesn't match any existing pane.
        let all_pane_ids: Vec<u64> = state.dock_state.iter_all_tabs().map(|(_, tab)| tab.id).collect();
        if !all_pane_ids.contains(&state.active_pane_id) {
            state.active_pane_id = all_pane_ids.first().copied().unwrap_or(0);
        }
        let current_active_id = state.active_pane_id;

        // Ship selector sidebar (rendered once)
        let mut sidebar_compare: Option<CompareSettings> = None;
        {
            let mut sidebar_ui =
                ui.new_child(egui::UiBuilder::new().max_rect(sidebar_rect).id_salt("armor_sidebar_global"));
            // Search bar
            sidebar_ui.horizontal(|ui| {
                ui.label(icons::MAGNIFYING_GLASS);
                ui.text_edit_singleline(&mut state.selector_search);
            });
            sidebar_ui.separator();

            // Ship tree using egui_ltreeview
            if let Some(catalog) = &ship_catalog {
                let search = state.selector_search.to_lowercase();

                // Sort nations by translated display name.
                let mut sorted_nations: Vec<&_> = catalog.nations.iter().collect();
                sorted_nations.sort_by(|a, b| {
                    let ta = translate_part_ref(&a.nation);
                    let tb = translate_part_ref(&b.nation);
                    ta.cmp(&tb)
                });

                // Build ID-to-ship mapping for action handling.
                let mut id_to_ship: std::collections::HashMap<egui::Id, (String, String)> =
                    std::collections::HashMap::new();

                // Find the currently selected ship in the active pane.
                let selected_param = state
                    .dock_state
                    .iter_all_tabs()
                    .find(|(_, tab)| tab.id == current_active_id)
                    .and_then(|(_, tab)| tab.selected_ship.clone());

                // Deferred compare action from context menu (needs Cell since the closure is FnMut).
                let deferred_compare: std::cell::Cell<Option<(String, String)>> = std::cell::Cell::new(None);
                let deferred_compare_ref = &deferred_compare;

                let tree_id = sidebar_ui.make_persistent_id("armor_ship_tree");

                // Auto-expand/collapse nodes based on search state.
                if !search.is_empty() {
                    sidebar_ui.ctx().data_mut(|data| {
                        let tree_state =
                            data.get_temp_mut_or_default::<egui_ltreeview::TreeViewState<egui::Id>>(tree_id);
                        for nation in &sorted_nations {
                            let nation_id = egui::Id::new(("armor_nation", &nation.nation));
                            tree_state.set_openness(nation_id, true);
                            for class in &nation.classes {
                                let class_id =
                                    egui::Id::new(("armor_class", &nation.nation, species_name(&class.species)));
                                tree_state.set_openness(class_id, true);
                            }
                        }
                    });
                }

                let tree = egui_ltreeview::TreeView::new(tree_id);

                let (_response, actions) = tree.show(&mut sidebar_ui, |builder| {
                    for nation in &sorted_nations {
                        let has_match = search.is_empty()
                            || nation
                                .classes
                                .iter()
                                .any(|c| c.ships.iter().any(|s| s.display_name.to_lowercase().contains(&search)));
                        if !has_match {
                            continue;
                        }

                        let nation_id = egui::Id::new(("armor_nation", &nation.nation));
                        let flag_asset = nation_flags.get(&nation.nation).cloned();
                        let nation_display = translate_part_ref(&nation.nation);
                        let dir_node = egui_ltreeview::NodeBuilder::dir(nation_id)
                            .default_open(false)
                            .icon(move |ui| {
                                if let Some(ref flag) = flag_asset {
                                    ui.add(
                                        egui::Image::new(egui::ImageSource::Bytes {
                                            uri: flag.path.clone().into(),
                                            bytes: flag.data.clone().into(),
                                        })
                                        .fit_to_exact_size(egui::vec2(23.0, 16.0)),
                                    );
                                }
                            })
                            .label(nation_display);

                        let is_open = builder.node(dir_node);
                        if is_open {
                            for class in &nation.classes {
                                let has_class_match = search.is_empty()
                                    || class.ships.iter().any(|s| s.display_name.to_lowercase().contains(&search));
                                if !has_class_match {
                                    continue;
                                }

                                let class_id =
                                    egui::Id::new(("armor_class", &nation.nation, species_name(&class.species)));
                                let icon_asset = ship_icons.get(&class.species).cloned();
                                let class_dir = egui_ltreeview::NodeBuilder::dir(class_id)
                                    .default_open(false)
                                    .icon(move |ui| {
                                        if let Some(ref icon) = icon_asset {
                                            ui.add(
                                                egui::Image::new(egui::ImageSource::Bytes {
                                                    uri: icon.path.clone().into(),
                                                    bytes: icon.data.clone().into(),
                                                })
                                                .fit_to_exact_size(egui::vec2(16.0, 16.0))
                                                .rotate(90.0_f32.to_radians(), egui::Vec2::splat(0.5)),
                                            );
                                        }
                                    })
                                    .label(species_name(&class.species));

                                let class_open = builder.node(class_dir);
                                if class_open {
                                    for ship in &class.ships {
                                        if !search.is_empty() && !ship.display_name.to_lowercase().contains(&search) {
                                            continue;
                                        }

                                        let ship_id = egui::Id::new(("armor_ship", &ship.param_index));
                                        id_to_ship
                                            .insert(ship_id, (ship.param_index.clone(), ship.display_name.clone()));

                                        let label = format!("{} {}", tier_roman(ship.tier), ship.display_name);

                                        let param_idx = ship.param_index.clone();
                                        let display_name = ship.display_name.clone();

                                        let leaf = egui_ltreeview::NodeBuilder::leaf(ship_id)
                                            .label(label)
                                            .context_menu(move |ui| {
                                                if ui.button("Compare in new split").clicked() {
                                                    deferred_compare_ref
                                                        .set(Some((param_idx.clone(), display_name.clone())));
                                                    ui.close();
                                                }
                                            });

                                        builder.node(leaf);
                                    }
                                }
                                builder.close_dir();
                            }
                        }
                        builder.close_dir();
                    }
                });

                // Set selection to match the active pane's currently loaded ship.
                if let Some(ref param) = selected_param {
                    let selected_id = egui::Id::new(("armor_ship", param));
                    sidebar_ui.ctx().data_mut(|data| {
                        let tree_state =
                            data.get_temp_mut_or_default::<egui_ltreeview::TreeViewState<egui::Id>>(tree_id);
                        if tree_state.selected() != &vec![selected_id] {
                            tree_state.set_selected(vec![selected_id]);
                        }
                    });
                }

                // Handle tree actions.
                for action in actions {
                    match action {
                        egui_ltreeview::Action::SetSelected(selected_ids) => {
                            // Single-click on a leaf: load the ship into the active pane.
                            for id in &selected_ids {
                                if let Some((param_index, display_name)) = id_to_ship.get(id) {
                                    let already_selected = selected_param.as_deref() == Some(param_index.as_str());
                                    if !already_selected {
                                        let pane = state
                                            .dock_state
                                            .iter_all_tabs_mut()
                                            .find(|(_, tab)| tab.id == current_active_id)
                                            .map(|(_, tab)| tab);
                                        if let Some(pane) = pane {
                                            load_ship_for_pane(pane, param_index, display_name, &ship_assets);
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                        egui_ltreeview::Action::Activate(activate) => {
                            // Double-click: also load the ship.
                            for id in &activate.selected {
                                if let Some((param_index, display_name)) = id_to_ship.get(id) {
                                    let pane = state
                                        .dock_state
                                        .iter_all_tabs_mut()
                                        .find(|(_, tab)| tab.id == current_active_id)
                                        .map(|(_, tab)| tab);
                                    if let Some(pane) = pane {
                                        load_ship_for_pane(pane, param_index, display_name, &ship_assets);
                                    }
                                    break;
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Handle deferred compare action from context menu.
                if let Some((param_index, display_name)) = deferred_compare.take() {
                    // Find the focused pane (or first pane) to clone settings from.
                    let source_settings =
                        state.dock_state.iter_all_tabs().find(|(_, tab)| tab.id == current_active_id).map(
                            |(_, tab)| CompareSettings {
                                ship_param_index: param_index,
                                ship_display_name: display_name,
                                camera: tab.viewport.camera.clone(),
                                part_visibility: tab.part_visibility.clone(),
                                hull_visibility: tab.hull_visibility.clone(),
                            },
                        );
                    if let Some(settings) = source_settings {
                        sidebar_compare = Some(settings);
                    }
                }
            }
        }

        // Render dock area with armor panes
        {
            let tab_count = state.dock_state.main_surface().num_tabs();
            let mut viewer = ArmorPaneViewer {
                render_state: &render_state,
                gpu_pipeline: &gpu_pipeline,
                mirror_camera_signal: if mirror_cameras { active_camera_ref } else { &std::cell::Cell::new(None) },
                active_pane_signal: active_pane_ref,
                translate_part: translate_part_ref,
                tab_count,
            };
            let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect).id_salt("armor_content"));
            DockArea::new(&mut state.dock_state)
                .id(egui::Id::new("armor_dock"))
                .style(egui_dock::Style::from_egui(content_ui.style().as_ref()))
                .allowed_splits(egui_dock::AllowedSplits::All)
                .show_close_buttons(true)
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_inside(&mut content_ui, &mut viewer);
        }

        // Global armor thickness legend (shown once, not per-pane)
        let any_ship_loaded = state.dock_state.iter_all_tabs().any(|(_, tab)| tab.loaded_armor.is_some());
        if any_ship_loaded {
            egui::Window::new("Armor Thickness")
                .id(egui::Id::new("armor_legend_global"))
                .collapsible(true)
                .resizable(false)
                .title_bar(true)
                .show(ui.ctx(), |ui| {
                    show_armor_legend(ui);
                });
        }

        // Mirror cameras: broadcast the interacted pane's camera to all others.
        if mirror_cameras {
            if let Some(active_id) = active_camera_pane.get() {
                let cam = state
                    .dock_state
                    .iter_all_tabs()
                    .find(|(_, tab)| tab.id == active_id)
                    .map(|(_, tab)| tab.viewport.camera.clone());
                if let Some(cam) = cam {
                    for (_, tab) in state.dock_state.iter_all_tabs_mut() {
                        if tab.id != active_id {
                            tab.viewport.camera = cam.clone();
                            tab.viewport.mark_dirty();
                        }
                    }
                }
            }
        }

        // Update active pane from viewport interaction
        if let Some(new_active) = active_pane_cell.get() {
            state.active_pane_id = new_active;
        }

        // Sync options: broadcast visibility from active pane to all others each frame
        if state.sync_options {
            let active_id = state.active_pane_id;
            let source_vis = state
                .dock_state
                .iter_all_tabs()
                .find(|(_, tab)| tab.id == active_id)
                .map(|(_, tab)| (tab.part_visibility.clone(), tab.hull_visibility.clone()));
            if let Some((part_vis, hull_vis)) = source_vis {
                for (_, tab) in state.dock_state.iter_all_tabs_mut() {
                    if tab.id != active_id && (tab.part_visibility != part_vis || tab.hull_visibility != hull_vis) {
                        tab.part_visibility = part_vis.clone();
                        tab.hull_visibility = hull_vis.clone();
                        if let Some(armor) = tab.loaded_armor.take() {
                            upload_armor_to_viewport(tab, &armor, &render_state.device);
                            tab.loaded_armor = Some(armor);
                        }
                    }
                }
            }
        }

        // Apply deferred compare action from sidebar
        if let Some(settings) = sidebar_compare {
            let next_id = state.allocate_pane_id();
            let mut new_pane = ArmorPane::empty(next_id);
            new_pane.viewport.camera = settings.camera.clone();
            new_pane.part_visibility = settings.part_visibility.clone();
            new_pane.hull_visibility = settings.hull_visibility.clone();
            load_ship_for_pane(&mut new_pane, &settings.ship_param_index, &settings.ship_display_name, &ship_assets);
            let tree = state.dock_state.main_surface_mut();
            let target = tree.focused_leaf().unwrap_or(egui_dock::NodeIndex::root());
            tree.split_right(target, 0.5, vec![new_pane]);
        }
    }
}

/// Poll all panes for completed ship loads.
fn poll_pane_loads(dock_state: &mut DockState<ArmorPane>, device: &wgpu::Device, _pipeline: &GpuPipeline) {
    for (_, pane) in dock_state.iter_all_tabs_mut() {
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
}

/// Upload loaded armor meshes to the viewport's GPU buffers,
/// filtering out triangles belonging to invisible parts (by (zone, material_name)).
fn upload_armor_to_viewport(pane: &mut ArmorPane, armor: &LoadedShipArmor, device: &wgpu::Device) {
    pane.viewport.clear();
    pane.mesh_triangle_info.clear();
    pane.hover_highlight = None;
    pane.pinned_highlights.clear();

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
                layers: info.layers.clone(),
                color: info.color,
            });
        }

        if !indices.is_empty() {
            let mesh_id = pane.viewport.add_mesh(device, &vertices, &indices, LAYER_DEFAULT);
            pane.mesh_triangle_info.push((mesh_id, tooltips));
        }
    }

    // Upload hull visual meshes (semi-transparent gray overlay)
    for mesh in &armor.hull_meshes {
        let visible = pane.hull_visibility.get(&mesh.name).copied().unwrap_or(false);
        if !visible {
            continue;
        }

        let hull_alpha: f32 = 0.7;
        let fallback_color: [f32; 4] = [0.6, 0.6, 0.65, hull_alpha];
        let has_baked_colors = mesh.colors.len() == mesh.positions.len();
        let mut vertices: Vec<Vertex> = Vec::with_capacity(mesh.positions.len());
        for i in 0..mesh.positions.len() {
            let mut pos = mesh.positions[i];
            let mut norm = if i < mesh.normals.len() { mesh.normals[i] } else { [0.0, 1.0, 0.0] };

            if let Some(t) = &mesh.transform {
                pos = transform_point(t, pos);
                norm = transform_normal(t, norm);
            }

            let color = if has_baked_colors {
                let c = mesh.colors[i];
                [c[0], c[1], c[2], hull_alpha]
            } else {
                fallback_color
            };
            vertices.push(Vertex { position: pos, normal: norm, color });
        }

        if !mesh.indices.is_empty() {
            pane.viewport.add_mesh(device, &vertices, &mesh.indices, LAYER_HULL);
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

    // Hull parts default to not visible
    pane.hull_visibility.clear();
    for (_group, names) in &armor.hull_part_groups {
        for name in names {
            pane.hull_visibility.insert(name.clone(), false);
        }
    }

    upload_armor_to_viewport(pane, armor, device);

    // Frame camera on the model
    pane.viewport.camera = crate::viewport_3d::ArcballCamera::from_bounds(armor.bounds.0, armor.bounds.1);
    pane.viewport.mark_dirty();
}

/// Render a single armor pane (viewport only, no sidebar — sidebar is rendered once at the top level).
fn render_armor_pane(
    ui: &mut egui::Ui,
    pane: &mut ArmorPane,
    render_state: &eframe::egui_wgpu::RenderState,
    gpu_pipeline: &GpuPipeline,
    mirror_camera_signal: &std::cell::Cell<Option<u64>>,
    active_pane_signal: &std::cell::Cell<Option<u64>>,
    translate_part: &dyn Fn(&str) -> String,
) {
    let pane_id = pane.id;

    // Full viewport area (no sidebar)
    {
        let vp_ui = ui;

        // Zone toggles (top bar, before the 3D image)
        let mut zone_changed = false;
        if let Some(armor) = &pane.loaded_armor {
            if !armor.zone_parts.is_empty() {
                vp_ui.horizontal_wrapped(|ui| {
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
                                    if ui.checkbox(&mut v, translate_part(part)).changed() {
                                        pane.part_visibility.insert(key, v);
                                        zone_changed = true;
                                    }
                                }
                            });

                        ui.add_space(4.0);
                    }

                    // Hull render set toggles, grouped by category.
                    if !armor.hull_part_groups.is_empty() {
                        ui.separator();

                        // Collect all hull part names for the master toggle.
                        let all_hull_names: Vec<&String> =
                            armor.hull_part_groups.iter().flat_map(|(_, names)| names).collect();
                        let all_on =
                            all_hull_names.iter().all(|n| pane.hull_visibility.get(*n).copied().unwrap_or(false));
                        let any_on =
                            all_hull_names.iter().any(|n| pane.hull_visibility.get(*n).copied().unwrap_or(false));

                        let mut checked = all_on;
                        let cb_response = ui.checkbox(&mut checked, "");
                        if any_on && !all_on {
                            let c = cb_response.rect.center();
                            ui.painter().line_segment(
                                [egui::pos2(c.x - 3.5, c.y), egui::pos2(c.x + 3.5, c.y)],
                                egui::Stroke::new(2.0, ui.visuals().warn_fg_color),
                            );
                        }
                        if cb_response.changed() {
                            for name in &all_hull_names {
                                pane.hull_visibility.insert((*name).clone(), checked);
                            }
                            zone_changed = true;
                        }

                        let label_response = ui.selectable_label(false, "Hull Model");
                        egui::Popup::from_toggle_button_response(&label_response)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.horizontal(|ui| {
                                    if ui.small_button("All").clicked() {
                                        for name in &all_hull_names {
                                            pane.hull_visibility.insert((*name).clone(), true);
                                        }
                                        zone_changed = true;
                                    }
                                    if ui.small_button("None").clicked() {
                                        for name in &all_hull_names {
                                            pane.hull_visibility.insert((*name).clone(), false);
                                        }
                                        zone_changed = true;
                                    }
                                });
                                ui.separator();

                                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                                    for (group, names) in &armor.hull_part_groups {
                                        // Per-group header with toggle
                                        let group_all_on =
                                            names.iter().all(|n| pane.hull_visibility.get(n).copied().unwrap_or(false));
                                        let group_any_on =
                                            names.iter().any(|n| pane.hull_visibility.get(n).copied().unwrap_or(false));

                                        ui.horizontal(|ui| {
                                            let mut group_checked = group_all_on;
                                            let gcb = ui.checkbox(&mut group_checked, "");
                                            if group_any_on && !group_all_on {
                                                let c = gcb.rect.center();
                                                ui.painter().line_segment(
                                                    [egui::pos2(c.x - 3.5, c.y), egui::pos2(c.x + 3.5, c.y)],
                                                    egui::Stroke::new(2.0, ui.visuals().warn_fg_color),
                                                );
                                            }
                                            if gcb.changed() {
                                                for name in names {
                                                    pane.hull_visibility.insert(name.clone(), group_checked);
                                                }
                                                zone_changed = true;
                                            }
                                            ui.strong(group);
                                        });

                                        // Individual parts within the group
                                        ui.indent(group, |ui| {
                                            for name in names {
                                                let mut visible =
                                                    pane.hull_visibility.get(name).copied().unwrap_or(false);
                                                if ui.checkbox(&mut visible, name).changed() {
                                                    pane.hull_visibility.insert(name.clone(), visible);
                                                    zone_changed = true;
                                                }
                                            }
                                        });
                                    }
                                }); // ScrollArea
                            });
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
                    mirror_camera_signal.set(Some(pane_id));
                    active_pane_signal.set(Some(pane_id));
                }

                // Clicking on the viewport also makes this the active pane
                if response.clicked() || response.drag_started() {
                    active_pane_signal.set(Some(pane_id));
                }

                // Picking on hover / click / right-click
                let mut hovered_key: Option<(String, String)> = None;
                let context_menu_open =
                    egui::Popup::is_id_open(vp_ui.ctx(), egui::Popup::default_response_id(&response));
                if response.hovered() {
                    if let Some(hover_pos) = response.hover_pos() {
                        if let Some(hit) = pane.viewport.pick(hover_pos, response.rect) {
                            let tooltip = pane
                                .mesh_triangle_info
                                .iter()
                                .find(|(id, _)| *id == hit.mesh_id)
                                .and_then(|(_, infos)| infos.get(hit.triangle_index));

                            if let Some(tooltip) = tooltip {
                                hovered_key = Some((tooltip.zone.clone(), tooltip.material_name.clone()));
                                pane.hovered_info = Some(tooltip.clone());
                                if !context_menu_open {
                                    egui::containers::Tooltip::for_widget(&response).at_pointer().show(|ui| {
                                        show_armor_tooltip(ui, tooltip, translate_part);
                                    });
                                }
                            } else {
                                pane.hovered_info = None;
                            }
                        } else {
                            pane.hovered_info = None;
                        }
                    }
                }

                // Click to toggle pinned highlight
                if response.clicked() {
                    if let Some(ref key) = hovered_key {
                        if pane.pinned_highlights.contains_key(key) {
                            // Unpin
                            if let Some(mesh_id) = pane.pinned_highlights.remove(key) {
                                pane.viewport.remove_mesh(mesh_id);
                            }
                        } else {
                            // Pin with a distinct color
                            if let Some(armor) = pane.loaded_armor.take() {
                                let mesh_id = upload_subcomponent_highlight(
                                    pane,
                                    &armor,
                                    key,
                                    &render_state.device,
                                    [0.2, 0.6, 1.0, 0.45],
                                );
                                pane.pinned_highlights.insert(key.clone(), mesh_id);
                                pane.loaded_armor = Some(armor);
                            }
                        }
                    }
                }

                // Latch context menu key on right-click; clear when menu closes.
                if response.secondary_clicked() {
                    pane.context_menu_key = hovered_key.clone();
                } else if !context_menu_open {
                    pane.context_menu_key = None;
                }

                // Right-click context menu — always call to keep popup alive.
                if let Some(ref ctx_key) = pane.context_menu_key.clone() {
                    let ctx_key = ctx_key.clone();
                    let ctx_name = translate_part(&ctx_key.1);
                    response.context_menu(|ui| {
                        if ui.button(format!("Disable {}", ctx_name)).clicked() {
                            pane.part_visibility.insert(ctx_key.clone(), false);
                            zone_changed = true;
                            // Remove pinned highlight if present
                            if let Some(mesh_id) = pane.pinned_highlights.remove(&ctx_key) {
                                pane.viewport.remove_mesh(mesh_id);
                            }
                            ui.close();
                        }
                        if !pane.pinned_highlights.is_empty() {
                            if ui.button("Disable all selected").clicked() {
                                let keys: Vec<_> = pane.pinned_highlights.keys().cloned().collect();
                                for k in &keys {
                                    pane.part_visibility.insert(k.clone(), false);
                                    if let Some(mesh_id) = pane.pinned_highlights.remove(k) {
                                        pane.viewport.remove_mesh(mesh_id);
                                    }
                                }
                                zone_changed = true;
                                ui.close();
                            }
                        }
                    });
                }

                // Update hover highlight overlay (skip if subcomponent is pinned)
                let hover_key_for_highlight = hovered_key.filter(|k| !pane.pinned_highlights.contains_key(k));
                let current_hover = pane.hover_highlight.as_ref().map(|(k, _)| k.clone());
                if hover_key_for_highlight != current_hover {
                    // Remove old hover highlight
                    if let Some((_, old_id)) = pane.hover_highlight.take() {
                        pane.viewport.remove_mesh(old_id);
                    }
                    // Upload new hover highlight
                    if let Some(ref key) = hover_key_for_highlight {
                        if let Some(armor) = pane.loaded_armor.take() {
                            let mesh_id = upload_subcomponent_highlight(
                                pane,
                                &armor,
                                key,
                                &render_state.device,
                                [1.0, 1.0, 1.0, 0.35],
                            );
                            pane.hover_highlight = Some((key.clone(), mesh_id));
                            pane.loaded_armor = Some(armor);
                        }
                    }
                }
            }
        } else {
            vp_ui.vertical_centered(|ui| {
                let available = ui.available_height();
                ui.add_space(available * 0.4);
                ui.label("Select a ship from the list");
            });
        }
    }
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
    pane.hover_highlight = None;
    pane.pinned_highlights.clear();

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

            // Load hull visual meshes
            let hull_meshes = ctx.interactive_hull_meshes().map_err(|e| format!("{e:?}"))?;

            // Include hull meshes in bounding box
            for mesh in &hull_meshes {
                for pos in &mesh.positions {
                    let p = if let Some(t) = &mesh.transform { transform_point(t, *pos) } else { *pos };
                    for i in 0..3 {
                        min[i] = min[i].min(p[i]);
                        max[i] = max[i].max(p[i]);
                    }
                }
            }

            // Categorize hull parts into logical groups.
            let hull_part_groups = build_hull_part_groups(&hull_meshes);

            Ok(LoadedShipArmor {
                ship_name: index,
                display_name: ship_display_name,
                meshes,
                bounds: (min, max),
                zones,
                zone_parts,
                hull_meshes,
                hull_part_groups,
            })
        })();

        let _ = tx.send(result);
    });

    pane.load_receiver = Some(rx);
}

/// Show tooltip for a hovered armor triangle.
fn show_armor_tooltip(ui: &mut egui::Ui, info: &ArmorTriangleTooltip, translate: &dyn Fn(&str) -> String) {
    ui.horizontal(|ui| {
        let color = egui::Color32::from_rgba_unmultiplied(
            (info.color[0] * 255.0) as u8,
            (info.color[1] * 255.0) as u8,
            (info.color[2] * 255.0) as u8,
            255,
        );
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, color);
        if info.layers.len() > 1 {
            let layer_str: Vec<String> = info.layers.iter().map(|l| format!("{:.0}", l)).collect();
            ui.label(format!("{:.0} mm ({})", info.thickness_mm, layer_str.join(" + ")));
        } else {
            ui.label(format!("{:.0} mm", info.thickness_mm));
        }
    });
    ui.label(format!("Zone: {}", &info.zone));
    ui.label(format!("Part: {}", translate(&info.material_name)));
}

/// Upload a highlight overlay mesh for all triangles in the given (zone, material_name) subcomponent.
/// Returns the MeshId of the uploaded highlight mesh.
fn upload_subcomponent_highlight(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    key: &(String, String),
    device: &wgpu::Device,
    highlight_color: [f32; 4],
) -> MeshId {
    let normal_offset = 0.01; // slight offset along normal to avoid z-fighting

    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for mesh in &armor.meshes {
        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            if info.zone != key.0 || info.material_name != key.1 {
                continue;
            }

            // Check part visibility (skip if this part is toggled off)
            let part_key = (info.zone.clone(), info.material_name.clone());
            if !pane.part_visibility.get(&part_key).copied().unwrap_or(true) {
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

                    // Offset position along normal to render in front
                    pos[0] += norm[0] * normal_offset;
                    pos[1] += norm[1] * normal_offset;
                    pos[2] += norm[2] * normal_offset;

                    vertices.push(Vertex { position: pos, normal: norm, color: highlight_color });
                }
            }
            indices.extend_from_slice(&[new_base, new_base + 1, new_base + 2]);
        }
    }

    pane.viewport.add_overlay_mesh(device, &vertices, &indices)
}

/// Categorize a hull mesh name into a display group.
///
/// Mounted parts have names like `"RenderSet [HP_AGM_1]"` — the HP_ prefix determines the group.
/// Non-mounted hull render sets (no `[HP_...]` suffix) go into a group named after themselves
/// (e.g. "Hull", "Superstructure", "DeckHouse").
fn hull_part_group(name: &str) -> &'static str {
    if let Some(start) = name.find("[HP_") {
        let hp = &name[start + 1..name.len() - 1]; // strip brackets
        if hp.starts_with("HP_AGM") {
            "Main Battery"
        } else if hp.starts_with("HP_AGS") {
            "Secondary Battery"
        } else if hp.starts_with("HP_AGA") {
            "AA Guns"
        } else if hp.starts_with("HP_ATB") || hp.starts_with("HP_AT_") {
            "Torpedoes"
        } else {
            "Other"
        }
    } else {
        "Hull"
    }
}

/// Fixed display order for hull part groups.
fn hull_group_order(group: &str) -> u32 {
    match group {
        "Hull" => 0,
        "Main Battery" => 1,
        "Secondary Battery" => 2,
        "AA Guns" => 3,
        "Torpedoes" => 4,
        "Other" => 5,
        _ => 6,
    }
}

/// Build hull part groups from a list of hull meshes.
fn build_hull_part_groups(
    hull_meshes: &[wowsunpack::export::gltf_export::InteractiveHullMesh],
) -> Vec<(String, Vec<String>)> {
    use std::collections::{BTreeSet, HashMap};

    let mut group_map: HashMap<&str, BTreeSet<String>> = HashMap::new();
    for mesh in hull_meshes {
        let group = hull_part_group(&mesh.name);
        group_map.entry(group).or_default().insert(mesh.name.clone());
    }

    let mut groups: Vec<(String, Vec<String>)> =
        group_map.into_iter().map(|(group, names)| (group.to_string(), names.into_iter().collect())).collect();
    groups.sort_by_key(|(g, _)| hull_group_order(g));
    groups
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
