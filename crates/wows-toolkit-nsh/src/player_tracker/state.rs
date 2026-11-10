//! What the tracker holds between requests, and the tables it draws from it.

use std::collections::HashMap;

use jiff::Timestamp;
use wows_replays::types::AccountId;
use wows_toolkit_config::index::rows::ClanCorrection;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_viewmodel::formatting::separate_number;
use wows_toolkit_viewmodel::player_tracker::ClanSortColumn;
use wows_toolkit_viewmodel::player_tracker::SortColumn;
use wows_toolkit_viewmodel::player_tracker::SortOrder;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::player_tracker::TrackerSettings;
use wows_toolkit_viewmodel::player_tracker::clans::build_clan_breakdown;
use wows_toolkit_viewmodel::player_tracker::clans::visible_clans;
use wows_toolkit_viewmodel::player_tracker::encounter_severity;
use wows_toolkit_viewmodel::player_tracker::history;
use wows_toolkit_viewmodel::player_tracker::live::CurrentMatchViewMode;
use wows_toolkit_viewmodel::player_tracker::live::WinRateMode;
use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;
use wows_toolkit_viewmodel::player_tracker::tracked::ViewModes;
use wows_toolkit_viewmodel::player_tracker::visible_player_rows;

use crate::text::text;
use crate::text::text_with;

use super::view::ClanColumnHeader;
use super::view::ClanMemberView;
use super::view::ClanRowView;
use super::view::ClansTable;
use super::view::PeriodOption;
use super::view::PlayerColumnHeader;
use super::view::PlayerDetailView;
use super::view::PlayerRowView;
use super::view::PlayersTable;
use super::view::ScopeOption;
use super::view::TrackerControls;
use super::view::TrackerLabels;
use super::view::ViewModeOption;

/// Counts index reads, so a slower earlier one cannot land over a later.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexGeneration(u64);

impl IndexGeneration {
    pub fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// Counts edits of the tracked rows made here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrackedRevision(u64);

impl TrackedRevision {
    pub fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// Where one of the tables is in reading the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    /// Not asked for yet.
    Idle,
    Loading,
    Failed(String),
    Loaded,
}

/// The tracker as both tables and the roster read it.
pub struct Tracker {
    pub settings: TrackerSettings,
    pub modes: ViewModes,
    /// Everyone the index returned for the current period, unfiltered.
    pub players: Vec<PlayerFacet>,
    /// Notes and encounters kept against the players met, shared with the
    /// other front ends through the `tracked_player*` tables.
    pub tracked: HashMap<AccountId, TrackedPlayer>,
    /// The index's latest clan per account, and the encounters whose clan at
    /// the time differed.
    pub clan_latest: HashMap<AccountId, String>,
    pub clan_corrections: Vec<ClanCorrection>,
    pub index: LoadState,
    pub clans: LoadState,
    /// Why the last note did not save, or why the notes could not be read.
    pub note_error: Option<String>,
    /// Bumped per index read so a slower earlier one cannot land over a later.
    pub generation: IndexGeneration,
    /// Bumped whenever `tracked` is edited here, so a read of the stored rows
    /// taken before the edit does not put the old rows back.
    pub tracked_revision: TrackedRevision,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    pub fn new() -> Self {
        Self {
            settings: TrackerSettings::default(),
            modes: ViewModes::default(),
            players: Vec::new(),
            tracked: HashMap::new(),
            clan_latest: HashMap::new(),
            clan_corrections: Vec::new(),
            index: LoadState::Idle,
            clans: LoadState::Idle,
            note_error: None,
            generation: IndexGeneration::default(),
            tracked_revision: TrackedRevision::default(),
        }
    }

    pub fn labels(&self) -> TrackerLabels {
        TrackerLabels {
            players_tab: text("ui.player_tracker.subtab_players"),
            current_match_tab: text("ui.player_tracker.subtab_current_match"),
            clans_tab: text("ui.player_tracker.subtab_clans"),
            time_period: text("ui.player_tracker.time_period"),
            filter_hint: text("ui.player_tracker.filter_hint"),
            populate: text("ui.player_tracker.populate_from_replays"),
            clear: text("ui.player_tracker.clear_stats"),
            clear_confirm: text("confirm.clear_all_session_stats"),
            cancel: text("ui.buttons.cancel"),
            show_division_mates: text("ui.player_tracker.show_division_mates"),
            show_division_mates_hover: text("ui.player_tracker.show_division_mates_hover"),
            find_matches: text("ui.player_tracker.find_matches"),
            find_clan_matches: text("ui.player_tracker.find_clan_matches"),
            notes_hint: text("ui.player_tracker.notes_hint"),
            copy_account_id: text("ui.player_tracker.copy_wg_id"),
            allies: text("ui.player_tracker.allies"),
            enemies: text("ui.player_tracker.enemies"),
            player_column: text("ui.player_tracker.column.player_name"),
            ship_column: text("ui.player_tracker.win_rate_ship"),
            seen_column: text("ui.player_tracker.column.encounters"),
            open_wows_numbers: text("ui.player_tracker.open_wows_numbers"),
            open_shipbuilds: text("ui.player_tracker.open_shipbuilds"),
            player_filter: text("ui.player_tracker.player_filter"),
            loading_players: text("ui.player_tracker.loading_players"),
            loading_clans: text("ui.player_tracker.loading_clans"),
            copied_account_id: text("ui.player_tracker.copied_wg_id"),
            copied_login: text("ui.twitch.copied"),
        }
    }

    pub fn controls(&self) -> TrackerControls {
        let sort = self.settings.sort;
        let clan_sort = self.settings.clan_sort;

        TrackerControls {
            period: self.settings.period.into(),
            periods: TimePeriod::ALL
                .iter()
                .map(|period| PeriodOption { period: (*period).into(), label: text(period.label_key()) })
                .collect(),
            filter: self.settings.filter.clone(),
            show_division_mates: self.modes.show_division_mates,
            view_mode: self.modes.current_match_view_mode.into(),
            scope: self.modes.win_rate_mode.into(),
            view_modes: CurrentMatchViewMode::ALL
                .iter()
                .map(|mode| ViewModeOption { mode: (*mode).into(), label: text(mode.label_key()) })
                .collect(),
            scopes: WinRateMode::ALL
                .iter()
                .map(|mode| ScopeOption { scope: (*mode).into(), label: text(mode.label_key()) })
                .collect(),
            player_columns: SortColumn::ALL
                .iter()
                .map(|column| PlayerColumnHeader {
                    column: (*column).into(),
                    label: text(column.label_key()),
                    active: sort.column == *column,
                    descending: sort.order == SortOrder::Descending,
                })
                .collect(),
            clan_columns: ClanSortColumn::ALL
                .iter()
                .map(|column| ClanColumnHeader {
                    column: (*column).into(),
                    label: text(column.label_key()),
                    active: clan_sort.column == *column,
                    descending: clan_sort.order == SortOrder::Descending,
                })
                .collect(),
            note_error: self
                .note_error
                .as_ref()
                .map(|reason| text_with("ui.player_tracker.note_not_saved", &[("reason", reason)])),
        }
    }

    /// The players table: the indexed players joined to what the tracker
    /// recorded about the same accounts, filtered and sorted.
    pub fn players_table(&self, now: Timestamp) -> PlayersTable {
        let show_division_mates = self.modes.show_division_mates;
        let rows = visible_player_rows(
            &self.players,
            &self.tracked,
            &self.settings.filter,
            self.settings.period.earliest(now),
            show_division_mates,
            self.settings.sort,
        );

        let rows: Vec<PlayerRowView> = rows
            .into_iter()
            .map(|row| {
                let account = row.facet.account_id;
                let tracked = self.tracked.get(&account);
                PlayerRowView {
                    account_id: account.into(),
                    name: row.facet.latest_name,
                    clan: row.facet.clan,
                    total_encounters: count_text(row.total_encounters),
                    encounters_in_range: separate_number(row.encounters_in_range as i64, None),
                    severity: encounter_severity(row.encounters_in_range).into(),
                    last_seen: history::last_seen_text(row.last_seen, now),
                    last_seen_exact: history::last_seen_timestamp_text(row.last_seen),
                    // An untracked player has no note; the editor shows it empty.
                    note: tracked.map(|player| player.notes.clone()).unwrap_or_default(),
                    detail: tracked.map(|player| self.player_detail(player)),
                }
            })
            .collect();

        let status = match &self.index {
            LoadState::Idle => Some(text("ui.player_tracker.waiting_for_index")),
            LoadState::Loading => Some(text("ui.player_tracker.loading_players")),
            LoadState::Failed(reason) => {
                Some(text_with("ui.player_tracker.index_failed", &[("reason", reason)]))
            }
            LoadState::Loaded if rows.is_empty() => Some(text("ui.player_tracker.no_players")),
            LoadState::Loaded => None,
        };

        PlayersTable { rows, status }
    }

    fn player_detail(&self, player: &TrackedPlayer) -> PlayerDetailView {
        let detail = history::player_detail(player, self.modes.show_division_mates);
        let count = detail.total_encounters.to_string();

        PlayerDetailView {
            aliases: (!detail.aliases.is_empty()).then(|| {
                text_with("ui.player_tracker.aliases_hover", &[("names", &detail.aliases.join(", "))])
            }),
            last_encountered: detail.last_seen.map(|last_seen| {
                let timestamp = history::exact_timestamp_text(last_seen);
                text_with("ui.player_tracker.last_encountered_exact", &[("timestamp", &timestamp)])
            }),
            match_count: text_with("ui.player_tracker.arena_count", &[("count", &count)]),
        }
    }

    /// The clans table, counted over the tracked encounters so it follows the
    /// division-mate toggle the way the players table does.
    pub fn clans_table(&self, now: Timestamp) -> ClansTable {
        let rows = build_clan_breakdown(
            &self.tracked,
            &self.clan_latest,
            &self.clan_corrections,
            self.settings.period.earliest(now),
            self.modes.show_division_mates,
        );
        let rows = visible_clans(rows, &self.settings.filter, self.settings.clan_sort);

        let rows: Vec<ClanRowView> = rows
            .into_iter()
            .map(|row| {
                let in_range = row.sightings_in_range.to_string();
                ClanRowView {
                    severity: encounter_severity(row.matches_in_range).into(),
                    members: row.members.len().to_string(),
                    matches: separate_number(row.matches as i64, None),
                    matches_in_range: separate_number(row.matches_in_range as i64, None),
                    sightings: separate_number(row.sightings as i64, None),
                    sightings_hover: text_with("ui.player_tracker.clan_sightings_hover", &[("range", &in_range)]),
                    last_seen: history::relative_age_text(row.last_seen, now),
                    last_seen_exact: history::exact_timestamp_text(row.last_seen),
                    member_list: row
                        .members
                        .iter()
                        .map(|(account, matches)| ClanMemberView {
                            account_id: (*account).into(),
                            // A member the tracker recorded under no name is
                            // still worth a row: the account id is what the
                            // search needs.
                            name: self
                                .tracked
                                .get(account)
                                .map(|player| player.last_name.clone())
                                .filter(|name| !name.is_empty())
                                .unwrap_or_else(|| account.0.to_string()),
                            matches: text_with(
                                "ui.player_tracker.clan_member_matches",
                                &[("count", &matches.to_string())],
                            ),
                        })
                        .collect(),
                    clan: row.clan,
                }
            })
            .collect();

        let status = match &self.clans {
            LoadState::Idle => Some(text("ui.player_tracker.waiting_for_index")),
            LoadState::Loading => Some(text("ui.player_tracker.loading_clans")),
            LoadState::Failed(reason) => {
                Some(text_with("ui.player_tracker.index_failed", &[("reason", reason)]))
            }
            LoadState::Loaded if rows.is_empty() => Some(text("ui.player_tracker.clan_no_data")),
            LoadState::Loaded => None,
        };

        ClansTable { rows, status, needs_load: self.clans == LoadState::Idle }
    }

    /// Records `note` against `account`, returning the row to write, or
    /// `None` when nothing changed.
    pub fn set_note(&mut self, account: AccountId, note: String) -> Option<TrackedPlayer> {
        // An empty note about a player nothing else is recorded for is not an
        // entry worth creating.
        if note.is_empty() && !self.tracked.contains_key(&account) {
            return None;
        }

        // The name the index knows, so a note written here is legible in the
        // egui tab's own list, which keys its rows on the stored name.
        let facet = self.players.iter().find(|facet| facet.account_id == account);
        // The stored rows hold an unknown name or clan as empty, as the other
        // front ends write them.
        let entry = self.tracked.entry(account).or_insert_with(|| TrackedPlayer {
            db_id: account,
            last_name: facet.map(|facet| facet.latest_name.clone()).unwrap_or_default(),
            clan: facet.map(|facet| facet.clan.clone()).unwrap_or_default(),
            ..TrackedPlayer::default()
        });
        if entry.notes == note {
            return None;
        }
        entry.notes = note;
        let written = entry.clone();
        self.tracked_revision = self.tracked_revision.next();

        Some(written)
    }

    /// Forgets every tracked player.
    pub fn clear_tracked(&mut self) {
        self.tracked.clear();
        self.tracked_revision = self.tracked_revision.next();
    }

    /// Installs the stored rows a read taken at `read_at` returned, unless
    /// the rows were edited here since. Returns whether they were installed.
    pub fn install_tracked(&mut self, read_at: TrackedRevision, tracked: HashMap<AccountId, TrackedPlayer>) -> bool {
        if self.tracked_revision != read_at {
            return false;
        }
        self.tracked = tracked;

        true
    }
}

/// A count the tracker has no record of reads as a dash rather than as zero,
/// which would claim the player was never met.
fn count_text(count: Option<usize>) -> String {
    match count {
        Some(count) => separate_number(count as i64, None),
        None => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use jiff::ToSpan as _;
    use wows_replays::types::ArenaId;

    use super::*;

    fn facet(account: i64, name: &str, clan: &str, matches: i64) -> PlayerFacet {
        PlayerFacet { account_id: AccountId(account), latest_name: name.to_string(), clan: clan.to_string(), match_count: matches }
    }

    fn tracked(name: &str, clan: &str, encounters: &[(i64, Timestamp, bool)]) -> TrackedPlayer {
        let mut player = TrackedPlayer {
            last_name: name.to_string(),
            clan: clan.to_string(),
            names: [name.to_string()].into_iter().collect(),
            ..TrackedPlayer::default()
        };
        for (arena, timestamp, division) in encounters {
            let arena = ArenaId::from(*arena);
            player.arena_ids.insert(arena);
            player.timestamps.insert(*timestamp);
            if *division {
                player.division_encounters.mark(arena, *timestamp);
            }
        }
        player
    }

    fn loaded() -> Tracker {
        let mut tracker = Tracker::new();
        tracker.index = LoadState::Loaded;
        tracker.clans = LoadState::Loaded;
        tracker
    }

    #[test]
    fn a_player_the_tracker_never_recorded_has_no_total_and_no_detail() {
        let mut tracker = loaded();
        tracker.players = vec![facet(1, "Stranger", "", 2)];

        let table = tracker.players_table(Timestamp::now());
        let row = &table.rows[0];
        assert_eq!(row.total_encounters, "-");
        assert_eq!(row.encounters_in_range, "2");
        assert!(row.detail.is_none());
        assert!(table.status.is_none());
    }

    #[test]
    fn an_opened_row_names_the_aliases_and_the_match_count() {
        let now = Timestamp::now();
        let mut tracker = loaded();
        tracker.players = vec![facet(7, "Harvey635", "WTK", 3)];
        let mut player = tracked("Harvey635", "WTK", &[(1, now - 1.hour(), false)]);
        player.names.insert("Harvey42".to_string());
        tracker.tracked.insert(AccountId(7), player);

        let detail = tracker.players_table(now).rows[0].detail.clone().expect("a tracked player has a detail");
        assert_eq!(detail.aliases.as_deref(), Some("Also known as: Harvey42"));
        assert_eq!(detail.match_count, "1 matches recorded");
        assert!(detail.last_encountered.is_some());
    }

    #[test]
    fn the_division_toggle_hides_players_met_only_in_division() {
        let now = Timestamp::now();
        let mut tracker = loaded();
        tracker.players = vec![facet(1, "Mate", "", 1), facet(2, "Foe", "", 1)];
        tracker.tracked.insert(AccountId(1), tracked("Mate", "", &[(1, now - 1.hour(), true)]));
        tracker.tracked.insert(AccountId(2), tracked("Foe", "", &[(1, now - 1.hour(), false)]));

        let names = |tracker: &Tracker| -> Vec<String> {
            tracker.players_table(now).rows.into_iter().map(|row| row.name).collect()
        };
        assert_eq!(names(&tracker), vec!["Foe".to_string()]);

        tracker.modes.show_division_mates = true;
        assert_eq!(names(&tracker).len(), 2);
    }

    #[test]
    fn an_empty_loaded_table_says_so_and_an_unread_one_says_it_waits() {
        let mut tracker = Tracker::new();
        assert_eq!(tracker.players_table(Timestamp::now()).status.as_deref(), Some("Waiting for the replay index"));
        assert!(tracker.clans_table(Timestamp::now()).needs_load);

        tracker.index = LoadState::Loaded;
        assert_eq!(
            tracker.players_table(Timestamp::now()).status.as_deref(),
            Some("No players indexed for this period")
        );
    }

    #[test]
    fn the_clans_table_counts_tracked_encounters_and_lists_members() {
        let now = Timestamp::now();
        let mut tracker = loaded();
        tracker.tracked.insert(
            AccountId(1),
            tracked("Alpha", "RAIN", &[(1, now - 1.hour(), false), (2, now - 2.hours(), false)]),
        );
        tracker.tracked.insert(AccountId(2), tracked("Beta", "RAIN", &[(1, now - 1.hour(), false)]));

        let table = tracker.clans_table(now);
        let row = &table.rows[0];
        assert_eq!(row.clan, "RAIN");
        assert_eq!(row.members, "2");
        assert_eq!(row.matches, "2");
        assert_eq!(row.sightings, "3");
        assert_eq!(row.member_list[0].name, "Alpha");
        assert_eq!(row.member_list[0].matches, "seen in 2 matches");
        assert_eq!(row.severity, super::super::view::Severity::Noted);
    }

    /// Rows read before a note or a clear do not put the old rows back; rows
    /// read after them do.
    #[test]
    fn rows_read_before_an_edit_are_not_installed_over_it() {
        let mut tracker = loaded();
        let stored: HashMap<AccountId, TrackedPlayer> =
            [(AccountId(9), TrackedPlayer { notes: "old".to_string(), ..TrackedPlayer::default() })].into();

        let read_at = tracker.tracked_revision;
        tracker.set_note(AccountId(9), "new".to_string()).expect("the note changed");
        assert!(!tracker.install_tracked(read_at, stored.clone()), "a read from before the note is stale");
        assert_eq!(tracker.tracked[&AccountId(9)].notes, "new");

        let read_at = tracker.tracked_revision;
        tracker.clear_tracked();
        assert!(!tracker.install_tracked(read_at, stored.clone()), "a read from before the clear is stale");
        assert!(tracker.tracked.is_empty());

        let read_at = tracker.tracked_revision;
        assert!(tracker.install_tracked(read_at, stored));
        assert_eq!(tracker.tracked[&AccountId(9)].notes, "old");
    }

    #[test]
    fn a_note_creates_an_entry_named_as_the_index_names_the_player() {
        let mut tracker = loaded();
        tracker.players = vec![facet(9, "Indexed", "TAG", 1)];

        assert!(tracker.set_note(AccountId(9), String::new()).is_none(), "an empty note creates nothing");
        let written = tracker.set_note(AccountId(9), "flanks early".to_string()).expect("a note is written");
        assert_eq!(written.last_name, "Indexed");
        assert_eq!(written.clan, "TAG");
        assert!(tracker.set_note(AccountId(9), "flanks early".to_string()).is_none(), "an unchanged note is not");
    }
}
