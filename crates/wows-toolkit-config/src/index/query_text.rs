//! The query text grammar: parser and printer.
//!
//! The printed form is both the persistence format and the share format, so
//! `parse(print(x)) == x` is a correctness requirement, not a nicety. See the
//! round-trip test in this module.

use std::ops::Range;

use jiff::Timestamp;

use super::query_ast::Expr;
use super::query_ast::MatchExpr;
use super::query_ast::MatchField;
use super::query_ast::MatchTerm;
use super::query_ast::Op;
use super::query_ast::Value;
use super::query_ast::ValueKind;
use super::rows::MatchOutcome;

/// A parse failure, carrying the byte range of the offending input so the query
/// bar can underline exactly that substring.
///
/// The span and the structured fields are the interface. Never recover any of
/// this by parsing the `Display` output.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{kind}")]
pub struct QueryParseError {
    pub span: Range<usize>,
    pub kind: ParseErrorKind,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ParseErrorKind {
    #[error("unexpected input, expected one of: {}", expected.join(", "))]
    Unexpected { expected: Vec<&'static str> },
    #[error("unknown field {name}")]
    UnknownField { name: String, suggestion: Option<&'static str> },
    #[error("operator {op} is not valid for field {field}")]
    BadOperator { field: &'static str, op: String, allowed: Vec<&'static str> },
    #[error("could not read a value for {field}")]
    BadValue { field: &'static str, allowed: Option<Vec<String>> },
    #[error("unbalanced parenthesis")]
    Unbalanced,
}

impl QueryParseError {
    fn new(span: Range<usize>, kind: ParseErrorKind) -> Self {
        Self { span, kind }
    }
}

/// Parse a query string into a `MatchExpr`. An empty or whitespace-only input
/// is the empty conjunction, which matches everything.
pub fn parse_query(input: &str) -> Result<MatchExpr, QueryParseError> {
    if input.trim().is_empty() {
        return Ok(Expr::All(vec![]));
    }
    parse_term_at(input, 0)
}

/// Parse exactly one `key op value` term, or a bare word, starting at byte
/// `base` within the full input (so reported spans are absolute).
fn parse_term_at(input: &str, base: usize) -> Result<MatchExpr, QueryParseError> {
    let trimmed_start = input.len() - input.trim_start().len();
    let s = input.trim();
    let base = base + trimmed_start;

    let Some((key, op_str, value_str, key_span, value_span)) = split_term(s, base) else {
        return Ok(Expr::Leaf(MatchTerm::FreeText(unquote(s))));
    };

    let Some(field) = MatchField::from_name(&key) else {
        return Err(QueryParseError::new(
            key_span,
            ParseErrorKind::UnknownField { name: key.clone(), suggestion: closest_field(&key) },
        ));
    };

    let op = op_from_token(&op_str, field.value_kind()).ok_or_else(|| {
        QueryParseError::new(
            key_span.clone(),
            ParseErrorKind::BadOperator {
                field: field.name(),
                op: op_str.clone(),
                allowed: field.allowed_ops().iter().map(|o| o.as_token()).collect(),
            },
        )
    })?;

    if !field.allowed_ops().contains(&op) {
        return Err(QueryParseError::new(
            key_span,
            ParseErrorKind::BadOperator {
                field: field.name(),
                op: op_str,
                allowed: field.allowed_ops().iter().map(|o| o.as_token()).collect(),
            },
        ));
    }

    if op.is_nullary() {
        return Ok(Expr::Leaf(MatchTerm::Field(field, op, Value::NoOperand)));
    }

    let value = parse_value(field.value_kind(), &value_str).ok_or_else(|| {
        QueryParseError::new(
            value_span,
            ParseErrorKind::BadValue { field: field.name(), allowed: enumerable_values(field.value_kind()) },
        )
    })?;

    Ok(Expr::Leaf(MatchTerm::Field(field, op, value)))
}

/// `(key, op, value, key_span, value_span)`.
type SplitTerm = (String, String, String, Range<usize>, Range<usize>);

/// Split `key op value` into its parts with absolute spans, or `None` if the
/// input has no operator and is therefore a bare word.
fn split_term(s: &str, base: usize) -> Option<SplitTerm> {
    for token in ["is-not-set", "is-set"] {
        if let Some(idx) = s.find(token) {
            let key = s[..idx].trim().to_string();
            let key_span = base..base + key.len();
            return Some((key, token.to_string(), String::new(), key_span, base..base + s.len()));
        }
    }
    // Longest operators first so ">=" is not read as ">".
    for token in [">=", "<=", "!=", ">", "<", "=", ":"] {
        if let Some(idx) = s.find(token) {
            let key = s[..idx].trim().to_string();
            if key.is_empty() {
                return None;
            }
            let value_start = idx + token.len();
            let key_span = base..base + key.len();
            let value_span = base + value_start..base + s.len();
            return Some((key, token.to_string(), s[value_start..].trim().to_string(), key_span, value_span));
        }
    }
    None
}

fn op_from_token(token: &str, kind: ValueKind) -> Option<Op> {
    let textual = matches!(kind, ValueKind::Text);
    let enumish = matches!(
        kind,
        ValueKind::Outcome
            | ValueKind::Relation
            | ValueKind::Division
            | ValueKind::Class
            | ValueKind::Bool
            | ValueKind::Ship
            | ValueKind::Account
            | ValueKind::Source
    );
    match token {
        ":" if textual => Some(Op::Contains),
        "=" if textual => Some(Op::Equals),
        ":" | "=" if enumish => Some(Op::Is),
        ":" | "=" => Some(Op::Eq),
        "!=" if textual => Some(Op::NotEquals),
        "!=" if enumish => Some(Op::IsNot),
        "!=" => Some(Op::Ne),
        ">" => Some(Op::Gt),
        ">=" => Some(Op::Ge),
        "<" => Some(Op::Lt),
        "<=" => Some(Op::Le),
        "is-set" => Some(Op::IsSet),
        "is-not-set" => Some(Op::IsNotSet),
        _ => None,
    }
}

fn parse_value(kind: ValueKind, raw: &str) -> Option<Value> {
    let s = unquote(raw);
    if s.is_empty() {
        return None;
    }
    match kind {
        ValueKind::Text => Some(Value::Text(s)),
        ValueKind::Int => parse_int(&s).map(Value::Int),
        ValueKind::Float => s.parse::<f64>().ok().map(Value::Float),
        ValueKind::Bool => match s.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Some(Value::Bool(true)),
            "false" | "no" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        ValueKind::Outcome => MatchOutcome::from_db_str(&s.to_ascii_lowercase()).map(Value::Outcome),
        ValueKind::Timestamp => parse_date(&s).map(Value::Timestamp),
        // Ship, Account, Source, Relation, Division, and Class values arrive
        // from the widget's pickers as ids; Task 8 adds their text forms.
        _ => None,
    }
}

/// An integer with an optional `k` or `m` multiplier.
fn parse_int(s: &str) -> Option<i64> {
    let lower = s.to_ascii_lowercase();
    let (digits, mult) = match lower.strip_suffix('k') {
        Some(d) => (d, 1_000),
        None => match lower.strip_suffix('m') {
            Some(d) => (d, 1_000_000),
            None => (lower.as_str(), 1),
        },
    };
    if mult == 1 {
        return digits.parse::<i64>().ok();
    }
    // Allow a fractional multiplier, so "1.5k" is 1500.
    digits.parse::<f64>().ok().map(|v| (v * mult as f64).round() as i64)
}

/// `YYYY-MM-DD`, interpreted as midnight UTC.
fn parse_date(s: &str) -> Option<Timestamp> {
    let date: jiff::civil::Date = s.parse().ok()?;
    date.to_zoned(jiff::tz::TimeZone::UTC).ok().map(|z| z.timestamp())
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

/// The accepted values for an enum-valued kind, so a `BadValue` error can tell
/// the user what would have worked. `None` for open kinds like text.
fn enumerable_values(kind: ValueKind) -> Option<Vec<String>> {
    match kind {
        ValueKind::Outcome => Some(
            [MatchOutcome::Win, MatchOutcome::Loss, MatchOutcome::Draw, MatchOutcome::Unknown]
                .iter()
                .map(|o| o.as_db_str().to_string())
                .collect(),
        ),
        ValueKind::Bool => Some(vec!["true".into(), "false".into()]),
        _ => None,
    }
}

/// The known field name closest to `name` by a single edit, for the
/// "did you mean" in `UnknownField`.
fn closest_field(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    MatchField::ALL
        .iter()
        .flat_map(|f| std::iter::once(f.name()).chain(f.aliases().iter().copied()))
        .find(|candidate| within_one_edit(&lower, candidate))
}

/// True when `a` and `b` differ by at most one insertion, deletion, or
/// substitution. Cheaper and more predictable than a full edit distance for the
/// only thing this is used for.
fn within_one_edit(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (short, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    let mut i = 0;
    let mut j = 0;
    let mut edits = 0;
    while i < short.len() && j < long.len() {
        if short[i] == long[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        if short.len() == long.len() {
            i += 1;
        }
        j += 1;
    }
    edits + (long.len() - j) <= 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::query_ast::MatchField;
    use crate::index::query_ast::MatchTerm;
    use crate::index::query_ast::Op;
    use crate::index::query_ast::Value;
    use crate::index::rows::MatchOutcome;

    fn one(input: &str) -> MatchTerm {
        match parse_query(input).unwrap() {
            Expr::Leaf(t) => t,
            other => panic!("expected a single leaf, got {other:?}"),
        }
    }

    #[test]
    fn parses_an_enum_term() {
        assert_eq!(
            one("outcome:win"),
            MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win))
        );
    }

    #[test]
    fn parses_a_text_term_with_contains_and_equals() {
        assert_eq!(one("map:ocean"), MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("ocean".into())));
        assert_eq!(one("map=ocean"), MatchTerm::Field(MatchField::Map, Op::Equals, Value::Text("ocean".into())));
    }

    #[test]
    fn parses_a_quoted_value_with_spaces() {
        assert_eq!(
            one("map:\"new dawn\""),
            MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("new dawn".into()))
        );
    }

    #[test]
    fn parses_numeric_comparisons_with_k_and_m_suffixes() {
        assert_eq!(one("build>=1234"), MatchTerm::Field(MatchField::Build, Op::Ge, Value::Int(1234)));
        // Suffixes are a value-level concern, so they work on any int field.
        match one("build>100k") {
            MatchTerm::Field(_, Op::Gt, Value::Int(n)) => assert_eq!(n, 100_000),
            other => panic!("got {other:?}"),
        }
        match one("build>2m") {
            MatchTerm::Field(_, Op::Gt, Value::Int(n)) => assert_eq!(n, 2_000_000),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_an_absolute_date() {
        match one("date>=2026-01-01") {
            MatchTerm::Field(MatchField::Date, Op::Ge, Value::Timestamp(_)) => {}
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn field_aliases_resolve_to_the_canonical_field() {
        assert!(matches!(one("result:win"), MatchTerm::Field(MatchField::Outcome, _, _)));
        assert!(matches!(one("mode:pvp"), MatchTerm::Field(MatchField::GameType, _, _)));
    }

    #[test]
    fn field_names_are_case_insensitive() {
        assert!(matches!(one("OUTCOME:Win"), MatchTerm::Field(MatchField::Outcome, _, _)));
    }

    #[test]
    fn nullary_operators_take_no_value() {
        assert_eq!(one("build is-set"), MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand));
        assert_eq!(one("build is-not-set"), MatchTerm::Field(MatchField::Build, Op::IsNotSet, Value::NoOperand));
    }

    #[test]
    fn a_bare_word_becomes_free_text() {
        assert_eq!(one("yamato"), MatchTerm::FreeText("yamato".into()));
    }

    #[test]
    fn an_unknown_field_reports_its_span_and_a_suggestion() {
        let err = parse_query("outcom:win").unwrap_err();
        match &err.kind {
            ParseErrorKind::UnknownField { name, suggestion } => {
                assert_eq!(name, "outcom");
                assert_eq!(*suggestion, Some("outcome"));
            }
            other => panic!("got {other:?}"),
        }
        assert_eq!(err.span, 0..6, "span must cover exactly the bad field name");
    }

    #[test]
    fn a_bad_operator_for_a_field_reports_the_allowed_set() {
        let err = parse_query("outcome>win").unwrap_err();
        match &err.kind {
            ParseErrorKind::BadOperator { field, allowed, .. } => {
                assert_eq!(*field, "outcome");
                assert!(allowed.contains(&"="), "got {allowed:?}");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn a_bad_value_reports_the_field_and_its_span() {
        let err = parse_query("outcome:banana").unwrap_err();
        match &err.kind {
            ParseErrorKind::BadValue { field, allowed } => {
                assert_eq!(*field, "outcome");
                let allowed = allowed.as_ref().expect("outcome is an enum, so its values are enumerable");
                assert!(allowed.iter().any(|a| a == "win"), "got {allowed:?}");
            }
            other => panic!("got {other:?}"),
        }
        assert_eq!(err.span, 8..14, "span must cover exactly the bad value");
    }

    #[test]
    fn an_empty_query_is_the_empty_conjunction() {
        assert_eq!(parse_query("").unwrap(), Expr::All(vec![]));
        assert_eq!(parse_query("   ").unwrap(), Expr::All(vec![]));
    }
}
