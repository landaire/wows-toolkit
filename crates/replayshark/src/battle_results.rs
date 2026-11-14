//! A replay's battle results: raw, merged with the build's field names, or
//! normalized per player, with per-build wows-constants resolution.
//!
//! `ConstantsFetcher::fetch` never errors on a build mismatch: it silently
//! falls back to the nearest older published build. Callers must compare the
//! returned build against the requested one to detect an approximate match.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;
use wows_battle_world::process::ProcessOptions;
use wows_battle_world::process::battle_report_for;
use wows_battle_world::report::BattleReport;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replay_insights::battle_report::resolve_battle_results;
use wows_replay_insights::personal_rating::PersonalRatingData;
use wows_replay_insights::personal_rating::ShipBattleStats;
use wows_replays::ReplayFile;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::context::GameDataContext;
use wowsunpack::data::Version;

use crate::error::BattleResultsError;

pub enum ConstantsResolution {
    Exact(serde_json::Value),
    Approximate { data: serde_json::Value, resolved_build: u32 },
}

#[derive(thiserror::Error, Debug)]
pub enum ConstantsError {
    #[error("no constants published upstream for build {requested} or any older build")]
    Unresolved { requested: u32 },
    #[error("failed to initialize constants fetcher: {0}")]
    Init(String),
}

pub struct ConstantsResolver {
    fetcher: wows_data_mgr::constants::ConstantsFetcher,
    memo: HashMap<u32, ConstantsResolution>,
}

impl ConstantsResolver {
    pub fn new() -> Result<Self, ConstantsError> {
        let fetcher =
            wows_data_mgr::constants::ConstantsFetcher::new().map_err(|e| ConstantsError::Init(e.to_string()))?;
        Ok(Self { fetcher, memo: HashMap::new() })
    }

    /// Resolve constants for a replay build. Memoized by the requested build.
    pub fn resolve(
        &mut self,
        build: u32,
        friendly_version: Option<&str>,
    ) -> Result<&ConstantsResolution, ConstantsError> {
        if !self.memo.contains_key(&build) {
            let (data, actual) =
                self.fetcher.fetch(build, friendly_version).ok_or(ConstantsError::Unresolved { requested: build })?;
            self.memo.insert(build, classify(build, actual, data));
        }
        Ok(self.memo.get(&build).unwrap())
    }
}

// Variant doc comments would show in `battle-results --help`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ResultsFormat {
    // The server's results as the replay carries them.
    Raw,
    // The raw results with field names from the build's constants.
    Merged,
    // One record per player, built from the replay and the merged results.
    Normalized,
}

impl ResultsFormat {
    /// Whether this format reads field names from the build's constants.
    pub fn needs_constants(self) -> bool {
        matches!(self, Self::Merged | Self::Normalized)
    }
}

/// Pure classifier: exact match vs fallback to an older published build.
pub fn classify(requested: u32, actual: u32, data: serde_json::Value) -> ConstantsResolution {
    if actual == requested {
        ConstantsResolution::Exact(data)
    } else {
        ConstantsResolution::Approximate { data, resolved_build: actual }
    }
}

/// `replay`'s battle results in `format`. `constants` is the build's
/// wows-constants JSON, which merged and normalized results need. With
/// `pr_data`, normalized results carry each player's personal rating.
///
/// Every format parses the packet stream, even `raw`, which reads the
/// results string off the parsed `BattleReport`.
pub fn battle_results(
    replay: &ReplayFile,
    ctx: &dyn GameDataContext,
    format: ResultsFormat,
    constants: Option<&Value>,
    pr_data: Option<&PersonalRatingData>,
) -> Result<Value, BattleResultsError> {
    let version = Version::try_from_client_exe(&replay.meta.clientVersionFromExe)
        .ok_or_else(|| BattleResultsError::UnparsableVersion { version: replay.meta.clientVersionFromExe.clone() })?;
    let provider = ctx.metadata_provider(&version)?;
    let report = battle_report_for(replay, ctx, ProcessOptions::default())?;

    let constants = || constants.ok_or(BattleResultsError::MissingConstants { format });

    match format {
        ResultsFormat::Raw => raw_results(&report),
        ResultsFormat::Merged => Ok(resolve_battle_results(raw_results(&report)?, constants()?)),
        ResultsFormat::Normalized => {
            let mut normalized =
                NormalizedBattleReport::from_battle_report(&report, &replay.meta, &provider, constants()?);
            if let Some(pr_data) = pr_data {
                populate_personal_ratings(&mut normalized, &report, pr_data);
                normalized.populate_match_contribution(pr_data);
            }
            serde_json::to_value(&normalized).map_err(BattleResultsError::Serialize)
        }
    }
}

fn raw_results(report: &BattleReport) -> Result<Value, BattleResultsError> {
    let raw = report.battle_results().ok_or(BattleResultsError::NoBattleResults)?;

    serde_json::from_str(raw).map_err(BattleResultsError::RawResults)
}

/// Compute and fill in each player's `personal_rating` from a single battle's
/// worth of `ShipBattleStats`. Skipped for an unrated battle, and for players
/// with no server results (no `damage` key), matching the toolkit's
/// actual-damage gate. `report`'s players are positional with
/// `normalized.players` (both built from the same `report.players()`
/// iteration order).
fn populate_personal_ratings(
    normalized: &mut NormalizedBattleReport,
    report: &BattleReport,
    pr_data: &PersonalRatingData,
) {
    if !normalized.metadata.rating_scope().is_rated() {
        return;
    }

    let battle_result = normalized.metadata.battle_result;
    for (player, entity_player) in normalized.players.iter_mut().zip(report.players().iter()) {
        let Some(sr) = player.server_results.as_ref() else {
            continue;
        };
        let Some(damage) = sr.damage else {
            continue;
        };
        let frags = sr.kills.unwrap_or(0);
        let is_win = matches!(
            battle_result,
            Some(BattleResult::Win(team)) | Some(BattleResult::Loss(team)) if team == player.team_id as i8
        );

        let stats = ShipBattleStats {
            ship_id: entity_player.vehicle().id(),
            battles: 1,
            damage,
            wins: if is_win { 1 } else { 0 },
            frags,
        };

        player.personal_rating = pr_data.calculate_pr(&[stats]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_exact_vs_approximate() {
        let v = serde_json::json!({"COMMON_RESULTS": []});
        assert!(matches!(classify(100, 100, v.clone()), ConstantsResolution::Exact(_)));
        match classify(100, 90, v) {
            ConstantsResolution::Approximate { resolved_build, .. } => assert_eq!(resolved_build, 90),
            _ => panic!("expected approximate"),
        }
    }
}
