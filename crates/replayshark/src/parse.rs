//! Runs a packet analyzer over a whole replay.

use serde::Serialize;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::context::GameDataContext;
use wows_replays::packet2::PayloadDiagnostic;

use crate::analysis::replay_version;
use crate::error::AnalysisError;

/// An analysis of a replay's packets with what the parser noticed on the way.
#[derive(Debug, Clone, Serialize)]
pub struct Parsed<T> {
    pub value: T,

    /// Payloads the parser did not fully consume, which usually means the
    /// entity definitions do not match the replay's build.
    pub diagnostics: Vec<Diagnostic>,
}

/// A method or property payload the parser left bytes of.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    /// Where the payload came from, such as `EntityMethod::Avatar::onFoo`.
    pub context: String,
    pub semantic_name: Option<String>,
    pub payload_len: usize,
    pub consumed: usize,
}

impl From<PayloadDiagnostic> for Diagnostic {
    fn from(diagnostic: PayloadDiagnostic) -> Self {
        let PayloadDiagnostic { context, semantic_name, payload_len, consumed } = diagnostic;

        Self { context, semantic_name, payload_len, consumed }
    }
}

/// Feeds every packet of `replay` to `analyzer`, then finishes it.
pub(crate) fn parse_into(
    replay: &ReplayFile,
    ctx: &dyn GameDataContext,
    analyzer: &mut dyn Analyzer,
) -> Result<Vec<Diagnostic>, AnalysisError> {
    let version = replay_version(replay)?;
    let specs = ctx.entity_specs(&version)?;

    let mut parser = wows_replays::packet2::Parser::with_version(&specs, version);
    let mut remaining = replay.packet_data();
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).map_err(wows_replays::ParseError::from)?;
        analyzer.process(&packet);
    }
    let diagnostics = parser.drain_diagnostics().into_iter().map(Diagnostic::from).collect();
    analyzer.finish();

    Ok(diagnostics)
}
