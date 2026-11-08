use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Km;
use wows_ship_stats::BuildError;

use crate::armor_trace::TraceWeapon;
use crate::armor_trace::WeaponAmmo;
use crate::replay_worker::ByteCount;
use crate::shells::Ammo;
use crate::ships::ShipClass;
use crate::ships::Tier;

/// Failure to load a game data build.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{dir} is not a game data dump: it has no readable metadata.toml")]
    NotADump { dir: PathBuf },

    #[error("{root} lists no game data builds in builds.toml")]
    NoBuilds { root: PathBuf },

    #[error("failed to load the game data builds index")]
    BuildsIndex {
        #[source]
        source: wows_data_mgr::error::BuildsIndexError,
    },

    #[error("game data dump {dir} has no {file}")]
    MissingFile { dir: PathBuf, file: &'static str },

    #[error("game version {version:?} in {dir} is not major.minor.patch")]
    Version { dir: PathBuf, version: String },

    #[error("failed to read {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse GameParams in {dir}")]
    GameParams {
        dir: PathBuf,
        #[source]
        source: wowsunpack::error::GameDataError,
    },

    #[error("failed to parse translations {path}")]
    Translations {
        path: PathBuf,
        #[source]
        source: gettext::Error,
    },

    #[error("the game data build index lists no builds")]
    NoRemoteBuilds,

    #[error("failed to download game data")]
    Download,
}

/// A query that cannot be answered from the loaded data. Messages are written
/// for the person or model that made the query.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("no ship matches {query:?}; {}", did_you_mean(suggestions, "try find_ships with a shorter name"))]
    ShipNotFound { query: String, suggestions: Vec<String> },

    #[error("{query:?} matches several ships: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousShip { query: String, candidates: Vec<String> },

    #[error("{query:?} asks for the tier 11 ship researched from {ship}, but {ship} leads to no tier 11 ship")]
    NoSupership { query: String, ship: String },

    #[error("{query:?} names {ship}, which is tier {}, not tier {}", actual.0, tier.0)]
    TierMismatch { query: String, ship: String, tier: Tier, actual: Tier },

    #[error("{query:?} names {ship}, which has no {variant}; leave out the qualifier or name the ship by its index")]
    NoShipVariant { query: String, ship: String, variant: &'static str },

    #[error("{ship} has no armor zone or plate named {zone:?}; zones: {}", zones.join(", "))]
    ZoneNotFound { ship: String, zone: String, zones: Vec<&'static str> },

    #[error("{plate:?} names several plates of {ship}: {}; give one of these names or a zone", candidates.join(", "))]
    AmbiguousPlate { ship: String, plate: String, candidates: Vec<String> },

    #[error(
        "no captain skill matches {query:?}; {}",
        did_you_mean(suggestions, "use the skill's English name or a common acronym such as IFHE")
    )]
    SkillNotFound { query: String, suggestions: Vec<String> },

    #[error("{}", skill_not_for_class(query, skill, *class, ship.as_deref(), classes))]
    SkillNotForClass {
        query: String,
        /// English name of the skill `query` resolved to.
        skill: String,
        class: ShipClass,
        ship: Option<String>,
        /// Classes whose skill tree has the skill.
        classes: Vec<ShipClass>,
    },

    #[error("{query:?} matches several captain skills: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousSkill { query: String, candidates: Vec<String> },

    #[error("{ship} does not end a tech tree line; name a tier 10 tech tree ship")]
    NotLineEnd { ship: String },

    #[error(
        "no upgrade matches {query:?}; {}",
        did_you_mean(suggestions, "use the upgrade's English name, e.g. \"Main Battery Modification 2\"")
    )]
    UpgradeNotFound { query: String, suggestions: Vec<String> },

    #[error("{query:?} matches several upgrades: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousUpgrade { query: String, candidates: Vec<String> },

    #[error(
        "no unique commander matches {query:?}; {}",
        did_you_mean(suggestions, "use the commander's English name, e.g. \"Yamamoto Isoroku\"")
    )]
    CommanderNotFound { query: String, suggestions: Vec<String> },

    #[error("{query:?} matches several unique commanders: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousCommander { query: String, candidates: Vec<String> },

    #[error(
        "{commander} does not serve on {ship}; they serve on ships of {}{}",
        nations.join(", "),
        if ships.is_empty() { String::new() } else { format!(" and on {}", ships.join(", ")) }
    )]
    CommanderNotForShip { commander: String, ship: String, nations: Vec<String>, ships: Vec<String> },

    #[error("no game mode with buffs matches {query:?}; modes: {}", modes.join(", "))]
    GameModeNotFound { query: String, modes: Vec<String> },

    #[error("buff drop {drop} has no maxBuffLevel")]
    BuffDropWithoutStackLimit { drop: String },

    #[error("upgrade {upgrade} does not fit {ship}")]
    UpgradeNotApplicable { upgrade: String, ship: String },

    #[error("upgrades {first} and {second} both use slot {slot}; a ship mounts one upgrade per slot")]
    UpgradeSlotConflict { first: String, second: String, slot: u8 },

    #[error(
        "no consumable matches {query:?}; {}",
        did_you_mean(suggestions, "use the consumable's English name, e.g. \"Surveillance Radar\"")
    )]
    ConsumableNotFound { query: String, suggestions: Vec<String> },

    #[error("{query:?} matches several consumables: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousConsumable { query: String, candidates: Vec<String> },

    #[error("{ship} has no {consumable}")]
    ConsumableNotOnShip { consumable: String, ship: String },

    #[error("{ship} has no module matching {query:?}; modules: {}", modules.join(", "))]
    ModuleNotFound { ship: String, query: String, modules: Vec<String> },

    #[error("{query:?} matches several modules: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousModule { query: String, candidates: Vec<String> },

    #[error("modules {first} and {second} are both {kind}; a ship mounts one module of each kind")]
    ModuleSlotConflict { first: String, second: String, kind: &'static str },

    #[error(
        "the game data has no hull model hardpoints for {ship}, so its mount facings are unknown; the dump lacks the armor extract for this ship, which wows-data-mgr dump-renderer-data writes unless given --no-armor"
    )]
    HullFacingsUnavailable { ship: String },

    #[error("module {module} does not fit {ship}'s {hull}; add {} to modules", hulls.join(" or "))]
    ModuleNeedsHull { ship: String, module: String, hull: String, hulls: Vec<String> },

    #[error("{ship}'s {hull} does not take the stock {kind}; add {} to modules", modules.join(" or "))]
    HullNeedsModule { ship: String, hull: String, kind: &'static str, modules: Vec<String> },

    #[error("{ship}'s hull model has no hardpoint {hardpoint}, so that mount's facing is unknown")]
    MountFacingMissing { ship: String, hardpoint: String },

    #[error("{ship}'s hull has no fire nodes")]
    FireNodesMissing { ship: String },

    #[error("{ship}'s hulls list no hull sections with HP pools")]
    HitLocationsMissing { ship: String },

    #[error("no stat matches {query:?}; {}", did_you_mean(suggestions, "list stat names with stat_names"))]
    StatNotFound { query: String, suggestions: Vec<String> },

    #[error("no stat matches {query:?}; use stat {stat} with qualifier {:?}", ammo.label())]
    StatNameNamesAmmo { query: String, stat: String, ammo: Ammo },

    #[error("stat {stat} is yes or no, not a number; it cannot be compared")]
    StatNotNumeric { stat: &'static str },

    #[error("rank.limit must be from 1 to {max}, not {limit}")]
    RankLimitOutOfRange { limit: usize, max: usize },

    #[error("{reason}")]
    InvalidArguments { reason: &'static str },

    #[error(
        "{ship} has no special ability (neither Combat Instructions nor an alternative fire mode); ask again without special_ability_active"
    )]
    NoSpecialAbility { ship: String },

    #[error("failed to compute the stats of {ship}")]
    Build {
        ship: String,
        #[source]
        source: BuildError,
    },

    #[error("not a readable World of Warships replay")]
    Replay(#[source] wows_replays::ParseError),

    #[error("no GameParams entry has the name, index or id {query:?}")]
    ParamNotFound { query: String },

    /// Shared because a failed build is reported to every call until it is
    /// retried.
    #[error("raw GameParams are not available: {source}")]
    RawParams {
        #[source]
        source: Arc<RawParamsError>,
    },

    #[error(transparent)]
    Jq(#[from] JqError),

    #[error("the armor model of {ship} in this game data is unreadable")]
    ArmorUnreadable {
        ship: String,
        #[source]
        source: ArmorReadError,
    },

    #[error(
        "this game data holds no armor model for {ship}; the dump lacks the armor extract for this ship, which wows-data-mgr dump-renderer-data writes unless given --no-armor"
    )]
    ArmorMissing { ship: String },

    #[error("{ship} has no {weapon}")]
    WeaponNotFound { ship: String, weapon: String },

    #[error("{}", ammo_not_carried(ship, *weapon, aircraft.as_deref(), *ammo, fitting, available))]
    AmmoNotCarried {
        ship: String,
        weapon: TraceWeapon,
        /// The aircraft asked for, when bombs were limited to one.
        aircraft: Option<String>,
        ammo: Ammo,
        /// Ammo types of `weapon`, to ask for instead.
        fitting: Vec<Ammo>,
        /// Every traceable weapon of `ship`.
        available: Vec<WeaponAmmo>,
    },

    #[error("projectile {projectile} is not in this game data")]
    ProjectileNotFound { projectile: String },

    #[error("{range} is beyond the maximum range of {projectile}")]
    OutOfRange { projectile: String, range: Km },

    #[error("the ordnance of {aircraft} cannot be traced: {reason}")]
    OrdnanceNotTraceable { aircraft: String, reason: &'static str },

    #[error("{ship}'s armor model has no citadel plates")]
    NoCitadel { ship: String },

    #[error("projectile {projectile} lacks data the ballistic solution needs")]
    ProjectileIncomplete { projectile: String },
}

impl QueryError {
    /// Names an ambiguous query matched equally. Empty for other errors.
    pub fn candidates(&self) -> &[String] {
        match self {
            Self::AmbiguousShip { candidates, .. }
            | Self::AmbiguousPlate { candidates, .. }
            | Self::AmbiguousSkill { candidates, .. }
            | Self::AmbiguousUpgrade { candidates, .. }
            | Self::AmbiguousCommander { candidates, .. }
            | Self::AmbiguousConsumable { candidates, .. }
            | Self::AmbiguousModule { candidates, .. } => candidates,
            _ => &[],
        }
    }

    /// Names near a query that matched nothing. Empty for other errors.
    pub fn suggestions(&self) -> &[String] {
        match self {
            Self::ShipNotFound { suggestions, .. }
            | Self::SkillNotFound { suggestions, .. }
            | Self::UpgradeNotFound { suggestions, .. }
            | Self::CommanderNotFound { suggestions, .. }
            | Self::ConsumableNotFound { suggestions, .. }
            | Self::StatNotFound { suggestions, .. } => suggestions,
            _ => &[],
        }
    }
}

/// Failure to build or read the raw GameParams store.
#[derive(Debug, thiserror::Error)]
pub enum RawParamsError {
    #[error("the game data was loaded without a cache directory to hold them")]
    NoCacheDir,

    #[error("failed to open GameParams.data in {dir}")]
    Source {
        dir: PathBuf,
        #[source]
        source: wowsunpack::vfs::VfsError,
    },

    #[error("failed to decode GameParams.data")]
    Decode(#[source] wowsunpack::error::GameDataError),

    #[error("failed to encode GameParams entry {name} as JSON")]
    Encode {
        name: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("GameParams entry {name} is {len} bytes, more than a record holds")]
    RecordTooLarge { name: String, len: usize },

    #[error("I/O error on {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read or write the index {path}")]
    Index {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("{path} is {found} bytes; its index expects {expected}")]
    Truncated { path: PathBuf, expected: u64, found: u64 },

    #[error("the stored record of {name} is damaged")]
    DamagedRecord { name: String },

    #[error("{path} belongs to build {found}, not {expected}")]
    WrongBuild { path: PathBuf, expected: u32, found: u32 },

    #[error("the store published at {dir} was removed before it could be opened")]
    Vanished { dir: PathBuf },
}

/// A jq filter that does not compile. Messages are written for the model
/// that wrote the filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum CompileError {
    #[error("jq syntax error: {}", errors.join("; "))]
    Syntax { errors: Vec<String> },

    #[error("jq filter uses undefined {}", names.join(", "))]
    Undefined { names: Vec<String> },

    #[error("jq modules (import, include) are not available")]
    Module,

    #[error("names starting with __ are reserved; the filter uses {name}")]
    Reserved { name: String },
}

/// Failure to evaluate a jq filter.
#[derive(Debug, thiserror::Error)]
pub enum JqError {
    #[error(transparent)]
    Compile(#[from] CompileError),

    #[error("jq evaluation is not set up in this program; call wows_query::jq_worker::init() first in main")]
    NoWorker,

    #[error("failed to start the jq worker {program}")]
    Spawn {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to create the jq worker's working directory {path}")]
    Workdir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("all jq workers stayed busy for {waited:?}; retry later")]
    Busy { waited: std::time::Duration },

    #[error("the jq worker exited before it read the request ({}); check that main calls wows_query::jq_worker::init() first", exit_description(.status.as_ref()))]
    Exited { status: Option<std::process::ExitStatus> },

    #[error("a jq input of {bytes} bytes exceeds the 4 GiB record limit")]
    InputTooLarge { bytes: usize },

    #[error("the jq worker sent an unreadable reply")]
    Reply(#[source] serde_json::Error),
}

/// Failure to read a replay in a replay worker.
#[derive(Debug, thiserror::Error)]
pub enum ReplayWorkerError {
    #[error("replay reading is not set up in this program; call wows_query::replay_worker::init() first in main")]
    NoWorker,

    #[error("the replay is {} bytes, more than the {} byte limit", bytes.get(), limit.get())]
    TooLarge { bytes: ByteCount, limit: ByteCount },

    #[error("failed to start the replay worker {program}")]
    Spawn {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to create the replay worker's working directory {path}")]
    Workdir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("the replay worker stayed busy for {waited:?}; retry later")]
    Busy { waited: std::time::Duration },

    #[error("reading the replay took longer than {limit:?}")]
    TimedOut { limit: std::time::Duration },

    #[error("the replay worker stopped without an answer ({})", exit_description(.status.as_ref()))]
    Crashed { status: Option<std::process::ExitStatus> },

    #[error("the replay worker sent an unreadable reply")]
    Reply(#[source] serde_json::Error),

    #[error("the replay worker answered out of order")]
    UnexpectedReply,

    #[error(transparent)]
    Failed(#[from] crate::replay_worker::WorkerFailure),
}

fn exit_description(status: Option<&std::process::ExitStatus>) -> String {
    match status {
        Some(status) => status.to_string(),
        None => "exit status unknown".to_owned(),
    }
}

/// "did you mean: A, B?" when there are suggestions, otherwise `hint`.
fn did_you_mean(suggestions: &[String], hint: &str) -> String {
    if suggestions.is_empty() {
        return hint.to_owned();
    }

    format!("did you mean: {}?", suggestions.join(", "))
}

/// "Survivability Expert (SE) is not in the battleship skill tree (G.
/// Kurfuerst); it is in the destroyer, cruiser and aircraft carrier trees".
fn skill_not_for_class(
    query: &str,
    skill: &str,
    class: ShipClass,
    ship: Option<&str>,
    classes: &[ShipClass],
) -> String {
    let named = if query.eq_ignore_ascii_case(skill) { skill.to_owned() } else { format!("{skill} ({query})") };
    let ship = ship.map(|ship| format!(" ({ship})")).unwrap_or_default();
    let labels: Vec<String> = classes.iter().map(|class| class.label().replace('_', " ")).collect();
    let elsewhere = match labels.split_last() {
        None => String::new(),
        Some((only, [])) => format!("; it is in the {only} tree"),
        Some((last, rest)) => format!("; it is in the {} and {last} trees", rest.join(", ")),
    };

    format!("{named} is not in the {} skill tree{ship}{elsewhere}", class.label().replace('_', " "))
}

/// "Aki has no AP bombs; its Suisei dive bombers carry HE bombs, its main
/// battery fires AP and HE; call again with ammo HE or without ammo". The
/// weapon asked for is described first.
fn ammo_not_carried(
    ship: &str,
    weapon: TraceWeapon,
    aircraft: Option<&str>,
    ammo: Ammo,
    fitting: &[Ammo],
    available: &[WeaponAmmo],
) -> String {
    let missing = match (weapon, aircraft) {
        (TraceWeapon::Bombs, Some(aircraft)) => format!("{aircraft} with {} bombs", ammo.label()),
        (TraceWeapon::Bombs, None) => format!("{} bombs", ammo.label()),
        (battery, _) => format!("{} {}", battery.label(), ammo.label()),
    };
    let (asked, others): (Vec<&WeaponAmmo>, Vec<&WeaponAmmo>) =
        available.iter().partition(|weapon_ammo| weapon_ammo.source.weapon() == weapon);
    // Another squadron carries the ammo the named aircraft lacks.
    let other_aircraft = aircraft.is_some() && asked.iter().any(|weapon_ammo| weapon_ammo.ammo.contains(&ammo));
    let weapons: Vec<String> = asked.into_iter().chain(others).map(WeaponAmmo::describe).collect();
    let fitting: Vec<&str> = fitting.iter().map(|ammo| ammo.label()).collect();
    let retry = if other_aircraft {
        format!("call again with ammo {}, without ammo, or without aircraft", fitting.join(" or "))
    } else {
        format!("call again with ammo {} or without ammo", fitting.join(" or "))
    };

    format!("{ship} has no {missing}; {}; {retry}", weapons.join(", "))
}

/// Why an armor extract object could not be read from a dump.
#[derive(Debug, thiserror::Error)]
pub enum ArmorReadError {
    #[error("dump lookup failed")]
    Vfs(#[from] wowsunpack::vfs::VfsError),

    #[error("reading the object failed")]
    Io(#[from] std::io::Error),

    #[error("the object is not a readable armor extract")]
    Format(#[from] wowsunpack::export::armor_extract::ArmorExtractError),
}
