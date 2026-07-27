use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

use egui::RichText;
use jiff::Timestamp;
use rust_i18n::t;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;

use crate::app::ToolkitTabViewer;
use crate::db::index::rows::ClanCorrection;
use crate::icons;

use super::PlayerTracker;
use super::SortOrder;
use super::detail_rect;
use super::encounter_severity_color;
use super::exact_timestamp_text;
use super::expanded_rows;
use super::model::TrackedPlayer;
use super::relative_age_text;
use super::row_offset;
use super::sort_header_label;

/// One clan's encounter aggregates.
#[derive(Debug, Clone)]
pub(crate) struct ClanRow {
    pub clan: String,
    /// Accounts with at least one encounter attributed here, with the match
    /// count each contributed. Drives the expanded member list.
    pub members: Vec<(AccountId, usize)>,
    pub matches: usize,
    pub matches_in_range: usize,
    pub sightings: usize,
    pub sightings_in_range: usize,
    pub last_seen: Timestamp,
}

/// The whole breakdown plus how much of it carries exact attribution.
#[derive(Debug, Clone)]
pub(crate) struct ClanBreakdown {
    /// Tracked-player count this was built against, so new history invalidates it.
    pub tracked_count: usize,
    pub rows: Vec<ClanRow>,
    /// Encounters whose arena the index covers, so their clan label is the clan
    /// the player was actually in.
    pub exact_encounters: usize,
    pub total_encounters: usize,
}

#[derive(Default)]
struct ClanAccumulator {
    members: HashMap<AccountId, usize>,
    arenas: HashSet<ArenaId>,
    range_timestamps: HashSet<Timestamp>,
    sightings: usize,
    sightings_in_range: usize,
    last_seen: Option<Timestamp>,
}

/// Aggregate tracked encounters by clan.
///
/// `index_latest_clan` is the index's latest clan per account, which wins over
/// the tracker's when the index knows the account. `corrections` are the roster
/// rows whose clan at the time differed from that latest clan. `indexed_arenas`
/// is only used to report attribution coverage.
///
/// Matches are counted by distinct arena and in-range matches by distinct
/// timestamp. Both keys are exact on their own, which is why the tracker's
/// unpaired `arena_ids` and `timestamps` sets need no reconciliation: every
/// player in a battle shares that battle's timestamp.
pub(crate) fn build_clan_breakdown(
    tracked: &HashMap<AccountId, TrackedPlayer>,
    index_latest_clan: &HashMap<AccountId, String>,
    corrections: &[ClanCorrection],
    indexed_arenas: &HashSet<ArenaId>,
    since: Option<Timestamp>,
) -> ClanBreakdown {
    let mut by_arena: HashMap<(AccountId, ArenaId), &str> = HashMap::new();
    let mut by_timestamp: HashMap<(AccountId, Timestamp), &str> = HashMap::new();
    for correction in corrections {
        by_arena.insert((correction.account_id, correction.arena_id), &correction.clan);
        by_timestamp.insert((correction.account_id, correction.timestamp), &correction.clan);
    }

    let mut clans: HashMap<String, ClanAccumulator> = HashMap::new();
    let mut exact_encounters = 0;
    let mut total_encounters = 0;

    for (account_id, player) in tracked {
        // The tracker holds one clan per player, the latest it saw. The index's
        // latest is fresher wherever the index knows the account at all.
        let baseline = index_latest_clan.get(account_id).map(String::as_str).unwrap_or(player.clan.as_str());

        for arena_id in &player.arena_ids {
            total_encounters += 1;
            if indexed_arenas.contains(arena_id) {
                exact_encounters += 1;
            }

            let clan = by_arena.get(&(*account_id, *arena_id)).copied().unwrap_or(baseline);
            if clan.is_empty() {
                continue;
            }
            let entry = clans.entry(clan.to_string()).or_default();
            entry.arenas.insert(*arena_id);
            entry.sightings += 1;
            *entry.members.entry(*account_id).or_default() += 1;
        }

        for timestamp in &player.timestamps {
            let clan = by_timestamp.get(&(*account_id, *timestamp)).copied().unwrap_or(baseline);
            if clan.is_empty() {
                continue;
            }
            let entry = clans.entry(clan.to_string()).or_default();
            entry.last_seen = Some(entry.last_seen.map_or(*timestamp, |seen| seen.max(*timestamp)));

            if since.is_none_or(|since| *timestamp > since) {
                entry.range_timestamps.insert(*timestamp);
                entry.sightings_in_range += 1;
            }
        }
    }

    let mut rows: Vec<ClanRow> = clans
        .into_iter()
        .filter_map(|(clan, acc)| {
            // A clan only reached here through an encounter, so `last_seen` is
            // set unless the arena and timestamp passes disagreed on the label,
            // which a correction to a different clan can cause. Drop those
            // rather than invent a timestamp.
            let last_seen = acc.last_seen?;
            let mut members: Vec<(AccountId, usize)> = acc.members.into_iter().collect();
            members.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.raw().cmp(&b.0.raw())));

            Some(ClanRow {
                clan,
                members,
                matches: acc.arenas.len(),
                matches_in_range: acc.range_timestamps.len(),
                sightings: acc.sightings,
                sightings_in_range: acc.sightings_in_range,
                last_seen,
            })
        })
        .collect();

    rows.sort_by(|a, b| b.matches.cmp(&a.matches).then_with(|| a.clan.cmp(&b.clan)));

    ClanBreakdown { tracked_count: tracked.len(), rows, exact_encounters, total_encounters }
}

/// Columns of the clans table, in render order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClanColumn {
    Clan,
    Members,
    Matches,
    MatchesInRange,
    Sightings,
    LastSeen,
    Actions,
}

const CLAN_COLUMNS: [ClanColumn; 7] = [
    ClanColumn::Clan,
    ClanColumn::Members,
    ClanColumn::Matches,
    ClanColumn::MatchesInRange,
    ClanColumn::Sightings,
    ClanColumn::LastSeen,
    ClanColumn::Actions,
];

const CLAN_ROW_HEIGHT: f32 = 30.0;

/// Salts the row animations so they do not collide with another table's, which
/// key on the bare row number.
const CLAN_ROW_SALT: &str = "player_tracker_clan_row";

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum ClanSortedBy {
    Clan(SortOrder),
    Members(SortOrder),
    Matches(SortOrder),
    MatchesInRange(SortOrder),
    Sightings(SortOrder),
    LastSeen(SortOrder),
}

impl Default for ClanSortedBy {
    fn default() -> Self {
        ClanSortedBy::Matches(SortOrder::Desc)
    }
}

impl ClanSortedBy {
    /// The direction this sort is currently running in, whichever column it is on.
    fn order(self) -> SortOrder {
        match self {
            ClanSortedBy::Clan(order)
            | ClanSortedBy::Members(order)
            | ClanSortedBy::Matches(order)
            | ClanSortedBy::MatchesInRange(order)
            | ClanSortedBy::Sightings(order)
            | ClanSortedBy::LastSeen(order) => order,
        }
    }

    /// Clicking the active column flips its direction; clicking another moves the
    /// sort to it in that column's starting direction.
    fn transition_to(&mut self, new: ClanSortedBy) {
        match (self, new) {
            (ClanSortedBy::Clan(order), ClanSortedBy::Clan(_))
            | (ClanSortedBy::Members(order), ClanSortedBy::Members(_))
            | (ClanSortedBy::Matches(order), ClanSortedBy::Matches(_))
            | (ClanSortedBy::MatchesInRange(order), ClanSortedBy::MatchesInRange(_))
            | (ClanSortedBy::Sightings(order), ClanSortedBy::Sightings(_))
            | (ClanSortedBy::LastSeen(order), ClanSortedBy::LastSeen(_)) => order.toggle(),
            (old, new) => *old = new,
        }
    }
}

/// Indices into `rows`, in the order the active sort puts them. Indices rather
/// than a sorted copy so a re-sort does not clone every clan's member list.
fn sorted_order(rows: &[ClanRow], sort: ClanSortedBy) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|a, b| {
        let (a, b) = (&rows[*a], &rows[*b]);
        let ordering = match sort {
            ClanSortedBy::Clan(_) => a.clan.cmp(&b.clan),
            ClanSortedBy::Members(_) => a.members.len().cmp(&b.members.len()),
            ClanSortedBy::Matches(_) => a.matches.cmp(&b.matches),
            ClanSortedBy::MatchesInRange(_) => a.matches_in_range.cmp(&b.matches_in_range),
            ClanSortedBy::Sightings(_) => a.sightings.cmp(&b.sightings),
            ClanSortedBy::LastSeen(_) => a.last_seen.cmp(&b.last_seen),
        };
        let ordering = if sort.order() == SortOrder::Asc { ordering } else { ordering.reverse() };
        // Tags are unique, so this makes the order total whatever the key ties on.
        ordering.then_with(|| a.clan.cmp(&b.clan))
    });
    order
}

/// Values copied out of one clan row for a single frame's render. Cheap:
/// egui_table only paints visible rows.
struct RowView {
    clan: String,
    members: usize,
    matches: usize,
    matches_in_range: usize,
    sightings: usize,
    sightings_in_range: usize,
    last_seen: String,
    last_seen_exact: String,
}

struct ClansTable<'a> {
    tracker: &'a mut PlayerTracker,
    /// Taken out of the tracker for the frame so the delegate can hold both it
    /// and the mutable tracker borrow it needs for the open set.
    breakdown: ClanBreakdown,
    order: Vec<usize>,
    /// Height each open row's detail block adds on top of the default row height,
    /// measured as it is painted and used to lay the next frame out.
    detail_heights: BTreeMap<u64, f32>,
    /// Rows whose detail block has already been claimed this frame, since
    /// `row_ui` is called once per split-scroll region.
    detail_rows_painted: HashSet<u64>,
    /// Animation factor per open or still-closing row, derived from
    /// `expanded_clans` each frame because `row_top_offset` addresses rows
    /// positionally. Rows contributing no extra height are absent.
    expanded_rows: BTreeMap<u64, f32>,
    now: Timestamp,
    find_matches_target: Option<AccountId>,
    clan_search_target: Option<String>,
}

impl ClansTable<'_> {
    fn row(&self, row_nr: u64) -> Option<&ClanRow> {
        let index = *self.order.get(row_nr as usize)?;
        self.breakdown.rows.get(index)
    }

    fn row_view(&self, row_nr: u64) -> Option<RowView> {
        let row = self.row(row_nr)?;
        Some(RowView {
            clan: row.clan.clone(),
            members: row.members.len(),
            matches: row.matches,
            matches_in_range: row.matches_in_range,
            sightings: row.sightings,
            sightings_in_range: row.sightings_in_range,
            last_seen: relative_age_text(row.last_seen, self.now),
            last_seen_exact: exact_timestamp_text(row.last_seen),
        })
    }

    /// The collapsed content of one cell. The expanded member list is painted by
    /// [`egui_table::TableDelegate::row_ui`] instead, so it can span the row.
    fn cell_content_ui(&mut self, row_nr: u64, column: ClanColumn, ui: &mut egui::Ui) {
        let Some(view) = self.row_view(row_nr) else {
            return;
        };

        // Absent means the row adds no height this frame: either it is closed, or
        // its collapse animation has already run out.
        let expandedness = self.expanded_rows.get(&row_nr).copied().unwrap_or(0.0);
        let mut toggle_expand = false;

        ui.horizontal(|ui| match column {
            ClanColumn::Clan => {
                let (_, response) = ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::click());
                egui::collapsing_header::paint_default_icon(ui, expandedness, &response);
                if response.clicked() {
                    toggle_expand = true;
                }

                let mut tag = RichText::new(&view.clan);
                if let Some(color) = encounter_severity_color(ui, view.matches_in_range) {
                    tag = tag.color(color);
                }
                ui.label(tag);
            }
            ClanColumn::Members => {
                ui.label(view.members.to_string());
            }
            ClanColumn::Matches => {
                ui.label(view.matches.to_string());
            }
            ClanColumn::MatchesInRange => {
                let mut text = RichText::new(view.matches_in_range.to_string());
                if let Some(color) = encounter_severity_color(ui, view.matches_in_range) {
                    text = text.color(color);
                }
                ui.label(text);
            }
            ClanColumn::Sightings => {
                ui.label(view.sightings.to_string())
                    .on_hover_text(t!("ui.player_tracker.clan_sightings_hover", range = view.sightings_in_range));
            }
            ClanColumn::LastSeen => {
                ui.label(&view.last_seen).on_hover_text(&view.last_seen_exact);
            }
            ClanColumn::Actions => {
                if ui.button(icons::MAGNIFYING_GLASS).on_hover_text(t!("ui.player_tracker.find_clan_matches")).clicked()
                {
                    self.clan_search_target = Some(view.clan.clone());
                }
            }
        });

        if toggle_expand {
            // `remove` reports whether it was there, which is the toggle. The row's
            // animation factor only picks the change up on the next frame.
            if !self.tracker.expanded_clans.remove(&view.clan) {
                self.tracker.expanded_clans.insert(view.clan);
            }
        }
    }

    /// The member list of an open row, filling the row below the collapsed
    /// content. Returns its height, which is what the row is stretched by.
    fn detail_ui(&mut self, row_nr: u64, ui: &mut egui::Ui) -> f32 {
        // Cloned so the buttons below can write back to `self`; only ever runs for
        // the handful of rows that are open.
        let Some(members) = self.row(row_nr).map(|row| row.members.clone()) else {
            return 0.0;
        };

        let inner_response = egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            for (account_id, matches) in members {
                // A member the tracker has no entry for cannot happen (the
                // breakdown is built from the tracker), but the account id is a
                // usable label if it ever does.
                let name = self
                    .tracker
                    .tracked_players
                    .get(&account_id)
                    .map(|player| player.last_name.clone())
                    .unwrap_or_else(|| account_id.to_string());

                ui.horizontal(|ui| {
                    if ui.button(icons::MAGNIFYING_GLASS).on_hover_text(t!("ui.player_tracker.find_matches")).clicked()
                    {
                        self.find_matches_target = Some(account_id);
                    }
                    ui.label(name);
                    ui.label(
                        RichText::new(t!("ui.player_tracker.clan_member_matches", count = matches)).small().weak(),
                    );
                });
            }
        });

        inner_response.response.rect.height()
    }
}

impl egui_table::TableDelegate for ClansTable<'_> {
    fn header_cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::HeaderCellInfo) {
        let column = CLAN_COLUMNS[cell.group_index];
        let sorted_by = self.tracker.clan_sort_order;
        let (key, target, is_active) = match column {
            ClanColumn::Clan => (
                "ui.player_tracker.column.clan",
                ClanSortedBy::Clan(SortOrder::Asc),
                matches!(sorted_by, ClanSortedBy::Clan(_)),
            ),
            ClanColumn::Members => (
                "ui.player_tracker.column.members",
                ClanSortedBy::Members(SortOrder::Desc),
                matches!(sorted_by, ClanSortedBy::Members(_)),
            ),
            ClanColumn::Matches => (
                "ui.player_tracker.column.matches",
                ClanSortedBy::Matches(SortOrder::Desc),
                matches!(sorted_by, ClanSortedBy::Matches(_)),
            ),
            ClanColumn::MatchesInRange => (
                "ui.player_tracker.column.encounters_in_range",
                ClanSortedBy::MatchesInRange(SortOrder::Desc),
                matches!(sorted_by, ClanSortedBy::MatchesInRange(_)),
            ),
            ClanColumn::Sightings => (
                "ui.player_tracker.column.sightings",
                ClanSortedBy::Sightings(SortOrder::Desc),
                matches!(sorted_by, ClanSortedBy::Sightings(_)),
            ),
            ClanColumn::LastSeen => (
                "ui.player_tracker.column.last_encountered",
                ClanSortedBy::LastSeen(SortOrder::Desc),
                matches!(sorted_by, ClanSortedBy::LastSeen(_)),
            ),
            ClanColumn::Actions => return,
        };

        egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 0)).show(ui, |ui| {
            if ui.strong(sort_header_label(key, sorted_by.order(), is_active)).clicked() {
                self.tracker.clan_sort_order.transition_to(target);
                // The row order was built before the table was shown, so the new
                // sort lands on the next frame.
                ui.ctx().request_repaint();
            }
        });
    }

    fn row_ui(&mut self, ui: &mut egui::Ui, row_nr: u64) {
        // Striping belongs here rather than in `cell_ui`, which runs afterwards and
        // would paint over the member list.
        if row_nr % 2 == 1 {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().faint_bg_color);
        }

        // egui_table drives `row_ui` once per split-scroll region, so the block has
        // to be claimed by exactly one of them. The fully scrollable region always
        // runs first and is the wide one, so the first call for a row wins.
        if !self.detail_rows_painted.insert(row_nr) {
            return;
        }

        let expandedness = self.expanded_rows.get(&row_nr).copied().unwrap_or(0.0);
        if expandedness <= 0.0 {
            return;
        }

        let row_rect = ui.max_rect();
        let Some(detail_rect) = detail_rect(row_rect, ui.clip_rect(), CLAN_ROW_HEIGHT) else {
            return;
        };

        let mut detail_ui =
            ui.new_child(egui::UiBuilder::new().max_rect(detail_rect).layout(egui::Layout::top_down(egui::Align::Min)));
        // `row_ui` is not clipped to its own row, so without this the block would
        // paint over the rows below it while the row is still animating open.
        detail_ui.shrink_clip_rect(row_rect);

        // Deliberately not advancing the row's cursor: the block is measured for the
        // row height alone and must not feed egui_table's per-column sizing.
        let height = self.detail_ui(row_nr, &mut detail_ui);
        self.detail_heights.insert(row_nr, height);
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::CellInfo) {
        // Cells are handed the whole row, member list included. Pin the collapsed
        // content to the top of it, or a vertically centred layout would drift it
        // into the middle of an open row.
        let mut content_rect = ui.max_rect();
        content_rect.max.y = content_rect.min.y + CLAN_ROW_HEIGHT;
        let mut content_ui = ui.new_child(
            egui::UiBuilder::new().max_rect(content_rect).layout(egui::Layout::left_to_right(egui::Align::Center)),
        );

        egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 4)).show(&mut content_ui, |ui| {
            self.cell_content_ui(cell.row_nr, CLAN_COLUMNS[cell.col_nr], ui);
        });

        // A child `Ui` does not grow its parent, and egui_table sizes columns from
        // the cell's `min_size`. Without this every column would fit to nothing on
        // the sizing pass and land on its minimum width.
        ui.advance_cursor_after_rect(content_ui.min_rect());
    }

    fn default_row_height(&self) -> f32 {
        CLAN_ROW_HEIGHT
    }

    fn row_top_offset(&self, _ctx: &egui::Context, _table_id: egui::Id, row_nr: u64) -> f32 {
        row_offset(&self.detail_heights, &self.expanded_rows, row_nr, CLAN_ROW_HEIGHT)
    }
}

/// The index's latest clan per account, the per-encounter clan corrections, and
/// the arenas the index covers. Each degrades to its empty value on query error,
/// so the tab falls back to current-clan attribution rather than showing nothing.
fn fetch_index_inputs(
    pool: &sqlx::SqlitePool,
    rt: &tokio::runtime::Runtime,
) -> (HashMap<AccountId, String>, Vec<ClanCorrection>, HashSet<ArenaId>) {
    use crate::db::index::query;
    use crate::db::index::rows::MatchFilter;

    let filter = MatchFilter::default();

    let latest = match rt.block_on(query::distinct_players(pool, &filter)) {
        Ok(players) => players.into_iter().map(|facet| (facet.account_id, facet.clan)).collect(),
        Err(e) => {
            tracing::warn!("clan breakdown: distinct_players failed: {e}");
            HashMap::new()
        }
    };

    let corrections = match rt.block_on(query::clan_history_corrections(pool, &filter)) {
        Ok(corrections) => corrections,
        Err(e) => {
            tracing::warn!("clan breakdown: clan_history_corrections failed: {e}");
            Vec::new()
        }
    };

    let indexed_arenas = match rt.block_on(query::indexed_arena_ids(pool, &filter)) {
        Ok(arenas) => arenas,
        Err(e) => {
            tracing::warn!("clan breakdown: indexed_arena_ids failed: {e}");
            HashSet::new()
        }
    };

    (latest, corrections, indexed_arenas)
}

/// Rebuild the breakdown when one of its inputs changed. The index queries run
/// synchronously on the UI thread, the way `populate_from_index` does on its
/// button press, so this must not fire on an unchanged frame.
fn refresh_clan_breakdown(
    tracker: &mut PlayerTracker,
    pool: Option<&sqlx::SqlitePool>,
    rt: Option<&tokio::runtime::Runtime>,
) {
    let period = tracker.filter_time_period;
    let current = tracker.clan_breakdown.as_ref().is_some_and(|breakdown| {
        breakdown.tracked_count == tracker.tracked_players.len() && tracker.clan_breakdown_period == Some(period)
    });
    if current {
        return;
    }

    let (latest, corrections, indexed_arenas) = match (pool, rt) {
        (Some(pool), Some(rt)) => fetch_index_inputs(pool, rt),
        // With no index open there is nothing to correct against, so every
        // encounter falls back to the player's current clan and the attribution
        // line reports zero coverage.
        _ => (HashMap::new(), Vec::new(), HashSet::new()),
    };

    tracker.clan_breakdown =
        Some(build_clan_breakdown(&tracker.tracked_players, &latest, &corrections, &indexed_arenas, period.to_date()));
    tracker.clan_breakdown_period = Some(period);
}

impl ToolkitTabViewer<'_> {
    pub(crate) fn build_clans_sub_tab(&mut self, ui: &mut egui::Ui) {
        // Collected during the table pass and applied after the table (the
        // player-tracker write lock is held while rendering rows, so we can't
        // touch other `self.tab_state` fields from inside a cell body).
        let mut find_matches_target: Option<AccountId> = None;
        let mut clan_search_target: Option<String> = None;

        // Scoped so the write guard is released before the deferred actions run.
        {
            let mut player_tracker_settings = self.tab_state.player_tracker.write();
            let player_tracker = &mut *player_tracker_settings;
            let now = Timestamp::now();

            refresh_clan_breakdown(
                player_tracker,
                self.tab_state.db_pool.as_ref(),
                self.tab_state.tokio_runtime.as_deref(),
            );

            ui.vertical(|ui| {
                // Taken for the frame so the delegate can hold it alongside the
                // mutable tracker borrow; put back below.
                let Some(breakdown) = player_tracker.clan_breakdown.take() else {
                    return;
                };

                ui.horizontal(|ui| {
                    if ui.button(t!("ui.player_tracker.clan_refresh")).clicked() {
                        // Clearing the key is the whole refresh, and it survives
                        // the breakdown being put back at the end of this frame.
                        player_tracker.clan_breakdown_period = None;
                    }
                    ui.label(t!(
                        "ui.player_tracker.clan_attribution",
                        exact = breakdown.exact_encounters,
                        total = breakdown.total_encounters
                    ));
                });

                ui.add_space(10.0);
                ui.separator();

                if breakdown.rows.is_empty() {
                    ui.label(t!("ui.player_tracker.clan_no_data"));
                    player_tracker.clan_breakdown = Some(breakdown);
                    return;
                }

                let order = sorted_order(&breakdown.rows, player_tracker.clan_sort_order);
                let expanded_rows = expanded_rows(
                    ui.ctx(),
                    CLAN_ROW_SALT,
                    order.iter().map(|index| &breakdown.rows[*index].clan),
                    &player_tracker.expanded_clans,
                );
                let detail_heights = std::mem::take(&mut player_tracker.clan_detail_heights);

                let mut delegate = ClansTable {
                    tracker: player_tracker,
                    breakdown,
                    order,
                    detail_heights,
                    detail_rows_painted: Default::default(),
                    expanded_rows,
                    now,
                    find_matches_target: None,
                    clan_search_target: None,
                };

                let columns = vec![
                    egui_table::Column::new(110.0).range(60.0..=250.0).resizable(true),
                    egui_table::Column::new(90.0).range(50.0..=200.0).resizable(true),
                    egui_table::Column::new(90.0).range(50.0..=200.0).resizable(true),
                    egui_table::Column::new(150.0).range(60.0..=300.0).resizable(true),
                    egui_table::Column::new(100.0).range(50.0..=250.0).resizable(true),
                    egui_table::Column::new(130.0).range(80.0..=300.0).resizable(true),
                    egui_table::Column::new(70.0).resizable(false),
                ];

                egui_table::Table::new()
                    .id_salt("player_tracker_clans")
                    .num_rows(delegate.order.len() as u64)
                    .columns(columns)
                    .num_sticky_cols(1)
                    .headers([egui_table::HeaderRow { height: 20.0, groups: Default::default() }])
                    .auto_size_mode(egui_table::AutoSizeMode::Never)
                    .show(ui, &mut delegate);

                // Destructured in one go so the delegate's borrow of the tracker
                // ends before anything is written back to it.
                let ClansTable {
                    breakdown,
                    detail_heights,
                    find_matches_target: clicked_member,
                    clan_search_target: clicked_clan,
                    ..
                } = delegate;

                find_matches_target = clicked_member;
                clan_search_target = clicked_clan;
                player_tracker.clan_detail_heights = detail_heights;
                player_tracker.clan_breakdown = Some(breakdown);
            });
        }

        if let Some(id) = find_matches_target {
            self.queue_player_search(id);
        }
        if let Some(clan) = clan_search_target {
            self.queue_clan_search(&clan);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(clan: &str, encounters: &[(i64, i64)]) -> TrackedPlayer {
        let mut p = TrackedPlayer::default();
        clan.clone_into(&mut p.clan);
        for (arena, second) in encounters {
            p.arena_ids.insert(ArenaId::new(*arena));
            p.timestamps.insert(Timestamp::from_second(*second).unwrap());
        }
        p
    }

    fn tracked(entries: &[(i64, TrackedPlayer)]) -> HashMap<AccountId, TrackedPlayer> {
        entries.iter().map(|(id, p)| (AccountId(*id), p.clone())).collect()
    }

    #[test]
    fn groups_by_latest_clan_when_the_index_has_no_corrections() {
        let players = tracked(&[(1, player("RAIN", &[(10, 1000), (11, 2000)])), (2, player("RAIN", &[(10, 1000)]))]);

        let breakdown = build_clan_breakdown(&players, &HashMap::new(), &[], &HashSet::new(), None);

        assert_eq!(breakdown.rows.len(), 1);
        let rain = &breakdown.rows[0];
        assert_eq!(rain.clan, "RAIN");
        assert_eq!(rain.members.len(), 2);
        // Arena 10 holds both members, so it counts once.
        assert_eq!(rain.matches, 2);
        // Both members in arena 10 plus one in arena 11.
        assert_eq!(rain.sightings, 3);
    }

    #[test]
    fn a_correction_moves_one_encounter_to_the_historical_clan() {
        let players = tracked(&[(1, player("WOLF", &[(10, 1000), (11, 2000)]))]);
        let corrections = vec![ClanCorrection {
            account_id: AccountId(1),
            arena_id: ArenaId::new(10),
            timestamp: Timestamp::from_second(1000).unwrap(),
            clan: "RAIN".into(),
        }];

        let breakdown = build_clan_breakdown(&players, &HashMap::new(), &corrections, &HashSet::new(), None);

        let wolf = breakdown.rows.iter().find(|r| r.clan == "WOLF").expect("WOLF row");
        let rain = breakdown.rows.iter().find(|r| r.clan == "RAIN").expect("RAIN row");
        assert_eq!(wolf.matches, 1);
        assert_eq!(rain.matches, 1);
    }

    #[test]
    fn the_index_latest_clan_overrides_the_trackers_when_both_are_known() {
        let players = tracked(&[(1, player("STALE", &[(10, 1000)]))]);
        let latest = HashMap::from([(AccountId(1), "FRESH".to_string())]);

        let breakdown = build_clan_breakdown(&players, &latest, &[], &HashSet::new(), None);

        assert_eq!(breakdown.rows.len(), 1);
        assert_eq!(breakdown.rows[0].clan, "FRESH");
    }

    #[test]
    fn clanless_players_are_excluded() {
        let players = tracked(&[(1, player("", &[(10, 1000)]))]);

        let breakdown = build_clan_breakdown(&players, &HashMap::new(), &[], &HashSet::new(), None);

        assert!(breakdown.rows.is_empty());
    }

    #[test]
    fn a_correction_to_no_clan_drops_that_encounter() {
        let players = tracked(&[(1, player("RAIN", &[(10, 1000), (11, 2000)]))]);
        let corrections = vec![ClanCorrection {
            account_id: AccountId(1),
            arena_id: ArenaId::new(10),
            timestamp: Timestamp::from_second(1000).unwrap(),
            clan: String::new(),
        }];

        let breakdown = build_clan_breakdown(&players, &HashMap::new(), &corrections, &HashSet::new(), None);

        assert_eq!(breakdown.rows.len(), 1);
        assert_eq!(breakdown.rows[0].matches, 1);
    }

    #[test]
    fn the_range_filter_counts_only_timestamps_after_since() {
        let players = tracked(&[(1, player("RAIN", &[(10, 1000), (11, 5000)]))]);

        let breakdown = build_clan_breakdown(
            &players,
            &HashMap::new(),
            &[],
            &HashSet::new(),
            Some(Timestamp::from_second(3000).unwrap()),
        );

        let rain = &breakdown.rows[0];
        assert_eq!(rain.matches, 2);
        assert_eq!(rain.matches_in_range, 1);
        assert_eq!(rain.sightings_in_range, 1);
        assert_eq!(rain.last_seen, Timestamp::from_second(5000).unwrap());
    }

    #[test]
    fn coverage_counts_encounters_whose_arena_the_index_knows() {
        let players = tracked(&[(1, player("RAIN", &[(10, 1000), (11, 2000)]))]);
        let indexed = HashSet::from([ArenaId::new(10)]);

        let breakdown = build_clan_breakdown(&players, &HashMap::new(), &[], &indexed, None);

        assert_eq!(breakdown.total_encounters, 2);
        assert_eq!(breakdown.exact_encounters, 1);
    }
}
