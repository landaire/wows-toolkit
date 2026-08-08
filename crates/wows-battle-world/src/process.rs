//! Context-driven replay processing.
//!
//! [`battle_report_for`] is the whole-replay entry point for batch tools and
//! services: resolve the build's game data through the caller's
//! [`GameDataContext`], parse the packet stream, and assemble a
//! [`BattleReport`]. Callers with bespoke pipelines keep driving
//! [`BattleWorld`] and `Parser` directly with borrowed data; this function
//! adds nothing they need.

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::context::GameDataContext;
use wows_replays::context::GameDataContextError;
use wows_replays::packet2::Parser;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use crate::ids::ShotTracking;
use crate::report::BattleReport;
use crate::world::BattleWorld;

#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("replay carries an unparsable client version {version:?}")]
    UnparsableVersion { version: String },

    #[error(transparent)]
    GameData(#[from] GameDataContextError),
}

/// What [`battle_report_for`] records while processing. The default is the
/// lightweight batch configuration: no shot tracking and no per-match hit or
/// salvo history.
#[derive(Debug, Clone, Copy)]
pub struct ProcessOptions {
    pub shot_tracking: ShotTracking,
    pub record_hit_history: bool,
    pub record_salvo_history: bool,
}

impl Default for ProcessOptions {
    fn default() -> Self {
        Self { shot_tracking: ShotTracking::Untracked, record_hit_history: false, record_salvo_history: false }
    }
}

/// Process a fully loaded replay into a [`BattleReport`].
///
/// Packet parse errors end the stream early rather than failing the report:
/// a replay truncated mid-battle still yields everything up to the cut.
pub fn battle_report_for(
    replay: &ReplayFile,
    ctx: &dyn GameDataContext,
    options: ProcessOptions,
) -> Result<BattleReport, ProcessError> {
    let version = Version::try_from_client_exe(&replay.meta.clientVersionFromExe)
        .ok_or_else(|| ProcessError::UnparsableVersion { version: replay.meta.clientVersionFromExe.clone() })?;

    let provider = ctx.metadata_provider(&version)?;
    let constants = ctx.game_constants(&version)?;

    let mut world = BattleWorld::new(&replay.meta, &*provider, Some(&*constants));
    world.set_shot_tracking(options.shot_tracking);
    world.set_record_hit_history(options.record_hit_history);
    world.set_record_salvo_history(options.record_salvo_history);

    let mut parser = Parser::with_version(provider.entity_specs(), version);
    let mut remaining = replay.packet_data();
    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => world.process(&packet),
            Err(_) => break,
        }
    }
    world.finish();

    Ok(world.into_report())
}
