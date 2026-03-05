use std::cell::Cell;
use std::cell::RefCell;

use egui_dock::DockArea;
use egui_dock::DockState;
use egui_dock::TabViewer;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::ShellInfo;

use crate::armor_viewer::constants::*;
use crate::armor_viewer::penetration::ComparisonShip;
use crate::armor_viewer::state::AnalysisTab;
use crate::armor_viewer::state::ArmorPane;
use crate::armor_viewer::state::ArmorViewerState;
use crate::armor_viewer::state::StoredTrajectory;
use crate::data::wows_data::SharedWoWsData;
use crate::icons;
/// Actions collected from the trajectory tab that must be applied to the active
/// pane after the window has been drawn (because the window borrows state immutably).
#[derive(Default)]
pub struct TrajectoryActions {
    pub clear_all: bool,
    pub delete_id: Option<u64>,
    pub new_range: Option<Km>,
    pub per_arc_range_changes: Vec<(usize, Km)>,
    pub arc_plate_toggles: Vec<(usize, bool)>,
    pub arc_zone_toggles: Vec<(usize, bool)>,
    pub show_all_hit_plates: bool,
    pub show_all_hit_zones: bool,
}
/// Programmatically focus a specific analysis tab in the dock.
pub fn focus_analysis_tab(dock_state: &mut DockState<AnalysisTab>, tab: AnalysisTab) {
    if let Some((surface, node, tab_idx)) = dock_state.find_tab(&tab) {
        dock_state.set_active_tab((surface, node, tab_idx));
        dock_state.set_focused_node_and_surface((surface, node));
    }
}
/// Per-frame viewer struct implementing `egui_dock::TabViewer` for analysis tabs.
#[allow(dead_code)]
struct AnalysisPaneViewer<'a> {
    // Read-only shared data
    comparison_ships: &'a [ComparisonShip],
    ifhe_enabled: bool,
    wows_data: &'a SharedWoWsData,
    ship_catalog: Option<&'a crate::armor_viewer::ship_selector::ShipCatalog>,
    translate_part: &'a dyn Fn(&str) -> String,
    active_pane: Option<&'a ArmorPane>,

    // Ships tab deferred mutations
    ifhe_cell: &'a Cell<bool>,
    search_cell: &'a RefCell<String>,
    ships_to_add: &'a RefCell<Vec<String>>,
    remove_ship_idx: &'a Cell<Option<usize>>,
    clear_all_ships: &'a Cell<bool>,

    // Trajectory tab output
    trajectory_actions: &'a RefCell<TrajectoryActions>,
}

impl TabViewer for AnalysisPaneViewer<'_> {
    type Tab = AnalysisTab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("analysis_tab", *tab))
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            AnalysisTab::Ships => "Ships",
            AnalysisTab::Trajectory => "Trajectory",
            AnalysisTab::Splash => "Splash",
        }
        .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            AnalysisTab::Ships => self.show_ships_tab(ui),
            AnalysisTab::Trajectory => self.show_trajectory_tab(ui),
            AnalysisTab::Splash => self.show_splash_tab(ui),
        }
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }
}
/// Draw the unified analysis window. Returns deferred trajectory actions
/// that the caller must apply to the active pane.
pub fn show_analysis_window(
    ctx: &egui::Context,
    state: &mut ArmorViewerState,
    translate_part: &dyn Fn(&str) -> String,
    wows_data: &SharedWoWsData,
    ship_catalog: Option<&crate::armor_viewer::ship_selector::ShipCatalog>,
) -> TrajectoryActions {
    if !state.show_comparison_panel {
        return TrajectoryActions::default();
    }

    // Create cells for deferred mutations
    let ifhe_cell = Cell::new(state.ifhe_enabled);
    let search_cell = RefCell::new(state.comparison_search.clone());
    let ships_to_add = RefCell::new(Vec::new());
    let remove_ship_idx = Cell::new(None);
    let clear_all_ships = Cell::new(false);
    let trajectory_actions = RefCell::new(TrajectoryActions::default());

    // Find active pane (immutable borrow of armor dock_state)
    let active_pane_id = state.active_pane_id;
    let active_pane: Option<&ArmorPane> =
        state.dock_state.iter_all_tabs().find(|(_, tab)| tab.id == active_pane_id).map(|(_, tab)| tab);

    let mut viewer = AnalysisPaneViewer {
        comparison_ships: &state.comparison_ships,
        ifhe_enabled: state.ifhe_enabled,
        wows_data,
        ship_catalog,
        translate_part,
        active_pane,
        ifhe_cell: &ifhe_cell,
        search_cell: &search_cell,
        ships_to_add: &ships_to_add,
        remove_ship_idx: &remove_ship_idx,
        clear_all_ships: &clear_all_ships,
        trajectory_actions: &trajectory_actions,
    };

    // Move analysis dock state out to avoid borrow conflicts with the closure
    let mut analysis_dock = std::mem::replace(&mut state.analysis_dock_state, DockState::new(vec![]));

    let mut open = state.show_comparison_panel;
    egui::Window::new("Analysis")
        .id(egui::Id::new("analysis_panel"))
        .open(&mut open)
        .collapsible(true)
        .resizable(true)
        .default_width(400.0)
        .show(ctx, |ui| {
            DockArea::new(&mut analysis_dock)
                .id(egui::Id::new("analysis_dock"))
                .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                .show_close_buttons(false)
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .allowed_splits(egui_dock::AllowedSplits::All)
                .show_inside(ui, &mut viewer);
        });

    // Put the dock state back
    state.analysis_dock_state = analysis_dock;
    state.show_comparison_panel = open;

    // Apply deferred Ships tab mutations
    state.ifhe_enabled = ifhe_cell.get();
    state.comparison_search = search_cell.into_inner();
    for param_idx in ships_to_add.into_inner() {
        let wd = wows_data.read();
        if let Some(metadata) = wd.game_metadata.as_ref()
            && let Some(ship) = crate::armor_viewer::penetration::resolve_ship_shells(metadata, &param_idx)
        {
            state.comparison_ships.push(ship);
            state.comparison_ships_version += 1;
        }
    }
    if let Some(idx) = remove_ship_idx.get() {
        state.comparison_ships.remove(idx);
        state.comparison_ships_version += 1;
    }
    if clear_all_ships.get() {
        state.comparison_ships.clear();
        state.comparison_ships_version += 1;
    }

    trajectory_actions.into_inner()
}
impl AnalysisPaneViewer<'_> {
    fn show_ships_tab(&self, ui: &mut egui::Ui) {
        // IFHE toggle
        let mut ifhe = self.ifhe_cell.get();
        if ui.checkbox(&mut ifhe, "IFHE (+25% HE penetration)").changed() {
            self.ifhe_cell.set(ifhe);
        }
        ui.separator();

        // Search bar
        let mut search = self.search_cell.borrow_mut();
        ui.horizontal(|ui| {
            ui.label(icons::MAGNIFYING_GLASS);
            ui.text_edit_singleline(&mut *search);
        });

        // Search results
        if !search.is_empty()
            && let Some(catalog) = self.ship_catalog
        {
            let search_lower = unidecode::unidecode(&search).to_lowercase();
            let already_added: std::collections::HashSet<&str> =
                self.comparison_ships.iter().map(|s| s.param_index.as_str()).collect();

            let mut results = Vec::new();
            for nation in &catalog.nations {
                for class in &nation.classes {
                    for ship in &class.ships {
                        if ship.search_name.contains(&search_lower)
                            && !already_added.contains(ship.param_index.as_str())
                        {
                            results.push(ship.clone());
                        }
                    }
                }
            }
            results.truncate(10);

            if !results.is_empty() {
                egui::ScrollArea::vertical().id_salt("pen_check_search_results").max_height(150.0).show(ui, |ui| {
                    for ship in &results {
                        let label = format!(
                            "{} {}",
                            crate::armor_viewer::ship_selector::tier_roman(ship.tier),
                            &ship.display_name
                        );
                        if ui.button(label).clicked() {
                            self.ships_to_add.borrow_mut().push(ship.param_index.clone());
                            search.clear();
                        }
                    }
                });
            }
        }
        // Drop the search borrow before the rest of the UI
        drop(search);

        ui.separator();

        // Added ships list
        if self.comparison_ships.is_empty() {
            ui.label(
                egui::RichText::new("Search and add ships above to compare penetration")
                    .small()
                    .color(egui::Color32::GRAY),
            );
        } else {
            egui::ScrollArea::vertical().id_salt("pen_check_ships_list").max_height(300.0).show(ui, |ui| {
                for (i, ship) in self.comparison_ships.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button(icons::X).clicked() {
                            self.remove_ship_idx.set(Some(i));
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
                        let pen_text = match &shell.ammo_type {
                            AmmoType::HE => {
                                let pen = shell.he_pen_mm.unwrap_or(0.0);
                                format!(
                                    "  {} {:.0}mm — {:.0}mm pen",
                                    shell.ammo_type.display_name(),
                                    shell.caliber.value(),
                                    pen
                                )
                            }
                            AmmoType::SAP => {
                                let pen = shell.sap_pen_mm.unwrap_or(0.0);
                                format!(
                                    "  {} {:.0}mm — {:.0}mm pen",
                                    shell.ammo_type.display_name(),
                                    shell.caliber.value(),
                                    pen
                                )
                            }
                            AmmoType::AP => {
                                format!(
                                    "  {} {:.0}mm — {:.0} krupp",
                                    shell.ammo_type.display_name(),
                                    shell.caliber.value(),
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

            if ui.button("Clear all").clicked() {
                self.clear_all_ships.set(true);
            }
        }
    }

    // ─── Trajectory Tab ──────────────────────────────────────────────────────

    fn show_trajectory_tab(&self, ui: &mut egui::Ui) {
        let mut actions = TrajectoryActions::default();

        let pane = match self.active_pane {
            Some(p) if !p.trajectories.is_empty() => p,
            Some(_) => {
                ui.label(
                    egui::RichText::new("Click on armor in Trajectory mode to cast shell arcs.")
                        .small()
                        .color(egui::Color32::GRAY),
                );
                return;
            }
            None => {
                ui.label(egui::RichText::new("No active pane.").small().color(egui::Color32::GRAY));
                return;
            }
        };

        let pane_id = pane.id;

        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(
                    "This simulation is based on reverse engineered data and may not accurately reflect how the game simulates ballistics.",
                )
                .small()
                .color(egui::Color32::from_rgb(220, 160, 60)),
            );
        });
        ui.separator();

        // Shared range slider
        {
            let mut range_km_val = pane.ballistic_range.value();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Range:").small().color(egui::Color32::GRAY));
                ui.add(egui::Slider::new(&mut range_km_val, 0.0..=30.0).suffix(" km").step_by(0.5).max_decimals(1));
            });
            let new_range = Km::new(range_km_val);
            if (new_range.value() - pane.ballistic_range.value()).abs() > 0.01 {
                actions.new_range = Some(new_range);
            }
        }

        ui.horizontal(|ui| {
            if ui.button("Clear All").clicked() {
                actions.clear_all = true;
            }
            if ui.button("Show Hit Plates").on_hover_text("Isolate only armor plates hit by all trajectories").clicked()
            {
                actions.show_all_hit_plates = true;
            }
            if ui.button("Show Hit Zones").on_hover_text("Isolate entire armor zones hit by all trajectories").clicked()
            {
                actions.show_all_hit_zones = true;
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
        let ifhe_enabled = self.ifhe_enabled;
        let translate_part = self.translate_part;

        // Helper: render one trajectory column
        let render_traj_column =
            |ui: &mut egui::Ui, ti: usize, traj: &StoredTrajectory, actions: &mut TrajectoryActions| {
                let result = &traj.result;
                let palette_color = TRAJECTORY_PALETTE[traj.meta.color_index % TRAJECTORY_PALETTE.len()];
                let header_color = egui::Color32::from_rgba_unmultiplied(
                    (palette_color[0] * 255.0) as u8,
                    (palette_color[1] * 255.0) as u8,
                    (palette_color[2] * 255.0) as u8,
                    255,
                );

                // Header line with color swatch
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Arc {}", ti + 1)).strong().color(header_color));
                    ui.label(
                        egui::RichText::new(format!(
                            "{} hits, {:.0}mm @ {:.1}km",
                            result.hits.len(),
                            result.total_armor_mm,
                            traj.meta.range.value(),
                        ))
                        .small()
                        .color(egui::Color32::GRAY),
                    );
                });

                // Toolbar: delete + isolation toggles
                ui.horizontal(|ui| {
                    if ui.button(icons::TRASH).on_hover_text("Delete this arc").clicked() {
                        actions.delete_id = Some(traj.meta.id);
                    }
                    if ui
                        .selectable_label(traj.show_plates_active, "Isolate Plates")
                        .on_hover_text("Show only plates hit by this arc")
                        .clicked()
                    {
                        actions.arc_plate_toggles.push((ti, !traj.show_plates_active));
                    }
                    if ui
                        .selectable_label(traj.show_zones_active, "Isolate Zones")
                        .on_hover_text("Show entire zones hit by this arc")
                        .clicked()
                    {
                        actions.arc_zone_toggles.push((ti, !traj.show_zones_active));
                    }
                });

                // Per-trajectory range slider
                {
                    let mut rng = traj.meta.range.value();
                    let resp = ui.add(
                        egui::Slider::new(&mut rng, 0.0..=30.0).suffix(" km").step_by(0.5).max_decimals(1).text(""),
                    );
                    if resp.changed() {
                        actions.per_arc_range_changes.push((ti, Km::new(rng)));
                    }
                }

                // Ballistic impact info (per-ship)
                for arc_data in &result.ship_arcs {
                    if let Some(ref impact) = arc_data.ballistic_impact {
                        let sc = SHIP_COLORS[arc_data.ship_index % SHIP_COLORS.len()];
                        ui.horizontal(|ui| {
                            if result.ship_arcs.len() > 1 {
                                ui.label(egui::RichText::new("\u{25CF}").color(egui::Color32::from_rgb(
                                    (sc[0] * 255.0) as u8,
                                    (sc[1] * 255.0) as u8,
                                    (sc[2] * 255.0) as u8,
                                )));
                            }
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
                        });
                    }
                }

                // Use cached shell simulation results (populated by update_shell_sim_cache
                // when trajectory is created, range changes, or comparison ships change).
                let empty_sims: Vec<crate::armor_viewer::state::CachedShellSim> = Vec::new();
                let (shell_sims, last_visible_hit) = match &traj.shell_sim_cache {
                    Some(cache) => (&cache.sims, cache.last_visible_hit),
                    None => (&empty_sims, None),
                };

                // Outcome badges per shell
                for ss in shell_sims {
                    let ammo = ss.shell.ammo_type.display_name();
                    let shell_label = format!("{} {} {:.0}mm", &ss.ship_name, ammo, ss.shell.caliber.value());
                    if let Some(ref sim) = ss.sim {
                        use crate::armor_viewer::penetration::PlateOutcome;
                        let (icon, badge_color, outcome_text) = if let Some(det_idx) = sim.detonated_at {
                            let volume_desc = crate::armor_viewer::penetration::enclosing_zone(&result.hits, det_idx);
                            (
                                icons::BOMB,
                                egui::Color32::from_rgb(255, 140, 40),
                                format!("detonation inside {}", volume_desc),
                            )
                        } else if let Some(stop_idx) = sim.stopped_at {
                            let plate_desc = result
                                .hits
                                .get(stop_idx)
                                .map(|h| {
                                    format!("#{} {:.0}mm {}", stop_idx + 1, h.thickness_mm, translate_part(&h.material))
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
                            (icons::ARROWS_OUT_SIMPLE, egui::Color32::from_rgb(220, 180, 80), "overpen".to_string())
                        } else {
                            (
                                icons::ARROWS_OUT_SIMPLE,
                                egui::Color32::from_rgb(220, 180, 80),
                                "overpen (fuse never armed)".to_string(),
                            )
                        };

                        ui.horizontal(|ui| {
                            let sc = SHIP_COLORS[ss.ship_index % SHIP_COLORS.len()];
                            let ship_dot_color = egui::Color32::from_rgb(
                                (sc[0] * 255.0) as u8,
                                (sc[1] * 255.0) as u8,
                                (sc[2] * 255.0) as u8,
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
                    let is_post_detonation = last_visible_hit.is_some_and(|lv| i > lv);

                    // Skip ghost plates that have no detonation event on them
                    if is_post_detonation {
                        let has_detonation_here =
                            shell_sims.iter().any(|ss| ss.sim.as_ref().is_some_and(|sim| sim.detonated_at == Some(i)));
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
                        ui.label(egui::RichText::new(format!("#{}", i + 1)).small().color(egui::Color32::GRAY));
                        ui.label(egui::RichText::new(format!("{:.0} mm", hit.thickness_mm)).strong().color(color));
                        ui.label(egui::RichText::new(format!("{:.1}\u{00B0}", hit.angle_deg)).small().color(color));
                    });
                    ui.label(
                        egui::RichText::new(format!("  {} / {}", &hit.zone, translate_part(&hit.material)))
                            .small()
                            .color(egui::Color32::GRAY),
                    );

                    if !is_post_detonation {
                        for ss in shell_sims {
                            if let Some(ref sim) = ss.sim {
                                if let Some(plate) = sim.plates.get(i) {
                                    use crate::armor_viewer::penetration::PlateOutcome;
                                    let (icon, detail_color, detail) = match plate.outcome {
                                        PlateOutcome::Overmatch => (
                                            "\u{2705}",
                                            egui::Color32::from_rgb(100, 220, 100),
                                            format!(
                                                "overmatch \u{2014} {:.0}mm pen, v={:.0} m/s",
                                                plate.raw_pen_before_mm, plate.velocity_before
                                            ),
                                        ),
                                        PlateOutcome::Penetrate => (
                                            "\u{2705}",
                                            egui::Color32::from_rgb(100, 220, 100),
                                            format!(
                                                "{:.0}/{:.0}mm eff \u{2014} v={:.0} m/s",
                                                plate.raw_pen_before_mm,
                                                plate.effective_thickness_mm,
                                                plate.velocity_before
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
                                                plate.raw_pen_before_mm, plate.effective_thickness_mm
                                            ),
                                        ),
                                    };

                                    ui.horizontal(|ui| {
                                        ui.add_space(12.0);
                                        let sc = SHIP_COLORS[ss.ship_index % SHIP_COLORS.len()];
                                        ui.label(egui::RichText::new("\u{25CF}").color(egui::Color32::from_rgb(
                                            (sc[0] * 255.0) as u8,
                                            (sc[1] * 255.0) as u8,
                                            (sc[2] * 255.0) as u8,
                                        )));
                                        ui.label(egui::RichText::new(icon));
                                        let mut label_text = format!(
                                            "{} {} {:.0}mm",
                                            &ss.ship_name,
                                            ss.shell.ammo_type.display_name(),
                                            ss.shell.caliber.value(),
                                        );
                                        if plate.fuse_armed_here {
                                            label_text.push_str(" \u{23F1}");
                                        }
                                        ui.label(egui::RichText::new(label_text).small().color(detail_color));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.add_space(28.0);
                                        ui.label(egui::RichText::new(detail).small().color(egui::Color32::GRAY));
                                    });
                                }
                            } else if i == 0 {
                                // HE/SAP on first hit
                                let (icon, detail_color, detail) = match &ss.shell.ammo_type {
                                    AmmoType::HE => {
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
                                                format!("{:.0}mm pen < {:.0}mm", pen, hit.thickness_mm),
                                            )
                                        }
                                    }
                                    AmmoType::SAP => {
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
                                                format!("{:.0}mm pen < {:.0}mm", pen, hit.thickness_mm),
                                            )
                                        }
                                    }
                                    _ => ("\u{2796}", egui::Color32::GRAY, "unknown".to_string()),
                                };
                                ui.horizontal(|ui| {
                                    ui.add_space(12.0);
                                    let sc = SHIP_COLORS[ss.ship_index % SHIP_COLORS.len()];
                                    ui.label(egui::RichText::new("\u{25CF}").color(egui::Color32::from_rgb(
                                        (sc[0] * 255.0) as u8,
                                        (sc[1] * 255.0) as u8,
                                        (sc[2] * 255.0) as u8,
                                    )));
                                    ui.label(egui::RichText::new(icon).color(detail_color));
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} {} {:.0}mm",
                                            &ss.ship_name,
                                            ss.shell.ammo_type.display_name(),
                                            ss.shell.caliber.value(),
                                        ))
                                        .small()
                                        .color(detail_color),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    ui.add_space(28.0);
                                    ui.label(egui::RichText::new(detail).small().color(egui::Color32::GRAY));
                                });
                            }
                        }
                    }

                    // Inline detonation markers
                    for ss in shell_sims {
                        if let Some(ref sim) = ss.sim
                            && sim.detonated_at == Some(i)
                            && let Some(ref det) = sim.detonation
                        {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                let sc = SHIP_COLORS[ss.ship_index % SHIP_COLORS.len()];
                                ui.label(egui::RichText::new("\u{25CF}").color(egui::Color32::from_rgb(
                                    (sc[0] * 255.0) as u8,
                                    (sc[1] * 255.0) as u8,
                                    (sc[2] * 255.0) as u8,
                                )));
                                ui.label(egui::RichText::new(icons::BOMB).color(egui::Color32::from_rgb(255, 140, 40)));
                                let volume_desc = crate::armor_viewer::penetration::enclosing_zone(&result.hits, i);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} {} detonates inside {} \u{2014} {:.1}m after plate #{}",
                                        &ss.ship_name,
                                        ss.shell.ammo_type.display_name(),
                                        volume_desc,
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

                    if i + 1 < result.hits.len() {
                        ui.separator();
                    }
                }
            };

        egui::ScrollArea::vertical().id_salt(("traj_scroll", pane_id)).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for ti in 0..traj_count {
                    if ti > 0 {
                        ui.separator();
                    }
                    ui.push_id(("traj_col", pane.trajectories[ti].meta.id), |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(320.0);
                            render_traj_column(ui, ti, &pane.trajectories[ti], &mut actions);
                        });
                    });
                }
            });
        });

        *self.trajectory_actions.borrow_mut() = actions;
    }

    // ─── Splash Tab ──────────────────────────────────────────────────────────

    fn show_splash_tab(&self, ui: &mut egui::Ui) {
        let splash_result = match self.active_pane.and_then(|p| p.splash_result.as_ref()) {
            Some(r) => r,
            None => {
                ui.label(
                    egui::RichText::new("Click on armor in Splash mode to analyze HE splash damage.")
                        .small()
                        .color(egui::Color32::GRAY),
                );
                return;
            }
        };

        // Collect all HE/SAP shells with their ship names
        let shells_with_names: Vec<(&str, &ShellInfo)> = self
            .comparison_ships
            .iter()
            .flat_map(|s| {
                s.shells
                    .iter()
                    .filter(|sh| sh.ammo_type == AmmoType::HE || sh.ammo_type == AmmoType::SAP)
                    .map(move |sh| (s.display_name.as_str(), sh))
            })
            .collect();

        // Splash cube info
        ui.label(
            egui::RichText::new(format!("Splash half-extent: {:.4} model units", splash_result.half_extent.value(),))
                .small()
                .color(egui::Color32::GRAY),
        );

        if splash_result.triangles_in_volume > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "Triangles in volume: {}  |  Penetrated: {}",
                    splash_result.triangles_in_volume, splash_result.triangles_penetrated,
                ))
                .small(),
            );
        }

        ui.separator();

        // Hit zones table with per-shell penetration
        if splash_result.hit_zones.is_empty() {
            ui.label(
                egui::RichText::new("No splash boxes overlap at this point.")
                    .color(egui::Color32::from_rgb(180, 180, 180)),
            );
        } else {
            ui.label(egui::RichText::new("Zones in splash volume:").strong());
            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                for zone in &splash_result.hit_zones {
                    ui.horizontal(|ui| {
                        let label = if zone.is_direct_hit {
                            format!("\u{25C9} {}", zone.zone_name)
                        } else {
                            format!("  {}", zone.zone_name)
                        };
                        ui.label(egui::RichText::new(label).strong().small());
                        if zone.thickness.value() > 0.0 {
                            ui.label(
                                egui::RichText::new(format!("{:.0}mm", zone.thickness.value()))
                                    .small()
                                    .color(egui::Color32::GRAY),
                            );
                        }
                        if zone.max_hp > 0.0 {
                            ui.label(
                                egui::RichText::new(format!("{:.0} HP", zone.max_hp))
                                    .small()
                                    .color(egui::Color32::GRAY),
                            );
                        }
                    });

                    // Per-shell penetration results
                    for (ship_name, shell) in &shells_with_names {
                        let pen_mm = crate::armor_viewer::splash::shell_pen_mm(shell, self.ifhe_enabled);
                        let penetrates =
                            crate::armor_viewer::splash::shell_penetrates(shell, zone.thickness, self.ifhe_enabled);
                        let ammo_label = match shell.ammo_type {
                            AmmoType::HE => "HE",
                            AmmoType::SAP => "SAP",
                            _ => "?",
                        };
                        let (icon, color) = if penetrates {
                            ("\u{2705}", egui::Color32::from_rgb(100, 220, 100))
                        } else {
                            ("\u{274C}", egui::Color32::from_rgb(220, 100, 100))
                        };
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new(icon).small());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {:.0}mm {} \u{2014} {:.0}mm pen{}",
                                    ship_name,
                                    shell.caliber.value(),
                                    ammo_label,
                                    pen_mm,
                                    if self.ifhe_enabled { " (IFHE)" } else { "" },
                                ))
                                .small()
                                .color(color),
                            );
                        });
                    }

                    ui.add_space(2.0);
                }
            });
        }
    }
}
