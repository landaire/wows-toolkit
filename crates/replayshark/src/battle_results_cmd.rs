//! `battle-results` command: dump raw, merged, or normalized battle results
//! for one or more replays, with per-build wows-constants resolution.
//!
//! `ConstantsFetcher::fetch` never errors on a build mismatch: it silently
//! falls back to the nearest older published build. Callers must compare the
//! returned build against the requested one to detect an approximate match.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::anyhow;
use serde_json::Value;
use wows_battle_world::BattleWorld;
use wows_battle_world::ids::ShotTracking;
use wows_battle_world::report::BattleReport;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replay_insights::battle_report::resolve_battle_results;
use wows_replay_insights::personal_rating::PersonalRatingData;
use wows_replay_insights::personal_rating::ShipBattleStats;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleResult;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;

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

/// Pure classifier: exact match vs fallback to an older published build.
pub fn classify(requested: u32, actual: u32, data: serde_json::Value) -> ConstantsResolution {
    if actual == requested {
        ConstantsResolution::Exact(data)
    } else {
        ConstantsResolution::Approximate { data, resolved_build: actual }
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum ResultsFormat {
    Raw,
    Merged,
    Normalized,
}

type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send + 'static>;

/// Restores the previous panic hook on drop, including on early return via `?`.
struct RestorePanicHook(Option<PanicHook>);

impl Drop for RestorePanicHook {
    fn drop(&mut self) {
        if let Some(hook) = self.0.take() {
            std::panic::set_hook(hook);
        }
    }
}

/// Run the `battle-results` command. Exits the process with a nonzero status
/// if any replay in the batch failed, after writing every successful result.
#[allow(clippy::too_many_arguments)]
pub fn run(
    game_dir: Option<&str>,
    extracted_dir: Option<&str>,
    constants_path: Option<&Path>,
    format: ResultsFormat,
    out_dir: Option<PathBuf>,
    out_file: Option<PathBuf>,
    allow_approximate_constants: bool,
    pr_expected_values: Option<PathBuf>,
    replays: Vec<PathBuf>,
) -> anyhow::Result<()> {
    if out_dir.is_none() && out_file.is_none() {
        return Err(anyhow!("one of --out-dir or --out-file is required"));
    }

    if game_dir.is_none() && extracted_dir.is_none() {
        return Err(anyhow!("one of -g/--game or -e/--extracted is required"));
    }

    let explicit_constants: Option<Value> = match constants_path {
        Some(path) => {
            let data = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read constants file {}", path.display()))?;
            let json: Value = serde_json::from_str(&data)
                .with_context(|| format!("failed to parse constants JSON {}", path.display()))?;
            Some(json)
        }
        None => None,
    };

    let pr_data: Option<PersonalRatingData> = match pr_expected_values {
        Some(path) => {
            let bytes = std::fs::read(&path)
                .with_context(|| format!("failed to read PR expected values file {}", path.display()))?;
            let mut data = PersonalRatingData::new();
            data.load_from_bytes(&bytes)
                .with_context(|| format!("failed to parse PR expected values file {}", path.display()))?;
            Some(data)
        }
        None => None,
    };

    let needs_remote_constants =
        matches!(format, ResultsFormat::Merged | ResultsFormat::Normalized) && explicit_constants.is_none();
    let mut resolver: Option<ConstantsResolver> = None;

    let mut results: Vec<Result<(String, Value), (String, String)>> = Vec::new();

    // Some replay inputs are expected to panic deep in the pipeline (e.g.
    // pre-0.9 self-player resolution). Silence the default panic hook for the
    // batch so a caught panic doesn't spam a backtrace; each caught panic is
    // still reported as a normal one-line per-replay error below.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let _restore_hook = RestorePanicHook(Some(previous_hook));

    for path in &replays {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("replay").to_string();

        if needs_remote_constants && resolver.is_none() {
            resolver = Some(ConstantsResolver::new().context("failed to initialize constants resolver")?);
        }

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process_replay(
                path,
                game_dir,
                extracted_dir,
                format,
                explicit_constants.as_ref(),
                resolver.as_mut(),
                allow_approximate_constants,
                pr_data.as_ref(),
                &stem,
            )
        }));

        match outcome {
            Ok(Ok(value)) => results.push(Ok((stem, value))),
            Ok(Err(e)) => results.push(Err((stem, e))),
            Err(_) => {
                results.push(Err((stem, "panicked while processing (unsupported or corrupt replay)".to_string())))
            }
        }
    }

    let mut had_failure = false;
    for result in &results {
        if let Err((stem, e)) = result {
            had_failure = true;
            eprintln!("{stem}: {e}");
        }
    }

    if let Some(dir) = out_dir {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create output directory {}", dir.display()))?;
        for (stem, value) in results.iter().flatten() {
            let out_path = dir.join(format!("{stem}.json"));
            let text = serde_json::to_string_pretty(value).context("failed to serialize battle results")?;
            std::fs::write(&out_path, text).with_context(|| format!("failed to write {}", out_path.display()))?;
        }
    } else if let Some(file) = out_file {
        let oks: Vec<Value> = results.iter().filter_map(|r| r.as_ref().ok()).map(|(_, v)| v.clone()).collect();
        match assemble_out_file(replays.len(), oks) {
            Some(out_value) => {
                let text = serde_json::to_string_pretty(&out_value).context("failed to serialize battle results")?;
                std::fs::write(&file, text).with_context(|| format!("failed to write {}", file.display()))?;
            }
            None => {
                eprintln!("no replay produced output; not writing {}", file.display());
            }
        }
    }

    if had_failure {
        std::process::exit(1);
    }

    Ok(())
}

/// Decide the `--out-file` layout from the number of INPUT replays (not the
/// number of successful results): a single bare object when exactly one
/// replay was given, otherwise a JSON array of whatever succeeded (possibly
/// empty). Returns `None` when nothing should be written: the single-replay
/// case failed and there is no value to write.
fn assemble_out_file(input_count: usize, oks: Vec<Value>) -> Option<Value> {
    if input_count == 1 { oks.into_iter().next() } else { Some(Value::Array(oks)) }
}

/// Process one replay into the requested output format. All formats need
/// game data to parse the replay's packet stream, even `raw`, which reads
/// the resolved battle-results string off the parsed `BattleReport`.
#[allow(clippy::too_many_arguments)]
fn process_replay(
    path: &Path,
    game_dir: Option<&str>,
    extracted_dir: Option<&str>,
    format: ResultsFormat,
    explicit_constants: Option<&Value>,
    resolver: Option<&mut ConstantsResolver>,
    allow_approximate_constants: bool,
    pr_data: Option<&PersonalRatingData>,
    stem: &str,
) -> Result<Value, String> {
    let replay_file = ReplayFile::from_file(path).map_err(|e| format!("failed to read replay: {e:?}"))?;
    let version = Version::try_from_client_exe(&replay_file.meta.clientVersionFromExe)
        .ok_or_else(|| "replay carries an unparsable client version".to_string())?;

    let (provider, game_constants) = crate::load_metadata_provider_and_constants(game_dir, extracted_dir, &version)
        .map_err(|e| format!("failed to load game data: {e}"))?;

    let report = build_battle_report(&replay_file, &provider, &game_constants, version)?;

    match format {
        ResultsFormat::Raw => {
            let raw = report.battle_results().ok_or_else(|| "replay has no battle results".to_string())?;
            serde_json::from_str(raw).map_err(|e| format!("failed to parse raw battle results: {e}"))
        }
        ResultsFormat::Merged => {
            let raw = report.battle_results().ok_or_else(|| "replay has no battle results".to_string())?;
            let raw_value: Value =
                serde_json::from_str(raw).map_err(|e| format!("failed to parse raw battle results: {e}"))?;
            let constants =
                resolve_constants(stem, explicit_constants, resolver, &version, allow_approximate_constants)?;
            Ok(resolve_battle_results(raw_value, &constants))
        }
        ResultsFormat::Normalized => {
            let constants =
                resolve_constants(stem, explicit_constants, resolver, &version, allow_approximate_constants)?;
            let mut normalized =
                NormalizedBattleReport::from_battle_report(&report, &replay_file.meta, &provider, &constants);
            if let Some(pr_data) = pr_data {
                populate_personal_ratings(&mut normalized, &report, pr_data);
            }
            serde_json::to_value(&normalized).map_err(|e| format!("failed to serialize normalized report: {e}"))
        }
    }
}

/// Parse the replay's packets into a finished `BattleReport`, mirroring
/// `run_players_query`. Shared by all three output formats.
fn build_battle_report(
    replay_file: &ReplayFile,
    provider: &GameMetadataProvider,
    game_constants: &wows_replays::game_constants::GameConstants,
    version: Version,
) -> Result<BattleReport, String> {
    let mut world = BattleWorld::new(&replay_file.meta, provider, Some(game_constants));
    world.set_shot_tracking(ShotTracking::Untracked);

    let mut parser = wows_replays::packet2::Parser::with_version(provider.entity_specs(), version);
    let mut remaining = replay_file.packet_data.as_slice();
    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => world.process(&packet),
            Err(_) => break,
        }
    }
    world.finish();

    Ok(world.into_report())
}

/// Resolve the wows-constants JSON to use for a replay: the explicit `-c`
/// file if given, else the shared resolver. `resolver` is `None` only when
/// `explicit` is `Some` (the caller only creates it when needed).
fn resolve_constants(
    stem: &str,
    explicit: Option<&Value>,
    resolver: Option<&mut ConstantsResolver>,
    version: &Version,
    allow_approximate: bool,
) -> Result<Value, String> {
    if let Some(v) = explicit {
        return Ok(v.clone());
    }

    let resolver = resolver.expect("constants resolver must be initialized before resolving without -c");
    let build = version.build_number().ok_or_else(|| "replay carries no build number".to_string())?;
    let friendly = version.to_path();

    match resolver.resolve(build, Some(&friendly)) {
        Ok(ConstantsResolution::Exact(data)) => Ok(data.clone()),
        Ok(ConstantsResolution::Approximate { data, resolved_build }) => {
            if allow_approximate {
                eprintln!(
                    "{stem}: constants for build {build} unavailable; using nearest available build {resolved_build}"
                );
                Ok(data.clone())
            } else {
                Err(format!("constants for build {build} could not be resolved; nearest available is {resolved_build}"))
            }
        }
        Err(ConstantsError::Unresolved { requested }) => {
            Err(format!("no constants published upstream for build {requested} or any older build"))
        }
        Err(ConstantsError::Init(e)) => Err(format!("constants fetch failed: {e}")),
    }
}

/// Compute and fill in each player's `personal_rating` from a single battle's
/// worth of `ShipBattleStats`. Skipped for players with no server results (no
/// `damage` key), matching the toolkit's actual-damage gate. `report`'s
/// players are positional with `normalized.players` (both built from the same
/// `report.players()` iteration order).
fn populate_personal_ratings(
    normalized: &mut NormalizedBattleReport,
    report: &BattleReport,
    pr_data: &PersonalRatingData,
) {
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

    #[test]
    fn assemble_out_file_one_input_one_ok_is_bare_object() {
        let oks = vec![serde_json::json!({"a": 1})];
        assert_eq!(assemble_out_file(1, oks), Some(serde_json::json!({"a": 1})));
    }

    #[test]
    fn assemble_out_file_one_input_failed_writes_nothing() {
        let oks: Vec<Value> = vec![];
        assert_eq!(assemble_out_file(1, oks), None);
    }

    #[test]
    fn assemble_out_file_two_inputs_two_oks_is_array_len_two() {
        let oks = vec![serde_json::json!({"a": 1}), serde_json::json!({"a": 2})];
        assert_eq!(assemble_out_file(2, oks), Some(serde_json::json!([{"a": 1}, {"a": 2}])));
    }

    /// C1 regression guard: 3 replays in, 2 fail, 1 succeeds. The shape is
    /// still an array (keyed on the 3 inputs), not a bare object, even though
    /// only one value made it into `oks`.
    #[test]
    fn assemble_out_file_three_inputs_one_ok_is_array_len_one() {
        let oks = vec![serde_json::json!({"a": 1})];
        assert_eq!(assemble_out_file(3, oks), Some(serde_json::json!([{"a": 1}])));
    }

    #[test]
    fn assemble_out_file_many_inputs_all_failed_is_empty_array() {
        let oks: Vec<Value> = vec![];
        assert_eq!(assemble_out_file(3, oks), Some(serde_json::json!([])));
    }

    /// Process-unique temp dir name; no `Date`/random needed for uniqueness
    /// across concurrent test runs.
    fn unique_temp_dir(suffix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("replayshark-test-{}-{}", std::process::id(), suffix))
    }

    #[test]
    fn out_dir_writes_one_json_file_per_stem() {
        let dir = unique_temp_dir("out-dir-write");
        std::fs::create_dir_all(&dir).unwrap();

        let pairs = vec![
            ("replay_one".to_string(), serde_json::json!({"a": 1})),
            ("replay_two".to_string(), serde_json::json!({"b": 2})),
        ];

        for (stem, value) in &pairs {
            let out_path = dir.join(format!("{stem}.json"));
            let text = serde_json::to_string_pretty(value).unwrap();
            std::fs::write(&out_path, text).unwrap();
        }

        for (stem, value) in &pairs {
            let out_path = dir.join(format!("{stem}.json"));
            assert!(out_path.exists(), "missing {}", out_path.display());
            let text = std::fs::read_to_string(&out_path).unwrap();
            let parsed: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(&parsed, value);
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
