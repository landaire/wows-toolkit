use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

use egui::RichText;
use itertools::Itertools;
use jiff::Timestamp;
use rust_i18n::t;
use wows_replays::types::AccountId;

use crate::app::ToolkitTabViewer;
use crate::icons;
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

/// Salted so the animation does not collide with the replay inspector's
/// row animations, which key on the bare row number.
fn row_animation_id(row_nr: u64) -> egui::Id {
    egui::Id::new(("player_tracker_historical_row", row_nr))
}

/// Positional expansion state for `rows`, projected from the account-keyed set
/// so that a re-sort or re-filter carries an open row to the account's new
/// index instead of leaving it open on whoever landed there.
fn expanded_rows(rows: &[AccountId], expanded_players: &HashSet<AccountId>) -> BTreeMap<u64, bool> {
    rows.iter().enumerate().map(|(index, id)| (index as u64, expanded_players.contains(id))).collect()
}

/// Values copied out of one tracked player for a single frame's render. Cheap:
/// egui_table only paints visible rows.
struct RowView {
    account_id: AccountId,
    clan: String,
    last_name: String,
    aliases: Vec<String>,
    notes: String,
    total_encounters: usize,
    encounters_in_range: usize,
    last_seen: String,
    last_seen_exact: String,
}

struct HistoricalTable<'a> {
    tracker: &'a mut PlayerTracker,
    rows: Vec<AccountId>,
    row_heights: BTreeMap<u64, f32>,
    /// Per-row expansion derived from `expanded_players` each frame, because
    /// `row_top_offset` addresses rows positionally.
    expanded_rows: BTreeMap<u64, bool>,
    now: Timestamp,
    filter_range: Option<Timestamp>,
    find_matches_target: Option<AccountId>,
    copy_text: Option<String>,
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
            aliases: player.names.iter().sorted().cloned().collect(),
            notes: player.notes.clone(),
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

        let is_expanded = self.expanded_rows.get(&row_nr).copied().unwrap_or_default();
        let expandedness = ui.ctx().animate_bool(row_animation_id(row_nr), is_expanded);
        let mut toggle_expand = false;

        let inner_response = ui.vertical(|ui| {
            ui.horizontal(|ui| match column {
                HistoricalColumn::Clan => {
                    ui.label(&view.clan);
                }
                HistoricalColumn::Player => {
                    let (_, response) = ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::click());
                    egui::collapsing_header::paint_default_icon(ui, expandedness, &response);
                    if response.clicked() {
                        toggle_expand = true;
                    }

                    let mut name = RichText::new(&view.last_name);
                    if let Some(color) = encounter_severity_color(ui, view.encounters_in_range) {
                        name = name.color(color);
                    }
                    ui.label(name);

                    let account_text = view.account_id.to_string();
                    if ui
                        .add(egui::Label::new(RichText::new(&account_text).small().weak()).sense(egui::Sense::click()))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(t!("ui.player_tracker.copy_wg_id"))
                        .clicked()
                    {
                        self.copy_text = Some(account_text);
                    }

                    if !view.aliases.is_empty() {
                        ui.label(icons::USERS_THREE)
                            .on_hover_text(t!("ui.player_tracker.aliases_hover", names = view.aliases.join(", ")));
                    }
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
                    if !view.notes.is_empty()
                        && ui
                            .add(egui::Label::new(icons::NOTE_PENCIL).sense(egui::Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(&view.notes)
                            .clicked()
                    {
                        toggle_expand = true;
                    }

                    if ui.button(icons::MAGNIFYING_GLASS).on_hover_text(t!("ui.player_tracker.find_matches")).clicked()
                    {
                        self.find_matches_target = Some(view.account_id);
                    }
                }
            });

            // Only the Player column paints the detail block, so it is not repeated
            // once per column. Every other column contributes its collapsed height.
            if 0.0 < expandedness && matches!(column, HistoricalColumn::Player) {
                ui.add_space(4.0);
                if !view.aliases.is_empty() {
                    ui.label(t!("ui.player_tracker.aliases_hover", names = view.aliases.join(", ")));
                }
                ui.label(t!("ui.player_tracker.last_encountered_exact", timestamp = &view.last_seen_exact));
                ui.label(t!("ui.player_tracker.arena_count", count = view.total_encounters));
                ui.add_space(4.0);
                ui.label(t!("ui.player_tracker.notes_hint"));

                // `view` is a snapshot, so the editor takes its own mutable borrow here.
                if let Some(player) = self.tracker.tracked_players.get_mut(&view.account_id) {
                    ui.add(egui::TextEdit::multiline(&mut player.notes).desired_width(f32::INFINITY).desired_rows(3));
                }
            }
        });

        if toggle_expand {
            // `remove` reports whether it was there, which is the toggle.
            if !self.tracker.expanded_players.remove(&view.account_id) {
                self.tracker.expanded_players.insert(view.account_id);
            }
            let now_expanded = self.tracker.expanded_players.contains(&view.account_id);
            self.expanded_rows.insert(row_nr, now_expanded);
            // Force a re-measure at the new height.
            self.row_heights.remove(&row_nr);
        }

        let cell_height = inner_response.response.rect.height();
        // A row that has never been painted has no measured height yet, so seed it
        // with what this cell just measured and only ever grow it afterwards.
        let previous_height = self.row_heights.entry(row_nr).or_insert(cell_height);
        if *previous_height < cell_height {
            *previous_height = cell_height;
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

    fn row_top_offset(&self, ctx: &egui::Context, _table_id: egui::Id, row_nr: u64) -> f32 {
        self.expanded_rows
            .range(0..row_nr)
            .map(|(expanded_row_nr, expanded)| {
                let how_expanded = ctx.animate_bool(row_animation_id(*expanded_row_nr), *expanded);
                // A row that has never been painted has no measured height yet;
                // contributing zero offset is right until it is measured.
                how_expanded * self.row_heights.get(expanded_row_nr).copied().unwrap_or(0.0)
            })
            .sum::<f32>()
            + row_nr as f32 * HISTORICAL_ROW_HEIGHT
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
                let expanded_rows = expanded_rows(&rows, &player_tracker.expanded_players);
                let row_heights = std::mem::take(&mut player_tracker.historical_row_heights);
                let filter_range = player_tracker.filter_time_period.to_date();

                let mut delegate = HistoricalTable {
                    tracker: player_tracker,
                    rows,
                    row_heights,
                    expanded_rows,
                    now,
                    filter_range,
                    find_matches_target: None,
                    copy_text: None,
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
                if let Some(text) = delegate.copy_text {
                    ui.ctx().copy_text(text);
                }
                player_tracker.historical_row_heights = delegate.row_heights;
            });
        }

        if let Some(id) = find_matches_target {
            self.queue_player_search(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::ToSpan;

    use super::*;
    use crate::ui::player_tracker::TrackedPlayer;

    fn player(id: i64, clan: &str, last_name: &str, aliases: &[&str], timestamps: &[Timestamp]) -> TrackedPlayer {
        TrackedPlayer {
            last_name: last_name.to_string(),
            db_id: AccountId(id),
            names: aliases.iter().map(|name| name.to_string()).collect(),
            clan_id: 0,
            clan: clan.to_string(),
            timestamps: timestamps.iter().copied().collect(),
            arena_ids: Default::default(),
            notes: String::new(),
        }
    }

    /// A tracker holding `players`, with `tracked_players_by_time` derived from
    /// their timestamps the way the live and index ingest paths build it. Without
    /// that index every player falls outside `player_range` and nothing renders.
    fn tracker(period: TimePeriod, sort_order: SortedBy, players: Vec<TrackedPlayer>) -> PlayerTracker {
        let mut tracker = PlayerTracker { filter_time_period: period, sort_order, ..Default::default() };
        for player in players {
            let id = player.db_id;
            for ts in &player.timestamps {
                tracker.tracked_players_by_time.entry(*ts).or_default().push(id);
            }
            tracker.tracked_players.insert(id, player);
        }
        tracker
    }

    /// Three players whose clan, name, last-encounter and encounter-count keys
    /// each produce a different order, so a comparator wired into the wrong
    /// `SortedBy` arm cannot pass. Every key is distinct across the three, so no
    /// tie is left to `HashMap`'s iteration order.
    ///
    /// Ascending: clan `[1, 2, 3]`, name `[2, 3, 1]`, last encountered
    /// `[3, 1, 2]`, times encountered `[2, 1, 3]`.
    fn discriminating_tracker(sort_order: SortedBy) -> PlayerTracker {
        let t0 = Timestamp::from_second(1_700_000_000).expect("fixture timestamp is in range");
        tracker(
            TimePeriod::AllTime,
            sort_order,
            vec![
                player(1, "AAA", "cyd", &[], &[t0 + 10.seconds(), t0 + 11.seconds()]),
                player(2, "BBB", "ana", &[], &[t0 + 20.seconds()]),
                player(3, "CCC", "bob", &[], &[t0 + 1.second(), t0 + 2.seconds(), t0 + 3.seconds()]),
            ],
        )
    }

    #[test]
    fn clan_sort_orders_by_clan_tag_in_both_directions() {
        let asc = discriminating_tracker(SortedBy::Clan(SortOrder::Asc));
        assert_eq!(visible_rows(&asc), vec![AccountId(1), AccountId(2), AccountId(3)]);

        let desc = discriminating_tracker(SortedBy::Clan(SortOrder::Desc));
        assert_eq!(visible_rows(&desc), vec![AccountId(3), AccountId(2), AccountId(1)]);
    }

    #[test]
    fn name_sort_orders_by_current_name_in_both_directions() {
        let asc = discriminating_tracker(SortedBy::Name(SortOrder::Asc));
        assert_eq!(visible_rows(&asc), vec![AccountId(2), AccountId(3), AccountId(1)]);

        let desc = discriminating_tracker(SortedBy::Name(SortOrder::Desc));
        assert_eq!(visible_rows(&desc), vec![AccountId(1), AccountId(3), AccountId(2)]);
    }

    #[test]
    fn last_encountered_sort_orders_by_most_recent_timestamp_in_both_directions() {
        let asc = discriminating_tracker(SortedBy::LastEncountered(SortOrder::Asc));
        assert_eq!(visible_rows(&asc), vec![AccountId(3), AccountId(1), AccountId(2)]);

        let desc = discriminating_tracker(SortedBy::LastEncountered(SortOrder::Desc));
        assert_eq!(visible_rows(&desc), vec![AccountId(2), AccountId(1), AccountId(3)]);
    }

    #[test]
    fn times_encountered_sort_orders_by_encounter_count_in_both_directions() {
        let asc = discriminating_tracker(SortedBy::TimesEncountered(SortOrder::Asc));
        assert_eq!(visible_rows(&asc), vec![AccountId(2), AccountId(1), AccountId(3)]);

        let desc = discriminating_tracker(SortedBy::TimesEncountered(SortOrder::Desc));
        assert_eq!(visible_rows(&desc), vec![AccountId(3), AccountId(1), AccountId(2)]);
    }

    #[test]
    fn in_range_sort_falls_back_to_the_total_count_over_all_time() {
        let asc = discriminating_tracker(SortedBy::TimesEncounteredInTimeRange(SortOrder::Asc));
        assert_eq!(visible_rows(&asc), vec![AccountId(2), AccountId(1), AccountId(3)]);

        let desc = discriminating_tracker(SortedBy::TimesEncounteredInTimeRange(SortOrder::Desc));
        assert_eq!(visible_rows(&desc), vec![AccountId(3), AccountId(1), AccountId(2)]);
    }

    #[test]
    fn in_range_sort_counts_only_encounters_inside_the_period() {
        let now = Timestamp::now();
        // Player 1 has the larger total but the smaller in-range count, so the
        // two encounter sorts must disagree. This arm re-derives the range
        // itself rather than reusing the one the row filter applied.
        let players = || {
            vec![
                player(1, "", "bulk", &[], &[now - 30.hours(), now - 29.hours(), now - 28.hours(), now - 2.hours()]),
                player(2, "", "recent", &[], &[now - 3.hours(), now - 2.hours(), now - 1.hour()]),
            ]
        };

        let by_total = tracker(TimePeriod::LastDay, SortedBy::TimesEncountered(SortOrder::Asc), players());
        assert_eq!(visible_rows(&by_total), vec![AccountId(2), AccountId(1)]);

        let by_range = tracker(TimePeriod::LastDay, SortedBy::TimesEncounteredInTimeRange(SortOrder::Asc), players());
        assert_eq!(visible_rows(&by_range), vec![AccountId(1), AccountId(2)]);

        let by_range_desc =
            tracker(TimePeriod::LastDay, SortedBy::TimesEncounteredInTimeRange(SortOrder::Desc), players());
        assert_eq!(visible_rows(&by_range_desc), vec![AccountId(2), AccountId(1)]);
    }

    #[test]
    fn the_time_period_excludes_players_not_seen_inside_it() {
        let now = Timestamp::now();
        let players = || {
            vec![player(1, "", "inside", &[], &[now - 1.hour()]), player(2, "", "outside", &[], &[now - 48.hours()])]
        };

        let last_day = tracker(TimePeriod::LastDay, SortedBy::Name(SortOrder::Asc), players());
        assert_eq!(visible_rows(&last_day), vec![AccountId(1)]);

        let all_time = tracker(TimePeriod::AllTime, SortedBy::Name(SortOrder::Asc), players());
        assert_eq!(visible_rows(&all_time), vec![AccountId(1), AccountId(2)]);
    }

    #[test]
    fn the_name_filter_matches_clan_current_name_and_aliases_case_insensitively() {
        let base = Timestamp::from_second(1_700_000_000).expect("fixture timestamp is in range");
        let filtered = |filter: &str| {
            let mut tracker = tracker(
                TimePeriod::AllTime,
                SortedBy::Name(SortOrder::Asc),
                vec![
                    player(1, "ZULU", "helm", &[], &[base]),
                    player(2, "", "Tirpitz", &[], &[base]),
                    player(3, "", "keel", &["OldHandle"], &[base]),
                    player(4, "", "rudder", &[], &[base]),
                ],
            );
            tracker.player_filter = filter.to_string();
            visible_rows(&tracker)
        };

        assert_eq!(filtered("zul"), vec![AccountId(1)], "matches on clan");
        assert_eq!(filtered("TIRP"), vec![AccountId(2)], "matches on current name, ignoring case");
        assert_eq!(filtered("oldhandle"), vec![AccountId(3)], "matches on an alias, ignoring case");
        assert!(filtered("zzzz").is_empty(), "a filter matching nothing yields no rows");
    }

    #[test]
    fn an_open_row_follows_its_account_when_the_order_changes() {
        let open = HashSet::from([AccountId(3)]);

        let ascending = [AccountId(1), AccountId(2), AccountId(3)];
        assert_eq!(
            expanded_rows(&ascending, &open),
            BTreeMap::from([(0, false), (1, false), (2, true)]),
            "the open account is the last row before the re-sort"
        );

        let descending = [AccountId(3), AccountId(2), AccountId(1)];
        assert_eq!(
            expanded_rows(&descending, &open),
            BTreeMap::from([(0, true), (1, false), (2, false)]),
            "reversing the order moves the open state with the account, not the index"
        );

        assert!(
            expanded_rows(&[AccountId(1), AccountId(2)], &open).values().all(|expanded| !expanded),
            "filtering the open account out leaves no row open"
        );
    }
}
