use egui::Image;
use egui::ImageSource;
use egui::RichText;
use egui::ScrollArea;
use egui_dock::DockArea;
use egui_dock::TabViewer;
use egui_dock::tab_viewer::OnCloseResponse;

use crate::app::ToolkitTabViewer;
use crate::icon_str;
use crate::icons;
use crate::data::session_stats::DivisionFilter;
use crate::data::session_stats::PerformanceInfo;
use crate::tab_state::ChartMode;
use crate::tab_state::ChartableStat;
use crate::tab_state::StatsSubTab;
use crate::ui::session_stats_chart::render_bar_chart;
use crate::ui::session_stats_chart::render_line_chart;
use crate::util::separate_number;
use crate::data::wows_data::GameAsset;
use std::cmp::Reverse;
use std::sync::Arc;

/// TabViewer for the stats sub-tabs (Overview / Charts).
///
/// This borrows everything it needs from TabState so that each sub-tab can
/// render its content inside the `egui_dock` DockArea.
struct StatsTabViewer<'a> {
    tab_state: &'a mut crate::tab_state::TabState,
    /// Cached count of Charts tabs — used to decide closability.
    chart_tab_count: usize,
    /// Pending tab additions (surface, node) — applied after show_inside returns,
    /// because the dock state is swapped out during rendering.
    pending_adds: Vec<(egui_dock::SurfaceIndex, egui_dock::NodeIndex)>,
}

impl TabViewer for StatsTabViewer<'_> {
    type Tab = StatsSubTab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("stats_sub_tab", *tab))
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            StatsSubTab::Overview => icon_str!(icons::LIST, "Overview").into(),
            StatsSubTab::Charts(id) => {
                let cfg = self.tab_state.chart_config(*id);
                let stat = cfg.selected_stat;
                let combined = cfg.combined;
                let rolling_average = cfg.rolling_average || combined;
                let mode = cfg.mode;

                let title = match mode {
                    ChartMode::Bar => match stat {
                        ChartableStat::WinRate => stat.name().to_string(),
                        _ => format!("Avg {}", stat.name()),
                    },
                    ChartMode::Line => {
                        if combined {
                            format!("{} (Combined)", stat.name())
                        } else if rolling_average {
                            format!("{} (Rolling Average)", stat.name())
                        } else {
                            stat.name().to_string()
                        }
                    }
                };

                let icon = match mode {
                    ChartMode::Line => icons::CHART_LINE,
                    ChartMode::Bar => icons::CHART_BAR,
                };

                format!("{icon} {title}").into()
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match *tab {
            StatsSubTab::Overview => build_stats_overview(self.tab_state, ui),
            StatsSubTab::Charts(id) => build_stats_charts(self.tab_state, id, ui),
        }
    }

    fn closeable(&mut self, tab: &mut Self::Tab) -> bool {
        match tab {
            StatsSubTab::Overview => false,
            // Charts tabs are only closeable when there are more than one.
            StatsSubTab::Charts(_) => self.chart_tab_count > 1,
        }
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        if let StatsSubTab::Charts(id) = tab {
            self.tab_state.remove_chart_config(*id);
        }
        OnCloseResponse::Close
    }

    fn on_add(&mut self, surface: egui_dock::SurfaceIndex, node: egui_dock::NodeIndex) {
        self.pending_adds.push((surface, node));
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }
}

impl ToolkitTabViewer<'_> {
    pub fn build_stats_tab(&mut self, ui: &mut egui::Ui) {
        // ── Shared filter bar (above the dock, applies to all sub-tabs) ──
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.tab_state.settings.session_stats_limit_enabled, "Limit");
            let mut value = self.tab_state.settings.session_stats_game_count as u32;
            if ui
                .add_enabled(
                    self.tab_state.settings.session_stats_limit_enabled,
                    egui::DragValue::new(&mut value).range(1..=999).speed(0.2),
                )
                .changed()
            {
                self.tab_state.settings.session_stats_game_count = value as usize;
            }

            ui.separator();

            ui.label("Div:");
            ui.selectable_value(&mut self.tab_state.settings.session_stats_division_filter, DivisionFilter::All, "All");
            ui.selectable_value(
                &mut self.tab_state.settings.session_stats_division_filter,
                DivisionFilter::SoloOnly,
                "Solo",
            );
            ui.selectable_value(
                &mut self.tab_state.settings.session_stats_division_filter,
                DivisionFilter::DivOnly,
                "Div",
            );

            let all_modes = self.tab_state.settings.session_stats.all_match_groups();
            if all_modes.len() > 1 {
                ui.separator();
                ui.label("Mode:");
                if ui
                    .selectable_label(self.tab_state.settings.session_stats_game_mode_filter.is_empty(), "All")
                    .clicked()
                {
                    self.tab_state.settings.session_stats_game_mode_filter.clear();
                }
                for mode in &all_modes {
                    let display = crate::data::session_stats::match_group_display_name(mode);
                    let mut is_selected = self.tab_state.settings.session_stats_game_mode_filter.contains(mode);
                    if ui.selectable_label(is_selected, display).clicked() {
                        is_selected = !is_selected;
                        if is_selected {
                            self.tab_state.settings.session_stats_game_mode_filter.insert(mode.clone());
                        } else {
                            self.tab_state.settings.session_stats_game_mode_filter.remove(mode);
                        }
                    }
                }
            }

            ui.separator();

            if ui.button(icon_str!(icons::ERASER, "Clear")).clicked() {
                self.tab_state.pending_confirmation = Some(crate::tab_state::ConfirmableAction::ClearSessionStats);
            }
        });

        // Sync filter state to session_stats
        self.tab_state.settings.session_stats.game_count_limit = if self.tab_state.settings.session_stats_limit_enabled
        {
            Some(self.tab_state.settings.session_stats_game_count)
        } else {
            None
        };
        self.tab_state.settings.session_stats.division_filter = self.tab_state.settings.session_stats_division_filter;
        self.tab_state.settings.session_stats.game_mode_filter =
            self.tab_state.settings.session_stats_game_mode_filter.iter().cloned().collect();

        // ── Dock area with sub-tabs ──
        // Validate persisted dock state: must have Overview and at least one Charts tab.
        {
            let has_overview =
                self.tab_state.stats_dock_state.iter_all_tabs().any(|(_, t)| matches!(t, StatsSubTab::Overview));
            let has_chart =
                self.tab_state.stats_dock_state.iter_all_tabs().any(|(_, t)| matches!(t, StatsSubTab::Charts(_)));
            if !has_overview || !has_chart {
                self.tab_state.stats_dock_state = crate::tab_state::default_stats_dock_state();
            }
        }

        // Move dock state out temporarily to avoid double-borrow of tab_state
        let mut dock_state = std::mem::replace(&mut self.tab_state.stats_dock_state, egui_dock::DockState::new(vec![]));

        let chart_tab_count =
            dock_state.iter_all_tabs().filter(|(_, tab)| matches!(tab, StatsSubTab::Charts(_))).count();

        let mut viewer = StatsTabViewer { tab_state: self.tab_state, chart_tab_count, pending_adds: Vec::new() };

        DockArea::new(&mut dock_state)
            .id(egui::Id::new("stats_dock"))
            .style(egui_dock::Style::from_egui(ui.style().as_ref()))
            .show_add_buttons(true)
            .show_close_buttons(true)
            .show_leaf_collapse_buttons(false)
            .show_leaf_close_all_buttons(false)
            .allowed_splits(egui_dock::AllowedSplits::All)
            .show_inside(ui, &mut viewer);

        // Apply pending tab additions now that we have the real dock_state
        for (surface, node) in viewer.pending_adds {
            let id = self.tab_state.next_chart_tab_id;
            self.tab_state.next_chart_tab_id += 1;
            let tab = StatsSubTab::Charts(id);
            if let Some(leaf) = dock_state[surface][node].get_leaf_mut() {
                leaf.append_tab(tab);
            }
        }

        // Put the dock state back
        self.tab_state.stats_dock_state = dock_state;
    }
}
fn build_stats_overview(tab_state: &mut crate::tab_state::TabState, ui: &mut egui::Ui) {
    // ── Summary stats: compact horizontal flow ──
    let wins = tab_state.settings.session_stats.games_won();
    let losses = tab_state.settings.session_stats.games_lost();
    let draws = tab_state.settings.session_stats.games_drawn();
    let win_rate = tab_state.settings.session_stats.win_rate().unwrap_or_default();
    let locale = tab_state.settings.locale.as_deref();

    ui.horizontal_wrapped(|ui| {
        // Win rate
        let wld = if draws > 0 { format!("{wins}W/{losses}L/{draws}D") } else { format!("{wins}W/{losses}L") };
        ui.strong(format!("{wld} ({win_rate:.01}%)"));

        // PR
        if let Some(pr_result) = tab_state.settings.session_stats.calculate_pr(&tab_state.personal_rating_data.read()) {
            ui.separator();
            ui.label("PR:");
            ui.label(
                RichText::new(format!("{:.0} ({})", pr_result.pr, pr_result.category.name()))
                    .color(pr_result.category.color())
                    .strong(),
            );
        }

        // Total frags
        let total_frags = tab_state.settings.session_stats.total_frags();
        ui.separator();
        ui.label(format!("{total_frags} Frags"));

        // Max frags
        if let Some((ship_name, max_frags)) = tab_state.settings.session_stats.max_frags() {
            ui.separator();
            ui.label(format!("Best: {ship_name} ({max_frags} kills)"));
        }

        // Max damage
        if let Some((ship_name, max_damage)) = tab_state.settings.session_stats.max_damage() {
            ui.separator();
            ui.label(format!("Max DMG: {ship_name} ({})", separate_number(max_damage, locale)));
        }
    });

    // ── Achievements ──
    let mut all_achievements: Vec<crate::data::session_stats::SerializableAchievement> = Vec::new();
    for game in tab_state.settings.session_stats.filtered_games() {
        for achievement in &game.achievements {
            match all_achievements.iter_mut().find(|existing| existing.game_param_id == achievement.game_param_id) {
                Some(existing) => {
                    existing.count += achievement.count;
                }
                None => all_achievements.push(achievement.clone()),
            }
        }
    }

    all_achievements.sort_by(|a, b| (Reverse(a.count), &b.display_name).cmp(&(Reverse(b.count), &b.display_name)));

    if !all_achievements.is_empty()
        && let Some(wows_data_lock) = tab_state.world_of_warships_data.as_ref()
    {
        let icons: Vec<Option<Arc<GameAsset>>> = {
            let wows_data = wows_data_lock.read();
            all_achievements.iter().map(|a| wows_data.cached_achievement_icon(&a.icon_key)).collect()
        };

        let icons: Vec<Option<Arc<GameAsset>>> = if icons.iter().any(|i| i.is_none()) {
            let mut wows_data = wows_data_lock.write();
            all_achievements
                .iter()
                .zip(icons)
                .map(|(a, cached)| cached.or_else(|| wows_data.load_achievement_icon(&a.icon_key)))
                .collect()
        } else {
            icons
        };

        ui.horizontal_wrapped(|ui| {
            for (achievement, icon) in all_achievements.iter().zip(icons) {
                let tooltip = format!("{}: {}", &achievement.display_name, &achievement.description);
                ui.vertical(|ui| {
                    ui.set_width(56.0);
                    if let Some(icon) = icon {
                        let image = Image::new(ImageSource::Bytes {
                            uri: icon.path.clone().into(),
                            bytes: icon.data.clone().into(),
                        })
                        .fit_to_exact_size((48.0, 48.0).into());
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.add(image);
                            ui.label(RichText::new(format!("x{}", achievement.count)).small().strong());
                            ui.label(RichText::new(&achievement.display_name).small().weak());
                        });
                    } else {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("x{}", achievement.count)).strong());
                            ui.label(RichText::new(&achievement.display_name).small().weak());
                        });
                    }
                })
                .response
                .on_hover_text(&tooltip);
            }
        });
    }

    ui.separator();

    ScrollArea::vertical().show(ui, |ui| {
        // Collect per-ship PR stats (min/max/avg) before entering the mutable loop
        let pr_stats_by_ship: std::collections::HashMap<String, crate::data::session_stats::PrStats> = {
            let per_ship_games = tab_state.settings.session_stats.per_ship_limited_games();
            let mut games_by_ship: std::collections::HashMap<&str, Vec<&crate::data::session_stats::PerGameStat>> =
                std::collections::HashMap::new();
            for game in &per_ship_games {
                games_by_ship.entry(game.ship_name.as_str()).or_default().push(game);
            }
            let pr_data = tab_state.personal_rating_data.read();
            games_by_ship
                .into_iter()
                .filter_map(|(name, games)| {
                    crate::data::session_stats::PrStats::from_games(&games, &pr_data).map(|pr| (name.to_string(), pr))
                })
                .collect()
        };

        let mut battle_results: Vec<(String, PerformanceInfo)> =
            tab_state.settings.session_stats.ship_stats_per_ship_limited().drain().collect();
        battle_results.sort_by(|a, b| b.1.last_played().cmp(a.1.last_played()));
        for (ship_name, perf_info) in battle_results {
            if perf_info.win_rate().is_none() {
                continue;
            }

            let locale = tab_state.settings.locale.as_deref();
            let pr_data = tab_state.personal_rating_data.read();
            let ship_pr = perf_info.calculate_pr(&pr_data);
            drop(pr_data);
            let pr_stats = pr_stats_by_ship.get(&ship_name);

            let wld = if perf_info.draws() > 0 {
                format!("{}W/{}L/{}D", perf_info.wins(), perf_info.losses(), perf_info.draws())
            } else {
                format!("{}W/{}L", perf_info.wins(), perf_info.losses())
            };
            let header = if let Some(ref pr) = ship_pr {
                format!("{ship_name} {wld} ({:.0}%) - PR: {:.0}", perf_info.win_rate().unwrap(), pr.pr)
            } else {
                format!("{ship_name} {wld} ({:.0}%)", perf_info.win_rate().unwrap())
            };

            // Build table rows for copy-to-clipboard (label, min, max, total, avg)
            let mut table_rows: Vec<[String; 5]> = Vec::new();
            if let Some(pr) = pr_stats {
                table_rows.push([
                    "Personal Rating".into(),
                    format!("{:.0}", pr.min),
                    format!("{:.0}", pr.max),
                    String::new(),
                    format!("{:.0}", pr.avg),
                ]);
            }
            table_rows.push([
                "Damage".into(),
                separate_number(perf_info.min_damage(), locale),
                separate_number(perf_info.max_damage(), locale),
                separate_number(perf_info.total_damage(), locale),
                separate_number(perf_info.avg_damage().unwrap_or_default() as u64, locale),
            ]);
            table_rows.push([
                "Spotting Damage".into(),
                separate_number(perf_info.min_spotting_damage(), locale),
                separate_number(perf_info.max_spotting_damage(), locale),
                separate_number(perf_info.total_spotting_damage(), locale),
                separate_number(perf_info.avg_spotting_damage().unwrap_or_default() as u64, locale),
            ]);
            table_rows.push([
                "Frags".into(),
                separate_number(perf_info.min_frags(), locale),
                separate_number(perf_info.max_frags(), locale),
                separate_number(perf_info.total_frags(), locale),
                format!("{:.2}", perf_info.avg_frags().unwrap_or_default()),
            ]);
            table_rows.push([
                "Raw XP".into(),
                separate_number(perf_info.min_xp(), locale),
                separate_number(perf_info.max_xp(), locale),
                separate_number(perf_info.total_xp(), locale),
                separate_number(perf_info.avg_xp().unwrap_or_default() as i64, locale),
            ]);
            table_rows.push([
                "Base XP".into(),
                separate_number(perf_info.min_win_adjusted_xp(), locale),
                separate_number(perf_info.max_win_adjusted_xp(), locale),
                separate_number(perf_info.total_win_adjusted_xp(), locale),
                separate_number(perf_info.avg_win_adjusted_xp().unwrap_or_default() as i64, locale),
            ]);

            let collapsing_id = ui.make_persistent_id(format!("ship_stats_collapse_{ship_name}"));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), collapsing_id, false)
                .show_header(ui, |ui| {
                    ui.label(&header);
                    ui.menu_button(icons::COPY, |ui| {
                        if ui.button("Copy as Markdown").clicked() {
                            let mut md = format!("**{header}**\n\n");
                            md.push_str("| | Min | Max | Total | Average |\n");
                            md.push_str("|---|---|---|---|---|\n");
                            for row in &table_rows {
                                md.push_str(&format!(
                                    "| {} | {} | {} | {} | **{}** |\n",
                                    row[0], row[1], row[2], row[3], row[4]
                                ));
                            }
                            ui.ctx().copy_text(md);
                            ui.close();
                        }
                        if ui.button("Copy as CSV").clicked() {
                            let mut csv = String::from(",Min,Max,Total,Average\n");
                            for row in &table_rows {
                                csv.push_str(&format!("{},{},{},{},{}\n", row[0], row[1], row[2], row[3], row[4]));
                            }
                            ui.ctx().copy_text(csv);
                            ui.close();
                        }
                    });
                    if ui
                        .small_button(icons::TRASH)
                        .on_hover_text(format!("Remove all {} games (Ctrl+Click to skip confirmation)", ship_name))
                        .clicked()
                    {
                        if ui.input(|i| i.modifiers.ctrl) {
                            tab_state.settings.session_stats.clear_ship(&ship_name);
                        } else {
                            tab_state.pending_confirmation =
                                Some(crate::tab_state::ConfirmableAction::ClearShipSessionStats {
                                    ship_name: ship_name.clone(),
                                });
                        }
                    }
                })
                .body(|ui| {
                    egui::Grid::new(format!("ship_stats_{ship_name}")).num_columns(5).striped(true).show(ui, |ui| {
                        use crate::util::personal_rating::PersonalRatingCategory;

                        ui.strong("");
                        ui.strong("Min");
                        ui.strong("Max");
                        ui.strong("Total");
                        ui.strong("Average");
                        ui.end_row();

                        if let Some(pr) = pr_stats {
                            ui.label("Personal Rating");
                            let min_cat = PersonalRatingCategory::from_pr(pr.min);
                            ui.label(RichText::new(format!("{:.0}", pr.min)).color(min_cat.color()))
                                .on_hover_text(min_cat.name());
                            let max_cat = PersonalRatingCategory::from_pr(pr.max);
                            ui.label(RichText::new(format!("{:.0}", pr.max)).color(max_cat.color()))
                                .on_hover_text(max_cat.name());
                            ui.label("");
                            let avg_cat = PersonalRatingCategory::from_pr(pr.avg);
                            ui.label(RichText::new(format!("{:.0}", pr.avg)).color(avg_cat.color()).strong())
                                .on_hover_text(avg_cat.name());
                            ui.end_row();
                        }

                        ui.label("Damage");
                        ui.label(separate_number(perf_info.min_damage(), locale));
                        ui.label(separate_number(perf_info.max_damage(), locale));
                        ui.label(separate_number(perf_info.total_damage(), locale));
                        ui.strong(separate_number(perf_info.avg_damage().unwrap_or_default() as u64, locale));
                        ui.end_row();

                        ui.label("Spotting Damage");
                        ui.label(separate_number(perf_info.min_spotting_damage(), locale));
                        ui.label(separate_number(perf_info.max_spotting_damage(), locale));
                        ui.label(separate_number(perf_info.total_spotting_damage(), locale));
                        ui.strong(separate_number(perf_info.avg_spotting_damage().unwrap_or_default() as u64, locale));
                        ui.end_row();

                        ui.label("Frags");
                        ui.label(separate_number(perf_info.min_frags(), locale));
                        ui.label(separate_number(perf_info.max_frags(), locale));
                        ui.label(separate_number(perf_info.total_frags(), locale));
                        ui.strong(format!("{:.2}", perf_info.avg_frags().unwrap_or_default()));
                        ui.end_row();

                        ui.label("Raw XP");
                        ui.label(separate_number(perf_info.min_xp(), locale));
                        ui.label(separate_number(perf_info.max_xp(), locale));
                        ui.label(separate_number(perf_info.total_xp(), locale));
                        ui.strong(separate_number(perf_info.avg_xp().unwrap_or_default() as i64, locale));
                        ui.end_row();

                        ui.label("Base XP");
                        ui.label(separate_number(perf_info.min_win_adjusted_xp(), locale));
                        ui.label(separate_number(perf_info.max_win_adjusted_xp(), locale));
                        ui.label(separate_number(perf_info.total_win_adjusted_xp(), locale));
                        ui.strong(separate_number(perf_info.avg_win_adjusted_xp().unwrap_or_default() as i64, locale));
                        ui.end_row();
                    });
                });
        }
    });
}
fn build_stats_charts(tab_state: &mut crate::tab_state::TabState, chart_id: u64, ui: &mut egui::Ui) {
    // Collect all session data into owned locals first to avoid borrow conflicts
    // with chart_configs (which requires &mut self via chart_config()).
    let ship_stats: Vec<(String, PerformanceInfo)> = tab_state
        .settings
        .session_stats
        .ship_stats_per_ship_limited()
        .into_iter()
        .filter(|(_, perf)| perf.win_rate().is_some())
        .collect();

    let per_game_data: Vec<crate::data::session_stats::PerGameStat> =
        tab_state.settings.session_stats.per_ship_limited_games().into_iter().cloned().collect();

    // Clone Arc so the read guard doesn't borrow tab_state
    let pr_data_lock = Arc::clone(&tab_state.personal_rating_data);
    let pr_data = pr_data_lock.read();

    let ctx = ui.ctx().clone();
    let cfg = tab_state.chart_config(chart_id);

    // Handle screenshot capture if one was requested
    if cfg.screenshot_requested {
        let screenshot = ctx.input(|i| {
            for event in &i.raw.events {
                if let egui::Event::Screenshot { image, .. } = event {
                    return Some(image.clone());
                }
            }
            None
        });

        if let Some(screenshot) = screenshot {
            cfg.screenshot_requested = false;

            if let Some(plot_rect) = cfg.plot_rect {
                let pixels_per_point = ctx.pixels_per_point();
                let plot_image = screenshot.region(&plot_rect, Some(pixels_per_point));

                // Copy to clipboard using arboard
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let image_data = arboard::ImageData {
                        width: plot_image.width(),
                        height: plot_image.height(),
                        bytes: std::borrow::Cow::from(plot_image.as_raw().to_vec()),
                    };
                    let _ = clipboard.set_image(image_data);
                }
            }
        }
    }

    if ship_stats.is_empty() {
        ui.label("No session stats available. Play some games first!");
        return;
    }

    // Get list of ship names for selection
    let mut ship_names: Vec<String> = ship_stats.iter().map(|(name, _)| name.clone()).collect();
    ship_names.sort();

    let cfg = tab_state.chart_config(chart_id);

    // If no ships selected, select all by default
    if !cfg.selected_ships_manually_changed {
        cfg.selected_ships = ship_names.clone();
    }

    // ── Toolbar: settings popover + copy button ──
    ui.horizontal(|ui| {
        // Settings menu button with nested popup support
        ui.menu_button(icon_str!(icons::GEAR_FINE, "Settings"), |ui| {
            let cfg = tab_state.chart_config(chart_id);

            ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                ui.set_min_width(280.0);

                // ── Chart Type ──
                ui.strong("Chart Type");
                ui.indent(("chart_type_indent", chart_id), |ui| {
                    ui.label("Stat:");
                    ui.horizontal_wrapped(|ui| {
                        for stat in ChartableStat::all() {
                            if ui.selectable_label(cfg.selected_stat == *stat, stat.name()).clicked() {
                                cfg.selected_stat = *stat;
                                cfg.reset_plot = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Mode:");
                        ui.add_enabled_ui(!cfg.combined, |ui| {
                            if ui.selectable_value(&mut cfg.mode, ChartMode::Line, "Line").clicked() {
                                cfg.reset_plot = true;
                            }
                            if ui.selectable_value(&mut cfg.mode, ChartMode::Bar, "Bar").clicked() {
                                cfg.reset_plot = true;
                            }
                        });
                    });
                });

                ui.separator();

                // ── Options ──
                ui.strong("Options");
                ui.indent(("options_indent", chart_id), |ui| {
                    if ui.checkbox(&mut cfg.combined, "Combined").changed() {
                        cfg.reset_plot = true;
                        if cfg.combined {
                            cfg.mode = ChartMode::Line;
                        }
                    }
                    if cfg.mode == ChartMode::Line {
                        ui.add_enabled(!cfg.combined, egui::Checkbox::new(&mut cfg.rolling_average, "Rolling Avg"));
                    }
                    ui.checkbox(&mut cfg.show_labels, "Labels");
                });

                ui.separator();

                // ── Ships ──
                ui.strong("Ships");
                ui.indent(("ships_indent", chart_id), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("All").clicked() {
                            cfg.selected_ships = ship_names.clone();
                            cfg.selected_ships_manually_changed = true;
                        }
                        if ui.button("None").clicked() {
                            cfg.selected_ships.clear();
                            cfg.selected_ships_manually_changed = true;
                        }
                    });
                    for ship_name in &ship_names {
                        let mut is_selected = cfg.selected_ships.contains(ship_name);
                        if ui.checkbox(&mut is_selected, ship_name).changed() {
                            if is_selected {
                                cfg.selected_ships.push(ship_name.clone());
                            } else {
                                cfg.selected_ships.retain(|s| s != ship_name);
                            }
                            cfg.selected_ships_manually_changed = true;
                        }
                    }
                });
            });
        });

        // Copy as Image button (stays outside popover)
        let has_plot = tab_state.chart_config(chart_id).plot_rect.is_some();
        if has_plot && ui.button(icon_str!(icons::CAMERA, "Copy as Image")).clicked() {
            let cfg = tab_state.chart_config(chart_id);
            cfg.screenshot_requested = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    });

    // ── Chart fills all remaining space ──
    let cfg = tab_state.chart_config(chart_id);
    let selected_stat = cfg.selected_stat;
    let selected_ships = cfg.selected_ships.clone();
    let show_labels = cfg.show_labels;
    let reset_plot = std::mem::take(&mut cfg.reset_plot);
    let mode = cfg.mode;
    let rolling_average = cfg.rolling_average || cfg.combined;
    let combined = cfg.combined;

    let mut plot_rect: Option<egui::Rect> = None;

    match mode {
        ChartMode::Line => {
            let filtered_data: Vec<&crate::data::session_stats::PerGameStat> =
                per_game_data.iter().filter(|g| selected_ships.contains(&g.ship_name)).collect();

            if !filtered_data.is_empty() {
                plot_rect = render_line_chart(
                    ui,
                    &filtered_data,
                    selected_stat,
                    &selected_ships,
                    &pr_data,
                    rolling_average,
                    combined,
                    show_labels,
                    reset_plot,
                    chart_id,
                );
            }
        }
        ChartMode::Bar => {
            let mut selected_stats: Vec<(&String, &PerformanceInfo)> = ship_stats
                .iter()
                .filter(|(name, _)| selected_ships.contains(name))
                .map(|(name, perf)| (name, perf))
                .collect();

            selected_stats.sort_by_key(|a| a.0);

            if !selected_stats.is_empty() {
                plot_rect = Some(render_bar_chart(
                    ui,
                    &selected_stats,
                    selected_stat,
                    &pr_data,
                    show_labels,
                    reset_plot,
                    chart_id,
                ));
            }
        }
    }

    // Store the plot rect for screenshot cropping
    tab_state.chart_config(chart_id).plot_rect = plot_rect;
}
