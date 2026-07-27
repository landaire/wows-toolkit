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

use super::ExpandingColumn;
use super::PlayerTracker;
use super::SortOrder;
use super::SortedBy;
use super::TimePeriod;
use super::TrackedPlayer;
use super::cell_is_in_this_region;
use super::detail_rect;
use super::encounter_severity_color;
use super::encounters_in_range;
use super::expanded_rows;
use super::last_seen_text;
use super::last_seen_timestamp_text;
use super::row_offset;
use super::sort_header_label;

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

/// The Player column. `expanded_width` is what the notes editor gets, less the
/// detail block's own margins, since the block is painted inside this column.
/// A column the user dragged wider than that narrows back to it on expand; see
/// [`ExpandingColumn`] for why the two regimes cannot share a remembered width.
const PLAYER_COLUMN: ExpandingColumn = ExpandingColumn {
    collapsed_width: 220.0,
    expanded_width: 420.0,
    min_width: 120.0,
    max_width: 600.0,
    collapsed_id: "player_tracker_historical_player",
    expanded_id: "player_tracker_historical_player_expanded",
};

/// Accounts to render, filtered by the active time range and name filter, in
/// the order the active sort puts them.
fn visible_rows(tracker: &PlayerTracker) -> Vec<AccountId> {
    let filter_lower = tracker.player_filter.to_ascii_lowercase();
    let sorted_by = tracker.sort_order;
    let tracked_players_by_ts = &tracker.tracked_players_by_time;

    // Resolved once for the whole call: `to_date` reads the clock, so a
    // comparator that called it would not be a fixed total order.
    let filter_range = tracker.filter_time_period.to_date();

    // Filter by the date range
    let player_range: HashSet<_> = if let Some(filter_range) = filter_range {
        tracked_players_by_ts
            .iter()
            .filter_map(|(ts, ids)| if *ts > filter_range { Some(ids) } else { None })
            .flatten()
            .cloned()
            .collect()
    } else {
        tracked_players_by_ts.iter().flat_map(|(_ts, ids)| ids).cloned().collect()
    };

    let mut rows: Vec<(AccountId, &TrackedPlayer)> = tracker
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
        .map(|(id, player)| (*id, player))
        .collect();

    let ids = |rows: Vec<(AccountId, &TrackedPlayer)>| rows.into_iter().map(|(id, _)| id).collect();

    match sorted_by {
        SortedBy::Name(order) => {
            rows.sort_by(|(_, a), (_, b)| order.direct(a.last_name.cmp(&b.last_name)));
            ids(rows)
        }
        SortedBy::Clan(order) => {
            rows.sort_by(|(_, a), (_, b)| order.direct(a.clan.cmp(&b.clan)));
            ids(rows)
        }
        SortedBy::LastEncountered(order) => {
            rows.sort_by(|(_, a), (_, b)| order.direct(a.timestamps.last().cmp(&b.timestamps.last())));
            ids(rows)
        }
        SortedBy::TimesEncountered(order) => {
            rows.sort_by(|(_, a), (_, b)| order.direct(a.timestamps.len().cmp(&b.timestamps.len())));
            ids(rows)
        }
        // The only key that costs a scan of a player's whole history, and the
        // default sort: decorated onto the row so the scan runs once per player
        // instead of once per comparison.
        SortedBy::TimesEncounteredInTimeRange(order) => {
            let mut decorated: Vec<(usize, AccountId)> =
                rows.into_iter().map(|(id, player)| (encounters_in_range(player, filter_range), id)).collect();
            decorated.sort_by(|(a, _), (b, _)| order.direct(a.cmp(b)));
            decorated.into_iter().map(|(_, id)| id).collect()
        }
    }
}

/// Salts the row animations so they do not collide with another table's, which
/// key on the bare row number.
const HISTORICAL_ROW_SALT: &str = "player_tracker_historical_row";

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
    /// Height each open row's detail block adds on top of the default row height,
    /// measured as it is painted and used to lay the next frame out.
    detail_heights: BTreeMap<u64, f32>,
    /// Animation factor per open or still-closing row, derived from
    /// `expanded_players` each frame because `row_top_offset` addresses rows
    /// positionally. Rows contributing no extra height are absent.
    expanded_rows: BTreeMap<u64, f32>,
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
            // A player who reverts to an earlier name lands their current name in
            // `names`; listing it as an alias would only repeat the row's own label.
            aliases: player
                .names
                .iter()
                .filter(|name| name.as_str() != player.last_name.as_str())
                .sorted()
                .cloned()
                .collect(),
            notes: player.notes.clone(),
            total_encounters,
            encounters_in_range,
            last_seen: last_seen_text(player, self.now),
            last_seen_exact: last_seen_timestamp_text(player),
        })
    }

    /// The collapsed content of one cell, pinned to the top of the row.
    fn cell_content_ui(&mut self, row_nr: u64, column: HistoricalColumn, ui: &mut egui::Ui) {
        let Some(view) = self.row_view(row_nr) else {
            return;
        };

        // Absent means the row adds no height this frame: either it is closed, or
        // its collapse animation has already run out.
        let expandedness = self.expanded_rows.get(&row_nr).copied().unwrap_or(0.0);
        let mut toggle_expand = false;

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

                if ui.button(icons::MAGNIFYING_GLASS).on_hover_text(t!("ui.player_tracker.find_matches")).clicked() {
                    self.find_matches_target = Some(view.account_id);
                }
            }
        });

        if toggle_expand {
            // `remove` reports whether it was there, which is the toggle. The row's
            // animation factor only picks the change up on the next frame.
            if !self.tracker.expanded_players.remove(&view.account_id) {
                self.tracker.expanded_players.insert(view.account_id);
            }
        }
    }

    /// The detail block of an open row, filling the Player cell below the
    /// collapsed content. Returns its height, which is what the row is stretched
    /// by. The height is the block's natural one even while the row is part-way
    /// through its animation and only a slice of the block is on screen, so the
    /// row settles at exactly this height once the animation completes.
    fn detail_ui(&mut self, view: &RowView, ui: &mut egui::Ui) -> f32 {
        let inner_response = egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            // egui_table runs the whole table on `TextWrapMode::Extend`, which suits
            // single-line cells but would push a long alias list out past the
            // column's edge. Wrapping keeps the block inside its column and folds
            // the extra lines into the height the row is stretched by.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);

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
        });

        inner_response.response.rect.height()
    }

    /// Paints an open row's detail block into the part of the Player cell left
    /// below the collapsed content, and records the height the row is stretched
    /// by. `cell_rect` supplies both the column's width and the row's full band,
    /// which is what keeps the block inside its column separators.
    fn detail_block_ui(&mut self, row_nr: u64, ui: &mut egui::Ui, cell_rect: egui::Rect) {
        // Absent means the row adds no height this frame: either it is closed, or
        // its collapse animation has already run out.
        let expandedness = self.expanded_rows.get(&row_nr).copied().unwrap_or(0.0);
        if expandedness <= 0.0 {
            return;
        }

        if !cell_is_in_this_region(ui.clip_rect(), cell_rect) {
            return;
        }

        let Some(view) = self.row_view(row_nr) else {
            return;
        };
        let Some(detail_rect) = detail_rect(cell_rect, HISTORICAL_ROW_HEIGHT) else {
            return;
        };

        let mut detail_ui =
            ui.new_child(egui::UiBuilder::new().max_rect(detail_rect).layout(egui::Layout::top_down(egui::Align::Min)));

        // Deliberately not advancing the cell's cursor: the block is measured for
        // the row height alone and must not feed egui_table's per-column sizing,
        // which would ratchet the column wider on every frame it is open.
        let height = self.detail_ui(&view, &mut detail_ui);
        self.detail_heights.insert(row_nr, height);
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
            if ui.strong(sort_header_label(key, sorted_by.order(), is_active)).clicked() {
                self.tracker.sort_order.transition_to(target);
                // The row order was built before the table was shown, so the new
                // sort lands on the next frame.
                ui.ctx().request_repaint();
            }
        });
    }

    fn row_ui(&mut self, ui: &mut egui::Ui, row_nr: u64) {
        // Striping belongs here rather than in `cell_ui`, which runs afterwards and
        // would paint over the cell contents.
        if row_nr % 2 == 1 {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().faint_bg_color);
        }
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::CellInfo) {
        let column = HISTORICAL_COLUMNS[cell.col_nr];
        let cell_rect = ui.max_rect();

        // Cells are handed the whole row, detail block included. Pin the collapsed
        // content to the top of it, or a vertically centred layout would drift it
        // into the middle of an open row.
        let mut content_rect = cell_rect;
        content_rect.max.y = content_rect.min.y + HISTORICAL_ROW_HEIGHT;
        let mut content_ui = ui.new_child(
            egui::UiBuilder::new().max_rect(content_rect).layout(egui::Layout::left_to_right(egui::Align::Center)),
        );

        egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 4)).show(&mut content_ui, |ui| {
            self.cell_content_ui(cell.row_nr, column, ui);
        });

        // A child `Ui` does not grow its parent, and egui_table sizes columns from
        // the cell's `min_size`. Without this every column would fit to nothing on
        // the sizing pass and land on its minimum width.
        ui.advance_cursor_after_rect(content_ui.min_rect());

        if column == HistoricalColumn::Player {
            self.detail_block_ui(cell.row_nr, ui, cell_rect);
        }
    }

    fn default_row_height(&self) -> f32 {
        HISTORICAL_ROW_HEIGHT
    }

    fn row_top_offset(&self, _ctx: &egui::Context, _table_id: egui::Id, row_nr: u64) -> f32 {
        row_offset(&self.detail_heights, &self.expanded_rows, row_nr, HISTORICAL_ROW_HEIGHT)
    }
}

impl ToolkitTabViewer<'_> {
    pub(crate) fn build_historical_sub_tab(&mut self, ui: &mut egui::Ui) {
        // Collected during the table pass and applied after the table (the
        // player-tracker write lock is held while rendering rows, so we can't
        // touch other `self.tab_state` fields from inside a cell body).
        let mut find_matches_target: Option<AccountId> = None;
        let mut copy_text: Option<String> = None;

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
                        player_tracker.note_encounters_changed();
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
                let expanded_rows =
                    expanded_rows(ui.ctx(), HISTORICAL_ROW_SALT, &rows, &player_tracker.expanded_players);
                let detail_heights = std::mem::take(&mut player_tracker.historical_detail_heights);
                let filter_range = player_tracker.filter_time_period.to_date();
                // A row still animating shut keeps its factor, so the column stays
                // wide for as long as any of the block is on screen.
                let any_expanded = !expanded_rows.is_empty();

                let mut delegate = HistoricalTable {
                    tracker: player_tracker,
                    rows,
                    detail_heights,
                    expanded_rows,
                    now,
                    filter_range,
                    find_matches_target: None,
                    copy_text: None,
                };

                let columns = vec![
                    egui_table::Column::new(70.0).range(40.0..=200.0).resizable(true),
                    PLAYER_COLUMN.column(any_expanded),
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
                copy_text = delegate.copy_text;
                player_tracker.historical_detail_heights = delegate.detail_heights;
            });
        }

        if let Some(text) = copy_text {
            self.tab_state.toasts.lock().success(t!("ui.player_tracker.copied_wg_id", id = &text).to_string());
            ui.ctx().copy_text(text);
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

    /// A row's first animation step lands on its target immediately, so a fresh
    /// context yields exactly 1.0 for an open row and drops the closed ones.
    #[test]
    fn an_open_row_follows_its_account_when_the_order_changes() {
        let open = HashSet::from([AccountId(3)]);

        let ascending = [AccountId(1), AccountId(2), AccountId(3)];
        assert_eq!(
            expanded_rows(&egui::Context::default(), HISTORICAL_ROW_SALT, &ascending, &open),
            BTreeMap::from([(2, 1.0)]),
            "the open account is the last row before the re-sort, and no closed row is carried"
        );

        let descending = [AccountId(3), AccountId(2), AccountId(1)];
        assert_eq!(
            expanded_rows(&egui::Context::default(), HISTORICAL_ROW_SALT, &descending, &open),
            BTreeMap::from([(0, 1.0)]),
            "reversing the order moves the open state with the account, not the index"
        );

        assert!(
            expanded_rows(&egui::Context::default(), HISTORICAL_ROW_SALT, &[AccountId(1), AccountId(2)], &open)
                .is_empty(),
            "filtering the open account out leaves no row open"
        );
    }

    #[test]
    fn the_row_offset_stacks_only_the_expanded_rows_above_the_queried_one() {
        let heights = BTreeMap::from([(0, 100.0), (1, 100.0), (2, 100.0), (3, 100.0)]);

        assert_eq!(
            row_offset(&heights, &BTreeMap::new(), 2, HISTORICAL_ROW_HEIGHT),
            2.0 * HISTORICAL_ROW_HEIGHT,
            "with nothing expanded every row is exactly one default height tall"
        );

        assert_eq!(
            row_offset(&heights, &BTreeMap::from([(0, 1.0)]), 2, HISTORICAL_ROW_HEIGHT),
            2.0 * HISTORICAL_ROW_HEIGHT + 100.0,
            "an expanded row above the queried one pushes it down by its measured height"
        );

        assert_eq!(
            row_offset(&heights, &BTreeMap::from([(2, 1.0)]), 2, HISTORICAL_ROW_HEIGHT),
            2.0 * HISTORICAL_ROW_HEIGHT,
            "the queried row's own expansion does not move its top edge"
        );

        assert_eq!(
            row_offset(&heights, &BTreeMap::from([(3, 1.0)]), 2, HISTORICAL_ROW_HEIGHT),
            2.0 * HISTORICAL_ROW_HEIGHT,
            "an expanded row below the queried one does not move it"
        );

        assert_eq!(
            row_offset(&heights, &BTreeMap::from([(0, 0.25)]), 2, HISTORICAL_ROW_HEIGHT),
            2.0 * HISTORICAL_ROW_HEIGHT + 25.0,
            "a part-way animation contributes its fraction of the measured height"
        );

        assert_eq!(
            row_offset(&BTreeMap::new(), &BTreeMap::from([(0, 1.0)]), 2, HISTORICAL_ROW_HEIGHT),
            2.0 * HISTORICAL_ROW_HEIGHT,
            "a row expanded but never yet painted contributes nothing until it is measured"
        );
    }

    /// The identity the whole layout rests on: a row is exactly as tall as the
    /// collapsed content plus however much of its detail block is showing. If this
    /// drifts, rows overlap or gap and nothing else catches it.
    #[test]
    fn a_rows_height_is_the_default_plus_its_animated_detail_block() {
        let heights = BTreeMap::from([(1, 80.0)]);
        let height_of_row_1 = |expandedness: &BTreeMap<u64, f32>| {
            row_offset(&heights, expandedness, 2, HISTORICAL_ROW_HEIGHT)
                - row_offset(&heights, expandedness, 1, HISTORICAL_ROW_HEIGHT)
        };

        assert_eq!(
            height_of_row_1(&BTreeMap::new()),
            HISTORICAL_ROW_HEIGHT,
            "a closed row is exactly the default height"
        );

        // 0.25 and 1.0 are exact in binary, so these compare without a tolerance.
        for factor in [0.25_f32, 1.0] {
            assert_eq!(
                height_of_row_1(&BTreeMap::from([(1, factor)])),
                HISTORICAL_ROW_HEIGHT + factor * 80.0,
                "a row {factor} of the way open is the default height plus that share of its block"
            );
        }
    }

    /// The historical block lives inside the Player cell, so it spans exactly
    /// that column and cannot reach across a column separator.
    #[test]
    fn the_detail_block_spans_the_player_cell_and_no_more() {
        // What egui_table hands a Player cell of an open row: the column's x
        // range, and the row's whole band including the block.
        let cell = egui::Rect::from_min_max(egui::pos2(70.0, 100.0), egui::pos2(490.0, 210.0));

        let rect = detail_rect(cell, HISTORICAL_ROW_HEIGHT).expect("a cell with width yields a rect");
        assert_eq!(rect.x_range(), cell.x_range(), "the block takes the column's width, and no more");
        assert_eq!(rect.top(), cell.top() + HISTORICAL_ROW_HEIGHT, "the block starts below the collapsed content");
        assert_eq!(rect.bottom(), cell.bottom(), "the block ends with the row, which is what the height feeds");
    }

    #[test]
    fn the_detail_rect_is_none_when_the_cell_has_no_width() {
        let collapsed = egui::Rect::from_min_max(egui::pos2(290.0, 100.0), egui::pos2(290.0, 210.0));
        assert_eq!(detail_rect(collapsed, HISTORICAL_ROW_HEIGHT), None, "a zero-width cell yields no block");

        let inverted = egui::Rect::from_min_max(egui::pos2(290.0, 100.0), egui::pos2(250.0, 210.0));
        assert_eq!(detail_rect(inverted, HISTORICAL_ROW_HEIGHT), None, "a negative-width cell yields no block");
    }

    /// The block is as wide as the column, so an open row has to widen it or the
    /// notes editor is unusable. egui_table only ever grows a remembered width,
    /// so the two regimes must not share a column id.
    #[test]
    fn the_player_column_widens_while_a_row_is_open() {
        let collapsed = PLAYER_COLUMN.column(false);
        let expanded = PLAYER_COLUMN.column(true);

        assert_eq!(collapsed.current, PLAYER_COLUMN.collapsed_width);
        assert_eq!(expanded.current, PLAYER_COLUMN.expanded_width);
        assert!(
            collapsed.range.max >= expanded.range.min,
            "the expanded width has to be reachable by the collapsed range too, or a drag cannot follow it"
        );
        assert!(
            expanded.range.min > collapsed.current,
            "an open row must force the column past the width it sits at when closed"
        );
        assert_ne!(
            collapsed.id_for(1),
            expanded.id_for(1),
            "sharing an id would leave the column expanded for good once a row had been opened"
        );
    }
}
