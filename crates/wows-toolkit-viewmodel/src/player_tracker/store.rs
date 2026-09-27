//! Reading and writing the tracked players.
//!
//! The rows live in the `tracked_player*` tables and the model this builds from
//! them is [`TrackedPlayer`]. Both front ends go through here, so neither can
//! write a shape the other cannot read.

use std::collections::HashMap;

use sqlx::SqlitePool;
use wows_replays::types::AccountId;
use wows_toolkit_config::queries;
use wows_toolkit_config::tracker;

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
            db_id: AccountId(account),
            last_name: format!("Player{account}"),
            clan: "RAIN".to_owned(),
            clan_id: 7,
            ..TrackedPlayer::default()
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
}
