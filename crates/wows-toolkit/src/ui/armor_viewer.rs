use std::sync::Arc;
use std::sync::mpsc;

use egui_dock::DockArea;
use egui_dock::DockState;
use egui_dock::TabViewer;

use crate::app::ToolkitTabViewer;
use crate::armor_viewer::constants::*;
use crate::armor_viewer::legend::show_armor_legend;
use crate::armor_viewer::ship_selector::ShipCatalog;
use crate::armor_viewer::ship_selector::species_name;
use crate::armor_viewer::ship_selector::tier_roman;
use crate::armor_viewer::state::AnalysisTab;
use crate::armor_viewer::state::ArmorPane;
use crate::armor_viewer::state::ArmorTriangleTooltip;
use crate::armor_viewer::state::ArmorViewerDefaults;
use crate::armor_viewer::state::ArmorZone;
use crate::armor_viewer::state::CompareSettings;
use crate::armor_viewer::state::ExportRequest;
use crate::armor_viewer::state::HullPopoverResult;
use crate::armor_viewer::state::HullReloadData;
use crate::armor_viewer::state::LoadedShipArmor;
use crate::armor_viewer::state::PlateKey;
use crate::armor_viewer::state::ShipAssetsState;
use crate::armor_viewer::state::SidebarHighlightKey;
use crate::armor_viewer::state::UpgradeReloadData;
use crate::armor_viewer::state::VisibilitySnapshot;
use crate::armor_viewer::state::ZonePart;
use crate::icon_str;
use crate::icons;
use crate::ui::analysis_panel::focus_analysis_tab;
use crate::viewport_3d::GpuPipeline;
use crate::viewport_3d::LAYER_DEFAULT;
use crate::viewport_3d::LAYER_HULL;
use crate::viewport_3d::LAYER_OVERLAY;
use crate::viewport_3d::MeshId;
use crate::viewport_3d::Vertex;
use wowsunpack::game_params::types::AmmoType;

/// Per-frame viewer struct implementing `egui_dock::TabViewer` for armor panes.
struct ArmorPaneViewer<'a> {
    render_state: &'a eframe::egui_wgpu::RenderState,
    gpu_pipeline: &'a GpuPipeline,
    mirror_camera_signal: &'a std::cell::Cell<Option<u64>>,
    active_pane_signal: &'a std::cell::Cell<Option<u64>>,
    save_defaults_signal: &'a std::cell::Cell<Option<ArmorViewerDefaults>>,
    export_signal: &'a std::cell::Cell<Option<ExportRequest>>,
    pen_check_toggle: &'a std::cell::Cell<bool>,
    analysis_tab_signal: &'a std::cell::Cell<Option<AnalysisTab>>,
    comparison_ships: &'a [crate::armor_viewer::penetration::ComparisonShip],
    ifhe_enabled: bool,
    translate_part: &'a dyn Fn(&str) -> String,
    comparison_ships_version: u64,
    /// Signal: (pane_id, new_lod) when user changes hull LOD in a popover.
    hull_lod_signal: &'a std::cell::Cell<Option<(u64, usize)>>,
    /// Signal: pane_id when user changes hull upgrade selection in a popover.
    hull_change_signal: &'a std::cell::Cell<Option<u64>>,
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
        render_armor_pane(ui, tab, self);
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        true
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
            state.gpu_pipeline = Some(Arc::new(GpuPipeline::new(&render_state.device, &render_state.queue)));
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
        if let ShipAssetsState::Loading(rx) = &state.ship_assets
            && let Ok(result) = rx.try_recv()
        {
            match result {
                Ok(assets) => {
                    // Build catalog from wows_data.game_metadata which has translations loaded,
                    // not from assets.metadata() which has translations: None
                    let wd = wows_data.read();
                    if let Some(metadata) = wd.game_metadata.as_ref() {
                        let catalog = ShipCatalog::build(metadata);
                        // Load nation flags for each nation in the catalog.
                        for nation_group in &catalog.nations {
                            if !state.nation_flag_textures.contains_key(&nation_group.nation)
                                && let Some(asset) = crate::task::load_nation_flag(&wd.vfs, &nation_group.nation)
                            {
                                state.nation_flag_textures.insert(nation_group.nation.clone(), asset);
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
        poll_pane_loads(
            &mut state.dock_state,
            &render_state.device,
            &render_state.queue,
            &gpu_pipeline,
            &state.comparison_ships,
            state.ifhe_enabled,
        );

        // If all tabs were closed, create a fresh empty pane so the user isn't stuck.
        if state.dock_state.main_surface().num_tabs() == 0 {
            let next_id = state.allocate_pane_id();
            let new_pane = ArmorPane::with_defaults(next_id, &armor_defaults);
            state.dock_state = DockState::new(vec![new_pane]);
            state.active_pane_id = next_id;
        }

        let nation_flags = &state.nation_flag_textures;

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
                let deferred_export: std::cell::Cell<Option<ExportRequest>> = std::cell::Cell::new(None);
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
                                                        deferred_export_ref.set(Some(ExportRequest {
                                                            param_index: export_param_idx.clone(),
                                                            display_name: export_display_name.clone(),
                                                            selected_hull: None,
                                                        }));
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
        let export_cell: std::cell::Cell<Option<ExportRequest>> = std::cell::Cell::new(None);
        let export_ref = &export_cell;
        let pen_check_toggle_cell: std::cell::Cell<bool> = std::cell::Cell::new(false);
        let pen_check_toggle_ref = &pen_check_toggle_cell;
        let analysis_tab_cell: std::cell::Cell<Option<AnalysisTab>> = std::cell::Cell::new(None);
        let analysis_tab_ref = &analysis_tab_cell;
        let hull_lod_cell: std::cell::Cell<Option<(u64, usize)>> = std::cell::Cell::new(None);
        let hull_lod_ref = &hull_lod_cell;
        let hull_change_cell: std::cell::Cell<Option<u64>> = std::cell::Cell::new(None);
        let hull_change_ref = &hull_change_cell;
        let comparison_ships_snapshot = &state.comparison_ships;
        let ifhe_snapshot = state.ifhe_enabled;
        {
            let mut viewer = ArmorPaneViewer {
                render_state: &render_state,
                gpu_pipeline: &gpu_pipeline,
                mirror_camera_signal: if mirror_cameras { active_camera_ref } else { &std::cell::Cell::new(None) },
                active_pane_signal: active_pane_ref,
                save_defaults_signal: save_defaults_ref,
                export_signal: export_ref,
                pen_check_toggle: pen_check_toggle_ref,
                analysis_tab_signal: analysis_tab_ref,
                comparison_ships: comparison_ships_snapshot,
                ifhe_enabled: ifhe_snapshot,
                translate_part: translate_part_ref,
                comparison_ships_version: state.comparison_ships_version,
                hull_lod_signal: hull_lod_ref,
                hull_change_signal: hull_change_ref,
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
        if mirror_cameras && let Some(active_id) = active_camera_pane.get() {
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
                            upload_armor_to_viewport(
                                tab,
                                &armor,
                                &render_state.device,
                                &render_state.queue,
                                &gpu_pipeline,
                            );
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

        // Auto-snapshot active pane's display options so new panes/loads inherit them.
        if let Some(active_pane) =
            state.dock_state.iter_all_tabs().find(|(_, tab)| tab.id == state.active_pane_id).map(|(_, tab)| tab)
            && active_pane.loaded_armor.is_some()
        {
            let hull_all_on =
                !active_pane.hull_visibility.is_empty() && active_pane.hull_visibility.values().all(|&v| v);
            let armor_all_on = !active_pane.part_visibility.is_empty()
                && active_pane.part_visibility.values().all(|&v| v)
                && !active_pane.plate_visibility.values().any(|&v| !v);
            let d = &mut self.tab_state.armor_viewer_defaults;
            d.show_plate_edges = active_pane.show_plate_edges;
            d.show_waterline = active_pane.show_waterline;
            d.show_zero_mm = active_pane.show_zero_mm;
            d.armor_opacity = active_pane.armor_opacity;
            d.waterline_opacity = active_pane.waterline_opacity;
            d.hull_opaque = active_pane.hull_opaque;
            d.hull_all_visible = hull_all_on;
            d.armor_all_visible = armor_all_on;
            d.show_splash_boxes = active_pane.show_splash_boxes;
        }

        // Handle export signal from toolbar button
        if let Some(export_req) = export_cell.take() {
            state.export_confirm = Some(export_req);
        }

        // Handle pen check toggle from toolbar button
        if pen_check_toggle_cell.get() {
            state.show_comparison_panel = !state.show_comparison_panel;
            if state.show_comparison_panel {
                focus_analysis_tab(&mut state.analysis_dock_state, AnalysisTab::Ships);
            }
        }

        // Handle hull LOD change signal — reload only hull meshes at the new LOD
        if let Some((pane_id, new_lod)) = hull_lod_cell.get()
            && let Some((_, pane)) = state.dock_state.iter_all_tabs_mut().find(|(_, t)| t.id == pane_id)
            && let Some(param_index) = pane.selected_ship.clone()
        {
            start_hull_lod_reload(pane, &ship_assets, &param_index, new_lod);
        }

        // Handle hull upgrade change signal — incremental reload (turrets + turret armor only)
        if let Some(pane_id) = hull_change_cell.get()
            && let Some((_, pane)) = state.dock_state.iter_all_tabs_mut().find(|(_, t)| t.id == pane_id)
            && let Some(param_index) = pane.selected_ship.clone()
        {
            if pane.loaded_armor.is_some() {
                start_upgrade_reload(pane, &ship_assets, &param_index);
            } else {
                // No armor loaded yet — fall back to full load
                let display_name = pane.loaded_armor.as_ref().map(|a| a.display_name.clone()).unwrap_or_default();
                load_ship_for_pane_with_lod(pane, &param_index, &display_name, &ship_assets, pane.hull_lod);
            }
        }

        // Handle trajectory/splash mode activation → open panel and switch tab
        if let Some(tab) = analysis_tab_cell.get() {
            state.show_comparison_panel = true;
            focus_analysis_tab(&mut state.analysis_dock_state, tab);
        }

        // ── Unified analysis window (Ships / Trajectory / Splash tabs) ──
        {
            let catalog_clone = state.ship_catalog.clone();
            let traj_actions = crate::ui::analysis_panel::show_analysis_window(
                ui.ctx(),
                state,
                translate_part_ref,
                &wows_data,
                catalog_clone.as_deref(),
            );

            // Apply deferred trajectory actions to the active pane
            if let Some(pane) =
                state.dock_state.iter_all_tabs_mut().find(|(_, tab)| tab.id == state.active_pane_id).map(|(_, tab)| tab)
            {
                if let Some(new_range) = traj_actions.new_range {
                    pane.ballistic_range = new_range;
                }

                let comparison_ships_snapshot = &state.comparison_ships;
                let comparison_ships_version = state.comparison_ships_version;

                // Apply per-arc range changes
                let cam_dist = pane.viewport.camera.distance;
                let mo = pane.marker_opacity;
                for (ti, new_range) in &traj_actions.per_arc_range_changes {
                    if *ti < pane.trajectories.len()
                        && (pane.trajectories[*ti].meta.range.value() - new_range.value()).abs() > 0.01
                    {
                        pane.trajectories[*ti].meta.range = *new_range;
                        recompute_trajectory_for_range(
                            &mut pane.trajectories[*ti],
                            comparison_ships_snapshot,
                            &mut pane.viewport,
                            pane.loaded_armor.as_ref(),
                            &render_state.device,
                            cam_dist,
                            mo,
                            comparison_ships_version,
                        );
                    }
                }

                // Delete single trajectory
                if let Some(del_id) = traj_actions.delete_id {
                    let had_active = pane.trajectories.iter().any(|t| t.show_plates_active || t.show_zones_active);
                    if let Some(pos) = pane.trajectories.iter().position(|t| t.meta.id == del_id) {
                        let removed = pane.trajectories.remove(pos);
                        if let Some(mid) = removed.mesh_id {
                            pane.viewport.remove_mesh(mid);
                        }
                    }
                    if had_active && !pane.trajectories.iter().any(|t| t.show_plates_active || t.show_zones_active) {
                        pane.part_visibility.clear();
                        pane.plate_visibility.clear();
                    }
                }

                // Clear all trajectories
                if traj_actions.clear_all {
                    let had_active = pane.trajectories.iter().any(|t| t.show_plates_active || t.show_zones_active);
                    for traj in pane.trajectories.drain(..) {
                        if let Some(mid) = traj.mesh_id {
                            pane.viewport.remove_mesh(mid);
                        }
                    }
                    if had_active {
                        pane.part_visibility.clear();
                        pane.plate_visibility.clear();
                    }
                }

                // Per-arc isolation toggles
                for (ti, new_state) in &traj_actions.arc_plate_toggles {
                    if let Some(traj) = pane.trajectories.get_mut(*ti) {
                        traj.show_plates_active = *new_state;
                        if *new_state {
                            traj.show_zones_active = false;
                        }
                    }
                }
                for (ti, new_state) in &traj_actions.arc_zone_toggles {
                    if let Some(traj) = pane.trajectories.get_mut(*ti) {
                        traj.show_zones_active = *new_state;
                        if *new_state {
                            traj.show_plates_active = false;
                        }
                    }
                }

                // Global show-all toggles
                if traj_actions.show_all_hit_plates {
                    for traj in &mut pane.trajectories {
                        traj.show_plates_active = true;
                        traj.show_zones_active = false;
                    }
                }
                if traj_actions.show_all_hit_zones {
                    for traj in &mut pane.trajectories {
                        traj.show_zones_active = true;
                        traj.show_plates_active = false;
                    }
                }

                // Apply plate/zone isolation visibility
                let any_isolation_changed = !traj_actions.arc_plate_toggles.is_empty()
                    || !traj_actions.arc_zone_toggles.is_empty()
                    || traj_actions.show_all_hit_plates
                    || traj_actions.show_all_hit_zones;
                if any_isolation_changed {
                    pane.undo_stack.push(VisibilitySnapshot {
                        part_visibility: pane.part_visibility.clone(),
                        plate_visibility: pane.plate_visibility.clone(),
                    });

                    let any_plates = pane.trajectories.iter().any(|t| t.show_plates_active);
                    let any_zones = pane.trajectories.iter().any(|t| t.show_zones_active);

                    if any_plates || any_zones {
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

                        if let Some(ref armor) = pane.loaded_armor {
                            pane.plate_visibility.clear();
                            for zone in &armor.zone_part_plates {
                                let zone_hit = hit_zones.contains(&zone.name);
                                for part in &zone.parts {
                                    let part_key = (zone.name.clone(), part.name.clone());
                                    if zone_hit {
                                        pane.part_visibility.insert(part_key, true);
                                    } else {
                                        let part_has_hit = part
                                            .plates
                                            .iter()
                                            .any(|&t| hit_plates.contains(&(zone.name.clone(), part.name.clone(), t)));
                                        if part_has_hit {
                                            pane.part_visibility.insert(part_key, true);
                                            for &t in &part.plates {
                                                let pk: PlateKey = (zone.name.clone(), part.name.clone(), t);
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
                        pane.part_visibility.clear();
                        pane.plate_visibility.clear();
                    }
                    // Re-upload armor + overlays after visibility change
                    let dp = crate::armor_viewer::common::default_trajectory_display_params(&pane.trajectories);
                    crate::armor_viewer::common::reupload_after_zone_change(
                        pane,
                        &render_state.device,
                        &render_state.queue,
                        &gpu_pipeline,
                        comparison_ships_snapshot,
                        state.ifhe_enabled,
                        &dp,
                    );
                }
            }
        }

        // ── Export confirmation dialog ──
        let mut close_export_dialog = false;
        if let Some(ref export_req) = state.export_confirm {
            let param_index = export_req.param_index.clone();
            let display_name = export_req.display_name.clone();
            let selected_hull = export_req.selected_hull.clone();
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
                                let hull = selected_hull.clone();
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
                                            hull,
                                            textures: true,
                                            damaged: false,
                                            ..Default::default()
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
fn poll_pane_loads(
    dock_state: &mut DockState<ArmorPane>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &GpuPipeline,
    comparison_ships: &[crate::armor_viewer::penetration::ComparisonShip],
    ifhe_enabled: bool,
) {
    for (_, pane) in dock_state.iter_all_tabs_mut() {
        crate::armor_viewer::common::poll_pane_load_receivers(pane, device, queue, pipeline);

        // Poll upgrade-only reload (hull upgrade change)
        if let Some(rx) = &pane.upgrade_load_receiver
            && let Ok(result) = rx.try_recv()
        {
            match result {
                Ok(data) => {
                    apply_upgrade_reload(pane, data, device, queue, pipeline, comparison_ships, ifhe_enabled);
                }
                Err(e) => {
                    tracing::error!("Failed to reload hull upgrade: {e}");
                }
            }
            pane.upgrade_load_receiver = None;
        }
    }
}

/// Upload loaded armor meshes to the viewport's GPU buffers,
/// filtering out triangles belonging to invisible parts or hidden plates.
pub(crate) fn upload_armor_to_viewport(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &GpuPipeline,
) {
    pane.viewport.clear();
    pane.mesh_triangle_info.clear();
    pane.hover_highlight = None;
    pane.sidebar_highlight = None;

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

    // Upload hull visual meshes
    upload_hull_meshes_to_viewport(pane, armor, device, queue, pipeline);

    // Water plane at Y=0 (hull is pre-shifted so waterline sits at origin).
    // Uses world-space mesh so it stays horizontal when ship is rolled.
    if pane.show_waterline {
        let (verts, indices) = create_water_plane(0.0, armor.bounds, pane.waterline_opacity);
        pane.viewport.add_world_space_mesh(device, &verts, &indices, LAYER_HULL);
    }

    pane.viewport.mark_dirty();
}

/// Upload (or re-upload) hull visual meshes to the viewport, tracking their IDs for later removal.
/// Removes any previously tracked hull meshes first.
pub(crate) fn upload_hull_meshes_to_viewport(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &GpuPipeline,
) {
    // Remove old hull meshes
    for mid in pane.hull_mesh_ids.drain(..) {
        pane.viewport.remove_mesh(mid);
    }

    let hull_alpha: f32 = if pane.hull_opaque { 1.0 } else { 0.7 };
    let hull_layer = if pane.hull_opaque { LAYER_DEFAULT } else { LAYER_HULL };
    for mesh in &armor.hull_meshes {
        let visible = pane.hull_visibility.get(&mesh.name).copied().unwrap_or(false);
        if !visible {
            continue;
        }

        let has_uvs = mesh.uvs.len() == mesh.positions.len();
        let texture_data = mesh.mfm_path.as_ref().and_then(|p| armor.hull_textures.get(p));
        let has_texture = texture_data.is_some() && has_uvs;

        // Brightness boost compensates for the shader's 0.7 ambient multiplier.
        // Textured:  3.5 * 0.7 ≈ 2.45 effective (vivid hull textures).
        // Baked/flat: 2.0 * 0.7 ≈ 1.40 effective.
        let hull_brightness: f32 = 2.0;
        let tex_brightness: f32 = 3.5;
        let fallback_color: [f32; 4] =
            [0.6 * hull_brightness, 0.6 * hull_brightness, 0.65 * hull_brightness, hull_alpha];
        let has_baked_colors = mesh.colors.len() == mesh.positions.len();
        let mut vertices: Vec<Vertex> = Vec::with_capacity(mesh.positions.len());
        for i in 0..mesh.positions.len() {
            let mut pos = mesh.positions[i];
            let mut norm = if i < mesh.normals.len() { mesh.normals[i] } else { [0.0, 1.0, 0.0] };

            if let Some(t) = &mesh.transform {
                pos = transform_point(t, pos);
                norm = transform_normal(t, norm);
            }

            let uv = if has_uvs { mesh.uvs[i] } else { [0.0, 0.0] };
            let color = if has_texture {
                [tex_brightness, tex_brightness, tex_brightness, hull_alpha]
            } else if has_baked_colors {
                let c = mesh.colors[i];
                [c[0] * hull_brightness, c[1] * hull_brightness, c[2] * hull_brightness, hull_alpha]
            } else {
                fallback_color
            };
            vertices.push(Vertex { position: pos, normal: norm, color, uv });
        }

        if !mesh.indices.is_empty() {
            let mid = if let Some((w, h, rgba)) = texture_data.filter(|_| has_uvs) {
                let tex_bg = pipeline.create_texture_bind_group(device, queue, rgba, *w, *h);
                pane.viewport.add_textured_non_pickable_mesh(device, &vertices, &mesh.indices, hull_layer, tex_bg)
            } else {
                pane.viewport.add_non_pickable_mesh(device, &vertices, &mesh.indices, hull_layer)
            };
            pane.hull_mesh_ids.push(mid);
        }
    }

    pane.viewport.mark_dirty();
}

/// Initial upload when a ship is first loaded. Sets up part visibility and camera.
pub(crate) fn init_armor_viewport(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &GpuPipeline,
) {
    pane.part_visibility.clear();
    pane.plate_visibility.clear();
    pane.undo_stack.clear();
    let armor_default = pane.default_armor_all_visible;
    for (zone, parts) in &armor.zone_parts {
        for part in parts {
            pane.part_visibility.insert((zone.clone(), part.clone()), armor_default);
        }
    }

    // Hull parts: default to all visible if saved in defaults, otherwise not visible
    pane.hull_visibility.clear();
    let hull_default = pane.default_hull_all_visible;
    for (_group, names) in &armor.hull_part_groups {
        for name in names {
            pane.hull_visibility.insert(name.clone(), hull_default);
        }
    }

    // Splash boxes: default to all visible if show_splash_boxes is set
    pane.splash_box_visibility.clear();
    if pane.show_splash_boxes {
        for (_group, names) in &armor.splash_box_groups {
            for name in names {
                pane.splash_box_visibility.insert(name.clone(), true);
            }
        }
    }

    upload_armor_to_viewport(pane, armor, device, queue, pipeline);

    // Upload splash box wireframes if enabled
    upload_splash_box_wireframes(pane, device, Some(armor));

    // Frame camera on the model
    pane.viewport.camera = crate::viewport_3d::ArcballCamera::from_bounds(armor.bounds.0, armor.bounds.1);
    pane.viewport.mark_dirty();
}

/// Render a single armor pane (viewport only, no sidebar — sidebar is rendered once at the top level).
fn render_armor_pane(ui: &mut egui::Ui, pane: &mut ArmorPane, ctx: &ArmorPaneViewer<'_>) {
    let render_state = ctx.render_state;
    let gpu_pipeline = ctx.gpu_pipeline;
    let mirror_camera_signal = ctx.mirror_camera_signal;
    let active_pane_signal = ctx.active_pane_signal;
    let save_defaults_signal = ctx.save_defaults_signal;
    let export_signal = ctx.export_signal;
    let pen_check_toggle = ctx.pen_check_toggle;
    let analysis_tab_signal = ctx.analysis_tab_signal;
    let comparison_ships = ctx.comparison_ships;
    let ifhe_enabled = ctx.ifhe_enabled;
    let translate_part = ctx.translate_part;
    let comparison_ships_version = ctx.comparison_ships_version;
    let hull_lod_signal = ctx.hull_lod_signal;
    let hull_change_signal = ctx.hull_change_signal;
    let pane_id = pane.id;

    // Full viewport area (no sidebar)
    {
        let vp_ui = ui;

        // Undo/redo keyboard shortcuts (Ctrl+Z / Ctrl+Shift+Z / Ctrl+R)
        let mut zone_changed = crate::armor_viewer::common::handle_undo_redo(vp_ui, pane);

        // Ctrl+T toggles trajectory mode
        if vp_ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::T)) {
            pane.trajectory_mode = !pane.trajectory_mode;
        }

        // Settings toolbar (single row with popover buttons)
        let prev_marker_opacity = pane.marker_opacity;
        let ctrl_s_pressed = vp_ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S));
        let sidebar_hovered_key: std::cell::Cell<Option<SidebarHighlightKey>> = std::cell::Cell::new(None);
        if let Some(armor) = pane.loaded_armor.take() {
            if !armor.zone_parts.is_empty() {
                vp_ui.horizontal(|ui| {
                    // ── Armor Zones button with popover ──
                    let armor_btn =
                        ui.button(icon_str!(icons::SHIELD, "Armor")).on_hover_text("Toggle armor zone visibility");
                    egui::Popup::from_toggle_button_response(&armor_btn)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            let (changed, hkey) = draw_armor_visibility_popover(ui, pane, &armor, translate_part);
                            if changed {
                                zone_changed = true;
                            }
                            if hkey.is_some() {
                                sidebar_hovered_key.set(hkey);
                            }
                        });

                    // ── Hull Model button with popover ──
                    if !armor.hull_part_groups.is_empty() {
                        let hull_btn = ui
                            .button(icon_str!(icons::THREE_D, "Hull"))
                            .on_hover_text("Toggle hull model part visibility");
                        egui::Popup::from_toggle_button_response(&hull_btn)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                let hull_result = draw_hull_visibility_popover(ui, pane, &armor);
                                if hull_result.zone_changed {
                                    zone_changed = true;
                                }
                                if let Some(k) = hull_result.hovered_key {
                                    sidebar_hovered_key.set(Some(k));
                                }
                                if let Some(new_lod) = hull_result.new_lod {
                                    hull_lod_signal.set(Some((pane_id, new_lod)));
                                }
                                if hull_result.hull_changed || hull_result.module_changed {
                                    hull_change_signal.set(Some(pane_id));
                                }
                            });
                    }

                    // ── Splash Boxes button with popover ──
                    if !armor.splash_box_groups.is_empty() {
                        let splash_label = if pane.show_splash_boxes {
                            format!("{} Splash \u{25CF}", icons::CUBE)
                        } else {
                            icon_str!(icons::CUBE, "Splash").to_string()
                        };
                        let splash_btn = ui.button(splash_label).on_hover_text("Toggle splash box visibility");
                        egui::Popup::from_toggle_button_response(&splash_btn)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                let (changed, hkey) = draw_splash_box_visibility_popover(ui, pane, &armor);
                                if changed {
                                    zone_changed = true;
                                }
                                if let Some(k) = hkey {
                                    sidebar_hovered_key.set(Some(k));
                                }
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
                            if draw_display_settings_popover(ui, pane, &armor) {
                                zone_changed = true;
                            }
                            if !pane.trajectories.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.label("Marker Opacity");
                                    ui.add(egui::Slider::new(&mut pane.marker_opacity, 0.0..=1.0).fixed_decimals(2));
                                });
                            }
                            ui.separator();
                            if ui.button("Save as defaults").clicked() {
                                let hull_all_on =
                                    pane.hull_visibility.values().all(|&v| v) && !pane.hull_visibility.is_empty();
                                let armor_all_on = !pane.part_visibility.is_empty()
                                    && pane.part_visibility.values().all(|&v| v)
                                    && !pane.plate_visibility.values().any(|&v| !v);
                                save_defaults_signal.set(Some(ArmorViewerDefaults {
                                    show_plate_edges: pane.show_plate_edges,
                                    show_waterline: pane.show_waterline,
                                    show_zero_mm: pane.show_zero_mm,
                                    armor_opacity: pane.armor_opacity,
                                    waterline_opacity: pane.waterline_opacity,
                                    hull_opaque: pane.hull_opaque,
                                    hull_all_visible: hull_all_on,
                                    armor_all_visible: armor_all_on,
                                    show_splash_boxes: pane.show_splash_boxes,
                                }));
                            }
                        });

                    // ── Export Ship Model button ──
                    if let Some(param_index) = &pane.selected_ship
                        && ui
                            .button(icon_str!(icons::DOWNLOAD_SIMPLE, "Export"))
                            .on_hover_text("Export ship model to OBJ file")
                            .clicked()
                    {
                        let display_name = armor.display_name.clone();
                        export_signal.set(Some(ExportRequest {
                            param_index: param_index.clone(),
                            display_name,
                            selected_hull: pane.selected_hull.clone(),
                        }));
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
                            if pane.trajectory_mode {
                                pane.splash_mode = false; // mutually exclusive
                                analysis_tab_signal.set(Some(AnalysisTab::Trajectory));
                            }
                        }
                    }

                    // ── Splash mode toggle ──
                    {
                        let has_splash = armor.splash_data.is_some();
                        let has_he_shell = comparison_ships.iter().any(|s| {
                            s.shells.iter().any(|sh| sh.ammo_type == AmmoType::HE || sh.ammo_type == AmmoType::SAP)
                        });
                        let splash_label = if pane.splash_mode { "Splash [ON]" } else { "Splash" };
                        let btn = egui::Button::new(splash_label);
                        let btn = if pane.splash_mode { btn.fill(egui::Color32::from_rgb(80, 40, 10)) } else { btn };
                        let enabled = has_splash && has_he_shell;
                        let resp = ui.add_enabled(enabled, btn);
                        let resp = if !has_splash {
                            resp.on_disabled_hover_text("No splash data for this ship")
                        } else if !has_he_shell {
                            resp.on_disabled_hover_text("Add a ship with HE/SAP shells to the comparison list")
                        } else {
                            resp.on_hover_text("Click armor to visualize HE splash damage volume")
                        };
                        if resp.clicked() {
                            pane.splash_mode = !pane.splash_mode;
                            if pane.splash_mode {
                                pane.trajectory_mode = false; // mutually exclusive
                                analysis_tab_signal.set(Some(AnalysisTab::Splash));
                            }
                        }
                    }

                    // ── Roll slider ──
                    ui.separator();
                    let roll_changed = draw_roll_slider(ui, &mut pane.viewport);
                    if roll_changed && !pane.trajectories.is_empty() {
                        let new_roll = pane.viewport.model_roll;
                        let cam_dist = pane.viewport.camera.distance;
                        let mo = pane.marker_opacity;
                        for ti in 0..pane.trajectories.len() {
                            recompute_trajectory_for_roll(
                                &mut pane.trajectories[ti],
                                new_roll,
                                comparison_ships,
                                &mut pane.viewport,
                                &pane.mesh_triangle_info,
                                Some(&armor),
                                &render_state.device,
                                cam_dist,
                                mo,
                                comparison_ships_version,
                            );
                        }
                    }
                });
                vp_ui.separator();
            }
            pane.loaded_armor = Some(armor);
        }
        if zone_changed {
            let dp = crate::armor_viewer::common::default_trajectory_display_params(&pane.trajectories);
            crate::armor_viewer::common::reupload_after_zone_change(
                pane,
                &render_state.device,
                &render_state.queue,
                gpu_pipeline,
                comparison_ships,
                ifhe_enabled,
                &dp,
            );
        }

        // Sidebar hover highlight lifecycle
        if let Some(armor) = pane.loaded_armor.take() {
            crate::armor_viewer::common::update_sidebar_highlight(
                pane,
                &armor,
                sidebar_hovered_key.into_inner(),
                &render_state.device,
            );
            pane.loaded_armor = Some(armor);
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
                            !(0.7..=1.4).contains(&ratio)
                        });
                    if needs_rescale {
                        let dp = crate::armor_viewer::common::default_trajectory_display_params(&pane.trajectories);
                        crate::armor_viewer::common::reupload_trajectory_meshes(pane, &render_state.device, &dp, true);
                    }
                }

                // Clicking on the viewport also makes this the active pane
                if response.clicked() || response.drag_started() {
                    active_pane_signal.set(Some(pane_id));
                }

                // Trajectory mode: click to cast ray through model
                // Normal click = replace all trajectories; Shift+click = add another
                if pane.trajectory_mode && response.clicked() {
                    let shift_held = vp_ui.input(|i| i.modifiers.shift);
                    if let Some(click_pos) = response.interact_pointer_pos() {
                        use crate::viewport_3d::camera::normalize;
                        use crate::viewport_3d::camera::scale;
                        use crate::viewport_3d::camera::sub;

                        // Step 1: Use camera ray to find the click point on the hull surface
                        let camera_hit = pane.viewport.pick(click_pos, response.rect);

                        if let Some(surface_hit) = camera_hit {
                            let click_point = surface_hit.world_position;
                            let range_meters = pane.ballistic_range.to_meters();

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
                                .find(|s| s.ammo_type == AmmoType::AP)
                                .or_else(|| comparison_ships.iter().flat_map(|s| s.shells.iter()).next());

                            let ballistic_impact = first_shell.and_then(|shell| {
                                let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
                                crate::armor_viewer::ballistics::solve_for_range(&params, range_meters)
                            });

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
                            let traj_hits = crate::armor_viewer::common::build_traj_hits(
                                relevant_hits,
                                &pane.mesh_triangle_info,
                                &shell_dir,
                            );

                            // Generate per-ship 3D arc points
                            let model_extent = pane.loaded_armor.as_ref().map(|a| a.max_extent_xz()).unwrap_or(10.0);
                            let first_hit_pos = traj_hits.first().map(|h| h.position).unwrap_or(click_point);

                            let mut ship_arcs: Vec<crate::armor_viewer::penetration::ShipArc> = Vec::new();
                            if range_meters.value() > 0.0 {
                                for (ship_idx, ship) in comparison_ships.iter().enumerate() {
                                    let shell = ship
                                        .shells
                                        .iter()
                                        .find(|s| s.ammo_type == AmmoType::AP)
                                        .or_else(|| ship.shells.first());
                                    if let Some(shell) = shell {
                                        let params =
                                            crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
                                        if let Some(impact) =
                                            crate::armor_viewer::ballistics::solve_for_range(&params, range_meters)
                                        {
                                            let arc_points_3d = crate::armor_viewer::common::build_ballistic_arc_3d(
                                                &params,
                                                &impact,
                                                approach_xz,
                                                first_hit_pos,
                                                model_extent,
                                            );
                                            ship_arcs.push(crate::armor_viewer::penetration::ShipArc {
                                                ship_index: ship_idx,
                                                arc_points_3d,
                                                ballistic_impact: Some(impact),
                                            });
                                        }
                                    }
                                }
                            }

                            let total_armor: f32 = traj_hits.iter().map(|h| h.thickness_mm).sum();

                            // Compute detonation points and last visible hit for AP shells
                            // Each shell gets its own ballistic solve (different velocity/angle at range)
                            let mut detonation_points: Vec<crate::armor_viewer::penetration::DetonationMarker> =
                                Vec::new();
                            let mut last_visible_hit: Option<usize> = None;
                            for (ship_idx, ship) in comparison_ships.iter().enumerate() {
                                for shell in ship.shells.iter().filter(|s| s.ammo_type == AmmoType::AP) {
                                    let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
                                    let shell_impact = if range_meters.value() > 0.0 {
                                        crate::armor_viewer::ballistics::solve_for_range(&params, range_meters)
                                    } else {
                                        None
                                    };
                                    if let Some(ref impact) = shell_impact {
                                        let ap = crate::armor_viewer::common::simulate_ap_shell(
                                            &params, impact, &traj_hits, &shell_dir,
                                        );
                                        if let Some(pos) = ap.detonation_point {
                                            detonation_points.push(
                                                crate::armor_viewer::penetration::DetonationMarker {
                                                    position: pos,
                                                    ship_index: ship_idx,
                                                },
                                            );
                                        }
                                        if let Some(idx) = ap.last_visible_hit {
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
                                ship_arcs,
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
                                1.0,
                            );
                            pane.trajectories.push(crate::armor_viewer::state::StoredTrajectory {
                                meta: crate::armor_viewer::penetration::TrajectoryMeta {
                                    id: traj_id,
                                    color_index,
                                    range: pane.ballistic_range,
                                },
                                result,
                                mesh_id: Some(mesh_id),
                                last_visible_hit,
                                marker_cam_dist: cam_dist,
                                show_plates_active: false,
                                show_zones_active: false,
                                shell_sim_cache: None,
                                created_at_roll: pane.viewport.model_roll,
                            });
                            update_shell_sim_cache(
                                pane.trajectories.last_mut().unwrap(),
                                comparison_ships,
                                comparison_ships_version,
                            );
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

                // Splash mode: click to place splash volume
                if pane.splash_mode
                    && response.clicked()
                    && let Some(click_pos) = response.interact_pointer_pos()
                {
                    let camera_hit = pane.viewport.pick(click_pos, response.rect);
                    if let Some(surface_hit) = camera_hit {
                        // Clear previous splash overlays
                        for mid in pane.splash_mesh_ids.drain(..) {
                            pane.viewport.remove_mesh(mid);
                        }

                        let click_point = surface_hit.world_position;

                        // Use the largest-caliber HE/SAP shell to size the splash cube
                        let max_caliber = comparison_ships
                            .iter()
                            .flat_map(|s| s.shells.iter())
                            .filter(|s| s.ammo_type == AmmoType::HE || s.ammo_type == AmmoType::SAP)
                            .map(|s| s.caliber)
                            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

                        if let Some(max_caliber) = max_caliber {
                            let half_ext = crate::armor_viewer::splash::splash_half_extent(max_caliber);

                            // Compute zone-level splash result (shell-independent)
                            let splash_result = if let Some(armor) = &pane.loaded_armor {
                                armor.splash_data.as_ref().map(|sd| {
                                    crate::armor_viewer::splash::compute_splash(
                                        click_point,
                                        half_ext,
                                        sd,
                                        armor.hit_locations.as_ref(),
                                    )
                                })
                            } else {
                                None
                            };

                            // Always draw the wireframe cube so the user can
                            // see the splash volume even when no boxes are hit.
                            let (cube_verts, cube_indices) = crate::armor_viewer::splash::build_splash_cube_mesh(
                                click_point,
                                half_ext,
                                crate::armor_viewer::splash::SPLASH_CUBE_COLOR,
                            );
                            if !cube_verts.is_empty() {
                                let cube_mid =
                                    pane.viewport.add_overlay_mesh(&render_state.device, &cube_verts, &cube_indices);
                                pane.viewport.set_world_space(cube_mid, true);
                                pane.splash_mesh_ids.push(cube_mid);
                            }

                            let has_zones = splash_result.as_ref().is_some_and(|r| !r.hit_zones.is_empty());

                            // Only highlight armor triangles when splash boxes
                            // were actually hit — coloring plates without a zone
                            // match would imply damage the game wouldn't deal.
                            if has_zones {
                                let first_shell = comparison_ships
                                    .iter()
                                    .flat_map(|s| s.shells.iter())
                                    .find(|s| s.ammo_type == AmmoType::HE || s.ammo_type == AmmoType::SAP);
                                if let (Some(armor), Some(shell)) = (&pane.loaded_armor, first_shell) {
                                    let (hl_verts, hl_indices, tri_total, tri_pen) =
                                        crate::armor_viewer::splash::build_splash_highlight_mesh(
                                            &armor.meshes,
                                            click_point,
                                            half_ext,
                                            shell,
                                            ifhe_enabled,
                                        );
                                    if !hl_verts.is_empty() {
                                        let hl_mid = pane.viewport.add_overlay_mesh(
                                            &render_state.device,
                                            &hl_verts,
                                            &hl_indices,
                                        );
                                        pane.viewport.set_world_space(hl_mid, true);
                                        pane.splash_mesh_ids.push(hl_mid);
                                    }

                                    if let Some(mut result) = splash_result {
                                        result.triangles_in_volume = tri_total;
                                        result.triangles_penetrated = tri_pen;
                                        pane.splash_result = Some(result);
                                    }
                                } else {
                                    pane.splash_result = splash_result;
                                }
                            } else {
                                pane.splash_result = splash_result;
                            }
                        }
                    } else {
                        // Clicked empty space: clear splash
                        for mid in pane.splash_mesh_ids.drain(..) {
                            pane.viewport.remove_mesh(mid);
                        }
                        pane.splash_result = None;
                    }
                }

                // Hover tooltip, click-to-toggle, right-click context menu, hover highlight
                if handle_plate_interaction(
                    vp_ui,
                    &response,
                    pane,
                    &render_state.device,
                    translate_part,
                    !pane.trajectory_mode && !pane.splash_mode,
                    comparison_ships,
                    ifhe_enabled,
                ) {
                    zone_changed = true;
                }

                // Recompute all trajectory arcs if comparison ships changed
                if pane.comparison_ships_version != comparison_ships_version && !pane.trajectories.is_empty() {
                    pane.comparison_ships_version = comparison_ships_version;
                    let cam_dist = pane.viewport.camera.distance;
                    let mo = pane.marker_opacity;
                    for ti in 0..pane.trajectories.len() {
                        recompute_trajectory_for_range(
                            &mut pane.trajectories[ti],
                            comparison_ships,
                            &mut pane.viewport,
                            pane.loaded_armor.as_ref(),
                            &render_state.device,
                            cam_dist,
                            mo,
                            comparison_ships_version,
                        );
                    }
                }
                pane.comparison_ships_version = comparison_ships_version;

                // Handle marker opacity change — re-upload all trajectory meshes
                let marker_opacity_changed = (pane.marker_opacity - prev_marker_opacity).abs() > 0.001;
                if marker_opacity_changed && !pane.trajectories.is_empty() {
                    let dp = crate::armor_viewer::common::default_trajectory_display_params(&pane.trajectories);
                    crate::armor_viewer::common::reupload_trajectory_meshes(pane, &render_state.device, &dp, true);
                }

                // Draw splash box labels on top of the viewport
                draw_splash_box_labels(pane, vp_ui.painter(), response.rect);

                // Draw disclaimer watermark
                draw_viewport_watermark(vp_ui.painter(), response.rect);
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
            let dp = crate::armor_viewer::common::default_trajectory_display_params(&pane.trajectories);
            crate::armor_viewer::common::reupload_after_zone_change(
                pane,
                &render_state.device,
                &render_state.queue,
                gpu_pipeline,
                comparison_ships,
                ifhe_enabled,
                &dp,
            );
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
    load_ship_for_pane_with_lod(pane, param_index, display_name, ship_assets, pane.hull_lod);
}

fn load_ship_for_pane_with_lod(
    pane: &mut ArmorPane,
    param_index: &str,
    display_name: &str,
    ship_assets: &Arc<wowsunpack::export::ship::ShipAssets>,
    lod: usize,
) {
    // Snapshot hull visibility state so the next ship inherits the same defaults
    if !pane.hull_visibility.is_empty() {
        pane.default_hull_all_visible = pane.hull_visibility.values().all(|&v| v);
    }

    pane.hull_lod = lod;
    pane.selected_ship = Some(param_index.to_string());
    pane.loading = true;
    pane.loaded_armor = None;
    pane.viewport.clear();
    pane.mesh_triangle_info.clear();
    pane.hovered_info = None;
    pane.hover_highlight = None;
    pane.plate_visibility.clear();
    pane.part_visibility.clear();
    pane.trajectories.clear();
    pane.splash_mode = false;
    pane.splash_result = None;
    for mid in pane.splash_mesh_ids.drain(..) {
        pane.viewport.remove_mesh(mid);
    }
    for mid in pane.splash_box_mesh_ids.drain(..) {
        pane.viewport.remove_mesh(mid);
    }
    pane.splash_box_labels.clear();
    pane.splash_box_visibility.clear();

    let assets = ship_assets.clone();
    let ship_display_name = display_name.to_string();
    let (tx, rx) = mpsc::channel();
    let requested_lod = lod;

    // Resolve the Vehicle from GameParams on the main thread so we can use
    // load_ship_from_vehicle (avoids the fuzzy find_ship lookup entirely).
    use wowsunpack::game_params::types::GameParamProvider;
    let param = ship_assets.metadata().game_param_by_index(param_index);
    let vehicle = param.as_ref().and_then(|p| p.vehicle().cloned());
    let selected_hull = pane.selected_hull.clone();
    let module_overrides = pane.selected_modules.clone();

    // Build sorted hull upgrade list and dock_y_offset via shared helpers
    let hull_upgrade_names =
        vehicle.as_ref().map(crate::armor_viewer::common::build_hull_upgrade_names).unwrap_or_default();

    let dock_y_offset =
        vehicle.as_ref().and_then(|v| crate::armor_viewer::common::resolve_dock_y_offset(v, &selected_hull));

    // Extract module alternatives from the selected hull upgrade config.
    // Only includes component types with >1 option (e.g. artillery, torpedoes).
    let module_alternatives: Vec<(wowsunpack::game_params::keys::ComponentType, Vec<String>)> = param
        .as_ref()
        .and_then(|p| {
            p.vehicle().and_then(|v| v.hull_upgrades()).and_then(|upgrades| {
                let config = if let Some(sel) = &selected_hull {
                    upgrades.get(sel)
                } else {
                    let mut keys: Vec<&String> = upgrades.keys().collect();
                    keys.sort();
                    keys.first().and_then(|k| upgrades.get(*k))
                };
                config.map(|c| c.component_alternatives.iter().map(|(k, v)| (*k, v.clone())).collect())
            })
        })
        .unwrap_or_default();

    std::thread::spawn(move || {
        let result = match vehicle {
            Some(v) => {
                let load_opts = crate::armor_viewer::common::ShipLoadOptions {
                    display_name: ship_display_name,
                    lod: requested_lod,
                    selected_hull,
                    module_overrides,
                    include_splash_data: true,
                    include_hit_locations: true,
                    module_alternatives,
                    hull_upgrade_names,
                    dock_y_offset,
                };
                crate::armor_viewer::common::load_ship_armor(&v, &assets, load_opts)
            }
            None => Err("No vehicle found for param index".to_string()),
        };
        let _ = tx.send(result);
    });

    pane.load_receiver = Some(rx);
}

/// Start a background hull-only reload at a different LOD level without reloading armor/splash data.
/// The caller should poll `pane.hull_load_receiver` each frame and call `apply_hull_reload` when data arrives.
pub(crate) fn start_hull_lod_reload(
    pane: &mut ArmorPane,
    ship_assets: &Arc<wowsunpack::export::ship::ShipAssets>,
    param_index: &str,
    lod: usize,
) {
    pane.hull_lod = lod;

    let assets = ship_assets.clone();
    let (tx, rx) = mpsc::channel();
    let requested_lod = lod;

    use wowsunpack::game_params::types::GameParamProvider;
    let param = ship_assets.metadata().game_param_by_index(param_index);
    let vehicle = param.as_ref().and_then(|p| p.vehicle().cloned());
    let waterline_dy = pane.loaded_armor.as_ref().map(|a| a.waterline_dy).unwrap_or(0.0);
    let selected_hull = pane.selected_hull.clone();
    let module_overrides = pane.selected_modules.clone();

    std::thread::spawn(move || {
        let result = (|| {
            let vehicle = vehicle.ok_or_else(|| "No vehicle found for param index".to_string())?;
            let options = wowsunpack::export::ship::ShipExportOptions {
                lod: requested_lod,
                hull: selected_hull,
                textures: false,
                damaged: false,
                module_overrides,
            };
            let ctx = assets.load_ship_from_vehicle(&vehicle, &options).map_err(|e| format!("{e:?}"))?;
            let mut hull_meshes = ctx.interactive_hull_meshes().map_err(|e| format!("{e:?}"))?;

            // Apply waterline offset to match the already-shifted armor meshes
            if waterline_dy.abs() > 1e-7 {
                for mesh in &mut hull_meshes {
                    for pos in &mut mesh.positions {
                        pos[1] += waterline_dy;
                    }
                }
            }

            let hull_part_groups = crate::ui::armor_viewer::build_hull_part_groups(&hull_meshes);
            let hull_lod_count = ctx.hull_lod_count();

            // Load hull albedo textures
            let mut hull_textures = std::collections::HashMap::new();
            for mesh in &hull_meshes {
                if let Some(mfm) = &mesh.mfm_path {
                    if hull_textures.contains_key(mfm) {
                        continue;
                    }
                    if let Some(dds_bytes) = wowsunpack::export::texture::load_base_albedo_bytes(assets.vfs(), mfm)
                        && let Ok(dds) = image_dds::ddsfile::Dds::read(&mut std::io::Cursor::new(&dds_bytes))
                        && let Ok(img) = image_dds::image_from_dds(&dds, 0)
                    {
                        let w = img.width();
                        let h = img.height();
                        hull_textures.insert(mfm.clone(), (w, h, img.into_raw()));
                    }
                }
            }

            Ok(HullReloadData { hull_meshes, hull_part_groups, hull_textures, hull_lod: requested_lod, hull_lod_count })
        })();
        let _ = tx.send(result);
    });

    pane.hull_load_receiver = Some(rx);
}

/// Apply hull reload data to an existing LoadedShipArmor and re-upload hull meshes to the viewport.
pub(crate) fn apply_hull_reload(
    pane: &mut ArmorPane,
    data: HullReloadData,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &GpuPipeline,
) {
    if let Some(armor) = &mut pane.loaded_armor {
        armor.hull_meshes = data.hull_meshes;
        armor.hull_part_groups = data.hull_part_groups;
        armor.hull_textures = data.hull_textures;
        armor.hull_lod = data.hull_lod;
        armor.hull_lod_count = data.hull_lod_count;

        // Update hull visibility map for any new/changed parts
        let hull_default = pane.hull_visibility.values().any(|&v| v);
        pane.hull_visibility.retain(|name, _| armor.hull_part_groups.iter().any(|(_, names)| names.contains(name)));
        for (_group, names) in &armor.hull_part_groups {
            for name in names {
                pane.hull_visibility.entry(name.clone()).or_insert(hull_default);
            }
        }
    }

    if pane.loaded_armor.is_some() {
        // Temporarily take armor to satisfy borrow checker (need &armor + &mut pane)
        let armor_ref = pane.loaded_armor.take().unwrap();
        upload_hull_meshes_to_viewport(pane, &armor_ref, device, queue, pipeline);
        pane.loaded_armor = Some(armor_ref);
    }
}

/// Start a background upgrade-only reload: re-exports with the new hull selection,
/// replacing turret models and turret armor without a full ship reload.
/// The caller should poll `pane.upgrade_load_receiver` each frame and call `apply_upgrade_reload` when data arrives.
fn start_upgrade_reload(
    pane: &mut ArmorPane,
    ship_assets: &Arc<wowsunpack::export::ship::ShipAssets>,
    param_index: &str,
) {
    let assets = ship_assets.clone();
    let (tx, rx) = mpsc::channel();
    let selected_hull = pane.selected_hull.clone();
    let module_overrides = pane.selected_modules.clone();
    let lod = pane.hull_lod;
    let waterline_dy = pane.loaded_armor.as_ref().map(|a| a.waterline_dy).unwrap_or(0.0);

    use wowsunpack::game_params::types::GameParamProvider;
    let param = ship_assets.metadata().game_param_by_index(param_index);
    let vehicle = param.as_ref().and_then(|p| p.vehicle().cloned());

    std::thread::spawn(move || {
        let result = (|| {
            let vehicle = vehicle.ok_or_else(|| "No vehicle found for param index".to_string())?;
            let options = wowsunpack::export::ship::ShipExportOptions {
                lod,
                hull: selected_hull.clone(),
                textures: false,
                damaged: false,
                module_overrides,
            };
            let ctx = assets.load_ship_from_vehicle(&vehicle, &options).map_err(|e| format!("{e:?}"))?;

            // Reload armor meshes (hull armor unchanged, turret armor re-mounted)
            let mut armor_meshes = ctx.interactive_armor_meshes().map_err(|e| format!("{e:?}"))?;

            // Apply waterline offset to match existing shifted coordinates
            if waterline_dy.abs() > 1e-7 {
                for mesh in &mut armor_meshes {
                    for pos in &mut mesh.positions {
                        pos[1] += waterline_dy;
                    }
                }
            }

            // Build zone/part/plate metadata from new armor meshes
            let mut zone_parts_map: std::collections::HashMap<String, std::collections::HashSet<String>> =
                std::collections::HashMap::new();
            let mut zone_part_plates_map: std::collections::HashMap<
                String,
                std::collections::HashMap<String, std::collections::BTreeSet<i32>>,
            > = std::collections::HashMap::new();
            for mesh in &armor_meshes {
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

            // Reload hull visual meshes
            let mut hull_meshes = ctx.interactive_hull_meshes().map_err(|e| format!("{e:?}"))?;

            // Apply waterline offset
            if waterline_dy.abs() > 1e-7 {
                for mesh in &mut hull_meshes {
                    for pos in &mut mesh.positions {
                        pos[1] += waterline_dy;
                    }
                }
            }

            let hull_part_groups = build_hull_part_groups(&hull_meshes);

            // Load hull albedo textures for any new turret mfm paths
            let mut hull_textures = std::collections::HashMap::new();
            for mesh in &hull_meshes {
                if let Some(mfm) = &mesh.mfm_path {
                    if hull_textures.contains_key(mfm) {
                        continue;
                    }
                    if let Some(dds_bytes) = wowsunpack::export::texture::load_base_albedo_bytes(assets.vfs(), mfm)
                        && let Ok(dds) = image_dds::ddsfile::Dds::read(&mut std::io::Cursor::new(&dds_bytes))
                        && let Ok(img) = image_dds::image_from_dds(&dds, 0)
                    {
                        let w = img.width();
                        let h = img.height();
                        hull_textures.insert(mfm.clone(), (w, h, img.into_raw()));
                    }
                }
            }

            // Extract module alternatives from the selected hull upgrade config.
            let module_alternatives: Vec<(wowsunpack::game_params::keys::ComponentType, Vec<String>)> = vehicle
                .hull_upgrades()
                .and_then(|upgrades| {
                    let config = if let Some(sel) = &selected_hull {
                        upgrades.get(sel)
                    } else {
                        let mut keys: Vec<&String> = upgrades.keys().collect();
                        keys.sort();
                        keys.first().and_then(|k| upgrades.get(*k))
                    };
                    config.map(|c| c.component_alternatives.iter().map(|(k, v)| (*k, v.clone())).collect())
                })
                .unwrap_or_default();

            Ok(UpgradeReloadData {
                armor_meshes,
                zones,
                zone_parts,
                zone_part_plates,
                hull_meshes,
                hull_part_groups,
                hull_textures,
                loaded_hull: selected_hull,
                module_alternatives,
            })
        })();
        let _ = tx.send(result);
    });

    pane.upgrade_load_receiver = Some(rx);
}

/// Apply upgrade reload data to an existing LoadedShipArmor and re-upload meshes to the viewport.
/// Preserves camera, trajectories, splash overlays, and display settings.
fn apply_upgrade_reload(
    pane: &mut ArmorPane,
    data: UpgradeReloadData,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &GpuPipeline,
    comparison_ships: &[crate::armor_viewer::penetration::ComparisonShip],
    ifhe_enabled: bool,
) {
    if let Some(armor) = &mut pane.loaded_armor {
        // Update armor mesh data
        armor.meshes = data.armor_meshes;
        armor.zones = data.zones;
        armor.zone_parts = data.zone_parts;
        armor.zone_part_plates = data.zone_part_plates;

        // Update hull mesh data
        armor.hull_meshes = data.hull_meshes;
        armor.hull_part_groups = data.hull_part_groups;
        armor.hull_textures = data.hull_textures;
        armor.loaded_hull = data.loaded_hull;
        armor.module_alternatives = data.module_alternatives;

        // Preserve visibility for parts that still exist, default new parts to visible
        pane.part_visibility
            .retain(|key, _| armor.zone_parts.iter().any(|(zone, parts)| zone == &key.0 && parts.contains(&key.1)));
        for (zone, parts) in &armor.zone_parts {
            for part in parts {
                pane.part_visibility.entry((zone.clone(), part.clone())).or_insert(true);
            }
        }
        pane.plate_visibility.retain(|key, _| {
            armor.zone_part_plates.iter().any(|zone| {
                zone.name == key.0 && zone.parts.iter().any(|part| part.name == key.1 && part.plates.contains(&key.2))
            })
        });
        pane.undo_stack.clear();

        // Update hull visibility map (retain existing, add new with default)
        let hull_default = pane.hull_visibility.values().any(|&v| v);
        pane.hull_visibility.retain(|name, _| armor.hull_part_groups.iter().any(|(_, names)| names.contains(name)));
        for (_group, names) in &armor.hull_part_groups {
            for name in names {
                pane.hull_visibility.entry(name.clone()).or_insert(hull_default);
            }
        }
    }

    // Re-upload armor + all overlays (viewport.clear() destroys everything)
    let dp = crate::armor_viewer::common::default_trajectory_display_params(&pane.trajectories);
    crate::armor_viewer::common::reupload_after_zone_change(
        pane,
        device,
        queue,
        pipeline,
        comparison_ships,
        ifhe_enabled,
        &dp,
    );
}

/// Helper to create an egui Color32 from an [f32; 4] RGBA color.
pub(crate) fn color32_from_f32(c: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, 255)
}

/// Paint a small color swatch rectangle inline.
pub(crate) fn paint_swatch(ui: &mut egui::Ui, color: egui::Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, color);
}

/// Show tooltip for a hovered armor triangle.
pub(crate) fn show_armor_tooltip(
    ui: &mut egui::Ui,
    info: &ArmorTriangleTooltip,
    comparison_ships: &[crate::armor_viewer::penetration::ComparisonShip],
    ifhe_enabled: bool,
    translate: &dyn Fn(&str) -> String,
) {
    use crate::armor_viewer::penetration::PenResult;
    use crate::armor_viewer::penetration::check_penetration;
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
                    Some(PenResult::Penetrates) => ("\u{2705}", egui::Color32::from_rgb(100, 220, 100)),
                    Some(PenResult::Bounces) => ("\u{274C}", egui::Color32::from_rgb(220, 100, 100)),
                    Some(PenResult::AngleDependent) => ("\u{2796}", egui::Color32::GRAY),
                    None => ("\u{2753}", egui::Color32::GRAY),
                };
                let pen_info = match &shell.ammo_type {
                    AmmoType::HE => {
                        let pen = if ifhe_enabled {
                            shell.he_pen_mm.unwrap_or(0.0) * 1.25
                        } else {
                            shell.he_pen_mm.unwrap_or(0.0)
                        };
                        format!("{:.0}mm pen", pen)
                    }
                    AmmoType::SAP => format!("{:.0}mm pen", shell.sap_pen_mm.unwrap_or(0.0)),
                    AmmoType::AP => {
                        if shell.caliber.value() > info.thickness_mm * 14.3 {
                            "overmatch".to_string()
                        } else {
                            "angle-dependent".to_string()
                        }
                    }
                    AmmoType::Unknown(_) => String::new(),
                };
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon));
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {:.0}mm ({})",
                            shell.ammo_type.display_name(),
                            shell.caliber.value(),
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
pub(crate) fn upload_plate_highlight(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    key: &PlateKey,
    device: &wgpu::Device,
    highlight_color: [f32; 4],
) -> MeshId {
    let normal_offset = TRAJECTORY_NORMAL_OFFSET;

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

                    vertices.push(Vertex { position: pos, normal: norm, color: highlight_color, uv: [0.0, 0.0] });
                }
            }
            indices.extend_from_slice(&[new_base, new_base + 1, new_base + 2]);
        }
    }

    pane.viewport.add_overlay_mesh(device, &vertices, &indices)
}

pub const SIDEBAR_HIGHLIGHT_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.35];

/// Upload a highlight overlay for all visible armor triangles in a given zone.
pub fn upload_zone_highlight(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    zone: &str,
    device: &wgpu::Device,
) -> MeshId {
    let normal_offset = TRAJECTORY_NORMAL_OFFSET;
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for mesh in &armor.meshes {
        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            if info.zone != zone {
                continue;
            }
            if !pane.show_zero_mm && info.thickness_mm.abs() < 0.05 {
                continue;
            }
            if pane.show_hidden_only && !info.hidden {
                continue;
            }
            let part_key = (info.zone.clone(), info.material_name.clone());
            if !pane.part_visibility.get(&part_key).copied().unwrap_or(true) {
                continue;
            }
            let thickness_key = (info.thickness_mm * 10.0).round() as i32;
            let plate_key: PlateKey = (info.zone.clone(), info.material_name.clone(), thickness_key);
            if !pane.plate_visibility.get(&plate_key).copied().unwrap_or(true) {
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
                    pos[0] += norm[0] * normal_offset;
                    pos[1] += norm[1] * normal_offset;
                    pos[2] += norm[2] * normal_offset;
                    vertices.push(Vertex {
                        position: pos,
                        normal: norm,
                        color: SIDEBAR_HIGHLIGHT_COLOR,
                        uv: [0.0, 0.0],
                    });
                }
            }
            indices.extend_from_slice(&[new_base, new_base + 1, new_base + 2]);
        }
    }

    pane.viewport.add_overlay_mesh(device, &vertices, &indices)
}

/// Upload a highlight overlay for all visible armor triangles matching a (zone, part/material).
pub fn upload_part_highlight(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    zone: &str,
    part: &str,
    device: &wgpu::Device,
) -> MeshId {
    let normal_offset = TRAJECTORY_NORMAL_OFFSET;
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for mesh in &armor.meshes {
        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            if info.zone != zone || info.material_name != part {
                continue;
            }
            if !pane.show_zero_mm && info.thickness_mm.abs() < 0.05 {
                continue;
            }
            if pane.show_hidden_only && !info.hidden {
                continue;
            }
            let part_key = (info.zone.clone(), info.material_name.clone());
            if !pane.part_visibility.get(&part_key).copied().unwrap_or(true) {
                continue;
            }
            let thickness_key = (info.thickness_mm * 10.0).round() as i32;
            let plate_key: PlateKey = (info.zone.clone(), info.material_name.clone(), thickness_key);
            if !pane.plate_visibility.get(&plate_key).copied().unwrap_or(true) {
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
                    pos[0] += norm[0] * normal_offset;
                    pos[1] += norm[1] * normal_offset;
                    pos[2] += norm[2] * normal_offset;
                    vertices.push(Vertex {
                        position: pos,
                        normal: norm,
                        color: SIDEBAR_HIGHLIGHT_COLOR,
                        uv: [0.0, 0.0],
                    });
                }
            }
            indices.extend_from_slice(&[new_base, new_base + 1, new_base + 2]);
        }
    }

    pane.viewport.add_overlay_mesh(device, &vertices, &indices)
}

/// Upload a highlight overlay for a specific hull mesh by name.
pub fn upload_hull_highlight(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    names: &[&str],
    device: &wgpu::Device,
) -> MeshId {
    let normal_offset = TRAJECTORY_NORMAL_OFFSET;
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for mesh in &armor.hull_meshes {
        if !names.contains(&mesh.name.as_str()) {
            continue;
        }
        for tri_start in (0..mesh.indices.len()).step_by(3) {
            if tri_start + 2 >= mesh.indices.len() {
                break;
            }
            let new_base = vertices.len() as u32;
            for k in 0..3 {
                let orig_idx = mesh.indices[tri_start + k] as usize;
                if orig_idx < mesh.positions.len() {
                    let mut pos = mesh.positions[orig_idx];
                    let mut norm = if orig_idx < mesh.normals.len() { mesh.normals[orig_idx] } else { [0.0, 1.0, 0.0] };
                    if let Some(t) = &mesh.transform {
                        pos = transform_point(t, pos);
                        norm = transform_normal(t, norm);
                    }
                    pos[0] += norm[0] * normal_offset;
                    pos[1] += norm[1] * normal_offset;
                    pos[2] += norm[2] * normal_offset;
                    vertices.push(Vertex {
                        position: pos,
                        normal: norm,
                        color: SIDEBAR_HIGHLIGHT_COLOR,
                        uv: [0.0, 0.0],
                    });
                }
            }
            indices.extend_from_slice(&[new_base, new_base + 1, new_base + 2]);
        }
    }

    pane.viewport.add_overlay_mesh(device, &vertices, &indices)
}

/// Draw the hull visibility popover content (groups + individual meshes with hover detection).
pub(crate) fn draw_hull_visibility_popover(
    ui: &mut egui::Ui,
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
) -> HullPopoverResult {
    let mut result = HullPopoverResult::default();
    let hovered: std::cell::Cell<Option<SidebarHighlightKey>> = std::cell::Cell::new(None);

    let all_hull_names: Vec<&String> = armor.hull_part_groups.iter().flat_map(|(_, names)| names).collect();

    ui.horizontal(|ui| {
        if ui.small_button("All").clicked() {
            for name in &all_hull_names {
                pane.hull_visibility.insert((*name).clone(), true);
            }
            result.zone_changed = true;
        }
        if ui.small_button("None").clicked() {
            for name in &all_hull_names {
                pane.hull_visibility.insert((*name).clone(), false);
            }
            result.zone_changed = true;
        }
        if ui.checkbox(&mut pane.hull_opaque, "Opaque").changed() {
            result.zone_changed = true;
        }
    });

    // Hull upgrade selector
    if armor.hull_upgrade_names.len() > 1 {
        ui.horizontal(|ui| {
            ui.label("Upgrade:");
            for (key, label) in &armor.hull_upgrade_names {
                let is_selected = pane.selected_hull.as_ref() == Some(key)
                    || (pane.selected_hull.is_none() && *key == armor.hull_upgrade_names[0].0);
                if ui.selectable_label(is_selected, label).clicked() && !is_selected {
                    pane.selected_hull = Some(key.clone());
                    pane.selected_modules.clear(); // alternatives may differ per hull
                    result.hull_changed = true;
                }
            }
        });
    }

    // Module alternative selectors (e.g. artillery, torpedoes with multiple options)
    for (ct, alternatives) in &armor.module_alternatives {
        if alternatives.len() > 1 {
            ui.horizontal(|ui| {
                ui.label(format!("{}:", ct));
                for (i, name) in alternatives.iter().enumerate() {
                    let is_selected = pane.selected_modules.get(ct).map_or(i == 0, |sel| sel == name);
                    let label = format!("{} {}", ct, (b'A' + i as u8) as char);
                    if ui.selectable_label(is_selected, &label).on_hover_text(name).clicked() && !is_selected {
                        pane.selected_modules.insert(*ct, name.clone());
                        result.module_changed = true;
                    }
                }
            });
        }
    }

    // LOD selector
    if armor.hull_lod_count > 1 {
        ui.horizontal(|ui| {
            ui.label("LOD:");
            for i in 0..armor.hull_lod_count {
                let label = if i == 0 { "0 (highest)".to_string() } else { format!("{}", i) };
                if ui.selectable_label(pane.hull_lod == i, label).clicked() && pane.hull_lod != i {
                    pane.hull_lod = i;
                    result.new_lod = Some(i);
                }
            }
        });
    }

    ui.separator();

    ui.set_min_width(220.0);
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.6).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for (group, names) in &armor.hull_part_groups {
            let group_all_on = names.iter().all(|n| pane.hull_visibility.get(n).copied().unwrap_or(false));
            let group_any_on = names.iter().any(|n| pane.hull_visibility.get(n).copied().unwrap_or(false));

            let id = ui.make_persistent_id(("hull_group", group));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
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
                        result.zone_changed = true;
                    }
                    let lbl = ui.label(group);
                    if gcb.hovered() || lbl.hovered() {
                        hovered.set(Some(SidebarHighlightKey::HullMeshes(names.clone())));
                    }
                })
                .body(|ui| {
                    for name in names {
                        let mut visible = pane.hull_visibility.get(name).copied().unwrap_or(false);
                        let resp = ui.checkbox(&mut visible, name.as_str());
                        if resp.changed() {
                            pane.hull_visibility.insert(name.clone(), visible);
                            result.zone_changed = true;
                        }
                        if resp.hovered() {
                            hovered.set(Some(SidebarHighlightKey::HullMeshes(vec![name.clone()])));
                        }
                    }
                });
        }
    });

    result.hovered_key = hovered.into_inner();
    result
}

/// Add a line segment as two cross-shaped quads into the vertex/index buffers.
#[allow(clippy::too_many_arguments)]
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
    use crate::viewport_3d::camera::add;
    use crate::viewport_3d::camera::scale;
    use crate::viewport_3d::camera::sub;
    let offset1 = scale(perp1, line_width * 0.5);
    let offset2 = scale(perp2, line_width * 0.5);

    for offset in [offset1, offset2] {
        let b = vertices.len() as u32;
        vertices.push(Vertex { position: sub(p0, offset), normal: perp1, color, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: add(p0, offset), normal: perp1, color, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: add(p1, offset), normal: perp1, color, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: sub(p1, offset), normal: perp1, color, uv: [0.0, 0.0] });
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
}

/// Add a diamond-shaped marker at a point into the vertex/index buffers.
#[allow(clippy::too_many_arguments)]
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
    use crate::viewport_3d::camera::add;
    use crate::viewport_3d::camera::scale;
    use crate::viewport_3d::camera::sub;
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
        vertices.push(Vertex { position: a, normal: n, color, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: b, normal: n, color, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: c, normal: n, color, uv: [0.0, 0.0] });
    }

    for i in 0..24 {
        indices.push(base + i);
    }
}

/// Recompute a trajectory's arc, impact data, detonation points, and 3D mesh for a new range.
#[allow(clippy::too_many_arguments)]
fn recompute_trajectory_for_range(
    traj: &mut crate::armor_viewer::state::StoredTrajectory,
    comparison_ships: &[crate::armor_viewer::penetration::ComparisonShip],
    viewport: &mut crate::viewport_3d::Viewport3D,
    loaded_armor: Option<&crate::armor_viewer::state::LoadedShipArmor>,
    device: &wgpu::Device,
    cam_distance: f32,
    marker_opacity: f32,
    comparison_ships_version: u64,
) {
    use crate::viewport_3d::camera::normalize;

    let range_meters = traj.meta.range.to_meters();
    let result = &mut traj.result;

    // Derive horizontal approach direction from the existing shell direction
    let dir = result.direction;
    let horiz_len = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
    let approach_xz = if horiz_len > 1e-6 { [dir[0] / horiz_len, 0.0, dir[2] / horiz_len] } else { [0.0, 0.0, -1.0] };

    // Get first AP shell for shell direction (shared ray direction)
    let first_shell = comparison_ships
        .iter()
        .flat_map(|s| s.shells.iter())
        .find(|s| s.ammo_type == AmmoType::AP)
        .or_else(|| comparison_ships.iter().flat_map(|s| s.shells.iter()).next());

    // Update shell direction from first shell's impact angle
    if let Some(shell) = first_shell
        && range_meters.value() > 0.0
    {
        let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
        if let Some(impact) = crate::armor_viewer::ballistics::solve_for_range(&params, range_meters) {
            let horiz_angle = impact.impact_angle_horizontal as f32;
            let cos_h = horiz_angle.cos();
            let sin_h = horiz_angle.sin();
            result.direction = normalize([approach_xz[0] * cos_h, -sin_h, approach_xz[2] * cos_h]);
        }
    }

    // Recompute per-ship arcs
    let model_extent = loaded_armor.map(|a| a.max_extent_xz()).unwrap_or(10.0);
    let arc_horiz_extent = model_extent * 2.0;
    let first_hit_pos = result.hits.first().map(|h| h.position).unwrap_or(result.origin);

    let mut new_ship_arcs: Vec<crate::armor_viewer::penetration::ShipArc> = Vec::new();
    if range_meters.value() > 0.0 {
        for (ship_idx, ship) in comparison_ships.iter().enumerate() {
            let shell = ship.shells.iter().find(|s| s.ammo_type == AmmoType::AP).or_else(|| ship.shells.first());
            if let Some(shell) = shell {
                let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
                if let Some(impact) = crate::armor_viewer::ballistics::solve_for_range(&params, range_meters) {
                    let (arc_2d, height_ratio) =
                        crate::armor_viewer::ballistics::simulate_arc_points(&params, impact.launch_angle, 60);
                    let arc_height_extent = arc_horiz_extent * (height_ratio as f32).max(0.02);
                    let arc_points_3d: Vec<[f32; 3]> = arc_2d
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
                        .collect();
                    new_ship_arcs.push(crate::armor_viewer::penetration::ShipArc {
                        ship_index: ship_idx,
                        arc_points_3d,
                        ballistic_impact: Some(impact),
                    });
                }
            }
        }
    }
    result.ship_arcs = new_ship_arcs;

    // Recompute detonation points and last visible hit
    // Each shell gets its own ballistic solve (different velocity/angle at range)
    let mut new_detonation_points: Vec<crate::armor_viewer::penetration::DetonationMarker> = Vec::new();
    let mut new_last_visible: Option<usize> = None;
    for (ship_idx, ship) in comparison_ships.iter().enumerate() {
        for shell in ship.shells.iter().filter(|s| s.ammo_type == AmmoType::AP) {
            let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
            let shell_impact = if range_meters.value() > 0.0 {
                crate::armor_viewer::ballistics::solve_for_range(&params, range_meters)
            } else {
                None
            };
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
        1.0,
    ));
    traj.marker_cam_dist = cam_distance;

    // Update the shell sim cache for the analysis panel.
    update_shell_sim_cache(traj, comparison_ships, comparison_ships_version);
}

/// Recompute a trajectory after the ship's roll changes.
///
/// Rotates the stored ray into the new model space, re-ray-casts against the armor,
/// rebuilds hits with updated impact angles, then runs the full arc/detonation/cache
/// recomputation.
#[allow(clippy::too_many_arguments)]
fn recompute_trajectory_for_roll(
    traj: &mut crate::armor_viewer::state::StoredTrajectory,
    new_roll: f32,
    comparison_ships: &[crate::armor_viewer::penetration::ComparisonShip],
    viewport: &mut crate::viewport_3d::Viewport3D,
    mesh_triangle_info: &[(crate::viewport_3d::MeshId, Vec<crate::armor_viewer::state::ArmorTriangleTooltip>)],
    loaded_armor: Option<&crate::armor_viewer::state::LoadedShipArmor>,
    device: &wgpu::Device,
    cam_distance: f32,
    marker_opacity: f32,
    comparison_ships_version: u64,
) {
    use crate::viewport_3d::camera::normalize;
    use crate::viewport_3d::camera::scale;
    use crate::viewport_3d::camera::sub;

    let old_roll = traj.created_at_roll;
    let delta = old_roll - new_roll;

    // Rotate origin and direction around Z by delta to transform from old model space to new.
    let rotate_z = |v: [f32; 3], angle: f32| -> [f32; 3] {
        let (s, c) = angle.sin_cos();
        [v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]]
    };

    let rotated_origin = rotate_z(traj.result.origin, delta);
    let rotated_dir = normalize(rotate_z(traj.result.direction, delta));

    // Re-ray-cast against armor meshes
    let ray_origin = sub(rotated_origin, scale(rotated_dir, 50.0));
    let all_hits = viewport.pick_all_ray(ray_origin, rotated_dir);

    // Find the hit closest to the rotated origin (same logic as initial trajectory creation)
    let start_idx = all_hits
        .iter()
        .enumerate()
        .min_by(|(_, (a, _)), (_, (b, _))| {
            let da = (a.world_position[0] - rotated_origin[0]).powi(2)
                + (a.world_position[1] - rotated_origin[1]).powi(2)
                + (a.world_position[2] - rotated_origin[2]).powi(2);
            let db = (b.world_position[0] - rotated_origin[0]).powi(2)
                + (b.world_position[1] - rotated_origin[1]).powi(2)
                + (b.world_position[2] - rotated_origin[2]).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0);
    let relevant_hits = &all_hits[start_idx..];

    // Rebuild trajectory hits
    let mut traj_hits = Vec::new();
    let first_dist = relevant_hits.first().map(|h| h.0.distance).unwrap_or(0.0);
    for (hit, normal) in relevant_hits {
        let tooltip = mesh_triangle_info
            .iter()
            .find(|(id, _)| *id == hit.mesh_id)
            .and_then(|(_, infos)| infos.get(hit.triangle_index));

        if let Some(info) = tooltip {
            let angle = crate::armor_viewer::penetration::impact_angle_deg(&rotated_dir, normal);
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

    let total_armor: f32 = traj_hits.iter().map(|h| h.thickness_mm).sum();

    // Update stored result with new hits and direction
    traj.result.origin = rotated_origin;
    traj.result.direction = rotated_dir;
    traj.result.hits = traj_hits;
    traj.result.total_armor_mm = total_armor;
    traj.created_at_roll = new_roll;

    // Recompute arcs, detonation points, visualization mesh, and shell sim cache
    // (reuses the existing range-based recomputation which handles all of these)
    recompute_trajectory_for_range(
        traj,
        comparison_ships,
        viewport,
        loaded_armor,
        device,
        cam_distance,
        marker_opacity,
        comparison_ships_version,
    );
}

/// Recompute and cache per-shell simulation results for the analysis panel.
///
/// This is called when a trajectory is created, its range changes, or comparison ships
/// change. The analysis panel reads from this cache instead of recomputing every frame.
fn update_shell_sim_cache(
    traj: &mut crate::armor_viewer::state::StoredTrajectory,
    comparison_ships: &[crate::armor_viewer::penetration::ComparisonShip],
    comparison_ships_version: u64,
) {
    use wowsunpack::game_params::types::AmmoType;

    let range_meters = traj.meta.range.to_meters();
    let hits = &traj.result.hits;
    let direction = &traj.result.direction;
    let sims: Vec<crate::armor_viewer::state::CachedShellSim> = comparison_ships
        .iter()
        .enumerate()
        .flat_map(|(si, ship)| {
            ship.shells.iter().map(move |shell| {
                let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
                let impact = if shell.ammo_type == AmmoType::AP && range_meters.value() > 0.0 {
                    crate::armor_viewer::ballistics::solve_for_range(&params, range_meters)
                } else {
                    None
                };
                let sim = if shell.ammo_type == AmmoType::AP {
                    impact.as_ref().map(|imp| {
                        crate::armor_viewer::penetration::simulate_shell_through_hits(&params, imp, hits, direction)
                    })
                } else {
                    None
                };
                crate::armor_viewer::state::CachedShellSim {
                    ship_name: ship.display_name.clone(),
                    ship_index: si,
                    shell: shell.clone(),
                    sim,
                }
            })
        })
        .collect();

    let last_visible_hit: Option<usize> = sims
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

    traj.shell_sim_cache = Some(crate::armor_viewer::state::ShellSimCache {
        sims,
        last_visible_hit,
        range_km: traj.meta.range,
        comparison_ships_version,
    });
}

/// Compute perpendicular vectors for a line segment direction, for cross-shaped quad rendering.
fn segment_perps(seg_dir: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    use crate::viewport_3d::camera::cross;
    use crate::viewport_3d::camera::normalize;
    let arbitrary = if seg_dir[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let p1 = normalize(cross(seg_dir, arbitrary));
    let p2 = normalize(cross(seg_dir, p1));
    (p1, p2)
}

/// Upload a trajectory visualization as colored line segments on the overlay layer.
/// If arc_points_3d is non-empty, draws a curved arc from firing position to first hit,
/// then straight segments through subsequent armor plates.
#[allow(clippy::too_many_arguments)]
pub(crate) fn upload_trajectory_visualization(
    viewport: &mut crate::viewport_3d::Viewport3D,
    result: &crate::armor_viewer::penetration::TrajectoryResult,
    device: &wgpu::Device,
    traj_color: [f32; 4],
    last_visible_hit: Option<usize>,
    cam_distance: f32,
    marker_opacity: f32,
    line_width_mult: f32,
) -> MeshId {
    use crate::viewport_3d::camera::add;
    use crate::viewport_3d::camera::cross;
    use crate::viewport_3d::camera::normalize;
    use crate::viewport_3d::camera::scale;
    use crate::viewport_3d::camera::sub;

    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let dir = result.direction;
    // Scale markers and line width with camera distance so they shrink when zoomed in
    let scale_factor = (cam_distance / 200.0).clamp(0.15, 3.0);
    let line_width = TRAJECTORY_LINE_WIDTH_FACTOR * scale_factor * line_width_mult;

    let arbitrary = if dir[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let perp1 = normalize(cross(dir, arbitrary));
    let perp2 = normalize(cross(dir, perp1));

    if !result.hits.is_empty() {
        let first_pos = result.hits[0].position;

        let has_any_arc = result.ship_arcs.iter().any(|a| a.arc_points_3d.len() >= 2);
        if has_any_arc {
            // Draw per-ship ballistic arcs in ship colors
            for arc_data in &result.ship_arcs {
                if arc_data.arc_points_3d.len() < 2 {
                    continue;
                }
                let sc = SHIP_COLORS[arc_data.ship_index % SHIP_COLORS.len()];
                let arc = &arc_data.arc_points_3d;
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
                    let frac = i as f32 / (arc.len() - 1) as f32;
                    let alpha = 0.7 + 0.3 * frac;
                    traj_line_segment(
                        &mut vertices,
                        &mut indices,
                        arc[i],
                        arc[i + 1],
                        [sc[0], sc[1], sc[2], alpha],
                        sp1,
                        sp2,
                        line_width,
                    );
                }
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
            let rgb = if hit.angle_deg < SHALLOW_ANGLE_DEG {
                IMPACT_COLOR_SHALLOW
            } else if hit.angle_deg < STEEP_ANGLE_DEG {
                IMPACT_COLOR_MEDIUM
            } else {
                IMPACT_COLOR_STEEP
            };
            let color = [rgb[0], rgb[1], rgb[2], marker_opacity];

            traj_marker(
                &mut vertices,
                &mut indices,
                hit.position,
                color,
                MARKER_SIZE_FACTOR * scale_factor,
                dir,
                perp1,
                perp2,
            );

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
            let burst_size = DETONATION_BURST_SIZE_FACTOR * scale_factor;
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
            vertices.push(Vertex { position: det_pos, normal: [0.0, 1.0, 0.0], color: burst_color, uv: [0.0, 0.0] });

            for offset in &offsets {
                vertices.push(Vertex {
                    position: add(det_pos, *offset),
                    normal: [0.0, 1.0, 0.0],
                    color: burst_color,
                    uv: [0.0, 0.0],
                });
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

    let id = viewport.add_overlay_mesh(device, &vertices, &indices);
    // Trajectories are analysis overlays — they should not rotate with model_roll.
    viewport.set_world_space(id, true);
    id
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
pub(crate) fn build_hull_part_groups(
    hull_meshes: &[wowsunpack::export::gltf_export::InteractiveHullMesh],
) -> Vec<(String, Vec<String>)> {
    use std::collections::BTreeSet;
    use std::collections::HashMap;

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
pub(crate) fn create_water_plane(y: f32, bounds: ([f32; 3], [f32; 3]), opacity: f32) -> (Vec<Vertex>, Vec<u32>) {
    let cx = (bounds.0[0] + bounds.1[0]) * 0.5;
    let cz = (bounds.0[2] + bounds.1[2]) * 0.5;
    let ex = (bounds.1[0] - bounds.0[0]) * 2.25;
    let ez = (bounds.1[2] - bounds.0[2]) * 2.25;

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

/// Upload plate boundary edge outlines where adjacent triangles have different thickness values.
/// These appear as thin black lines along edges where two plates of different thickness meet.
pub(crate) fn upload_plate_boundary_edges(pane: &mut ArmorPane, armor: &LoadedShipArmor, device: &wgpu::Device) {
    use std::collections::HashMap;

    let edge_half_width: f32 = PLATE_EDGE_HALF_WIDTH;
    let normal_offset: f32 = PLATE_EDGE_NORMAL_OFFSET;
    let edge_color: [f32; 4] = EDGE_COLOR;

    // Quantize a float position to an integer key to avoid floating-point comparison issues.
    fn quantize(v: [f32; 3]) -> [i32; 3] {
        [(v[0] * 10000.0).round() as i32, (v[1] * 10000.0).round() as i32, (v[2] * 10000.0).round() as i32]
    }

    // Canonical edge key: sorted pair of quantized positions.
    type EdgeKey = ([i32; 3], [i32; 3]);
    fn make_edge_key(a: [i32; 3], b: [i32; 3]) -> EdgeKey {
        if a < b { (a, b) } else { (b, a) }
    }

    // For each edge, store the plate identity and face normal from each side.
    struct EdgeInfo {
        plate_key: PlateKey,
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
                    plate_key: plate_key.clone(),
                    normal: face_normal,
                    p0: tri_pos[a],
                    p1: tri_pos[b],
                });
            }
        }
    }

    // Find boundary edges: mesh silhouettes or edges between different plates.
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for infos in edge_map.values() {
        // Draw edges that form plate outlines:
        //  - mesh boundary edges (only 1 triangle) — silhouette of each armor piece
        //  - edges between different plates (zone, material, or thickness)
        //  - crease edges where adjacent face normals differ sharply (e.g. box corners)
        let is_mesh_boundary = infos.len() == 1;
        let first_plate = &infos[0].plate_key;
        let is_plate_boundary = infos.len() >= 2 && infos.iter().any(|i| i.plate_key != *first_plate);
        let is_crease = infos.len() >= 2 && {
            let n0 = &infos[0].normal;
            infos[1..].iter().any(|i| {
                let dot = n0[0] * i.normal[0] + n0[1] * i.normal[1] + n0[2] * i.normal[2];
                dot < 0.7 // ~45 degrees
            })
        };
        if !is_mesh_boundary && !is_plate_boundary && !is_crease {
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

        // Build edge quads in two perpendicular orientations so edges are
        // visible from any viewing angle:
        //  1. Expand along the face normal (visible when surface is edge-on)
        //  2. Expand along the in-plane tangent (visible when looking at the surface)
        let tx = edge_dir[1] * avg_normal[2] - edge_dir[2] * avg_normal[1];
        let ty = edge_dir[2] * avg_normal[0] - edge_dir[0] * avg_normal[2];
        let tz = edge_dir[0] * avg_normal[1] - edge_dir[1] * avg_normal[0];
        let t_len = (tx * tx + ty * ty + tz * tz).sqrt();
        if t_len < 1e-10 {
            continue;
        }
        let tangent = [tx / t_len, ty / t_len, tz / t_len];

        // Emit edge quads on both sides of the surface (+/- normal offset).
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
                        color: edge_color,
                        uv: [0.0, 0.0],
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

    let edge_half_width: f32 = GAP_EDGE_HALF_WIDTH;
    let normal_offset: f32 = GAP_EDGE_NORMAL_OFFSET;
    let gap_color: [f32; 4] = GAP_COLOR;
    let max_edge_length: f32 = MAX_GAP_EDGE_LENGTH;

    fn quantize(v: [f32; 3]) -> [i32; 3] {
        [(v[0] * 10000.0).round() as i32, (v[1] * 10000.0).round() as i32, (v[2] * 10000.0).round() as i32]
    }

    type EdgeKey = ([i32; 3], [i32; 3]);
    fn make_edge_key(a: [i32; 3], b: [i32; 3]) -> EdgeKey {
        if a < b { (a, b) } else { (b, a) }
    }

    #[derive(Clone, Copy)]
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

    // Collect boundary edges (shared by exactly 1 triangle), filtering by length
    let mut boundary_edges: Vec<(EdgeKey, &EdgeData)> = Vec::new();
    for (ek, (count, data)) in &edge_count {
        if *count != 1 {
            continue;
        }
        let dx = data.p1[0] - data.p0[0];
        let dy = data.p1[1] - data.p0[1];
        let dz = data.p1[2] - data.p0[2];
        let edge_len = (dx * dx + dy * dy + dz * dz).sqrt();
        if edge_len > max_edge_length || edge_len < 1e-6 {
            continue;
        }
        boundary_edges.push((*ek, data));
    }

    // Filter out narrow gaps: for each boundary edge midpoint, find the minimum
    // distance to any other boundary edge. If closer than MIN_GAP_WIDTH, the gap
    // is too narrow for a shell to pass through.
    let min_gap_width: f32 = crate::armor_viewer::constants::MIN_GAP_WIDTH;

    // Point-to-segment squared distance
    fn point_to_segment_dist_sq(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f32 {
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ap = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
        let ab_dot = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
        if ab_dot < 1e-12 {
            return ap[0] * ap[0] + ap[1] * ap[1] + ap[2] * ap[2];
        }
        let t = ((ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / ab_dot).clamp(0.0, 1.0);
        let closest = [a[0] + t * ab[0], a[1] + t * ab[1], a[2] + t * ab[2]];
        let d = [p[0] - closest[0], p[1] - closest[1], p[2] - closest[2]];
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
    }

    let min_gap_sq = min_gap_width * min_gap_width;

    let wide_edges: Vec<&EdgeData> = boundary_edges
        .iter()
        .filter(|(ek_i, data_i)| {
            let mid = [
                (data_i.p0[0] + data_i.p1[0]) * 0.5,
                (data_i.p0[1] + data_i.p1[1]) * 0.5,
                (data_i.p0[2] + data_i.p1[2]) * 0.5,
            ];
            // Find minimum distance from this edge's midpoint to any other boundary edge
            let mut min_dist_sq = f32::MAX;
            for (ek_j, data_j) in &boundary_edges {
                if ek_i == ek_j {
                    continue;
                }
                let d = point_to_segment_dist_sq(mid, data_j.p0, data_j.p1);
                if d < min_dist_sq {
                    min_dist_sq = d;
                }
            }
            // Keep this edge only if the nearest other boundary edge is farther than MIN_GAP_WIDTH
            min_dist_sq > min_gap_sq
        })
        .map(|(_, data)| *data)
        .collect();

    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut gap_count = 0;

    for data in &wide_edges {
        let p0 = data.p0;
        let p1 = data.p1;

        gap_count += 1;

        let avg_normal = data.normal;
        let dx = p1[0] - p0[0];
        let dy = p1[1] - p0[1];
        let dz = p1[2] - p0[2];
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
                        uv: [0.0, 0.0],
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
pub(crate) fn transform_point(t: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        t[0] * p[0] + t[4] * p[1] + t[8] * p[2] + t[12],
        t[1] * p[0] + t[5] * p[1] + t[9] * p[2] + t[13],
        t[2] * p[0] + t[6] * p[1] + t[10] * p[2] + t[14],
    ]
}

/// Apply the upper-left 3x3 of a column-major 4x4 transform to a normal and renormalize.
pub(crate) fn transform_normal(t: &[f32; 16], n: [f32; 3]) -> [f32; 3] {
    let x = t[0] * n[0] + t[4] * n[1] + t[8] * n[2];
    let y = t[1] * n[0] + t[5] * n[1] + t[9] * n[2];
    let z = t[2] * n[0] + t[6] * n[1] + t[10] * n[2];
    let len = (x * x + y * y + z * z).sqrt();
    if len < 1e-10 {
        return [0.0, 0.0, 0.0];
    }
    [x / len, y / len, z / len]
}

/// Draw the Armor zone/material/plate visibility popover content.
/// Returns `(zone_changed, hovered_key)` where `hovered_key` identifies the hovered item
/// for sidebar highlight overlay.
pub(crate) fn draw_armor_visibility_popover(
    ui: &mut egui::Ui,
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    translate_part: &dyn Fn(&str) -> String,
) -> (bool, Option<SidebarHighlightKey>) {
    let mut zone_changed = false;
    let hovered: std::cell::Cell<Option<SidebarHighlightKey>> = std::cell::Cell::new(None);
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
    if !pane.plate_visibility.is_empty() && ui.small_button("Reset plates").clicked() {
        pane.undo_stack.push(VisibilitySnapshot {
            part_visibility: pane.part_visibility.clone(),
            plate_visibility: pane.plate_visibility.clone(),
        });
        pane.plate_visibility.clear();
        zone_changed = true;
    }
    ui.separator();

    // Three-level hierarchy: zone > material > plate
    ui.set_min_width(250.0);
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.6).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let show_zero = pane.show_zero_mm;
        for zone in &armor.zone_part_plates {
            let zone_all_on = zone.parts.iter().all(|p| {
                let part_on = pane.part_visibility.get(&(zone.name.clone(), p.name.clone())).copied().unwrap_or(true);
                if !part_on {
                    return false;
                }
                !p.plates.iter().filter(|&&t| show_zero || t != 0).any(|&t| {
                    let pk: PlateKey = (zone.name.clone(), p.name.clone(), t);
                    pane.plate_visibility.get(&pk).copied() == Some(false)
                })
            });
            let zone_any_on = zone
                .parts
                .iter()
                .any(|p| pane.part_visibility.get(&(zone.name.clone(), p.name.clone())).copied().unwrap_or(true));

            let zone_id = ui.make_persistent_id(("armor_zone", &zone.name));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), zone_id, false)
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
                            for z in &armor.zone_part_plates {
                                let on = z.name == zone.name;
                                for p in &z.parts {
                                    pane.part_visibility.insert((z.name.clone(), p.name.clone()), on);
                                }
                            }
                        } else {
                            for p in &zone.parts {
                                pane.part_visibility.insert((zone.name.clone(), p.name.clone()), checked);
                            }
                        }
                        zone_changed = true;
                    }
                    let lbl = ui.label(&zone.name).on_hover_text("Ctrl+click to solo");
                    if cb.hovered() || lbl.hovered() {
                        hovered.set(Some(SidebarHighlightKey::Zone(zone.name.clone())));
                    }
                })
                .body(|ui| {
                    for part in &zone.parts {
                        let part_key = (zone.name.clone(), part.name.clone());
                        let part_on = pane.part_visibility.get(&part_key).copied().unwrap_or(true);
                        let visible_plates: Vec<i32> =
                            part.plates.iter().copied().filter(|&t| show_zero || t != 0).collect();
                        let any_plate_hidden = visible_plates.iter().any(|&t| {
                            let pk: PlateKey = (zone.name.clone(), part.name.clone(), t);
                            pane.plate_visibility.get(&pk).copied() == Some(false)
                        });
                        let display = translate_part(&part.name);

                        if visible_plates.len() <= 1 {
                            let mut v = part_on && !any_plate_hidden;
                            let resp = ui.checkbox(&mut v, &display);
                            if resp.changed() {
                                pane.undo_stack.push(VisibilitySnapshot {
                                    part_visibility: pane.part_visibility.clone(),
                                    plate_visibility: pane.plate_visibility.clone(),
                                });
                                pane.part_visibility.insert(part_key.clone(), v);
                                for &t in &visible_plates {
                                    pane.plate_visibility.remove(&(zone.name.clone(), part.name.clone(), t));
                                }
                                zone_changed = true;
                            }
                            if resp.hovered() {
                                hovered.set(Some(SidebarHighlightKey::Part(zone.name.clone(), part.name.clone())));
                            }
                        } else {
                            let mat_id = ui.make_persistent_id(("armor_mat", &zone.name, &part.name));
                            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), mat_id, false)
                                .show_header(ui, |ui| {
                                    let mut checked = part_on && !any_plate_hidden;
                                    let cb = ui.checkbox(&mut checked, "");
                                    if part_on && any_plate_hidden {
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
                                        pane.part_visibility.insert(part_key.clone(), checked);
                                        for &t in &visible_plates {
                                            pane.plate_visibility.remove(&(zone.name.clone(), part.name.clone(), t));
                                        }
                                        zone_changed = true;
                                    }
                                    let lbl = ui.label(&display);
                                    if cb.hovered() || lbl.hovered() {
                                        hovered
                                            .set(Some(SidebarHighlightKey::Part(zone.name.clone(), part.name.clone())));
                                    }
                                })
                                .body(|ui| {
                                    for &thickness_i32 in &visible_plates {
                                        let pk: PlateKey = (zone.name.clone(), part.name.clone(), thickness_i32);
                                        let plate_on = pane.plate_visibility.get(&pk).copied().unwrap_or(true);
                                        let thickness_mm = thickness_i32 as f32 / 10.0;
                                        let row = ui.horizontal(|ui| {
                                            let color =
                                                wowsunpack::export::gltf_export::thickness_to_color(thickness_mm);
                                            paint_swatch(ui, color32_from_f32(color), 10.0);
                                            let mut v = part_on && plate_on;
                                            let resp = ui.checkbox(&mut v, format!("{:.0} mm", thickness_mm));
                                            if resp.changed() {
                                                pane.undo_stack.push(VisibilitySnapshot {
                                                    part_visibility: pane.part_visibility.clone(),
                                                    plate_visibility: pane.plate_visibility.clone(),
                                                });
                                                pane.plate_visibility.insert(pk, !plate_on);
                                                zone_changed = true;
                                            }
                                            resp.hovered()
                                        });
                                        if row.inner {
                                            hovered.set(Some(SidebarHighlightKey::Plate((
                                                zone.name.clone(),
                                                part.name.clone(),
                                                thickness_i32,
                                            ))));
                                        }
                                    }
                                });
                        }
                    }
                });
        }
    });
    (zone_changed, hovered.into_inner())
}

/// Draw the splash box visibility popover content (groups + individual boxes with hover detection).
/// Returns (zone_changed, hovered_key).
pub(crate) fn draw_splash_box_visibility_popover(
    ui: &mut egui::Ui,
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
) -> (bool, Option<SidebarHighlightKey>) {
    let mut zone_changed = false;
    let hovered: std::cell::Cell<Option<SidebarHighlightKey>> = std::cell::Cell::new(None);

    let all_names: Vec<&String> = armor.splash_box_groups.iter().flat_map(|(_, names)| names).collect();

    ui.horizontal(|ui| {
        if ui.small_button("All").clicked() {
            pane.show_splash_boxes = true;
            for name in &all_names {
                pane.splash_box_visibility.insert((*name).clone(), true);
            }
            zone_changed = true;
        }
        if ui.small_button("None").clicked() {
            pane.show_splash_boxes = false;
            for name in &all_names {
                pane.splash_box_visibility.insert((*name).clone(), false);
            }
            zone_changed = true;
        }
    });
    ui.separator();

    ui.set_min_width(220.0);
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.6).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for (group, names) in &armor.splash_box_groups {
            let group_all_on = names.iter().all(|n| pane.splash_box_visibility.get(n).copied().unwrap_or(false));
            let group_any_on = names.iter().any(|n| pane.splash_box_visibility.get(n).copied().unwrap_or(false));

            let id = ui.make_persistent_id(("splash_box_group", group));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
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
                            pane.splash_box_visibility.insert(name.clone(), group_checked);
                        }
                        // Sync show_splash_boxes from any-visible state
                        pane.show_splash_boxes =
                            all_names.iter().any(|n| pane.splash_box_visibility.get(*n).copied().unwrap_or(false));
                        zone_changed = true;
                    }
                    let lbl = ui.label(format!("{} ({})", group, names.len()));
                    if gcb.hovered() || lbl.hovered() {
                        hovered.set(Some(SidebarHighlightKey::SplashBoxes(names.clone())));
                    }
                })
                .body(|ui| {
                    for name in names {
                        let mut visible = pane.splash_box_visibility.get(name).copied().unwrap_or(false);
                        let display = crate::armor_viewer::splash::prettify_box_name(name);
                        let resp = ui.checkbox(&mut visible, display);
                        if resp.changed() {
                            pane.splash_box_visibility.insert(name.clone(), visible);
                            // Sync show_splash_boxes from any-visible state
                            pane.show_splash_boxes =
                                all_names.iter().any(|n| pane.splash_box_visibility.get(*n).copied().unwrap_or(false));
                            zone_changed = true;
                        }
                        if resp.hovered() {
                            hovered.set(Some(SidebarHighlightKey::SplashBoxes(vec![name.clone()])));
                        }
                    }
                });
        }
    });

    (zone_changed, hovered.into_inner())
}

/// Upload a highlight overlay for specific splash boxes by name.
pub(crate) fn upload_splash_box_highlight(
    pane: &mut ArmorPane,
    armor: &LoadedShipArmor,
    names: &[String],
    device: &wgpu::Device,
) -> MeshId {
    let boxes: Vec<_> = armor
        .splash_data
        .as_ref()
        .map(|sd| sd.boxes.iter().filter(|b| names.contains(&b.name)).collect())
        .unwrap_or_default();

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let color = [1.0f32, 1.0, 1.0, 0.5];
    let w = 0.005_f32; // slightly thicker than normal wireframe for highlight

    for sbox in &boxes {
        let lo = sbox.min;
        let hi = sbox.max;
        let corners: [[f32; 3]; 8] = [
            [lo[0], lo[1], lo[2]],
            [hi[0], lo[1], lo[2]],
            [hi[0], hi[1], lo[2]],
            [lo[0], hi[1], lo[2]],
            [lo[0], lo[1], hi[2]],
            [hi[0], lo[1], hi[2]],
            [hi[0], hi[1], hi[2]],
            [lo[0], hi[1], hi[2]],
        ];
        let edges: [(usize, usize); 12] =
            [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
        for &(a, b) in &edges {
            let pa = corners[a];
            let pb = corners[b];
            let dx = pb[0] - pa[0];
            let dy = pb[1] - pa[1];
            let dz = pb[2] - pa[2];
            let (px, py, pz) = {
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
            let n = [0.0, 1.0, 0.0];
            let base = vertices.len() as u32;
            for &(x, y, z) in &[
                (pa[0] - px, pa[1] - py, pa[2] - pz),
                (pa[0] + px, pa[1] + py, pa[2] + pz),
                (pb[0] - px, pb[1] - py, pb[2] - pz),
                (pb[0] + px, pb[1] + py, pb[2] + pz),
            ] {
                vertices.push(Vertex { position: [x, y, z], normal: n, color, uv: [0.0, 0.0] });
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }

    pane.viewport.add_overlay_mesh(device, &vertices, &indices)
}

/// Upload (or clear) splash box wireframe overlays based on `pane.show_splash_boxes`.
/// Called after viewport.clear() to rebuild overlay meshes.
///
/// If `armor_override` is provided, splash data is read from it instead of
/// `pane.loaded_armor`. This is needed during initial load when `loaded_armor`
/// has not been set on the pane yet.
pub(crate) fn upload_splash_box_wireframes(
    pane: &mut ArmorPane,
    device: &wgpu::Device,
    armor_override: Option<&LoadedShipArmor>,
) {
    // Remove old meshes
    for mid in pane.splash_box_mesh_ids.drain(..) {
        pane.viewport.remove_mesh(mid);
    }
    pane.splash_box_labels.clear();

    if !pane.show_splash_boxes {
        return;
    }

    let splash_data = armor_override
        .and_then(|a| a.splash_data.as_ref())
        .or_else(|| pane.loaded_armor.as_ref().and_then(|a| a.splash_data.as_ref()));

    let visible_boxes: Vec<_> = splash_data
        .map(|sd| {
            sd.boxes.iter().filter(|b| pane.splash_box_visibility.get(&b.name).copied().unwrap_or(false)).collect()
        })
        .unwrap_or_default();

    if !visible_boxes.is_empty() {
        let (verts, indices, labels) = crate::armor_viewer::splash::build_splash_box_wireframes(&visible_boxes);
        if !verts.is_empty() {
            let mid = pane.viewport.add_overlay_mesh(device, &verts, &indices);
            pane.splash_box_mesh_ids.push(mid);
        }
        pane.splash_box_labels = labels;
    }
}

/// Draw splash box name labels projected onto the viewport.
pub(crate) fn draw_splash_box_labels(pane: &ArmorPane, painter: &egui::Painter, viewport_rect: egui::Rect) {
    if !pane.show_splash_boxes || pane.splash_box_labels.is_empty() {
        return;
    }

    let font = egui::FontId::proportional(11.0);
    let text_color = egui::Color32::from_rgba_unmultiplied(200, 220, 255, 220);
    let bg_color = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 140);

    for label in &pane.splash_box_labels {
        if let Some(screen_pos) = pane.viewport.camera.project_to_screen(label.position, viewport_rect) {
            // Only draw if within the viewport
            if viewport_rect.contains(screen_pos) {
                let galley = painter.layout_no_wrap(label.name.clone(), font.clone(), text_color);
                let text_rect = egui::Rect::from_min_size(
                    egui::pos2(screen_pos.x - galley.size().x * 0.5, screen_pos.y - galley.size().y - 2.0),
                    galley.size(),
                );
                let bg_rect = text_rect.expand(2.0);
                painter.rect_filled(bg_rect, 2.0, bg_color);
                painter.galley(text_rect.min, galley, text_color);
            }
        }
    }
}

/// Draw a disclaimer watermark in the bottom-left of the viewport.
pub(crate) fn draw_viewport_watermark(painter: &egui::Painter, viewport_rect: egui::Rect) {
    let font = egui::FontId::proportional(11.0);
    let text_color = egui::Color32::from_rgba_unmultiplied(180, 180, 180, 120);
    let pos = egui::pos2(viewport_rect.left() + 6.0, viewport_rect.bottom() - 18.0);
    painter.text(
        pos,
        egui::Align2::LEFT_BOTTOM,
        "Ballistic results are based on reverse engineering/estimates of how simulation works",
        font,
        text_color,
    );
}

/// Draw the ship roll slider. Returns true if the roll value changed.
pub(crate) fn draw_roll_slider(ui: &mut egui::Ui, viewport: &mut crate::viewport_3d::Viewport3D) -> bool {
    let prev = viewport.model_roll;
    let roll_deg = &mut viewport.model_roll;
    // Store in degrees for the slider, convert on render
    let mut deg = roll_deg.to_degrees();
    let response = ui.add(egui::Slider::new(&mut deg, -25.0..=25.0).fixed_decimals(1).suffix("°").text("Roll"));
    *roll_deg = deg.to_radians();
    if response.double_clicked() {
        *roll_deg = 0.0;
    }
    let changed = (*roll_deg - prev).abs() > 1e-6;
    if changed {
        viewport.mark_dirty();
    }
    changed
}

/// Draw the Display settings popover content (plate edges, waterline, 0mm, opacity).
/// Returns true if any visibility changed.
pub(crate) fn draw_display_settings_popover(ui: &mut egui::Ui, pane: &mut ArmorPane, armor: &LoadedShipArmor) -> bool {
    let mut zone_changed = false;
    ui.set_min_width(160.0);
    if ui.checkbox(&mut pane.show_plate_edges, "Plate Edges").changed() {
        zone_changed = true;
    }
    if armor.dock_y_offset.is_some() {
        if ui.checkbox(&mut pane.show_waterline, "Waterline").changed() {
            zone_changed = true;
        }
        if pane.show_waterline {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label("Opacity");
                if ui.add(egui::Slider::new(&mut pane.waterline_opacity, 0.05..=1.0).fixed_decimals(2)).changed() {
                    zone_changed = true;
                }
            });
        }
    }
    if ui.checkbox(&mut pane.show_zero_mm, "0 mm Plates").changed() {
        zone_changed = true;
    }
    if !armor.hull_meshes.is_empty() {
        let any_hull_on = pane.hull_visibility.values().any(|&v| v);
        let mut hull_checked = any_hull_on;
        if ui.checkbox(&mut hull_checked, "Ship Hull").changed() {
            for (_, vis) in pane.hull_visibility.iter_mut() {
                *vis = hull_checked;
            }
            zone_changed = true;
        }
        if any_hull_on {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                if ui.checkbox(&mut pane.hull_opaque, "Opaque Hull").changed() {
                    zone_changed = true;
                }
            });
        }
    }
    ui.horizontal(|ui| {
        ui.label("Armor Opacity");
        if ui.add(egui::Slider::new(&mut pane.armor_opacity, 0.1..=1.0).fixed_decimals(2)).changed() {
            zone_changed = true;
        }
    });
    zone_changed
}

/// Handle viewport hover (tooltip + highlight), click-to-hide, and right-click context menu.
/// Returns true if visibility changed (zone_changed).
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_plate_interaction(
    ui: &egui::Ui,
    response: &egui::Response,
    pane: &mut ArmorPane,
    device: &wgpu::Device,
    translate_part: &dyn Fn(&str) -> String,
    allow_click_toggle: bool,
    comparison_ships: &[crate::armor_viewer::penetration::ComparisonShip],
    ifhe_enabled: bool,
) -> bool {
    let mut zone_changed = false;

    // Picking on hover
    let mut hovered_plate_key: Option<PlateKey> = None;
    let context_menu_open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(response));
    if response.hovered()
        && let Some(hover_pos) = response.hover_pos()
    {
        if let Some(hit) = pane.viewport.pick(hover_pos, response.rect) {
            let tooltip = pane
                .mesh_triangle_info
                .iter()
                .find(|(id, _)| *id == hit.mesh_id)
                .and_then(|(_, infos)| infos.get(hit.triangle_index));

            if let Some(tooltip) = tooltip {
                let thickness_key = (tooltip.thickness_mm * 10.0).round() as i32;
                hovered_plate_key = Some((tooltip.zone.clone(), tooltip.material_name.clone(), thickness_key));
                pane.hovered_info = Some(tooltip.clone());
                if !context_menu_open {
                    egui::containers::Tooltip::for_widget(response).at_pointer().show(|ui| {
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

    // Click to toggle plate visibility
    if allow_click_toggle
        && response.clicked()
        && let Some(ref key) = hovered_plate_key
    {
        pane.undo_stack.push(VisibilitySnapshot {
            part_visibility: pane.part_visibility.clone(),
            plate_visibility: pane.plate_visibility.clone(),
        });
        let currently_visible = pane.plate_visibility.get(key).copied().unwrap_or(true);
        pane.plate_visibility.insert(key.clone(), !currently_visible);
        zone_changed = true;
    }

    // Right-click context menu
    if response.secondary_clicked() {
        pane.context_menu_key = hovered_plate_key.clone();
    } else if !context_menu_open {
        pane.context_menu_key = None;
    }

    if let Some(ref ctx_key) = pane.context_menu_key.clone() {
        let ctx_key = ctx_key.clone();
        let part_key = (ctx_key.0.clone(), ctx_key.1.clone());
        let thickness = ctx_key.2 as f32 / 10.0;
        let ctx_name = translate_part(&ctx_key.1);
        response.context_menu(|ui| {
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

            let hidden_count = pane.plate_visibility.values().filter(|&&v| !v).count();
            if hidden_count > 0 && ui.button(format!("Show all hidden plates ({})", hidden_count)).clicked() {
                pane.undo_stack.push(VisibilitySnapshot {
                    part_visibility: pane.part_visibility.clone(),
                    plate_visibility: pane.plate_visibility.clone(),
                });
                pane.plate_visibility.clear();
                zone_changed = true;
                ui.close();
            }

            ui.separator();

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
        if let Some((_, old_id)) = pane.hover_highlight.take() {
            pane.viewport.remove_mesh(old_id);
        }
        if let Some(ref key) = hovered_plate_key
            && let Some(armor) = pane.loaded_armor.take()
        {
            let mesh_id = upload_plate_highlight(pane, &armor, key, device, [1.0, 1.0, 1.0, 0.35]);
            pane.hover_highlight = Some((key.clone(), mesh_id));
            pane.loaded_armor = Some(armor);
        }
    }

    zone_changed
}
