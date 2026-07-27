mod clans;
mod current_match;
mod historical;
mod live;
mod model;

pub(crate) use clans::BreakdownWindow;
pub(crate) use clans::ClanBreakdown;
pub(crate) use clans::ClanSortedBy;
pub(crate) use model::ExpandingColumn;
pub(crate) use model::SortOrder;
pub(crate) use model::SortedBy;
pub use model::TimePeriod;
pub use model::TrackedPlayer;
pub(crate) use model::cell_is_in_this_region;
pub(crate) use model::detail_rect;
pub(crate) use model::encounter_severity_color;
pub(crate) use model::encounters_in_range;
pub(crate) use model::exact_timestamp_text;
pub(crate) use model::expanded_rows;
pub(crate) use model::last_seen_text;
pub(crate) use model::last_seen_timestamp_text;
pub(crate) use model::relative_age_text;
pub(crate) use model::row_offset;
pub(crate) use model::sort_header_label;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

use jiff::Timestamp;
use rust_i18n::t;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::Rc;
use wows_replays::ReplayMeta;
use wows_replays::analyzer::decoder::PlayerStateData;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;

use crate::app::ToolkitTabViewer;
use crate::data::wows_data::WorldOfWarshipsData;
use crate::ui::replay_parser::Replay;
use crate::util;

use live::LiveMatch;
use live::ResolvedRoster;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayerTracker {
    pub(crate) tracked_players: HashMap<AccountId, TrackedPlayer>,
    pub filter_time_period: TimePeriod,
    pub(crate) sort_order: SortedBy,
    #[serde(default)]
    pub(crate) clan_sort_order: ClanSortedBy,
    pub(crate) player_filter: String,

    /// Whether the Historical and Clans tables count the encounters marked in
    /// each player's [`TrackedPlayer::division_encounters`]. Shared by both
    /// sub-tabs so the two cannot disagree.
    #[serde(default)]
    pub show_division_mates: bool,

    /// Whether the index has already been asked for its division encounters. One
    /// attempt per session: the query is synchronous on the UI thread, and a
    /// failed one that retried would run on every frame.
    #[serde(skip)]
    pub(crate) division_mates_synced: bool,

    /// Measured height of each open historical row's detail block, keyed by row
    /// number, so `egui_table` can stretch those rows past the default height.
    #[serde(skip)]
    pub(crate) historical_detail_heights: BTreeMap<u64, f32>,

    /// Accounts whose detail row is open. Keyed by account rather than row index
    /// so a sort or filter change does not move the open row onto another player.
    #[serde(skip)]
    pub(crate) expanded_players: HashSet<AccountId>,

    /// Same role as `historical_detail_heights`, for the clans table.
    #[serde(skip)]
    pub(crate) clan_detail_heights: BTreeMap<u64, f32>,

    /// Clans whose member list is open, keyed by tag for the same reason
    /// `expanded_players` keys by account.
    #[serde(skip)]
    pub(crate) expanded_clans: HashSet<String>,

    /// Bumped whenever encounters are added to or removed from
    /// `tracked_players`. A caller caching an aggregate over the tracker holds
    /// the value it built against and rebuilds when it moves. Skipped rather
    /// than persisted so it resets together with the caches keyed on it, which
    /// are themselves `serde(skip)`: a restored counter paired with an empty
    /// cache would carry no more information than a zeroed one.
    #[serde(skip)]
    pub(crate) encounter_version: u64,

    /// Clan aggregates, rebuilt only when their inputs change: the index queries
    /// behind them run synchronously on the UI thread.
    #[serde(skip)]
    pub(crate) clan_breakdown: Option<ClanBreakdown>,

    /// The window `clan_breakdown` was built for, so a period change or the
    /// passage of wall-clock time rebuilds it.
    #[serde(skip)]
    pub(crate) clan_breakdown_window: Option<BreakdownWindow>,

    #[serde(skip)]
    pub(crate) live_match: Option<LiveMatch>,
    #[serde(skip)]
    pub(crate) resolved_roster: Option<ResolvedRoster>,
}

impl PlayerTracker {
    pub fn update_from_live_arena_info(&mut self, meta: &ReplayMeta) {
        self.live_match = Some(LiveMatch::from_meta(meta));
        self.resolved_roster = None;
    }

    /// The current roster, resolved against `wows_data` and tracked history.
    /// Rebuilds when the match changes, when game data arrives after a roster was
    /// resolved without it, or when the tracked-player set changes.
    pub(crate) fn roster(&mut self, wows_data: Option<&WorldOfWarshipsData>) -> Option<&ResolvedRoster> {
        let live = self.live_match.as_ref()?;

        // The game-data test must match what `ships_resolved` records: params
        // present, not merely a loaded build. A build whose params failed to
        // load would otherwise look perpetually stale and re-resolve every frame.
        let metadata_available = wows_data.and_then(|data| data.game_metadata.as_ref()).is_some();

        let stale = match self.resolved_roster.as_ref() {
            Some(resolved) => {
                resolved.started_at != live.started_at
                    || (!resolved.ships_resolved && metadata_available)
                    || resolved.tracked_count != self.tracked_players.len()
            }
            None => true,
        };

        if stale {
            self.resolved_roster = Some(live::resolve_roster(live, &self.tracked_players, wows_data));
        }

        self.resolved_roster.as_ref()
    }

    /// Record that the tracked encounter set changed, so aggregates cached over
    /// it rebuild. Saturating because a wrapped counter could land back on the
    /// value a stale cache holds.
    pub(crate) fn note_encounters_changed(&mut self) {
        self.encounter_version = self.encounter_version.saturating_add(1);
    }

    pub fn update_from_replay(&mut self, replay: &Replay) {
        let Some(report) = replay.battle_report.as_ref() else {
            return;
        };

        let timestamp = util::replay_timestamp(&replay.replay_file.meta);

        let self_player = report.players().iter().find(|player| {
            replay
                .replay_file
                .meta
                .vehicles
                .iter()
                .find(|metadata_player| metadata_player.name == player.initial_state().username())
                .is_some_and(|meta_player| meta_player.relation == 0)
        });

        self.ingest_roster(
            report.players(),
            self_player,
            |player| player.initial_state(),
            report.arena_id(),
            timestamp,
        );
    }

    /// Fold one battle's roster into the tracker.
    ///
    /// Generic over the roster's element type so the self guard can be tested
    /// without a parsed replay: that guard is identity-based, and pointer
    /// identity is the only thing separating the recording player from a roster
    /// entry that reports the same account.
    fn ingest_roster<P>(
        &mut self,
        players: &[Rc<P>],
        self_player: Option<&Rc<P>>,
        state_of: impl Fn(&P) -> &PlayerStateData,
        arena_id: ArenaId,
        timestamp: Timestamp,
    ) {
        let tracked_players = &mut self.tracked_players;
        let mut ingested_any = false;
        let mut marked_any = false;

        for player in players {
            let player_state = state_of(player);

            // Skip bots
            if player_state.is_bot() {
                continue;
            }

            let mut is_division_mate = false;
            if let Some(self_player) = self_player {
                // Ignore ourselves
                if Rc::ptr_eq(self_player, player) {
                    continue;
                }
                is_division_mate = state_of(self_player).is_division_mate(player_state);
            }

            let tracked_player = tracked_players.entry(player_state.db_id()).or_default();

            // Division encounters are recorded like any other and marked, so the
            // tables can hide this battle without losing the ones where the same
            // player was an opponent. Marked before the arena guard below: a
            // re-parse of a battle already ingested is still the first chance to
            // mark it.
            if is_division_mate {
                marked_any |= tracked_player.division_encounters.mark(arena_id, timestamp);
            }

            if tracked_player.arena_ids.contains(&arena_id) {
                continue;
            }
            // Past that guard this battle is new for the player, whether or not
            // the player themselves is.
            ingested_any = true;

            let mut update_metadata = false;

            if let Some(last_seen) = tracked_player.timestamps.first()
                && *last_seen < timestamp
            {
                update_metadata = true;
            }

            if update_metadata || tracked_player.timestamps.is_empty() {
                if update_metadata
                    && !tracked_player.names.contains(&tracked_player.last_name)
                    && tracked_player.last_name != player_state.username()
                    && !tracked_player.last_name.is_empty()
                {
                    // If we need to update the name, let's add the name to the alias list
                    tracked_player.names.insert(tracked_player.last_name.clone());
                }

                tracked_player.last_name = player_state.username().to_string();

                tracked_player.clan = player_state.clan().to_string();
            }

            tracked_player.db_id = player_state.db_id();
            tracked_player.clan_id = player_state.clan_id();
            tracked_player.timestamps.insert(timestamp);
            tracked_player.arena_ids.insert(arena_id);
        }

        // A fresh mark changes which encounters the tables count just as much as
        // a fresh encounter does, so the aggregates cached over the tracker have
        // to rebuild for either.
        if ingested_any || marked_any {
            self.note_encounters_changed();
        }
    }
}

/// The Player Tracker's views. None is closeable: closing one would leave no
/// way to get it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerTrackerSubTab {
    Historical,
    CurrentMatch,
    Clans,
}

/// Whether a deserialized layout has lost a sub-tab, either from a corrupt row
/// or from an older layout that predates a variant.
pub(crate) fn player_tracker_dock_needs_repair(dock: &egui_dock::DockState<PlayerTrackerSubTab>) -> bool {
    let has_historical = dock.iter_all_tabs().any(|(_, tab)| matches!(tab, PlayerTrackerSubTab::Historical));
    let has_current_match = dock.iter_all_tabs().any(|(_, tab)| matches!(tab, PlayerTrackerSubTab::CurrentMatch));
    let has_clans = dock.iter_all_tabs().any(|(_, tab)| matches!(tab, PlayerTrackerSubTab::Clans));

    !has_historical || !has_current_match || !has_clans
}

struct PlayerTrackerSubTabViewer<'a, 'b> {
    tab_viewer: &'a mut ToolkitTabViewer<'b>,
}

impl egui_dock::TabViewer for PlayerTrackerSubTabViewer<'_, '_> {
    type Tab = PlayerTrackerSubTab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("player_tracker_sub_tab", *tab))
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        let key = match tab {
            PlayerTrackerSubTab::Historical => "ui.player_tracker.subtab_historical",
            PlayerTrackerSubTab::CurrentMatch => "ui.player_tracker.subtab_current_match",
            PlayerTrackerSubTab::Clans => "ui.player_tracker.subtab_clans",
        };
        t!(key).to_string().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            PlayerTrackerSubTab::Historical => self.tab_viewer.build_historical_sub_tab(ui),
            PlayerTrackerSubTab::CurrentMatch => self.tab_viewer.build_current_match_sub_tab(ui),
            PlayerTrackerSubTab::Clans => self.tab_viewer.build_clans_sub_tab(ui),
        }
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }
}

impl ToolkitTabViewer<'_> {
    pub fn build_player_tracker_tab(&mut self, ui: &mut egui::Ui) {
        let needs_repair = {
            let p = self.tab_state.persisted.read();
            player_tracker_dock_needs_repair(&p.player_tracker_dock_state)
        };
        if needs_repair {
            self.tab_state.persisted.write().player_tracker_dock_state =
                crate::tab_state::default_player_tracker_dock_state();
        }

        // Moved out so the sub-tab bodies can take their own locks on
        // `persisted`. Untracked in both directions: taking the layout out and
        // putting it back leaves the persisted content as it was, and marking it
        // dirty every frame would re-serialize the whole tracker on the save
        // task's timer for as long as this tab is open.
        let mut dock_state = std::mem::replace(
            &mut self.tab_state.persisted.write_untracked().player_tracker_dock_state,
            egui_dock::DockState::new(vec![]),
        );
        let layout_before = crate::tab_state::dock_layout_fingerprint(&dock_state);

        let mut viewer = PlayerTrackerSubTabViewer { tab_viewer: self };

        egui_dock::DockArea::new(&mut dock_state)
            .id(egui::Id::new("player_tracker_dock"))
            .style(egui_dock::Style::from_egui(ui.style().as_ref()))
            .show_close_buttons(false)
            .show_leaf_collapse_buttons(false)
            .show_leaf_close_all_buttons(false)
            .allowed_splits(egui_dock::AllowedSplits::All)
            .show_inside(ui, &mut viewer);

        // A drag, a split or a sub-tab change moves the fingerprint, and only
        // then is the write worth saving.
        let changed = crate::tab_state::dock_layout_fingerprint(&dock_state) != layout_before;
        let mut persisted =
            if changed { self.tab_state.persisted.write() } else { self.tab_state.persisted.write_untracked() };
        persisted.player_tracker_dock_state = dock_state;
    }

    /// Queues an advanced search for every match this account appeared in and
    /// focuses the Search tab.
    pub(crate) fn queue_player_search(&mut self, id: AccountId) {
        use crate::db::index::query_model::Chip;
        use crate::db::index::query_model::Connector;
        use crate::db::index::query_model::Field;
        use crate::db::index::query_model::Group;
        use crate::db::index::query_model::Op;
        use crate::db::index::query_model::Query;
        use crate::db::index::query_model::Value;

        self.tab_state.pending_search_query = Some(Query {
            groups: vec![Group {
                chips: vec![Chip { field: Field::PlayerPresent, op: Op::Present, value: Value::Account(id) }],
            }],
            connector: Connector::And,
        });
        self.tab_state.pending_focus_search = true;
    }

    /// Queues an advanced search for matches mentioning this clan tag and
    /// focuses the Search tab. The index has no clan-only field, so this is a
    /// substring match over player name and clan alike: a tag that occurs inside
    /// someone's name matches too.
    pub(crate) fn queue_clan_search(&mut self, clan: &str) {
        use crate::db::index::query_model::Chip;
        use crate::db::index::query_model::Connector;
        use crate::db::index::query_model::Field;
        use crate::db::index::query_model::Group;
        use crate::db::index::query_model::Op;
        use crate::db::index::query_model::Query;
        use crate::db::index::query_model::Value;

        self.tab_state.pending_search_query = Some(Query {
            groups: vec![Group {
                chips: vec![Chip {
                    field: Field::PlayerNameOrClan,
                    op: Op::Contains,
                    value: Value::Text(clan.to_string()),
                }],
            }],
            connector: Connector::And,
        });
        self.tab_state.pending_focus_search = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The roster entry `ingest_roster` reads through its `state_of` accessor.
    /// `BattleReport` cannot be built outside the parser, but the ingest core is
    /// generic over the element type precisely so the guards it applies can be
    /// exercised on a hand-built roster.
    struct RosterEntry(PlayerStateData);

    /// One roster entry, deserialized because `PlayerStateData`'s fields are
    /// crate-private to the parser.
    fn roster_entry(db_id: i64, name: &str, division: i64, is_bot: bool) -> Rc<RosterEntry> {
        let state = serde_json::from_value(serde_json::json!({
            "username": name,
            "clan": "RAIN",
            "clan_id": 7,
            "clan_color": 0,
            "db_id": db_id,
            "realm": "na",
            "meta_ship_id": 0,
            "entity_id": 0,
            "team_id": 0,
            "max_health": 40_000,
            "is_abuser": false,
            "is_hidden": false,
            "is_bot": is_bot,
            "human_properties": {
                "avatar_id": 0,
                // The parser's name for the division id. Zero means no division.
                "prebattle_id": division,
                "is_client_loaded": true,
                "is_connected": true,
            },
        }))
        .expect("the roster fixture matches PlayerStateData's shape");

        Rc::new(RosterEntry(state))
    }

    fn ingest(tracker: &mut PlayerTracker, roster: &[Rc<RosterEntry>], self_index: usize, arena: i64, second: i64) {
        tracker.ingest_roster(
            roster,
            Some(&roster[self_index]),
            |entry| &entry.0,
            ArenaId::new(arena),
            Timestamp::from_second(second).expect("fixture timestamp is in range"),
        );
    }

    /// The live path's self guard is pointer identity, so it holds even against
    /// a roster entry reporting the same account, and it runs before the
    /// division marking that the self player would otherwise match on.
    #[test]
    fn the_live_path_never_records_the_recording_player() {
        let self_entry = roster_entry(7, "Me", 3, false);
        let roster = vec![
            Rc::clone(&self_entry),
            roster_entry(9, "Mate", 3, false),
            roster_entry(501, "Enemy", 0, false),
            roster_entry(0, "Bot", 0, true),
            // A second entry for the same account: only the one the report named
            // as the self player is the recording player.
            roster_entry(7, "MyOtherShip", 3, false),
        ];

        let mut tracker = PlayerTracker::default();
        ingest(&mut tracker, &roster, 0, 100, 1000);

        assert!(!tracker.tracked_players.contains_key(&AccountId(0)), "bots are not players to track");
        assert!(tracker.tracked_players.contains_key(&AccountId(9)));
        assert!(tracker.tracked_players.contains_key(&AccountId(501)));

        // The duplicate entry is recorded, but through its own identity: what
        // must never happen is the recording player's own entry landing here.
        let self_player = tracker.tracked_players.get(&AccountId(7)).expect("the duplicate entry is a roster row");
        assert_eq!(self_player.last_name, "MyOtherShip", "the recording player's own entry was skipped");
        assert_eq!(self_player.arena_ids.len(), 1, "and was not recorded a second time under the same account");
    }

    /// A division mate's battle is marked under both keys, and only that battle:
    /// meeting the same account again outside a division leaves that encounter
    /// counted.
    #[test]
    fn the_live_path_marks_the_division_battle_and_leaves_the_others() {
        let mut tracker = PlayerTracker::default();

        let divisioned = vec![roster_entry(7, "Me", 3, false), roster_entry(9, "Mate", 3, false)];
        ingest(&mut tracker, &divisioned, 0, 100, 1000);

        // The same account met again, this time in nobody's division.
        let opposed = vec![roster_entry(7, "Me", 0, false), roster_entry(9, "Mate", 0, false)];
        ingest(&mut tracker, &opposed, 0, 101, 2000);

        let mate = &tracker.tracked_players[&AccountId(9)];
        assert_eq!(mate.arena_ids.len(), 2, "both battles are recorded");
        assert_eq!(mate.visible_arena_ids(false).collect::<Vec<_>>(), vec![ArenaId::new(101)]);
        assert_eq!(
            mate.visible_timestamps(false).collect::<Vec<_>>(),
            vec![Timestamp::from_second(2000).unwrap()],
            "the timestamp key hides the same battle the arena key does"
        );
        assert_eq!(mate.visible_arena_ids(true).count(), 2, "the toggle brings the division battle back");
    }

    /// Re-parsing a battle already ingested still marks it, and the aggregates
    /// cached over the tracker have to be told: a mate marked for the first time
    /// changes what they count just as much as a new encounter does.
    #[test]
    fn marking_a_battle_already_ingested_still_bumps_the_encounter_version() {
        let mut tracker = PlayerTracker::default();

        // What an index-sourced populate leaves behind: the encounter recorded,
        // and no division marking on it.
        let solo = vec![roster_entry(7, "Me", 0, false), roster_entry(9, "Mate", 0, false)];
        ingest(&mut tracker, &solo, 0, 100, 1000);

        let version_before = tracker.encounter_version;
        let divisioned = vec![roster_entry(7, "Me", 3, false), roster_entry(9, "Mate", 3, false)];
        ingest(&mut tracker, &divisioned, 0, 100, 1000);

        assert_eq!(tracker.tracked_players[&AccountId(9)].visible_arena_ids(false).count(), 0, "the battle is marked");
        assert!(version_before < tracker.encounter_version, "a first marking has to invalidate the cached aggregates");

        // A re-parse that marks nothing new leaves the caches alone.
        let version_after = tracker.encounter_version;
        ingest(&mut tracker, &divisioned, 0, 100, 1000);
        assert_eq!(tracker.encounter_version, version_after, "an unchanged re-parse must not force a rebuild");
    }

    #[test]
    fn a_complete_dock_layout_needs_no_repair() {
        let dock = crate::tab_state::default_player_tracker_dock_state();
        assert!(!player_tracker_dock_needs_repair(&dock));
    }

    #[test]
    fn a_layout_missing_current_match_needs_repair() {
        let dock = egui_dock::DockState::new(vec![PlayerTrackerSubTab::Historical, PlayerTrackerSubTab::Clans]);
        assert!(player_tracker_dock_needs_repair(&dock));
    }

    #[test]
    fn a_layout_missing_historical_needs_repair() {
        let dock = egui_dock::DockState::new(vec![PlayerTrackerSubTab::CurrentMatch, PlayerTrackerSubTab::Clans]);
        assert!(player_tracker_dock_needs_repair(&dock));
    }

    /// A layout persisted before the Clans sub-tab existed holds exactly the two
    /// older variants, so it must fail repair and reset to the default.
    #[test]
    fn a_layout_missing_clans_needs_repair() {
        let dock = egui_dock::DockState::new(vec![PlayerTrackerSubTab::Historical, PlayerTrackerSubTab::CurrentMatch]);
        assert!(player_tracker_dock_needs_repair(&dock));
    }

    /// The tab moves its layout out of the persisted state and back every frame,
    /// and only marks that state dirty when this fingerprint moved. So it has to
    /// move for every change a user can make, and stay put for a frame that
    /// changed nothing.
    #[test]
    fn the_layout_fingerprint_moves_only_when_the_layout_does() {
        use crate::tab_state::dock_layout_fingerprint;

        let dock = crate::tab_state::default_player_tracker_dock_state();
        let untouched = dock_layout_fingerprint(&dock);

        // What the take-and-put-back does to a layout nobody touched.
        let round_tripped = std::mem::replace(&mut dock.clone(), egui_dock::DockState::new(vec![]));
        assert_eq!(dock_layout_fingerprint(&round_tripped), untouched, "a layout put back unchanged is not a change");

        let mut another_sub_tab = dock.clone();
        let (_, leaf) = another_sub_tab.iter_leaves_mut().next().expect("the default layout has a leaf");
        leaf.set_active_tab(2).expect("the default layout holds three sub-tabs");
        assert_ne!(
            dock_layout_fingerprint(&another_sub_tab),
            untouched,
            "picking another sub-tab has to be persisted, or it does not survive a restart"
        );

        let mut split = dock.clone();
        split.main_surface_mut().split_right(egui_dock::NodeIndex::root(), 0.5, vec![PlayerTrackerSubTab::Clans]);
        assert_ne!(dock_layout_fingerprint(&split), untouched, "splitting a pane has to be persisted");
    }

    #[test]
    fn the_default_layout_opens_on_historical() {
        // `find_active_focused` takes `&mut self`.
        let mut dock = crate::tab_state::default_player_tracker_dock_state();
        let (_rect, active) = dock.find_active_focused().expect("default layout has an active tab");
        assert_eq!(*active, PlayerTrackerSubTab::Historical);
    }

    /// The tracker gained fields that no saved state carries: everything new is
    /// either `serde(skip)` or `serde(default)`, and the older fields keep their
    /// names and types. This pins that, because the failure mode is a tracker
    /// that silently loads empty on the first run after an update.
    ///
    /// The payload also carries two keys the tracker no longer has fields for:
    /// the per-account `division_mates` set, which has no per-encounter reading
    /// and so is dropped rather than migrated, and `tracked_players_by_time`,
    /// whose contents are all derivable from `tracked_players`. Dropping either
    /// must not fail the whole load.
    #[test]
    fn a_payload_holding_only_the_older_fields_still_loads() {
        let saved = r#"{
            "tracked_players_by_time": { "2023-11-14T22:13:20Z": [501] },
            "tracked_players": {
                "501": {
                    "last_name": "Enemy",
                    "db_id": 501,
                    "names": ["OldHandle"],
                    "clan_id": 7,
                    "clan": "RAIN",
                    "timestamps": ["2023-11-14T22:13:20Z"],
                    "arena_ids": [100]
                }
            },
            "division_mates": [501],
            "filter_time_period": "LastWeek",
            "sort_order": { "TimesEncountered": "Desc" },
            "player_filter": "yamato"
        }"#;

        let tracker: PlayerTracker = serde_json::from_str(saved).expect("a payload without the newer fields loads");

        let timestamp = jiff::Timestamp::from_second(1_700_000_000).expect("fixture timestamp is in range");

        let player = tracker.tracked_players.get(&AccountId(501)).expect("the saved player is tracked");
        assert_eq!(player.last_name, "Enemy");
        assert_eq!(player.clan, "RAIN");
        assert_eq!(player.clan_id, 7);
        assert_eq!(player.timestamps.iter().copied().collect::<Vec<_>>(), vec![timestamp]);
        assert_eq!(player.arena_ids.len(), 1);
        assert_eq!(player.notes, "", "a player saved before notes existed loads without any");

        assert_eq!(tracker.filter_time_period, TimePeriod::LastWeek);
        assert_eq!(tracker.sort_order, SortedBy::TimesEncountered(SortOrder::Desc));
        assert_eq!(tracker.player_filter, "yamato");

        assert_eq!(tracker.clan_sort_order, ClanSortedBy::default(), "the clan sort falls back to its default");
        assert_eq!(
            player.visible_arena_ids(false).count(),
            1,
            "the retired per-account set marks nothing: only the index and the live path can, per encounter"
        );
        assert!(!tracker.show_division_mates, "the division-mate filter defaults to hiding them");
        assert!(!tracker.division_mates_synced, "a restored tracker still asks the index once");
        assert_eq!(tracker.encounter_version, 0);
        assert!(tracker.clan_breakdown.is_none());
        assert!(tracker.clan_breakdown_window.is_none());
        assert!(tracker.expanded_players.is_empty());
        assert!(tracker.expanded_clans.is_empty());
        assert!(tracker.historical_detail_heights.is_empty());
        assert!(tracker.clan_detail_heights.is_empty());
        assert!(tracker.live_match.is_none());
        assert!(tracker.resolved_roster.is_none());
    }

    /// A `division_encounters` object carrying one key and not the other has to
    /// load as the half it does carry. Without a default on each set the missing
    /// key would fail the player, and with it the whole tracker, which is the
    /// same silent-empty-load failure the test above guards.
    #[test]
    fn a_half_written_division_encounters_object_loads_the_half_it_has() {
        let saved = r#"{
            "tracked_players": {
                "501": {
                    "last_name": "Enemy",
                    "db_id": 501,
                    "names": [],
                    "clan_id": 7,
                    "clan": "RAIN",
                    "timestamps": ["2023-11-14T22:13:20Z"],
                    "arena_ids": [100],
                    "division_encounters": { "arena_ids": [100] }
                }
            },
            "filter_time_period": "LastWeek",
            "sort_order": { "TimesEncountered": "Desc" },
            "player_filter": ""
        }"#;

        let tracker: PlayerTracker =
            serde_json::from_str(saved).expect("a partial division_encounters object still loads the tracker");

        let player = tracker.tracked_players.get(&AccountId(501)).expect("the saved player is tracked");
        assert_eq!(player.visible_arena_ids(false).count(), 0, "the arena key that was written still hides its battle");
        assert_eq!(
            player.visible_timestamps(false).count(),
            1,
            "and the key that was not written hides nothing, rather than failing the load"
        );
    }
}
