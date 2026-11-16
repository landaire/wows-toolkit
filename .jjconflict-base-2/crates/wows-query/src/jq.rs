//! The jq engine: compiles a filter with jaq and runs it over JSON records.
//!
//! This runs inside the jq worker process (see [`crate::jq_worker`]), which
//! the caller kills at a deadline. The engine itself bounds what it can:
//! loops that need not end (`range`, `repeat`, `recurse(f)`, `while`,
//! `until`) spend steps from a per-input budget and stop at the deadline,
//! output stops at a byte limit, and `env` is disabled so a filter cannot
//! read the environment.

use std::cell::Cell;
use std::time::Duration;
use std::time::Instant;

use jaq_core::Bind;
use jaq_core::Compiler;
use jaq_core::Ctx;
use jaq_core::Error;
use jaq_core::Exn;
use jaq_core::Native;
use jaq_core::Vars;
use jaq_core::box_iter::box_once;
use jaq_core::data::JustLut;
use jaq_core::load::Arena;
use jaq_core::load::File;
use jaq_core::load::Loader;
use jaq_core::load::lex::StrPart;
use jaq_core::load::lex::Tok;
use jaq_core::load::lex::Token;
use jaq_core::native::Fun;
use jaq_core::native::bome;
use jaq_json::Val;
use serde::Deserialize;
use serde::Serialize;

use crate::error::CompileError;

type Data = JustLut<Val>;

/// Bounds on one evaluation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    /// Loop steps each input may spend.
    pub steps_per_input: u64,
    /// Wall time for the whole evaluation.
    pub time: Duration,
    /// Output, as compact JSON, after which evaluation stops.
    pub output_bytes: usize,
}

/// Outputs of the filter for one input.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct InputOutputs {
    /// Each output value as JSON text. Values JSON cannot represent, such as
    /// NaN, are written as jaq prints them. The value that passed the output
    /// limit is cut there.
    pub values: Vec<String>,
    /// Size of every output value before cutting.
    pub output_bytes: usize,
    /// The error that ended evaluation of this input.
    pub error: Option<String>,
}

/// Why an evaluation ended before every input was evaluated in full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stop {
    OutputLimit,
    TimeLimit,
    /// The worker process died, from stack overflow or the memory limit.
    Crashed,
}

/// A compiled filter.
pub(crate) struct Program(jaq_core::Filter<Data>);

impl Program {
    pub(crate) fn compile(program: &str) -> Result<Self, CompileError> {
        // `range/3` wraps the native, renamed `__range`, and precedes the
        // standard definitions that call it. The other loops follow them, so
        // they shadow the standard ones for the filter while the standard
        // library keeps its own data-bounded uses. Filters may not name `__`
        // identifiers, so `__range` is reachable only through `range`.
        const RANGE_DEFS: &str = "def range($from; $upto; $by): __range($from; $upto; $by) | __tick;";
        const LOOP_DEFS: &str = r#"
            def repeat(f): def rec: f, (__tick | rec); rec;
            def recurse(f): def rec: ., (f | __tick | rec); rec;
            def recurse(f; cond): recurse(f | select(cond));
            def while(cond; update): def rec: if cond then ., (update | __tick | rec) else empty end; rec;
            def until(cond; update): def rec: if cond then . else update | __tick | rec end; rec;
            def combinations($n): [limit($n; repeat(.))] | combinations;
        "#;

        check_names(program)?;

        let range_defs = jaq_core::load::parse(RANGE_DEFS, |parser| parser.defs()).expect("RANGE_DEFS parse");
        let loop_defs = jaq_core::load::parse(LOOP_DEFS, |parser| parser.defs()).expect("LOOP_DEFS parse");
        let defs = range_defs
            .into_iter()
            .chain(jaq_core::defs())
            .chain(jaq_std::defs())
            .chain(jaq_json::defs())
            .chain(loop_defs);
        let loader = Loader::new(defs);
        let arena = Arena::default();
        let modules =
            loader.load(&arena, File { code: program, path: () }).map_err(|errors| load_error(program, errors))?;
        // `import "x" as $y` is not a module load: the loader leaves the data
        // to the caller, and evaluating an unsupplied `$y` panics.
        jaq_core::load::import(&modules, |_| Err("data imports are not available".to_owned()))
            .map_err(|errors| load_error(program, errors))?;
        let filter = Compiler::default().with_funs(funs()).compile(modules).map_err(compile_error)?;

        Ok(Self(filter))
    }

    /// Runs the filter on one JSON text. `output_left` is the output the
    /// evaluation may still produce; it shrinks by what this input produced.
    pub(crate) fn run(
        &self,
        input: &[u8],
        steps: u64,
        deadline: Instant,
        output_left: &mut usize,
    ) -> (InputOutputs, Option<Stop>) {
        let mut outputs = InputOutputs::default();
        let value = match jaq_json::read::parse_single(input) {
            Ok(value) => value,
            Err(error) => {
                outputs.error = Some(format!("record is not JSON: {error}"));
                return (outputs, None);
            }
        };

        let mut stop = None;
        BUDGET.set(Some(Budget { steps_left: steps, deadline }));
        let ctx = Ctx::<Data>::new(&self.0.lut, Vars::new([]));
        for result in self.0.id.run((ctx, value)) {
            let value = match result {
                Ok(value) => value,
                Err(exception) => {
                    outputs.error = Some(exception_message(exception));
                    break;
                }
            };

            let mut text = value.to_string();
            outputs.output_bytes += text.len();
            if text.len() > *output_left {
                text.truncate(floor_char_boundary(&text, *output_left));
                outputs.values.push(text);
                *output_left = 0;
                stop = Some(Stop::OutputLimit);
                break;
            }
            *output_left -= text.len();
            outputs.values.push(text);
        }
        BUDGET.set(None);

        // A runtime error at or after the deadline is the step check's.
        if outputs.error.is_some() && Instant::now() >= deadline {
            stop = Some(Stop::TimeLimit);
        }

        (outputs, stop)
    }
}

#[derive(Debug, Clone, Copy)]
struct Budget {
    steps_left: u64,
    deadline: Instant,
}

thread_local! {
    static BUDGET: Cell<Option<Budget>> = const { Cell::new(None) };
}

fn exception_message(exception: Exn<'_, Val>) -> String {
    let error = match exception.get_err() {
        Ok(error) => error,
        // `halt` and an uncaught `break` are the other exceptions.
        Err(_) => return "the filter halted".to_owned(),
    };

    // jaq errors can quote the whole input value.
    const MAX_ERROR_BYTES: usize = 300;

    let mut message = match error.into_val() {
        Val::TStr(text) => String::from_utf8_lossy(&text).into_owned(),
        other => other.to_string(),
    };
    message.truncate(floor_char_boundary(&message, MAX_ERROR_BYTES));

    message
}

/// Spends one loop step.
fn tick() -> Result<(), Error<Val>> {
    let Some(mut budget) = BUDGET.get() else {
        return Ok(());
    };

    if budget.steps_left == 0 {
        return Err(Error::str("the filter exceeded its loop step limit; iterate over less data"));
    }
    budget.steps_left -= 1;
    // Reading the clock every step would dominate tight loops.
    if budget.steps_left % 1024 == 0 && Instant::now() >= budget.deadline {
        budget.steps_left = 0;
        BUDGET.set(Some(budget));
        return Err(Error::str("the filter ran out of time"));
    }
    BUDGET.set(Some(budget));

    Ok(())
}

fn funs() -> impl Iterator<Item = Fun<Data>> {
    let tick_native = Native::<Data>::new(|cv| bome(tick().map(|()| cv.1)))
        .with_paths(|cv| box_once(tick().map(|()| cv.1).map_err(Exn::from)))
        .with_update(|cv, update| match tick() {
            Ok(()) => update(cv.1),
            Err(error) => box_once(Err(Exn::from(error))),
        });

    jaq_core::funs()
        .chain(jaq_std::funs())
        .chain(jaq_json::funs())
        .map(|(name, args, native)| match name {
            "range" => ("__range", args, native),
            "env" => ("env", args, Native::new(|_| bome(Err(Error::str("env is not available"))))),
            name => (name, args, native),
        })
        .chain([("__tick", Box::<[Bind]>::default(), tick_native)])
}

/// Rejects a program that names the internal `__` natives. Lexing first also
/// reports syntax errors before loading.
fn check_names(program: &str) -> Result<(), CompileError> {
    let tokens = jaq_core::load::Lexer::new(program).lex().map_err(|errors| CompileError::Syntax {
        errors: distinct(errors.iter().map(|(expected, found)| syntax_message(program, expected.as_str(), found))),
    })?;

    let mut pending: Vec<&Token<&str>> = tokens.iter().collect();
    while let Some(Token(text, kind)) = pending.pop() {
        match kind {
            Tok::Word if text.starts_with("__") => return Err(CompileError::Reserved { name: (*text).to_owned() }),
            Tok::Block(inner) => pending.extend(inner),
            Tok::Str(parts) => pending.extend(parts.iter().filter_map(|part| match part {
                StrPart::Term(term) => Some(term),
                StrPart::Str(_) | StrPart::Char(_) => None,
            })),
            Tok::Word | Tok::Var | Tok::Fmt | Tok::Num | Tok::Sym => {}
        }
    }

    Ok(())
}

fn load_error(program: &str, errors: jaq_core::load::Errors<&str, ()>) -> CompileError {
    use jaq_core::load::Error as LoadError;

    let mut messages = Vec::new();
    for (_file, error) in errors {
        match error {
            LoadError::Io(_) => return CompileError::Module,
            LoadError::Lex(lex) => {
                messages.extend(lex.iter().map(|(expected, found)| syntax_message(program, expected.as_str(), found)))
            }
            LoadError::Parse(parse) => {
                messages.extend(parse.iter().map(|(expected, found)| syntax_message(program, expected.as_str(), found)))
            }
        }
    }

    CompileError::Syntax { errors: distinct(messages) }
}

/// `messages` without repeats, in first-seen order.
fn distinct(messages: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();

    messages.into_iter().filter(|message| seen.insert(message.clone())).collect()
}

fn compile_error(errors: jaq_core::compile::Errors<&str, ()>) -> CompileError {
    let names = errors
        .into_iter()
        .flat_map(|(_file, undefined)| undefined)
        .map(|(name, kind)| match kind {
            jaq_core::compile::Undefined::Filter(arity) => format!("filter {name}/{arity}"),
            other => format!("{} {name}", other.as_str()),
        })
        .collect();

    CompileError::Undefined { names }
}

/// "expected X at byte N" with the text found there.
fn syntax_message(program: &str, expected: &str, found: &str) -> String {
    let program_start = program.as_ptr() as usize;
    let found_start = found.as_ptr() as usize;
    let Some(offset) = found_start.checked_sub(program_start).filter(|offset| *offset <= program.len()) else {
        return format!("expected {expected}");
    };

    if found.is_empty() {
        return format!("expected {expected} at byte {offset} (end of filter)");
    }
    let shown: String = found.chars().take(20).collect();

    format!("expected {expected} at byte {offset}, found {shown:?}")
}

/// The largest index at most `max` that is a character boundary of `text`.
pub(crate) fn floor_char_boundary(text: &str, max: usize) -> usize {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }

    end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(program: &str, input: &str) -> Result<Vec<String>, String> {
        let program = Program::compile(program).map_err(|error| error.to_string())?;
        let mut output_left = 1 << 20;
        let deadline = Instant::now() + Duration::from_secs(5);
        let (outputs, _stop) = program.run(input.as_bytes(), 10_000, deadline, &mut output_left);

        match outputs.error {
            Some(error) => Err(error),
            None => Ok(outputs.values),
        }
    }

    #[test]
    fn filters_run_on_the_input() {
        let outcomes: Vec<Result<Vec<String>, String>> = [
            (".a, (keys | length)", r#"{"a": [1, 2], "b": null}"#),
            ("[range(3)], [limit(2; repeat(1))]", "null"),
            ("\"42\" | tonumber", "null"),
            ("def double: . * 2; .a | double", r#"{"a": 21}"#),
        ]
        .into_iter()
        .map(|(program, input)| run(program, input))
        .collect();

        insta::assert_yaml_snapshot!(outcomes);
    }

    #[test]
    fn unsafe_and_unbounded_filters_are_refused() {
        let outcomes: Vec<Result<Vec<String>, String>> = [
            ("env", "null"),
            ("$ENV", "null"),
            ("until(false; .)", "null"),
            ("last(range(1e12))", "null"),
            ("[recurse(. + 1)] | length", "0"),
            ("[combinations(1e9)] | length", "[1]"),
            ("__range(0; 1e12; 1)", "null"),
            ("import \"a\" as a; .", "null"),
            ("import \"a\" as $a; $a", "null"),
            (".a +", "null"),
            ("\"\\(.a\"", "null"),
            ("foo(1)", "null"),
        ]
        .into_iter()
        .map(|(program, input)| run(program, input))
        .collect();

        insta::assert_yaml_snapshot!(outcomes);
    }

    #[test]
    fn output_limit_cuts_the_last_value() {
        let program = Program::compile("\"abcdef\", 1").expect("compiles");
        let mut output_left = 4;
        let (outputs, stop) = program.run(b"null", 10, Instant::now() + Duration::from_secs(5), &mut output_left);

        assert_eq!(stop, Some(Stop::OutputLimit));
        assert_eq!(outputs.values, ["\"abc"]);
        assert_eq!(outputs.output_bytes, 8);
    }

    #[test]
    fn deadline_stops_loops() {
        let program = Program::compile("last(range(1e12))").expect("compiles");
        let mut output_left = 100;
        let deadline = Instant::now() + Duration::from_millis(50);
        let (outputs, stop) = program.run(b"null", u64::MAX, deadline, &mut output_left);

        assert_eq!(stop, Some(Stop::TimeLimit));
        assert_eq!(outputs.error.as_deref(), Some("the filter ran out of time"));
    }
}
