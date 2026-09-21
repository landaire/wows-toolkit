//! The players met across indexed battles, as both front ends store them.
//!
//! This is the shape behind the `player_tracker_data` setting, so a note
//! written in one app is the note the other reads.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;

use jiff::Timestamp;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;

/// The encounters in which a tracked player shared your division.
///
/// Marked under both keys the encounter counts are taken with: distinct arena
/// for the all-time count, distinct timestamp for the in-range one. A tracked
/// player's `arena_ids` and `timestamps` are unpaired sets, so a mark recorded
/// under only one of them would leave the two column families disagreeing about
/// which encounters are hidden.
///
/// Both sets are private so [`mark`](Self::mark) is the only way to add to
/// either: nothing outside this module can write one key and forget the other.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DivisionEncounters {
    #[serde(default)]
    arena_ids: BTreeSet<ArenaId>,
    #[serde(default)]
    timestamps: BTreeSet<Timestamp>,
}

impl DivisionEncounters {
    /// Record one encounter as a division one, under both keys at once.
    ///
    /// Returns whether either set gained the encounter, which is what tells a
    /// caller a first marking from a re-parse of a battle already marked. It is
    /// not a claim that both sets grew: two battles sharing a timestamp add the
    /// second arena without adding a second timestamp.
    pub fn mark(&mut self, arena_id: ArenaId, timestamp: Timestamp) -> bool {
        let arena_is_new = self.arena_ids.insert(arena_id);
        let timestamp_is_new = self.timestamps.insert(timestamp);
        arena_is_new || timestamp_is_new
    }

    /// Whether this encounter is one the division toggle hides.
    pub fn hides_arena(&self, arena_id: &ArenaId) -> bool {
        self.arena_ids.contains(arena_id)
    }

    /// The same question keyed by timestamp, which is what the in-range
    /// counts dedup on.
    pub fn hides_timestamp(&self, timestamp: &Timestamp) -> bool {
        self.timestamps.contains(timestamp)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TrackedPlayer {
    pub last_name: String,
    pub db_id: AccountId,
    pub names: HashSet<String>,
    pub clan_id: i64,
    pub clan: String,
    pub timestamps: BTreeSet<Timestamp>,
    pub arena_ids: BTreeSet<ArenaId>,
    #[serde(default)]
    pub notes: String,
    /// Which of this player's encounters were division ones. Per encounter, not
    /// per account: divisioning with someone once hides those battles and
    /// leaves every other meeting with them on the tables.
    #[serde(default)]
    pub division_encounters: DivisionEncounters,
}

impl TrackedPlayer {
    /// The battles this player was met in that the division-mate toggle leaves
    /// visible, keyed by arena. Drives the all-time counts.
    pub fn visible_arena_ids(&self, show_division_mates: bool) -> impl Iterator<Item = ArenaId> + '_ {
        self.arena_ids
            .iter()
            .copied()
            .filter(move |arena_id| show_division_mates || !self.division_encounters.hides_arena(arena_id))
    }

    /// The same encounters keyed by timestamp, which is what the in-range counts
    /// dedup on. Filtered against the marks recorded under the same key, so the
    /// two families always hide the same battles.
    ///
    /// Assumes distinct battles carry distinct replay timestamps, which the
    /// whole timestamp-keyed side of the tracker rests on. Where two battles do
    /// share one, they already counted as a single encounter here; a division
    /// mark on that timestamp additionally hides both of them from the in-range
    /// counts while the arena key hides only the battle actually marked.
    pub fn visible_timestamps(&self, show_division_mates: bool) -> impl DoubleEndedIterator<Item = Timestamp> + '_ {
        self.timestamps
            .iter()
            .copied()
            .filter(move |timestamp| show_division_mates || !self.division_encounters.hides_timestamp(timestamp))
    }

    /// The most recent visible encounter, or `None` when every encounter with
    /// this player was a division one and the toggle is off.
    pub fn last_visible_timestamp(&self, show_division_mates: bool) -> Option<Timestamp> {
        self.visible_timestamps(show_division_mates).next_back()
    }
}

/// Where the tracked players live in the shared config database.
///
/// The whole tracker is stored as one JSON blob, so a reader that wants only
/// the players goes through [`players_from_blob`] and a writer through
/// [`blob_with_players`], which keeps the fields it does not know about.
pub const SETTING_KEY: &str = "player_tracker_data";

/// The players a stored blob carries.
///
/// A blob that does not parse yields none rather than failing: the tracker is
/// an annotation over the replay index, and losing the annotations is better
/// than refusing to open the tab.
pub fn players_from_blob(json: &str) -> HashMap<AccountId, TrackedPlayer> {
    #[derive(Deserialize)]
    struct Blob {
        #[serde(default)]
        tracked_players: HashMap<AccountId, TrackedPlayer>,
    }

    match serde_json::from_str::<Blob>(json) {
        Ok(blob) => blob.tracked_players,
        Err(err) => {
            tracing::warn!("player tracker: the stored players could not be read: {err}");
            HashMap::new()
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("the stored tracker is not a JSON object")]
    NotAnObject,
    #[error("the players could not be encoded")]
    Encode(#[source] serde_json::Error),
}

/// `json` with its players replaced.
///
/// Every other field is carried through untouched: the blob is written by
/// both front ends and each knows fields the other does not, so a writer that
/// rebuilt it from its own struct would drop them.
pub fn blob_with_players(json: &str, players: &HashMap<AccountId, TrackedPlayer>) -> Result<String, BlobError> {
    let mut blob: serde_json::Value = match serde_json::from_str(json) {
        Ok(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
        // No usable blob yet: a fresh one carrying only what is being written.
        _ => serde_json::Value::Object(serde_json::Map::new()),
    };

    let object = blob.as_object_mut().ok_or(BlobError::NotAnObject)?;
    object.insert("tracked_players".to_string(), serde_json::to_value(players).map_err(BlobError::Encode)?);
    serde_json::to_string(&blob).map_err(BlobError::Encode)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: i64, name: &str, notes: &str) -> (AccountId, TrackedPlayer) {
        let player = TrackedPlayer {
            db_id: AccountId(id),
            last_name: name.to_string(),
            notes: notes.to_string(),
            ..TrackedPlayer::default()
        };
        (AccountId(id), player)
    }

    #[test]
    fn a_stored_blob_yields_its_players() {
        let players: HashMap<_, _> = [player(1, "Harvey635", "camps")].into_iter().collect();
        let blob = blob_with_players("{}", &players).expect("the blob encodes");

        let read = players_from_blob(&blob);
        assert_eq!(read.len(), 1);
        assert_eq!(read[&AccountId(1)].notes, "camps");
    }

    /// The other front end's fields must survive a write from this one.
    #[test]
    fn writing_players_keeps_every_other_field() {
        let existing = r#"{"tracked_players":{},"show_division_mates":true,"player_filter":"abc"}"#;
        let players: HashMap<_, _> = [player(2, "Someone", "")].into_iter().collect();

        let written = blob_with_players(existing, &players).expect("the blob encodes");
        let value: serde_json::Value = serde_json::from_str(&written).expect("the blob parses");

        assert_eq!(value.get("show_division_mates").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(value.get("player_filter").and_then(|v| v.as_str()), Some("abc"));
        assert_eq!(players_from_blob(&written).len(), 1);
    }

    #[test]
    fn an_unreadable_blob_yields_no_players_rather_than_failing() {
        assert!(players_from_blob("not json").is_empty());
        assert!(players_from_blob(r#"{"tracked_players": 7}"#).is_empty());
    }

    /// A first write with nothing stored yet still produces a usable blob.
    #[test]
    fn writing_into_nothing_produces_a_blob_that_reads_back() {
        let players: HashMap<_, _> = [player(3, "New", "first note")].into_iter().collect();
        let written = blob_with_players("", &players).expect("the blob encodes");

        assert_eq!(players_from_blob(&written)[&AccountId(3)].notes, "first note");
    }
}
