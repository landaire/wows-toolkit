use std::sync::Arc;
use std::sync::mpsc;

use egui_dock::{DockArea, DockState, TabViewer};

use crate::app::ToolkitTabViewer;
use crate::armor_viewer::legend::show_armor_legend;
use crate::armor_viewer::ship_selector::{ShipCatalog, species_name, tier_roman};
use crate::armor_viewer::state::{
    ArmorPane, ArmorTriangleTooltip, ArmorViewerDefaults, CompareSettings, LoadedShipArmor, PlateKey, ShipAssetsState,
    VisibilitySnapshot,
};
use crate::icons;
use crate::viewport_3d::{GpuPipeline, LAYER_DEFAULT, LAYER_HULL, MeshId, Vertex};

/// Per-frame viewer struct implementing `egui_dock::TabViewer` for armor panes.
struct ArmorPaneViewer<'a> {
    render_state: &'a eframe::egui_wgpu::RenderState,
    gpu_pipeline: &'a GpuPipeline,
    mirror_camera_signal: &'a std::cell::Cell<Option<u64>>,
    active_pane_signal: &'a std::cell::Cell<Option<u64>>,
    save_defaults_signal: &'a std::cell::Cell<Option<ArmorViewerDefaults>>,
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
            self.save_defaults_signal,
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
        let armor_defaults = self.tab_state.armor_viewer_defaults.clone();
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
            let wd = wows_data.read();
            let vfs = wd.vfs.clone();
            let game_metadata = wd.game_metadata.clone();
            drop(wd);
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let result = (|| -> Result<Arc<wowsunpack::export::ship::ShipAssets>, String> {
                    let metadata = game_metadata.ok_or_else(|| "GameMetadataProvider not loaded".to_string())?;
                    let assets = wowsunpack::export::ship::ShipAssets::from_vfs_with_metadata(&vfs, metadata)
                        .map_err(|e| format!("{e:?}"))?;
                    Ok(Arc::new(assets))
                })();
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
                let search = unidecode::unidecode(&state.selector_search).to_lowercase();

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

                let searching = !search.is_empty();

                // When search text changes, update node openness: expand matching, collapse non-matching.
                if search != state.prev_selector_search {
                    sidebar_ui.ctx().data_mut(|data| {
                        let tree_state =
                            data.get_temp_mut_or_default::<egui_ltreeview::TreeViewState<egui::Id>>(tree_id);
                        for nation in &sorted_nations {
                            let nation_id = egui::Id::new(("armor_nation", &nation.nation));
                            let nation_has_match = searching
                                && nation
                                    .classes
                                    .iter()
                                    .any(|c| c.ships.iter().any(|s| s.search_name.contains(&search)));
                            tree_state.set_openness(nation_id, searching && nation_has_match);
                            for class in &nation.classes {
                                let class_id =
                                    egui::Id::new(("armor_class", &nation.nation, species_name(&class.species)));
                                let class_has_match =
                                    searching && class.ships.iter().any(|s| s.search_name.contains(&search));
                                tree_state.set_openness(class_id, searching && class_has_match);
                            }
                        }
                    });
                    state.prev_selector_search = search.clone();
                }

                let tree = egui_ltreeview::TreeView::new(tree_id);

                let scroll_out = egui::ScrollArea::both().show(&mut sidebar_ui, |ui| {
                    tree.show(ui, |builder| {
                        for nation in &sorted_nations {
                            let has_match = search.is_empty()
                                || nation
                                    .classes
                                    .iter()
                                    .any(|c| c.ships.iter().any(|s| s.search_name.contains(&search)));
                            if !has_match {
                                continue;
                            }

                            let nation_id = egui::Id::new(("armor_nation", &nation.nation));
                            let flag_asset = nation_flags.get(&nation.nation).cloned();
                            let nation_display = translate_part_ref(&nation.nation);
                            let dir_node = egui_ltreeview::NodeBuilder::dir(nation_id)
                                .default_open(searching)
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
                                        || class.ships.iter().any(|s| s.search_name.contains(&search));
                                    if !has_class_match {
                                        continue;
                                    }

                                    let class_id =
                                        egui::Id::new(("armor_class", &nation.nation, species_name(&class.species)));
                                    let icon_asset = ship_icons.get(&class.species).cloned();
                                    let class_dir = egui_ltreeview::NodeBuilder::dir(class_id)
                                        .default_open(searching)
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
                                            if !search.is_empty() && !ship.search_name.contains(&search) {
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
                    })
                });
                let (_response, actions) = scroll_out.inner;

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
        let save_defaults_cell: std::cell::Cell<Option<ArmorViewerDefaults>> = std::cell::Cell::new(None);
        let save_defaults_ref = &save_defaults_cell;
        {
            let tab_count = state.dock_state.main_surface().num_tabs();
            let mut viewer = ArmorPaneViewer {
                render_state: &render_state,
                gpu_pipeline: &gpu_pipeline,
                mirror_camera_signal: if mirror_cameras { active_camera_ref } else { &std::cell::Cell::new(None) },
                active_pane_signal: active_pane_ref,
                save_defaults_signal: save_defaults_ref,
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
            let mut new_pane = ArmorPane::with_defaults(next_id, &armor_defaults);
            new_pane.viewport.camera = settings.camera.clone();
            new_pane.part_visibility = settings.part_visibility.clone();
            new_pane.hull_visibility = settings.hull_visibility.clone();
            load_ship_for_pane(&mut new_pane, &settings.ship_param_index, &settings.ship_display_name, &ship_assets);
            let tree = state.dock_state.main_surface_mut();
            let target = tree.focused_leaf().unwrap_or(egui_dock::NodeIndex::root());
            tree.split_right(target, 0.5, vec![new_pane]);
        }

        // Apply saved defaults from Display popover (signal set inside render_armor_pane)
        if let Some(new_defaults) = save_defaults_cell.take() {
            self.tab_state.armor_viewer_defaults = new_defaults;
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
/// filtering out triangles belonging to invisible parts or hidden plates.
fn upload_armor_to_viewport(pane: &mut ArmorPane, armor: &LoadedShipArmor, device: &wgpu::Device) {
    pane.viewport.clear();
    pane.mesh_triangle_info.clear();
    pane.hover_highlight = None;

    for mesh in &armor.meshes {
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut tooltips: Vec<ArmorTriangleTooltip> = Vec::new();

        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            // Skip 0mm plates unless explicitly enabled
            if !pane.show_zero_mm && info.thickness_mm.abs() < 0.05 {
                continue;
            }

            let key = (info.zone.clone(), info.material_name.clone());
            let part_visible = pane.part_visibility.get(&key).copied().unwrap_or(true);
            if !part_visible {
                continue;
            }

            let plate_key: PlateKey =
                (info.zone.clone(), info.material_name.clone(), (info.thickness_mm * 10.0).round() as i32);
            let plate_visible = pane.plate_visibility.get(&plate_key).copied().unwrap_or(true);
            if !plate_visible {
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

                    let mut color = mesh.colors[orig_idx];
                    color[3] = pane.armor_opacity;
                    vertices.push(Vertex { position: pos, normal: norm, color });
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

    // Upload plate boundary edge outlines
    if pane.show_plate_edges {
        upload_plate_boundary_edges(pane, armor, device);
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

    // Water plane at draft height.
    // The model's Y=0 appears to be at the keel. The `draft` value (in the same units as
    // the model) tells us how deep below the waterline the keel sits, so the waterline
    // is at Y = draft. However, draft is in real-world meters while the model uses a
    // smaller coordinate system. We estimate the scale from the model's vertical extent
    // vs a typical hull depth (~draft * 1.3 as a rough approximation).
    // For now, place the plane at Y=0 (which appears to be the waterline in BigWorld models).
    if pane.show_waterline && armor.draft_meters.is_some() {
        let (verts, indices) = create_water_plane(0.0, armor.bounds);
        pane.viewport.add_non_pickable_mesh(device, &verts, &indices, LAYER_HULL);
    }

    pane.viewport.mark_dirty();
}

/// Initial upload when a ship is first loaded. Sets up part visibility and camera.
fn init_armor_viewport(pane: &mut ArmorPane, armor: &LoadedShipArmor, device: &wgpu::Device) {
    pane.part_visibility.clear();
    pane.plate_visibility.clear();
    pane.undo_stack.clear();
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
    save_defaults_signal: &std::cell::Cell<Option<ArmorViewerDefaults>>,
    translate_part: &dyn Fn(&str) -> String,
) {
    let pane_id = pane.id;

    // Full viewport area (no sidebar)
    {
        let vp_ui = ui;

        // Undo/redo keyboard shortcuts (Ctrl+Z / Ctrl+Shift+Z / Ctrl+R)
        let mut zone_changed = false;
        {
            let wants_undo = vp_ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z) && !i.modifiers.shift);
            let wants_redo = vp_ui.input(|i| {
                i.modifiers.command
                    && (i.key_pressed(egui::Key::R) || (i.key_pressed(egui::Key::Z) && i.modifiers.shift))
            });
            if wants_undo {
                let current = VisibilitySnapshot {
                    part_visibility: pane.part_visibility.clone(),
                    plate_visibility: pane.plate_visibility.clone(),
                };
                if let Some(prev) = pane.undo_stack.undo(current) {
                    pane.part_visibility = prev.part_visibility;
                    pane.plate_visibility = prev.plate_visibility;
                    zone_changed = true;
                }
            } else if wants_redo {
                let current = VisibilitySnapshot {
                    part_visibility: pane.part_visibility.clone(),
                    plate_visibility: pane.plate_visibility.clone(),
                };
                if let Some(next) = pane.undo_stack.redo(current) {
                    pane.part_visibility = next.part_visibility;
                    pane.plate_visibility = next.plate_visibility;
                    zone_changed = true;
                }
            }
        }

        // Settings toolbar (single row with popover buttons)
        if let Some(armor) = &pane.loaded_armor {
            if !armor.zone_parts.is_empty() {
                vp_ui.horizontal(|ui| {
                    // ── Armor Zones button with popover ──
                    let armor_btn = ui.button(format!("{} Armor", icons::SHIELD));
                    egui::Popup::from_toggle_button_response(&armor_btn)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            ui.horizontal(|ui| {
                                if ui.small_button("All").clicked() {
                                    pane.undo_stack.push(VisibilitySnapshot {
                                        part_visibility: pane.part_visibility.clone(),
                                        plate_visibility: pane.plate_visibility.clone(),
                                    });
                                    for (zone, parts) in &armor.zone_parts {
                                        for part in parts {
                                            pane.part_visibility.insert((zone.clone(), part.clone()), true);
                                        }
                                    }
                                    pane.plate_visibility.clear();
                                    zone_changed = true;
                                }
                                if ui.small_button("None").clicked() {
                                    pane.undo_stack.push(VisibilitySnapshot {
                                        part_visibility: pane.part_visibility.clone(),
                                        plate_visibility: pane.plate_visibility.clone(),
                                    });
                                    for (zone, parts) in &armor.zone_parts {
                                        for part in parts {
                                            pane.part_visibility.insert((zone.clone(), part.clone()), false);
                                        }
                                    }
                                    zone_changed = true;
                                }
                            });
                            // "Reset plates" clears plate-level overrides
                            if !pane.plate_visibility.is_empty() {
                                if ui.small_button("Reset plates").clicked() {
                                    pane.undo_stack.push(VisibilitySnapshot {
                                        part_visibility: pane.part_visibility.clone(),
                                        plate_visibility: pane.plate_visibility.clone(),
                                    });
                                    pane.plate_visibility.clear();
                                    zone_changed = true;
                                }
                            }
                            ui.separator();

                            // Three-level hierarchy: zone > material > plate
                            ui.set_min_width(250.0);
                            egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.6).show(
                                ui,
                                |ui| {
                                    ui.set_width(ui.available_width());
                                    let show_zero = pane.show_zero_mm;
                                    for (zone, parts_with_plates) in &armor.zone_part_plates {
                                        // Zone-level: check if all parts in zone are on
                                        let zone_all_on = parts_with_plates.iter().all(|(p, _)| {
                                            pane.part_visibility
                                                .get(&(zone.clone(), p.clone()))
                                                .copied()
                                                .unwrap_or(true)
                                        });
                                        let zone_any_on = parts_with_plates.iter().any(|(p, _)| {
                                            pane.part_visibility
                                                .get(&(zone.clone(), p.clone()))
                                                .copied()
                                                .unwrap_or(true)
                                        });

                                        let zone_id = ui.make_persistent_id(("armor_zone", zone));
                                        egui::collapsing_header::CollapsingState::load_with_default_open(
                                            ui.ctx(),
                                            zone_id,
                                            false,
                                        )
                                        .show_header(ui, |ui| {
                                            let mut checked = zone_all_on;
                                            let cb = ui.checkbox(&mut checked, "");
                                            if zone_any_on && !zone_all_on {
                                                let c = cb.rect.center();
                                                ui.painter().line_segment(
                                                    [egui::pos2(c.x - 3.5, c.y), egui::pos2(c.x + 3.5, c.y)],
                                                    egui::Stroke::new(2.0, ui.visuals().warn_fg_color),
                                                );
                                            }
                                            if cb.changed() {
                                                pane.undo_stack.push(VisibilitySnapshot {
                                                    part_visibility: pane.part_visibility.clone(),
                                                    plate_visibility: pane.plate_visibility.clone(),
                                                });
                                                let is_ctrl = ui.input(|i| i.modifiers.command);
                                                if is_ctrl {
                                                    // Solo: disable all zones, enable only this one
                                                    for (z, pp) in &armor.zone_part_plates {
                                                        let on = z == zone;
                                                        for (p, _) in pp {
                                                            pane.part_visibility.insert((z.clone(), p.clone()), on);
                                                        }
                                                    }
                                                } else {
                                                    for (p, _) in parts_with_plates {
                                                        pane.part_visibility.insert((zone.clone(), p.clone()), checked);
                                                    }
                                                }
                                                zone_changed = true;
                                            }
                                            ui.label(zone).on_hover_text("Ctrl+click to solo");
                                        })
                                        .body(|ui| {
                                            for (part, plates) in parts_with_plates {
                                                let part_key = (zone.clone(), part.clone());
                                                let part_on =
                                                    pane.part_visibility.get(&part_key).copied().unwrap_or(true);

                                                // Filter plates by 0mm setting
                                                let visible_plates: Vec<i32> =
                                                    plates.iter().copied().filter(|&t| show_zero || t != 0).collect();

                                                // Check if any plates are hidden at plate level
                                                let any_plate_hidden = visible_plates.iter().any(|&t| {
                                                    let pk: PlateKey = (zone.clone(), part.clone(), t);
                                                    pane.plate_visibility.get(&pk).copied() == Some(false)
                                                });

                                                let display = translate_part(part);

                                                if visible_plates.len() <= 1 {
                                                    // Single plate or no plates: just show material checkbox
                                                    let mut v = part_on && !any_plate_hidden;
                                                    if ui.checkbox(&mut v, &display).changed() {
                                                        pane.undo_stack.push(VisibilitySnapshot {
                                                            part_visibility: pane.part_visibility.clone(),
                                                            plate_visibility: pane.plate_visibility.clone(),
                                                        });
                                                        pane.part_visibility.insert(part_key.clone(), v);
                                                        // Clear plate-level overrides for this part
                                                        for &t in &visible_plates {
                                                            pane.plate_visibility.remove(&(
                                                                zone.clone(),
                                                                part.clone(),
                                                                t,
                                                            ));
                                                        }
                                                        zone_changed = true;
                                                    }
                                                } else {
                                                    // Multiple plates: collapsible material header
                                                    let mat_id = ui.make_persistent_id(("armor_mat", zone, part));
                                                    egui::collapsing_header::CollapsingState::load_with_default_open(
                                                        ui.ctx(),
                                                        mat_id,
                                                        false,
                                                    )
                                                    .show_header(ui, |ui| {
                                                        let mut checked = part_on && !any_plate_hidden;
                                                        let cb = ui.checkbox(&mut checked, "");
                                                        // Indeterminate: part enabled but some plates hidden
                                                        if part_on && any_plate_hidden {
                                                            let c = cb.rect.center();
                                                            ui.painter().line_segment(
                                                                [
                                                                    egui::pos2(c.x - 3.5, c.y),
                                                                    egui::pos2(c.x + 3.5, c.y),
                                                                ],
                                                                egui::Stroke::new(2.0, ui.visuals().warn_fg_color),
                                                            );
                                                        }
                                                        if cb.changed() {
                                                            pane.undo_stack.push(VisibilitySnapshot {
                                                                part_visibility: pane.part_visibility.clone(),
                                                                plate_visibility: pane.plate_visibility.clone(),
                                                            });
                                                            pane.part_visibility.insert(part_key.clone(), checked);
                                                            // Clear plate-level overrides
                                                            for &t in &visible_plates {
                                                                pane.plate_visibility.remove(&(
                                                                    zone.clone(),
                                                                    part.clone(),
                                                                    t,
                                                                ));
                                                            }
                                                            zone_changed = true;
                                                        }
                                                        ui.label(&display);
                                                    })
                                                    .body(|ui| {
                                                        for &thickness_i32 in &visible_plates {
                                                            let pk: PlateKey =
                                                                (zone.clone(), part.clone(), thickness_i32);
                                                            let plate_on =
                                                                pane.plate_visibility.get(&pk).copied().unwrap_or(true);
                                                            let thickness_mm = thickness_i32 as f32 / 10.0;

                                                            ui.horizontal(|ui| {
                                                                let color =
                                                                    wowsunpack::export::gltf_export::thickness_to_color(
                                                                        thickness_mm,
                                                                    );
                                                                paint_swatch(ui, color32_from_f32(color), 10.0);
                                                                let mut v = part_on && plate_on;
                                                                if ui
                                                                    .checkbox(&mut v, format!("{:.0} mm", thickness_mm))
                                                                    .changed()
                                                                {
                                                                    pane.undo_stack.push(VisibilitySnapshot {
                                                                        part_visibility: pane.part_visibility.clone(),
                                                                        plate_visibility: pane.plate_visibility.clone(),
                                                                    });
                                                                    pane.plate_visibility.insert(pk, !plate_on);
                                                                    zone_changed = true;
                                                                }
                                                            });
                                                        }
                                                    });
                                                }
                                            }
                                        });
                                    }
                                },
                            );
                        });

                    // ── Hull Model button with popover ──
                    if !armor.hull_part_groups.is_empty() {
                        let hull_btn = ui.button(format!("{} Hull", icons::CUBE));
                        egui::Popup::from_toggle_button_response(&hull_btn)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                let all_hull_names: Vec<&String> =
                                    armor.hull_part_groups.iter().flat_map(|(_, names)| names).collect();

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

                                ui.set_min_width(220.0);
                                egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.6).show(
                                    ui,
                                    |ui| {
                                        ui.set_width(ui.available_width());
                                        for (group, names) in &armor.hull_part_groups {
                                            let group_all_on = names
                                                .iter()
                                                .all(|n| pane.hull_visibility.get(n).copied().unwrap_or(false));
                                            let group_any_on = names
                                                .iter()
                                                .any(|n| pane.hull_visibility.get(n).copied().unwrap_or(false));

                                            let id = ui.make_persistent_id(("hull_group", group));
                                            egui::collapsing_header::CollapsingState::load_with_default_open(
                                                ui.ctx(),
                                                id,
                                                false,
                                            )
                                            .show_header(ui, |ui| {
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
                                                ui.label(group);
                                            })
                                            .body(|ui| {
                                                for name in names {
                                                    let mut visible =
                                                        pane.hull_visibility.get(name).copied().unwrap_or(false);
                                                    if ui.checkbox(&mut visible, name.as_str()).changed() {
                                                        pane.hull_visibility.insert(name.clone(), visible);
                                                        zone_changed = true;
                                                    }
                                                }
                                            });
                                        }
                                    },
                                );
                            });
                    }

                    // ── Display settings button with popover ──
                    let display_btn = ui.button(format!("{} Display", icons::GEAR_FINE));
                    egui::Popup::from_toggle_button_response(&display_btn)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            ui.set_min_width(160.0);
                            if ui.checkbox(&mut pane.show_plate_edges, "Plate Edges").changed() {
                                zone_changed = true;
                            }
                            if armor.draft_meters.is_some() {
                                if ui.checkbox(&mut pane.show_waterline, "Waterline").changed() {
                                    zone_changed = true;
                                }
                            }
                            if ui.checkbox(&mut pane.show_zero_mm, "0 mm Plates").changed() {
                                zone_changed = true;
                            }
                            ui.horizontal(|ui| {
                                ui.label("Opacity");
                                if ui
                                    .add(egui::Slider::new(&mut pane.armor_opacity, 0.1..=1.0).fixed_decimals(2))
                                    .changed()
                                {
                                    zone_changed = true;
                                }
                            });
                            ui.separator();
                            if ui.button("Save as defaults").clicked() {
                                save_defaults_signal.set(Some(ArmorViewerDefaults {
                                    show_plate_edges: pane.show_plate_edges,
                                    show_waterline: pane.show_waterline,
                                    show_zero_mm: pane.show_zero_mm,
                                    armor_opacity: pane.armor_opacity,
                                }));
                            }
                        });
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
                let mut hovered_plate_key: Option<PlateKey> = None;
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
                                let thickness_key = (tooltip.thickness_mm * 10.0).round() as i32;
                                hovered_plate_key =
                                    Some((tooltip.zone.clone(), tooltip.material_name.clone(), thickness_key));
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

                // Single click: toggle plate visibility (hide/show)
                if response.clicked() {
                    if let Some(ref key) = hovered_plate_key {
                        pane.undo_stack.push(VisibilitySnapshot {
                            part_visibility: pane.part_visibility.clone(),
                            plate_visibility: pane.plate_visibility.clone(),
                        });
                        let currently_visible = pane.plate_visibility.get(key).copied().unwrap_or(true);
                        pane.plate_visibility.insert(key.clone(), !currently_visible);
                        zone_changed = true;
                    }
                }

                // Latch context menu key on right-click; clear when menu closes.
                if response.secondary_clicked() {
                    pane.context_menu_key = hovered_plate_key.clone();
                } else if !context_menu_open {
                    pane.context_menu_key = None;
                }

                // Right-click context menu with plate-level and part-level actions.
                if let Some(ref ctx_key) = pane.context_menu_key.clone() {
                    let ctx_key = ctx_key.clone();
                    let part_key = (ctx_key.0.clone(), ctx_key.1.clone());
                    let thickness = ctx_key.2 as f32 / 10.0;
                    let ctx_name = translate_part(&ctx_key.1);
                    response.context_menu(|ui| {
                        // Plate-level: hide/show this specific plate
                        let plate_visible = pane.plate_visibility.get(&ctx_key).copied().unwrap_or(true);
                        let plate_label = if plate_visible {
                            format!("Hide {:.0} mm {}", thickness, ctx_name)
                        } else {
                            format!("Show {:.0} mm {}", thickness, ctx_name)
                        };
                        if ui.button(plate_label).clicked() {
                            pane.undo_stack.push(VisibilitySnapshot {
                                part_visibility: pane.part_visibility.clone(),
                                plate_visibility: pane.plate_visibility.clone(),
                            });
                            pane.plate_visibility.insert(ctx_key.clone(), !plate_visible);
                            zone_changed = true;
                            ui.close();
                        }

                        // Show all hidden plates
                        let hidden_count = pane.plate_visibility.values().filter(|&&v| !v).count();
                        if hidden_count > 0 {
                            if ui.button(format!("Show all hidden plates ({})", hidden_count)).clicked() {
                                pane.undo_stack.push(VisibilitySnapshot {
                                    part_visibility: pane.part_visibility.clone(),
                                    plate_visibility: pane.plate_visibility.clone(),
                                });
                                pane.plate_visibility.clear();
                                zone_changed = true;
                                ui.close();
                            }
                        }

                        ui.separator();

                        // Part-level: disable entire (zone, material_name)
                        if ui.button(format!("Disable {}", ctx_name)).clicked() {
                            pane.undo_stack.push(VisibilitySnapshot {
                                part_visibility: pane.part_visibility.clone(),
                                plate_visibility: pane.plate_visibility.clone(),
                            });
                            pane.part_visibility.insert(part_key, false);
                            zone_changed = true;
                            ui.close();
                        }
                    });
                }

                // Update hover highlight overlay
                let current_hover = pane.hover_highlight.as_ref().map(|(k, _)| k.clone());
                if hovered_plate_key != current_hover {
                    // Remove old hover highlight
                    if let Some((_, old_id)) = pane.hover_highlight.take() {
                        pane.viewport.remove_mesh(old_id);
                    }
                    // Upload new hover highlight
                    if let Some(ref key) = hovered_plate_key {
                        if let Some(armor) = pane.loaded_armor.take() {
                            let mesh_id =
                                upload_plate_highlight(pane, &armor, key, &render_state.device, [1.0, 1.0, 1.0, 0.35]);
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

        // Re-upload after context menu changes (the check at the top of this function
        // only catches zone-bar toggles; context menu sets zone_changed later).
        if zone_changed {
            if let Some(armor) = pane.loaded_armor.take() {
                upload_armor_to_viewport(pane, &armor, &render_state.device);
                pane.loaded_armor = Some(armor);
            }
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
    pane.plate_visibility.clear();

    let assets = ship_assets.clone();
    let ship_display_name = display_name.to_string();
    let (tx, rx) = mpsc::channel();

    // Resolve the Vehicle from GameParams on the main thread so we can use
    // load_ship_from_vehicle (avoids the fuzzy find_ship lookup entirely).
    use wowsunpack::game_params::types::GameParamProvider;
    let param = ship_assets.metadata().game_param_by_index(param_index);
    let vehicle = param.as_ref().and_then(|p| p.vehicle().cloned());
    let draft_meters = param.as_ref().and_then(|p| {
        p.vehicle()
            .and_then(|v| v.hull_upgrades())
            .and_then(|upgrades| upgrades.values().next())
            .and_then(|config| config.draft())
            .map(|m| m.value())
    });

    std::thread::spawn(move || {
        let result = (|| {
            let vehicle = vehicle.ok_or_else(|| format!("No vehicle found for param index"))?;
            let options =
                wowsunpack::export::ship::ShipExportOptions { lod: 0, hull: None, textures: false, damaged: false };

            let ctx = assets.load_ship_from_vehicle(&vehicle, &options).map_err(|e| format!("{e:?}"))?;

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

            // Build zone -> parts mapping and zone -> parts -> plates mapping
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

            // Build three-level hierarchy matching zone_parts order
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

            tracing::debug!("Ship loaded: draft={:?}, bounds Y=[{:.2}, {:.2}]", draft_meters, min[1], max[1]);

            Ok(LoadedShipArmor {
                display_name: ship_display_name,
                meshes,
                bounds: (min, max),
                zones,
                zone_parts,
                zone_part_plates,
                hull_meshes,
                hull_part_groups,
                draft_meters,
            })
        })();

        let _ = tx.send(result);
    });

    pane.load_receiver = Some(rx);
}

/// Helper to create an egui Color32 from an [f32; 4] RGBA color.
fn color32_from_f32(c: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, 255)
}

/// Paint a small color swatch rectangle inline.
fn paint_swatch(ui: &mut egui::Ui, color: egui::Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, color);
}

/// Show tooltip for a hovered armor triangle.
fn show_armor_tooltip(ui: &mut egui::Ui, info: &ArmorTriangleTooltip, translate: &dyn Fn(&str) -> String) {
    use wowsunpack::export::gltf_export::thickness_to_color;

    // Main header: this plate's thickness with color swatch
    ui.horizontal(|ui| {
        paint_swatch(ui, color32_from_f32(info.color), 12.0);
        ui.label(egui::RichText::new(format!("{:.0} mm", info.thickness_mm)).strong().size(14.0));
    });

    // Zone and part info
    ui.label(format!("{} / {}", &info.zone, translate(&info.material_name)));

    if info.layers.len() > 1 {
        ui.separator();
        ui.label(
            egui::RichText::new(format!("{} layers for this part:", info.layers.len()))
                .small()
                .color(egui::Color32::GRAY),
        );

        // Individual layers with swatches
        for &layer_mm in &info.layers {
            let layer_color = thickness_to_color(layer_mm);
            let is_this = (layer_mm - info.thickness_mm).abs() < 0.1;
            ui.horizontal(|ui| {
                paint_swatch(ui, color32_from_f32(layer_color), 10.0);
                let text = if is_this {
                    egui::RichText::new(format!("{:.0} mm", layer_mm)).strong()
                } else {
                    egui::RichText::new(format!("{:.0} mm", layer_mm))
                };
                ui.label(text);
            });
        }
    }
}

/// Upload a highlight overlay mesh for all triangles matching the given plate key
/// (zone, material_name, thickness). Returns the MeshId of the uploaded highlight mesh.
fn upload_plate_highlight(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    key: &PlateKey,
    device: &wgpu::Device,
    highlight_color: [f32; 4],
) -> MeshId {
    let normal_offset = 0.01; // slight offset along normal to avoid z-fighting

    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for mesh in &armor.meshes {
        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            if !pane.show_zero_mm && info.thickness_mm.abs() < 0.05 {
                continue;
            }

            let thickness_key = (info.thickness_mm * 10.0).round() as i32;
            if info.zone != key.0 || info.material_name != key.1 || thickness_key != key.2 {
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

/// Create a water plane quad at the given Y height, extending beyond the hull bounding box.
/// Returns (vertices, indices) for a semi-transparent blue quad.
fn create_water_plane(y: f32, bounds: ([f32; 3], [f32; 3])) -> (Vec<Vertex>, Vec<u32>) {
    let cx = (bounds.0[0] + bounds.1[0]) * 0.5;
    let cz = (bounds.0[2] + bounds.1[2]) * 0.5;
    let ex = (bounds.1[0] - bounds.0[0]) * 0.75;
    let ez = (bounds.1[2] - bounds.0[2]) * 0.75;

    let color = [0.1, 0.4, 0.8, 0.3];
    let normal = [0.0, 1.0, 0.0];

    let vertices = vec![
        Vertex { position: [cx - ex, y, cz - ez], normal, color },
        Vertex { position: [cx + ex, y, cz - ez], normal, color },
        Vertex { position: [cx + ex, y, cz + ez], normal, color },
        Vertex { position: [cx - ex, y, cz + ez], normal, color },
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];

    (vertices, indices)
}

/// Upload plate boundary edge outlines where adjacent triangles have different thickness values.
/// These appear as thin black lines along edges where two plates of different thickness meet.
fn upload_plate_boundary_edges(pane: &mut ArmorPane, armor: &LoadedShipArmor, device: &wgpu::Device) {
    use std::collections::HashMap;

    let edge_half_width: f32 = 0.003; // half-width of the edge quad in world space
    let normal_offset: f32 = 0.005; // offset to avoid z-fighting with the armor surface
    let edge_color: [f32; 4] = [0.0, 0.0, 0.0, 1.0]; // black

    // Quantize a float position to an integer key to avoid floating-point comparison issues.
    fn quantize(v: [f32; 3]) -> [i32; 3] {
        [(v[0] * 10000.0).round() as i32, (v[1] * 10000.0).round() as i32, (v[2] * 10000.0).round() as i32]
    }

    // Canonical edge key: sorted pair of quantized positions.
    type EdgeKey = ([i32; 3], [i32; 3]);
    fn make_edge_key(a: [i32; 3], b: [i32; 3]) -> EdgeKey {
        if a < b { (a, b) } else { (b, a) }
    }

    // For each edge, store the thickness and face normal from each side.
    struct EdgeInfo {
        thickness: f32,
        normal: [f32; 3],
        // Original (non-quantized) positions for rendering
        p0: [f32; 3],
        p1: [f32; 3],
    }

    let mut edge_map: HashMap<EdgeKey, Vec<EdgeInfo>> = HashMap::new();

    for mesh in &armor.meshes {
        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            // Skip 0mm plates unless explicitly enabled
            if !pane.show_zero_mm && info.thickness_mm.abs() < 0.05 {
                continue;
            }
            // Skip invisible parts and hidden plates
            let key = (info.zone.clone(), info.material_name.clone());
            if !pane.part_visibility.get(&key).copied().unwrap_or(true) {
                continue;
            }
            let plate_key: PlateKey =
                (info.zone.clone(), info.material_name.clone(), (info.thickness_mm * 10.0).round() as i32);
            if !pane.plate_visibility.get(&plate_key).copied().unwrap_or(true) {
                continue;
            }

            let base_idx = tri_idx * 3;
            if base_idx + 2 >= mesh.indices.len() {
                continue;
            }

            // Get transformed positions for this triangle
            let mut tri_pos = [[0.0_f32; 3]; 3];
            for k in 0..3 {
                let orig_idx = mesh.indices[base_idx + k] as usize;
                if orig_idx >= mesh.positions.len() {
                    continue;
                }
                let mut pos = mesh.positions[orig_idx];
                if let Some(t) = &mesh.transform {
                    pos = transform_point(t, pos);
                }
                tri_pos[k] = pos;
            }

            // Compute face normal
            let e1 = [tri_pos[1][0] - tri_pos[0][0], tri_pos[1][1] - tri_pos[0][1], tri_pos[1][2] - tri_pos[0][2]];
            let e2 = [tri_pos[2][0] - tri_pos[0][0], tri_pos[2][1] - tri_pos[0][1], tri_pos[2][2] - tri_pos[0][2]];
            let nx = e1[1] * e2[2] - e1[2] * e2[1];
            let ny = e1[2] * e2[0] - e1[0] * e2[2];
            let nz = e1[0] * e2[1] - e1[1] * e2[0];
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            let face_normal = if len > 1e-10 { [nx / len, ny / len, nz / len] } else { [0.0, 1.0, 0.0] };

            // For each of the 3 edges of this triangle
            let edges = [(0, 1), (1, 2), (2, 0)];
            for (a, b) in edges {
                let qa = quantize(tri_pos[a]);
                let qb = quantize(tri_pos[b]);
                let edge_key = make_edge_key(qa, qb);

                edge_map.entry(edge_key).or_default().push(EdgeInfo {
                    thickness: info.thickness_mm,
                    normal: face_normal,
                    p0: tri_pos[a],
                    p1: tri_pos[b],
                });
            }
        }
    }

    // Now find boundary edges: edges shared by triangles with different thickness values
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for (_edge_key, infos) in &edge_map {
        if infos.len() < 2 {
            continue; // boundary edge of mesh, not a plate boundary
        }

        // Check if there are different thickness values on this edge
        let first_thickness = infos[0].thickness;
        let has_boundary = infos.iter().any(|i| (i.thickness - first_thickness).abs() > 0.1);
        if !has_boundary {
            continue;
        }

        // Use the first info's positions and average normal from all sides
        let p0 = infos[0].p0;
        let p1 = infos[0].p1;

        // Average normal of all adjacent faces
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

        // Edge direction
        let edge_dir = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let edge_len = (edge_dir[0] * edge_dir[0] + edge_dir[1] * edge_dir[1] + edge_dir[2] * edge_dir[2]).sqrt();
        if edge_len < 1e-10 {
            continue;
        }

        // Tangent perpendicular to edge, in the surface plane
        // tangent = normalize(cross(edge_dir, avg_normal))
        let tx = edge_dir[1] * avg_normal[2] - edge_dir[2] * avg_normal[1];
        let ty = edge_dir[2] * avg_normal[0] - edge_dir[0] * avg_normal[2];
        let tz = edge_dir[0] * avg_normal[1] - edge_dir[1] * avg_normal[0];
        let t_len = (tx * tx + ty * ty + tz * tz).sqrt();
        if t_len < 1e-10 {
            continue;
        }
        let tangent = [tx / t_len, ty / t_len, tz / t_len];

        // Build a thin quad: 4 vertices offset along tangent and normal
        let base = vertices.len() as u32;
        for &p in &[p0, p1] {
            // Two vertices per endpoint, offset ±tangent and +normal
            for &sign in &[-1.0_f32, 1.0] {
                vertices.push(Vertex {
                    position: [
                        p[0] + tangent[0] * edge_half_width * sign + avg_normal[0] * normal_offset,
                        p[1] + tangent[1] * edge_half_width * sign + avg_normal[1] * normal_offset,
                        p[2] + tangent[2] * edge_half_width * sign + avg_normal[2] * normal_offset,
                    ],
                    normal: avg_normal,
                    color: edge_color,
                });
            }
        }
        // Two triangles forming the quad: (0,1,2), (1,3,2)
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }

    if !indices.is_empty() {
        pane.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_DEFAULT);
    }
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
