use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

use egui::RichText;
use itertools::Itertools;
use jiff::Timestamp;
use rust_i18n::t;
use wows_replays::types::AccountId;

use crate::app::ToolkitTabViewer;
use crate::task;

use super::PlayerTracker;
use super::SortOrder;
use super::SortedBy;
use super::TimePeriod;
use super::encounter_severity_color;
use super::last_seen_text;
use super::last_seen_timestamp_text;

/// Columns of the historical table, in render order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoricalColumn {
    Clan,
    Player,
    TotalEncounters,
    EncountersInRange,
    LastEncountered,
    Actions,
}

const HISTORICAL_COLUMNS: [HistoricalColumn; 6] = [
    HistoricalColumn::Clan,
    HistoricalColumn::Player,
    HistoricalColumn::TotalEncounters,
    HistoricalColumn::EncountersInRange,
    HistoricalColumn::LastEncountered,
    HistoricalColumn::Actions,
];

const HISTORICAL_ROW_HEIGHT: f32 = 30.0;

/// Accounts to render, filtered by the active time range and name filter, in
/// the order the active sort puts them.
fn visible_rows(tracker: &PlayerTracker) -> Vec<AccountId> {
    let filter_lower = tracker.player_filter.to_ascii_lowercase();
    let sorted_by = tracker.sort_order;
    let tracked_players_by_ts = &tracker.tracked_players_by_time;

    // Filter by the date range
    let player_range: HashSet<_> = if let Some(filter_range) = tracker.filter_time_period.to_date() {
        tracked_players_by_ts
            .iter()
            .filter_map(|(ts, ids)| if *ts > filter_range { Some(ids) } else { None })
            .flatten()
            .cloned()
            .collect()
    } else {
        tracked_players_by_ts.iter().flat_map(|(_ts, ids)| ids).cloned().collect()
    };

    tracker
        .tracked_players
        .iter()
        .filter(|(id, player)| {
            if !tracker.player_filter.is_empty() {
                player_range.contains(id)
                    && (player.clan.to_ascii_lowercase().contains(&filter_lower)
                        || player.last_name.to_ascii_lowercase().contains(&filter_lower)
                        || player.names.iter().any(|name| name.to_ascii_lowercase().contains(&filter_lower)))
            } else {
                player_range.contains(id)
            }
        })
        .sorted_by(|(_ida, playera), (_idb, playerb)| match sorted_by {
            SortedBy::Name(sort_order) => {
                let playera_name = &playera.last_name;
                let playerb_name = &playerb.last_name;

                if sort_order == SortOrder::Asc {
                    playera_name.cmp(playerb_name)
                } else {
                    playerb_name.cmp(playera_name)
                }
            }
            SortedBy::Clan(sort_order) => {
                let playera_clan = &playera.clan;
                let playerb_clan = &playerb.clan;

                if sort_order == SortOrder::Asc {
                    playera_clan.cmp(playerb_clan)
                } else {
                    playerb_clan.cmp(playera_clan)
                }
            }
            SortedBy::LastEncountered(sort_order) => {
                let playera_last = playera.timestamps.last().copied();
                let playerb_last = playerb.timestamps.last().copied();

                if sort_order == SortOrder::Asc {
                    playera_last.cmp(&playerb_last)
                } else {
                    playerb_last.cmp(&playera_last)
                }
            }
            SortedBy::TimesEncountered(sort_order) => {
                let playera_count = playera.timestamps.len();
                let playerb_count = playerb.timestamps.len();

                if sort_order == SortOrder::Asc {
                    playera_count.cmp(&playerb_count)
                } else {
                    playerb_count.cmp(&playera_count)
                }
            }
            SortedBy::TimesEncounteredInTimeRange(sort_order) => {
                let (playera_count, playerb_count) = if let Some(filter_range) = tracker.filter_time_period.to_date() {
                    let playera_count = playera.timestamps.iter().filter(|ts| **ts > filter_range).count();
                    let playerb_count = playerb.timestamps.iter().filter(|ts| **ts > filter_range).count();

                    (playera_count, playerb_count)
                } else {
                    let playera_count = playera.timestamps.len();
                    let playerb_count = playerb.timestamps.len();

                    (playera_count, playerb_count)
                };

                if sort_order == SortOrder::Asc {
                    playera_count.cmp(&playerb_count)
                } else {
                    playerb_count.cmp(&playera_count)
                }
            }
        })
        .map(|(id, _)| *id)
        .collect()
}

/// Header label with the sort arrow appended when this column drives the sort.
fn header_label(key: &str, sorted_by: SortedBy, is_active: bool) -> String {
    let label: String = t!(key).into();
    if is_active { format!("{} {}", label, sorted_by.order().icon()) } else { label }
}

/// Values copied out of one tracked player for a single frame's render. Cheap:
/// egui_table only paints visible rows.
struct RowView {
    account_id: AccountId,
    clan: String,
    last_name: String,
    total_encounters: usize,
    encounters_in_range: usize,
    last_seen: String,
    last_seen_exact: String,
}

struct HistoricalTable<'a> {
    tracker: &'a mut PlayerTracker,
    rows: Vec<AccountId>,
    row_heights: BTreeMap<u64, f32>,
    now: Timestamp,
    filter_range: Option<Timestamp>,
    find_matches_target: Option<AccountId>,
}

impl HistoricalTable<'_> {
    fn row_view(&self, row_nr: u64) -> Option<RowView> {
        let account_id = self.rows.get(row_nr as usize).copied()?;
        let player = self.tracker.tracked_players.get(&account_id)?;

        let total_encounters = player.arena_ids.len();
        let encounters_in_range = match self.filter_range {
            Some(range) => player.timestamps.iter().filter(|ts| **ts > range).count(),
            None => total_encounters,
        };

        Some(RowView {
            account_id,
            clan: player.clan.clone(),
            last_name: player.last_name.clone(),
            total_encounters,
            encounters_in_range,
            last_seen: last_seen_text(player, self.now),
            last_seen_exact: last_seen_timestamp_text(player),
        })
    }

    fn cell_content_ui(&mut self, row_nr: u64, column: HistoricalColumn, ui: &mut egui::Ui) {
        let Some(view) = self.row_view(row_nr) else {
            return;
        };

        match column {
            HistoricalColumn::Clan => {
                ui.label(&view.clan);
            }
            HistoricalColumn::Player => {
                let mut text = RichText::new(&view.last_name);
                if let Some(color) = encounter_severity_color(ui, view.encounters_in_range) {
                    text = text.color(color);
                }
                ui.label(text);
            }
            HistoricalColumn::TotalEncounters => {
                let mut text = RichText::new(view.total_encounters.to_string());
                if let Some(color) = encounter_severity_color(ui, view.encounters_in_range) {
                    text = text.color(color);
                }
                ui.label(text);
            }
            HistoricalColumn::EncountersInRange => {
                let mut text = RichText::new(view.encounters_in_range.to_string());
                if let Some(color) = encounter_severity_color(ui, view.encounters_in_range) {
                    text = text.color(color);
                }
                ui.label(text);
            }
            HistoricalColumn::LastEncountered => {
                ui.label(&view.last_seen).on_hover_text(&view.last_seen_exact);
            }
            HistoricalColumn::Actions => {
                if ui.button(t!("ui.player_tracker.find_matches")).clicked() {
                    self.find_matches_target = Some(view.account_id);
                }
            }
        }
    }
}

impl egui_table::TableDelegate for HistoricalTable<'_> {
    fn header_cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::HeaderCellInfo) {
        let column = HISTORICAL_COLUMNS[cell.group_index];
        let sorted_by = self.tracker.sort_order;
        let (key, target, is_active) = match column {
            HistoricalColumn::Clan => (
                "ui.player_tracker.column.clan",
                SortedBy::Clan(SortOrder::Asc),
                matches!(sorted_by, SortedBy::Clan(_)),
            ),
            HistoricalColumn::Player => (
                "ui.player_tracker.column.player_name",
                SortedBy::Name(SortOrder::Asc),
                matches!(sorted_by, SortedBy::Name(_)),
            ),
            HistoricalColumn::TotalEncounters => (
                "ui.player_tracker.column.total_encounters",
                SortedBy::TimesEncountered(SortOrder::Asc),
                matches!(sorted_by, SortedBy::TimesEncountered(_)),
            ),
            HistoricalColumn::EncountersInRange => (
                "ui.player_tracker.column.encounters_in_range",
                SortedBy::TimesEncounteredInTimeRange(SortOrder::Asc),
                matches!(sorted_by, SortedBy::TimesEncounteredInTimeRange(_)),
            ),
            HistoricalColumn::LastEncountered => (
                "ui.player_tracker.column.last_encountered",
                SortedBy::LastEncountered(SortOrder::Asc),
                matches!(sorted_by, SortedBy::LastEncountered(_)),
            ),
            HistoricalColumn::Actions => return,
        };

        egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 0)).show(ui, |ui| {
            if ui.strong(header_label(key, sorted_by, is_active)).clicked() {
                self.tracker.sort_order.transition_to(target);
                // The row order was built before the table was shown, so the new
                // sort lands on the next frame.
                ui.ctx().request_repaint();
            }
        });
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::CellInfo) {
        if cell.row_nr % 2 == 1 {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().faint_bg_color);
        }
        egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 4)).show(ui, |ui| {
            self.cell_content_ui(cell.row_nr, HISTORICAL_COLUMNS[cell.col_nr], ui);
        });
    }

    fn default_row_height(&self) -> f32 {
        HISTORICAL_ROW_HEIGHT
    }
}

impl ToolkitTabViewer<'_> {
    pub(crate) fn build_historical_sub_tab(&mut self, ui: &mut egui::Ui) {
        // Collected during the table pass and applied after the table (the
        // player-tracker write lock is held while rendering rows, so we can't
        // touch other `self.tab_state` fields from inside a cell body).
        let mut find_matches_target: Option<AccountId> = None;

        // Scoped so the write guard is released before the deferred actions run.
        {
            let mut player_tracker_settings = self.tab_state.player_tracker.write();
            let player_tracker = &mut *player_tracker_settings;
            let now = Timestamp::now();
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui.button(t!("ui.player_tracker.clear_stats")).clicked() {
                        player_tracker.tracked_players.clear();
                        player_tracker.tracked_players_by_time.clear();
                    }

                    let selected = &mut player_tracker.filter_time_period;
                    egui::ComboBox::from_id_salt("player_inspector_time_period_selection")
                        .selected_text(selected.description())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                selected,
                                TimePeriod::LastHour,
                                t!("ui.player_tracker.period.past_hour"),
                            );
                            ui.selectable_value(
                                selected,
                                TimePeriod::LastSixHours,
                                t!("ui.player_tracker.period.past_six_hours"),
                            );
                            ui.selectable_value(selected, TimePeriod::LastDay, t!("ui.player_tracker.period.past_day"));
                            ui.selectable_value(
                                selected,
                                TimePeriod::LastWeek,
                                t!("ui.player_tracker.period.past_week"),
                            );
                            ui.selectable_value(
                                selected,
                                TimePeriod::LastMonth,
                                t!("ui.player_tracker.period.past_month"),
                            );
                            ui.selectable_value(selected, TimePeriod::AllTime, t!("ui.player_tracker.period.all_time"));
                        });
                    ui.label(t!("ui.player_tracker.player_filter"));
                    ui.text_edit_singleline(&mut player_tracker.player_filter);

                    // Never a silent no-op: only enable the button when at least one of the
                    // two population paths (durable index, or on-demand replay re-parse) has
                    // its prerequisites available.
                    let index_path_available =
                        self.tab_state.db_pool.is_some() && self.tab_state.tokio_runtime.is_some();
                    let fallback_path_available =
                        self.tab_state.replay_files.is_some() && self.tab_state.wows_data_map.is_some();
                    let populate_enabled = index_path_available || fallback_path_available;

                    if ui
                        .add_enabled(populate_enabled, egui::Button::new(t!("ui.player_tracker.populate_from_replays")))
                        .clicked()
                    {
                        // Prefer the durable index: it's already parsed and avoids
                        // re-reading every replay from disk. Fall back to the
                        // background re-parse task when the index has nothing yet.
                        let populated_from_index =
                            match (self.tab_state.db_pool.as_ref(), self.tab_state.tokio_runtime.as_ref()) {
                                (Some(pool), Some(rt)) => player_tracker.populate_from_index(pool, rt),
                                _ => false,
                            };

                        if !populated_from_index
                            && let Some(replay_files) = self.tab_state.replay_files.as_ref()
                            && let Some(wows_data_map) = self.tab_state.wows_data_map.as_ref()
                        {
                            crate::update_background_task!(
                                self.tab_state.background_tasks,
                                Some(task::start_populating_player_inspector(
                                    replay_files.keys().cloned().collect(),
                                    wows_data_map.clone(),
                                    Arc::clone(&self.tab_state.player_tracker)
                                ))
                            );
                        }
                    }
                });

                ui.add_space(10.0);

                ui.separator();

                // Everything the delegate needs from the tracker is read before it takes the
                // mutable borrow.
                let rows = visible_rows(player_tracker);
                let row_heights = std::mem::take(&mut player_tracker.historical_row_heights);
                let filter_range = player_tracker.filter_time_period.to_date();

                let mut delegate = HistoricalTable {
                    tracker: player_tracker,
                    rows,
                    row_heights,
                    now,
                    filter_range,
                    find_matches_target: None,
                };

                let columns = vec![
                    egui_table::Column::new(70.0).range(40.0..=200.0).resizable(true),
                    egui_table::Column::new(220.0).range(120.0..=600.0).resizable(true),
                    egui_table::Column::new(110.0).range(60.0..=250.0).resizable(true),
                    egui_table::Column::new(150.0).range(60.0..=300.0).resizable(true),
                    egui_table::Column::new(130.0).range(80.0..=300.0).resizable(true),
                    egui_table::Column::new(70.0).resizable(false),
                ];

                egui_table::Table::new()
                    .id_salt("player_tracker_historical")
                    .num_rows(delegate.rows.len() as u64)
                    .columns(columns)
                    .num_sticky_cols(2)
                    .headers([egui_table::HeaderRow { height: 20.0, groups: Default::default() }])
                    .auto_size_mode(egui_table::AutoSizeMode::Never)
                    .show(ui, &mut delegate);

                find_matches_target = delegate.find_matches_target;
                player_tracker.historical_row_heights = delegate.row_heights;
            });
        }

        if let Some(id) = find_matches_target {
            self.queue_player_search(id);
        }
    }
}
