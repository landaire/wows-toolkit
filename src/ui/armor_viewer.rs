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
use crate::icon_str;
use crate::icons;
use crate::viewport_3d::{GpuPipeline, LAYER_DEFAULT, LAYER_HULL, LAYER_OVERLAY, MeshId, Vertex};

/// Color palette for distinguishing multiple trajectories in the 3D view.
const TRAJECTORY_PALETTE: [[f32; 4]; 8] = [
    [1.0, 0.8, 0.2, 1.0], // gold (original)
    [0.3, 0.7, 1.0, 1.0], // sky blue
    [1.0, 0.4, 0.4, 1.0], // coral
    [0.4, 0.9, 0.4, 1.0], // lime
    [1.0, 0.5, 0.8, 1.0], // pink
    [1.0, 0.6, 0.2, 1.0], // orange
    [0.3, 0.9, 0.9, 1.0], // cyan
    [0.7, 0.5, 1.0, 1.0], // lavender
];

/// Color palette for distinguishing comparison ships in detonation markers and UI labels.
const SHIP_COLORS: [[f32; 3]; 8] = [
    [1.0, 0.5, 0.1], // orange
    [0.3, 0.6, 1.0], // blue
    [1.0, 0.3, 0.5], // pink
    [0.3, 0.9, 0.4], // green
    [0.9, 0.3, 0.9], // magenta
    [1.0, 0.9, 0.2], // yellow
    [0.2, 0.9, 0.8], // teal
    [0.8, 0.5, 1.0], // purple
];

/// Per-frame viewer struct implementing `egui_dock::TabViewer` for armor panes.
struct ArmorPaneViewer<'a> {
    render_state: &'a eframe::egui_wgpu::RenderState,
    gpu_pipeline: &'a GpuPipeline,
    mirror_camera_signal: &'a std::cell::Cell<Option<u64>>,
    active_pane_signal: &'a std::cell::Cell<Option<u64>>,
    save_defaults_signal: &'a std::cell::Cell<Option<ArmorViewerDefaults>>,
    export_signal: &'a std::cell::Cell<Option<(String, String)>>,
    pen_check_toggle: &'a std::cell::Cell<bool>,
    comparison_ships: &'a [crate::armor_viewer::penetration::ComparisonShip],
    ifhe_enabled: bool,
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
            self.export_signal,
            self.pen_check_toggle,
            self.comparison_ships,
            self.ifhe_enabled,
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
                // Deferred export action from context menu.
                let deferred_export: std::cell::Cell<Option<(String, String)>> = std::cell::Cell::new(None);
                let deferred_export_ref = &deferred_export;

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
                                            let export_param_idx = ship.param_index.clone();
                                            let export_display_name = ship.display_name.clone();

                                            let leaf = egui_ltreeview::NodeBuilder::leaf(ship_id)
                                                .label(label)
                                                .context_menu(move |ui| {
                                                    if ui.button("Compare in new split").clicked() {
                                                        deferred_compare_ref
                                                            .set(Some((param_idx.clone(), display_name.clone())));
                                                        ui.close();
                                                    }
                                                    if ui
                                                        .button(icon_str!(icons::DOWNLOAD_SIMPLE, "Export Ship Model"))
                                                        .clicked()
                                                    {
                                                        deferred_export_ref.set(Some((
                                                            export_param_idx.clone(),
                                                            export_display_name.clone(),
                                                        )));
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

                // Handle deferred export from context menu
                if let Some(export_req) = deferred_export.take() {
                    state.export_confirm = Some(export_req);
                }
            }
        }

        // Render dock area with armor panes
        let save_defaults_cell: std::cell::Cell<Option<ArmorViewerDefaults>> = std::cell::Cell::new(None);
        let save_defaults_ref = &save_defaults_cell;
        let export_cell: std::cell::Cell<Option<(String, String)>> = std::cell::Cell::new(None);
        let export_ref = &export_cell;
        let pen_check_toggle_cell: std::cell::Cell<bool> = std::cell::Cell::new(false);
        let pen_check_toggle_ref = &pen_check_toggle_cell;
        let comparison_ships_snapshot = &state.comparison_ships;
        let ifhe_snapshot = state.ifhe_enabled;
        {
            let tab_count = state.dock_state.main_surface().num_tabs();
            let mut viewer = ArmorPaneViewer {
                render_state: &render_state,
                gpu_pipeline: &gpu_pipeline,
                mirror_camera_signal: if mirror_cameras { active_camera_ref } else { &std::cell::Cell::new(None) },
                active_pane_signal: active_pane_ref,
                save_defaults_signal: save_defaults_ref,
                export_signal: export_ref,
                pen_check_toggle: pen_check_toggle_ref,
                comparison_ships: comparison_ships_snapshot,
                ifhe_enabled: ifhe_snapshot,
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

        // Handle export signal from toolbar button
        if let Some(export_req) = export_cell.take() {
            state.export_confirm = Some(export_req);
        }

        // Handle pen check toggle from toolbar button
        if pen_check_toggle_cell.get() {
            state.show_comparison_panel = !state.show_comparison_panel;
        }

        // ── Penetration Comparison floating window ──
        if state.show_comparison_panel {
            let mut open = state.show_comparison_panel;
            egui::Window::new(icon_str!(icons::CROSSHAIR, "Penetration Check"))
                .id(egui::Id::new("pen_check_panel"))
                .open(&mut open)
                .collapsible(true)
                .resizable(true)
                .default_width(280.0)
                .show(ui.ctx(), |ui| {
                    // IFHE toggle
                    ui.checkbox(&mut state.ifhe_enabled, "IFHE (+25% HE penetration)");
                    ui.separator();

                    // Search bar
                    ui.horizontal(|ui| {
                        ui.label(icons::MAGNIFYING_GLASS);
                        ui.text_edit_singleline(&mut state.comparison_search);
                    });

                    // Search results
                    if !state.comparison_search.is_empty() {
                        if let Some(catalog) = &ship_catalog {
                            let search = unidecode::unidecode(&state.comparison_search).to_lowercase();
                            let already_added: std::collections::HashSet<&str> =
                                state.comparison_ships.iter().map(|s| s.param_index.as_str()).collect();

                            let mut results = Vec::new();
                            for nation in &catalog.nations {
                                for class in &nation.classes {
                                    for ship in &class.ships {
                                        if ship.search_name.contains(&search)
                                            && !already_added.contains(ship.param_index.as_str())
                                        {
                                            results.push(ship.clone());
                                        }
                                    }
                                }
                            }
                            results.truncate(10);

                            if !results.is_empty() {
                                egui::ScrollArea::vertical()
                                    .id_salt("pen_check_search_results")
                                    .max_height(150.0)
                                    .show(ui, |ui| {
                                        // Collect indices to add
                                        let mut to_add: Vec<String> = Vec::new();
                                        for ship in &results {
                                            let label = format!(
                                                "{} {}",
                                                crate::armor_viewer::ship_selector::tier_roman(ship.tier),
                                                &ship.display_name
                                            );
                                            if ui.button(label).clicked() {
                                                to_add.push(ship.param_index.clone());
                                                state.comparison_search.clear();
                                            }
                                        }
                                        // Process additions outside immutable borrow scope
                                        for param_idx in to_add {
                                            let wd = wows_data.read();
                                            if let Some(metadata) = wd.game_metadata.as_ref() {
                                                if let Some(comp_ship) =
                                                    crate::armor_viewer::penetration::resolve_ship_shells(
                                                        metadata, &param_idx,
                                                    )
                                                {
                                                    state.comparison_ships.push(comp_ship);
                                                }
                                            }
                                        }
                                    });
                            }
                        }
                    }

                    ui.separator();

                    // Added ships list
                    if state.comparison_ships.is_empty() {
                        ui.label(
                            egui::RichText::new("Search and add ships above to compare penetration")
                                .small()
                                .color(egui::Color32::GRAY),
                        );
                    } else {
                        let mut remove_idx: Option<usize> = None;
                        egui::ScrollArea::vertical().id_salt("pen_check_ships_list").max_height(300.0).show(ui, |ui| {
                            for (i, ship) in state.comparison_ships.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    if ui.small_button(icons::X).clicked() {
                                        remove_idx = Some(i);
                                    }
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} {}",
                                            crate::armor_viewer::ship_selector::tier_roman(ship.tier),
                                            &ship.display_name
                                        ))
                                        .strong(),
                                    );
                                });
                                for shell in &ship.shells {
                                    let pen_text = match shell.ammo_type.as_str() {
                                        "HE" => {
                                            let pen = shell.he_pen_mm.unwrap_or(0.0);
                                            format!(
                                                "  {} {:.0}mm — {:.0}mm pen",
                                                crate::armor_viewer::penetration::ammo_type_display(&shell.ammo_type),
                                                shell.caliber_mm,
                                                pen
                                            )
                                        }
                                        "CS" => {
                                            let pen = shell.sap_pen_mm.unwrap_or(0.0);
                                            format!(
                                                "  {} {:.0}mm — {:.0}mm pen",
                                                crate::armor_viewer::penetration::ammo_type_display(&shell.ammo_type),
                                                shell.caliber_mm,
                                                pen
                                            )
                                        }
                                        "AP" => {
                                            format!(
                                                "  {} {:.0}mm — {:.0} krupp",
                                                crate::armor_viewer::penetration::ammo_type_display(&shell.ammo_type),
                                                shell.caliber_mm,
                                                shell.krupp
                                            )
                                        }
                                        _ => String::new(),
                                    };
                                    ui.label(egui::RichText::new(pen_text).small());
                                }
                                ui.add_space(4.0);
                            }
                        });
                        if let Some(idx) = remove_idx {
                            state.comparison_ships.remove(idx);
                        }

                        if ui.button("Clear all").clicked() {
                            state.comparison_ships.clear();
                        }
                    }
                });
            state.show_comparison_panel = open;
        }

        // ── Export confirmation dialog ──
        let mut close_export_dialog = false;
        if let Some((ref param_index, ref display_name)) = state.export_confirm {
            let param_index = param_index.clone();
            let display_name = display_name.clone();
            egui::Window::new("Export Ship Model")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label("These 3D models and textures are IP of Wargaming and any usage of these models should be in compliance with your local laws.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Export").clicked() {
                            let default_filename = format!("{}.glb", display_name);
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name(&default_filename)
                                .add_filter("GLB", &["glb"])
                                .save_file()
                            {
                                let assets = ship_assets.clone();
                                let toasts = self.tab_state.toasts.clone();
                                let ship_name = display_name.clone();
                                let param_idx = param_index.clone();
                                std::thread::spawn(move || {
                                    let result = (|| -> Result<(), String> {
                                        use wowsunpack::game_params::types::GameParamProvider;
                                        let param = assets.metadata().game_param_by_index(&param_idx);
                                        let vehicle = param
                                            .as_ref()
                                            .and_then(|p| p.vehicle().cloned())
                                            .ok_or_else(|| "Vehicle not found".to_string())?;
                                        let options = wowsunpack::export::ship::ShipExportOptions {
                                            lod: 0,
                                            hull: None,
                                            textures: true,
                                            damaged: false,
                                        };
                                        let ctx = assets
                                            .load_ship_from_vehicle(&vehicle, &options)
                                            .map_err(|e| format!("{e:?}"))?;
                                        let mut file = std::fs::File::create(&path)
                                            .map_err(|e| format!("Failed to create file: {e}"))?;
                                        ctx.export_glb(&mut file)
                                            .map_err(|e| format!("Export failed: {e:?}"))?;
                                        Ok(())
                                    })();
                                    match result {
                                        Ok(()) => {
                                            toasts.lock().success(format!("Exported {}", ship_name));
                                        }
                                        Err(e) => {
                                            toasts.lock().error(format!("Export failed: {e}"));
                                        }
                                    }
                                });
                            }
                            close_export_dialog = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_export_dialog = true;
                        }
                    });
                });
        }
        if close_export_dialog {
            state.export_confirm = None;
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

            // Show Hidden mode: only show plates the in-game viewer hides
            if pane.show_hidden_only && !info.hidden {
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

    // Upload gap detection overlay
    if pane.show_gaps {
        pane.gap_count = upload_gap_edges(pane, armor, device);
    } else {
        pane.gap_count = 0;
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
        let (verts, indices) = create_water_plane(0.0, armor.bounds, pane.waterline_opacity);
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
    export_signal: &std::cell::Cell<Option<(String, String)>>,
    pen_check_toggle: &std::cell::Cell<bool>,
    comparison_ships: &[crate::armor_viewer::penetration::ComparisonShip],
    ifhe_enabled: bool,
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

        // Ctrl+T toggles trajectory mode
        if vp_ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::T)) {
            pane.trajectory_mode = !pane.trajectory_mode;
        }

        // Settings toolbar (single row with popover buttons)
        let prev_marker_opacity = pane.marker_opacity;
        let ctrl_s_pressed = vp_ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S));
        if let Some(armor) = &pane.loaded_armor {
            if !armor.zone_parts.is_empty() {
                vp_ui.horizontal(|ui| {
                    // ── Armor Zones button with popover ──
                    let armor_btn =
                        ui.button(icon_str!(icons::SHIELD, "Armor")).on_hover_text("Toggle armor zone visibility");
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
                                        // Zone-level: "all on" only if every part is enabled
                                        // AND no plates within any part are explicitly hidden.
                                        let zone_all_on = parts_with_plates.iter().all(|(p, plates)| {
                                            let part_on = pane
                                                .part_visibility
                                                .get(&(zone.clone(), p.clone()))
                                                .copied()
                                                .unwrap_or(true);
                                            if !part_on {
                                                return false;
                                            }
                                            !plates.iter().filter(|&&t| show_zero || t != 0).any(|&t| {
                                                let pk: PlateKey = (zone.clone(), p.clone(), t);
                                                pane.plate_visibility.get(&pk).copied() == Some(false)
                                            })
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
                        let hull_btn = ui
                            .button(icon_str!(icons::CUBE, "Hull"))
                            .on_hover_text("Toggle hull model part visibility");
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

                    // ── Show Hidden Plates toggle ──
                    if ui
                        .selectable_label(pane.show_hidden_only, icon_str!(icons::EYE_SLASH, "Show Hidden"))
                        .on_hover_text("Toggle (possibly) hidden panel visibility")
                        .clicked()
                    {
                        pane.show_hidden_only = !pane.show_hidden_only;
                        zone_changed = true;
                    }

                    // ── Gap Detection toggle ──
                    {
                        let gap_label = if pane.show_gaps && pane.gap_count > 0 {
                            format!("{} Gaps ({})", icons::WARNING, pane.gap_count)
                        } else if pane.show_gaps {
                            format!("{} Gaps (0)", icons::CHECK)
                        } else {
                            icon_str!(icons::WARNING, "Gaps").to_string()
                        };
                        let gap_color = if pane.show_gaps && pane.gap_count > 0 {
                            Some(egui::Color32::from_rgb(255, 100, 100))
                        } else if pane.show_gaps {
                            Some(egui::Color32::from_rgb(100, 200, 100))
                        } else {
                            None
                        };
                        let mut label = egui::RichText::new(gap_label);
                        if let Some(c) = gap_color {
                            label = label.color(c);
                        }
                        if ui
                            .selectable_label(pane.show_gaps, label)
                            .on_hover_text("Highlight boundary edges where armor panels don't connect (potential gaps)")
                            .clicked()
                        {
                            pane.show_gaps = !pane.show_gaps;
                            zone_changed = true;
                        }
                    }

                    // ── Display settings button with popover ──
                    let display_btn =
                        ui.button(icon_str!(icons::GEAR_FINE, "Display")).on_hover_text("Display settings (Ctrl+S)");
                    let display_popup_id = display_btn.id.with("display_popup");
                    // Ctrl+S opens display popover at mouse position
                    if ctrl_s_pressed {
                        egui::Popup::toggle_id(ui.ctx(), display_popup_id);
                    }
                    egui::Popup::from_toggle_button_response(&display_btn)
                        .id(display_popup_id)
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
                                if pane.show_waterline {
                                    ui.horizontal(|ui| {
                                        ui.add_space(20.0);
                                        ui.label("Opacity");
                                        if ui
                                            .add(
                                                egui::Slider::new(&mut pane.waterline_opacity, 0.05..=1.0)
                                                    .fixed_decimals(2),
                                            )
                                            .changed()
                                        {
                                            zone_changed = true;
                                        }
                                    });
                                }
                            }
                            if ui.checkbox(&mut pane.show_zero_mm, "0 mm Plates").changed() {
                                zone_changed = true;
                            }
                            ui.horizontal(|ui| {
                                ui.label("Armor Opacity");
                                if ui
                                    .add(egui::Slider::new(&mut pane.armor_opacity, 0.1..=1.0).fixed_decimals(2))
                                    .changed()
                                {
                                    zone_changed = true;
                                }
                            });
                            if !pane.trajectories.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.label("Marker Opacity");
                                    ui.add(egui::Slider::new(&mut pane.marker_opacity, 0.0..=1.0).fixed_decimals(2));
                                });
                            }
                            ui.separator();
                            if ui.button("Save as defaults").clicked() {
                                save_defaults_signal.set(Some(ArmorViewerDefaults {
                                    show_plate_edges: pane.show_plate_edges,
                                    show_waterline: pane.show_waterline,
                                    show_zero_mm: pane.show_zero_mm,
                                    armor_opacity: pane.armor_opacity,
                                    waterline_opacity: pane.waterline_opacity,
                                }));
                            }
                        });

                    // ── Export Ship Model button ──
                    if let Some(param_index) = &pane.selected_ship {
                        if ui
                            .button(icon_str!(icons::DOWNLOAD_SIMPLE, "Export"))
                            .on_hover_text("Export ship model to OBJ file")
                            .clicked()
                        {
                            let display_name = armor.display_name.clone();
                            export_signal.set(Some((param_index.clone(), display_name)));
                        }
                    }

                    // ── Penetration Check toggle ──
                    {
                        let label = if comparison_ships.is_empty() {
                            icon_str!(icons::CROSSHAIR, "Pen Check").to_string()
                        } else {
                            format!("{} Pen Check ({})", icons::CROSSHAIR, comparison_ships.len())
                        };
                        if ui.button(label).on_hover_text("Configure shells for penetration comparison").clicked() {
                            pen_check_toggle.set(true);
                        }
                    }

                    // ── Trajectory mode toggle ──
                    {
                        let traj_label = if pane.trajectory_mode { "Trajectory [ON]" } else { "Trajectory" };
                        let btn = egui::Button::new(traj_label);
                        let btn =
                            if pane.trajectory_mode { btn.fill(egui::Color32::from_rgb(80, 60, 20)) } else { btn };
                        if ui.add(btn).on_hover_text("Click armor to simulate shell trajectories (Ctrl+T)").clicked() {
                            pane.trajectory_mode = !pane.trajectory_mode;
                        }
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
            // Re-upload trajectory visualizations (viewport.clear() destroyed them)
            let cam_dist = pane.viewport.camera.distance;
            for traj in &mut pane.trajectories {
                let color = TRAJECTORY_PALETTE[traj.meta.color_index % TRAJECTORY_PALETTE.len()];
                let mesh_id = upload_trajectory_visualization(
                    &mut pane.viewport,
                    &traj.result,
                    &render_state.device,
                    color,
                    traj.last_visible_hit,
                    cam_dist,
                    pane.marker_opacity,
                );
                traj.mesh_id = Some(mesh_id);
                traj.marker_cam_dist = cam_dist;
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

                    // Re-upload trajectory markers if camera distance changed significantly
                    // so that marker/line sizes scale with zoom level.
                    let cam_dist = pane.viewport.camera.distance;
                    let needs_rescale = !pane.trajectories.is_empty()
                        && pane.trajectories.iter().any(|t| {
                            let ratio = cam_dist / t.marker_cam_dist.max(1e-6);
                            ratio < 0.7 || ratio > 1.4
                        });
                    if needs_rescale {
                        for traj in &mut pane.trajectories {
                            if let Some(old_mid) = traj.mesh_id.take() {
                                pane.viewport.remove_mesh(old_mid);
                            }
                            let color = TRAJECTORY_PALETTE[traj.meta.color_index % TRAJECTORY_PALETTE.len()];
                            traj.mesh_id = Some(upload_trajectory_visualization(
                                &mut pane.viewport,
                                &traj.result,
                                &render_state.device,
                                color,
                                traj.last_visible_hit,
                                cam_dist,
                                pane.marker_opacity,
                            ));
                            traj.marker_cam_dist = cam_dist;
                        }
                    }
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
                                        show_armor_tooltip(ui, tooltip, comparison_ships, ifhe_enabled, translate_part);
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

                // Trajectory mode: click to cast ray through model
                // Normal click = replace all trajectories; Shift+click = add another
                if pane.trajectory_mode && response.clicked() {
                    let shift_held = vp_ui.input(|i| i.modifiers.shift);
                    if let Some(click_pos) = response.interact_pointer_pos() {
                        use crate::viewport_3d::camera::{normalize, scale, sub};

                        // Step 1: Use camera ray to find the click point on the hull surface
                        let camera_hit = pane.viewport.pick(click_pos, response.rect);

                        if let Some(surface_hit) = camera_hit {
                            let click_point = surface_hit.world_position;
                            let range_m = pane.ballistic_range_km as f64 * 1000.0;

                            // Compute shell approach direction from the camera ray at the click point.
                            // Project onto XZ plane so the shell approaches horizontally from
                            // the camera's direction toward the clicked surface.
                            let approach_xz: [f32; 3] =
                                if let Some((_, cam_dir)) = pane.viewport.screen_to_ray(click_pos, response.rect) {
                                    let xz_len = (cam_dir[0] * cam_dir[0] + cam_dir[2] * cam_dir[2]).sqrt().max(1e-6);
                                    [cam_dir[0] / xz_len, 0.0, cam_dir[2] / xz_len]
                                } else {
                                    let cam_eye = pane.viewport.camera.eye_position();
                                    let cam_tgt = pane.viewport.camera.target;
                                    let dx = cam_tgt[0] - cam_eye[0];
                                    let dz = cam_tgt[2] - cam_eye[2];
                                    let len = (dx * dx + dz * dz).sqrt().max(1e-6);
                                    [dx / len, 0.0, dz / len]
                                };

                            // Try to get ballistic impact for the first AP shell (or any shell)
                            let first_shell = comparison_ships
                                .iter()
                                .flat_map(|s| s.shells.iter())
                                .find(|s| s.ammo_type == "AP")
                                .or_else(|| comparison_ships.iter().flat_map(|s| s.shells.iter()).next());

                            let ballistic_impact = if range_m > 0.0 {
                                first_shell.and_then(|shell| {
                                    let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
                                    crate::armor_viewer::ballistics::solve_for_range(&params, range_m)
                                })
                            } else {
                                None
                            };

                            // Step 2: Compute shell direction from ballistic impact angle
                            let shell_dir = if let Some(ref impact) = ballistic_impact {
                                let horiz_angle = impact.impact_angle_horizontal as f32;
                                let cos_h = horiz_angle.cos();
                                let sin_h = horiz_angle.sin();
                                normalize([
                                    approach_xz[0] * cos_h,
                                    -sin_h, // shell falls downward
                                    approach_xz[2] * cos_h,
                                ])
                            } else {
                                // Point-blank: use camera ray direction
                                pane.viewport
                                    .screen_to_ray(click_pos, response.rect)
                                    .map(|(_, d)| d)
                                    .unwrap_or([0.0, 0.0, -1.0])
                            };

                            // Step 3: Cast a ray from far behind the click point along shell_dir
                            // Origin = click_point - shell_dir * large_distance (so ray starts well behind)
                            let ray_origin = sub(click_point, scale(shell_dir, 50.0));
                            let all_hits = pane.viewport.pick_all_ray(ray_origin, shell_dir);

                            // Find the hit closest to the user's click_point and start from there.
                            // The shell-direction ray may hit earlier surfaces (e.g. deck) before
                            // reaching the plate the user actually clicked on.
                            let start_idx = all_hits
                                .iter()
                                .enumerate()
                                .min_by(|(_, (a, _)), (_, (b, _))| {
                                    let da = (a.world_position[0] - click_point[0]).powi(2)
                                        + (a.world_position[1] - click_point[1]).powi(2)
                                        + (a.world_position[2] - click_point[2]).powi(2);
                                    let db = (b.world_position[0] - click_point[0]).powi(2)
                                        + (b.world_position[1] - click_point[1]).powi(2)
                                        + (b.world_position[2] - click_point[2]).powi(2);
                                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                                })
                                .map(|(i, _)| i)
                                .unwrap_or(0);
                            let relevant_hits = &all_hits[start_idx..];

                            // Build trajectory result from hits
                            let mut traj_hits = Vec::new();
                            let first_dist = relevant_hits.first().map(|h| h.0.distance).unwrap_or(0.0);
                            for (hit, normal) in relevant_hits {
                                let tooltip = pane
                                    .mesh_triangle_info
                                    .iter()
                                    .find(|(id, _)| *id == hit.mesh_id)
                                    .and_then(|(_, infos)| infos.get(hit.triangle_index));

                                if let Some(info) = tooltip {
                                    let angle = crate::armor_viewer::penetration::impact_angle_deg(&shell_dir, normal);
                                    traj_hits.push(crate::armor_viewer::penetration::TrajectoryHit {
                                        position: hit.world_position,
                                        thickness_mm: info.thickness_mm,
                                        zone: info.zone.clone(),
                                        material: info.material_name.clone(),
                                        angle_deg: angle,
                                        distance_from_start: hit.distance - first_dist,
                                    });
                                }
                            }

                            // Generate 3D arc points if we have ballistic data
                            let arc_points_3d = if let Some(ref impact) = ballistic_impact {
                                if let Some(shell) = first_shell {
                                    let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
                                    let (arc_2d, height_ratio) = crate::armor_viewer::ballistics::simulate_arc_points(
                                        &params,
                                        impact.launch_angle,
                                        60,
                                    );
                                    let model_extent = pane
                                        .loaded_armor
                                        .as_ref()
                                        .map(|a| {
                                            let dx = a.bounds.1[0] - a.bounds.0[0];
                                            let dz = a.bounds.1[2] - a.bounds.0[2];
                                            dx.max(dz)
                                        })
                                        .unwrap_or(10.0);
                                    let arc_horiz_extent = model_extent * 2.0;
                                    // Use real height ratio with a minimum so flat arcs are still visible
                                    let arc_height_extent = arc_horiz_extent * (height_ratio as f32).max(0.02);
                                    let first_hit_pos = traj_hits.first().map(|h| h.position).unwrap_or(click_point);
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
                                } else {
                                    Vec::new()
                                }
                            } else {
                                Vec::new()
                            };

                            let total_armor: f32 = traj_hits.iter().map(|h| h.thickness_mm).sum();

                            // Compute detonation points and last visible hit for AP shells
                            // Each shell gets its own ballistic solve (different velocity/angle at range)
                            let mut detonation_points: Vec<crate::armor_viewer::penetration::DetonationMarker> =
                                Vec::new();
                            let mut last_visible_hit: Option<usize> = None;
                            let range_m_f64 = pane.ballistic_range_km as f64 * 1000.0;
                            for (ship_idx, ship) in comparison_ships.iter().enumerate() {
                                for shell in ship.shells.iter().filter(|s| s.ammo_type == "AP") {
                                    let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
                                    let shell_impact = if range_m_f64 > 0.0 {
                                        crate::armor_viewer::ballistics::solve_for_range(&params, range_m_f64)
                                    } else {
                                        None
                                    };
                                    if let Some(ref impact) = shell_impact {
                                        let sim = crate::armor_viewer::penetration::simulate_shell_through_hits(
                                            &params, impact, &traj_hits, &shell_dir,
                                        );
                                        if let Some(det) = sim.detonation {
                                            detonation_points.push(
                                                crate::armor_viewer::penetration::DetonationMarker {
                                                    position: det.position,
                                                    ship_index: ship_idx,
                                                },
                                            );
                                        }
                                        // Earliest terminating event: detonation or ricochet/shatter
                                        let shell_stop = match (sim.detonated_at, sim.stopped_at) {
                                            (Some(d), Some(s)) => Some(d.min(s)),
                                            (Some(d), None) => Some(d),
                                            (None, Some(s)) => Some(s),
                                            (None, None) => None,
                                        };
                                        if let Some(idx) = shell_stop {
                                            last_visible_hit =
                                                Some(last_visible_hit.map_or(idx, |prev: usize| prev.min(idx)));
                                        }
                                    }
                                }
                            }

                            let result = crate::armor_viewer::penetration::TrajectoryResult {
                                origin: ray_origin,
                                direction: shell_dir,
                                hits: traj_hits,
                                total_armor_mm: total_armor,
                                arc_points_3d,
                                ballistic_impact,
                                detonation_points,
                            };

                            // Shift+click = add; normal click = replace all
                            if !shift_held {
                                for traj in pane.trajectories.drain(..) {
                                    if let Some(mid) = traj.mesh_id {
                                        pane.viewport.remove_mesh(mid);
                                    }
                                }
                            }

                            let traj_id = pane.next_trajectory_id;
                            pane.next_trajectory_id += 1;
                            let color_index = pane.trajectories.len() % TRAJECTORY_PALETTE.len();
                            let color = TRAJECTORY_PALETTE[color_index];
                            let cam_dist = pane.viewport.camera.distance;
                            let mesh_id = upload_trajectory_visualization(
                                &mut pane.viewport,
                                &result,
                                &render_state.device,
                                color,
                                last_visible_hit,
                                cam_dist,
                                pane.marker_opacity,
                            );
                            pane.trajectories.push(crate::armor_viewer::state::StoredTrajectory {
                                meta: crate::armor_viewer::penetration::TrajectoryMeta {
                                    id: traj_id,
                                    color_index,
                                    range_km: pane.ballistic_range_km,
                                    range_locked: true,
                                },
                                result,
                                mesh_id: Some(mesh_id),
                                last_visible_hit,
                                marker_cam_dist: cam_dist,
                                show_plates_active: false,
                                show_zones_active: false,
                            });
                        } else if !shift_held {
                            // Clicked empty space without shift: clear all
                            for traj in pane.trajectories.drain(..) {
                                if let Some(mid) = traj.mesh_id {
                                    pane.viewport.remove_mesh(mid);
                                }
                            }
                        }
                    }
                }

                // Single click: toggle plate visibility (hide/show) — skip in trajectory mode
                if !pane.trajectory_mode && response.clicked() {
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

                // ── Trajectory results floating panel ──
                let clear_traj = std::cell::Cell::new(false);
                let delete_traj_id: std::cell::Cell<Option<u64>> = std::cell::Cell::new(None);
                let range_km_cell = std::cell::Cell::new(pane.ballistic_range_km);
                // Collect per-trajectory range changes: (traj_index, new_range_km, force_locked: Option<bool>)
                // force_locked = Some(true/false) from checkbox toggle, None from slider drag
                let traj_range_changes: std::cell::RefCell<Vec<(usize, f32, Option<bool>)>> =
                    std::cell::RefCell::new(Vec::new());
                let show_all_hit_plates = std::cell::Cell::new(false);
                let show_all_hit_zones = std::cell::Cell::new(false);
                // Per-arc plate/zone isolation toggles: (arc_index, new_active_state)
                let arc_plate_toggles: std::cell::RefCell<Vec<(usize, bool)>> = std::cell::RefCell::new(Vec::new());
                let arc_zone_toggles: std::cell::RefCell<Vec<(usize, bool)>> = std::cell::RefCell::new(Vec::new());

                if !pane.trajectories.is_empty() {
                    let traj_id = egui::Id::new(("trajectory_results", pane_id));
                    let traj_count_for_width = pane.trajectories.len();
                    let window_width = traj_count_for_width as f32 * 330.0 + 20.0;
                    egui::Window::new("Trajectory Analysis")
                        .id(traj_id)
                        .collapsible(true)
                        .resizable(true)
                        .default_width(window_width)
                        .default_pos(egui::pos2(
                            response.rect.right() - window_width - 10.0,
                            response.rect.top() + 40.0,
                        ))
                        .show(vp_ui.ctx(), |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("This simulation is based on reverse engineered data and may not accurately reflect how the game simulates ballistics.")
                                        .small()
                                        .color(egui::Color32::from_rgb(220, 160, 60)),
                                );
                            });
                            ui.separator();

                            // Shared range slider
                            if !comparison_ships.is_empty() {
                                let mut range_km = range_km_cell.get();
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Range:").small().color(egui::Color32::GRAY));
                                    ui.add(
                                        egui::Slider::new(&mut range_km, 0.0..=30.0)
                                            .suffix(" km")
                                            .step_by(0.5)
                                            .max_decimals(1),
                                    );
                                });
                                range_km_cell.set(range_km);
                            }

                            ui.horizontal(|ui| {
                                if ui.button("Clear All").clicked() {
                                    clear_traj.set(true);
                                }
                                if ui.button("Show Hit Plates").on_hover_text("Isolate only armor plates hit by all trajectories").clicked() {
                                    show_all_hit_plates.set(true);
                                }
                                if ui.button("Show Hit Zones").on_hover_text("Isolate entire armor zones hit by all trajectories").clicked() {
                                    show_all_hit_zones.set(true);
                                }
                                // Angle color legend
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("\u{25CF}").color(egui::Color32::from_rgb(100, 220, 100)));
                                ui.label(egui::RichText::new("<30\u{00B0}").small().color(egui::Color32::GRAY));
                                ui.label(egui::RichText::new("\u{25CF}").color(egui::Color32::from_rgb(220, 180, 80)));
                                ui.label(egui::RichText::new("30-45\u{00B0}").small().color(egui::Color32::GRAY));
                                ui.label(egui::RichText::new("\u{25CF}").color(egui::Color32::from_rgb(220, 100, 100)));
                                ui.label(egui::RichText::new(">45\u{00B0}").small().color(egui::Color32::GRAY));
                            });

                            ui.separator();

                            let traj_count = pane.trajectories.len();

                            // Helper: render one trajectory column
                            let render_traj_column =
                                |ui: &mut egui::Ui, ti: usize, traj: &crate::armor_viewer::state::StoredTrajectory| {
                                    let result = &traj.result;
                                    let palette_color =
                                        TRAJECTORY_PALETTE[traj.meta.color_index % TRAJECTORY_PALETTE.len()];
                                    let header_color = egui::Color32::from_rgba_unmultiplied(
                                        (palette_color[0] * 255.0) as u8,
                                        (palette_color[1] * 255.0) as u8,
                                        (palette_color[2] * 255.0) as u8,
                                        255,
                                    );

                                    // Header line with color swatch
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("Arc {}", ti + 1)).strong().color(header_color),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{} hits, {:.0}mm @ {:.1}km",
                                                result.hits.len(),
                                                result.total_armor_mm,
                                                traj.meta.range_km,
                                            ))
                                            .small()
                                            .color(egui::Color32::GRAY),
                                        );
                                    });

                                    // Per-trajectory range control
                                    ui.horizontal(|ui| {
                                        let mut locked = traj.meta.range_locked;
                                        if ui.checkbox(&mut locked, "").changed() {
                                            traj_range_changes.borrow_mut().push((
                                                ti,
                                                if locked { range_km_cell.get() } else { traj.meta.range_km },
                                                Some(locked),
                                            ));
                                        }
                                        if locked {
                                            ui.label(
                                                egui::RichText::new(format!("{:.1} km", traj.meta.range_km))
                                                    .small()
                                                    .color(egui::Color32::GRAY),
                                            );
                                        } else {
                                            let mut rng = traj.meta.range_km;
                                            let resp = ui.add(
                                                egui::Slider::new(&mut rng, 0.0..=30.0)
                                                    .suffix(" km")
                                                    .step_by(0.5)
                                                    .max_decimals(1)
                                                    .text(""),
                                            );
                                            if resp.changed() {
                                                traj_range_changes.borrow_mut().push((ti, rng, None));
                                            }
                                        }
                                    });

                                    // Ballistic impact info
                                    if let Some(ref impact) = result.ballistic_impact {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "v={:.0} m/s  t={:.1}s  fall={:.1}\u{00B0}",
                                                impact.impact_velocity,
                                                impact.time_to_target,
                                                impact.impact_angle_horizontal.to_degrees(),
                                            ))
                                            .small()
                                            .color(egui::Color32::from_rgb(180, 180, 220)),
                                        );
                                    }

                                    // Compute shell simulations for this trajectory
                                    let range_m = traj.meta.range_km as f64 * 1000.0;

                                    struct ShellSim {
                                        ship_name: String,
                                        ship_index: usize,
                                        shell: crate::armor_viewer::penetration::ShellInfo,
                                        sim: Option<crate::armor_viewer::penetration::ShellSimResult>,
                                    }

                                    let shell_sims: Vec<ShellSim> = comparison_ships
                                        .iter()
                                        .enumerate()
                                        .flat_map(|(si, ship)| {
                                            ship.shells.iter().map(move |shell| {
                                                let params =
                                                    crate::armor_viewer::ballistics::ShellParams::from_shell_info(
                                                        shell,
                                                    );
                                                let impact = if shell.ammo_type == "AP" && range_m > 0.0 {
                                                    crate::armor_viewer::ballistics::solve_for_range(&params, range_m)
                                                } else {
                                                    None
                                                };
                                                let sim =
                                                    if shell.ammo_type == "AP" {
                                                        impact.as_ref().map(|imp| {
                                                    crate::armor_viewer::penetration::simulate_shell_through_hits(
                                                        &params, imp, &result.hits, &result.direction,
                                                    )
                                                })
                                                    } else {
                                                        None
                                                    };
                                                ShellSim {
                                                    ship_name: ship.display_name.clone(),
                                                    ship_index: si,
                                                    shell: shell.clone(),
                                                    sim,
                                                }
                                            })
                                        })
                                        .collect();

                                    // Find last visible hit (earliest terminating event across all shells)
                                    let last_visible_hit: Option<usize> = shell_sims
                                        .iter()
                                        .filter_map(|ss| {
                                            ss.sim.as_ref().and_then(|s| match (s.detonated_at, s.stopped_at) {
                                                (Some(d), Some(s)) => Some(d.min(s)),
                                                (Some(d), None) => Some(d),
                                                (None, Some(s)) => Some(s),
                                                (None, None) => None,
                                            })
                                        })
                                        .min();

                                    // Outcome badges per shell
                                    for ss in &shell_sims {
                                        let ammo =
                                            crate::armor_viewer::penetration::ammo_type_display(&ss.shell.ammo_type);
                                        let shell_label =
                                            format!("{} {} {:.0}mm", &ss.ship_name, ammo, ss.shell.caliber_mm);
                                        if let Some(ref sim) = ss.sim {
                                            use crate::armor_viewer::penetration::PlateOutcome;
                                            let (icon, badge_color, outcome_text) =
                                                if let Some(det_idx) = sim.detonated_at {
                                                    // Shell detonated — show which plate
                                                    let plate_desc = result
                                                        .hits
                                                        .get(det_idx)
                                                        .map(|h| {
                                                            format!(
                                                                "#{} {:.0}mm {}",
                                                                det_idx + 1,
                                                                h.thickness_mm,
                                                                translate_part(&h.material)
                                                            )
                                                        })
                                                        .unwrap_or_default();
                                                    (
                                                        icons::BOMB,
                                                        egui::Color32::from_rgb(255, 140, 40),
                                                        format!("detonation @ {}", plate_desc),
                                                    )
                                                } else if let Some(stop_idx) = sim.stopped_at {
                                                    // Shell stopped — show which plate and why
                                                    let plate_desc = result
                                                        .hits
                                                        .get(stop_idx)
                                                        .map(|h| {
                                                            format!(
                                                                "#{} {:.0}mm {}",
                                                                stop_idx + 1,
                                                                h.thickness_mm,
                                                                translate_part(&h.material)
                                                            )
                                                        })
                                                        .unwrap_or_default();
                                                    let last_outcome = sim.plates.last().map(|p| &p.outcome);
                                                    match last_outcome {
                                                        Some(PlateOutcome::Ricochet) => (
                                                            icons::PROHIBIT,
                                                            egui::Color32::from_rgb(220, 100, 100),
                                                            format!("ricochet @ {}", plate_desc),
                                                        ),
                                                        Some(PlateOutcome::Shatter) => (
                                                            icons::X_CIRCLE,
                                                            egui::Color32::from_rgb(220, 100, 100),
                                                            format!("shatter @ {}", plate_desc),
                                                        ),
                                                        _ => (
                                                            icons::X_CIRCLE,
                                                            egui::Color32::from_rgb(220, 100, 100),
                                                            format!("stopped @ {}", plate_desc),
                                                        ),
                                                    }
                                                } else if sim.detonation.is_some() {
                                                    // Fuse armed but shell exited before detonation = overpen
                                                    (
                                                        icons::ARROWS_OUT_SIMPLE,
                                                        egui::Color32::from_rgb(220, 180, 80),
                                                        "overpen".to_string(),
                                                    )
                                                } else {
                                                    // Fuse never armed, shell passed through = overpen
                                                    (
                                                        icons::ARROWS_OUT_SIMPLE,
                                                        egui::Color32::from_rgb(220, 180, 80),
                                                        "overpen (fuse never armed)".to_string(),
                                                    )
                                                };

                                            ui.horizontal(|ui| {
                                                let sc = SHIP_COLORS[ss.ship_index % SHIP_COLORS.len()];
                                                let ship_dot_color = egui::Color32::from_rgb(
                                                    (sc[0] * 255.0) as u8, (sc[1] * 255.0) as u8, (sc[2] * 255.0) as u8,
                                                );
                                                ui.label(egui::RichText::new("\u{25CF}").color(ship_dot_color));
                                                ui.label(egui::RichText::new(icon).color(badge_color));
                                                ui.label(
                                                    egui::RichText::new(format!("{} — {}", shell_label, outcome_text))
                                                        .small()
                                                        .strong()
                                                        .color(badge_color),
                                                );
                                            });
                                        }
                                    }

                                    ui.separator();

                                    for (i, hit) in result.hits.iter().enumerate() {
                                        let is_post_detonation = last_visible_hit.map_or(false, |lv| i > lv);

                                        // Skip ghost plates that have no detonation event on them
                                        if is_post_detonation {
                                            let has_detonation_here = shell_sims.iter().any(|ss| {
                                                ss.sim.as_ref().map_or(false, |sim| sim.detonated_at == Some(i))
                                            });
                                            if !has_detonation_here {
                                                continue;
                                            }
                                        }

                                        let color = if is_post_detonation {
                                            egui::Color32::from_rgb(100, 100, 100)
                                        } else if hit.angle_deg < 30.0 {
                                            egui::Color32::from_rgb(100, 220, 100)
                                        } else if hit.angle_deg < 45.0 {
                                            egui::Color32::from_rgb(220, 180, 80)
                                        } else {
                                            egui::Color32::from_rgb(220, 100, 100)
                                        };

                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(format!("#{}", i + 1))
                                                    .small()
                                                    .color(egui::Color32::GRAY),
                                            );
                                            ui.label(
                                                egui::RichText::new(format!("{:.0} mm", hit.thickness_mm))
                                                    .strong()
                                                    .color(color),
                                            );
                                            ui.label(
                                                egui::RichText::new(format!("{:.1}\u{00B0}", hit.angle_deg))
                                                    .small()
                                                    .color(color),
                                            );

                                        });
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "  {} / {}",
                                                &hit.zone,
                                                translate_part(&hit.material)
                                            ))
                                            .small()
                                            .color(egui::Color32::GRAY),
                                        );

                                        if !is_post_detonation {
                                            for ss in &shell_sims {
                                                if let Some(ref sim) = ss.sim {
                                                    if let Some(plate) = sim.plates.get(i) {
                                                        use crate::armor_viewer::penetration::PlateOutcome;
                                                        let (icon, detail_color, detail) = match plate.outcome {
                                                            PlateOutcome::Overmatch => (
                                                                "\u{2705}",
                                                                egui::Color32::from_rgb(100, 220, 100),
                                                                format!(
                                                                    "overmatch \u{2014} {:.0}mm pen, v={:.0} m/s",
                                                                    plate.raw_pen_before_mm, plate.velocity_after
                                                                ),
                                                            ),
                                                            PlateOutcome::Penetrate => (
                                                                "\u{2705}",
                                                                egui::Color32::from_rgb(100, 220, 100),
                                                                format!(
                                                                    "{:.0}/{:.0}mm eff \u{2014} v={:.0} m/s",
                                                                    plate.raw_pen_before_mm,
                                                                    plate.effective_thickness_mm,
                                                                    plate.velocity_after
                                                                ),
                                                            ),
                                                            PlateOutcome::Ricochet => (
                                                                "\u{274C}",
                                                                egui::Color32::from_rgb(220, 100, 100),
                                                                format!("ricochet @ {:.1}\u{00B0}", hit.angle_deg),
                                                            ),
                                                            PlateOutcome::Shatter => (
                                                                "\u{274C}",
                                                                egui::Color32::from_rgb(220, 100, 100),
                                                                format!(
                                                                    "shatter \u{2014} {:.0} < {:.0}mm eff",
                                                                    plate.raw_pen_before_mm,
                                                                    plate.effective_thickness_mm
                                                                ),
                                                            ),
                                                        };

                                                        ui.horizontal(|ui| {
                                                            ui.add_space(12.0);
                                                            let sc = SHIP_COLORS[ss.ship_index % SHIP_COLORS.len()];
                                                            ui.label(egui::RichText::new("\u{25CF}").color(
                                                                egui::Color32::from_rgb(
                                                                    (sc[0] * 255.0) as u8, (sc[1] * 255.0) as u8, (sc[2] * 255.0) as u8,
                                                                ),
                                                            ));
                                                            ui.label(egui::RichText::new(icon));
                                                            let mut label_text = format!(
                                                                "{} {} {:.0}mm",
                                                                &ss.ship_name,
                                                                crate::armor_viewer::penetration::ammo_type_display(
                                                                    &ss.shell.ammo_type
                                                                ),
                                                                ss.shell.caliber_mm,
                                                            );
                                                            if plate.fuse_armed_here {
                                                                label_text.push_str(&format!(" {}", icons::BOMB));
                                                            }
                                                            ui.label(
                                                                egui::RichText::new(label_text)
                                                                    .small()
                                                                    .color(detail_color),
                                                            );
                                                        });
                                                        ui.horizontal(|ui| {
                                                            ui.add_space(28.0);
                                                            ui.label(
                                                                egui::RichText::new(detail)
                                                                    .small()
                                                                    .color(egui::Color32::GRAY),
                                                            );
                                                        });
                                                    }
                                                } else if i == 0 {
                                                    // HE/SAP on first hit
                                                    let (icon, detail_color, detail) = match ss.shell.ammo_type.as_str()
                                                    {
                                                        "HE" => {
                                                            let pen = if ifhe_enabled {
                                                                ss.shell.he_pen_mm.unwrap_or(0.0) * 1.25
                                                            } else {
                                                                ss.shell.he_pen_mm.unwrap_or(0.0)
                                                            };
                                                            if pen >= hit.thickness_mm {
                                                                (
                                                                    icons::FIRE,
                                                                    egui::Color32::from_rgb(255, 140, 40),
                                                                    format!("{:.0}mm pen \u{2014} detonates", pen),
                                                                )
                                                            } else {
                                                                (
                                                                    "\u{274C}",
                                                                    egui::Color32::from_rgb(220, 100, 100),
                                                                    format!(
                                                                        "{:.0}mm pen < {:.0}mm",
                                                                        pen, hit.thickness_mm
                                                                    ),
                                                                )
                                                            }
                                                        }
                                                        "CS" => {
                                                            let pen = ss.shell.sap_pen_mm.unwrap_or(0.0);
                                                            if pen >= hit.thickness_mm {
                                                                (
                                                                    icons::SHIELD_STAR,
                                                                    egui::Color32::from_rgb(255, 140, 40),
                                                                    format!("{:.0}mm pen \u{2014} detonates", pen),
                                                                )
                                                            } else {
                                                                (
                                                                    "\u{274C}",
                                                                    egui::Color32::from_rgb(220, 100, 100),
                                                                    format!(
                                                                        "{:.0}mm pen < {:.0}mm",
                                                                        pen, hit.thickness_mm
                                                                    ),
                                                                )
                                                            }
                                                        }
                                                        _ => ("\u{2796}", egui::Color32::GRAY, "unknown".to_string()),
                                                    };
                                                    ui.horizontal(|ui| {
                                                        ui.add_space(12.0);
                                                        let sc = SHIP_COLORS[ss.ship_index % SHIP_COLORS.len()];
                                                        ui.label(egui::RichText::new("\u{25CF}").color(
                                                            egui::Color32::from_rgb(
                                                                (sc[0] * 255.0) as u8, (sc[1] * 255.0) as u8, (sc[2] * 255.0) as u8,
                                                            ),
                                                        ));
                                                        ui.label(egui::RichText::new(icon).color(detail_color));
                                                        ui.label(
                                                            egui::RichText::new(format!(
                                                                "{} {} {:.0}mm",
                                                                &ss.ship_name,
                                                                crate::armor_viewer::penetration::ammo_type_display(
                                                                    &ss.shell.ammo_type
                                                                ),
                                                                ss.shell.caliber_mm,
                                                            ))
                                                            .small()
                                                            .color(detail_color),
                                                        );
                                                    });
                                                    ui.horizontal(|ui| {
                                                        ui.add_space(28.0);
                                                        ui.label(
                                                            egui::RichText::new(detail)
                                                                .small()
                                                                .color(egui::Color32::GRAY),
                                                        );
                                                    });
                                                }
                                            }
                                        }

                                        // Inline detonation markers: after hit #i, show any shells that detonate here
                                        for ss in &shell_sims {
                                            if let Some(ref sim) = ss.sim {
                                                if sim.detonated_at == Some(i) {
                                                    if let Some(ref det) = sim.detonation {
                                                        ui.horizontal(|ui| {
                                                            ui.add_space(8.0);
                                                            let sc = SHIP_COLORS[ss.ship_index % SHIP_COLORS.len()];
                                                            ui.label(egui::RichText::new("\u{25CF}").color(
                                                                egui::Color32::from_rgb(
                                                                    (sc[0] * 255.0) as u8, (sc[1] * 255.0) as u8, (sc[2] * 255.0) as u8,
                                                                ),
                                                            ));
                                                            ui.label(
                                                                egui::RichText::new(icons::BOMB)
                                                                    .color(egui::Color32::from_rgb(255, 140, 40)),
                                                            );
                                                            ui.label(
                                                                egui::RichText::new(format!(
                                                                    "{} {} detonates \u{2014} {:.1}m after plate #{}",
                                                                    &ss.ship_name,
                                                                    crate::armor_viewer::penetration::ammo_type_display(
                                                                        &ss.shell.ammo_type
                                                                    ),
                                                                    det.travel_distance,
                                                                    det.armed_at_hit + 1,
                                                                ))
                                                                .small()
                                                                .strong()
                                                                .color(egui::Color32::from_rgb(255, 140, 40)),
                                                            );
                                                        });
                                                    }
                                                }
                                            }
                                        }

                                        if i + 1 < result.hits.len() {
                                            ui.separator();
                                        }
                                    }

                                    ui.separator();
                                    ui.horizontal(|ui| {
                                        if ui.button("Delete").clicked() {
                                            delete_traj_id.set(Some(traj.meta.id));
                                        }
                                        let btn_label = if traj.show_plates_active {
                                            egui::RichText::new("Isolate Plates").color(egui::Color32::from_rgb(100, 220, 100))
                                        } else {
                                            egui::RichText::new("Isolate Plates")
                                        };
                                        if ui.button(btn_label)
                                            .on_hover_text("Toggle: show only plates hit by this arc")
                                            .clicked()
                                        {
                                            arc_plate_toggles.borrow_mut().push((ti, !traj.show_plates_active));
                                        }
                                        let zone_label = if traj.show_zones_active {
                                            egui::RichText::new("Isolate Zones").color(egui::Color32::from_rgb(100, 220, 100))
                                        } else {
                                            egui::RichText::new("Isolate Zones")
                                        };
                                        if ui.button(zone_label)
                                            .on_hover_text("Toggle: show entire zones hit by this arc")
                                            .clicked()
                                        {
                                            arc_zone_toggles.borrow_mut().push((ti, !traj.show_zones_active));
                                        }
                                    });
                                };

                            egui::ScrollArea::vertical().id_salt(("traj_scroll", pane_id)).max_height(500.0).show(
                                ui,
                                |ui| {
                                    ui.horizontal_top(|ui| {
                                        for ti in 0..traj_count {
                                            if ti > 0 {
                                                ui.separator(); // vertical divider
                                            }
                                            ui.push_id(("traj_col", pane.trajectories[ti].meta.id), |ui| {
                                                ui.vertical(|ui| {
                                                    ui.set_width(320.0);
                                                    render_traj_column(ui, ti, &pane.trajectories[ti]);
                                                });
                                            });
                                        }
                                    });
                                },
                            );
                        });
                }

                // Apply per-trajectory range changes and lock toggling
                {
                    let cam_dist_for_recompute = pane.viewport.camera.distance;
                    let mo = pane.marker_opacity;
                    let changes = traj_range_changes.into_inner();
                    for (ti, new_range, force_locked) in &changes {
                        if *ti < pane.trajectories.len() {
                            let range_changed = (pane.trajectories[*ti].meta.range_km - *new_range).abs() > 0.01;
                            let lock_changed = force_locked.is_some();
                            if range_changed || lock_changed {
                                pane.trajectories[*ti].meta.range_km = *new_range;
                                if let Some(locked) = force_locked {
                                    pane.trajectories[*ti].meta.range_locked = *locked;
                                }
                                recompute_trajectory_for_range(
                                    &mut pane.trajectories[*ti],
                                    comparison_ships,
                                    &mut pane.viewport,
                                    pane.loaded_armor.as_ref(),
                                    &render_state.device,
                                    cam_dist_for_recompute,
                                    mo,
                                );
                            }
                        }
                    }
                }

                // Write back shared range slider and recompute locked trajectories
                let new_range_km = range_km_cell.get();
                if (new_range_km - pane.ballistic_range_km).abs() > 0.01 {
                    pane.ballistic_range_km = new_range_km;
                    let cam_dist = pane.viewport.camera.distance;
                    let mo = pane.marker_opacity;
                    for ti in 0..pane.trajectories.len() {
                        if pane.trajectories[ti].meta.range_locked {
                            pane.trajectories[ti].meta.range_km = new_range_km;
                            recompute_trajectory_for_range(
                                &mut pane.trajectories[ti],
                                comparison_ships,
                                &mut pane.viewport,
                                pane.loaded_armor.as_ref(),
                                &render_state.device,
                                cam_dist,
                                mo,
                            );
                        }
                    }
                } else {
                    pane.ballistic_range_km = new_range_km;
                }

                // Handle delete single trajectory
                if let Some(del_id) = delete_traj_id.get() {
                    let had_active = pane.trajectories.iter().any(|t| t.show_plates_active || t.show_zones_active);
                    if let Some(pos) = pane.trajectories.iter().position(|t| t.meta.id == del_id) {
                        let removed = pane.trajectories.remove(pos);
                        if let Some(mid) = removed.mesh_id {
                            pane.viewport.remove_mesh(mid);
                        }
                    }
                    // If removed arc was active and no others remain active, restore visibility
                    if had_active && !pane.trajectories.iter().any(|t| t.show_plates_active || t.show_zones_active) {
                        pane.part_visibility.clear();
                        pane.plate_visibility.clear();
                        zone_changed = true;
                    }
                }

                // Handle clear all
                if clear_traj.get() {
                    let had_active = pane.trajectories.iter().any(|t| t.show_plates_active || t.show_zones_active);
                    for traj in pane.trajectories.drain(..) {
                        if let Some(mid) = traj.mesh_id {
                            pane.viewport.remove_mesh(mid);
                        }
                    }
                    if had_active {
                        pane.part_visibility.clear();
                        pane.plate_visibility.clear();
                        zone_changed = true;
                    }
                }

                // Handle per-arc plate isolation toggles
                for (ti, new_state) in arc_plate_toggles.borrow().iter() {
                    if let Some(traj) = pane.trajectories.get_mut(*ti) {
                        traj.show_plates_active = *new_state;
                        if *new_state {
                            traj.show_zones_active = false;
                        } // mutually exclusive
                    }
                }

                // Handle per-arc zone isolation toggles
                for (ti, new_state) in arc_zone_toggles.borrow().iter() {
                    if let Some(traj) = pane.trajectories.get_mut(*ti) {
                        traj.show_zones_active = *new_state;
                        if *new_state {
                            traj.show_plates_active = false;
                        } // mutually exclusive
                    }
                }

                // Handle global "Show Hit Plates" — activate all arcs
                if show_all_hit_plates.get() {
                    for traj in &mut pane.trajectories {
                        traj.show_plates_active = true;
                        traj.show_zones_active = false;
                    }
                }

                // Handle global "Show Hit Zones" — activate all arcs
                if show_all_hit_zones.get() {
                    for traj in &mut pane.trajectories {
                        traj.show_zones_active = true;
                        traj.show_plates_active = false;
                    }
                }

                let any_isolation_changed = !arc_plate_toggles.borrow().is_empty()
                    || !arc_zone_toggles.borrow().is_empty()
                    || show_all_hit_plates.get()
                    || show_all_hit_zones.get();

                // Apply plate/zone isolation
                if any_isolation_changed {
                    pane.undo_stack.push(VisibilitySnapshot {
                        part_visibility: pane.part_visibility.clone(),
                        plate_visibility: pane.plate_visibility.clone(),
                    });

                    let any_plates = pane.trajectories.iter().any(|t| t.show_plates_active);
                    let any_zones = pane.trajectories.iter().any(|t| t.show_zones_active);

                    if any_plates || any_zones {
                        // Collect hit zones and plate keys from active arcs
                        let mut hit_zones: std::collections::HashSet<String> = std::collections::HashSet::new();
                        let mut hit_plates: std::collections::HashSet<PlateKey> = std::collections::HashSet::new();
                        for traj in &pane.trajectories {
                            if traj.show_zones_active {
                                for hit in &traj.result.hits {
                                    hit_zones.insert(hit.zone.clone());
                                }
                            }
                            if traj.show_plates_active {
                                for hit in &traj.result.hits {
                                    hit_plates.insert((
                                        hit.zone.clone(),
                                        hit.material.clone(),
                                        (hit.thickness_mm * 10.0).round() as i32,
                                    ));
                                }
                            }
                        }

                        // Apply visibility: iterate all parts, show if zone matches or plate matches
                        if let Some(ref armor) = pane.loaded_armor {
                            pane.plate_visibility.clear();
                            for (zone, parts_with_plates) in &armor.zone_part_plates {
                                let zone_hit = hit_zones.contains(zone);
                                for (part, thicknesses) in parts_with_plates {
                                    let part_key = (zone.clone(), part.clone());
                                    if zone_hit {
                                        // Entire zone is visible
                                        pane.part_visibility.insert(part_key, true);
                                    } else {
                                        // Check plate-level hits
                                        let part_has_hit = thicknesses
                                            .iter()
                                            .any(|&t| hit_plates.contains(&(zone.clone(), part.clone(), t)));
                                        if part_has_hit {
                                            pane.part_visibility.insert(part_key, true);
                                            for &t in thicknesses {
                                                let pk: PlateKey = (zone.clone(), part.clone(), t);
                                                if hit_plates.contains(&pk) {
                                                    pane.plate_visibility.remove(&pk);
                                                } else {
                                                    pane.plate_visibility.insert(pk, false);
                                                }
                                            }
                                        } else {
                                            pane.part_visibility.insert(part_key, false);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // No arcs active — restore all visibility
                        pane.part_visibility.clear();
                        pane.plate_visibility.clear();
                    }
                    zone_changed = true;
                }

                // Handle marker opacity change — re-upload all trajectory meshes
                let marker_opacity_changed = (pane.marker_opacity - prev_marker_opacity).abs() > 0.001;
                if marker_opacity_changed && !pane.trajectories.is_empty() {
                    let cam_dist = pane.viewport.camera.distance;
                    let mo = pane.marker_opacity;
                    for traj in &mut pane.trajectories {
                        if let Some(old_mid) = traj.mesh_id.take() {
                            pane.viewport.remove_mesh(old_mid);
                        }
                        let color = TRAJECTORY_PALETTE[traj.meta.color_index % TRAJECTORY_PALETTE.len()];
                        traj.mesh_id = Some(upload_trajectory_visualization(
                            &mut pane.viewport,
                            &traj.result,
                            &render_state.device,
                            color,
                            traj.last_visible_hit,
                            cam_dist,
                            mo,
                        ));
                        traj.marker_cam_dist = cam_dist;
                    }
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
            // Re-upload trajectory visualizations (viewport.clear() destroyed them)
            let cam_dist = pane.viewport.camera.distance;
            for traj in &mut pane.trajectories {
                let color = TRAJECTORY_PALETTE[traj.meta.color_index % TRAJECTORY_PALETTE.len()];
                let mesh_id = upload_trajectory_visualization(
                    &mut pane.viewport,
                    &traj.result,
                    &render_state.device,
                    color,
                    traj.last_visible_hit,
                    cam_dist,
                    pane.marker_opacity,
                );
                traj.mesh_id = Some(mesh_id);
                traj.marker_cam_dist = cam_dist;
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
fn show_armor_tooltip(
    ui: &mut egui::Ui,
    info: &ArmorTriangleTooltip,
    comparison_ships: &[crate::armor_viewer::penetration::ComparisonShip],
    ifhe_enabled: bool,
    translate: &dyn Fn(&str) -> String,
) {
    use crate::armor_viewer::penetration::{PenResult, ammo_type_display, check_penetration};
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

    // Penetration check results
    if !comparison_ships.is_empty() {
        ui.separator();
        ui.label(egui::RichText::new("Penetration Check").small().strong());
        if ifhe_enabled {
            ui.label(egui::RichText::new("IFHE active (+25% HE pen)").small().color(egui::Color32::YELLOW));
        }

        for ship in comparison_ships {
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} {}",
                    crate::armor_viewer::ship_selector::tier_roman(ship.tier),
                    &ship.display_name
                ))
                .small()
                .strong(),
            );
            for shell in &ship.shells {
                let result = check_penetration(shell, info.thickness_mm, ifhe_enabled);
                let (icon, color) = match result {
                    PenResult::Penetrates => ("\u{2705}", egui::Color32::from_rgb(100, 220, 100)),
                    PenResult::Bounces => ("\u{274C}", egui::Color32::from_rgb(220, 100, 100)),
                    PenResult::AngleDependent => ("\u{2796}", egui::Color32::GRAY),
                };
                let pen_info = match shell.ammo_type.as_str() {
                    "HE" => {
                        let pen = if ifhe_enabled {
                            shell.he_pen_mm.unwrap_or(0.0) * 1.25
                        } else {
                            shell.he_pen_mm.unwrap_or(0.0)
                        };
                        format!("{:.0}mm pen", pen)
                    }
                    "CS" => format!("{:.0}mm pen", shell.sap_pen_mm.unwrap_or(0.0)),
                    "AP" => {
                        if shell.caliber_mm > info.thickness_mm * 14.3 {
                            "overmatch".to_string()
                        } else {
                            "angle-dependent".to_string()
                        }
                    }
                    _ => String::new(),
                };
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon));
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {:.0}mm ({})",
                            ammo_type_display(&shell.ammo_type),
                            shell.caliber_mm,
                            pen_info,
                        ))
                        .color(color)
                        .small(),
                    );
                });
            }
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

            // Show Hidden mode: only show plates the in-game viewer hides
            if pane.show_hidden_only && !info.hidden {
                continue;
            }
            let part_key = (info.zone.clone(), info.material_name.clone());
            if !pane.part_visibility.get(&part_key).copied().unwrap_or(true) {
                continue;
            }
            if !pane.plate_visibility.get(key).copied().unwrap_or(true) {
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

/// Add a line segment as two cross-shaped quads into the vertex/index buffers.
fn traj_line_segment(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    p0: [f32; 3],
    p1: [f32; 3],
    color: [f32; 4],
    perp1: [f32; 3],
    perp2: [f32; 3],
    line_width: f32,
) {
    use crate::viewport_3d::camera::{add, scale, sub};
    let offset1 = scale(perp1, line_width * 0.5);
    let offset2 = scale(perp2, line_width * 0.5);

    for offset in [offset1, offset2] {
        let b = vertices.len() as u32;
        vertices.push(Vertex { position: sub(p0, offset), normal: perp1, color });
        vertices.push(Vertex { position: add(p0, offset), normal: perp1, color });
        vertices.push(Vertex { position: add(p1, offset), normal: perp1, color });
        vertices.push(Vertex { position: sub(p1, offset), normal: perp1, color });
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
}

/// Add a diamond-shaped marker at a point into the vertex/index buffers.
fn traj_marker(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    pos: [f32; 3],
    color: [f32; 4],
    size: f32,
    dir: [f32; 3],
    perp1: [f32; 3],
    perp2: [f32; 3],
) {
    use crate::viewport_3d::camera::{add, scale, sub};
    let base = vertices.len() as u32;
    let o1 = scale(perp1, size);
    let o2 = scale(perp2, size);
    let od = scale(dir, size);

    let top = add(pos, od);
    let bottom = sub(pos, od);
    let left = sub(pos, o1);
    let right = add(pos, o1);
    let front = add(pos, o2);
    let back = sub(pos, o2);

    let n = [0.0, 1.0, 0.0];
    for &[a, b, c] in &[
        [top, right, front],
        [top, front, left],
        [top, left, back],
        [top, back, right],
        [bottom, front, right],
        [bottom, left, front],
        [bottom, back, left],
        [bottom, right, back],
    ] {
        vertices.push(Vertex { position: a, normal: n, color });
        vertices.push(Vertex { position: b, normal: n, color });
        vertices.push(Vertex { position: c, normal: n, color });
    }

    for i in 0..24 {
        indices.push(base + i);
    }
}

/// Recompute a trajectory's arc, impact data, detonation points, and 3D mesh for a new range.
fn recompute_trajectory_for_range(
    traj: &mut crate::armor_viewer::state::StoredTrajectory,
    comparison_ships: &[crate::armor_viewer::penetration::ComparisonShip],
    viewport: &mut crate::viewport_3d::Viewport3D,
    loaded_armor: Option<&crate::armor_viewer::state::LoadedShipArmor>,
    device: &wgpu::Device,
    cam_distance: f32,
    marker_opacity: f32,
) {
    use crate::viewport_3d::camera::normalize;

    let range_m = traj.meta.range_km as f64 * 1000.0;
    let result = &mut traj.result;

    // Derive horizontal approach direction from the existing shell direction
    let dir = result.direction;
    let horiz_len = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
    let approach_xz = if horiz_len > 1e-6 { [dir[0] / horiz_len, 0.0, dir[2] / horiz_len] } else { [0.0, 0.0, -1.0] };

    // Get first AP shell for ballistic arc computation
    let first_shell = comparison_ships
        .iter()
        .flat_map(|s| s.shells.iter())
        .find(|s| s.ammo_type == "AP")
        .or_else(|| comparison_ships.iter().flat_map(|s| s.shells.iter()).next());

    // Recompute ballistic impact
    let ballistic_impact = if range_m > 0.0 {
        first_shell.and_then(|shell| {
            let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
            crate::armor_viewer::ballistics::solve_for_range(&params, range_m)
        })
    } else {
        None
    };

    // Update shell direction from new impact angle
    if let Some(ref impact) = ballistic_impact {
        let horiz_angle = impact.impact_angle_horizontal as f32;
        let cos_h = horiz_angle.cos();
        let sin_h = horiz_angle.sin();
        result.direction = normalize([approach_xz[0] * cos_h, -sin_h, approach_xz[2] * cos_h]);
    }

    // Recompute arc points
    result.arc_points_3d = if let Some(ref impact) = ballistic_impact {
        if let Some(shell) = first_shell {
            let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
            let (arc_2d, height_ratio) =
                crate::armor_viewer::ballistics::simulate_arc_points(&params, impact.launch_angle, 60);
            let model_extent = loaded_armor
                .map(|a| {
                    let dx = a.bounds.1[0] - a.bounds.0[0];
                    let dz = a.bounds.1[2] - a.bounds.0[2];
                    dx.max(dz)
                })
                .unwrap_or(10.0);
            let arc_horiz_extent = model_extent * 2.0;
            let arc_height_extent = arc_horiz_extent * (height_ratio as f32).max(0.02);
            let first_hit_pos = result.hits.first().map(|h| h.position).unwrap_or(result.origin);
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
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // Recompute detonation points and last visible hit
    // Each shell gets its own ballistic solve (different velocity/angle at range)
    let mut new_detonation_points: Vec<crate::armor_viewer::penetration::DetonationMarker> = Vec::new();
    let mut new_last_visible: Option<usize> = None;
    for (ship_idx, ship) in comparison_ships.iter().enumerate() {
        for shell in ship.shells.iter().filter(|s| s.ammo_type == "AP") {
            let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
            let shell_impact =
                if range_m > 0.0 { crate::armor_viewer::ballistics::solve_for_range(&params, range_m) } else { None };
            if let Some(ref impact) = shell_impact {
                let sim = crate::armor_viewer::penetration::simulate_shell_through_hits(
                    &params,
                    impact,
                    &result.hits,
                    &result.direction,
                );
                if let Some(det) = sim.detonation {
                    new_detonation_points.push(crate::armor_viewer::penetration::DetonationMarker {
                        position: det.position,
                        ship_index: ship_idx,
                    });
                }
                // Earliest terminating event: detonation or ricochet/shatter
                let shell_stop = match (sim.detonated_at, sim.stopped_at) {
                    (Some(d), Some(s)) => Some(d.min(s)),
                    (Some(d), None) => Some(d),
                    (None, Some(s)) => Some(s),
                    (None, None) => None,
                };
                if let Some(idx) = shell_stop {
                    new_last_visible = Some(new_last_visible.map_or(idx, |prev: usize| prev.min(idx)));
                }
            }
        }
    }
    result.detonation_points = new_detonation_points;
    result.ballistic_impact = ballistic_impact;
    traj.last_visible_hit = new_last_visible;

    // Remove old mesh and upload new one
    if let Some(old_mid) = traj.mesh_id.take() {
        viewport.remove_mesh(old_mid);
    }
    let color = TRAJECTORY_PALETTE[traj.meta.color_index % TRAJECTORY_PALETTE.len()];
    traj.mesh_id = Some(upload_trajectory_visualization(
        viewport,
        &traj.result,
        device,
        color,
        traj.last_visible_hit,
        cam_distance,
        marker_opacity,
    ));
    traj.marker_cam_dist = cam_distance;
}

/// Compute perpendicular vectors for a line segment direction, for cross-shaped quad rendering.
fn segment_perps(seg_dir: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    use crate::viewport_3d::camera::{cross, normalize};
    let arbitrary = if seg_dir[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let p1 = normalize(cross(seg_dir, arbitrary));
    let p2 = normalize(cross(seg_dir, p1));
    (p1, p2)
}

/// Upload a trajectory visualization as colored line segments on the overlay layer.
/// If arc_points_3d is non-empty, draws a curved arc from firing position to first hit,
/// then straight segments through subsequent armor plates.
fn upload_trajectory_visualization(
    viewport: &mut crate::viewport_3d::Viewport3D,
    result: &crate::armor_viewer::penetration::TrajectoryResult,
    device: &wgpu::Device,
    traj_color: [f32; 4],
    last_visible_hit: Option<usize>,
    cam_distance: f32,
    marker_opacity: f32,
) -> MeshId {
    use crate::viewport_3d::camera::{add, cross, normalize, scale, sub};

    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let dir = result.direction;
    // Scale markers and line width with camera distance so they shrink when zoomed in
    let scale_factor = (cam_distance / 200.0).clamp(0.15, 3.0);
    let line_width = 0.05 * scale_factor;

    let arbitrary = if dir[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let perp1 = normalize(cross(dir, arbitrary));
    let perp2 = normalize(cross(dir, perp1));

    if !result.hits.is_empty() {
        let first_pos = result.hits[0].position;

        if result.arc_points_3d.len() >= 2 {
            // Draw the ballistic arc as connected line segments
            let arc = &result.arc_points_3d;
            for i in 0..arc.len() - 1 {
                let seg_dir_raw = sub(arc[i + 1], arc[i]);
                let len = (seg_dir_raw[0] * seg_dir_raw[0]
                    + seg_dir_raw[1] * seg_dir_raw[1]
                    + seg_dir_raw[2] * seg_dir_raw[2])
                    .sqrt();
                if len < 1e-6 {
                    continue;
                }
                let seg_dir = [seg_dir_raw[0] / len, seg_dir_raw[1] / len, seg_dir_raw[2] / len];
                let (sp1, sp2) = segment_perps(seg_dir);
                // Fade in: more opaque closer to the ship
                let frac = i as f32 / (arc.len() - 1) as f32;
                let alpha = 0.3 + 0.6 * frac; // 0.3 at start → 0.9 at impact
                traj_line_segment(
                    &mut vertices,
                    &mut indices,
                    arc[i],
                    arc[i + 1],
                    [traj_color[0], traj_color[1], traj_color[2], alpha],
                    sp1,
                    sp2,
                    line_width,
                );
            }
        } else {
            // No arc: draw flat leading segment (point-blank mode)
            let lead_start = sub(first_pos, scale(dir, 2.0));
            traj_line_segment(
                &mut vertices,
                &mut indices,
                lead_start,
                first_pos,
                [traj_color[0], traj_color[1], traj_color[2], 0.9],
                perp1,
                perp2,
                line_width,
            );
        }

        // Hit markers and inter-hit segments — stop at detonation
        let max_hit = last_visible_hit.unwrap_or(result.hits.len().saturating_sub(1));
        for i in 0..result.hits.len() {
            if i > max_hit {
                break;
            }
            let hit = &result.hits[i];

            // Angle from normal: 0°=head-on (green), 45°+=ricochet zone (red)
            let color = if hit.angle_deg < 30.0 {
                [0.3, 0.9, 0.3, marker_opacity]
            } else if hit.angle_deg < 45.0 {
                [0.9, 0.7, 0.2, marker_opacity]
            } else {
                [0.9, 0.3, 0.3, marker_opacity]
            };

            traj_marker(&mut vertices, &mut indices, hit.position, color, 0.15 * scale_factor, dir, perp1, perp2);

            if i + 1 < result.hits.len() && i < max_hit {
                let next_pos = result.hits[i + 1].position;
                traj_line_segment(
                    &mut vertices,
                    &mut indices,
                    hit.position,
                    next_pos,
                    [traj_color[0], traj_color[1], traj_color[2], 0.9],
                    perp1,
                    perp2,
                    line_width,
                );
            }
        }

        // Trailing segment: show the shell's continued path after the last visible hit.
        // This visualizes overpen exit, ricochet bounce path, or post-detonation trajectory.
        {
            let last_rendered_idx = last_visible_hit.unwrap_or(result.hits.len().saturating_sub(1));
            let last_pos = result.hits[last_rendered_idx].position;
            let trail_end = add(last_pos, scale(dir, 2.0));
            traj_line_segment(
                &mut vertices,
                &mut indices,
                last_pos,
                trail_end,
                [traj_color[0], traj_color[1], traj_color[2], 0.3],
                perp1,
                perp2,
                line_width,
            );
        }

        // Detonation markers — diamond shapes tinted per-ship color
        let num_dets = result.detonation_points.len();
        for (di, det) in result.detonation_points.iter().enumerate() {
            // Offset each marker sideways so overlapping detonations are visible
            let lateral_offset = if num_dets > 1 {
                let spread = 0.4 * scale_factor;
                let t = if num_dets == 1 { 0.0 } else { (di as f32 / (num_dets - 1) as f32) - 0.5 };
                scale(perp1, t * spread)
            } else {
                [0.0, 0.0, 0.0]
            };
            let det_pos = add(det.position, lateral_offset);
            let burst_size = 0.25 * scale_factor;
            let sc = SHIP_COLORS[det.ship_index % SHIP_COLORS.len()];
            let burst_color = [sc[0], sc[1], sc[2], marker_opacity];

            // Octahedron diamond: 6 tip points along each axis
            let base_idx = vertices.len() as u32;
            let offsets = [
                scale(perp1, burst_size),
                scale(perp1, -burst_size),
                scale(perp2, burst_size),
                scale(perp2, -burst_size),
                scale(dir, burst_size),
                scale(dir, -burst_size),
            ];

            // Center vertex
            vertices.push(Vertex { position: det_pos, normal: [0.0, 1.0, 0.0], color: burst_color });

            for offset in &offsets {
                vertices.push(Vertex { position: add(det_pos, *offset), normal: [0.0, 1.0, 0.0], color: burst_color });
            }

            // 8 triangular faces of the octahedron
            let tip_indices = [(1, 3, 5), (3, 2, 5), (2, 4, 5), (4, 1, 5), (1, 3, 6), (3, 2, 6), (2, 4, 6), (4, 1, 6)];
            for (a, b, c) in tip_indices {
                indices.push(base_idx + a);
                indices.push(base_idx + b);
                indices.push(base_idx + c);
            }
        }
    }

    viewport.add_overlay_mesh(device, &vertices, &indices)
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
fn create_water_plane(y: f32, bounds: ([f32; 3], [f32; 3]), opacity: f32) -> (Vec<Vertex>, Vec<u32>) {
    let cx = (bounds.0[0] + bounds.1[0]) * 0.5;
    let cz = (bounds.0[2] + bounds.1[2]) * 0.5;
    let ex = (bounds.1[0] - bounds.0[0]) * 2.25;
    let ez = (bounds.1[2] - bounds.0[2]) * 2.25;

    let color = [0.1, 0.4, 0.8, opacity];
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
            // Show Hidden mode: only show plates the in-game viewer hides
            if pane.show_hidden_only && !info.hidden {
                continue;
            }
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

        // Build two thin quads: one offset +normal (front), one -normal (back)
        for &n_sign in &[1.0_f32, -1.0] {
            let base = vertices.len() as u32;
            let offset_normal = [
                avg_normal[0] * normal_offset * n_sign,
                avg_normal[1] * normal_offset * n_sign,
                avg_normal[2] * normal_offset * n_sign,
            ];
            let vert_normal = [avg_normal[0] * n_sign, avg_normal[1] * n_sign, avg_normal[2] * n_sign];
            for &p in &[p0, p1] {
                for &sign in &[-1.0_f32, 1.0] {
                    vertices.push(Vertex {
                        position: [
                            p[0] + tangent[0] * edge_half_width * sign + offset_normal[0],
                            p[1] + tangent[1] * edge_half_width * sign + offset_normal[1],
                            p[2] + tangent[2] * edge_half_width * sign + offset_normal[2],
                        ],
                        normal: vert_normal,
                        color: edge_color,
                    });
                }
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }

    if !indices.is_empty() {
        pane.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_DEFAULT);
    }
}

/// Detect and render gap edges (boundary edges shared by only 1 triangle) in the armor mesh.
/// Returns the number of gap edges found.
fn upload_gap_edges(pane: &mut ArmorPane, armor: &LoadedShipArmor, device: &wgpu::Device) -> usize {
    use std::collections::HashMap;

    let edge_half_width: f32 = 0.006;
    let normal_offset: f32 = 0.008;
    let gap_color: [f32; 4] = [1.0, 0.15, 0.1, 1.0]; // red
    let max_edge_length: f32 = 5.0; // filter out very long edges (mesh outer boundaries)

    fn quantize(v: [f32; 3]) -> [i32; 3] {
        [(v[0] * 10000.0).round() as i32, (v[1] * 10000.0).round() as i32, (v[2] * 10000.0).round() as i32]
    }

    type EdgeKey = ([i32; 3], [i32; 3]);
    fn make_edge_key(a: [i32; 3], b: [i32; 3]) -> EdgeKey {
        if a < b { (a, b) } else { (b, a) }
    }

    struct EdgeData {
        p0: [f32; 3],
        p1: [f32; 3],
        normal: [f32; 3],
    }

    // Count how many triangles share each edge, and store position/normal data.
    let mut edge_count: HashMap<EdgeKey, (usize, EdgeData)> = HashMap::new();

    for mesh in &armor.meshes {
        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            if !pane.show_zero_mm && info.thickness_mm.abs() < 0.05 {
                continue;
            }
            if pane.show_hidden_only && !info.hidden {
                continue;
            }
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

            // Face normal
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
                let ek = make_edge_key(qa, qb);

                edge_count
                    .entry(ek)
                    .and_modify(|(count, _)| *count += 1)
                    .or_insert((1, EdgeData { p0: tri_pos[a], p1: tri_pos[b], normal: face_normal }));
            }
        }
    }

    // Boundary edges: shared by exactly 1 triangle
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut gap_count = 0;

    for (_ek, (count, data)) in &edge_count {
        if *count != 1 {
            continue;
        }

        let p0 = data.p0;
        let p1 = data.p1;

        // Filter out very long edges (likely outer mesh boundaries, not gaps)
        let dx = p1[0] - p0[0];
        let dy = p1[1] - p0[1];
        let dz = p1[2] - p0[2];
        let edge_len = (dx * dx + dy * dy + dz * dz).sqrt();
        if edge_len > max_edge_length || edge_len < 1e-6 {
            continue;
        }

        gap_count += 1;

        let avg_normal = data.normal;
        let edge_dir = [dx, dy, dz];

        // Tangent perpendicular to edge in the surface plane
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
                avg_normal[0] * normal_offset * n_sign,
                avg_normal[1] * normal_offset * n_sign,
                avg_normal[2] * normal_offset * n_sign,
            ];
            let vert_normal = [avg_normal[0] * n_sign, avg_normal[1] * n_sign, avg_normal[2] * n_sign];
            for &p in &[p0, p1] {
                for &sign in &[-1.0_f32, 1.0] {
                    vertices.push(Vertex {
                        position: [
                            p[0] + tangent[0] * edge_half_width * sign + offset[0],
                            p[1] + tangent[1] * edge_half_width * sign + offset[1],
                            p[2] + tangent[2] * edge_half_width * sign + offset[2],
                        ],
                        normal: vert_normal,
                        color: gap_color,
                    });
                }
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }

    if !indices.is_empty() {
        pane.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
    }

    gap_count
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
