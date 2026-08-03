//! The query text grammar: parser and printer.
//!
//! The printed form is both the persistence format and the share format, so
//! `parse(print(x)) == x` is a correctness requirement, not a nicety. See the
//! round-trip test in this module.

use std::ops::Range;

use jiff::Timestamp;
use winnow::LocatingSlice;
use winnow::ModalResult;
use winnow::Parser;
use winnow::combinator::alt;
use winnow::combinator::delimited;
use winnow::combinator::opt;
use winnow::combinator::preceded;
use winnow::combinator::repeat;
use winnow::error::AddContext;
use winnow::error::ErrMode;
use winnow::error::ParserError;
use winnow::error::StrContext;
use winnow::stream::Stream;
use winnow::token::take_while;

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

type Input<'a> = LocatingSlice<&'a str>;

/// winnow's error for this grammar. Carries a finished `QueryParseError` when a
/// leaf production already diagnosed the problem precisely; `None` means a
/// purely structural failure, which the top level turns into `Unexpected` using
/// winnow's own reported offset.
#[derive(Debug, Clone, PartialEq)]
struct QueryErr {
    inner: Option<QueryParseError>,
    labels: Vec<&'static str>,
}

impl QueryErr {
    fn structural() -> Self {
        QueryErr { inner: None, labels: Vec::new() }
    }

    /// Wrap an already-diagnosed failure so the top level can return it verbatim.
    fn diagnosed(e: QueryParseError) -> ErrMode<Self> {
        ErrMode::Cut(QueryErr { inner: Some(e), labels: Vec::new() })
    }
}

impl<I: Stream> ParserError<I> for QueryErr {
    type Inner = Self;

    fn from_input(_input: &I) -> Self {
        QueryErr::structural()
    }

    fn into_inner(self) -> Result<Self::Inner, Self> {
        Ok(self)
    }

    /// Union the branches an `alt` tried instead of keeping only the last, so
    /// the expected-set names every alternative that could have matched here.
    fn or(mut self, other: Self) -> Self {
        for label in other.labels {
            if !self.labels.contains(&label) {
                self.labels.push(label);
            }
        }
        // A branch that got far enough to diagnose the problem outranks a bare
        // structural failure from a branch that did not.
        if self.inner.is_none() {
            self.inner = other.inner;
        }
        self
    }
}

impl<I: Stream> AddContext<I, StrContext> for QueryErr {
    fn add_context(mut self, _input: &I, _token_start: &I::Checkpoint, ctx: StrContext) -> Self {
        // Only labels are useful in the expected-set; the other StrContext
        // variants describe character classes the user never typed a name for.
        if let StrContext::Label(l) = ctx
            && !self.labels.contains(&l)
        {
            self.labels.push(l);
        }
        self
    }
}

/// What may start an operand, for failures that carry no label of their own
/// (a trailing `and`, say, where the parse dies at end-of-input). Phrased the
/// same way as the `StrContext::Label`s so a message never mixes styles.
const FALLBACK_EXPECTED: [&str; 3] = [LABEL_TERM, LABEL_GROUP, "not"];

const LABEL_TERM: &str = "a filter term";
const LABEL_GROUP: &str = "a parenthesised group";

/// Parse a query string into a `MatchExpr`. An empty or whitespace-only input
/// is the empty conjunction, which matches everything.
pub fn parse_query(input: &str) -> Result<MatchExpr, QueryParseError> {
    if input.trim().is_empty() {
        return Ok(Expr::All(vec![]));
    }
    let stream = LocatingSlice::new(input);
    match delimited(ws, or_expr, ws).parse(stream) {
        Ok(expr) => Ok(expr),
        Err(parse_err) => {
            let offset = parse_err.offset();
            let err = parse_err.into_inner();
            if let Some(diagnosed) = err.inner {
                return Err(diagnosed);
            }
            if let Some(unbalanced) = unbalanced_paren(input) {
                return Err(unbalanced);
            }
            let expected = if err.labels.is_empty() { FALLBACK_EXPECTED.to_vec() } else { err.labels };
            Err(QueryParseError::new(offset..input.len(), ParseErrorKind::Unexpected { expected }))
        }
    }
}

/// Report the position of the paren that has no partner, if any. Checked before
/// the generic `Unexpected` because "you forgot a bracket" is a far more useful
/// message than "unexpected input at byte 16".
fn unbalanced_paren(input: &str) -> Option<QueryParseError> {
    let mut opens: Vec<usize> = Vec::new();
    let mut in_quotes = false;
    for (i, c) in input.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => opens.push(i),
            ')' if !in_quotes => {
                if opens.pop().is_none() {
                    return Some(QueryParseError::new(i..i + 1, ParseErrorKind::Unbalanced));
                }
            }
            _ => {}
        }
    }
    opens.last().map(|&at| QueryParseError::new(at..at + 1, ParseErrorKind::Unbalanced))
}

fn ws(input: &mut Input<'_>) -> ModalResult<(), QueryErr> {
    take_while(0.., |c: char| c.is_whitespace()).void().parse_next(input)
}

fn or_expr(input: &mut Input<'_>) -> ModalResult<MatchExpr, QueryErr> {
    let first = and_expr.parse_next(input)?;
    let rest: Vec<MatchExpr> = repeat(0.., preceded((ws, keyword_or, ws), and_expr)).parse_next(input)?;
    Ok(if rest.is_empty() {
        first
    } else {
        let mut cs = vec![first];
        cs.extend(rest);
        Expr::Any(cs)
    })
}

fn and_expr(input: &mut Input<'_>) -> ModalResult<MatchExpr, QueryErr> {
    let first = unary.parse_next(input)?;
    // The `and` keyword is optional: juxtaposition binds the same way.
    let rest: Vec<MatchExpr> = repeat(0.., preceded((ws, opt(keyword_and), ws), unary)).parse_next(input)?;
    Ok(if rest.is_empty() {
        first
    } else {
        let mut cs = vec![first];
        cs.extend(rest);
        Expr::All(cs)
    })
}

fn unary(input: &mut Input<'_>) -> ModalResult<MatchExpr, QueryErr> {
    let negated = opt(alt((keyword_not, '-'.void()))).parse_next(input)?.is_some();
    ws.parse_next(input)?;
    let inner = primary.parse_next(input)?;
    Ok(if negated { Expr::Not(Box::new(inner)) } else { inner })
}

fn primary(input: &mut Input<'_>) -> ModalResult<MatchExpr, QueryErr> {
    alt((
        delimited(('(', ws), or_expr, (ws, ')')).context(StrContext::Label(LABEL_GROUP)),
        term.context(StrContext::Label(LABEL_TERM)),
    ))
    .parse_next(input)
}

/// One `key op value` term or bare word, delegating to the Task 6 term parser
/// once the extent of the term is known. A diagnosed failure is a `Cut`, not a
/// `Backtrack`: once the text is unambiguously a term, trying another
/// alternative would only lose the precise error.
fn term(input: &mut Input<'_>) -> ModalResult<MatchExpr, QueryErr> {
    let (raw, span) = term_text.with_span().parse_next(input)?;
    parse_term_at(raw, span.start).map_err(QueryErr::diagnosed)
}

/// The text of one term: everything up to whitespace or a paren, with quoted
/// runs kept whole so `map:"new dawn"` is a single term, plus a trailing
/// nullary operator so `build is-set` is one term rather than two.
///
/// A leading `and` / `or` / `not` is refused so a dangling keyword fails the
/// parse instead of becoming a `FreeText` leaf.
fn term_text<'a>(input: &mut Input<'a>) -> ModalResult<&'a str, QueryErr> {
    let s: &str = input;
    let mut end = word_end(s);
    if end == 0 || matches!(s[..end].to_ascii_lowercase().as_str(), "and" | "or" | "not") {
        return Err(ErrMode::Backtrack(QueryErr::structural()));
    }
    if let Some(len) = trailing_nullary_op_len(&s[end..]) {
        end += len;
    }
    Ok(input.next_slice(end))
}

/// The byte offset at which a term's leading word ends: the first unquoted
/// terminator, or the end of input.
fn word_end(s: &str) -> usize {
    let mut in_quotes = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            in_quotes = !in_quotes;
            continue;
        }
        if !in_quotes && is_term_boundary(c) {
            return i;
        }
    }
    s.len()
}

/// What ends a term. `|` is here because it is a spelling of `or`, so it has to
/// separate its operands the way whitespace does; inside quotes it is ordinary
/// text and `word_end` never consults this.
fn is_term_boundary(c: char) -> bool {
    c.is_whitespace() || c == '(' || c == ')' || c == '|'
}

/// The length of a `<space> is-set` / `<space> is-not-set` tail, which belongs
/// to the term before it rather than starting a new one.
fn trailing_nullary_op_len(rest: &str) -> Option<usize> {
    let gap = rest.len() - rest.trim_start().len();
    if gap == 0 {
        return None;
    }
    let after_gap = &rest[gap..];
    let token = ["is-not-set", "is-set"].into_iter().find(|token| {
        after_gap.get(..token.len()).is_some_and(|head| head.eq_ignore_ascii_case(token))
            && word_end(after_gap) == token.len()
    })?;
    Some(gap + token.len())
}

fn keyword_and(input: &mut Input<'_>) -> ModalResult<(), QueryErr> {
    keyword("and").parse_next(input)
}

fn keyword_or(input: &mut Input<'_>) -> ModalResult<(), QueryErr> {
    alt((keyword("or"), '|'.void())).parse_next(input)
}

fn keyword_not(input: &mut Input<'_>) -> ModalResult<(), QueryErr> {
    keyword("not").parse_next(input)
}

/// A case-insensitive keyword that must not be a prefix of a longer word, so
/// `android` does not tokenize as `and` followed by `roid`.
fn keyword(word: &'static str) -> impl FnMut(&mut Input<'_>) -> ModalResult<(), QueryErr> {
    move |input: &mut Input<'_>| {
        let s: &str = input;
        let matches_prefix = s.get(..word.len()).is_some_and(|head| head.eq_ignore_ascii_case(word));
        let boundary = s.get(word.len()..).and_then(|rest| rest.chars().next()).map(is_term_boundary).unwrap_or(true);
        if matches_prefix && boundary {
            input.next_slice(word.len());
            Ok(())
        } else {
            Err(ErrMode::Backtrack(QueryErr::structural()))
        }
    }
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

    #[test]
    fn juxtaposition_binds_as_and() {
        let q = parse_query("outcome:win map:ocean").unwrap();
        match q {
            Expr::All(cs) => assert_eq!(cs.len(), 2),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn explicit_and_matches_juxtaposition() {
        assert_eq!(parse_query("outcome:win and map:ocean").unwrap(), parse_query("outcome:win map:ocean").unwrap());
    }

    #[test]
    fn or_binds_looser_than_and() {
        // a AND b OR c parses as (a AND b) OR c
        let q = parse_query("outcome:win map:ocean or build>1000").unwrap();
        match q {
            Expr::Any(cs) => {
                assert_eq!(cs.len(), 2);
                assert!(matches!(cs[0], Expr::All(ref inner) if inner.len() == 2), "got {:?}", cs[0]);
                assert!(matches!(cs[1], Expr::Leaf(_)), "got {:?}", cs[1]);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parentheses_override_precedence() {
        let q = parse_query("outcome:win and (map:ocean or map:north)").unwrap();
        match q {
            Expr::All(cs) => {
                assert_eq!(cs.len(), 2);
                assert!(matches!(cs[1], Expr::Any(ref inner) if inner.len() == 2), "got {:?}", cs[1]);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn not_binds_tightest_and_accepts_a_dash() {
        let word = parse_query("not outcome:win").unwrap();
        assert!(matches!(word, Expr::Not(_)), "got {word:?}");
        assert_eq!(parse_query("-outcome:win").unwrap(), word);
    }

    /// A flipped precedence would produce `Not(All([a, b]))`, which this shape
    /// check rejects. `not_binds_tightest_and_accepts_a_dash` cannot see the
    /// difference because its input has no `and`.
    #[test]
    fn not_binds_tighter_than_and() {
        match parse_query("not outcome:win and map:ocean").unwrap() {
            Expr::All(cs) => {
                assert_eq!(cs.len(), 2);
                assert!(matches!(cs[0], Expr::Not(_)), "not must bind to the first term only, got {:?}", cs[0]);
                assert!(matches!(cs[1], Expr::Leaf(_)), "got {:?}", cs[1]);
            }
            other => panic!("expected All at the top, got {other:?}"),
        }
    }

    #[test]
    fn a_pipe_is_or_at_every_spacing() {
        let spaced = parse_query("outcome:win or map:ocean").unwrap();
        for input in
            ["outcome:win | map:ocean", "outcome:win |map:ocean", "outcome:win| map:ocean", "outcome:win|map:ocean"]
        {
            assert_eq!(parse_query(input).unwrap(), spaced, "{input:?} must be a disjunction");
        }
    }

    #[test]
    fn a_pipe_inside_quotes_stays_in_the_value() {
        assert_eq!(one("map:\"a|b\""), MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("a|b".into())));
    }

    #[test]
    fn a_structural_failure_names_every_alternative_it_tried() {
        // Both `alt` branches in `primary` backtrack here, so both labels must
        // survive rather than the later one replacing the earlier.
        let err = parse_query("and").unwrap_err();
        match &err.kind {
            ParseErrorKind::Unexpected { expected } => {
                assert_eq!(*expected, vec!["a parenthesised group", "a filter term"]);
            }
            other => panic!("got {other:?}"),
        }

        // Dying at end-of-input carries no label, so the fallback set is used.
        let err = parse_query("outcome:win and").unwrap_err();
        match &err.kind {
            ParseErrorKind::Unexpected { expected } => {
                assert_eq!(*expected, vec!["a filter term", "a parenthesised group", "not"]);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn not_applies_to_a_parenthesised_group() {
        let q = parse_query("not (map:ocean or map:north)").unwrap();
        match q {
            Expr::Not(inner) => assert!(matches!(*inner, Expr::Any(_)), "got {inner:?}"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn keywords_are_case_insensitive() {
        assert_eq!(
            parse_query("outcome:win AND map:ocean").unwrap(),
            parse_query("outcome:win and map:ocean").unwrap()
        );
        assert_eq!(parse_query("outcome:win OR map:ocean").unwrap(), parse_query("outcome:win or map:ocean").unwrap());
    }

    #[test]
    fn a_single_term_is_not_wrapped_in_a_one_element_all() {
        assert!(matches!(parse_query("outcome:win").unwrap(), Expr::Leaf(_)));
    }

    #[test]
    fn an_unbalanced_parenthesis_reports_its_span() {
        let err = parse_query("outcome:win and (map:ocean").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::Unbalanced), "got {:?}", err.kind);
        assert_eq!(err.span.start, 16, "span must point at the unclosed paren");
    }

    #[test]
    fn a_free_text_word_composes_with_operators() {
        let q = parse_query("yamato and outcome:win").unwrap();
        match q {
            Expr::All(cs) => {
                assert_eq!(cs[0], Expr::Leaf(MatchTerm::FreeText("yamato".into())));
                assert!(matches!(cs[1], Expr::Leaf(MatchTerm::Field(..))));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn and_or_and_not_are_not_mistaken_for_free_text() {
        // A bare "and" with nothing to join is an error, not a FreeText("and").
        assert!(parse_query("and").is_err());
        assert!(parse_query("outcome:win and").is_err());
    }

    /// Keyword and operator lookahead slices by the keyword's byte length, so a
    /// multi-byte character straddling that length must not split a `char`.
    #[test]
    fn a_multi_byte_word_does_not_split_a_char_boundary() {
        let q = parse_query("outcome:win aa\u{e9}b").unwrap();
        match q {
            Expr::All(cs) => assert_eq!(cs[1], Expr::Leaf(MatchTerm::FreeText("aa\u{e9}b".into()))),
            other => panic!("got {other:?}"),
        }
        // The nullary-operator lookahead slices by `is-set`'s six bytes.
        let q = parse_query("build a\u{e9}\u{e9}\u{e9}").unwrap();
        match q {
            Expr::All(cs) => assert_eq!(cs[1], Expr::Leaf(MatchTerm::FreeText("a\u{e9}\u{e9}\u{e9}".into()))),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn whitespace_around_a_query_is_ignored() {
        let bare = parse_query("outcome:win and map:ocean").unwrap();
        assert_eq!(parse_query("  outcome:win and map:ocean  ").unwrap(), bare);
        assert_eq!(parse_query("outcome:win\tand\nmap:ocean\n").unwrap(), bare);
    }

    /// The offset of a term within the whole query has to be threaded into
    /// `parse_term_at`, so a failure in anything but the first term still
    /// underlines the right substring.
    #[test]
    fn a_span_from_a_later_term_is_absolute_not_relative() {
        let err = parse_query("map:ocean outcom:win").unwrap_err();
        match &err.kind {
            ParseErrorKind::UnknownField { name, .. } => assert_eq!(name, "outcom"),
            other => panic!("got {other:?}"),
        }
        assert_eq!(err.span, 10..16, "span must cover the bad field name where it sits in the whole query");

        let err = parse_query("map:ocean and outcome:banana").unwrap_err();
        match &err.kind {
            ParseErrorKind::BadValue { field, .. } => assert_eq!(*field, "outcome"),
            other => panic!("got {other:?}"),
        }
        assert_eq!(err.span, 22..28, "span must cover the bad value where it sits in the whole query");
    }
}
