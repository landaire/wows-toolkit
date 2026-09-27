//! The players met across indexed battles, as both front ends hold them.
//!
//! The rows behind this live in the `tracked_player*` tables, which
//! [`super::store`] reads and writes, so a note written in one app is the note
//! the other reads.

use std::collections::HashMap;
use std::collections::HashSet;

use jiff::Timestamp;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;

/// A sorted set of encounter keys.
///
/// A `BTreeSet` over the same keys allocates a fixed 11-slot node however few
/// keys it holds, and most tracked players are met once or twice: at 460,000
/// encounters that overhead measured larger than the encounters themselves.
/// Sorted insertion into a `Vec` keeps the iteration order the tables read in
/// and the binary-search lookups the division marks need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct EncounterSet<T> {
    keys: Vec<T>,
}

/// Hand-written: the derive would ask `T` for a default the keys never need.
impl<T> Default for EncounterSet<T> {
    fn default() -> Self {
        EncounterSet { keys: Vec::new() }
    }
}

impl<T: Ord + Copy> EncounterSet<T> {
    /// Adds `key`, reporting whether the set gained it.
    pub fn insert(&mut self, key: T) -> bool {
        match self.keys.binary_search(&key) {
            Ok(_) => false,
            Err(at) => {
                self.keys.insert(at, key);
                true
            }
        }
    }

    pub fn contains(&self, key: &T) -> bool {
        self.keys.binary_search(key).is_ok()
    }

    /// The keys in order, oldest or lowest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = T> + '_ {
        self.keys.iter().copied()
    }

    pub fn first(&self) -> Option<T> {
        self.keys.first().copied()
    }

    pub fn last(&self) -> Option<T> {
        self.keys.last().copied()
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Builds a set from keys in any order.
    pub fn from_keys(mut keys: Vec<T>) -> Self {
        keys.sort_unstable();
        keys.dedup();
        EncounterSet { keys }
    }
}

impl<'de, T: Ord + Copy + Deserialize<'de>> Deserialize<'de> for EncounterSet<T> {
    /// Sorted and deduped on the way in: the stored order is this type's own
    /// invariant, and a reader that trusted the stored order would binary-search a
    /// list that is not sorted.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<T>::deserialize(deserializer).map(EncounterSet::from_keys)
    }
}

impl<T: Ord + Copy> FromIterator<T> for EncounterSet<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        EncounterSet::from_keys(iter.into_iter().collect())
    }
}

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
    arena_ids: EncounterSet<ArenaId>,
    #[serde(default)]
    timestamps: EncounterSet<Timestamp>,
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

    /// Marks one key on its own, for the store reading the two row families
    /// back one at a time. Crate-private: a caller recording a live encounter
    /// goes through [`mark`](Self::mark), which cannot write one key and forget
    /// the other.
    pub(crate) fn mark_arena(&mut self, arena_id: ArenaId) {
        self.arena_ids.insert(arena_id);
    }

    pub(crate) fn mark_timestamp(&mut self, timestamp: Timestamp) {
        self.timestamps.insert(timestamp);
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
    /// When this player was met. Whole seconds in practice, which is all a
    /// replay header carries and all the `tracked_player_timestamp` key holds:
    /// two encounters inside one second are one row there.
    pub timestamps: EncounterSet<Timestamp>,
    pub arena_ids: EncounterSet<ArenaId>,
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
            .filter(move |timestamp| show_division_mates || !self.division_encounters.hides_timestamp(timestamp))
    }

    /// The most recent visible encounter, or `None` when every encounter with
    /// this player was a division one and the toggle is off.
    pub fn last_visible_timestamp(&self, show_division_mates: bool) -> Option<Timestamp> {
        self.visible_timestamps(show_division_mates).next_back()
    }

    /// Folds `other`'s encounters and marks into this player, and takes its
    /// identity when it has one.
    ///
    /// For the import: a battle that ended while a stored tracker was being
    /// imported left a player holding only what that battle reported, and the
    /// imported row is the historical one. The keys are merged per family, since
    /// two battles inside one second are two arenas under one timestamp and
    /// pairing them would drop whichever family is longer.
    pub fn merge_encounters_from(&mut self, other: &TrackedPlayer) {
        for arena_id in other.arena_ids.iter() {
            self.arena_ids.insert(arena_id);
            if other.arena_in_division(arena_id) {
                self.division_encounters.mark_arena(arena_id);
            }
        }
        for timestamp in other.timestamps.iter() {
            self.timestamps.insert(timestamp);
            if other.timestamp_in_division(timestamp) {
                self.division_encounters.mark_timestamp(timestamp);
            }
        }
        // A battle's report of who a player is is the newer one; a player the
        // other side never named says nothing about it.
        if !other.last_name.is_empty() {
            if self.last_name != other.last_name && !self.last_name.is_empty() {
                self.names.insert(std::mem::take(&mut self.last_name));
            }
            self.last_name = other.last_name.clone();
            self.clan = other.clan.clone();
            self.clan_id = other.clan_id;
        }
        for name in &other.names {
            self.names.insert(name.clone());
        }
    }

    /// Whether this encounter was one the recording player arranged, which is
    /// the mark the tables can hide by.
    pub fn arena_in_division(&self, arena_id: ArenaId) -> bool {
        self.division_encounters.hides_arena(&arena_id)
    }

    /// The same question keyed by timestamp.
    pub fn timestamp_in_division(&self, timestamp: Timestamp) -> bool {
        self.division_encounters.hides_timestamp(&timestamp)
    }
}

/// What has changed in the tracker since the last write.
///
/// The tracker used to be written whole on a timer; a save now states its
/// changes, so what it costs is what moved rather than what is held. Kept apart
/// by kind because both front ends write these rows: a save that restated a note
/// it had not edited would put its own stale copy over the other app's edit.
#[derive(Debug, Default, Clone)]
pub struct Pending {
    everything: bool,
    /// Accounts whose name or clan a battle reported.
    identities: HashSet<AccountId>,
    /// Accounts whose note was edited here.
    notes: HashSet<AccountId>,
    arenas: HashSet<(AccountId, ArenaId)>,
    timestamps: HashSet<(AccountId, Timestamp)>,
}

impl Pending {
    /// This account's name or clan changed, which also settles its aliases.
    pub fn identity_changed(&mut self, account: AccountId) {
        if !self.everything {
            self.identities.insert(account);
        }
    }

    /// This account's note was edited.
    pub fn note_changed(&mut self, account: AccountId) {
        if !self.everything {
            self.notes.insert(account);
        }
    }

    /// One encounter was recorded or marked, under both of the keys the tracker
    /// records it by.
    pub fn encounter_changed(&mut self, account: AccountId, arena_id: ArenaId, timestamp: Timestamp) {
        self.arena_changed(account, arena_id);
        self.timestamp_changed(account, timestamp);
    }

    /// One encounter under its arena key alone. The two key families are
    /// unpaired, so a caller restating a player's encounters walks each of them.
    pub fn arena_changed(&mut self, account: AccountId, arena_id: ArenaId) {
        if !self.everything {
            self.arenas.insert((account, arena_id));
        }
    }

    /// The same under the timestamp key.
    pub fn timestamp_changed(&mut self, account: AccountId, timestamp: Timestamp) {
        if !self.everything {
            self.timestamps.insert((account, timestamp));
        }
    }

    /// Every row is now whatever the tracker holds: what a bulk repopulate and
    /// a cleared tracker both leave behind, neither of which can say which rows
    /// it no longer has.
    pub fn everything_changed(&mut self) {
        self.everything = true;
        self.identities.clear();
        self.notes.clear();
        self.arenas.clear();
        self.timestamps.clear();
    }

    pub fn is_empty(&self) -> bool {
        !self.everything
            && self.identities.is_empty()
            && self.notes.is_empty()
            && self.arenas.is_empty()
            && self.timestamps.is_empty()
    }

    /// Forgets the changes `written` covered, keeping anything recorded since.
    ///
    /// A save copies the changes rather than taking them, and clears them only
    /// once the write has landed: a change taken by a save that then failed, or
    /// that never returned, is one nothing else would state again.
    pub fn forget(&mut self, written: &Pending) {
        if written.everything {
            // The write covered the whole tracker, so anything recorded while it
            // ran is already in what it wrote.
            *self = Pending::default();
            return;
        }
        if self.everything {
            // A bulk change since the write supersedes it.
            return;
        }
        self.identities.retain(|account| !written.identities.contains(account));
        self.notes.retain(|account| !written.notes.contains(account));
        self.arenas.retain(|key| !written.arenas.contains(key));
        self.timestamps.retain(|key| !written.timestamps.contains(key));
    }

    pub(crate) fn replaces_everything(&self) -> bool {
        self.everything
    }

    pub(crate) fn changed_identities(&self) -> impl Iterator<Item = AccountId> + '_ {
        self.identities.iter().copied()
    }

    pub(crate) fn changed_notes(&self) -> impl Iterator<Item = AccountId> + '_ {
        self.notes.iter().copied()
    }

    pub(crate) fn changed_arenas(&self) -> impl Iterator<Item = (AccountId, ArenaId)> + '_ {
        self.arenas.iter().copied()
    }

    pub(crate) fn changed_timestamps(&self) -> impl Iterator<Item = (AccountId, Timestamp)> + '_ {
        self.timestamps.iter().copied()
    }
}

/// Where the tracker used to live: one JSON blob in the settings table, read
/// once by [`players_from_blob`] so the tables can be filled from it.
pub const SETTING_KEY: &str = "player_tracker_data";

/// Where the Current Match view settings live now that the blob is gone. One
/// key apiece, so writing one does not rewrite the others.
pub const WIN_RATE_MODE_KEY: &str = "player_tracker.win_rate_mode";
pub const CURRENT_MATCH_VIEW_MODE_KEY: &str = "player_tracker.current_match_view_mode";
pub const SHOW_DIVISION_MATES_KEY: &str = "player_tracker.show_division_mates";

/// The players a stored blob carries.
///
/// A blob that does not parse is reported rather than read as empty: the import
/// that reads it deletes it afterwards, and taking an unreadable blob for an
/// empty tracker would throw away every encounter it holds.
pub fn players_from_blob(json: &str) -> Result<HashMap<AccountId, TrackedPlayer>, BlobError> {
    #[derive(Deserialize)]
    struct Blob {
        #[serde(default)]
        tracked_players: HashMap<AccountId, TrackedPlayer>,
    }

    serde_json::from_str::<Blob>(json).map(|blob| blob.tracked_players).map_err(BlobError::Decode)
}

/// The Current Match view settings.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct ViewModes {
    #[serde(default)]
    pub win_rate_mode: crate::player_tracker::live::WinRateMode,
    #[serde(default)]
    pub current_match_view_mode: crate::player_tracker::live::CurrentMatchViewMode,
    /// Whether the Historical and Clans tables count the encounters marked in
    /// each player's `division_encounters`. Off by default: a battle you
    /// arranged says less about meeting someone than one you did not.
    #[serde(default)]
    pub show_division_mates: bool,
}

/// The view settings a stored blob carries, for the one-time import.
///
/// A blob that carries none reads as the defaults, which is what an account
/// that never opened the tab has.
pub fn view_modes_from_blob(json: &str) -> Result<ViewModes, BlobError> {
    if json.trim().is_empty() {
        return Ok(ViewModes::default());
    }
    serde_json::from_str(json).map_err(BlobError::Decode)
}

#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("the stored tracker could not be read")]
    Decode(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(second: i64) -> Timestamp {
        Timestamp::from_second(second).expect("a second in range")
    }

    /// Keys come back in order however they went in, and a repeat is not a
    /// second key.
    #[test]
    fn an_encounter_set_is_sorted_and_deduped() {
        let mut set = EncounterSet::default();
        assert!(set.insert(seen(300)));
        assert!(set.insert(seen(100)));
        assert!(set.insert(seen(200)));
        assert!(!set.insert(seen(200)), "a repeat is not a new encounter");

        assert_eq!(set.iter().collect::<Vec<_>>(), vec![seen(100), seen(200), seen(300)]);
        assert_eq!(set.first(), Some(seen(100)));
        assert_eq!(set.last(), Some(seen(300)));
        assert_eq!(set.len(), 3);
        assert!(set.contains(&seen(200)));
        assert!(!set.contains(&seen(250)));
    }

    /// A stored set in any order reads back sorted, so the binary searches over
    /// it hold whatever wrote it.
    #[test]
    fn a_stored_set_reads_back_sorted() {
        let set: EncounterSet<Timestamp> =
            serde_json::from_str(r#"["1970-01-01T00:05:00Z","1970-01-01T00:01:40Z","1970-01-01T00:05:00Z"]"#)
                .expect("the set decodes");

        assert_eq!(set.iter().collect::<Vec<_>>(), vec![seen(100), seen(300)]);
    }

    /// The set serializes as the bare list it used to be, which is what lets a
    /// blob written by an older build still read.
    #[test]
    fn an_encounter_set_encodes_as_a_list() {
        let set: EncounterSet<Timestamp> = [seen(100), seen(200)].into_iter().collect();

        assert_eq!(
            serde_json::to_string(&set).expect("the set encodes"),
            r#"["1970-01-01T00:01:40Z","1970-01-01T00:03:20Z"]"#
        );
    }

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
        let blob = r#"{"tracked_players":{"1":{"last_name":"Harvey635","db_id":1,"names":[],"clan_id":0,
            "clan":"","timestamps":["1970-01-01T00:01:40Z"],"arena_ids":[10],"notes":"camps"}}}"#;

        let read = players_from_blob(blob).expect("the blob reads back");

        assert_eq!(read.len(), 1);
        assert_eq!(read[&AccountId(1)].notes, "camps");
        assert_eq!(read[&AccountId(1)].timestamps.len(), 1);
        assert_eq!(read[&AccountId(1)].arena_ids.len(), 1);
    }

    /// An unreadable blob is reported: the import deletes the blob once it has
    /// read it, so taking one for empty would lose every encounter in it.
    #[test]
    fn an_unreadable_blob_is_reported_rather_than_read_as_empty() {
        assert!(players_from_blob("not json").is_err());
        assert!(players_from_blob(r#"{"tracked_players": 7}"#).is_err());
    }

    #[test]
    fn the_view_modes_read_out_of_a_blob() {
        use crate::player_tracker::live::CurrentMatchViewMode;
        use crate::player_tracker::live::WinRateMode;

        let modes = view_modes_from_blob(
            r#"{"win_rate_mode":"Ship","current_match_view_mode":"Compact","show_division_mates":true}"#,
        )
        .expect("the blob reads back");

        assert_eq!(modes.win_rate_mode, WinRateMode::Ship);
        assert_eq!(modes.current_match_view_mode, CurrentMatchViewMode::Compact);
        assert!(modes.show_division_mates);
        assert!(!view_modes_from_blob("").expect("nothing stored is the defaults").show_division_mates);
    }

    /// Nothing pending is nothing written; a recorded change is.
    #[test]
    fn pending_changes_report_whether_there_is_anything_to_write() {
        let mut pending = Pending::default();
        assert!(pending.is_empty());

        pending.identity_changed(AccountId(1));
        assert!(!pending.is_empty());
        assert_eq!(pending.changed_identities().count(), 1);
        assert_eq!(pending.changed_notes().count(), 0, "a battle does not edit a note");
    }

    /// A save forgets what it wrote and keeps what arrived while it ran.
    #[test]
    fn forgetting_a_write_keeps_the_changes_it_did_not_cover() {
        let mut pending = Pending::default();
        pending.identity_changed(AccountId(1));
        pending.encounter_changed(AccountId(1), ArenaId::new(10), seen(100));
        let written = pending.clone();

        pending.note_changed(AccountId(2));
        pending.forget(&written);

        assert_eq!(pending.changed_identities().count(), 0, "what was written is gone");
        assert_eq!(pending.changed_arenas().count(), 0);
        assert_eq!(pending.changed_notes().collect::<Vec<_>>(), vec![AccountId(2)], "what came after is kept");
    }

    /// A write that failed is not forgotten, so the next save states it again.
    #[test]
    fn changes_survive_a_write_that_did_not_land() {
        let mut pending = Pending::default();
        pending.identity_changed(AccountId(1));
        let _attempted = pending.clone();

        // `forget` is what a landed write calls; a failed one calls nothing.
        assert_eq!(pending.changed_identities().count(), 1);
    }

    /// A bulk change recorded while a write ran supersedes it rather than being
    /// cleared by it.
    #[test]
    fn a_bulk_change_since_the_write_survives_forgetting() {
        let mut pending = Pending::default();
        pending.identity_changed(AccountId(1));
        let written = pending.clone();

        pending.everything_changed();
        pending.forget(&written);

        assert!(pending.replaces_everything(), "the repopulate still has to be written");
    }

    /// Once everything has changed, single changes add nothing: the write is
    /// already the whole tracker.
    #[test]
    fn everything_changed_swallows_the_rest() {
        let mut pending = Pending::default();
        pending.identity_changed(AccountId(1));
        pending.everything_changed();
        pending.identity_changed(AccountId(2));
        pending.encounter_changed(AccountId(2), ArenaId::new(11), seen(200));

        assert!(!pending.is_empty());
        assert!(pending.replaces_everything());
        assert_eq!(pending.changed_identities().count(), 0);
        assert_eq!(pending.changed_arenas().count(), 0);
    }

    /// The two key families are recorded apart, since two battles inside one
    /// second are two arenas under one timestamp.
    #[test]
    fn the_two_encounter_keys_are_recorded_independently() {
        let mut pending = Pending::default();
        pending.arena_changed(AccountId(1), ArenaId::new(10));
        pending.arena_changed(AccountId(1), ArenaId::new(11));
        pending.timestamp_changed(AccountId(1), seen(100));

        assert_eq!(pending.changed_arenas().count(), 2);
        assert_eq!(pending.changed_timestamps().count(), 1);
    }

    /// A merge keeps what each side knows: the stored note and aliases, and the
    /// encounters and identity the live side just reported.
    #[test]
    fn merging_keeps_both_sides() {
        let (_, mut stored) = player(1, "OldName", "camps hard");
        stored.names.insert("EvenOlder".to_owned());
        stored.arena_ids.insert(ArenaId::new(10));
        stored.timestamps.insert(seen(100));
        stored.division_encounters.mark(ArenaId::new(10), seen(100));

        let (_, mut live) = player(1, "NewName", "");
        live.clan = "RAIN".to_owned();
        live.clan_id = 7;
        live.arena_ids.insert(ArenaId::new(11));
        live.timestamps.insert(seen(200));

        stored.merge_encounters_from(&live);

        assert_eq!(stored.notes, "camps hard", "the stored note survives");
        assert_eq!(stored.last_name, "NewName", "the live identity is the newer one");
        assert_eq!(stored.clan, "RAIN");
        assert!(stored.names.contains("EvenOlder"), "and the stored aliases");
        assert!(stored.names.contains("OldName"), "with the displaced name added to them");
        assert_eq!(stored.arena_ids.len(), 2, "both encounters");
        assert_eq!(stored.timestamps.len(), 2);
        assert!(stored.arena_in_division(ArenaId::new(10)), "and the mark on the stored one");
        assert!(!stored.arena_in_division(ArenaId::new(11)));
    }

    /// The two key families merge independently: a side with more arenas than
    /// timestamps keeps all of both.
    #[test]
    fn merging_does_not_pair_the_two_key_families() {
        let (_, mut stored) = player(1, "Someone", "");
        let (_, mut live) = player(1, "Someone", "");
        live.arena_ids.insert(ArenaId::new(10));
        live.arena_ids.insert(ArenaId::new(11));
        live.timestamps.insert(seen(100));

        stored.merge_encounters_from(&live);

        assert_eq!(stored.arena_ids.len(), 2);
        assert_eq!(stored.timestamps.len(), 1);
    }

    #[test]
    fn a_player_reads_back_its_division_marks() {
        let (_, mut tracked) = player(1, "Someone", "");
        tracked.arena_ids.insert(ArenaId::new(10));
        tracked.timestamps.insert(seen(100));
        tracked.division_encounters.mark(ArenaId::new(10), seen(100));

        assert!(tracked.arena_in_division(ArenaId::new(10)));
        assert!(tracked.timestamp_in_division(seen(100)));
        assert!(!tracked.arena_in_division(ArenaId::new(11)));
    }
}
