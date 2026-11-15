//! The recording player's game in brief.

use serde::Serialize;
use wows_replays::ReplayFile;
use wows_replays::analyzer::summary::Summary;
use wows_replays::context::GameDataContext;

use crate::error::AnalysisError;
use crate::parse::Parsed;
use crate::parse::parse_into;

pub use wows_replays::analyzer::summary::Ribbon;
pub use wows_replays::analyzer::summary::SummaryStats;

/// The replay header's facts with the recording player's ribbons and damage.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReplaySummary {
    pub player_name: String,

    /// The header's local recording time, `dd.mm.yyyy hh:mm:ss`.
    pub date_time: String,
    pub map: String,

    /// The recording player's ship, such as `PASB110-Vermont`.
    pub vehicle: String,

    /// The header's match name, such as `12x12`.
    pub game_mode: String,
    pub game_logic: Option<String>,

    /// The header's client version, `major,minor,patch,build`.
    pub client_version: String,
    pub stats: SummaryStats,
}

pub fn summarize(replay: &ReplayFile, ctx: &dyn GameDataContext) -> Result<Parsed<ReplaySummary>, AnalysisError> {
    let mut summary = Summary::new();
    let diagnostics = parse_into(replay, ctx, &mut summary)?;

    let meta = &replay.meta;
    let value = ReplaySummary {
        player_name: meta.playerName.clone(),
        date_time: meta.dateTime.clone(),
        map: meta.mapDisplayName.clone(),
        vehicle: meta.playerVehicle.clone(),
        game_mode: meta.name.clone(),
        game_logic: meta.gameLogic.clone(),
        client_version: meta.clientVersionFromExe.clone(),
        stats: summary.stats(),
    };

    Ok(Parsed { value, diagnostics })
}
