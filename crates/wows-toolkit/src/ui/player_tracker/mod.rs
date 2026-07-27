mod historical;
mod model;

pub(crate) use model::SortOrder;
pub(crate) use model::SortedBy;
pub use model::TimePeriod;
pub use model::TrackedPlayer;
pub(crate) use model::encounter_severity_color;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::ReplayMeta;
use wows_replays::types::AccountId;

use crate::ui::replay_parser::Replay;
use crate::util;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayerTracker {
    pub(crate) tracked_players_by_time: BTreeMap<Timestamp, Vec<AccountId>>,
    pub(crate) tracked_players: HashMap<AccountId, TrackedPlayer>,
    pub filter_time_period: TimePeriod,
    pub(crate) sort_order: SortedBy,
    pub(crate) player_filter: String,

    #[serde(skip)]
    pub(crate) live_game_players: Option<(Timestamp, Vec<String>)>,
}

impl PlayerTracker {
    pub fn update_from_live_arena_info(&mut self, meta: &ReplayMeta) {
        // Clear the data from the last game
        self.live_game_players = None;

        let timestamp = util::replay_timestamp(meta);
        let players = meta.vehicles.iter().map(|player| player.name.clone()).collect();

        self.live_game_players = Some((timestamp, players))
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
