//! Reading and writing the tracked players.
//!
//! The rows live in the `tracked_player*` tables and the model this builds from
//! them is [`TrackedPlayer`]. Both front ends go through here, so neither can
//! write a shape the other cannot read.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use jiff::Timestamp;
use sqlx::SqlitePool;
use wows_replays::types::AccountId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::DivisionMateEncounter;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::index::rows::MatchFilter;
use wows_toolkit_config::index::rows::PlayerEncounter;
use wows_toolkit_config::queries;
use wows_toolkit_config::tracker;

use crate::player_tracker::record::BattleRoster;
use crate::player_tracker::record::record_roster;
use crate::player_tracker::tracked;
use crate::player_tracker::tracked::BlobError;
use crate::player_tracker::tracked::Pending;
use crate::player_tracker::tracked::TrackedPlayer;
use crate::player_tracker::tracked::ViewModes;

/// Every tracked player the database holds.
pub async fn load(pool: &SqlitePool) -> Result<HashMap<AccountId, TrackedPlayer>, sqlx::Error> {
    Ok(players_from_rows(tracker::load_tracker(pool).await?))
}

/// The model the stored rows describe.
///
/// A row family whose account has no `tracked_player` row is dropped: the row
/// it would belong to is what carries the player's identity, and inventing one
/// would put a nameless entry on the tables.
fn players_from_rows(rows: tracker::TrackerRows) -> HashMap<AccountId, TrackedPlayer> {
    let mut players: HashMap<AccountId, TrackedPlayer> = rows
        .players
        .into_iter()
        .map(|row| {
            let player = TrackedPlayer {
                last_name: row.last_name,
                db_id: row.account_id,
                names: Default::default(),
                clan_id: row.clan_id,
                clan: row.clan,
                timestamps: Default::default(),
                arena_ids: Default::default(),
                notes: row.notes,
                division_encounters: Default::default(),
            };
            (row.account_id, player)
        })
        .collect();

    // Counted rather than reported one by one: an orphan means a delete that did
    // not go through `apply_tracker_write`, which is worth one line, not
    // thousands.
    let mut orphans = 0usize;

    for (account, name) in rows.names {
        match players.get_mut(&account) {
            Some(player) => {
                player.names.insert(name);
            }
            None => orphans += 1,
        }
    }

    for row in rows.arenas {
        match players.get_mut(&row.account_id) {
            Some(player) => {
                player.arena_ids.insert(row.key);
                if row.in_division {
                    // Marked under this key alone: the timestamp rows carry
                    // their own mark, and each family is read without the other.
                    player.division_encounters.mark_arena(row.key);
                }
            }
            None => orphans += 1,
        }
    }

    for row in rows.timestamps {
        match players.get_mut(&row.account_id) {
            Some(player) => {
                player.timestamps.insert(row.key);
                if row.in_division {
                    player.division_encounters.mark_timestamp(row.key);
                }
            }
            None => orphans += 1,
        }
    }

    if orphans > 0 {
        tracing::warn!("{orphans} tracked encounter or alias rows belong to no player row and were left out");
    }

    players
}

/// The write `pending` describes, against the tracker as it stands.
///
/// Values come from `players` rather than from the change record: a save states
/// which accounts moved, and what they now hold is whatever the tracker holds
/// when the save runs.
///
/// An encounter is written only where the player actually holds that key. The
/// index's division sync marks encounters without adding them -- an account it
/// has no row for is not one the tracker met -- so a mark must not become a row
/// the model does not have.
pub fn write_for(pending: &Pending, players: &HashMap<AccountId, TrackedPlayer>) -> tracker::TrackerWrite {
    let mut write = tracker::TrackerWrite { replaces_everything: pending.replaces_everything(), ..Default::default() };

    if pending.replaces_everything() {
        write.players.reserve(players.len());
        for player in players.values() {
            write.players.push(player_write(player, tracker::PlayerFields::All));
            for arena_id in player.arena_ids.iter() {
                write.arenas.push(tracker::EncounterRow {
                    account_id: player.db_id,
                    key: arena_id,
                    in_division: player.arena_in_division(arena_id),
                });
            }
            for timestamp in player.timestamps.iter() {
                write.timestamps.push(tracker::EncounterRow {
                    account_id: player.db_id,
                    key: timestamp,
                    in_division: player.timestamp_in_division(timestamp),
                });
            }
        }
        return write;
    }

    // What each account's row is being written for. An account named by both an
    // identity change and a note edit is written once, for both.
    let mut fields: HashMap<AccountId, tracker::PlayerFields> = HashMap::new();
    for account in pending.changed_identities() {
        fields.insert(account, tracker::PlayerFields::Identity);
    }
    for account in pending.changed_notes() {
        let entry = fields.entry(account).or_insert(tracker::PlayerFields::Note);
        if *entry == tracker::PlayerFields::Identity {
            *entry = tracker::PlayerFields::All;
        }
    }

    for (account, arena_id) in pending.changed_arenas() {
        let Some(player) = players.get(&account) else { continue };
        if !player.arena_ids.contains(&arena_id) {
            continue;
        }
        // The row has to exist for the encounter to point at, but this write
        // speaks for no field of it unless something else already did.
        fields.entry(account).or_insert(tracker::PlayerFields::None);
        write.arenas.push(tracker::EncounterRow {
            account_id: account,
            key: arena_id,
            in_division: player.arena_in_division(arena_id),
        });
    }

    for (account, timestamp) in pending.changed_timestamps() {
        let Some(player) = players.get(&account) else { continue };
        if !player.timestamps.contains(&timestamp) {
            continue;
        }
        fields.entry(account).or_insert(tracker::PlayerFields::None);
        write.timestamps.push(tracker::EncounterRow {
            account_id: account,
            key: timestamp,
            in_division: player.timestamp_in_division(timestamp),
        });
    }

    for (account, fields) in fields {
        if let Some(player) = players.get(&account) {
            write.players.push(player_write(player, fields));
        }
    }

    write
}

/// The whole tracker as rows to fold into what is stored, for the import.
///
/// Every field of every account, since the blob is the only copy of them, but
/// not a replacement: rows the blob does not name are someone else's and stay.
fn import_write(players: &HashMap<AccountId, TrackedPlayer>) -> tracker::TrackerWrite {
    let mut write = tracker::TrackerWrite::default();
    write.players.reserve(players.len());
    for player in players.values() {
        write.players.push(player_write(player, tracker::PlayerFields::All));
        for arena_id in player.arena_ids.iter() {
            write.arenas.push(tracker::EncounterRow {
                account_id: player.db_id,
                key: arena_id,
                in_division: player.arena_in_division(arena_id),
            });
        }
        for timestamp in player.timestamps.iter() {
            write.timestamps.push(tracker::EncounterRow {
                account_id: player.db_id,
                key: timestamp,
                in_division: player.timestamp_in_division(timestamp),
            });
        }
    }
    write
}

/// One account's row, stating which of its fields the caller speaks for.
fn player_write(player: &TrackedPlayer, fields: tracker::PlayerFields) -> tracker::TrackedPlayerWrite {
    tracker::TrackedPlayerWrite {
        fields,
        row: tracker::TrackedPlayerRow {
            account_id: player.db_id,
            last_name: player.last_name.clone(),
            clan_id: player.clan_id,
            clan: player.clan.clone(),
            notes: player.notes.clone(),
        },
        names: player.names.iter().cloned().collect(),
    }
}

/// Applies `pending` against `players`.
pub async fn save(
    pool: &SqlitePool,
    pending: &Pending,
    players: &HashMap<AccountId, TrackedPlayer>,
) -> Result<(), sqlx::Error> {
    tracker::apply_tracker_write(pool, &write_for(pending, players)).await
}

/// Folds one battle's roster into the stored tracker, reading only the accounts
/// it met.
pub async fn record_battle(pool: &SqlitePool, roster: &BattleRoster<'_>) -> Result<(), sqlx::Error> {
    let accounts = roster.met_accounts();

    tracker::update_tracker(pool, tracker::TrackerScope::Accounts(&accounts), |rows| {
        let mut players = players_from_rows(rows);
        let mut pending = Pending::default();
        record_roster(&mut players, &mut pending, roster);
        write_for(&pending, &players)
    })
    .await
}

#[derive(Debug, thiserror::Error)]
pub enum DivisionSyncError {
    #[error("the index's division encounters could not be read")]
    Index(#[source] IndexError),
    #[error("the division marks could not be stored")]
    Store(#[source] sqlx::Error),
}

/// Marks every division encounter the index holds on the stored players.
pub async fn sync_division_mates(pool: &SqlitePool) -> Result<(), DivisionSyncError> {
    let encounters =
        query::division_mate_encounters(pool, &MatchFilter::default()).await.map_err(DivisionSyncError::Index)?;

    tracker::update_tracker(pool, tracker::TrackerScope::All, |rows| {
        let mut players = players_from_rows(rows);
        let mut pending = Pending::default();
        mark_division_encounters(&mut players, &mut pending, &encounters);
        write_for(&pending, &players)
    })
    .await
    .map_err(DivisionSyncError::Store)
}

#[derive(Debug, thiserror::Error)]
pub enum PopulateError {
    #[error("the index's encounters could not be read")]
    Index(#[source] IndexError),
    #[error("the tracker could not be read or written")]
    Store(#[source] sqlx::Error),
}

/// Records every encounter the index holds into the stored tracker and marks
/// the division ones. The egui tracker's Populate (`ui/player_tracker/model.rs`'s
/// `populate_from_index`).
///
/// A player's name and clan are taken from the index only where its latest
/// battle with them is the tracker's latest too, so a name a newer replay
/// recorded is not put back.
pub async fn populate_from_index(pool: &SqlitePool) -> Result<(), PopulateError> {
    let encounters = query::player_encounters(pool).await.map_err(PopulateError::Index)?;
    let division =
        query::division_mate_encounters(pool, &MatchFilter::default()).await.map_err(PopulateError::Index)?;

    tracker::update_tracker(pool, tracker::TrackerScope::All, |rows| {
        let mut players = players_from_rows(rows);
        let mut pending = Pending::default();
        record_index_encounters(&mut players, &mut pending, &encounters);
        // After the encounters: a mark only lands on a player the tracker holds.
        mark_division_encounters(&mut players, &mut pending, &division);
        write_for(&pending, &players)
    })
    .await
    .map_err(PopulateError::Store)
}

fn record_index_encounters(
    players: &mut HashMap<AccountId, TrackedPlayer>,
    pending: &mut Pending,
    encounters: &[PlayerEncounter],
) {
    let mut latest: HashMap<AccountId, (Timestamp, &str, &str)> = HashMap::new();
    for encounter in encounters {
        let account = encounter.account_id;
        let player = players.entry(account).or_insert_with(|| TrackedPlayer::new(account));
        if player.arena_ids.insert(encounter.arena_id) {
            pending.arena_changed(account, encounter.arena_id);
        }
        if player.timestamps.insert(encounter.timestamp) {
            pending.timestamp_changed(account, encounter.timestamp);
        }

        let identity = (encounter.timestamp, encounter.name.as_str(), encounter.clan.as_str());
        match latest.entry(account) {
            Entry::Occupied(mut held) if held.get().0 < encounter.timestamp => {
                held.insert(identity);
            }
            // An older battle's name is not the player's.
            Entry::Occupied(_) => {}
            Entry::Vacant(vacant) => {
                vacant.insert(identity);
            }
        }
    }

    for (account, (seen, name, clan)) in latest {
        let Some(player) = players.get_mut(&account) else { continue };
        if player.timestamps.last() != Some(seen) || (player.last_name == name && player.clan == clan) {
            continue;
        }
        if player.last_name != name && !player.last_name.is_empty() {
            player.names.insert(std::mem::take(&mut player.last_name));
        }
        player.last_name = name.to_owned();
        player.clan = clan.to_owned();
        pending.identity_changed(account);
    }
}

/// Marks `encounters` on the players `players` already has. An account the
/// tracker has no row for is not marked: it has no row to hide, and creating one
/// would invent a player the tracker never recorded.
fn mark_division_encounters(
    players: &mut HashMap<AccountId, TrackedPlayer>,
    pending: &mut Pending,
    encounters: &[DivisionMateEncounter],
) {
    for encounter in encounters {
        let Some(player) = players.get_mut(&encounter.account_id) else { continue };
        if player.division_encounters.mark(encounter.arena_id, encounter.timestamp) {
            pending.encounter_changed(encounter.account_id, encounter.arena_id, encounter.timestamp);
        }
    }
}

/// What [`import_blob_once`] found to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Imported {
    /// No blob was stored: a fresh install, or one already imported. The blob's
    /// absence is what says the import has happened, which is why it is deleted
    /// only once its rows are there.
    NothingStored,
    /// The blob's players are in the tables, and the blob is gone.
    Migrated { players: usize, arenas: u64, timestamps: u64 },
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("the stored tracker could not be read")]
    Read(#[source] queries::SettingError),
    #[error(transparent)]
    Blob(#[from] BlobError),
    #[error(transparent)]
    Rows(#[from] tracker::ImportRowsError),
    #[error("the imported tracker could not be tidied up")]
    Cleanup(#[source] sqlx::Error),
}

/// Moves a tracker stored as the old settings blob into its tables, once.
///
/// The blob is deleted only after its rows are written and checked back inside
/// one transaction: an import that did not land rolls away and leaves the blob as
/// the only copy, so the next launch tries again. It folds into whatever is
/// already stored rather than replacing it, so a battle written in the first
/// seconds of a launch, or the other front end importing the same blob, costs
/// nothing.
///
/// The space the blob held is reclaimed on a best-effort basis, since a `VACUUM`
/// cannot run while another process has the database open.
pub async fn import_blob_once(pool: &SqlitePool) -> Result<Imported, ImportError> {
    let stored: Option<String> =
        queries::try_get_setting(pool, tracked::SETTING_KEY).await.map_err(ImportError::Read)?;
    let Some(stored) = stored else { return Ok(Imported::NothingStored) };

    let players = tracked::players_from_blob(&stored)?;
    let modes = tracked::view_modes_from_blob(&stored)?;

    let counts = tracker::import_tracker_rows(pool, &import_write(&players)).await?;

    store_view_modes(pool, modes).await.map_err(ImportError::Cleanup)?;

    queries::delete_setting(pool, tracked::SETTING_KEY).await.map_err(ImportError::Cleanup)?;
    if let Err(err) = tracker::vacuum(pool).await {
        tracing::info!("the tracker blob's space will be reclaimed later: {err}");
    }

    Ok(Imported::Migrated { players: players.len(), arenas: counts.arenas, timestamps: counts.timestamps })
}

/// The Current Match view settings, each under its own key.
pub async fn load_view_modes(pool: &SqlitePool) -> ViewModes {
    ViewModes {
        win_rate_mode: queries::get_setting(pool, tracked::WIN_RATE_MODE_KEY).await.unwrap_or_default(),
        current_match_view_mode: queries::get_setting(pool, tracked::CURRENT_MATCH_VIEW_MODE_KEY)
            .await
            .unwrap_or_default(),
        show_division_mates: queries::get_setting(pool, tracked::SHOW_DIVISION_MATES_KEY).await.unwrap_or_default(),
    }
}

pub async fn store_view_modes(pool: &SqlitePool, modes: ViewModes) -> Result<(), sqlx::Error> {
    queries::set_setting(pool, tracked::WIN_RATE_MODE_KEY, &modes.win_rate_mode).await?;
    queries::set_setting(pool, tracked::CURRENT_MATCH_VIEW_MODE_KEY, &modes.current_match_view_mode).await?;
    queries::set_setting(pool, tracked::SHOW_DIVISION_MATES_KEY, &modes.show_division_mates).await
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use wows_replays::types::ArenaId;

    use super::*;

    fn seen(second: i64) -> Timestamp {
        Timestamp::from_second(second).expect("a second in range")
    }

    fn tracked_player(account: i64) -> TrackedPlayer {
        TrackedPlayer {
            last_name: format!("Player{account}"),
            clan: "RAIN".to_owned(),
            clan_id: Some(7),
            ..TrackedPlayer::new(AccountId(account))
        }
    }

    async fn pool() -> SqlitePool {
        wows_toolkit_config::test_pool().await
    }

    /// A save writes what changed, and the tracker reads back the same.
    #[tokio::test]
    async fn an_encounter_round_trips_through_the_tables() {
        let pool = pool().await;
        let mut players = HashMap::new();
        let mut player = tracked_player(1);
        player.arena_ids.insert(ArenaId::new(10));
        player.timestamps.insert(seen(1_700_000_000));
        player.notes = "camps".to_owned();
        players.insert(AccountId(1), player);

        let mut pending = Pending::default();
        pending.identity_changed(AccountId(1));
        pending.encounter_changed(AccountId(1), ArenaId::new(10), seen(1_700_000_000));
        save(&pool, &pending, &players).await.expect("the save lands");

        let read = load(&pool).await.expect("the tables read back");
        assert_eq!(read.len(), 1);
        let player = &read[&AccountId(1)];
        assert_eq!(player.notes, "camps");
        assert_eq!(player.last_name, "Player1");
        assert_eq!(player.arena_ids.iter().collect::<Vec<_>>(), vec![ArenaId::new(10)]);
        assert_eq!(player.timestamps.iter().collect::<Vec<_>>(), vec![seen(1_700_000_000)]);
    }

    /// A division mark survives the round trip under both keys, which is what
    /// the two column families read.
    #[tokio::test]
    async fn a_division_mark_survives_the_round_trip() {
        let pool = pool().await;
        let mut players = HashMap::new();
        let mut player = tracked_player(1);
        player.arena_ids.insert(ArenaId::new(10));
        player.timestamps.insert(seen(1_700_000_000));
        player.division_encounters.mark(ArenaId::new(10), seen(1_700_000_000));
        players.insert(AccountId(1), player);

        let mut pending = Pending::default();
        pending.everything_changed();
        save(&pool, &pending, &players).await.expect("the save lands");

        let read = load(&pool).await.expect("the tables read back");
        let player = &read[&AccountId(1)];
        assert!(player.arena_in_division(ArenaId::new(10)));
        assert!(player.timestamp_in_division(seen(1_700_000_000)));
        assert_eq!(player.visible_arena_ids(false).count(), 0, "a marked battle is hidden with the toggle off");
        assert_eq!(player.visible_arena_ids(true).count(), 1);
    }

    /// An encounter recorded for an account whose row has not been written yet
    /// still gets one: a change record that named only the encounter would
    /// otherwise write a row family with no player.
    #[tokio::test]
    async fn an_encounter_alone_still_writes_its_player() {
        let pool = pool().await;
        let mut players = HashMap::new();
        let mut player = tracked_player(2);
        player.arena_ids.insert(ArenaId::new(11));
        player.timestamps.insert(seen(1_700_000_500));
        players.insert(AccountId(2), player);

        let mut pending = Pending::default();
        pending.encounter_changed(AccountId(2), ArenaId::new(11), seen(1_700_000_500));
        save(&pool, &pending, &players).await.expect("the save lands");

        let read = load(&pool).await.expect("the tables read back");
        assert_eq!(read.len(), 1, "the player came with its encounter");
        assert_eq!(read[&AccountId(2)].arena_ids.len(), 1);
    }

    /// A cleared tracker reaches the tables: the rows it no longer holds go.
    #[tokio::test]
    async fn clearing_the_tracker_empties_the_tables() {
        let pool = pool().await;
        let mut players = HashMap::new();
        let mut player = tracked_player(1);
        player.arena_ids.insert(ArenaId::new(10));
        player.timestamps.insert(seen(1_700_000_000));
        players.insert(AccountId(1), player);

        let mut pending = Pending::default();
        pending.everything_changed();
        save(&pool, &pending, &players).await.expect("the save lands");

        players.clear();
        let mut pending = Pending::default();
        pending.everything_changed();
        save(&pool, &pending, &players).await.expect("the clear lands");

        assert!(load(&pool).await.expect("the tables read back").is_empty());
    }

    /// The old blob becomes rows, its view settings become their own keys, and
    /// the blob is gone afterwards.
    #[tokio::test]
    async fn the_old_blob_is_imported_once_and_then_dropped() {
        let pool = pool().await;
        let blob = r#"{"tracked_players":{"1":{"last_name":"Harvey635","db_id":1,"names":["OldName"],
            "clan_id":7,"clan":"RAIN","timestamps":["2023-11-14T22:13:20Z"],"arena_ids":[10],"notes":"camps",
            "division_encounters":{"arena_ids":[10],"timestamps":["2023-11-14T22:13:20Z"]}}},
            "show_division_mates":true}"#;
        queries::set_setting(&pool, tracked::SETTING_KEY, &blob.to_owned()).await.expect("the blob stores");

        let outcome = import_blob_once(&pool).await.expect("the import runs");

        assert_eq!(outcome, Imported::Migrated { players: 1, arenas: 1, timestamps: 1 });
        let read = load(&pool).await.expect("the tables read back");
        assert_eq!(read[&AccountId(1)].notes, "camps");
        assert!(read[&AccountId(1)].names.contains("OldName"));
        assert!(read[&AccountId(1)].arena_in_division(ArenaId::new(10)), "the marks came across");
        assert!(load_view_modes(&pool).await.show_division_mates, "so did the view settings");

        let left: Option<String> =
            queries::try_get_setting(&pool, tracked::SETTING_KEY).await.expect("the settings read");
        assert!(left.is_none(), "the blob is gone once its rows are counted");
        assert_eq!(
            import_blob_once(&pool).await.expect("the second run"),
            Imported::NothingStored,
            "and with it gone there is nothing left to import"
        );
    }

    /// Nothing stored is not an error, and not an import either.
    #[tokio::test]
    async fn an_install_with_no_blob_imports_nothing() {
        let pool = pool().await;
        assert_eq!(import_blob_once(&pool).await.expect("the import runs"), Imported::NothingStored);
    }

    /// Rows already stored -- the other front end's import, or a battle the save
    /// task wrote in the first seconds of a launch -- do not stop the import and
    /// are not replaced by it. Refusing would strand the blob: the rows that
    /// made it refuse would still be there on the next launch.
    #[tokio::test]
    async fn an_import_folds_in_beside_rows_that_are_already_stored() {
        let pool = pool().await;
        let mut players = HashMap::new();
        let mut live = tracked_player(9);
        live.arena_ids.insert(ArenaId::new(99));
        live.timestamps.insert(seen(1_700_009_000));
        players.insert(AccountId(9), live);
        let mut pending = Pending::default();
        pending.identity_changed(AccountId(9));
        pending.encounter_changed(AccountId(9), ArenaId::new(99), seen(1_700_009_000));
        save(&pool, &pending, &players).await.expect("the live save lands");

        let blob = r#"{"tracked_players":{"1":{"last_name":"FromBlob","db_id":1,"names":[],"clan_id":0,
            "clan":"","timestamps":[],"arena_ids":[],"notes":""}}}"#;
        queries::set_setting(&pool, tracked::SETTING_KEY, &blob.to_owned()).await.expect("the blob stores");

        let outcome = import_blob_once(&pool).await.expect("the import runs");

        assert_eq!(outcome, Imported::Migrated { players: 1, arenas: 1, timestamps: 1 });
        let read = load(&pool).await.expect("the tables read back");
        assert_eq!(read.len(), 2, "both the resident player and the imported one");
        assert!(read.contains_key(&AccountId(9)), "the battle already stored survived");
        assert!(read.contains_key(&AccountId(1)), "and the blob was imported beside it");
        let left: Option<String> =
            queries::try_get_setting(&pool, tracked::SETTING_KEY).await.expect("the settings read");
        assert!(left.is_none(), "so there is nothing left to import on the next launch");
    }

    /// A mark for a battle the tracker never recorded writes no encounter row:
    /// the index marks accounts it has rows for, which is not the same set.
    #[tokio::test]
    async fn a_mark_without_an_encounter_writes_no_row() {
        let pool = pool().await;
        let mut players = HashMap::new();
        let mut player = tracked_player(1);
        player.arena_ids.insert(ArenaId::new(10));
        player.timestamps.insert(seen(1_700_000_000));
        // Marked for a battle the model does not hold, which is what
        // `refresh_division_mates_from_index` can produce.
        player.division_encounters.mark(ArenaId::new(11), seen(1_700_000_900));
        players.insert(AccountId(1), player);

        let mut pending = Pending::default();
        pending.encounter_changed(AccountId(1), ArenaId::new(10), seen(1_700_000_000));
        pending.encounter_changed(AccountId(1), ArenaId::new(11), seen(1_700_000_900));
        save(&pool, &pending, &players).await.expect("the save lands");

        let read = load(&pool).await.expect("the tables read back");
        let stored = &read[&AccountId(1)];
        assert_eq!(stored.arena_ids.iter().collect::<Vec<_>>(), vec![ArenaId::new(10)], "only what was met");
        assert_eq!(stored.timestamps.len(), 1);
    }

    /// A blob that will not parse is reported and kept: it is still the only
    /// copy of whatever it holds.
    #[tokio::test]
    async fn an_unreadable_blob_is_kept() {
        let pool = pool().await;
        queries::set_setting(&pool, tracked::SETTING_KEY, &"not json".to_owned()).await.expect("the blob stores");

        assert!(import_blob_once(&pool).await.is_err());
        let left: Option<String> =
            queries::try_get_setting(&pool, tracked::SETTING_KEY).await.expect("the settings read");
        assert!(left.is_some(), "an unreadable blob is not deleted");
    }

    /// One battle in the index, recorded by `recorder`, with each player's name, clan and
    /// division.
    async fn index_battle(
        pool: &SqlitePool,
        arena: i64,
        second: i64,
        recorder: AccountId,
        roster: &[(AccountId, &str, &str, Option<i64>)],
    ) {
        use std::path::Path;
        use std::path::PathBuf;

        use wows_replays::types::GameParamId;
        use wows_toolkit_config::index::rows::IndexedVehicleRow;
        use wows_toolkit_config::index::rows::MatchOutcome;
        use wows_toolkit_config::index::rows::ObjectiveMatch;
        use wows_toolkit_config::index::rows::ReplayRecord;
        use wows_toolkit_config::index::rows::VehicleRelation;

        let at = seen(second);
        let arena_id = ArenaId::new(arena);
        let source = query::ensure_default_source(pool, Path::new("C:/wows/replays"), at).await.expect("a source");
        let objective = ObjectiveMatch {
            arena_id,
            timestamp: at,
            map: "Ocean".into(),
            game_mode: "Domination".into(),
            game_mode_id: None,
            game_type: "pvp".into(),
            match_group: "pvp".into(),
            version_build: None,
        };
        query::upsert_match(pool, &objective).await.expect("the match is stored");

        let vehicles: Vec<IndexedVehicleRow> = roster
            .iter()
            .map(|&(account, name, clan, division)| IndexedVehicleRow {
                arena_id,
                account_id: account,
                player_name: name.to_owned(),
                clan: clan.to_owned(),
                realm: None,
                ship_id: GameParamId::from(111u64),
                ship_index: "PJSB018".into(),
                ship_name: "Yamato".into(),
                nation: "japan".into(),
                species: "Battleship".into(),
                tier: 10,
                relation: if account == recorder { VehicleRelation::SelfPlayer } else { VehicleRelation::Enemy },
                division_id: division,
                survived: None,
                damage: None,
                kills: None,
                spotting: None,
                potential: None,
                received: None,
                pr: None,
                mc: None,
                mc_version: None,
                mc_torpedoes_observed: false,
                is_test_ship: false,
                disconnected: None,
                is_stream_sniper: None,
                sniper_twitch_login: None,
            })
            .collect();
        query::upsert_vehicles(pool, &vehicles).await.expect("the roster is stored");

        let record = ReplayRecord {
            arena_id,
            source_id: source,
            replay_path: PathBuf::from(format!("C:/wows/replays/{arena}.wowsreplay")),
            file_mtime: None,
            outcome: MatchOutcome::Win,
            self_account_id: Some(recorder),
            self_ship_id: None,
            self_survived: None,
            self_damage: None,
            self_kills: None,
            self_pr: None,
            results_available: true,
            indexed_at: at,
        };
        query::upsert_record(pool, &record).await.expect("the record is stored");
    }

    /// A battle read twice is stored once, its division mate marked and its recorder left out.
    #[tokio::test]
    async fn a_recorded_battle_is_stored_once() {
        use crate::player_tracker::record::RosterPlayer;

        let pool = pool().await;
        let player = |account: i64, name: &'static str, division_id: Option<u32>, is_self: bool| RosterPlayer {
            account: AccountId(account),
            name,
            clan: "WTK",
            clan_id: Some(7),
            division_id,
            is_bot: false,
            is_self,
        };
        let roster = BattleRoster {
            arena_id: ArenaId::new(10),
            timestamp: seen(1_700_000_000),
            players: vec![
                player(1, "Me", Some(4), true),
                player(2, "Mate", Some(4), false),
                player(3, "Enemy", None, false),
            ],
        };

        record_battle(&pool, &roster).await.expect("the battle is recorded");
        record_battle(&pool, &roster).await.expect("the battle is recorded again");

        let read = load(&pool).await.expect("the tables read back");
        assert!(!read.contains_key(&AccountId(1)), "the recorder is not someone they met");
        let enemy = &read[&AccountId(3)];
        assert_eq!(enemy.last_name, "Enemy");
        assert_eq!(enemy.clan, "WTK");
        assert_eq!(enemy.arena_ids.len(), 1);
        assert_eq!(enemy.timestamps.len(), 1);
        assert!(read[&AccountId(2)].arena_in_division(ArenaId::new(10)));
        assert!(!enemy.arena_in_division(ArenaId::new(10)));
    }

    /// Populate fills the tracker from the index, takes the latest name, marks division mates,
    /// leaves notes alone, and counts nothing twice when pressed again.
    #[tokio::test]
    async fn populate_fills_the_tracker_from_the_index() {
        let pool = pool().await;
        let (me, mate, enemy) = (AccountId(1), AccountId(9), AccountId(5));
        index_battle(
            &pool,
            100,
            1_700_000_000,
            me,
            &[(me, "Me", "", Some(3)), (mate, "Mate", "", Some(3)), (enemy, "Old", "OLD", None)],
        )
        .await;
        index_battle(&pool, 101, 1_700_009_000, me, &[(me, "Me", "", None), (enemy, "New", "NEW", None)]).await;

        let held =
            TrackedPlayer { last_name: "Old".to_owned(), notes: "camps".to_owned(), ..TrackedPlayer::new(enemy) };
        let noted = HashMap::from([(enemy, held)]);
        let mut pending = Pending::default();
        pending.identity_changed(enemy);
        pending.note_changed(enemy);
        save(&pool, &pending, &noted).await.expect("the note is stored");

        populate_from_index(&pool).await.expect("the tracker populates");
        populate_from_index(&pool).await.expect("the tracker populates again");

        let read = load(&pool).await.expect("the tables read back");
        assert!(!read.contains_key(&me), "the recorder is not someone they met");
        let stored = &read[&enemy];
        assert_eq!(stored.arena_ids.len(), 2);
        assert_eq!(stored.timestamps.len(), 2);
        assert_eq!(stored.last_name, "New");
        assert_eq!(stored.clan, "NEW");
        assert!(stored.names.contains("Old"), "the earlier name is kept as an alias");
        assert_eq!(stored.notes, "camps");
        assert!(read[&mate].arena_in_division(ArenaId::new(100)));
        assert!(read[&mate].timestamp_in_division(seen(1_700_000_000)));
        assert!(!stored.arena_in_division(ArenaId::new(100)));
    }

    /// The division sync marks the players the tracker holds, and only them.
    #[tokio::test]
    async fn the_division_sync_marks_only_tracked_mates() {
        let pool = pool().await;
        let (me, mate, stranger) = (AccountId(1), AccountId(9), AccountId(8));
        index_battle(
            &pool,
            100,
            1_700_000_000,
            me,
            &[(me, "Me", "", Some(3)), (mate, "Mate", "", Some(3)), (stranger, "Stranger", "", Some(3))],
        )
        .await;
        let mut recorded = tracked_player(mate.0);
        recorded.arena_ids.insert(ArenaId::new(100));
        recorded.timestamps.insert(seen(1_700_000_000));
        let players = HashMap::from([(mate, recorded)]);
        let mut pending = Pending::default();
        pending.identity_changed(mate);
        pending.encounter_changed(mate, ArenaId::new(100), seen(1_700_000_000));
        save(&pool, &pending, &players).await.expect("the encounter is stored");

        sync_division_mates(&pool).await.expect("the sync runs");

        let read = load(&pool).await.expect("the tables read back");
        assert!(read[&mate].arena_in_division(ArenaId::new(100)));
        assert!(!read.contains_key(&stranger), "an account the tracker never recorded is not created");
    }
}
