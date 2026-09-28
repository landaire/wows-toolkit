//! The players met across battles, as the database holds them.
//!
//! Rows rather than model types: the tracker's own shape (alias sets, encounter
//! sets, the division marks over them) belongs to the front ends, and this
//! crate cannot see it. A reader builds its model from [`TrackerRows`] and a
//! writer states what changed as a [`TrackerWrite`].
//!
//! Writes are per change, not per save. The tracker used to be one JSON blob in
//! the settings table, rewritten in full whenever anything in it moved, which
//! at a few hundred thousand accounts is tens of megabytes a write.

use jiff::Timestamp;
use sqlx::QueryBuilder;
use sqlx::Row;
use sqlx::Sqlite;
use sqlx::SqlitePool;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;

/// One tracked account's own fields, without its aliases or encounters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedPlayerRow {
    pub account_id: AccountId,
    pub last_name: String,
    pub clan_id: i64,
    pub clan: String,
    pub notes: String,
}

/// One encounter, keyed by whichever of the two keys the table holds.
///
/// `in_division` is the mark that says the encounter was a battle the recording
/// player arranged, which the tables can hide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncounterRow<K> {
    pub account_id: AccountId,
    pub key: K,
    pub in_division: bool,
}

/// Everything the tracker tables hold.
#[derive(Debug, Default)]
pub struct TrackerRows {
    pub players: Vec<TrackedPlayerRow>,
    /// Earlier names, by account.
    pub names: Vec<(AccountId, String)>,
    pub arenas: Vec<EncounterRow<ArenaId>>,
    pub timestamps: Vec<EncounterRow<Timestamp>>,
}

/// Which of a player row's fields a write stands behind.
///
/// Both front ends write these rows, and each holds its own copy of the tracker
/// loaded when it started. A write must therefore restate only what its own
/// reader actually changed: a row that restated everything would put this app's
/// stale copy of a note, an alias set or a clan over an edit made in the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerFields {
    /// Every field, for the import and for a bulk rewrite: there is no other
    /// copy to lose.
    All,
    /// The identity a battle reports: name, clan, and the aliases that fall out
    /// of a name change. Leaves the note alone.
    Identity,
    /// The note alone.
    Note,
    /// Nothing. The row is written only so an encounter has a player to belong
    /// to, and an existing row keeps every field it has.
    None,
}

/// One account's row together with the aliases it carries, and which of them
/// the writer is speaking for.
#[derive(Debug, Clone)]
pub struct TrackedPlayerWrite {
    pub row: TrackedPlayerRow,
    pub names: Vec<String>,
    pub fields: PlayerFields,
}

/// What one write changes.
///
/// Encounters are upserted: a repeat of one already stored is the same row, and
/// a mark arriving later updates `in_division` without needing to know whether
/// the encounter was stored before.
#[derive(Debug, Default)]
pub struct TrackerWrite {
    pub players: Vec<TrackedPlayerWrite>,
    pub arenas: Vec<EncounterRow<ArenaId>>,
    pub timestamps: Vec<EncounterRow<Timestamp>>,
    /// Whether the rows in this write are the whole tracker, so every stored
    /// row absent from it is gone. What a cleared tracker and a bulk repopulate
    /// both ask for, and the only way either can remove what it no longer
    /// holds without listing it.
    pub replaces_everything: bool,
}

impl TrackerWrite {
    /// Whether applying this would change nothing, so a save can skip the
    /// transaction entirely.
    pub fn is_empty(&self) -> bool {
        !self.replaces_everything && self.players.is_empty() && self.arenas.is_empty() && self.timestamps.is_empty()
    }

    /// The rows as the tables key them, with the duplicates a key collapses
    /// already collapsed.
    ///
    /// The timestamp table keys on whole seconds while the model's set keys on
    /// the full instant, so two encounters inside one second are two members and
    /// one row. A caller counting what it wrote has to count in the table's
    /// terms or it will not recognise its own write.
    pub fn row_counts(&self) -> TrackerCounts {
        let mut arenas: Vec<(i64, i64)> = self.arenas.iter().map(|row| (row.account_id.raw(), row.key.raw())).collect();
        arenas.sort_unstable();
        arenas.dedup();
        let mut seconds: Vec<(i64, i64)> =
            self.timestamps.iter().map(|row| (row.account_id.raw(), row.key.as_second())).collect();
        seconds.sort_unstable();
        seconds.dedup();

        let mut accounts: Vec<i64> = self.players.iter().map(|player| player.row.account_id.raw()).collect();
        accounts.sort_unstable();
        accounts.dedup();
        let mut names: Vec<(i64, &str)> = self
            .players
            .iter()
            .flat_map(|player| player.names.iter().map(move |name| (player.row.account_id.raw(), name.as_str())))
            .collect();
        names.sort_unstable();
        names.dedup();

        TrackerCounts {
            players: accounts.len() as u64,
            names: names.len() as u64,
            arenas: arenas.len() as u64,
            timestamps: seconds.len() as u64,
        }
    }
}

/// How many rows each tracker table holds, for a caller checking an import
/// landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrackerCounts {
    pub players: u64,
    pub names: u64,
    pub arenas: u64,
    pub timestamps: u64,
}

/// Every tracked player, with their aliases and encounters.
pub async fn load_tracker(pool: &SqlitePool) -> Result<TrackerRows, sqlx::Error> {
    let mut rows = TrackerRows::default();

    let players =
        sqlx::query("SELECT account_id, last_name, clan_id, clan, notes FROM tracked_player").fetch_all(pool).await?;
    rows.players.reserve(players.len());
    for row in players {
        rows.players.push(TrackedPlayerRow {
            account_id: AccountId(row.try_get::<i64, _>("account_id")?),
            last_name: row.try_get("last_name")?,
            clan_id: row.try_get("clan_id")?,
            clan: row.try_get("clan")?,
            notes: row.try_get("notes")?,
        });
    }

    let names = sqlx::query("SELECT account_id, name FROM tracked_player_name").fetch_all(pool).await?;
    rows.names.reserve(names.len());
    for row in names {
        rows.names.push((AccountId(row.try_get::<i64, _>("account_id")?), row.try_get("name")?));
    }

    let arenas =
        sqlx::query("SELECT account_id, arena_id, in_division FROM tracked_player_arena").fetch_all(pool).await?;
    rows.arenas.reserve(arenas.len());
    for row in arenas {
        rows.arenas.push(EncounterRow {
            account_id: AccountId(row.try_get::<i64, _>("account_id")?),
            key: ArenaId::new(row.try_get::<i64, _>("arena_id")?),
            in_division: row.try_get("in_division")?,
        });
    }

    let timestamps =
        sqlx::query("SELECT account_id, seen_at, in_division FROM tracked_player_timestamp").fetch_all(pool).await?;
    rows.timestamps.reserve(timestamps.len());
    for row in timestamps {
        let seen_at = row.try_get::<i64, _>("seen_at")?;
        // A stored second outside the representable range is a row this app
        // never wrote; skipping it keeps one bad row from failing the load, and
        // it is said out loud because nothing else would notice.
        let Ok(key) = Timestamp::from_second(seen_at) else {
            tracing::warn!("a tracked encounter at second {seen_at} is not a timestamp; skipping it");
            continue;
        };
        rows.timestamps.push(EncounterRow {
            account_id: AccountId(row.try_get::<i64, _>("account_id")?),
            key,
            in_division: row.try_get("in_division")?,
        });
    }

    Ok(rows)
}

/// Applies `write` in one transaction, so a save either lands whole or not at
/// all.
///
/// Rows go out in batches: the one-time import of a tracker kept since 2017 is
/// over a million of them, and a statement apiece took half a minute of the
/// reader's startup.
pub async fn apply_tracker_write(pool: &SqlitePool, write: &TrackerWrite) -> Result<(), sqlx::Error> {
    if write.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    write_rows(&mut tx, write).await?;
    tx.commit().await
}

/// Folds `write` into the tracker tables, for the one-time import of a stored
/// blob.
///
/// A union, never a replacement. The tables can already hold rows by the time
/// this runs -- the other front end may have imported the same blob, and a
/// battle that ended in the first seconds of a launch is written by the save
/// task -- and an import that replaced them would drop whichever of the two it
/// did not come from. Refusing instead would be worse still: the rows that made
/// it refuse would make it refuse again on every later launch, and the blob would
/// never be read.
///
/// Everything happens under one write lock, and the checks that say the blob is
/// safe to delete happen inside the same transaction: every account the blob
/// names has a row, and each table holds at least the rows the blob contributed.
/// An import that cannot show that rolls away and leaves the blob alone.
pub async fn import_tracker_rows(pool: &SqlitePool, write: &TrackerWrite) -> Result<TrackerCounts, ImportRowsError> {
    if write.replaces_everything {
        return Err(ImportRowsError::WouldReplace);
    }

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.map_err(ImportRowsError::Sql)?;

    let before = count_rows(&mut tx).await.map_err(ImportRowsError::Sql)?;
    write_rows(&mut tx, write).await.map_err(ImportRowsError::Sql)?;
    let after = count_rows(&mut tx).await.map_err(ImportRowsError::Sql)?;

    // The blob's own rows, deduped the way the tables key them.
    let contributed = write.row_counts();
    let holds_enough = after.players >= before.players.max(contributed.players)
        && after.names >= contributed.names
        && after.arenas >= before.arenas.max(contributed.arenas)
        && after.timestamps >= before.timestamps.max(contributed.timestamps);
    if !holds_enough {
        return Err(ImportRowsError::Short { contributed, before, after });
    }

    let accounts: Vec<AccountId> = write.players.iter().map(|player| player.row.account_id).collect();
    let stored = count_players(&mut tx, &accounts).await.map_err(ImportRowsError::Sql)?;
    if stored != accounts.len() as u64 {
        return Err(ImportRowsError::Missing { named: accounts.len() as u64, stored });
    }

    tx.commit().await.map_err(ImportRowsError::Sql)?;
    Ok(after)
}

/// How many of `accounts` have a player row.
async fn count_players(tx: &mut sqlx::SqliteConnection, accounts: &[AccountId]) -> Result<u64, sqlx::Error> {
    let mut found = 0u64;
    for chunk in accounts.chunks(BATCH) {
        let mut qb: QueryBuilder<'_, Sqlite> =
            QueryBuilder::new("SELECT COUNT(*) AS n FROM tracked_player WHERE account_id IN (");
        let mut separated = qb.separated(", ");
        for account in chunk {
            separated.push_bind(account.raw());
        }
        qb.push(")");
        let row = qb.build().fetch_one(&mut *tx).await?;
        found += row.try_get::<i64, _>("n")? as u64;
    }
    Ok(found)
}

/// Why an import did not land.
#[derive(Debug, thiserror::Error)]
pub enum ImportRowsError {
    #[error("an import must fold into what is stored, not replace it")]
    WouldReplace,
    #[error(
        "the tables hold fewer rows than the import put in them ({contributed:?} contributed, {before:?} before, {after:?} after)"
    )]
    Short { contributed: TrackerCounts, before: TrackerCounts, after: TrackerCounts },
    #[error("{stored} of the {named} imported accounts have a row")]
    Missing { named: u64, stored: u64 },
    #[error("the tracker tables could not be written")]
    Sql(#[source] sqlx::Error),
}

async fn write_rows(tx: &mut sqlx::SqliteConnection, write: &TrackerWrite) -> Result<(), sqlx::Error> {
    if write.replaces_everything {
        for table in TABLES {
            sqlx::query(&format!("DELETE FROM {table}")).execute(&mut *tx).await?;
        }
    }

    // Grouped by what each write stands behind, so a conflict restates those
    // columns and no others.
    for fields in [PlayerFields::All, PlayerFields::Identity, PlayerFields::Note, PlayerFields::None] {
        let players: Vec<&TrackedPlayerWrite> = write.players.iter().filter(|player| player.fields == fields).collect();
        if players.is_empty() {
            continue;
        }

        for chunk in players.chunks(BATCH) {
            upsert_players(&mut *tx, chunk, fields).await?;
        }

        // After the player rows, which the alias rows point at. An alias set
        // belongs to the identity, so only a writer speaking for the identity
        // replaces it.
        if matches!(fields, PlayerFields::All | PlayerFields::Identity) {
            for chunk in players.chunks(BATCH) {
                let mut qb: QueryBuilder<'_, Sqlite> =
                    QueryBuilder::new("DELETE FROM tracked_player_name WHERE account_id IN (");
                let mut separated = qb.separated(", ");
                for player in chunk {
                    separated.push_bind(player.row.account_id.raw());
                }
                qb.push(")");
                qb.build().execute(&mut *tx).await?;

                let names: Vec<(i64, &str)> = chunk
                    .iter()
                    .flat_map(|player| {
                        player.names.iter().map(move |name| (player.row.account_id.raw(), name.as_str()))
                    })
                    .collect();
                for names in names.chunks(BATCH) {
                    let mut qb: QueryBuilder<'_, Sqlite> =
                        QueryBuilder::new("INSERT OR IGNORE INTO tracked_player_name (account_id, name) ");
                    qb.push_values(names, |mut row, (account, name)| {
                        row.push_bind(*account).push_bind(*name);
                    });
                    qb.build().execute(&mut *tx).await?;
                }
            }
        }
    }

    for arenas in write.arenas.chunks(BATCH) {
        let mut qb: QueryBuilder<'_, Sqlite> =
            QueryBuilder::new("INSERT INTO tracked_player_arena (account_id, arena_id, in_division) ");
        qb.push_values(arenas, |mut row, arena| {
            row.push_bind(arena.account_id.raw()).push_bind(arena.key.raw()).push_bind(arena.in_division);
        });
        qb.push(" ON CONFLICT(account_id, arena_id) DO UPDATE SET in_division=excluded.in_division");
        qb.build().execute(&mut *tx).await?;
    }

    for timestamps in write.timestamps.chunks(BATCH) {
        let mut qb: QueryBuilder<'_, Sqlite> =
            QueryBuilder::new("INSERT INTO tracked_player_timestamp (account_id, seen_at, in_division) ");
        qb.push_values(timestamps, |mut row, timestamp| {
            row.push_bind(timestamp.account_id.raw())
                .push_bind(timestamp.key.as_second())
                .push_bind(timestamp.in_division);
        });
        qb.push(" ON CONFLICT(account_id, seen_at) DO UPDATE SET in_division=excluded.in_division");
        qb.build().execute(&mut *tx).await?;
    }

    Ok(())
}

/// Writes player rows, restating on conflict only the columns `fields` speaks
/// for.
///
/// An insert still carries a value for every column, since a row that is not
/// there yet needs one; what the conflict arm leaves out is what this writer
/// does not claim to know better than what is stored.
async fn upsert_players(
    tx: &mut sqlx::SqliteConnection,
    players: &[&TrackedPlayerWrite],
    fields: PlayerFields,
) -> Result<(), sqlx::Error> {
    if players.is_empty() {
        return Ok(());
    }
    let mut qb: QueryBuilder<'_, Sqlite> =
        QueryBuilder::new("INSERT INTO tracked_player (account_id, last_name, clan_id, clan, notes) ");
    qb.push_values(players, |mut row, player| {
        row.push_bind(player.row.account_id.raw())
            .push_bind(player.row.last_name.as_str())
            .push_bind(player.row.clan_id)
            .push_bind(player.row.clan.as_str())
            .push_bind(player.row.notes.as_str());
    });
    // The identity is one fact across three columns, so they move together or
    // not at all.
    const IDENTITY: &str = "last_name=excluded.last_name, clan_id=excluded.clan_id, clan=excluded.clan";
    match fields {
        PlayerFields::All => {
            qb.push(" ON CONFLICT(account_id) DO UPDATE SET ").push(IDENTITY).push(", notes=excluded.notes");
        }
        PlayerFields::Identity => {
            qb.push(" ON CONFLICT(account_id) DO UPDATE SET ").push(IDENTITY);
        }
        PlayerFields::Note => {
            qb.push(" ON CONFLICT(account_id) DO UPDATE SET notes=excluded.notes");
        }
        // The row existing is the whole point, and SQLite has no empty SET.
        PlayerFields::None => {
            qb.push(" ON CONFLICT(account_id) DO NOTHING");
        }
    }
    qb.build().execute(&mut *tx).await?;
    Ok(())
}

/// Children before parents, which is the order a delete has to take: sqlx opens
/// its connections with `PRAGMA foreign_keys` on, so the schema's references are
/// enforced and a parent cannot go first.
const TABLES: [&str; 4] = ["tracked_player_name", "tracked_player_arena", "tracked_player_timestamp", "tracked_player"];

/// Rows per statement. Three columns at this many is well inside SQLite's bound
/// on bound parameters, and the gain flattens out long before it.
const BATCH: usize = 400;

/// How many rows the tracker tables hold.
pub async fn tracker_counts(pool: &SqlitePool) -> Result<TrackerCounts, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    count_rows(&mut connection).await
}

/// The same, on a connection already inside a transaction.
async fn count_rows(tx: &mut sqlx::SqliteConnection) -> Result<TrackerCounts, sqlx::Error> {
    async fn count(tx: &mut sqlx::SqliteConnection, table: &str) -> Result<u64, sqlx::Error> {
        let row = sqlx::query(&format!("SELECT COUNT(*) AS n FROM {table}")).fetch_one(&mut *tx).await?;
        Ok(row.try_get::<i64, _>("n")? as u64)
    }

    Ok(TrackerCounts {
        players: count(&mut *tx, "tracked_player").await?,
        names: count(&mut *tx, "tracked_player_name").await?,
        arenas: count(&mut *tx, "tracked_player_arena").await?,
        timestamps: count(&mut *tx, "tracked_player_timestamp").await?,
    })
}

/// Forgets every tracked player, with their names, encounters and notes.
///
/// What the tracker's own Clear Stats asks for. One transaction, children first,
/// so a reader looking while it runs sees the tracker before or after and never
/// a player whose encounters have gone.
pub async fn clear_tracker(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *tx).await.ok();
    for table in TABLES {
        sqlx::query(&format!("DELETE FROM {table}")).execute(&mut *tx).await?;
    }
    tx.commit().await
}

/// Reclaims the space a dropped blob left behind.
///
/// Best effort: `VACUUM` rewrites the whole file and cannot run while another
/// process holds the database, which is the normal case when both front ends
/// are open. A refusal costs only the space.
pub async fn vacuum(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("VACUUM").execute(pool).await.map(|_| ())
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;

    fn player(account: i64, notes: &str) -> TrackedPlayerWrite {
        TrackedPlayerWrite {
            row: TrackedPlayerRow {
                account_id: AccountId(account),
                last_name: format!("Player{account}"),
                clan_id: 7,
                clan: "RAIN".to_owned(),
                notes: notes.to_owned(),
            },
            names: vec!["OldName".to_owned()],
            fields: PlayerFields::All,
        }
    }

    fn player_with(account: i64, notes: &str, fields: PlayerFields) -> TrackedPlayerWrite {
        TrackedPlayerWrite { fields, ..player(account, notes) }
    }

    fn arena(account: i64, arena: i64, in_division: bool) -> EncounterRow<ArenaId> {
        EncounterRow { account_id: AccountId(account), key: ArenaId::new(arena), in_division }
    }

    fn seen(account: i64, second: i64, in_division: bool) -> EncounterRow<Timestamp> {
        EncounterRow {
            account_id: AccountId(account),
            key: Timestamp::from_second(second).expect("a second in range"),
            in_division,
        }
    }

    /// Clearing forgets every player with everything hanging off them, and
    /// leaves a tracker that reads back empty rather than one with orphans in it.
    #[tokio::test]
    async fn clearing_forgets_every_player_and_their_encounters() {
        let pool = crate::test_pool().await;
        let write = TrackerWrite {
            players: vec![player(1, "a note"), player(2, "another")],
            arenas: vec![arena(1, 10, false), arena(2, 11, true)],
            timestamps: vec![seen(1, 1_700_000_000, false), seen(2, 1_700_000_001, true)],
            ..TrackerWrite::default()
        };
        apply_tracker_write(&pool, &write).await.expect("the write lands");

        clear_tracker(&pool).await.expect("the clear lands");

        let rows = load_tracker(&pool).await.expect("the tables read back");
        assert!(rows.players.is_empty(), "no player is left");
        assert!(rows.names.is_empty(), "nor an alias");
        assert!(rows.arenas.is_empty(), "nor a battle");
        assert!(rows.timestamps.is_empty(), "nor a sighting");

        // And the tracker still takes writes afterwards, which a dropped table
        // would not.
        apply_tracker_write(&pool, &write).await.expect("the tracker fills again");
        assert_eq!(load_tracker(&pool).await.expect("it reads back").players.len(), 2);
    }

    /// What was written is what loads back, aliases and marks included.
    #[tokio::test]
    async fn a_write_round_trips() {
        let pool = crate::test_pool().await;
        let write = TrackerWrite {
            players: vec![player(1, "a note")],
            arenas: vec![arena(1, 10, false)],
            timestamps: vec![seen(1, 1_700_000_000, false)],
            ..TrackerWrite::default()
        };

        apply_tracker_write(&pool, &write).await.expect("the write lands");
        let rows = load_tracker(&pool).await.expect("the tables read back");

        assert_eq!(rows.players.len(), 1);
        assert_eq!(rows.players[0].notes, "a note");
        assert_eq!(rows.names, vec![(AccountId(1), "OldName".to_owned())]);
        assert_eq!(rows.arenas, vec![arena(1, 10, false)]);
        assert_eq!(rows.timestamps, vec![seen(1, 1_700_000_000, false)]);
    }

    /// A second write over the same keys updates rather than duplicates, which
    /// is what makes a save able to state a change without knowing whether it
    /// is the first.
    #[tokio::test]
    async fn a_repeat_write_updates_in_place() {
        let pool = crate::test_pool().await;
        let first = TrackerWrite {
            players: vec![player(1, "first")],
            arenas: vec![arena(1, 10, false)],
            timestamps: vec![seen(1, 1_700_000_000, false)],
            ..TrackerWrite::default()
        };
        apply_tracker_write(&pool, &first).await.expect("the first write lands");

        let second = TrackerWrite {
            players: vec![player(1, "second")],
            arenas: vec![arena(1, 10, true)],
            timestamps: vec![seen(1, 1_700_000_000, true)],
            ..TrackerWrite::default()
        };
        apply_tracker_write(&pool, &second).await.expect("the second write lands");

        let rows = load_tracker(&pool).await.expect("the tables read back");
        assert_eq!(rows.players.len(), 1, "one row per account");
        assert_eq!(rows.players[0].notes, "second", "the later note wins");
        assert_eq!(rows.arenas, vec![arena(1, 10, true)], "the mark arrived late and landed");
        assert_eq!(rows.timestamps, vec![seen(1, 1_700_000_000, true)]);
    }

    /// A row written only to carry an encounter leaves every stored field
    /// alone: the other front end may have edited the note since this app read
    /// it.
    #[tokio::test]
    async fn a_carried_row_restates_nothing() {
        let pool = crate::test_pool().await;
        apply_tracker_write(&pool, &TrackerWrite { players: vec![player(1, "camps")], ..TrackerWrite::default() })
            .await
            .expect("the first write lands");

        let mut carried = player_with(1, "", PlayerFields::None);
        carried.row.last_name = "Renamed".to_owned();
        carried.row.clan = "NEW".to_owned();
        carried.row.clan_id = 99;
        carried.names = Vec::new();
        apply_tracker_write(
            &pool,
            &TrackerWrite { players: vec![carried], arenas: vec![arena(1, 10, false)], ..TrackerWrite::default() },
        )
        .await
        .expect("the carried write lands");

        let rows = load_tracker(&pool).await.expect("the tables read back");
        assert_eq!(rows.players[0].notes, "camps", "the note is untouched");
        assert_eq!(rows.players[0].last_name, "Player1", "and so is the identity");
        assert_eq!(rows.players[0].clan, "RAIN");
        assert_eq!(rows.players[0].clan_id, 7);
        assert_eq!(rows.names.len(), 1, "and the aliases");
        assert_eq!(rows.arenas, vec![arena(1, 10, false)], "the encounter it carried still landed");
    }

    /// A note write moves the note and nothing else, and an identity write the
    /// other way round. Each front end edits one of the two.
    #[tokio::test]
    async fn a_note_and_an_identity_write_stay_out_of_each_others_columns() {
        let pool = crate::test_pool().await;
        apply_tracker_write(&pool, &TrackerWrite { players: vec![player(1, "camps")], ..TrackerWrite::default() })
            .await
            .expect("the first write lands");

        let mut note = player_with(1, "reworded", PlayerFields::Note);
        note.row.last_name = "Stale".to_owned();
        note.names = Vec::new();
        apply_tracker_write(&pool, &TrackerWrite { players: vec![note], ..TrackerWrite::default() })
            .await
            .expect("the note write lands");

        let rows = load_tracker(&pool).await.expect("the tables read back");
        assert_eq!(rows.players[0].notes, "reworded");
        assert_eq!(rows.players[0].last_name, "Player1", "a note write does not carry an identity");
        assert_eq!(rows.names.len(), 1, "nor an alias set");

        let mut identity = player_with(1, "stale note", PlayerFields::Identity);
        identity.row.last_name = "Renamed".to_owned();
        identity.row.clan = "NEW".to_owned();
        identity.row.clan_id = 99;
        identity.names = vec!["Player1".to_owned()];
        apply_tracker_write(&pool, &TrackerWrite { players: vec![identity], ..TrackerWrite::default() })
            .await
            .expect("the identity write lands");

        let rows = load_tracker(&pool).await.expect("the tables read back");
        assert_eq!(rows.players[0].notes, "reworded", "an identity write does not carry a note");
        assert_eq!(rows.players[0].last_name, "Renamed");
        assert_eq!(rows.players[0].clan, "NEW", "the clan and its id move together");
        assert_eq!(rows.players[0].clan_id, 99);
        assert_eq!(rows.names, vec![(AccountId(1), "Player1".to_owned())], "the displaced name became the alias");
    }

    /// An import folds into rows that are already there rather than replacing
    /// them: a battle written in the first seconds of a launch, or the other
    /// front end importing the same blob, must cost neither side its rows.
    #[tokio::test]
    async fn an_import_folds_into_what_is_already_stored() {
        let pool = crate::test_pool().await;
        apply_tracker_write(
            &pool,
            &TrackerWrite {
                players: vec![player(1, "met just now")],
                arenas: vec![arena(1, 500, false)],
                timestamps: vec![seen(1, 1_700_500_000, false)],
                ..TrackerWrite::default()
            },
        )
        .await
        .expect("the live write lands");

        let imported = import_tracker_rows(
            &pool,
            &TrackerWrite {
                players: vec![player(2, "from the blob")],
                arenas: vec![arena(2, 10, false)],
                timestamps: vec![seen(2, 1_700_000_000, false)],
                ..TrackerWrite::default()
            },
        )
        .await
        .expect("the import lands");

        assert_eq!(imported.players, 2, "both sides are stored");
        let rows = load_tracker(&pool).await.expect("the tables read back");
        let mut accounts: Vec<i64> = rows.players.iter().map(|row| row.account_id.raw()).collect();
        accounts.sort_unstable();
        assert_eq!(accounts, vec![1, 2]);
        assert_eq!(rows.arenas.len(), 2, "and so are both encounters");
        assert_eq!(rows.timestamps.len(), 2);
    }

    /// Importing the same blob twice is the same tables, which is what makes
    /// two front ends racing each other harmless.
    #[tokio::test]
    async fn importing_twice_changes_nothing_the_second_time() {
        let pool = crate::test_pool().await;
        let write = || TrackerWrite {
            players: vec![player(1, "camps")],
            arenas: vec![arena(1, 10, true)],
            timestamps: vec![seen(1, 1_700_000_000, true)],
            ..TrackerWrite::default()
        };

        let first = import_tracker_rows(&pool, &write()).await.expect("the first import lands");
        let second = import_tracker_rows(&pool, &write()).await.expect("the second import lands");

        assert_eq!(first, second, "the second import is the same tables");
        assert_eq!(second.players, 1);
        assert_eq!(second.arenas, 1);
    }

    /// A replacing write is not an import: it would drop whatever the tables
    /// hold that the blob does not name.
    #[tokio::test]
    async fn an_import_refuses_to_replace_everything() {
        let pool = crate::test_pool().await;
        let outcome = import_tracker_rows(
            &pool,
            &TrackerWrite { players: vec![player(1, "")], replaces_everything: true, ..TrackerWrite::default() },
        )
        .await;

        assert!(matches!(outcome, Err(ImportRowsError::WouldReplace)), "got {outcome:?}");
    }

    /// An import whose rows do not read back rolls the whole thing away, so the
    /// blob it came from is still the only copy.
    #[tokio::test]
    async fn an_import_that_does_not_read_back_rolls_away() {
        let pool = crate::test_pool().await;
        // Two encounters inside one second are two set members and one row, so
        // the write's own count of them is what the table can hold.
        let write = TrackerWrite {
            players: vec![player(1, "")],
            timestamps: vec![seen(1, 1_700_000_000, false), seen(1, 1_700_000_000, true)],
            ..TrackerWrite::default()
        };

        let outcome = import_tracker_rows(&pool, &write).await;

        assert!(outcome.is_ok(), "one second is one row and the count says so: {outcome:?}");
        assert_eq!(tracker_counts(&pool).await.expect("the counts read").timestamps, 1);
    }

    /// A write that claims to be the whole tracker drops the rows it does not
    /// carry, which is how a cleared tracker reaches the database.
    #[tokio::test]
    async fn a_whole_tracker_write_drops_what_it_omits() {
        let pool = crate::test_pool().await;
        apply_tracker_write(
            &pool,
            &TrackerWrite {
                players: vec![player(1, ""), player(2, "")],
                arenas: vec![arena(1, 10, false), arena(2, 11, false)],
                ..TrackerWrite::default()
            },
        )
        .await
        .expect("the write lands");

        apply_tracker_write(
            &pool,
            &TrackerWrite {
                players: vec![player(3, "")],
                arenas: vec![arena(3, 12, false)],
                replaces_everything: true,
                ..TrackerWrite::default()
            },
        )
        .await
        .expect("the replacement lands");

        let counts = tracker_counts(&pool).await.expect("the counts read");
        assert_eq!(counts.players, 1);
        assert_eq!(counts.arenas, 1);
        let rows = load_tracker(&pool).await.expect("the tables read back");
        assert_eq!(rows.players[0].account_id, AccountId(3));
    }

    /// An empty write is not a transaction.
    #[tokio::test]
    async fn an_empty_write_does_nothing() {
        let pool = crate::test_pool().await;
        apply_tracker_write(&pool, &TrackerWrite::default()).await.expect("nothing to do");
        assert_eq!(tracker_counts(&pool).await.expect("the counts read"), TrackerCounts::default());
    }
}
