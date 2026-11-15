//! The replay index: searching matches, and one match's roster.

use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::game_types::ArenaId;
use wows_mcp::tool_set::typed_tool;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortDirection;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::query_sql::CompileCtx;
use wows_toolkit_config::index::query_text::parse_query;
use wows_toolkit_config::index::rows::IndexedVehicleRow;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::VehicleRelation;

use crate::services::mcp::tools::Page;
use crate::services::mcp::tools::ReplayTools;
use crate::services::mcp::tools::error::ToolError;
use crate::services::mcp::tools::shown;

pub const SEARCH: &str = "replay_index_search";
pub const PLAYERS: &str = "replay_index_players";

/// Arguments for `replay_index_search`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    /// A query in the Search tab's syntax, such as `map:Ocean and outcome:win`,
    /// `self.damage>=100000`, `count(relation:enemy and class:dd)>=3`, or bare words matched against maps, player
    /// names, clans and ships. Empty or absent matches every indexed replay.
    #[serde(default)]
    pub query: Option<String>,

    /// The column to sort by. Newest first when absent.
    #[serde(default)]
    pub sort: Option<SortField>,

    /// Sort ascending rather than descending.
    #[serde(default)]
    pub ascending: bool,

    /// Matches to skip, for the next page.
    #[serde(default)]
    pub offset: Option<u32>,

    /// Matches to return, 1 to 100. Default 25.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    Date,
    Map,
    Mode,
    Outcome,
    Damage,
    Kills,
    Pr,
}

impl SortField {
    fn column(self) -> SortColumn {
        match self {
            Self::Date => SortColumn::Date,
            Self::Map => SortColumn::Map,
            Self::Mode => SortColumn::Mode,
            Self::Outcome => SortColumn::Outcome,
            Self::Damage => SortColumn::Damage,
            Self::Kills => SortColumn::Kills,
            Self::Pr => SortColumn::Pr,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchAnswer {
    pub matches: Vec<IndexedMatch>,
    /// The offset of the next page, when there are more matches.
    pub next_offset: Option<u32>,
}

/// One indexed match, as the recording player saw it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexedMatch {
    pub arena_id: i64,
    /// When the match was played, RFC 3339.
    pub played_at: String,
    pub map: String,
    pub game_mode: String,
    pub game_type: String,
    pub match_group: String,
    /// The game build the replay was recorded on.
    pub build: Option<u32>,
    pub outcome: Outcome,
    pub ship: Option<String>,
    pub survived: Option<bool>,
    pub damage: Option<u64>,
    pub kills: Option<i64>,
    /// Personal rating of the recording player's battle.
    pub personal_rating: Option<f64>,
    /// Whether the server's battle results are in the replay.
    pub results_available: bool,
    pub replay_path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Win,
    Loss,
    Draw,
    Unknown,
}

impl From<MatchOutcome> for Outcome {
    fn from(outcome: MatchOutcome) -> Self {
        match outcome {
            MatchOutcome::Win => Self::Win,
            MatchOutcome::Loss => Self::Loss,
            MatchOutcome::Draw => Self::Draw,
            MatchOutcome::Unknown => Self::Unknown,
        }
    }
}

impl From<MatchHit> for IndexedMatch {
    fn from(hit: MatchHit) -> Self {
        Self {
            arena_id: hit.arena_id.raw(),
            played_at: hit.timestamp.to_string(),
            map: hit.map,
            game_mode: hit.game_mode,
            game_type: hit.game_type,
            match_group: hit.match_group,
            build: hit.version_build,
            outcome: hit.outcome.into(),
            ship: hit.self_ship_name,
            survived: hit.self_survived,
            damage: hit.self_damage,
            kills: hit.self_kills,
            personal_rating: hit.self_pr,
            results_available: hit.results_available,
            replay_path: shown(&hit.replay_path),
        }
    }
}

/// Arguments for `replay_index_players`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayersArgs {
    /// The match, as `replay_index_search` gives it.
    pub arena_id: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PlayersAnswer {
    pub arena_id: i64,
    pub players: Vec<IndexedPlayer>,
}

/// One player of an indexed match. Results are absent when the replay holds none.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexedPlayer {
    pub account_id: i64,
    pub name: String,
    pub clan: String,
    pub realm: Option<String>,
    pub relation: Relation,
    pub division_id: Option<i64>,
    pub ship_id: u64,
    pub ship_index: String,
    pub ship: String,
    pub nation: String,
    pub class: String,
    pub tier: u32,
    pub test_ship: bool,
    pub survived: Option<bool>,
    pub damage: Option<u64>,
    pub kills: Option<i64>,
    pub spotting_damage: Option<u64>,
    pub potential_damage: Option<u64>,
    pub damage_received: Option<u64>,
    pub personal_rating: Option<f64>,
    pub disconnected: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// The player who recorded the replay.
    Recorder,
    Ally,
    Enemy,
}

impl From<IndexedVehicleRow> for IndexedPlayer {
    fn from(row: IndexedVehicleRow) -> Self {
        Self {
            account_id: row.account_id.raw(),
            name: row.player_name,
            clan: row.clan,
            realm: row.realm,
            relation: match row.relation {
                VehicleRelation::SelfPlayer => Relation::Recorder,
                VehicleRelation::Ally => Relation::Ally,
                VehicleRelation::Enemy => Relation::Enemy,
            },
            division_id: row.division_id,
            ship_id: row.ship_id.raw(),
            ship_index: row.ship_index,
            ship: row.ship_name,
            nation: row.nation,
            class: row.species,
            tier: row.tier,
            test_ship: row.is_test_ship,
            survived: row.survived,
            damage: row.damage,
            kills: row.kills,
            spotting_damage: row.spotting,
            potential_damage: row.potential,
            damage_received: row.received,
            personal_rating: row.pr,
            disconnected: row.disconnected,
        }
    }
}

pub fn search_tool() -> Tool {
    typed_tool::<SearchArgs, SearchAnswer>(
        SEARCH,
        "Search the replays WoWs Toolkit has indexed, newest first, with the Search tab's query syntax. Gives each match's arena_id, map, mode, outcome and the recording player's ship and results. Paginated.",
    )
}

pub fn players_tool() -> Tool {
    typed_tool::<PlayersArgs, PlayersAnswer>(
        PLAYERS,
        "Every player of one indexed match: name, clan, side, division, ship, and their damage, kills, spotting, potential damage, damage received and personal rating when the replay holds results.",
    )
}

/// The parser recurses once per nesting level on the server's stack, so a query is bounded before
/// it is parsed.
fn check_query_bounds(text: &str) -> Result<(), ToolError> {
    const MAX_LENGTH: usize = 4096;
    const MAX_DEPTH: usize = 64;

    if text.len() > MAX_LENGTH {
        return Err(ToolError::InvalidArgument { message: format!("the query must be at most {MAX_LENGTH} bytes") });
    }

    let mut depth = 0usize;
    for character in text.chars() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => continue,
        }
        if depth > MAX_DEPTH {
            return Err(ToolError::InvalidArgument { message: format!("the query nests deeper than {MAX_DEPTH}") });
        }
    }

    Ok(())
}

pub async fn search(tools: &ReplayTools, args: SearchArgs) -> Result<SearchAnswer, ToolError> {
    // Deep pages read every row before them; this keeps one call's read bounded.
    const MAX_OFFSET: u32 = 10_000;

    let page = Page::new(args.offset, args.limit, 25, 100)?;
    if page.offset > MAX_OFFSET as usize {
        return Err(ToolError::InvalidArgument { message: format!("offset must be at most {MAX_OFFSET}") });
    }
    let text = args.query.as_deref().unwrap_or("");
    check_query_bounds(text)?;
    let expression = parse_query(text).map_err(|error| ToolError::Query {
        start: error.span.start,
        end: error.span.end,
        kind: error.kind,
    })?;
    let sort = match args.sort {
        Some(field) => SortSpec {
            column: field.column(),
            direction: if args.ascending { SortDirection::Ascending } else { SortDirection::Descending },
        },
        None => SortSpec::default(),
    };

    let pool = tools.index()?;
    // Fetches one row past the page, which says whether another page follows.
    let wanted = (page.offset + page.limit) as i64;
    let hits = query::search_by_ast(&pool, &expression, &CompileCtx::default(), wanted, sort)
        .await
        .map_err(|source| ToolError::Index { source })?;
    let (hits, next_offset) = page.slice(hits);

    Ok(SearchAnswer { matches: hits.into_iter().map(IndexedMatch::from).collect(), next_offset })
}

pub async fn players(tools: &ReplayTools, args: PlayersArgs) -> Result<PlayersAnswer, ToolError> {
    let pool = tools.index()?;
    let rows = query::roster(&pool, ArenaId::new(args.arena_id)).await.map_err(|source| ToolError::Index { source })?;
    if rows.is_empty() {
        return Err(ToolError::UnknownArena { arena_id: args.arena_id });
    }

    Ok(PlayersAnswer { arena_id: args.arena_id, players: rows.into_iter().map(IndexedPlayer::from).collect() })
}
