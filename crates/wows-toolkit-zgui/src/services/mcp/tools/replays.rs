//! One replay: its header, and its complete results.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use wows_core::game_types::ArenaId;
use wows_mcp::tool_set::typed_tool;
use wows_replays::ReplayFile;
use wows_toolkit_config::index::query;
use wows_toolkit_replay_data::load::parse_replay;

use crate::services::mcp::tools::ReplayTools;
use crate::services::mcp::tools::blocking;
use crate::services::mcp::tools::error::ToolError;
use crate::services::mcp::tools::shown;

pub const HEADER: &str = "replay_header";
pub const RESULTS: &str = "replay_battle_results";

/// Replays are tens of MB; the bound keeps a device or a mislabeled file from being read whole.
const REPLAY_LIMIT: u64 = 256 * 1024 * 1024;

/// One replay, by the index's arena id or by path. Exactly one is given.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplayRef {
    /// The match's arena id, as `replay_index_search` gives it.
    #[serde(default)]
    pub arena_id: Option<i64>,

    /// A .wowsreplay file inside the game's replays folder or a folder the index holds.
    #[serde(default)]
    pub path: Option<String>,
}

impl ReplayRef {
    /// The replay file. An indexed replay is the file the index recorded; a path must name a
    /// replay inside the replay folders.
    pub(super) async fn resolve(&self, tools: &ReplayTools) -> Result<PathBuf, ToolError> {
        let path = match (self.arena_id, self.path.as_deref()) {
            (Some(_), Some(_)) => {
                return Err(ToolError::InvalidArgument {
                    message: "name the replay by arena_id or by path, not both".to_owned(),
                });
            }
            (None, None) => {
                return Err(ToolError::InvalidArgument {
                    message: "name the replay by arena_id or by path".to_owned(),
                });
            }
            (Some(arena_id), None) => {
                let pool = tools.index()?;
                query::replay_path_for_arena(&pool, ArenaId::new(arena_id))
                    .await
                    .map_err(|source| ToolError::Index { source })?
                    .ok_or(ToolError::UnknownArena { arena_id })?
            }
            (None, Some(path)) => {
                let roots = tools.replay_roots().await?;
                let resolved = roots.resolve(Path::new(path)).map_err(|source| ToolError::ReplayPath { source })?;
                let is_replay =
                    resolved.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("wowsreplay"));
                if !is_replay {
                    return Err(ToolError::NotAReplay { path: PathBuf::from(path) });
                }
                resolved
            }
        };

        let metadata = std::fs::metadata(&path)
            .map_err(|error| ToolError::ReplayRead { path: path.clone(), reason: error.to_string() })?;
        // A FIFO would block the parse that opens it, holding a parse permit, until a writer came.
        if !metadata.is_file() {
            return Err(ToolError::NotAReplay { path });
        }
        let size = metadata.len();
        if size > REPLAY_LIMIT {
            return Err(ToolError::ReplayTooLarge { path, size });
        }

        Ok(path)
    }
}

/// Arguments for `replay_header`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeaderArgs {
    pub replay: ReplayRef,
}

/// What the replay file says about its match before any game data is read.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HeaderAnswer {
    pub replay_path: String,
    pub player_name: String,
    /// The recording player's ship, such as `PASB110-Vermont`.
    pub player_vehicle: String,
    pub map: String,
    pub map_id: u32,
    /// The match's name, such as `12x12`.
    pub match_name: String,
    pub scenario: String,
    pub game_type: Option<String>,
    pub match_group: Option<String>,
    /// `major,minor,patch,build`.
    pub client_version: String,
    /// The local recording time, `dd.mm.yyyy hh:mm:ss`.
    pub date_time: String,
    /// The match's time limit in seconds.
    pub duration_limit: u32,
    pub players_per_team: u32,
    pub vehicles: Vec<HeaderVehicle>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HeaderVehicle {
    pub name: String,
    pub ship_id: u64,
    /// 0 the recording player, 1 an ally, 2 an enemy.
    pub relation: u32,
}

/// Arguments for `replay_battle_results`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResultsArgs {
    pub replay: ReplayRef,

    /// The parts to return. Default `battle_report` and `battle_results`.
    #[serde(default)]
    pub include: Option<Vec<ResultsPart>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResultsPart {
    /// The match and every player's ship, build and results, as the app's report shows them.
    BattleReport,
    /// The server's battle results with every field named.
    BattleResults,
    /// The server's battle results as the replay holds them, positional arrays included.
    RawBattleResults,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ResultsAnswer {
    pub replay_path: String,
    pub arena_id: i64,
    /// The build whose game data read the replay.
    pub build: Option<u32>,
    /// Whether the server's results are still to come, as for a battle that has only just ended.
    pub results_pending: bool,
    pub battle_report: Option<Value>,
    pub battle_results: Option<Value>,
    pub raw_battle_results: Option<Value>,
}

pub fn header_tool() -> Tool {
    typed_tool::<HeaderArgs, HeaderAnswer>(
        HEADER,
        "Read a replay's header without game data: map, mode, client version, recording time, the recording player and every player's name and ship id.",
    )
}

pub fn results_tool() -> Tool {
    typed_tool::<ResultsArgs, ResultsAnswer>(
        RESULTS,
        "The complete results of one replay's match: the battle report (every player's ship, build, damage, ribbons and achievements) and the server's battle results. Needs the game data of the replay's build.",
    )
}

pub async fn header(tools: &ReplayTools, args: HeaderArgs) -> Result<HeaderAnswer, ToolError> {
    let path = args.replay.resolve(tools).await?;

    let read = path.clone();
    let meta = blocking(move || ReplayFile::meta_from_file(&read))
        .await?
        .map_err(|report| ToolError::ReplayRead { path: path.clone(), reason: report.current_context().to_string() })?;

    Ok(HeaderAnswer {
        replay_path: shown(&path),
        player_name: meta.playerName,
        player_vehicle: meta.playerVehicle,
        map: meta.mapDisplayName,
        map_id: meta.mapId,
        match_name: meta.name,
        scenario: meta.scenario,
        game_type: meta.gameType,
        match_group: meta.matchGroup,
        client_version: meta.clientVersionFromExe,
        date_time: meta.dateTime,
        duration_limit: meta.duration,
        players_per_team: meta.playersPerTeam,
        vehicles: meta
            .vehicles
            .into_iter()
            .map(|vehicle| HeaderVehicle {
                name: vehicle.name,
                ship_id: vehicle.shipId.raw(),
                relation: vehicle.relation,
            })
            .collect(),
    })
}

pub async fn results(tools: &ReplayTools, args: ResultsArgs) -> Result<ResultsAnswer, ToolError> {
    // A match's results are a few hundred KB; more is a part the caller can leave out.
    const ANSWER_LIMIT: usize = 8 * 1024 * 1024;

    let include = args.include.unwrap_or_else(|| vec![ResultsPart::BattleReport, ResultsPart::BattleResults]);
    let path = args.replay.resolve(tools).await?;
    let Some(game_data) = tools.context().game_data else {
        return Err(ToolError::NoGameData);
    };

    // Moved into the blocking parse, which runs on after a cancelled call.
    let permit = Arc::clone(&tools.parses)
        .acquire_owned()
        .await
        .map_err(|error| ToolError::Task { reason: error.to_string() })?;
    let read = path.clone();
    let parsed = blocking(move || {
        let _permit = permit;
        parse_replay(&read, &game_data, None)
    })
    .await?
    .map_err(|source| ToolError::ReplayLoad { source })?;

    let wants = |part: ResultsPart| include.contains(&part);
    let battle_report = wants(ResultsPart::BattleReport)
        .then(|| serde_json::to_value(&parsed.indexable.normalized))
        .transpose()
        .map_err(encode_failure)?;
    let battle_results = match (wants(ResultsPart::BattleResults), &parsed.mapped_results_json) {
        (true, Some(json)) => Some(serde_json::from_str(json).map_err(encode_failure)?),
        _ => None,
    };
    let raw_battle_results = match (wants(ResultsPart::RawBattleResults), &parsed.raw_results_json) {
        (true, Some(json)) => Some(serde_json::from_str(json).map_err(encode_failure)?),
        _ => None,
    };
    let answer = ResultsAnswer {
        replay_path: shown(&path),
        arena_id: parsed.indexable.arena_id.raw(),
        build: parsed.indexable.version_build,
        results_pending: parsed.indexable.results_pending,
        battle_report,
        battle_results,
        raw_battle_results,
    };

    let size = serde_json::to_vec(&answer).map_err(encode_failure)?.len();
    if size > ANSWER_LIMIT {
        return Err(ToolError::TooLarge { size, limit: ANSWER_LIMIT, narrow: "ask for fewer parts in include" });
    }

    Ok(answer)
}

fn encode_failure(source: serde_json::Error) -> ToolError {
    ToolError::Encode { source }
}
