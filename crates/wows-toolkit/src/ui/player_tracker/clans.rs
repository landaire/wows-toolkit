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

use super::ExpandingColumn;
use super::PlayerTracker;
use super::SortOrder;
use super::TimePeriod;
use super::TrackedPlayer;
use super::cell_is_in_this_region;
use super::detail_rect;
use super::encounter_severity_color;
use super::exact_timestamp_text;
use super::expanded_rows;
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

/// The clan rows, with the cache key they were built against.
#[derive(Debug, Clone)]
pub(crate) struct ClanBreakdown {
    /// [`PlayerTracker::encounter_version`] this was built against, so newly
    /// ingested history invalidates it. A count of tracked players would not:
    /// a battle against players already in the tracker adds arena ids and
    /// timestamps to existing entries and leaves the map the same size.
    pub encounter_version: u64,
    pub rows: Vec<ClanRow>,
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
/// rows whose clan at the time differed from that latest clan.
///
/// Matches are counted by distinct arena and in-range matches by distinct
/// timestamp. Both keys are exact on their own, which is why the tracker's
/// unpaired `arena_ids` and `timestamps` sets need no reconciliation: every
/// player in a battle shares that battle's timestamp.
///
/// `encounter_version` is the tracker's version of `tracked` at the time of the
/// call, carried on the result as its cache key.
pub(crate) fn build_clan_breakdown(
    tracked: &HashMap<AccountId, TrackedPlayer>,
    encounter_version: u64,
    index_latest_clan: &HashMap<AccountId, String>,
    corrections: &[ClanCorrection],
    since: Option<Timestamp>,
) -> ClanBreakdown {
    let mut by_arena: HashMap<(AccountId, ArenaId), &str> = HashMap::new();
    let mut by_timestamp: HashMap<(AccountId, Timestamp), &str> = HashMap::new();
    for correction in corrections {
        by_arena.insert((correction.account_id, correction.arena_id), &correction.clan);
        by_timestamp.insert((correction.account_id, correction.timestamp), &correction.clan);
    }

    let mut clans: HashMap<String, ClanAccumulator> = HashMap::new();

    for (account_id, player) in tracked {
        // The tracker holds one clan per player, the latest it saw. The index's
        // latest is fresher wherever the index knows the account at all.
        let baseline = index_latest_clan.get(account_id).map(String::as_str).unwrap_or(player.clan.as_str());

        for arena_id in &player.arena_ids {
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
            // rather than invent a timestamp, and say so: the row would
            // otherwise vanish with a non-zero match count and no signal.
            let Some(last_seen) = acc.last_seen else {
                tracing::warn!(
                    clan = clan.as_str(),
                    matches = acc.arenas.len(),
                    "clan breakdown: dropping a clan whose encounters carry no timestamp"
                );
                return None;
            };
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

    ClanBreakdown { encounter_version, rows }
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

/// The Clan column, which hosts the expanded member list. `expanded_width` only
/// has to fit one member per line (a search button, a player name and its match
/// count), which is narrower than the Historical tab's notes editor needs: the
/// list grows downwards, not sideways, and the row stretches to match.
/// A column the user dragged wider than that narrows back to it on expand; see
/// [`ExpandingColumn`] for why the two regimes cannot share a remembered width.
const CLAN_COLUMN: ExpandingColumn = ExpandingColumn {
    collapsed_width: 110.0,
    expanded_width: 320.0,
    min_width: 60.0,
    max_width: 450.0,
    collapsed_id: "player_tracker_clans_clan",
    expanded_id: "player_tracker_clans_clan_expanded",
};

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
        let ordering = sort.order().direct(ordering);
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

    /// The collapsed content of one cell, pinned to the top of the row.
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

    /// The member list of an open row, filling the Clan cell below the collapsed
    /// content. Returns its height, which is what the row is stretched by. The
    /// height is the list's natural one even while the row is part-way through
    /// its animation and only a slice of it is on screen, so the row settles at
    /// exactly this height once the animation completes.
    fn detail_ui(&mut self, row_nr: u64, ui: &mut egui::Ui) -> f32 {
        // Cloned so the buttons below can write back to `self`; only ever runs for
        // the handful of rows that are open.
        let Some(members) = self.row(row_nr).map(|row| row.members.clone()) else {
            return 0.0;
        };

        let inner_response = egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            // egui_table runs the whole table on `TextWrapMode::Extend`, which suits
            // single-line cells but would push a long player name out past the
            // column's edge. Wrapping keeps the list inside its column and folds
            // the extra lines into the height the row is stretched by.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);

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

    /// Paints an open row's member list into the part of the Clan cell left
    /// below the collapsed content, and records the height the row is stretched
    /// by. `cell_rect` supplies both the column's width and the row's full band,
    /// which is what keeps the list inside its column separators.
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

        let Some(detail_rect) = detail_rect(cell_rect, CLAN_ROW_HEIGHT) else {
            return;
        };

        let mut detail_ui =
            ui.new_child(egui::UiBuilder::new().max_rect(detail_rect).layout(egui::Layout::top_down(egui::Align::Min)));

        // Deliberately not advancing the cell's cursor: the list is measured for
        // the row height alone and must not feed egui_table's per-column sizing,
        // which would ratchet the column wider on every frame it is open.
        let height = self.detail_ui(row_nr, &mut detail_ui);
        self.detail_heights.insert(row_nr, height);
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
        // would paint over the cell contents.
        if row_nr % 2 == 1 {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().faint_bg_color);
        }
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::CellInfo) {
        let column = CLAN_COLUMNS[cell.col_nr];
        let cell_rect = ui.max_rect();

        // Cells are handed the whole row, member list included. Pin the collapsed
        // content to the top of it, or a vertically centred layout would drift it
        // into the middle of an open row.
        let mut content_rect = cell_rect;
        content_rect.max.y = content_rect.min.y + CLAN_ROW_HEIGHT;
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

        if column == ClanColumn::Clan {
            self.detail_block_ui(cell.row_nr, ui, cell_rect);
        }
    }

    fn default_row_height(&self) -> f32 {
        CLAN_ROW_HEIGHT
    }

    fn row_top_offset(&self, _ctx: &egui::Context, _table_id: egui::Id, row_nr: u64) -> f32 {
        row_offset(&self.detail_heights, &self.expanded_rows, row_nr, CLAN_ROW_HEIGHT)
    }
}

/// The index's latest clan per account and the per-encounter clan corrections.
/// Each degrades to its empty value on query error, so the tab falls back to
/// current-clan attribution rather than showing nothing.
fn fetch_index_inputs(
    pool: &sqlx::SqlitePool,
    rt: &tokio::runtime::Runtime,
) -> (HashMap<AccountId, String>, Vec<ClanCorrection>) {
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

    (latest, corrections)
}

/// `boundary` with everything below the minute cleared.
///
/// Truncation is what lets a wall-clock boundary sit in a cache key at all: an
/// exact boundary moves on every repaint, and the index queries behind the
/// breakdown would run on every frame.
fn truncate_to_minute(boundary: Timestamp) -> Timestamp {
    let second = boundary.as_second();
    // `rem_euclid` truncates backwards for pre-epoch timestamps too. The
    // subtraction can only leave the representable range within a minute of its
    // lower bound, where the untruncated boundary is the honest answer.
    Timestamp::from_second(second - second.rem_euclid(60)).unwrap_or(boundary)
}

/// The window a breakdown's in-range counts were built for.
///
/// The resolved boundary belongs in the key alongside the period because it
/// moves as wall-clock advances while the period does not. Without it the Clans
/// tab keeps counting a window the Historical tab, which resolves its own
/// boundary every paint, has already left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BreakdownWindow {
    period: TimePeriod,
    /// The period's boundary, truncated to the minute. `None` is the all-time
    /// period, which has no boundary.
    since: Option<Timestamp>,
}

impl BreakdownWindow {
    /// Resolve `period` against the current clock.
    fn resolve(period: TimePeriod) -> Self {
        Self { period, since: period.to_date().map(truncate_to_minute) }
    }
}

/// Whether a cached breakdown still matches what it would be built from now.
///
/// `encounter_version` moves on every ingest, including a battle against
/// players the tracker already holds, which is exactly the population this tab
/// exists to surface.
fn breakdown_is_current(
    breakdown: Option<&ClanBreakdown>,
    cached_window: Option<BreakdownWindow>,
    window: BreakdownWindow,
    encounter_version: u64,
) -> bool {
    breakdown.is_some_and(|breakdown| breakdown.encounter_version == encounter_version) && cached_window == Some(window)
}

/// Rebuild the breakdown when one of its inputs changed. The index queries run
/// synchronously on the UI thread, the way `populate_from_index` does on its
/// button press, so this must not fire on an unchanged frame.
fn refresh_clan_breakdown(
    tracker: &mut PlayerTracker,
    pool: Option<&sqlx::SqlitePool>,
    rt: Option<&tokio::runtime::Runtime>,
) {
    let window = BreakdownWindow::resolve(tracker.filter_time_period);
    if breakdown_is_current(
        tracker.clan_breakdown.as_ref(),
        tracker.clan_breakdown_window,
        window,
        tracker.encounter_version,
    ) {
        return;
    }

    let (latest, corrections) = match (pool, rt) {
        (Some(pool), Some(rt)) => fetch_index_inputs(pool, rt),
        // With no index open there is nothing to correct against, so every
        // encounter falls back to the player's current clan.
        _ => (HashMap::new(), Vec::new()),
    };

    // Built from the same truncated boundary the key carries, so the rows and
    // the key describe the same window.
    tracker.clan_breakdown = Some(build_clan_breakdown(
        &tracker.tracked_players,
        tracker.encounter_version,
        &latest,
        &corrections,
        window.since,
    ));
    tracker.clan_breakdown_window = Some(window);
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
                    // The same field the Historical tab's combo edits, so the two
                    // stay in sync in both directions with no extra state.
                    let selected = &mut player_tracker.filter_time_period;
                    egui::ComboBox::from_id_salt("player_tracker_clans_time_period_selection")
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
                // A row still animating shut keeps its factor, so the column stays
                // wide for as long as any of the list is on screen.
                let any_expanded = !expanded_rows.is_empty();

                let mut delegate = ClansTable {
                    tracker: player_tracker,
                    breakdown,
                    order,
                    detail_heights,
                    expanded_rows,
                    now,
                    find_matches_target: None,
                    clan_search_target: None,
                };

                let columns = vec![
                    CLAN_COLUMN.column(any_expanded),
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
    use jiff::ToSpan;

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

        let breakdown = build_clan_breakdown(&players, 0, &HashMap::new(), &[], None);

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

        let breakdown = build_clan_breakdown(&players, 0, &HashMap::new(), &corrections, None);

        let wolf = breakdown.rows.iter().find(|r| r.clan == "WOLF").expect("WOLF row");
        let rain = breakdown.rows.iter().find(|r| r.clan == "RAIN").expect("RAIN row");
        assert_eq!(wolf.matches, 1);
        assert_eq!(rain.matches, 1);
    }

    #[test]
    fn the_index_latest_clan_overrides_the_trackers_when_both_are_known() {
        let players = tracked(&[(1, player("STALE", &[(10, 1000)]))]);
        let latest = HashMap::from([(AccountId(1), "FRESH".to_string())]);

        let breakdown = build_clan_breakdown(&players, 0, &latest, &[], None);

        assert_eq!(breakdown.rows.len(), 1);
        assert_eq!(breakdown.rows[0].clan, "FRESH");
    }

    #[test]
    fn clanless_players_are_excluded() {
        let players = tracked(&[(1, player("", &[(10, 1000)]))]);

        let breakdown = build_clan_breakdown(&players, 0, &HashMap::new(), &[], None);

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

        let breakdown = build_clan_breakdown(&players, 0, &HashMap::new(), &corrections, None);

        assert_eq!(breakdown.rows.len(), 1);
        assert_eq!(breakdown.rows[0].matches, 1);
    }

    #[test]
    fn the_range_filter_counts_only_timestamps_after_since() {
        let players = tracked(&[(1, player("RAIN", &[(10, 1000), (11, 5000)]))]);

        let breakdown =
            build_clan_breakdown(&players, 0, &HashMap::new(), &[], Some(Timestamp::from_second(3000).unwrap()));

        let rain = &breakdown.rows[0];
        assert_eq!(rain.matches, 2);
        assert_eq!(rain.matches_in_range, 1);
        assert_eq!(rain.sightings_in_range, 1);
        assert_eq!(rain.last_seen, Timestamp::from_second(5000).unwrap());
    }

    /// The index queries behind the breakdown run synchronously on the UI
    /// thread, so an unchanged frame must not invalidate the cache.
    #[test]
    fn the_cached_breakdown_is_invalidated_by_its_inputs_and_nothing_else() {
        let breakdown =
            build_clan_breakdown(&tracked(&[(1, player("RAIN", &[(10, 1000)]))]), 7, &HashMap::new(), &[], None);
        let window = BreakdownWindow::resolve(TimePeriod::LastDay);
        let cached = Some(window);

        assert!(
            breakdown_is_current(Some(&breakdown), cached, window, 7),
            "an unchanged frame must reuse the cache rather than re-run the index queries"
        );
        assert!(
            !breakdown_is_current(Some(&breakdown), cached, BreakdownWindow::resolve(TimePeriod::LastWeek), 7),
            "changing the period on either sub-tab must invalidate the cache"
        );
        assert!(
            !breakdown_is_current(Some(&breakdown), cached, window, 8),
            "newly ingested history must invalidate the cache"
        );
        assert!(!breakdown_is_current(None, cached, window, 7), "the first paint of the tab has nothing cached");
    }

    /// The Clans tab and the Historical tab share one period, and Historical
    /// resolves its boundary every paint. If the cached window did not move with
    /// the clock the two would report different counts for the same window.
    #[test]
    fn the_cached_breakdown_expires_when_its_boundary_moves_past_a_minute() {
        let breakdown =
            build_clan_breakdown(&tracked(&[(1, player("RAIN", &[(10, 1000)]))]), 7, &HashMap::new(), &[], None);
        let window = BreakdownWindow::resolve(TimePeriod::LastHour);
        let since = window.since.expect("an hour-long period has a boundary");

        let a_minute_later = BreakdownWindow { since: Some(since + 1.minute()), ..window };
        assert!(
            !breakdown_is_current(Some(&breakdown), Some(window), a_minute_later, 7),
            "a boundary that has moved on must rebuild, or the two sub-tabs disagree"
        );

        // What repainting a few seconds later resolves to: the same key, so the
        // index queries stay put.
        let within_the_same_minute =
            BreakdownWindow { since: Some(truncate_to_minute(since + 20.seconds())), ..window };
        assert!(
            breakdown_is_current(Some(&breakdown), Some(window), within_the_same_minute, 7),
            "a frame inside the same minute must reuse the cache"
        );
    }

    /// The all-time period has no boundary to move, so its key is the period
    /// alone and every frame reuses the cache.
    #[test]
    fn the_all_time_window_has_no_boundary() {
        let window = BreakdownWindow::resolve(TimePeriod::AllTime);
        assert_eq!(window.since, None);
        assert_eq!(window, BreakdownWindow::resolve(TimePeriod::AllTime));
    }

    #[test]
    fn truncating_a_boundary_clears_everything_below_the_minute() {
        let ts = Timestamp::from_second(1_700_000_059).unwrap();
        assert_eq!(truncate_to_minute(ts), Timestamp::from_second(1_700_000_040).unwrap());
        assert_eq!(
            truncate_to_minute(Timestamp::from_second(1_700_000_040).unwrap()),
            Timestamp::from_second(1_700_000_040).unwrap(),
            "a boundary already on the minute is left alone"
        );

        let before_epoch = Timestamp::from_second(-59).unwrap();
        assert_eq!(
            truncate_to_minute(before_epoch),
            Timestamp::from_second(-60).unwrap(),
            "a pre-epoch boundary truncates backwards, not towards zero"
        );
    }

    /// The population this tab exists to surface is clans you meet repeatedly,
    /// so a battle against players the tracker already holds has to invalidate
    /// the cache. It adds arena ids and timestamps to existing entries and
    /// leaves `tracked_players` the same size, which is why the key cannot be
    /// that size.
    #[test]
    fn a_repeat_encounter_with_a_tracked_player_invalidates_the_cache() {
        let mut tracker = PlayerTracker::default();
        for (id, p) in tracked(&[(1, player("RAIN", &[(10, 1000)]))]) {
            tracker.tracked_players.insert(id, p);
        }
        tracker.note_encounters_changed();

        let breakdown =
            build_clan_breakdown(&tracker.tracked_players, tracker.encounter_version, &HashMap::new(), &[], None);
        let window = BreakdownWindow::resolve(TimePeriod::LastDay);
        let cached = Some(window);
        assert!(breakdown_is_current(Some(&breakdown), cached, window, tracker.encounter_version));

        // What a second battle against the same player does to the tracker.
        let players_before = tracker.tracked_players.len();
        let entry = tracker.tracked_players.get_mut(&AccountId(1)).expect("the player is tracked");
        entry.arena_ids.insert(ArenaId::new(11));
        entry.timestamps.insert(Timestamp::from_second(2000).unwrap());
        tracker.note_encounters_changed();

        assert_eq!(tracker.tracked_players.len(), players_before, "a repeat encounter adds no tracked player");
        assert!(
            !breakdown_is_current(Some(&breakdown), cached, window, tracker.encounter_version),
            "a repeat encounter must invalidate the cache, or the tab shows stale counts"
        );
    }

    /// The member list lives inside the Clan cell, so it spans exactly that
    /// column and cannot reach across a column separator.
    #[test]
    fn the_member_list_spans_the_clan_cell_and_no_more() {
        // What egui_table hands a Clan cell of an open row: the column's x
        // range, and the row's whole band including the member list.
        let cell = egui::Rect::from_min_max(egui::pos2(4.0, 100.0), egui::pos2(324.0, 260.0));

        let rect = detail_rect(cell, CLAN_ROW_HEIGHT).expect("a cell with width yields a rect");
        assert_eq!(rect.x_range(), cell.x_range(), "the list takes the column's width, and no more");
        assert_eq!(rect.top(), cell.top() + CLAN_ROW_HEIGHT, "the list starts below the collapsed content");
        assert_eq!(rect.bottom(), cell.bottom(), "the list ends with the row, which is what the height feeds");
    }

    /// The member list is as wide as the column, so an open row has to widen it
    /// or the names are unreadable. egui_table only ever grows a remembered
    /// width, so the two regimes must not share a column id.
    #[test]
    fn the_clan_column_widens_while_a_row_is_open() {
        let collapsed = CLAN_COLUMN.column(false);
        let expanded = CLAN_COLUMN.column(true);

        assert_eq!(collapsed.current, CLAN_COLUMN.collapsed_width);
        assert_eq!(expanded.current, CLAN_COLUMN.expanded_width);
        assert!(
            collapsed.range.max >= expanded.range.min,
            "the expanded width has to be reachable by the collapsed range too, or a drag cannot follow it"
        );
        assert!(
            expanded.range.min > collapsed.current,
            "an open row must force the column past the width it sits at when closed"
        );
        assert_ne!(
            collapsed.id_for(0),
            expanded.id_for(0),
            "sharing an id would leave the column expanded for good once a row had been opened"
        );
    }
}
