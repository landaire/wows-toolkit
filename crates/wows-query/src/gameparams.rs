//! `gameparams_query`: jq filters over raw GameParams entries, for values the
//! normalized queries do not report.

use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::Instant;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_data_mgr::Dump;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::types::GameParamProvider;

use crate::GameData;
use crate::data::BuildNumber;
use crate::data::GAME_PARAMS_DATA;
use crate::error::QueryError;
use crate::error::RawParamsError;
use crate::jq::Limits;
use crate::jq::Stop;
use crate::jq::floor_char_boundary;
use crate::jq_worker;
use crate::names::Interpretations;
use crate::raw_params::RawParamEntry;
use crate::raw_params::RawParams;
use crate::ships::Tier;

/// Arguments for `gameparams_query`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GameParamsQueryArgs {
    /// Which GameParams entries to read.
    pub select: ParamSelection,

    /// jq program (jaq dialect) run once per selected entry, with the entry
    /// object as input. Start with `keys` to list fields, then narrow, e.g.
    /// `.A_Hull | keys` or `.A1_Artillery | {maxDist, sigmaCount}`.
    pub filter: String,

    /// Most entries a set selection reads, 1 to 200, default 20.
    #[schemars(range(min = 1, max = 200))]
    pub limit: Option<u32>,

    /// Output budget in bytes, 500 to 24000, default 6000.
    #[schemars(range(min = 500, max = 24000))]
    pub max_bytes: Option<u32>,
}

/// One entry, or a set of entries of one type.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ParamSelection {
    /// One entry by param name (PASC020_Des_Moines_1948), index (PASC020)
    /// or numeric id.
    Param(String),
    /// One ship by English name, as the ship tools resolve it.
    Ship(String),
    /// Entries of one `typeinfo.type`, in GameParams order.
    Set(ParamSet),
}

/// Entries matching every given field. Text matches without regard to case.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamSet {
    /// `typeinfo.type`, e.g. Ship, Projectile, Gun, Modernization, Ability,
    /// Crew, Unit, Exterior.
    #[serde(rename = "type")]
    pub kind: String,
    /// `typeinfo.nation`, e.g. USA, Japan, Germany, Russia.
    pub nation: Option<String>,
    /// `typeinfo.species`, e.g. Cruiser, Destroyer, Artillery.
    pub species: Option<String>,
    /// The entry's `level` field: tier for ships and modules.
    pub tier: Option<Tier>,
}

impl ParamSet {
    fn matches(&self, entry: &RawParamEntry) -> bool {
        let same = |wanted: Option<&str>, found: Option<&str>| {
            wanted.is_none_or(|wanted| found.is_some_and(|found| found.eq_ignore_ascii_case(wanted)))
        };

        same(Some(&self.kind), entry.kind.as_deref())
            && same(self.nation.as_deref(), entry.nation.as_deref())
            && same(self.species.as_deref(), entry.species.as_deref())
            && self.tier.is_none_or(|tier| entry.level == Some(tier.0))
    }
}

/// Filter output, cut to the byte budget.
#[derive(Debug, Clone, Serialize)]
pub struct GameParamsQueryOutput {
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    /// Entries the selection matched.
    pub matched: usize,
    /// Entries the filter ran on: `matched` capped at the limit, fewer when
    /// evaluation stopped early.
    pub queried: usize,
    /// Size in bytes of all filter output as compact JSON. When evaluation
    /// stopped early, the size so far.
    pub total_bytes: usize,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<StopReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// The start of the first value that did not fit the budget.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial: Option<PartialValue>,
    /// Entries whose evaluation failed, how many.
    #[serde(skip_serializing_if = "is_zero")]
    pub failed: usize,
    /// The first few failures.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ParamFailure>,
    /// Entries with at least one output value, in selection order.
    pub results: Vec<ParamResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Output passed 1 MiB.
    OutputLimit,
    /// Evaluation passed its time limit.
    TimeLimit,
    /// The filter recursed too deeply or used too much memory.
    Crashed,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParamResult {
    pub param: String,
    /// English name, for entries that have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub values: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PartialValue {
    pub param: String,
    /// JSON text cut at the budget.
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParamFailure {
    pub param: String,
    /// The jaq error, cut to a few hundred bytes.
    pub error: String,
}

/// The raw GameParams store of a [`GameData`], opened or built on first use
/// and reopened when cache eviction has removed it.
pub(crate) struct RawParamsSlot {
    dump_dir: PathBuf,
    cache_root: Option<PathBuf>,
    build: BuildNumber,
    /// Locked while opening or building, so one process builds a store once.
    state: Mutex<SlotState>,
}

impl RawParamsSlot {
    pub(crate) fn new(dump_dir: &Path, cache_root: Option<&Path>, build: BuildNumber) -> Self {
        Self {
            dump_dir: dump_dir.to_path_buf(),
            cache_root: cache_root.map(Path::to_path_buf),
            build,
            state: Mutex::new(SlotState::Empty),
        }
    }

    fn get(&self) -> Result<Arc<RawParams>, Arc<RawParamsError>> {
        // A build takes seconds and hundreds of MB; repeating a failed one
        // on every call would stall every query behind it.
        const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);

        let Some(cache_root) = &self.cache_root else {
            return Err(Arc::new(RawParamsError::NoCacheDir));
        };
        // A poisoned lock only means another load panicked; every state is
        // consistent on its own.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match &*state {
            SlotState::Open(store) if store.is_present() => {
                store.touch();
                return Ok(Arc::clone(store));
            }
            SlotState::Failed { at, error } if at.elapsed() < RETRY_AFTER => return Err(Arc::clone(error)),
            // Evicted, never opened, or due for a retry.
            SlotState::Open(_) | SlotState::Failed { .. } | SlotState::Empty => {}
        }

        match RawParams::open_or_build(cache_root, self.build.raw(), || read_game_params(&self.dump_dir)) {
            Ok(store) => {
                let store = Arc::new(store);
                *state = SlotState::Open(Arc::clone(&store));
                Ok(store)
            }
            Err(error) => {
                let error = Arc::new(error);
                *state = SlotState::Failed { at: Instant::now(), error: Arc::clone(&error) };
                Err(error)
            }
        }
    }

    /// Drops `store` after a read failed, so the next call reopens or
    /// rebuilds it. A damaged record also unmarks the store on disk.
    fn discard(&self, store: &Arc<RawParams>, error: &RawParamsError) {
        if matches!(error, RawParamsError::DamagedRecord { .. }) {
            store.invalidate();
        }

        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(&*state, SlotState::Open(open) if Arc::ptr_eq(open, store)) {
            *state = SlotState::Empty;
        }
    }
}

impl GameData {
    /// The raw GameParams store, opened or built under the cache directory
    /// on first use. Building takes about ten seconds and up to 1 GB of
    /// memory while it runs; call this at startup to take that cost before
    /// the first query.
    pub fn raw_params(&self) -> Result<Arc<RawParams>, QueryError> {
        self.raw_params.get().map_err(|source| QueryError::RawParams { source })
    }

    /// Runs a jq filter over selected raw GameParams entries in a worker
    /// process (see [`crate::jq_worker`]).
    pub fn gameparams_query(&self, args: &GameParamsQueryArgs) -> Result<GameParamsQueryOutput, QueryError> {
        const DEFAULT_PARAM_LIMIT: u32 = 20;
        const MAX_PARAM_LIMIT: u32 = 200;
        const LIMITS: Limits =
            Limits { steps_per_input: 1_000_000, time: Duration::from_secs(5), output_bytes: 1024 * 1024 };

        let store = self.raw_params()?;
        let mut interpretations = Interpretations::default();

        let selected: Vec<&RawParamEntry> = match &args.select {
            ParamSelection::Param(key) => {
                let entry = store.find(key).ok_or_else(|| QueryError::ParamNotFound { query: key.clone() })?;
                vec![entry]
            }
            ParamSelection::Ship(name) => {
                let ship = self.resolve_ship(name, &mut interpretations)?;
                let param = ship.param.name();
                let entry = store.find(param).ok_or_else(|| QueryError::ParamNotFound { query: param.to_owned() })?;
                vec![entry]
            }
            ParamSelection::Set(set) => store.entries().iter().filter(|entry| set.matches(entry)).collect(),
        };
        let matched = selected.len();
        let limit = args.limit.unwrap_or(DEFAULT_PARAM_LIMIT).clamp(1, MAX_PARAM_LIMIT) as usize;
        let selected = &selected[..matched.min(limit)];

        let mut read_error = None;
        let records = selected.iter().map_while(|entry| match store.read(entry) {
            Ok(json) => Some(json),
            Err(error) => {
                read_error = Some(error);
                None
            }
        });
        let evaluation = jq_worker::evaluate(&args.filter, records, LIMITS)?;
        if let Some(error) = read_error {
            self.raw_params.discard(&store, &error);
            return Err(QueryError::RawParams { source: Arc::new(error) });
        }

        let stopped = evaluation.stopped.map(|stop| match stop {
            Stop::OutputLimit => StopReason::OutputLimit,
            Stop::TimeLimit => StopReason::TimeLimit,
            Stop::Crashed => StopReason::Crashed,
        });
        let mut output = GameParamsQueryOutput {
            interpretations,
            matched,
            queried: evaluation.inputs.len(),
            total_bytes: evaluation.output_bytes,
            truncated: stopped.is_some() || selected.len() < matched,
            stopped,
            hint: None,
            partial: None,
            failed: 0,
            errors: Vec::new(),
            results: Vec::new(),
        };
        let mut budget = Budget::new(args.max_bytes);
        for (entry, outputs) in selected.iter().zip(evaluation.inputs) {
            if let Some(error) = outputs.error {
                self.add_failure(&mut output, &mut budget, entry, &error);
            }
            if !outputs.values.is_empty() && !budget.exhausted {
                self.add_values(&mut output, &mut budget, entry, outputs.values);
            }
        }
        output.hint = hint(&output, &budget, limit);

        Ok(output)
    }

    fn add_failure(&self, output: &mut GameParamsQueryOutput, budget: &mut Budget, entry: &RawParamEntry, error: &str) {
        // jaq errors can quote the whole input value.
        const MAX_ERROR_BYTES: usize = 300;
        const MAX_LISTED_ERRORS: usize = 5;

        output.failed += 1;
        let error = &error[..floor_char_boundary(error, MAX_ERROR_BYTES)];
        let cost = json_string_len(&entry.name) + json_string_len(error) + Budget::RECORD_COST;
        if output.errors.len() < MAX_LISTED_ERRORS && budget.spend(cost) {
            output.errors.push(ParamFailure { param: entry.name.clone(), error: error.to_owned() });
        } else {
            output.truncated = true;
        }
    }

    fn add_values(
        &self,
        output: &mut GameParamsQueryOutput,
        budget: &mut Budget,
        entry: &RawParamEntry,
        texts: Vec<String>,
    ) {
        let name = self.param_display_name(&entry.name);
        let header = json_string_len(&entry.name) + name.as_deref().map_or(0, json_string_len) + Budget::RECORD_COST;
        if !budget.spend(header) {
            output.truncated = true;
            return;
        }

        let mut values = Vec::new();
        for text in texts {
            if !budget.spend(text.len() + 1) {
                output.truncated = true;
                // The partial value is a JSON string in a record of its own.
                let room = budget.left().saturating_sub(json_string_len(&entry.name) + Budget::RECORD_COST);
                let text = escaped_prefix(&text, room);
                if !text.is_empty() {
                    output.partial = Some(PartialValue { param: entry.name.clone(), text: text.to_owned() });
                }
                break;
            }
            // jaq prints values JSON cannot represent, such as NaN, as non-JSON text.
            values.push(serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)));
        }
        if !values.is_empty() {
            output.results.push(ParamResult { param: entry.name.clone(), name, values });
        }
    }

    fn param_display_name(&self, param_name: &str) -> Option<String> {
        let param = self.provider().game_param_by_name(param_name)?;

        self.provider().localized_name_from_param(&param)
    }
}

enum SlotState {
    Empty,
    Open(Arc<RawParams>),
    /// The last open or build failed; calls report it until a retry is due.
    Failed {
        at: Instant,
        error: Arc<RawParamsError>,
    },
}

/// Bytes of output left for results, errors and the partial value.
struct Budget {
    max: usize,
    used: usize,
    /// Something did not fit; later results are left out so the ones given
    /// stay a prefix of the output.
    exhausted: bool,
}

impl Budget {
    /// Approximate JSON framing of one result or error: keys, quotes, braces.
    const RECORD_COST: usize = 40;

    fn new(max_bytes: Option<u32>) -> Self {
        const DEFAULT_MAX_BYTES: u32 = 6_000;
        const MIN_MAX_BYTES: u32 = 500;
        const MAX_MAX_BYTES: u32 = 24_000;

        let max = max_bytes.unwrap_or(DEFAULT_MAX_BYTES).clamp(MIN_MAX_BYTES, MAX_MAX_BYTES) as usize;

        Self { max, used: 0, exhausted: false }
    }

    fn left(&self) -> usize {
        self.max - self.used
    }

    /// Takes `bytes` when they fit.
    fn spend(&mut self, bytes: usize) -> bool {
        if bytes > self.left() {
            self.exhausted = true;
            return false;
        }
        self.used += bytes;

        true
    }
}

fn read_game_params(dump_dir: &Path) -> Result<Vec<u8>, RawParamsError> {
    let dump = Dump::open(dump_dir);
    let mut file = dump
        .vfs()
        .join(GAME_PARAMS_DATA)
        .and_then(|path| path.open_file())
        .map_err(|source| RawParamsError::Source { dir: dump_dir.to_path_buf(), source })?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| RawParamsError::Io { path: dump_dir.join(GAME_PARAMS_DATA), source })?;

    Ok(bytes)
}

fn hint(output: &GameParamsQueryOutput, budget: &Budget, limit: usize) -> Option<String> {
    match output.stopped {
        Some(StopReason::TimeLimit) => {
            return Some("the filter ran out of time; select fewer entries or avoid nested iteration".to_owned());
        }
        Some(StopReason::Crashed) => {
            return Some(
                "the filter recursed too deeply or used too much memory; avoid recursive definitions and building \
                 large values"
                    .to_owned(),
            );
        }
        Some(StopReason::OutputLimit) | None => {}
    }
    if output.matched == 0 {
        return Some("no entry matched the selection; check type, nation and species with a single param".to_owned());
    }
    if budget.exhausted || output.stopped == Some(StopReason::OutputLimit) {
        const NARROW: &str = "narrow the filter: list fields with `keys`, descend with `.Field | keys`, or pick \
                              fields with `{a, b}` or `[.[] | .x?]`";
        // Record framing can fill the budget while the values stay under it.
        let size = match output.total_bytes {
            total if total > budget.max => format!("output is {total} bytes,"),
            _ => "output is".to_owned(),
        };
        return Some(format!("{size} over the {}-byte budget; {NARROW}", budget.max));
    }
    if output.queried < output.matched {
        return Some(format!(
            "{} entries matched and the first {limit} were read; add nation, species or tier, or raise limit",
            output.matched
        ));
    }
    if output.results.is_empty() && output.failed == 0 {
        return Some("the filter produced no output; check field names with `keys`".to_owned());
    }

    None
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

/// Bytes `c` takes inside a JSON string.
fn escaped_len(c: char) -> usize {
    match c {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        c if c < ' ' => 6,
        c => c.len_utf8(),
    }
}

/// Bytes `text` takes as a JSON string, without the quotes.
fn json_string_len(text: &str) -> usize {
    text.chars().map(escaped_len).sum()
}

/// The longest prefix of `text` that takes at most `max` bytes as a JSON
/// string, without the quotes.
fn escaped_prefix(text: &str, max: usize) -> &str {
    let mut used = 0;
    for (idx, c) in text.char_indices() {
        used += escaped_len(c);
        if used > max {
            return &text[..idx];
        }
    }

    text
}
