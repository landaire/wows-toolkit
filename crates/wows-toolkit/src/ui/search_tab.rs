//! The dockable Search tab: a structured MatchFilter bar over a results table
//! backed by the replay index. Results table is added in Task 3.

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
    pub fn build_search_tab(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            // Outcome selector: None (Any) / Win / Loss / Draw.
            let filter = &mut self.tab_state.search_tab.filter;
            egui::ComboBox::from_id_salt("search_outcome")
                .selected_text(match filter.outcome {
                    None => "Any result",
                    Some(MatchOutcome::Win) => "Win",
                    Some(MatchOutcome::Loss) => "Loss",
                    Some(MatchOutcome::Draw) => "Draw",
                    Some(MatchOutcome::Unknown) => "Unknown",
                })
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut filter.outcome, None, "Any result").changed();
                    changed |= ui.selectable_value(&mut filter.outcome, Some(MatchOutcome::Win), "Win").changed();
                    changed |= ui.selectable_value(&mut filter.outcome, Some(MatchOutcome::Loss), "Loss").changed();
                    changed |= ui.selectable_value(&mut filter.outcome, Some(MatchOutcome::Draw), "Draw").changed();
                });

            // Map free-text filter.
            let mut map = filter.map.clone().unwrap_or_default();
            ui.label("Map:");
            if ui.text_edit_singleline(&mut map).changed() {
                filter.map = (!map.is_empty()).then_some(map);
                changed = true;
            }

            // Survived toggle (None = any).
            ui.label("Survived:");
            let mut survived = filter.self_survived;
            egui::ComboBox::from_id_salt("search_survived")
                .selected_text(match survived {
                    None => "Any",
                    Some(true) => "Survived",
                    Some(false) => "Died",
                })
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut survived, None, "Any").changed();
                    changed |= ui.selectable_value(&mut survived, Some(true), "Survived").changed();
                    changed |= ui.selectable_value(&mut survived, Some(false), "Died").changed();
                });
            filter.self_survived = survived;

            if ui.button("Clear filters").clicked() {
                *filter = MatchFilter::default();
                changed = true;
            }
        });

        if changed {
            self.tab_state.search_tab.dirty = true;
        }

        ui.separator();
        // Results table lands in Task 3.
        ui.label(format!("{} match(es)", self.tab_state.search_tab.results.len()));
    }
}
