//! Parser validation over many replays.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use serde::Serialize;
use wows_replays::ReplayFile;
use wows_replays::analyzer::survey::SurveyBuilder;
use wows_replays::analyzer::survey::SurveyStats;
use wows_replays::context::GameDataContext;

use crate::analysis::replay_version;
use crate::error::AnalysisError;
use crate::parse::Parsed;
use crate::parse::parse_into;

/// What parsing one replay found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReplaySurvey {
    /// The header's local recording time, `dd.mm.yyyy hh:mm:ss`.
    pub date_time: String,
    pub total_packets: usize,
    pub invalid_packets: usize,

    /// The decoder's audit notes, such as unexpected method arguments.
    pub audits: Vec<String>,
}

/// Counts over a set of surveyed replays.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SurveyTotals {
    pub total: usize,
    pub successes: usize,
    pub successes_with_invalids: usize,
    pub parse_failures: usize,
    pub version_failures: usize,

    /// How often each unsupported version appeared.
    pub invalid_versions: BTreeMap<String, usize>,

    /// Replays with audit notes, keyed by file name.
    pub audits: BTreeMap<String, AuditedReplay>,
}

impl SurveyTotals {
    pub fn new() -> Self {
        Self {
            total: 0,
            successes: 0,
            successes_with_invalids: 0,
            parse_failures: 0,
            version_failures: 0,
            invalid_versions: BTreeMap::new(),
            audits: BTreeMap::new(),
        }
    }

    pub fn add_parsed(&mut self, file_name: &str, survey: &ReplaySurvey) {
        self.total += 1;
        self.successes += 1;
        if survey.invalid_packets > 0 {
            self.successes_with_invalids += 1;
        }
        if !survey.audits.is_empty() {
            let audited = AuditedReplay { date_time: survey.date_time.clone(), audits: survey.audits.clone() };
            self.audits.insert(file_name.to_owned(), audited);
        }
    }

    pub fn add_unsupported_version(&mut self, version: &str) {
        self.total += 1;
        self.version_failures += 1;
        *self.invalid_versions.entry(version.to_owned()).or_default() += 1;
    }

    pub fn add_parse_failure(&mut self) {
        self.total += 1;
        self.parse_failures += 1;
    }
}

impl Default for SurveyTotals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AuditedReplay {
    /// The header's local recording time, `dd.mm.yyyy hh:mm:ss`.
    pub date_time: String,
    pub audits: Vec<String>,
}

/// Parses every packet of `replay`, decoding each unless `skip_decode`.
pub fn survey_replay(
    replay: &ReplayFile,
    ctx: &dyn GameDataContext,
    skip_decode: bool,
) -> Result<Parsed<ReplaySurvey>, AnalysisError> {
    let version = replay_version(replay)?;
    let constants = ctx.game_constants(&version)?;

    let stats = Rc::new(RefCell::new(SurveyStats::new()));
    let mut survey = SurveyBuilder::new(Rc::clone(&stats), skip_decode).game_constants(&constants).build(&replay.meta);
    let diagnostics = parse_into(replay, ctx, &mut *survey)?;
    drop(survey);

    let stats = stats.take();
    let value = ReplaySurvey {
        date_time: stats.date_time,
        total_packets: stats.total_packets,
        invalid_packets: stats.invalid_packets,
        audits: stats.audits,
    };

    Ok(Parsed { value, diagnostics })
}
