//! Folding one battle's roster into the tracked players.
//!
//! The rules are the egui tracker's (`ui/player_tracker/mod.rs`'s
//! `ingest_roster`), so a battle recorded by either front end leaves the same
//! rows.

use std::collections::HashMap;

use jiff::Timestamp;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;

use crate::player_tracker::tracked::Pending;
use crate::player_tracker::tracked::TrackedPlayer;

/// One player in a battle, as the tracker records them.
#[derive(Debug, Clone, Copy)]
pub struct RosterPlayer<'a> {
    pub account: AccountId,
    pub name: &'a str,
    pub clan: &'a str,
    /// `None` for a player outside any clan.
    pub clan_id: Option<i64>,
    /// `None` for a player in no division.
    pub division_id: Option<u32>,
    pub is_bot: bool,
    /// The player the replay was recorded by.
    pub is_self: bool,
}

/// Everyone in one battle, and when and where it was fought.
#[derive(Debug, Clone)]
pub struct BattleRoster<'a> {
    pub arena_id: ArenaId,
    pub timestamp: Timestamp,
    pub players: Vec<RosterPlayer<'a>>,
}

impl<'a> BattleRoster<'a> {
    pub fn from_report(report: &'a NormalizedBattleReport, arena_id: ArenaId) -> Self {
        let players = report
            .players
            .iter()
            .map(|player| RosterPlayer {
                account: player.db_id,
                name: &player.name,
                clan: &player.clan,
                clan_id: player.clan_id,
                division_id: player.division_id,
                is_bot: player.is_bot,
                is_self: player.is_self,
            })
            .collect();

        Self { arena_id, timestamp: report.metadata.timestamp, players }
    }

    /// The accounts whose tracker rows this battle can change: every human but
    /// the recording player.
    pub fn met_accounts(&self) -> Vec<AccountId> {
        self.players.iter().filter(|player| !player.is_bot && !player.is_self).map(|player| player.account).collect()
    }
}

/// Folds `roster` into `players`, stating each change in `pending`.
///
/// A battle already recorded for a player adds nothing, so reading a replay
/// twice does not count it twice. The recording player and bots are never
/// recorded. A player in the recording player's division has the encounter
/// marked, so the tables can hide it.
pub fn record_roster(
    players: &mut HashMap<AccountId, TrackedPlayer>,
    pending: &mut Pending,
    roster: &BattleRoster<'_>,
) {
    let BattleRoster { arena_id, timestamp, .. } = *roster;
    let recorder = roster.players.iter().find(|player| player.is_self);

    for player in &roster.players {
        if player.is_bot || player.is_self {
            continue;
        }

        let is_division_mate = recorder.is_some_and(|recorder| {
            recorder.account != player.account
                && recorder.division_id.is_some()
                && recorder.division_id == player.division_id
        });

        let tracked = players.entry(player.account).or_insert_with(|| TrackedPlayer::new(player.account));

        // Marked before the arena guard below: reading a battle already recorded
        // is still the first chance to mark it.
        let marked = is_division_mate && tracked.division_encounters.mark(arena_id, timestamp);

        if tracked.arena_ids.contains(&arena_id) {
            if marked {
                pending.encounter_changed(player.account, arena_id, timestamp);
            }
            continue;
        }

        // Name and clan follow the latest battle, so an older replay read later
        // does not put back a name the player has since changed.
        let is_latest = tracked.timestamps.last().is_none_or(|latest| latest < timestamp);
        if is_latest {
            if tracked.last_name != player.name && !tracked.last_name.is_empty() {
                tracked.names.insert(std::mem::take(&mut tracked.last_name));
            }
            tracked.last_name = player.name.to_owned();
            tracked.clan = player.clan.to_owned();
            tracked.clan_id = player.clan_id;
            pending.identity_changed(player.account);
        }

        tracked.timestamps.insert(timestamp);
        tracked.arena_ids.insert(arena_id);
        pending.encounter_changed(player.account, arena_id, timestamp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORDER: AccountId = AccountId(1);
    const MATE: AccountId = AccountId(2);
    const ENEMY: AccountId = AccountId(3);

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).expect("a second in range")
    }

    fn player(account: AccountId, name: &'static str, division_id: Option<u32>) -> RosterPlayer<'static> {
        RosterPlayer { account, name, clan: "WTK", clan_id: Some(7), division_id, is_bot: false, is_self: false }
    }

    /// The recorder in division 4 with `MATE`, against `ENEMY` in a division of their own, and a
    /// bot.
    fn battle(arena: i64, second: i64, enemy_name: &'static str) -> BattleRoster<'static> {
        BattleRoster {
            arena_id: ArenaId::new(arena),
            timestamp: at(second),
            players: vec![
                RosterPlayer { is_self: true, ..player(RECORDER, "Me", Some(4)) },
                player(MATE, "Mate", Some(4)),
                player(ENEMY, enemy_name, Some(5)),
                RosterPlayer { is_bot: true, ..player(AccountId(0), "IDS_BOT", None) },
            ],
        }
    }

    #[test]
    fn a_battle_records_everyone_met_but_the_recorder_and_bots() {
        let mut players = HashMap::new();
        let mut pending = Pending::default();

        record_roster(&mut players, &mut pending, &battle(10, 1_000, "Enemy"));

        let mut recorded: Vec<AccountId> = players.keys().copied().collect();
        recorded.sort_by_key(|account| account.0);
        assert_eq!(recorded, vec![MATE, ENEMY]);
        let enemy = &players[&ENEMY];
        assert_eq!(enemy.last_name, "Enemy");
        assert_eq!(enemy.clan, "WTK");
        assert_eq!(enemy.clan_id, Some(7));
        assert_eq!(enemy.arena_ids.iter().collect::<Vec<_>>(), vec![ArenaId::new(10)]);
        assert_eq!(enemy.timestamps.iter().collect::<Vec<_>>(), vec![at(1_000)]);
        assert_eq!(pending.changed_arenas().count(), 2, "one encounter row per player met");
    }

    #[test]
    fn reading_a_battle_twice_does_not_count_it_twice() {
        let mut players = HashMap::new();
        let mut pending = Pending::default();
        record_roster(&mut players, &mut pending, &battle(10, 1_000, "Enemy"));

        let mut again = Pending::default();
        record_roster(&mut players, &mut again, &battle(10, 1_000, "Enemy"));

        assert_eq!(players[&ENEMY].arena_ids.len(), 1);
        assert_eq!(players[&ENEMY].timestamps.len(), 1);
        assert!(again.is_empty(), "nothing new to write: {again:?}");
    }

    #[test]
    fn only_the_recorders_division_is_marked() {
        let mut players = HashMap::new();
        let mut pending = Pending::default();

        record_roster(&mut players, &mut pending, &battle(10, 1_000, "Enemy"));

        assert!(players[&MATE].arena_in_division(ArenaId::new(10)));
        assert!(players[&MATE].timestamp_in_division(at(1_000)));
        assert!(!players[&ENEMY].arena_in_division(ArenaId::new(10)));
    }

    #[test]
    fn a_mark_on_a_battle_recorded_unmarked_is_written() {
        let mut mate = TrackedPlayer::new(MATE);
        mate.last_name = "Mate".to_owned();
        mate.arena_ids.insert(ArenaId::new(10));
        mate.timestamps.insert(at(1_000));
        let mut players = HashMap::from([(MATE, mate)]);
        let mut pending = Pending::default();

        record_roster(&mut players, &mut pending, &battle(10, 1_000, "Enemy"));

        assert!(players[&MATE].arena_in_division(ArenaId::new(10)));
        assert!(pending.changed_arenas().any(|change| change == (MATE, ArenaId::new(10))));
    }

    #[test]
    fn a_newer_name_keeps_the_old_one_as_an_alias_and_an_older_one_changes_nothing() {
        let mut players = HashMap::new();
        let mut pending = Pending::default();
        record_roster(&mut players, &mut pending, &battle(10, 1_000, "Enemy"));

        record_roster(&mut players, &mut pending, &battle(11, 2_000, "Renamed"));
        record_roster(&mut players, &mut pending, &battle(9, 500, "Ancient"));

        let enemy = &players[&ENEMY];
        assert_eq!(enemy.last_name, "Renamed");
        assert!(enemy.names.contains("Enemy"));
        assert!(!enemy.names.contains("Ancient"), "an older battle's name is not the player's");
        assert_eq!(enemy.arena_ids.len(), 3, "the older battle still counts as an encounter");
    }
}
