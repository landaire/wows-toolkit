mod current_match;
mod historical;
mod live;
mod model;

pub(crate) use model::SortOrder;
pub(crate) use model::SortedBy;
pub use model::TimePeriod;
pub use model::TrackedPlayer;
pub(crate) use model::encounter_severity_color;
pub(crate) use model::last_seen_text;
pub(crate) use model::last_seen_timestamp_text;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use rust_i18n::t;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::ReplayMeta;
use wows_replays::types::AccountId;

use crate::app::ToolkitTabViewer;
use crate::data::wows_data::WorldOfWarshipsData;
use crate::ui::replay_parser::Replay;
use crate::util;

use live::LiveMatch;
use live::ResolvedRoster;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayerTracker {
    pub(crate) tracked_players_by_time: BTreeMap<Timestamp, Vec<AccountId>>,
    pub(crate) tracked_players: HashMap<AccountId, TrackedPlayer>,
    pub filter_time_period: TimePeriod,
    pub(crate) sort_order: SortedBy,
    pub(crate) player_filter: String,

    /// Measured heights of the historical table's rows, keyed by row number, so
    /// `egui_table` can offset rows whose content is taller than the default.
    #[serde(skip)]
    pub(crate) historical_row_heights: BTreeMap<u64, f32>,
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

    pub fn update_from_replay(&mut self, replay: &Replay) {
        let Some(report) = replay.battle_report.as_ref() else {
            return;
        };

        let tracked_players = &mut self.tracked_players;
        let tracked_players_by_ts = &mut self.tracked_players_by_time;

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

        for player in report.players() {
            let player_state = player.initial_state();

            // Skip bots
            if player_state.is_bot() {
                continue;
            }

            if let Some(self_player) = self_player {
                let self_state = self_player.initial_state();
                // Ignore ourselves and people in our division
                if Arc::ptr_eq(self_player, player)
                    || (self_state.division_id() > 0 && player_state.division_id() == self_state.division_id())
                {
                    continue;
                }
            }

            let tracked_player = tracked_players.entry(player_state.db_id()).or_default();
            if tracked_player.arena_ids.contains(&report.arena_id()) {
                continue;
            }

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
            tracked_player.arena_ids.insert(report.arena_id());

            tracked_players_by_ts.entry(timestamp).or_default().push(player_state.db_id());
        }
    }
}

/// The Player Tracker's two views. Neither is closeable: closing one would
/// leave no way to get it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerTrackerSubTab {
    Historical,
    CurrentMatch,
}

/// Whether a deserialized layout has lost a sub-tab, either from a corrupt row
/// or from an older layout that predates a variant.
pub(crate) fn player_tracker_dock_needs_repair(dock: &egui_dock::DockState<PlayerTrackerSubTab>) -> bool {
    let has_historical = dock.iter_all_tabs().any(|(_, tab)| matches!(tab, PlayerTrackerSubTab::Historical));
    let has_current_match = dock.iter_all_tabs().any(|(_, tab)| matches!(tab, PlayerTrackerSubTab::CurrentMatch));

    !has_historical || !has_current_match
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
        };
        t!(key).to_string().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            PlayerTrackerSubTab::Historical => self.tab_viewer.build_historical_sub_tab(ui),
            PlayerTrackerSubTab::CurrentMatch => self.tab_viewer.build_current_match_sub_tab(ui),
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

        // Moved out so the sub-tab bodies can take their own locks on `persisted`.
        let mut dock_state = std::mem::replace(
            &mut self.tab_state.persisted.write().player_tracker_dock_state,
            egui_dock::DockState::new(vec![]),
        );

        let mut viewer = PlayerTrackerSubTabViewer { tab_viewer: self };

        egui_dock::DockArea::new(&mut dock_state)
            .id(egui::Id::new("player_tracker_dock"))
            .style(egui_dock::Style::from_egui(ui.style().as_ref()))
            .show_close_buttons(false)
            .show_leaf_collapse_buttons(false)
            .show_leaf_close_all_buttons(false)
            .allowed_splits(egui_dock::AllowedSplits::All)
            .show_inside(ui, &mut viewer);

        self.tab_state.persisted.write().player_tracker_dock_state = dock_state;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_complete_dock_layout_needs_no_repair() {
        let dock = crate::tab_state::default_player_tracker_dock_state();
        assert!(!player_tracker_dock_needs_repair(&dock));
    }

    #[test]
    fn a_layout_missing_current_match_needs_repair() {
        let dock = egui_dock::DockState::new(vec![PlayerTrackerSubTab::Historical]);
        assert!(player_tracker_dock_needs_repair(&dock));
    }

    #[test]
    fn a_layout_missing_historical_needs_repair() {
        let dock = egui_dock::DockState::new(vec![PlayerTrackerSubTab::CurrentMatch]);
        assert!(player_tracker_dock_needs_repair(&dock));
    }

    #[test]
    fn the_default_layout_opens_on_historical() {
        // `find_active_focused` takes `&mut self`.
        let mut dock = crate::tab_state::default_player_tracker_dock_state();
        let (_rect, active) = dock.find_active_focused().expect("default layout has an active tab");
        assert_eq!(*active, PlayerTrackerSubTab::Historical);
    }
}
