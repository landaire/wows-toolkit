//! A replay's chat messages and voicelines.

use wows_replays::ReplayFile;
use wows_replays::analyzer::chat::ChatLogger;
use wows_replays::context::GameDataContext;

use crate::analysis::replay_version;
use crate::error::AnalysisError;
use crate::parse::Parsed;
use crate::parse::parse_into;

pub use wows_replays::analyzer::chat::ChatBody;
pub use wows_replays::analyzer::chat::ChatEvent;

/// Every chat message and voiceline in `replay`, in the order recorded.
pub fn chat_log(replay: &ReplayFile, ctx: &dyn GameDataContext) -> Result<Parsed<Vec<ChatEvent>>, AnalysisError> {
    let mut logger = ChatLogger::new(replay_version(replay)?);
    let diagnostics = parse_into(replay, ctx, &mut logger)?;

    Ok(Parsed { value: logger.into_events(), diagnostics })
}
