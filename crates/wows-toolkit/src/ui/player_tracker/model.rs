use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::hash::Hash;

use egui::Color32;
use jiff::Timestamp;
use jiff::ToSpan;
use jiff::Unit;
use jiff::ZonedDifference;
use jiff::tz::TimeZone;
use rust_i18n::t;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;

use crate::icons;
use crate::ui::theme::semantic::SemanticExt;

use super::PlayerTracker;

/// Colour for the escalating encounter-count severity ramp, or `None` below
/// the threshold where it becomes notable.
pub(crate) fn encounter_severity_color(ui: &egui::Ui, times_encountered_in_range: usize) -> Option<Color32> {
    match times_encountered_in_range {
        0..=1 => None,
        2..=3 => Some(ui.sem().division),
        4..=5 => Some(ui.sem().warn),
        _ => Some(ui.sem().loss),
    }
}

/// Human-readable "how long ago" for a past timestamp.
pub(crate) fn relative_age_text(timestamp: Timestamp, now: Timestamp) -> String {
    let timestamp = timestamp.to_zoned(TimeZone::system());
    let now = now.to_zoned(TimeZone::system());
    let delta = now
        .since(
            ZonedDifference::new(&timestamp)
                .smallest(Unit::Minute)
                .largest(Unit::Year)
                .mode(jiff::RoundMode::HalfExpand),
        )
        .expect("failed to calculate the age of an encounter timestamp");

    format!("{delta:#}")
}

/// Absolute local-time rendering of a timestamp, for the hover behind a
/// relative age.
pub(crate) fn exact_timestamp_text(timestamp: Timestamp) -> String {
    timestamp.to_zoned(TimeZone::system()).strftime("%Y-%m-%d %H:%M:%S").to_string()
}

/// Human-readable "how long ago" for an encounter timestamp. Empty when there
/// is no encounter to describe, which is the right degradation for a hover.
pub(crate) fn last_seen_text(last_seen: Option<Timestamp>, now: Timestamp) -> String {
    match last_seen {
        Some(last) => relative_age_text(last, now),
        None => String::new(),
    }
}

/// How many of a player's encounters fall inside the active period, counting
/// only the ones the division-mate toggle leaves visible. `since` is the
/// period's resolved boundary; `None` is the all-time period, where every
/// visible encounter counts.
pub(crate) fn encounters_in_range(
    player: &TrackedPlayer,
    since: Option<Timestamp>,
    show_division_mates: bool,
) -> usize {
    player.visible_timestamps(show_division_mates).filter(|ts| since.is_none_or(|since| *ts > since)).count()
}

/// Absolute local-time stamp behind the relative "last seen" text. Empty for
/// the same reason [`last_seen_text`] is.
pub(crate) fn last_seen_timestamp_text(last_seen: Option<Timestamp>) -> String {
    last_seen.map(exact_timestamp_text).unwrap_or_default()
}

/// Header label with the sort arrow appended when this column drives the sort.
pub(crate) fn sort_header_label(key: &str, order: SortOrder, is_active: bool) -> String {
    let label: String = t!(key).into();
    if is_active { format!("{} {}", label, order.icon()) } else { label }
}

/// Per-row expansion factor, projected from the identity-keyed open set so that
/// a re-sort or re-filter carries an open row to that identity's new index
/// instead of leaving it open on whoever landed there.
///
/// Stepping every row's animation once here, and keeping only the rows that
/// still contribute height, is what lets [`row_offset`] walk a map bounded by
/// the open rows rather than by the whole table, without taking a context lock
/// per step. A row that is closing keeps a factor above zero until its animation
/// finishes, so the collapse still animates after it leaves the open set.
///
/// `salt` separates one table's row animations from another's, since the ids key
/// on the bare row number.
pub(crate) fn expanded_rows<'a, K>(
    ctx: &egui::Context,
    salt: &'static str,
    rows: impl IntoIterator<Item = &'a K>,
    expanded: &HashSet<K>,
) -> BTreeMap<u64, f32>
where
    K: Eq + Hash + 'a,
{
    rows.into_iter()
        .enumerate()
        .filter_map(|(index, key)| {
            let row_nr = index as u64;
            let factor = ctx.animate_bool(egui::Id::new((salt, row_nr)), expanded.contains(key));
            (0.0 < factor).then_some((row_nr, factor))
        })
        .collect()
}

/// Where an open row's detail block goes inside the cell that names the row:
/// the cell's full width, and the part of its band left below the collapsed
/// content.
///
/// Taking the width from the cell rather than from the scrollable region is
/// what keeps the block within its column, so it can never be drawn across a
/// column separator.
///
/// `None` when the cell has no usable width. Laying content out in a degenerate
/// rect would wrap it one glyph per line and feed a wildly inflated height into
/// every row offset below it.
pub(crate) fn detail_rect(cell_rect: egui::Rect, row_height: f32) -> Option<egui::Rect> {
    let rect = egui::Rect::from_x_y_ranges(
        cell_rect.x_range(),
        egui::Rangef::new(cell_rect.top() + row_height, cell_rect.bottom()),
    );

    (0.0 < rect.width()).then_some(rect)
}

/// A sticky column that hosts its rows' detail blocks, so it has to be wider
/// while one is open than it needs to be when every row is collapsed.
pub(crate) struct ExpandingColumn {
    pub(crate) collapsed_width: f32,
    pub(crate) expanded_width: f32,
    /// Narrowest the column can be dragged to while every row is collapsed. The
    /// expanded regime cannot go below `expanded_width`, which is what forces
    /// the column open.
    pub(crate) min_width: f32,
    /// Widest the column can be dragged to, in either regime.
    pub(crate) max_width: f32,
    /// egui_table remembers a column's width against its id and only ever grows
    /// it, so the two regimes carry separate ids: sharing one would leave the
    /// column at its expanded width for good once a row had been opened. The
    /// cost is that a column dragged wider than `expanded_width` narrows to it
    /// on expand, since the drag was recorded against the collapsed id.
    pub(crate) collapsed_id: &'static str,
    pub(crate) expanded_id: &'static str,
}

impl ExpandingColumn {
    /// The column, sized for whether a detail block is currently showing.
    pub(crate) fn column(&self, any_expanded: bool) -> egui_table::Column {
        let (current, min, id) = if any_expanded {
            (self.expanded_width, self.expanded_width, self.expanded_id)
        } else {
            (self.collapsed_width, self.min_width, self.collapsed_id)
        };

        egui_table::Column::new(current).range(min..=self.max_width).id(egui::Id::new(id)).resizable(true)
    }
}

/// Whether the region egui_table is currently walking is the one that puts
/// `cell_rect` on screen.
///
/// A sticky column's cells are walked once for the sticky region and once for
/// the scrollable one, and painting a detail block in both would run it twice
/// and record its height twice.
///
/// Discriminating on the cell's left edge is what egui_table does for its own
/// header groups: the sticky region's clip starts at or left of a sticky cell's
/// left edge, while the scrollable region's starts where the sticky columns end,
/// a whole column to its right. Testing the cell's centre picks the same region
/// while the panel is wide enough to show the middle of the column, but below
/// that the clip is cut short on the right and the cell lands in no region at
/// all, so the block neither paints nor measures.
pub(crate) fn cell_is_in_this_region(clip_rect: egui::Rect, cell_rect: egui::Rect) -> bool {
    clip_rect.x_range().contains(cell_rect.left())
}

/// Vertical offset of `row_nr` from the top of the table body: one default row
/// height per row above it, plus the animated extra height of the expanded ones.
pub(crate) fn row_offset(
    detail_heights: &BTreeMap<u64, f32>,
    expandedness: &BTreeMap<u64, f32>,
    row_nr: u64,
    row_height: f32,
) -> f32 {
    expandedness
        .range(0..row_nr)
        .map(|(expanded_row_nr, factor)| {
            // A row that has never been painted has no measured height yet;
            // contributing zero offset is right until it is measured.
            factor * detail_heights.get(expanded_row_nr).copied().unwrap_or(0.0)
        })
        .sum::<f32>()
        + row_nr as f32 * row_height
}

/// The encounters in which a tracked player shared your division.
///
/// Marked under both keys the encounter counts are taken with: distinct arena
/// for the all-time count, distinct timestamp for the in-range one. A tracked
/// player's `arena_ids` and `timestamps` are unpaired sets, so a mark recorded
/// under only one of them would leave the two column families disagreeing about
/// which encounters are hidden.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DivisionEncounters {
    pub(crate) arena_ids: BTreeSet<ArenaId>,
    pub(crate) timestamps: BTreeSet<Timestamp>,
}

impl DivisionEncounters {
    /// Record one encounter as a division one. Returns whether that added a
    /// mark, so a caller can tell a first marking from a re-parse of a battle
    /// already marked.
    pub(crate) fn mark(&mut self, arena_id: ArenaId, timestamp: Timestamp) -> bool {
        let arena_is_new = self.arena_ids.insert(arena_id);
        let timestamp_is_new = self.timestamps.insert(timestamp);
        arena_is_new || timestamp_is_new
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TrackedPlayer {
    pub(crate) last_name: String,
    pub(crate) db_id: AccountId,
    pub(crate) names: HashSet<String>,
    pub(crate) clan_id: i64,
    pub(crate) clan: String,
    pub(crate) timestamps: BTreeSet<Timestamp>,
    pub(crate) arena_ids: BTreeSet<ArenaId>,
    #[serde(default)]
    pub(crate) notes: String,
    /// Which of this player's encounters were division ones. Per encounter, not
    /// per account: divisioning with someone once hides those battles and
    /// leaves every other meeting with them on the tables.
    #[serde(default)]
    pub(crate) division_encounters: DivisionEncounters,
}

impl TrackedPlayer {
    /// The battles this player was met in that the division-mate toggle leaves
    /// visible, keyed by arena. Drives the all-time counts.
    pub(crate) fn visible_arena_ids(&self, show_division_mates: bool) -> impl Iterator<Item = ArenaId> + '_ {
        self.arena_ids
            .iter()
            .copied()
            .filter(move |arena_id| show_division_mates || !self.division_encounters.arena_ids.contains(arena_id))
    }

    /// The same encounters keyed by timestamp, which is what the in-range counts
    /// dedup on. Filtered against the marks recorded under the same key, so the
    /// two families always hide the same battles.
    pub(crate) fn visible_timestamps(
        &self,
        show_division_mates: bool,
    ) -> impl DoubleEndedIterator<Item = Timestamp> + '_ {
        self.timestamps
            .iter()
            .copied()
            .filter(move |timestamp| show_division_mates || !self.division_encounters.timestamps.contains(timestamp))
    }

    /// The most recent visible encounter, or `None` when every encounter with
    /// this player was a division one and the toggle is off.
    pub(crate) fn last_visible_timestamp(&self, show_division_mates: bool) -> Option<Timestamp> {
        self.visible_timestamps(show_division_mates).next_back()
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum TimePeriod {
    LastHour,
    LastSixHours,
    #[default]
    LastDay,
    LastWeek,
    LastMonth,
    AllTime,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    /// `ordering`, as computed ascending, turned to face this direction.
    pub(crate) fn direct(self, ordering: std::cmp::Ordering) -> std::cmp::Ordering {
        match self {
            SortOrder::Asc => ordering,
            SortOrder::Desc => ordering.reverse(),
        }
    }

    pub(crate) fn icon(&self) -> &'static str {
        match self {
            SortOrder::Asc => icons::SORT_ASCENDING,
            SortOrder::Desc => icons::SORT_DESCENDING,
        }
    }

    pub(crate) fn toggle(&mut self) {
        match self {
            SortOrder::Asc => *self = SortOrder::Desc,
            SortOrder::Desc => *self = SortOrder::Asc,
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SortedBy {
    Name(SortOrder),
    Clan(SortOrder),
    LastEncountered(SortOrder),
    TimesEncountered(SortOrder),
    TimesEncounteredInTimeRange(SortOrder),
}

impl SortedBy {
    /// The direction this sort is currently running in, whichever column it is on.
    pub(crate) fn order(&self) -> SortOrder {
        match self {
            SortedBy::Name(order)
            | SortedBy::Clan(order)
            | SortedBy::LastEncountered(order)
            | SortedBy::TimesEncountered(order)
            | SortedBy::TimesEncounteredInTimeRange(order) => *order,
        }
    }

    pub(crate) fn transition_to(&mut self, new: SortedBy) {
        match (self, new) {
            (SortedBy::Name(sort_order), SortedBy::Name(_)) => sort_order.toggle(),
            (SortedBy::Clan(sort_order), SortedBy::Clan(_)) => {
                sort_order.toggle();
            }
            (SortedBy::LastEncountered(sort_order), SortedBy::LastEncountered(_)) => {
                sort_order.toggle();
            }
            (SortedBy::TimesEncountered(sort_order), SortedBy::TimesEncountered(_)) => {
                sort_order.toggle();
            }
            (SortedBy::TimesEncounteredInTimeRange(sort_order), SortedBy::TimesEncounteredInTimeRange(_)) => {
                sort_order.toggle();
            }
            (old, new) => {
                *old = new;
            }
        }
    }
}

impl Default for SortedBy {
    fn default() -> Self {
        SortedBy::TimesEncounteredInTimeRange(SortOrder::Desc)
    }
}

impl TimePeriod {
    pub(crate) fn description(&self) -> String {
        match self {
            TimePeriod::LastHour => t!("ui.player_tracker.period.past_hour").into(),
            TimePeriod::LastSixHours => t!("ui.player_tracker.period.past_six_hours").into(),
            TimePeriod::LastDay => t!("ui.player_tracker.period.past_day").into(),
            TimePeriod::LastWeek => t!("ui.player_tracker.period.past_week").into(),
            TimePeriod::LastMonth => t!("ui.player_tracker.period.past_month").into(),
            TimePeriod::AllTime => t!("ui.player_tracker.period.all_time").into(),
        }
    }

    pub(crate) fn to_date(self) -> Option<Timestamp> {
        let now = Timestamp::now();
        match self {
            TimePeriod::LastHour => Some(now - 1.hour()),
            TimePeriod::LastSixHours => Some(now - 6.hours()),
            TimePeriod::LastDay => Some(now - 24.hours()),
            TimePeriod::LastWeek => Some(now - (24 * 7).hours()),
            TimePeriod::LastMonth => Some(now - (24 * 30).hours()),
            TimePeriod::AllTime => None,
        }
    }
}

impl PlayerTracker {
    /// Rebuild tracked-player aggregates from the durable replay index. Cheaper
    /// than re-parsing every replay from disk. Live updates via
    /// `update_from_replay` are unchanged and continue to layer on top.
    ///
    /// Returns `true` when the index had at least one player to populate from,
    /// so callers can fall back to re-parsing replays when the index is empty
    /// (e.g. before the first reconciliation pass has run).
    pub fn populate_from_index(&mut self, pool: &sqlx::SqlitePool, rt: &tokio::runtime::Runtime) -> bool {
        use crate::db::index::query;
        use crate::db::index::rows::MatchFilter;

        let players = match rt.block_on(query::distinct_players(pool, &MatchFilter::default())) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("player tracker: index query failed: {e}");
                return false;
            }
        };
        if players.is_empty() {
            return false;
        }

        // Never track the replay-perspective (self) account: the live path
        // (`update_from_replay`) deliberately excludes it, and it would otherwise
        // dominate the default times-encountered sort.
        let self_accounts = rt.block_on(query::self_account_ids(pool, &MatchFilter::default())).unwrap_or_default();

        for facet in players {
            if self_accounts.contains(&facet.account_id) {
                continue;
            }

            let hits = rt
                .block_on(query::matches_with_player(pool, facet.account_id, &MatchFilter::default()))
                .unwrap_or_default();

            let entry = self.tracked_players.entry(facet.account_id).or_default();
            entry.db_id = facet.account_id;
            entry.last_name = facet.latest_name;
            entry.clan = facet.clan;
            for hit in hits {
                entry.timestamps.insert(hit.timestamp);
                entry.arena_ids.insert(hit.arena_id);
            }
        }

        // The index knows every division the self player was in, which is what
        // the live path marks as it parses. Populating without this would leave
        // index-sourced encounters unmarked and the filter hiding an arbitrary
        // subset of them. Runs after the loop above, because a mark only lands
        // on a player the tracker already holds. Unconditional: the button is a
        // deliberate request to re-read the index, whatever an earlier sync
        // already found.
        self.refresh_division_mates_from_index(pool, rt);
        self.division_mates_synced = true;

        self.note_encounters_changed();
        true
    }

    /// Fold the index's division encounters into the tracked players. Returns
    /// whether any mark is new, which is what tells a caller the tables have to
    /// be rebuilt.
    ///
    /// Only accounts the tracker already holds are marked: an encounter with an
    /// untracked account has no row to hide, and creating an entry for it would
    /// invent a player the tracker never recorded. Since the self account is
    /// never tracked, that is also what keeps this path from marking it.
    fn refresh_division_mates_from_index(&mut self, pool: &sqlx::SqlitePool, rt: &tokio::runtime::Runtime) -> bool {
        use crate::db::index::query;
        use crate::db::index::rows::MatchFilter;

        let encounters = match rt.block_on(query::division_mate_encounters(pool, &MatchFilter::default())) {
            Ok(encounters) => encounters,
            Err(e) => {
                tracing::warn!("player tracker: division-mate query failed: {e}");
                return false;
            }
        };

        let mut marked_any = false;
        for encounter in encounters {
            if let Some(player) = self.tracked_players.get_mut(&encounter.account_id) {
                marked_any |= player.division_encounters.mark(encounter.arena_id, encounter.timestamp);
            }
        }
        marked_any
    }

    /// Read the index's division encounters back into the tracker, once per
    /// session.
    ///
    /// A tracker populated before encounters were marked holds them unmarked,
    /// and so does one whose live-path marking predates an account joining your
    /// division. Folding the index's answer in on the first paint corrects both
    /// without asking the user to populate again.
    pub(crate) fn sync_division_mates_from_index(&mut self, pool: &sqlx::SqlitePool, rt: &tokio::runtime::Runtime) {
        if self.division_mates_synced {
            return;
        }
        self.division_mates_synced = true;

        if self.refresh_division_mates_from_index(pool, rt) {
            // Aggregates cached over the tracker were built counting accounts
            // this just marked, so they no longer describe what is on screen.
            self.note_encounters_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use wows_replays::types::GameParamId;

    use super::*;
    use crate::db::index::query;
    use crate::db::index::rows::IndexedVehicleRow;
    use crate::db::index::rows::MatchOutcome;
    use crate::db::index::rows::ObjectiveMatch;
    use crate::db::index::rows::ReplayRecord;
    use crate::db::index::rows::VehicleRelation;

    fn build_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("failed to build test runtime")
    }

    async fn mem_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("../wows-toolkit-config/migrations").run(&pool).await.unwrap();
        pool
    }

    /// The two edges that have to coincide are built by different association
    /// orders, so they can differ by an ULP and leave the scrollable region a
    /// sub-pixel sliver of a sticky cell. The cell's left edge sits a whole
    /// column away from either, so no sliver reaches it.
    #[test]
    fn a_sticky_cell_belongs_to_exactly_one_region() {
        let cursor_x = 3.3_f32;
        let scrollable_left = (cursor_x + 70.0) + 420.0;
        let sticky_right = cursor_x + (70.0 + 420.0);
        let cell = egui::Rect::from_min_max(egui::pos2(cursor_x + 70.0, 100.0), egui::pos2(sticky_right, 210.0));

        let sticky_clip = egui::Rect::from_min_max(egui::pos2(cursor_x, 0.0), egui::pos2(sticky_right, 900.0));
        assert!(cell_is_in_this_region(sticky_clip, cell), "the region that puts the cell on screen owns it");

        let scrollable_clip = egui::Rect::from_min_max(egui::pos2(scrollable_left, 0.0), egui::pos2(1400.0, 900.0));
        assert!(
            !cell_is_in_this_region(scrollable_clip, cell),
            "the region the cell only borders on must not paint it a second time"
        );

        let sliver = egui::Rect::from_min_max(egui::pos2(sticky_right - 1e-5, 0.0), egui::pos2(1400.0, 900.0));
        assert!(
            !cell_is_in_this_region(sliver, cell),
            "a sub-pixel overlap is still the region the cell only borders on"
        );
    }

    /// egui_table hands a cell a clip already intersected with the cell's own
    /// rect, so in a panel narrower than the sticky columns the clip is cut well
    /// left of the cell's centre. The cell is still the sticky region's to paint,
    /// and a block that neither paints nor measures leaves the expand toggle
    /// doing nothing a user can see.
    #[test]
    fn a_sticky_cell_in_a_narrow_panel_still_belongs_to_its_region() {
        let cursor_x = 3.3_f32;
        let cell = egui::Rect::from_min_max(egui::pos2(cursor_x, 100.0), egui::pos2(cursor_x + 320.0, 210.0));

        let cut_clip = egui::Rect::from_min_max(egui::pos2(cursor_x, 0.0), egui::pos2(cursor_x + 150.0, 900.0));
        assert!(
            !cut_clip.x_range().contains(cell.center().x),
            "the panel is narrow enough that the cell's centre is off the clip"
        );
        assert!(cell_is_in_this_region(cut_clip, cell), "the sticky region still owns a cell it shows the left of");

        // The same cell as the scrollable region sees it: the clip collapses onto
        // the cell's right edge, a whole column away from its left.
        let collapsed =
            egui::Rect::from_min_max(egui::pos2(cell.right(), 0.0), egui::pos2(cell.right() + 900.0, 900.0));
        assert!(!cell_is_in_this_region(collapsed, cell), "the scrollable region must not paint it a second time");
    }

    #[test]
    fn populate_from_index_fills_tracker_from_seeded_matches() {
        let rt = build_runtime();
        let pool = rt.block_on(async {
            let pool = mem_pool().await;
            let now = Timestamp::from_second(1_700_000_000).unwrap();
            let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

            let objective = ObjectiveMatch {
                arena_id: ArenaId::new(100),
                timestamp: Timestamp::from_second(1_700_000_100).unwrap(),
                map: "Ocean".into(),
                game_mode: "Domination".into(),
                game_type: "pvp".into(),
                match_group: "pvp".into(),
                version_build: Some(1234),
            };
            query::upsert_match(&pool, &objective).await.unwrap();

            let enemy = IndexedVehicleRow {
                arena_id: ArenaId::new(100),
                account_id: AccountId(501),
                player_name: "Enemy".into(),
                clan: "CLAN".into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(111u64),
                ship_index: "PJSB018".into(),
                ship_name: "Yamato".into(),
                nation: "japan".into(),
                species: "Battleship".into(),
                tier: 10,
                relation: VehicleRelation::Enemy,
                division_id: None,
                survived: Some(true),
                damage: Some(50_000),
                kills: Some(1),
                spotting: Some(0),
                potential: Some(0),
                received: Some(0),
                pr: Some(1200.0),
                is_test_ship: false,
                disconnected: None,
                is_stream_sniper: None,
                sniper_twitch_login: None,
            };
            query::upsert_vehicles(&pool, &[enemy]).await.unwrap();

            let record = ReplayRecord {
                arena_id: ArenaId::new(100),
                source_id: src,
                replay_path: PathBuf::from("a.wowsreplay"),
                file_mtime: Some(1),
                outcome: MatchOutcome::Win,
                self_account_id: Some(AccountId(7)),
                self_ship_id: Some(GameParamId::from(999u64)),
                self_survived: Some(true),
                self_damage: Some(120_000),
                self_kills: Some(2),
                self_pr: Some(1500.0),
                results_available: true,
                indexed_at: now,
            };
            query::upsert_record(&pool, &record).await.unwrap();

            pool
        });

        let mut tracker = PlayerTracker::default();
        let populated = tracker.populate_from_index(&pool, &rt);
        assert!(populated, "index had a match to populate from");

        let enemy = tracker.tracked_players.get(&AccountId(501)).expect("enemy account tracked from index");
        assert_eq!(enemy.last_name, "Enemy");
        assert_eq!(enemy.clan, "CLAN");
        assert!(enemy.arena_ids.contains(&ArenaId::new(100)));
        assert_eq!(enemy.timestamps.len(), 1);
    }

    #[test]
    fn populate_from_index_reports_false_when_index_is_empty() {
        let rt = build_runtime();
        let pool = rt.block_on(mem_pool());

        let mut tracker = PlayerTracker::default();
        assert!(!tracker.populate_from_index(&pool, &rt), "empty index must not report as populated");
        assert!(tracker.tracked_players.is_empty());
    }

    #[test]
    fn populate_from_index_excludes_self_account() {
        let rt = build_runtime();
        let pool = rt.block_on(async {
            let pool = mem_pool().await;
            let now = Timestamp::from_second(1_700_000_000).unwrap();
            let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

            let objective = ObjectiveMatch {
                arena_id: ArenaId::new(100),
                timestamp: Timestamp::from_second(1_700_000_100).unwrap(),
                map: "Ocean".into(),
                game_mode: "Domination".into(),
                game_type: "pvp".into(),
                match_group: "pvp".into(),
                version_build: Some(1234),
            };
            query::upsert_match(&pool, &objective).await.unwrap();

            let self_vehicle = IndexedVehicleRow {
                arena_id: ArenaId::new(100),
                account_id: AccountId(7),
                player_name: "MyAccount".into(),
                clan: "SELF".into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(999u64),
                ship_index: "PJSD018".into(),
                ship_name: "Harugumo".into(),
                nation: "japan".into(),
                species: "Destroyer".into(),
                tier: 10,
                relation: VehicleRelation::SelfPlayer,
                division_id: None,
                survived: Some(true),
                damage: Some(120_000),
                kills: Some(2),
                spotting: Some(0),
                potential: Some(0),
                received: Some(0),
                pr: Some(1500.0),
                is_test_ship: false,
                disconnected: None,
                is_stream_sniper: None,
                sniper_twitch_login: None,
            };
            let enemy = IndexedVehicleRow {
                arena_id: ArenaId::new(100),
                account_id: AccountId(501),
                player_name: "Enemy".into(),
                clan: "CLAN".into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(111u64),
                ship_index: "PJSB018".into(),
                ship_name: "Yamato".into(),
                nation: "japan".into(),
                species: "Battleship".into(),
                tier: 10,
                relation: VehicleRelation::Enemy,
                division_id: None,
                survived: Some(true),
                damage: Some(50_000),
                kills: Some(1),
                spotting: Some(0),
                potential: Some(0),
                received: Some(0),
                pr: Some(1200.0),
                is_test_ship: false,
                disconnected: None,
                is_stream_sniper: None,
                sniper_twitch_login: None,
            };
            query::upsert_vehicles(&pool, &[self_vehicle, enemy]).await.unwrap();

            let record = ReplayRecord {
                arena_id: ArenaId::new(100),
                source_id: src,
                replay_path: PathBuf::from("a.wowsreplay"),
                file_mtime: Some(1),
                outcome: MatchOutcome::Win,
                self_account_id: Some(AccountId(7)),
                self_ship_id: Some(GameParamId::from(999u64)),
                self_survived: Some(true),
                self_damage: Some(120_000),
                self_kills: Some(2),
                self_pr: Some(1500.0),
                results_available: true,
                indexed_at: now,
            };
            query::upsert_record(&pool, &record).await.unwrap();

            pool
        });

        let mut tracker = PlayerTracker::default();
        assert!(tracker.populate_from_index(&pool, &rt), "index had a match to populate from");

        assert!(!tracker.tracked_players.contains_key(&AccountId(7)), "self-perspective account must not be tracked");
        assert!(tracker.tracked_players.contains_key(&AccountId(501)), "opponent account must still be tracked");
    }

    /// A tracker whose history predates division marking holds its encounters
    /// unmarked, and so does one upgraded from the per-account set that came
    /// before. The index knows which battles were division ones, so reading it
    /// back on the first paint corrects the filter without a fresh Populate run.
    #[test]
    fn syncing_from_the_index_marks_encounters_an_older_tracker_recorded_unmarked() {
        let rt = build_runtime();
        let pool = rt.block_on(async {
            let pool = mem_pool().await;
            let now = Timestamp::from_second(1_700_000_000).unwrap();
            let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

            let objective = ObjectiveMatch {
                arena_id: ArenaId::new(100),
                timestamp: Timestamp::from_second(1_700_000_100).unwrap(),
                map: "Ocean".into(),
                game_mode: "Domination".into(),
                game_type: "pvp".into(),
                match_group: "pvp".into(),
                version_build: Some(1234),
            };
            query::upsert_match(&pool, &objective).await.unwrap();

            let vehicle = |account: i64, relation: VehicleRelation, division: Option<i64>| IndexedVehicleRow {
                arena_id: ArenaId::new(100),
                account_id: AccountId(account),
                player_name: format!("Player{account}"),
                clan: "CLAN".into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(111u64),
                ship_index: "PJSB018".into(),
                ship_name: "Yamato".into(),
                nation: "japan".into(),
                species: "Battleship".into(),
                tier: 10,
                relation,
                division_id: division,
                survived: Some(true),
                damage: Some(50_000),
                kills: Some(1),
                spotting: Some(0),
                potential: Some(0),
                received: Some(0),
                pr: Some(1200.0),
                is_test_ship: false,
                disconnected: None,
                is_stream_sniper: None,
                sniper_twitch_login: None,
            };
            query::upsert_vehicles(
                &pool,
                &[
                    vehicle(7, VehicleRelation::SelfPlayer, Some(3)),
                    vehicle(9, VehicleRelation::Ally, Some(3)),
                    vehicle(501, VehicleRelation::Enemy, None),
                ],
            )
            .await
            .unwrap();

            let record = ReplayRecord {
                arena_id: ArenaId::new(100),
                source_id: src,
                replay_path: PathBuf::from("a.wowsreplay"),
                file_mtime: Some(1),
                outcome: MatchOutcome::Win,
                self_account_id: Some(AccountId(7)),
                self_ship_id: Some(GameParamId::from(999u64)),
                self_survived: Some(true),
                self_damage: Some(120_000),
                self_kills: Some(2),
                self_pr: Some(1500.0),
                results_available: true,
                indexed_at: now,
            };
            query::upsert_record(&pool, &record).await.unwrap();

            pool
        });

        // What an earlier Populate run left behind: both accounts tracked with
        // their encounter, and no idea which of them was a division one.
        let arena = ArenaId::new(100);
        let timestamp = Timestamp::from_second(1_700_000_100).unwrap();
        let mut tracker = PlayerTracker::default();
        for id in [AccountId(9), AccountId(501)] {
            let player = tracker.tracked_players.entry(id).or_default();
            player.db_id = id;
            player.arena_ids.insert(arena);
            player.timestamps.insert(timestamp);
        }

        let version_before = tracker.encounter_version;
        tracker.sync_division_mates_from_index(&pool, &rt);

        let mate = &tracker.tracked_players[&AccountId(9)];
        assert!(mate.division_encounters.arena_ids.contains(&arena), "the encounter is marked under the arena key");
        assert!(
            mate.division_encounters.timestamps.contains(&timestamp),
            "and under the timestamp key, or the two counts would disagree"
        );
        assert_eq!(mate.visible_arena_ids(false).count(), 0, "the division battle is all this player has");

        let opponent = &tracker.tracked_players[&AccountId(501)];
        assert_eq!(opponent.visible_arena_ids(false).count(), 1, "a solo opponent's encounter is untouched");

        assert!(!tracker.tracked_players.contains_key(&AccountId(7)), "syncing never adds the self account");
        assert!(
            version_before < tracker.encounter_version,
            "marking changes what the aggregates count, so they have to rebuild"
        );

        // One attempt per session: the query is synchronous on the UI thread.
        tracker.tracked_players.get_mut(&AccountId(9)).unwrap().division_encounters = Default::default();
        tracker.sync_division_mates_from_index(&pool, &rt);
        assert_eq!(
            tracker.tracked_players[&AccountId(9)].visible_arena_ids(false).count(),
            1,
            "a second sync in the same session must not re-run the query"
        );
    }

    /// The Populate button re-reads the index whatever an earlier sync found,
    /// and still refuses to track the self account.
    #[test]
    fn populating_marks_division_encounters_and_still_excludes_self() {
        let rt = build_runtime();
        let pool = rt.block_on(async {
            let pool = mem_pool().await;
            let now = Timestamp::from_second(1_700_000_000).unwrap();
            let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

            let objective = ObjectiveMatch {
                arena_id: ArenaId::new(100),
                timestamp: Timestamp::from_second(1_700_000_100).unwrap(),
                map: "Ocean".into(),
                game_mode: "Domination".into(),
                game_type: "pvp".into(),
                match_group: "pvp".into(),
                version_build: Some(1234),
            };
            query::upsert_match(&pool, &objective).await.unwrap();

            let vehicle = |account: i64, relation: VehicleRelation, division: Option<i64>| IndexedVehicleRow {
                arena_id: ArenaId::new(100),
                account_id: AccountId(account),
                player_name: format!("Player{account}"),
                clan: "CLAN".into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(111u64),
                ship_index: "PJSB018".into(),
                ship_name: "Yamato".into(),
                nation: "japan".into(),
                species: "Battleship".into(),
                tier: 10,
                relation,
                division_id: division,
                survived: Some(true),
                damage: Some(50_000),
                kills: Some(1),
                spotting: Some(0),
                potential: Some(0),
                received: Some(0),
                pr: Some(1200.0),
                is_test_ship: false,
                disconnected: None,
                is_stream_sniper: None,
                sniper_twitch_login: None,
            };
            query::upsert_vehicles(
                &pool,
                &[
                    vehicle(7, VehicleRelation::SelfPlayer, Some(3)),
                    vehicle(9, VehicleRelation::Ally, Some(3)),
                    vehicle(501, VehicleRelation::Enemy, None),
                ],
            )
            .await
            .unwrap();

            let record = ReplayRecord {
                arena_id: ArenaId::new(100),
                source_id: src,
                replay_path: PathBuf::from("a.wowsreplay"),
                file_mtime: Some(1),
                outcome: MatchOutcome::Win,
                self_account_id: Some(AccountId(7)),
                self_ship_id: Some(GameParamId::from(999u64)),
                self_survived: Some(true),
                self_damage: Some(120_000),
                self_kills: Some(2),
                self_pr: Some(1500.0),
                results_available: true,
                indexed_at: now,
            };
            query::upsert_record(&pool, &record).await.unwrap();

            pool
        });

        // An earlier sync in this session must not stop the button working.
        let mut tracker = PlayerTracker { division_mates_synced: true, ..Default::default() };

        assert!(tracker.populate_from_index(&pool, &rt), "index had a match to populate from");

        let mate = tracker.tracked_players.get(&AccountId(9)).expect("a division mate is recorded, not dropped");
        assert_eq!(mate.visible_arena_ids(false).count(), 0, "and its encounter is marked, so the filter can hide it");
        assert_eq!(mate.visible_arena_ids(true).count(), 1, "the toggle brings that encounter back");

        assert!(!tracker.tracked_players.contains_key(&AccountId(7)), "the self account is still never tracked");

        let opponent = tracker.tracked_players.get(&AccountId(501)).expect("a solo opponent is tracked");
        assert_eq!(opponent.visible_arena_ids(false).count(), 1, "and nothing about them is a division encounter");
    }
}
