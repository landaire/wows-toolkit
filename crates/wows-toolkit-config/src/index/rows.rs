use std::path::PathBuf;

use jiff::Timestamp;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;

/// Identifies one replay group (the live dir, an imported dir, or an ad-hoc set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SourceId(pub i64);

/// Identifies one row of `replay_record`: a single replay file's perspective on
/// a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordId(pub i64);

/// Runtime handle for one open replay workspace. `SourceId` is the durable
/// identity; this is only ever valid within a single run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceId(pub u64);

impl WorkspaceId {
    /// The workspace backed by the game's own replays directory.
    pub const LIVE: WorkspaceId = WorkspaceId(0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Live,
    ImportedDir,
    AdHoc,
}

impl SourceKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            SourceKind::Live => "live",
            SourceKind::ImportedDir => "imported_dir",
            SourceKind::AdHoc => "ad_hoc",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "live" => Some(SourceKind::Live),
            "imported_dir" => Some(SourceKind::ImportedDir),
            "ad_hoc" => Some(SourceKind::AdHoc),
            _ => None,
        }
    }
}

/// Perspective-relative battle outcome. `Unknown` when the player left before
/// results were written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MatchOutcome {
    Win,
    Loss,
    Draw,
    Unknown,
}

impl MatchOutcome {
    pub fn as_db_str(self) -> &'static str {
        match self {
            MatchOutcome::Win => "win",
            MatchOutcome::Loss => "loss",
            MatchOutcome::Draw => "draw",
            MatchOutcome::Unknown => "unknown",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "win" => Some(MatchOutcome::Win),
            "loss" => Some(MatchOutcome::Loss),
            "draw" => Some(MatchOutcome::Draw),
            "unknown" => Some(MatchOutcome::Unknown),
            _ => None,
        }
    }
}

/// A roster vehicle's relation to the indexing replay's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VehicleRelation {
    SelfPlayer,
    Ally,
    Enemy,
}

impl VehicleRelation {
    pub fn as_db_str(self) -> &'static str {
        match self {
            VehicleRelation::SelfPlayer => "self",
            VehicleRelation::Ally => "ally",
            VehicleRelation::Enemy => "enemy",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "self" => Some(VehicleRelation::SelfPlayer),
            "ally" => Some(VehicleRelation::Ally),
            "enemy" => Some(VehicleRelation::Enemy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IndexSource {
    pub id: SourceId,
    pub name: String,
    pub kind: SourceKind,
    pub root_path: Option<PathBuf>,
}

/// Objective, server-authoritative match facts (one row per `arena_id`).
#[derive(Debug, Clone)]
pub struct ObjectiveMatch {
    pub arena_id: ArenaId,
    pub timestamp: Timestamp,
    pub map: String,
    pub game_mode: String,
    pub game_type: String,
    pub match_group: String,
    pub version_build: Option<u32>,
}

/// A specific replay file of a match, within a group, with its perspective.
#[derive(Debug, Clone)]
pub struct ReplayRecord {
    pub arena_id: ArenaId,
    pub source_id: SourceId,
    pub replay_path: PathBuf,
    pub file_mtime: Option<i64>,
    pub outcome: MatchOutcome,
    pub self_account_id: Option<AccountId>,
    pub self_ship_id: Option<GameParamId>,
    pub self_survived: Option<bool>,
    pub self_damage: Option<u64>,
    pub self_kills: Option<i64>,
    pub self_pr: Option<f64>,
    pub results_available: bool,
    pub indexed_at: Timestamp,
}

/// A roster vehicle (objective, shared across perspectives).
#[derive(Debug, Clone)]
pub struct IndexedVehicleRow {
    pub arena_id: ArenaId,
    pub account_id: AccountId,
    pub player_name: String,
    pub clan: String,
    pub realm: Option<String>,
    pub ship_id: GameParamId,
    pub ship_index: String,
    pub ship_name: String,
    pub nation: String,
    pub species: String,
    pub tier: u32,
    pub relation: VehicleRelation,
    pub division_id: Option<i64>,
    pub survived: Option<bool>,
    pub damage: Option<u64>,
    pub kills: Option<i64>,
    pub spotting: Option<u64>,
    pub potential: Option<u64>,
    pub received: Option<u64>,
    pub pr: Option<f64>,
    pub is_test_ship: bool,
    /// `Some(true)` if the player had a mid-match disconnect: a `Disconnected`
    /// connection event that was not accompanied by a death. `Some(false)` if no
    /// such disconnect was observed -- this covers both a player connected
    /// throughout and a replay build that never reported connection state.
    /// `None` only appears on legacy rows written before this column existed;
    /// the live indexing path always writes `Some`.
    pub disconnected: Option<bool>,
    /// `None` if stream-sniper detection has not been computed for this row (legacy
    /// rows, or Twitch data unavailable at index time). `Some(true)` if a Twitch
    /// chatter login was fuzzy-matched to this player near the match's start time;
    /// `Some(false)` if detection ran and found no match.
    pub is_stream_sniper: Option<bool>,
    /// The Twitch chatter login that triggered `is_stream_sniper = Some(true)`, if any.
    pub sniper_twitch_login: Option<String>,
}

/// A search/recent result: objective match plus the chosen record's perspective.
#[derive(Debug, Clone)]
pub struct MatchHit {
    pub arena_id: ArenaId,
    pub timestamp: Timestamp,
    pub map: String,
    pub game_mode: String,
    pub game_type: String,
    pub match_group: String,
    pub version_build: Option<u32>,
    pub source_id: SourceId,
    pub outcome: MatchOutcome,
    pub self_account_id: Option<AccountId>,
    pub self_ship_id: Option<GameParamId>,
    /// The `indexed_vehicle.ship_name` recorded for `self_ship_id` in this
    /// match, written when the match was indexed with its own build's game data
    /// loaded. Lets a hit name its ship even when that build's data is no
    /// longer installed. `None` when the record has no self ship, or when the
    /// roster carries no row for it.
    pub self_ship_name: Option<String>,
    pub self_survived: Option<bool>,
    pub self_damage: Option<u64>,
    pub self_kills: Option<i64>,
    pub self_pr: Option<f64>,
    pub results_available: bool,
    pub replay_path: PathBuf,
    pub file_mtime: Option<i64>,
}

/// Which stored PR column a repair targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrTarget {
    /// `replay_record.self_pr` for one record.
    Record(RecordId),
    /// `indexed_vehicle.pr` for one roster row, keyed by that table's primary key.
    Vehicle { arena_id: ArenaId, account_id: AccountId, ship_id: GameParamId },
}

/// The stats a single-battle PR calculation reads, as an index row stores them.
#[derive(Debug, Clone, Copy)]
pub struct PrInputs {
    pub ship_id: GameParamId,
    pub damage: u64,
    pub kills: i64,
    pub is_win: bool,
}

/// A stored row whose PR column is NULL, with everything a single-battle PR
/// calculation needs. Read out of the index so the calculation can happen in
/// the crate that owns the expected-values data.
#[derive(Debug, Clone, Copy)]
pub struct PrGap {
    pub target: PrTarget,
    pub inputs: PrInputs,
}

/// A computed PR to write back into the row identified by `target`.
#[derive(Debug, Clone, Copy)]
pub struct PrRepair {
    pub target: PrTarget,
    pub pr: f64,
}

/// Another player who shared the perspective player's division.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DivisionMate {
    pub player_name: String,
    /// Empty when the player had no clan.
    pub clan: String,
}

/// Everything one replay-listing row draws beyond its identity, for a single
/// record. Keyed by `replay_path` within a source.
#[derive(Debug, Clone)]
pub struct RowSummary {
    pub outcome: MatchOutcome,
    pub self_damage: Option<u64>,
    pub self_kills: Option<i64>,
    pub self_survived: Option<bool>,
    pub self_pr: Option<f64>,
    /// The self player's division, resolved from the roster row belonging to
    /// `replay_record.self_account_id`. `None` when the perspective player had
    /// no account (a spectator recording), had no roster row, or was solo.
    pub division_id: Option<i64>,
    /// The other players who shared `division_id`, excluding the self player.
    /// Empty when solo or unknown. Ordered by `player_name`.
    pub division_mates: Vec<DivisionMate>,
    pub results_available: bool,
    /// Modification time of the file as of indexing. Compared against the
    /// on-disk value to decide whether the row is stale.
    pub file_mtime: Option<i64>,
}

/// A roster row whose clan differed from that account's latest known clan.
#[derive(Debug, Clone)]
pub struct ClanCorrection {
    pub account_id: AccountId,
    pub arena_id: ArenaId,
    pub timestamp: Timestamp,
    pub clan: String,
}

/// One encounter in which an account shared the self player's division.
///
/// Carries the arena and the timestamp because a consumer that dedups
/// encounters under either key has to be able to mark the same encounter under
/// both.
#[derive(Debug, Clone)]
pub struct DivisionMateEncounter {
    pub account_id: AccountId,
    pub arena_id: ArenaId,
    pub timestamp: Timestamp,
}

/// A distinct player seen in the index (for palette/Search facets).
#[derive(Debug, Clone)]
pub struct PlayerFacet {
    pub account_id: AccountId,
    pub latest_name: String,
    pub clan: String,
    pub match_count: i64,
}

/// A distinct ship the user has played (for palette/Search facets).
#[derive(Debug, Clone)]
pub struct ShipFacet {
    pub ship_id: GameParamId,
    pub ship_name: String,
    pub match_count: i64,
}

/// Optional constraints for a match search. All `None` matches everything.
#[derive(Debug, Clone, Default)]
pub struct MatchFilter {
    pub source_ids: Option<Vec<SourceId>>,
    pub outcome: Option<MatchOutcome>,
    pub self_ship: Option<GameParamId>,
    pub species: Option<String>,
    pub tier: Option<u32>,
    pub map: Option<String>,
    pub game_type: Option<String>,
    pub date_from: Option<Timestamp>,
    pub date_to: Option<Timestamp>,
    pub self_damage_min: Option<u64>,
    pub self_damage_max: Option<u64>,
    pub self_survived: Option<bool>,
    pub player_present: Option<AccountId>,
    pub enemy_ship: Option<GameParamId>,
}

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("index database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("unknown source: {0:?}")]
    UnknownSource(SourceId),
    /// `ensure_source` inserted, then failed to find a row for `root_path` --
    /// and for `Live`, also failed the kind-level lookup. The row it would have
    /// resolved to is gone by the time the reads run.
    #[error("failed to create or resolve index source for {root_path:?}")]
    SourceCreationFailed { root_path: PathBuf },
    /// `relocate_source` found a record whose rewritten path already names a
    /// different, non-relocating record in the same source. Rewriting it
    /// would collide on `replay_record`'s `(source_id, replay_path)` uniqueness,
    /// so the relocation is rejected before any row is touched.
    #[error("relocation target {path:?} already exists in the destination")]
    RelocationCollision { path: PathBuf },
    /// `relocate_source` rejected a request where `old_root` and `new_root`
    /// name the same directory, or one is an ancestor directory of the other.
    /// Rewriting records under a root that is itself moving cannot be done as
    /// a single prefix substitution: a record's rewritten path could land on
    /// another record's not-yet-rewritten path.
    #[error("cannot relocate {old_root:?} to {new_root:?}: one contains the other")]
    RelocationNested { old_root: PathBuf, new_root: PathBuf },
    /// `relocate_source` rejected a request to point `root_path` at a value
    /// another source (`owner`) already has. `index_source.root_path` is
    /// unique among non-NULL values.
    #[error("cannot relocate to {root_path:?}: already owned by source {owner:?}")]
    RootAlreadyOwned { root_path: PathBuf, owner: SourceId },
}
