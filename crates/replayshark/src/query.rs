//! Single facts read out of replays: players, rosters, arena ids, versions.

use std::collections::BTreeMap;

use serde::Serialize;
use serde::Serializer;
use wows_battle_world::process::ProcessOptions;
use wows_battle_world::process::battle_report_for;
use wows_replays::ReplayFile;
use wows_replays::ReplayMeta;
use wows_replays::context::GameDataContext;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;
use wows_replays::types::AvatarId;
use wows_replays::types::EntityId;
use wows_replays::types::GameParamId;
use wows_replays::types::Relation;
use wows_replays::types::TeamId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use crate::analysis::replay_version;
use crate::error::AnalysisError;

/// Which players [`players`] keeps. An unset field keeps everyone.
#[derive(Debug, Clone, Default)]
pub struct PlayerFilter {
    /// Kept when the player's name contains this, ignoring case.
    pub name: Option<String>,

    /// Kept when the player's ship is this entity.
    pub entity: Option<EntityId>,

    /// Kept when the ship's name, or its GameParams index when the build has
    /// no translation, contains this, ignoring case.
    pub ship: Option<String>,
}

/// A player with their in-replay ship entity and their ship.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayerRow {
    pub player_name: String,

    /// The player's ship entity.
    pub entity_id: EntityId,
    pub avatar_id: Option<AvatarId>,
    pub account_id: AccountId,
    pub ship_id: GameParamId,

    /// The ship's GameParams index, such as `PASB110`.
    pub ship_index: String,

    /// `None` when the build has no translation for the ship.
    pub ship_name: Option<String>,
    pub team_id: TeamId,

    /// Serialized as the relation's name, such as `Ally`.
    #[serde(serialize_with = "serialize_relation_name")]
    pub relation: Relation,
    pub is_bot: bool,
}

impl PlayerRow {
    /// The ship's name, or its GameParams index when the build has no
    /// translation.
    pub fn ship_label(&self) -> &str {
        self.ship_name.as_deref().unwrap_or(&self.ship_index)
    }
}

/// A replay's ships counted by class, from the plaintext header alone.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Roster {
    /// The header's client version, `major,minor,patch,build`.
    pub version: String,

    /// The recording player's ship class. `None` when the header lists no
    /// ship under the recording player's name.
    pub own_class: Option<String>,

    /// Ship counts keyed by class name, such as `Destroyer`. A ship the
    /// build's GameParams lack has no class and counts under `Unknown`.
    pub classes: BTreeMap<String, usize>,
}

impl Roster {
    /// Whether the roster has one of `has_class` (ignoring case) and, with
    /// `exclude_own`, the recording player's ship is none of them. An empty
    /// `has_class` keeps every roster.
    pub fn matches(&self, has_class: &[String], exclude_own: bool) -> bool {
        if has_class.is_empty() {
            return true;
        }

        let wanted: Vec<String> = has_class.iter().map(|class| class.to_lowercase()).collect();
        let is_wanted = |class: &str| wanted.contains(&class.to_lowercase());
        if !self.classes.keys().any(|class| is_wanted(class)) {
            return false;
        }

        !(exclude_own && self.own_class.as_deref().is_some_and(is_wanted))
    }
}

/// Every player in `replay` that `filter` keeps, ordered by team and then
/// by name ignoring case.
pub fn players(
    replay: &ReplayFile,
    ctx: &dyn GameDataContext,
    filter: &PlayerFilter,
) -> Result<Vec<PlayerRow>, AnalysisError> {
    let version = replay_version(replay)?;
    let provider = ctx.metadata_provider(&version)?;
    let report = battle_report_for(replay, ctx, ProcessOptions::default())?;

    let name_lc = filter.name.as_deref().map(str::to_lowercase);
    let ship_lc = filter.ship.as_deref().map(str::to_lowercase);

    let mut rows: Vec<PlayerRow> = Vec::new();
    for player in report.players() {
        let state = player.initial_state();
        let vehicle = player.vehicle();
        let row = PlayerRow {
            player_name: state.username().to_string(),
            entity_id: state.entity_id(),
            avatar_id: state.avatar_id(),
            account_id: state.db_id(),
            ship_id: vehicle.id(),
            ship_index: vehicle.index().to_string(),
            ship_name: provider.localized_name_from_param(vehicle),
            team_id: TeamId::from(state.team_id()),
            relation: player.relation(),
            is_bot: player.is_bot(),
        };

        if let Some(name) = &name_lc
            && !row.player_name.to_lowercase().contains(name)
        {
            continue;
        }
        if let Some(entity) = filter.entity
            && row.entity_id != entity
        {
            continue;
        }
        if let Some(ship) = &ship_lc
            && !row.ship_label().to_lowercase().contains(ship)
        {
            continue;
        }

        rows.push(row);
    }

    rows.sort_by(|a, b| {
        a.team_id.cmp(&b.team_id).then_with(|| a.player_name.to_lowercase().cmp(&b.player_name.to_lowercase()))
    });

    Ok(rows)
}

/// `meta`'s ships counted by class. Only the header is read, so a large
/// archive can be swept without decoding packets.
pub fn roster(meta: &ReplayMeta, ctx: &dyn GameDataContext) -> Result<Roster, AnalysisError> {
    let version = Version::try_from_client_exe(&meta.clientVersionFromExe)
        .ok_or_else(|| AnalysisError::UnparsableVersion { version: meta.clientVersionFromExe.clone() })?;
    let provider = ctx.metadata_provider(&version)?;

    // A ship id absent from this version's params (removed test ship,
    // partial dump) has no class to report, so it is grouped as such rather
    // than silently dropped from the roster.
    let class_of = |ship_id| {
        provider
            .game_param_by_id(ship_id)
            .and_then(|p| p.species().and_then(|s| s.known().map(|s| format!("{s:?}"))))
            .unwrap_or_else(|| "Unknown".to_string())
    };

    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    for vehicle in &meta.vehicles {
        *classes.entry(class_of(vehicle.shipId)).or_default() += 1;
    }
    let own_class = meta.vehicles.iter().find(|v| v.name == meta.playerName).map(|v| class_of(v.shipId));

    Ok(Roster { version: meta.clientVersionFromExe.clone(), own_class, classes })
}

/// The arena id `replay` records. Every recording of one match has the same
/// one. `None` when the replay never reaches the arena state.
pub fn arena_id(replay: &ReplayFile, ctx: &dyn GameDataContext) -> Result<Option<ArenaId>, AnalysisError> {
    let version = replay_version(replay)?;
    let specs = ctx.entity_specs(&version)?;

    Ok(wows_replays::analyzer::arena_scan::scan_arena_id(&specs, version, replay))
}

/// The game version in `meta`. `None` when the header's version does not
/// parse. Needs no game data.
pub fn game_version(meta: &ReplayMeta) -> Option<Version> {
    Version::try_from_client_exe(&meta.clientVersionFromExe)
}

fn serialize_relation_name<S>(relation: &Relation, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(relation.name())
}
