//! The dockable Search tab: a structured MatchFilter bar over a results table
//! backed by the replay index.

use rust_i18n::t;

use crate::app::ToolkitTabViewer;
use crate::db::index::rows::MatchFilter;
use crate::db::index::rows::MatchHit;
use crate::db::index::rows::MatchOutcome;

pub struct SearchTabState {
    pub filter: MatchFilter,
    pub results: Vec<MatchHit>,
    /// True when `filter` changed and results must be re-queried.
    pub dirty: bool,
}

impl Default for SearchTabState {
    fn default() -> Self {
        Self { filter: MatchFilter::default(), results: Vec::new(), dirty: true }
    }
}

impl ToolkitTabViewer<'_> {
    /// Resolve a display name for the match's self ship, using the game data for the
    /// match's own build when it is loaded. Falls back to a bracketed id when no
    /// matching build is loaded or the ship id is unknown.
    fn search_ship_display_name(&self, hit: &MatchHit) -> Option<String> {
        let ship_id = hit.self_ship_id?;
        let data = hit.version_build.and_then(|build| self.tab_state.wows_data_map.as_ref()?.get(build));
        let guard = data.as_ref().map(|d| d.read());
        let provider = guard.as_ref().and_then(|g| g.game_metadata.as_deref());
        Some(crate::data::session_stats::resolve_ship_name(ship_id, provider))
    }

    pub fn build_search_tab(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            // Outcome selector: None (Any) / Win / Loss / Draw.
            let filter = &mut self.tab_state.search_tab.filter;
            egui::ComboBox::from_id_salt("search_outcome")
                .selected_text(match filter.outcome {
                    None => t!("ui.search.outcome_any"),
                    Some(MatchOutcome::Win) => t!("ui.search.outcome_win"),
                    Some(MatchOutcome::Loss) => t!("ui.search.outcome_loss"),
                    Some(MatchOutcome::Draw) => t!("ui.search.outcome_draw"),
                    Some(MatchOutcome::Unknown) => t!("ui.search.outcome_unknown"),
                })
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut filter.outcome, None, t!("ui.search.outcome_any")).changed();
                    changed |= ui
                        .selectable_value(&mut filter.outcome, Some(MatchOutcome::Win), t!("ui.search.outcome_win"))
                        .changed();
                    changed |= ui
                        .selectable_value(&mut filter.outcome, Some(MatchOutcome::Loss), t!("ui.search.outcome_loss"))
                        .changed();
                    changed |= ui
                        .selectable_value(&mut filter.outcome, Some(MatchOutcome::Draw), t!("ui.search.outcome_draw"))
                        .changed();
                });

            // Map free-text filter.
            let mut map = filter.map.clone().unwrap_or_default();
            ui.label(t!("ui.search.map_label"));
            if ui.text_edit_singleline(&mut map).changed() {
                filter.map = (!map.is_empty()).then_some(map);
                changed = true;
            }

            // Survived toggle (None = any).
            ui.label(t!("ui.search.survived_label"));
            let mut survived = filter.self_survived;
            egui::ComboBox::from_id_salt("search_survived")
                .selected_text(match survived {
                    None => t!("ui.search.survived_any"),
                    Some(true) => t!("ui.search.survived_true"),
                    Some(false) => t!("ui.search.survived_false"),
                })
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut survived, None, t!("ui.search.survived_any")).changed();
                    changed |= ui.selectable_value(&mut survived, Some(true), t!("ui.search.survived_true")).changed();
                    changed |=
                        ui.selectable_value(&mut survived, Some(false), t!("ui.search.survived_false")).changed();
                });
            filter.self_survived = survived;

            if ui.button(t!("ui.search.clear_filters")).clicked() {
                *filter = MatchFilter::default();
                changed = true;
            }
        });

        if changed {
            self.tab_state.search_tab.dirty = true;
        }

        ui.separator();

        // Re-query when the filter changed and the DB is available.
        if self.tab_state.search_tab.dirty
            && let (Some(pool), Some(rt)) = (self.tab_state.db_pool.as_ref(), self.tab_state.tokio_runtime.as_ref())
        {
            let filter = self.tab_state.search_tab.filter.clone();
            match rt.block_on(crate::db::index::query::search_matches(pool, &filter)) {
                Ok(hits) => self.tab_state.search_tab.results = hits,
                Err(e) => tracing::warn!("search query failed: {e}"),
            }
            self.tab_state.search_tab.dirty = false;
        }

        ui.label(t!("ui.search.match_count", count = self.tab_state.search_tab.results.len()));

        let mut open_path: Option<std::path::PathBuf> = None;
        egui::ScrollArea::horizontal().id_salt("search_results").show(ui, |ui| {
            use egui_extras::Column;
            use egui_extras::TableBuilder;
            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(150.0)) // date
                .column(Column::initial(120.0)) // map
                .column(Column::initial(90.0)) // mode
                .column(Column::initial(140.0)) // ship
                .column(Column::initial(60.0)) // result
                .column(Column::initial(80.0)) // dmg
                .column(Column::initial(50.0)) // kills
                .column(Column::initial(60.0)) // pr
                .column(Column::remainder()) // open
                .header(20.0, |mut h| {
                    for label in [
                        t!("ui.search.column.date"),
                        t!("ui.search.column.map"),
                        t!("ui.search.column.mode"),
                        t!("ui.search.column.ship"),
                        t!("ui.search.column.result"),
                        t!("ui.search.column.damage"),
                        t!("ui.search.column.kills"),
                        t!("ui.search.column.pr"),
                    ] {
                        h.col(|ui| {
                            ui.strong(label);
                        });
                    }
                    h.col(|_ui| {});
                })
                .body(|mut body| {
                    for hit in &self.tab_state.search_tab.results {
                        let ship_name = self.search_ship_display_name(hit);
                        body.row(24.0, |mut row| {
                            row.col(|ui| {
                                ui.label(hit.timestamp.strftime("%Y-%m-%d %H:%M").to_string());
                            });
                            row.col(|ui| {
                                ui.label(&hit.map);
                            });
                            row.col(|ui| {
                                ui.label(&hit.game_type);
                            });
                            row.col(|ui| {
                                ui.label(ship_name.clone().unwrap_or_default());
                            });
                            row.col(|ui| {
                                ui.label(match hit.outcome {
                                    MatchOutcome::Win => "W",
                                    MatchOutcome::Loss => "L",
                                    MatchOutcome::Draw => "D",
                                    MatchOutcome::Unknown => "-",
                                });
                            });
                            row.col(|ui| {
                                ui.label(hit.self_damage.map(|d| d.to_string()).unwrap_or_default());
                            });
                            row.col(|ui| {
                                ui.label(hit.self_kills.map(|k| k.to_string()).unwrap_or_default());
                            });
                            row.col(|ui| {
                                ui.label(hit.self_pr.map(|pr| format!("{pr:.0}")).unwrap_or_default());
                            });
                            row.col(|ui| {
                                let exists = hit.replay_path.exists();
                                let btn = ui.add_enabled(exists, egui::Button::new(t!("ui.search.open")));
                                if !exists {
                                    btn.on_hover_text(t!("ui.search.open_missing"));
                                } else if btn.clicked() {
                                    open_path = Some(hit.replay_path.clone());
                                }
                            });
                        });
                    }
                });
        });

        if let Some(path) = open_path
            && let Some(deps) = self.tab_state.replay_dependencies()
        {
            crate::update_background_task!(
                self.tab_state.background_tasks,
                deps.parse_replay_from_path(path, crate::task::ReplaySource::ManualOpen)
            );
        }
    }
}
