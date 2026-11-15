//! Replayshark's analyses of one replay, read with the app's game data for the replay's build.

use std::sync::Arc;

use replayshark::LoadedBuild;
use replayshark::query::PlayerFilter;
use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use wows_mcp::tool_set::typed_tool;
use wows_replays::ReplayFile;
use wows_toolkit_replay_data::load::loaded_build_constants;
use wowsunpack::data::Version;

use crate::services::mcp::tools::Page;
use crate::services::mcp::tools::ReplayTools;
use crate::services::mcp::tools::blocking;
use crate::services::mcp::tools::error::ToolError;
use crate::services::mcp::tools::replays::ReplayRef;
use crate::services::mcp::tools::shown;

pub const ANALYSIS: &str = "replay_analysis";

/// An answer is bounded so one call cannot fill the model's context; a chat log is paged.
const ANSWER_LIMIT: usize = 1024 * 1024;

/// Arguments for `replay_analysis`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalysisArgs {
    pub replay: ReplayRef,
    pub analysis: AnalysisKind,

    /// `players` only: keeps players whose name contains this, ignoring case.
    #[serde(default)]
    pub player_name: Option<String>,

    /// `players` only: keeps players whose ship name or GameParams index contains this.
    #[serde(default)]
    pub ship: Option<String>,

    /// `chat_log` only: messages to skip, for the next page.
    #[serde(default)]
    pub offset: Option<u32>,

    /// `chat_log` only: messages to return, 1 to 1000. Default 200.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisKind {
    /// The recording player's game in brief: map, mode, ship, ribbons and damage dealt.
    Summary,
    /// Every chat message and voiceline with its game time and sender.
    ChatLog,
    /// Every player with their ship entity id, account id, ship and team.
    Players,
    /// The ships of each side counted by class, from the header alone.
    Roster,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AnalysisAnswer {
    pub replay_path: String,
    pub analysis: String,
    /// The analysis, shaped by its kind.
    pub result: Value,
    /// Payloads the parser left bytes of, which usually means the game data does not match the
    /// replay's build. Counted rather than listed.
    pub parser_diagnostics: usize,
    /// `chat_log`: the offset of the next page, when there are more messages.
    pub next_offset: Option<u32>,
}

pub fn analysis_tool() -> Tool {
    typed_tool::<AnalysisArgs, AnalysisAnswer>(
        ANALYSIS,
        "Analyze one replay's packets: summary (ribbons, damage), chat_log (paged), players (entity and account ids, ships, teams; filter by player_name or ship) or roster (ships per class). Needs the game data of the replay's build.",
    )
}

pub async fn analyze(tools: &ReplayTools, args: AnalysisArgs) -> Result<AnalysisAnswer, ToolError> {
    let page = Page::new(args.offset, args.limit, 200, 1000)?;
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
    let kind = args.analysis;
    let filter = PlayerFilter { name: args.player_name, entity: None, ship: args.ship };
    let (result, diagnostics, next_offset) = blocking(move || -> Result<(Value, usize, Option<u32>), ToolError> {
        let _permit = permit;
        let replay = ReplayFile::from_file(&read).map_err(|report| ToolError::ReplayRead {
            path: read.clone(),
            reason: report.current_context().to_string(),
        })?;
        let version = Version::try_from_client_exe(&replay.meta.clientVersionFromExe)
            .ok_or_else(|| ToolError::UnknownVersion { version: replay.meta.clientVersionFromExe.clone() })?;
        let build = version
            .build_number()
            .ok_or_else(|| ToolError::UnknownVersion { version: replay.meta.clientVersionFromExe.clone() })?;
        let loaded = game_data
            .get_or_load_build_for(build, Some(&version))
            .map_err(|source| ToolError::ReplayLoad { source })?;
        let constants = loaded_build_constants(&loaded, &game_data);
        let context = LoadedBuild::from_shared(Arc::clone(loaded.provider()), Arc::new(constants));

        match kind {
            AnalysisKind::Summary => {
                let parsed = replayshark::summary::summarize(&replay, &context).map_err(analysis_failure)?;
                Ok((encode(&parsed.value)?, parsed.diagnostics.len(), None))
            }
            AnalysisKind::ChatLog => {
                let parsed = replayshark::chat::chat_log(&replay, &context).map_err(analysis_failure)?;
                let diagnostics = parsed.diagnostics.len();
                let (events, next) = page.slice(parsed.value);
                Ok((encode(&events)?, diagnostics, next))
            }
            AnalysisKind::Players => {
                let players = replayshark::query::players(&replay, &context, &filter).map_err(analysis_failure)?;
                Ok((encode(&players)?, 0, None))
            }
            AnalysisKind::Roster => {
                let roster = replayshark::query::roster(&replay.meta, &context).map_err(analysis_failure)?;
                Ok((encode(&roster)?, 0, None))
            }
        }
    })
    .await??;

    let answer = AnalysisAnswer {
        replay_path: shown(&path),
        analysis: kind_name(kind).to_owned(),
        result,
        parser_diagnostics: diagnostics,
        next_offset,
    };
    let size = serde_json::to_vec(&answer).map_err(encode_failure)?.len();
    if size > ANSWER_LIMIT {
        return Err(ToolError::TooLarge { size, limit: ANSWER_LIMIT, narrow: "ask for a smaller page or filter" });
    }

    Ok(answer)
}

const fn kind_name(kind: AnalysisKind) -> &'static str {
    match kind {
        AnalysisKind::Summary => "summary",
        AnalysisKind::ChatLog => "chat_log",
        AnalysisKind::Players => "players",
        AnalysisKind::Roster => "roster",
    }
}

fn encode<T>(value: &T) -> Result<Value, ToolError>
where
    T: Serialize,
{
    serde_json::to_value(value).map_err(encode_failure)
}

fn analysis_failure(source: replayshark::AnalysisError) -> ToolError {
    ToolError::ReplayAnalysis { source }
}

fn encode_failure(source: serde_json::Error) -> ToolError {
    ToolError::Encode { source }
}
