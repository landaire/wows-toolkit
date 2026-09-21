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
/// A blob that does not parse is reported rather than read as empty: a reader
/// may show nothing, but a writer that rewrites what it read must not turn an
/// unreadable blob into an empty one.
pub fn players_from_blob(json: &str) -> Result<HashMap<AccountId, TrackedPlayer>, BlobError> {
    #[derive(Deserialize)]
    struct Blob {
        #[serde(default)]
        tracked_players: HashMap<AccountId, TrackedPlayer>,
    }

    serde_json::from_str::<Blob>(json).map(|blob| blob.tracked_players).map_err(BlobError::Decode)
}

/// The Current Match view settings the blob carries.
///
/// Read and written the same way the players are, so the mode chosen in one
/// app is the mode the other opens on.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct ViewModes {
    #[serde(default)]
    pub win_rate_mode: crate::player_tracker::live::WinRateMode,
    #[serde(default)]
    pub current_match_view_mode: crate::player_tracker::live::CurrentMatchViewMode,
}

/// The view settings a stored blob carries.
///
/// A blob that carries none reads as the defaults, which is what an account
/// that never opened the tab has. A blob that will not decode is reported
/// rather than read as defaults: silently resetting the settings would hide
/// the fact that the rest of the blob is unreadable too.
pub fn view_modes_from_blob(json: &str) -> Result<ViewModes, BlobError> {
    if json.trim().is_empty() {
        return Ok(ViewModes::default());
    }
    serde_json::from_str(json).map_err(BlobError::Decode)
}

/// `json` with the view settings replaced, keeping every other field.
pub fn blob_with_view_modes(json: &str, modes: ViewModes) -> Result<String, BlobError> {
    let mut blob = if json.trim().is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        match serde_json::from_str(json).map_err(BlobError::Decode)? {
            serde_json::Value::Object(map) => serde_json::Value::Object(map),
            _ => return Err(BlobError::NotAnObject),
        }
    };

    let object = blob.as_object_mut().ok_or(BlobError::NotAnObject)?;
    object.insert("win_rate_mode".to_string(), serde_json::to_value(modes.win_rate_mode).map_err(BlobError::Encode)?);
    object.insert(
        "current_match_view_mode".to_string(),
        serde_json::to_value(modes.current_match_view_mode).map_err(BlobError::Encode)?,
    );
    serde_json::to_string(&blob).map_err(BlobError::Encode)
}

#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("the stored tracker is not a JSON object")]
    NotAnObject,
    #[error("the stored tracker could not be read")]
    Decode(#[source] serde_json::Error),
    #[error("the players could not be encoded")]
    Encode(#[source] serde_json::Error),
}

/// `json` with its players replaced.
///
/// Every other field is carried through untouched: the blob is written by
/// both front ends and each knows fields the other does not, so a writer that
/// rebuilt it from its own struct would drop them.
pub fn blob_with_players(json: &str, players: &HashMap<AccountId, TrackedPlayer>) -> Result<String, BlobError> {
    // An empty string is genuinely nothing stored yet, which a fresh object
    // is the right answer to. Anything else that does not read as an object
    // is a blob this writer must not replace: the fields it cannot see are
    // the other front end's.
    let mut blob = if json.trim().is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        match serde_json::from_str(json).map_err(BlobError::Decode)? {
            serde_json::Value::Object(map) => serde_json::Value::Object(map),
            _ => return Err(BlobError::NotAnObject),
        }
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

        let read = players_from_blob(&blob).expect("the blob reads back");
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
        assert_eq!(players_from_blob(&written).expect("the blob reads back").len(), 1);
    }

    /// An unreadable blob is reported: a writer that took it for an empty
    /// one would replace every player it could not see.
    #[test]
    fn the_view_modes_round_trip_beside_the_players() {
        use crate::player_tracker::live::CurrentMatchViewMode;
        use crate::player_tracker::live::WinRateMode;

        let players: HashMap<_, _> = [player(1, "Someone", "note")].into_iter().collect();
        let blob = blob_with_players("{}", &players).expect("the blob encodes");
        let modes =
            ViewModes { win_rate_mode: WinRateMode::Ship, current_match_view_mode: CurrentMatchViewMode::Compact };

        let written = blob_with_view_modes(&blob, modes).expect("the blob encodes");
        let read = view_modes_from_blob(&written).expect("the blob reads back");

        assert_eq!(read.win_rate_mode, WinRateMode::Ship);
        assert_eq!(read.current_match_view_mode, CurrentMatchViewMode::Compact);
        assert_eq!(players_from_blob(&written).expect("the blob reads back").len(), 1, "the players survive");
    }

    #[test]
    fn an_unreadable_blob_is_reported_rather_than_read_as_empty() {
        assert!(players_from_blob("not json").is_err());
        assert!(players_from_blob(r#"{"tracked_players": 7}"#).is_err());
        assert!(blob_with_players("not json", &HashMap::new()).is_err());
        assert!(blob_with_players("[1, 2]", &HashMap::new()).is_err());
    }

    /// A first write with nothing stored yet still produces a usable blob.
    #[test]
    fn writing_into_nothing_produces_a_blob_that_reads_back() {
        let players: HashMap<_, _> = [player(3, "New", "first note")].into_iter().collect();
        let written = blob_with_players("", &players).expect("the blob encodes");

        assert_eq!(players_from_blob(&written).expect("the blob reads back")[&AccountId(3)].notes, "first note");
    }
}
