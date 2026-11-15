//! `battle-results`: writes the battle results of a batch of replays to JSON
//! files.

use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::*;
use serde_json::Value;
use wows_replay_insights::personal_rating::PersonalRatingData;
use wows_replays::ParseError;
use wows_replays::ReplayFile;
use wows_replays::context::GameDataContext;
use wowsunpack::data::Version;

use crate::battle_results::ConstantsError;
use crate::battle_results::ConstantsResolution;
use crate::battle_results::ConstantsResolver;
use crate::battle_results::ResultsFormat;
use crate::battle_results::battle_results;
use crate::error::BattleResultsError;
use crate::error::CommandError;
use crate::game_data::read_constants_file;

/// The command's flags.
pub struct BatchOptions {
    pub format: ResultsFormat,
    pub out_dir: Option<PathBuf>,
    pub out_file: Option<PathBuf>,
    pub allow_approximate_constants: bool,
    pub pr_expected_values: Option<PathBuf>,
    pub replays: Vec<PathBuf>,
}

/// Why one replay of the batch produced no results.
#[derive(Debug, thiserror::Error)]
enum ReplayFailure {
    #[error("failed to read replay: {0:?}")]
    Read(Report<ParseError>),

    #[error("replay carries an unparsable client version")]
    UnparsableVersion,

    #[error("replay carries no build number")]
    NoBuildNumber,

    #[error("constants for build {build} could not be resolved; nearest available is {resolved_build}")]
    ApproximateConstants { build: u32, resolved_build: u32 },

    #[error("no constants published upstream for build {requested} or any older build")]
    UnresolvedConstants { requested: u32 },

    #[error("constants fetch failed: {message}")]
    ConstantsFetch { message: String },

    #[error(transparent)]
    Results(#[from] BattleResultsError),

    #[error("panicked while processing (unsupported or corrupt replay)")]
    Panicked,
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

/// Runs `battle-results`. Fails if any replay in the batch failed, after
/// writing every successful result.
pub fn run(
    ctx: &dyn GameDataContext,
    has_game_data: bool,
    constants_path: Option<&Path>,
    options: BatchOptions,
) -> Result<(), Report<CommandError>> {
    let BatchOptions { format, out_dir, out_file, allow_approximate_constants, pr_expected_values, replays } = options;

    if out_dir.is_none() && out_file.is_none() {
        return Err(report!(CommandError::MissingOutput));
    }

    if !has_game_data {
        return Err(report!(CommandError::MissingGameData));
    }

    let explicit_constants = match constants_path {
        Some(path) => {
            Some(read_constants_file(path).context(CommandError::ReadConstants { path: path.to_path_buf() })?)
        }
        None => None,
    };

    let pr_data = match &pr_expected_values {
        Some(path) => Some(read_pr_data(path).context(CommandError::ReadPersonalRating { path: path.clone() })?),
        None => None,
    };

    let needs_remote_constants = format.needs_constants() && explicit_constants.is_none();
    let mut resolver: Option<ConstantsResolver> = None;

    let mut results: Vec<(String, Result<Value, ReplayFailure>)> = Vec::new();

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
            resolver = Some(ConstantsResolver::new().context(CommandError::ConstantsResolver)?);
        }

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let replay = ReplayFile::from_file(path).map_err(ReplayFailure::Read)?;
            let constants = if format.needs_constants() {
                Some(resolve_constants(
                    &stem,
                    &replay,
                    explicit_constants.as_ref(),
                    resolver.as_mut(),
                    allow_approximate_constants,
                )?)
            } else {
                None
            };

            Ok(battle_results(&replay, ctx, format, constants.as_ref(), pr_data.as_ref())?)
        }));

        results.push((stem, outcome.unwrap_or(Err(ReplayFailure::Panicked))));
    }

    let failed = results.iter().filter(|(_, result)| result.is_err()).count();
    for (stem, result) in &results {
        if let Err(e) = result {
            eprintln!("{stem}: {e}");
        }
    }

    if let Some(dir) = out_dir {
        std::fs::create_dir_all(&dir).context(CommandError::Write { path: dir.clone() })?;
        for (stem, value) in results.iter().filter_map(|(stem, result)| Some((stem, result.as_ref().ok()?))) {
            let out_path = dir.join(format!("{stem}.json"));
            write_json(&out_path, value)?;
        }
    } else if let Some(file) = out_file {
        let oks: Vec<Value> = results.into_iter().filter_map(|(_, result)| result.ok()).collect();
        match assemble_out_file(replays.len(), oks) {
            Some(out_value) => write_json(&file, &out_value)?,
            None => {
                eprintln!("no replay produced output; not writing {}", file.display());
            }
        }
    }

    if failed > 0 {
        return Err(report!(CommandError::ReplaysFailed { failed, total: replays.len() }));
    }

    Ok(())
}

fn read_pr_data(path: &Path) -> rootcause::Result<PersonalRatingData> {
    let bytes = std::fs::read(path)?;
    let mut data = PersonalRatingData::new();
    data.load_from_bytes(&bytes)?;

    Ok(data)
}

fn write_json(path: &Path, value: &Value) -> Result<(), Report<CommandError>> {
    let text = serde_json::to_string_pretty(value).context(CommandError::Write { path: path.to_path_buf() })?;

    std::fs::write(path, text).context(CommandError::Write { path: path.to_path_buf() })
}

/// Decide the `--out-file` layout from the number of INPUT replays (not the
/// number of successful results): a single bare object when exactly one
/// replay was given, otherwise a JSON array of whatever succeeded (possibly
/// empty). Returns `None` when nothing should be written: the single-replay
/// case failed and there is no value to write.
fn assemble_out_file(input_count: usize, oks: Vec<Value>) -> Option<Value> {
    if input_count == 1 { oks.into_iter().next() } else { Some(Value::Array(oks)) }
}

/// The wows-constants JSON to use for a replay: the explicit `-c` file if
/// given, else the shared resolver. `resolver` is `None` only when
/// `explicit` is `Some` (the caller only creates it when needed).
fn resolve_constants(
    stem: &str,
    replay: &ReplayFile,
    explicit: Option<&Value>,
    resolver: Option<&mut ConstantsResolver>,
    allow_approximate: bool,
) -> Result<Value, ReplayFailure> {
    if let Some(v) = explicit {
        return Ok(v.clone());
    }

    let version =
        Version::try_from_client_exe(&replay.meta.clientVersionFromExe).ok_or(ReplayFailure::UnparsableVersion)?;
    let resolver = resolver.expect("constants resolver must be initialized before resolving without -c");
    let build = version.build_number().ok_or(ReplayFailure::NoBuildNumber)?;
    let friendly = version.to_path();

    match resolver.resolve(build, Some(&friendly)) {
        Ok(ConstantsResolution::Exact(data)) => Ok(data.clone()),
        Ok(ConstantsResolution::Approximate { data, resolved_build }) => {
            if !allow_approximate {
                return Err(ReplayFailure::ApproximateConstants { build, resolved_build: *resolved_build });
            }

            eprintln!(
                "{stem}: constants for build {build} unavailable; using nearest available build {resolved_build}"
            );
            Ok(data.clone())
        }
        Err(ConstantsError::Unresolved { requested }) => Err(ReplayFailure::UnresolvedConstants { requested }),
        Err(ConstantsError::Init(message)) => Err(ReplayFailure::ConstantsFetch { message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            write_json(&dir.join(format!("{stem}.json")), value).unwrap();
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
