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
use winnow::combinator::cut_err;
use winnow::combinator::delimited;
use winnow::combinator::opt;
use winnow::combinator::preceded;
use winnow::combinator::repeat;
use winnow::error::AddContext;
use winnow::error::ErrMode;
use winnow::error::ParserError;
use winnow::error::StrContext;
use winnow::stream::Location;
use winnow::stream::Stream;
use winnow::token::take_while;
use wows_core::game_types::AccountId;
use wows_core::game_types::GameParamId;

use super::query_ast::CmpOp;
use super::query_ast::DivisionScope;
use super::query_ast::Expr;
use super::query_ast::MatchExpr;
use super::query_ast::MatchField;
use super::query_ast::MatchTerm;
use super::query_ast::Op;
use super::query_ast::Quant;
use super::query_ast::RosterExpr;
use super::query_ast::RosterField;
use super::query_ast::RosterTerm;
use super::query_ast::ShipClass;
use super::query_ast::Value;
use super::query_ast::ValueKind;
use super::rows::MatchOutcome;
use super::rows::SourceId;
use super::rows::VehicleRelation;

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
const FALLBACK_EXPECTED: [&str; 4] = [LABEL_TERM, LABEL_GROUP, LABEL_QUANT, "not"];

const LABEL_TERM: &str = "a filter term";
const LABEL_GROUP: &str = "a parenthesised group";
const LABEL_QUANT: &str = "a quantifier";
const LABEL_ROSTER_TERM: &str = "a roster field";
const LABEL_COUNT_CMP: &str = "a comparison after count(...), such as >=3";

/// Parse a query string into a `MatchExpr`, resolving relative dates against
/// the current instant.
pub fn parse_query(input: &str) -> Result<MatchExpr, QueryParseError> {
    parse_query_at(input, Timestamp::now())
}

/// Parse with an explicit "now" for relative dates. An empty or whitespace-only
/// input is the empty conjunction, which matches everything.
///
/// A relative date resolves at parse time and prints as an absolute timestamp,
/// so a saved or shared query keeps the meaning it had when it was written.
pub fn parse_query_at(input: &str, now: Timestamp) -> Result<MatchExpr, QueryParseError> {
    if input.trim().is_empty() {
        return Ok(Expr::All(vec![]));
    }
    let _guard = NowGuard::set(now);
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

thread_local! {
    /// The "now" for the parse in progress. A thread local rather than a
    /// threaded parameter because winnow parser functions have a fixed
    /// signature.
    static NOW: std::cell::Cell<Option<Timestamp>> = const { std::cell::Cell::new(None) };
}

/// Restores `NOW` on drop, so an early return or a panic mid-parse cannot leave
/// a stale instant visible to the next parse on this thread.
struct NowGuard {
    previous: Option<Timestamp>,
}

impl NowGuard {
    fn set(now: Timestamp) -> Self {
        NowGuard { previous: NOW.with(|cell| cell.replace(Some(now))) }
    }
}

impl Drop for NowGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        NOW.with(|cell| cell.set(previous));
    }
}

/// The instant relative dates resolve against. Outside a parse there is no
/// guard, so a direct call falls back to the real clock.
fn current_now() -> Timestamp {
    NOW.with(|cell| cell.get()).unwrap_or_else(Timestamp::now)
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
        quantified.context(StrContext::Label(LABEL_QUANT)),
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

/// The operator tokens a term can be split on, longest first so `>=` is not
/// read as `>`.
///
/// `split_term` consumes this to find the operator and `quote_if_needed` to
/// decide what has to be quoted. One array rather than two, so adding an
/// operator cannot leave the printer emitting text the parser then re-splits.
const OPERATOR_TOKENS: [&str; 7] = [">=", "<=", "!=", ">", "<", "=", ":"];

/// The operators that take no right-hand operand. Spelled as words, so they are
/// matched before `OPERATOR_TOKENS` and are also what `trailing_nullary_op_len`
/// looks for at the end of a term.
const NULLARY_TOKENS: [&str; 2] = ["is-not-set", "is-set"];

/// The length of a `<space> is-set` / `<space> is-not-set` tail, which belongs
/// to the term before it rather than starting a new one.
fn trailing_nullary_op_len(rest: &str) -> Option<usize> {
    let gap = rest.len() - rest.trim_start().len();
    if gap == 0 {
        return None;
    }
    let after_gap = &rest[gap..];
    let token = NULLARY_TOKENS.into_iter().find(|token| {
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

#[derive(Clone, Copy)]
enum QuantKind {
    Any,
    None,
    Count,
}

/// `any(...)`, `none(...)`, or `count(...) <cmp> N`.
///
/// The keyword alone is not enough to commit: without an open paren after it
/// this backtracks, so `any` stays usable as a free-text word.
fn quantified(input: &mut Input<'_>) -> ModalResult<MatchExpr, QueryErr> {
    let head_start = input.current_token_start();
    let which = alt((
        keyword("any").value(QuantKind::Any),
        keyword("none").value(QuantKind::None),
        keyword("count").value(QuantKind::Count),
    ))
    .parse_next(input)?;
    (ws, '(', ws).void().parse_next(input)?;
    // Past the open paren the text can only be a quantifier body, so the rest
    // of it is a cut. Backtracking would let `term` read the keyword as free
    // text and re-read the body as a match-level group, which answers a
    // half-typed `any(tier=10` with a bogus unknown-field error instead of the
    // bracket the user has yet to close.
    let pred = cut_err(roster_or_expr).parse_next(input)?;
    (ws, cut_err(')')).void().parse_next(input)?;
    let head = head_start..input.previous_token_end();
    let quant = match which {
        QuantKind::Any => Quant::Any,
        QuantKind::None => Quant::None,
        QuantKind::Count => {
            let (op, n) = count_comparison(input, head)?;
            Quant::Count(op, n)
        }
    };
    Ok(Expr::Leaf(MatchTerm::Roster { quant, pred }))
}

/// The `<cmp> N` that must follow `count(...)`. A missing or unreadable
/// comparison is a cut, not a backtrack: `count(<roster predicate>)` cannot be
/// anything else, so retreating would only trade this message for a worse one.
/// `head` is the span of the `count(...)` that lacks its comparison.
fn count_comparison(input: &mut Input<'_>, head: Range<usize>) -> ModalResult<(CmpOp, u32), QueryErr> {
    let missing = || {
        QueryErr::diagnosed(QueryParseError::new(
            head.clone(),
            ParseErrorKind::Unexpected { expected: vec![LABEL_COUNT_CMP] },
        ))
    };
    ws.parse_next(input)?;
    let Some(op) = opt(cmp_token).parse_next(input)? else {
        return Err(missing());
    };
    ws.parse_next(input)?;
    let digits: Option<&str> = opt(take_while(1.., |c: char| c.is_ascii_digit())).parse_next(input)?;
    let Some(n) = digits.and_then(|d| d.parse::<u32>().ok()) else {
        return Err(missing());
    };
    Ok((op, n))
}

fn cmp_token(input: &mut Input<'_>) -> ModalResult<CmpOp, QueryErr> {
    alt((
        ">=".value(CmpOp::Ge),
        "<=".value(CmpOp::Le),
        "!=".value(CmpOp::Ne),
        ">".value(CmpOp::Gt),
        "<".value(CmpOp::Lt),
        "=".value(CmpOp::Eq),
    ))
    .parse_next(input)
}

/// The roster level mirrors the match level production for production, over the
/// same stream. Parsing the predicate in place rather than slicing the body out
/// and re-basing it keeps every reported span absolute.
fn roster_or_expr(input: &mut Input<'_>) -> ModalResult<RosterExpr, QueryErr> {
    let first = roster_and_expr.parse_next(input)?;
    let rest: Vec<RosterExpr> = repeat(0.., preceded((ws, keyword_or, ws), roster_and_expr)).parse_next(input)?;
    Ok(if rest.is_empty() {
        first
    } else {
        let mut cs = vec![first];
        cs.extend(rest);
        Expr::Any(cs)
    })
}

fn roster_and_expr(input: &mut Input<'_>) -> ModalResult<RosterExpr, QueryErr> {
    let first = roster_unary.parse_next(input)?;
    let rest: Vec<RosterExpr> = repeat(0.., preceded((ws, opt(keyword_and), ws), roster_unary)).parse_next(input)?;
    Ok(if rest.is_empty() {
        first
    } else {
        let mut cs = vec![first];
        cs.extend(rest);
        Expr::All(cs)
    })
}

fn roster_unary(input: &mut Input<'_>) -> ModalResult<RosterExpr, QueryErr> {
    let negated = opt(alt((keyword_not, '-'.void()))).parse_next(input)?.is_some();
    ws.parse_next(input)?;
    let inner = roster_primary.parse_next(input)?;
    Ok(if negated { Expr::Not(Box::new(inner)) } else { inner })
}

fn roster_primary(input: &mut Input<'_>) -> ModalResult<RosterExpr, QueryErr> {
    alt((
        delimited(('(', ws), roster_or_expr, (ws, ')')).context(StrContext::Label(LABEL_GROUP)),
        roster_term.context(StrContext::Label(LABEL_ROSTER_TERM)),
    ))
    .parse_next(input)
}

fn roster_term(input: &mut Input<'_>) -> ModalResult<RosterExpr, QueryErr> {
    let (raw, span) = term_text.with_span().parse_next(input)?;
    parse_roster_term_at(raw, span.start).map_err(QueryErr::diagnosed)
}

/// One roster `key op value` term starting at byte `base` within the full
/// input. Unlike a match-level term there is no bare-word fallback: a roster
/// predicate names a field or it is a mistake.
fn parse_roster_term_at(input: &str, base: usize) -> Result<RosterExpr, QueryParseError> {
    let trimmed_start = input.len() - input.trim_start().len();
    let s = input.trim();
    let base = base + trimmed_start;

    let Some((key, op_str, value_str, key_span, value_span)) = split_term(s, base) else {
        return Err(QueryParseError::new(
            base..base + s.len(),
            ParseErrorKind::Unexpected { expected: vec![LABEL_ROSTER_TERM] },
        ));
    };
    roster_term_from_parts(&key, &op_str, &value_str, key_span, value_span)
}

/// Build a roster term from already-split parts. Taking the parts rather than a
/// string lets the scope sugar reuse this without synthesizing a source string
/// whose offsets would not line up with the real input.
fn roster_term_from_parts(
    key: &str,
    op_str: &str,
    value_str: &str,
    key_span: Range<usize>,
    value_span: Range<usize>,
) -> Result<RosterExpr, QueryParseError> {
    let Some(field) = RosterField::from_name(key) else {
        return Err(QueryParseError::new(
            key_span,
            ParseErrorKind::UnknownField { name: key.to_string(), suggestion: closest_roster_field(key) },
        ));
    };

    let op =
        op_from_token(op_str, field.value_kind()).filter(|o| field.allowed_ops().contains(o)).ok_or_else(|| {
            QueryParseError::new(
                key_span,
                ParseErrorKind::BadOperator {
                    field: field.name(),
                    op: op_str.to_string(),
                    allowed: field.allowed_ops().iter().map(|o| o.as_token()).collect(),
                },
            )
        })?;

    if op.is_nullary() {
        return Ok(Expr::Leaf(RosterTerm { field, op, value: Value::NoOperand }));
    }

    let value = parse_roster_value(field.value_kind(), value_str).ok_or_else(|| {
        QueryParseError::new(
            value_span,
            ParseErrorKind::BadValue { field: field.name(), allowed: enumerable_roster_values(field.value_kind()) },
        )
    })?;

    Ok(Expr::Leaf(RosterTerm { field, op, value }))
}

/// The roster-only value kinds, falling through to `parse_value` for the kinds
/// both levels share.
fn parse_roster_value(kind: ValueKind, raw: &str) -> Option<Value> {
    let s = unquote(raw);
    match kind {
        ValueKind::Relation => match s.to_ascii_lowercase().as_str() {
            "self" | "me" => Some(Value::Relation(VehicleRelation::SelfPlayer)),
            "ally" | "friendly" => Some(Value::Relation(VehicleRelation::Ally)),
            "enemy" => Some(Value::Relation(VehicleRelation::Enemy)),
            _ => None,
        },
        ValueKind::Division => DivisionScope::from_token(&s).map(Value::Division),
        ValueKind::Class => ShipClass::from_token(&s).map(Value::Class),
        ValueKind::Account => s.parse::<i64>().ok().map(|n| Value::Account(AccountId(n))),
        ValueKind::Ship => s.parse::<u64>().ok().map(|n| Value::Ship(GameParamId::from(n))),
        other => parse_value(other, raw),
    }
}

fn enumerable_roster_values(kind: ValueKind) -> Option<Vec<String>> {
    match kind {
        ValueKind::Relation => Some(vec!["self".into(), "ally".into(), "enemy".into()]),
        ValueKind::Division => Some(DivisionScope::ALL.iter().map(|d| d.as_token().to_string()).collect()),
        ValueKind::Class => Some(ShipClass::ALL.iter().map(|c| c.as_db_str().to_ascii_lowercase()).collect()),
        other => enumerable_values(other),
    }
}

fn closest_roster_field(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    RosterField::ALL
        .iter()
        .flat_map(|f| std::iter::once(f.name()).chain(f.aliases().iter().copied()))
        .find(|candidate| within_one_edit(&lower, candidate))
}

/// `<scope>.<field>` sugar: the scope's roster constraint, if it has one, and
/// the field name left over. `None` when the prefix names no known scope, which
/// leaves the key to be read as an ordinary match field.
fn scope_prefix(key: &str) -> Option<(Option<RosterTerm>, &str)> {
    let relation = |r| Some(RosterTerm { field: RosterField::Relation, op: Op::Is, value: Value::Relation(r) });
    let (scope, rest) = key.split_once('.')?;
    let constraint = match scope.to_ascii_lowercase().as_str() {
        "self" | "me" => relation(VehicleRelation::SelfPlayer),
        "ally" => relation(VehicleRelation::Ally),
        "enemy" => relation(VehicleRelation::Enemy),
        "div" | "division" => {
            Some(RosterTerm { field: RosterField::Division, op: Op::Is, value: Value::Division(DivisionScope::Mine) })
        }
        "anyone" | "any" => None,
        _ => return None,
    };
    Some((constraint, rest))
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

    if let Some((constraint, field_name)) = scope_prefix(&key) {
        // `split_term` ran on the real term at its real offset, so the value
        // span is already absolute. Only the key span needs adjusting, past the
        // scope prefix and its dot, to underline the field name alone. A
        // sugared term then reports exactly what the general form would.
        let field_span = key_span.start + (key.len() - field_name.len())..key_span.end;
        let inner = roster_term_from_parts(field_name, &op_str, &value_str, field_span, value_span)?;
        // Expanding to the same shape the general form parses to keeps the
        // sugar a pure abbreviation, which is what lets Task 9 print one form.
        let pred = match constraint {
            Some(c) => Expr::All(vec![Expr::Leaf(c), inner]),
            None => inner,
        };
        return Ok(Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred }));
    }

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
    for token in NULLARY_TOKENS {
        if let Some(idx) = find_unquoted(s, token) {
            let key = s[..idx].trim().to_string();
            let key_span = base..base + key.len();
            return Some((key, token.to_string(), String::new(), key_span, base..base + s.len()));
        }
    }
    for token in OPERATOR_TOKENS {
        if let Some(idx) = find_unquoted(s, token) {
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

/// Where `token` starts in `s`, skipping any occurrence inside a quoted run.
///
/// A quoted run is opaque, the same way `word_end` treats it, so `map:"a>b"` is
/// a `map` term whose value happens to contain a `>` rather than a term keyed on
/// the nonsense `map:"a`. This is what makes quoting a value sufficient to get
/// it back unchanged, which the printer relies on.
fn find_unquoted(s: &str, token: &str) -> Option<usize> {
    let mut in_quotes = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            in_quotes = !in_quotes;
            continue;
        }
        if !in_quotes && s[i..].starts_with(token) {
            return Some(i);
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
    // Nothing after the operator means the user has typed no value yet. `""` is
    // an explicit empty string, which a text chip can hold and which therefore
    // has to read back; every other kind rejects it below anyway.
    if s.is_empty() && raw.trim() != "\"\"" {
        return None;
    }
    match kind {
        ValueKind::Text => Some(Value::Text(s)),
        ValueKind::Int => parse_int(&s).map(Value::Int),
        // NaN is never equal to itself, so a NaN value would break
        // `parse(print(x)) == x`. Infinity compares fine and stays accepted.
        ValueKind::Float => s.parse::<f64>().ok().filter(|f| !f.is_nan()).map(Value::Float),
        ValueKind::Bool => match s.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Some(Value::Bool(true)),
            "false" | "no" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        ValueKind::Outcome => MatchOutcome::from_db_str(&s.to_ascii_lowercase()).map(Value::Outcome),
        ValueKind::Timestamp => parse_date(&s).or_else(|| parse_relative(&s)).map(Value::Timestamp),
        // A source is picked in the widget rather than typed, but the printed
        // form is the persistence format, so the id it prints has to read back.
        ValueKind::Source => s.parse::<i64>().ok().map(|n| Value::Source(SourceId(n))),
        // Relation, Division, Class, Ship, and Account are roster-only kinds,
        // handled by `parse_roster_value`.
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

/// `YYYY-MM-DD`, interpreted as midnight UTC, or the full RFC 3339 instant that
/// `print_timestamp` writes when a timestamp does not land on midnight.
fn parse_date(s: &str) -> Option<Timestamp> {
    if let Ok(date) = s.parse::<jiff::civil::Date>() {
        return date.to_zoned(jiff::tz::TimeZone::UTC).ok().map(|z| z.timestamp());
    }
    s.parse::<Timestamp>().ok()
}

/// A negative offset from the parse-time "now": `-30d`, `-6h`, `-1y`.
///
/// The unit is taken as the last `char`, not the last byte, so a multi-byte
/// tail is rejected rather than splitting mid-character.
fn parse_relative(s: &str) -> Option<Timestamp> {
    let body = s.strip_prefix('-')?;
    let (unit_at, unit) = body.char_indices().next_back()?;
    let digits = body.get(..unit_at)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: i64 = digits.parse().ok()?;
    let seconds: i64 = match unit {
        'h' => 3_600,
        'd' => 86_400,
        'w' => 7 * 86_400,
        'y' => 365 * 86_400,
        _ => return None,
    };
    let ago = n.checked_mul(seconds)?;
    Timestamp::from_second(current_now().as_second().checked_sub(ago)?).ok()
}

/// Strip a surrounding pair of quotes and undo the doubling `quote_if_needed`
/// applies to an interior one. Doubling rather than a backslash escape because
/// `word_end` counts quotes to find the end of a term: a doubled pair toggles
/// off and straight back on, so whitespace after it stays inside the run.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        return s[1..s.len() - 1].replace("\"\"", "\"");
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

/// Render an expression as query text. The output is both the persistence
/// format and the share format, so it must reparse to an identical tree; see
/// `every_supported_shape_round_trips`.
pub fn print_query(expr: &MatchExpr) -> String {
    if expr.is_empty_all() {
        return String::new();
    }
    print_expr(expr, Prec::Top, &print_match_term)
}

/// Where an expression sits relative to its parent, so parentheses are added
/// only where dropping them would change the tree.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prec {
    Top,
    InOr,
    InAnd,
    InNot,
}

/// Render a boolean tree, delegating leaves to `leaf`. Shared by the match and
/// the roster level because the two differ only in what a leaf is.
///
/// A nested `All` inside an `All` (and likewise for `Any`) keeps its brackets:
/// the parser builds those levels flat, so printing `a and (b and c)` without
/// them would read back as a single three-child `All`.
fn print_expr<L>(expr: &Expr<L>, prec: Prec, leaf: &dyn Fn(&L) -> String) -> String {
    match expr {
        // The grammar has no spelling for a tree that constrains nothing except
        // at the top level, which `print_query` returns early for, and none at
        // all for one that matches nothing. Neither shape is reachable from a
        // parse, so these two are a placeholder rather than a round trip.
        Expr::All(cs) if cs.is_empty() => "()".to_string(),
        Expr::Any(cs) if cs.is_empty() => "not ()".to_string(),
        Expr::All(cs) => {
            let body = cs.iter().map(|c| print_expr(c, Prec::InAnd, leaf)).collect::<Vec<_>>().join(" and ");
            parenthesise(body, matches!(prec, Prec::InAnd | Prec::InNot))
        }
        Expr::Any(cs) => {
            let body = cs.iter().map(|c| print_expr(c, Prec::InOr, leaf)).collect::<Vec<_>>().join(" or ");
            parenthesise(body, !matches!(prec, Prec::Top))
        }
        Expr::Not(inner) => {
            let body = format!("not {}", print_expr(inner, Prec::InNot, leaf));
            // `not not x` is not a sentence the grammar accepts: `term_text`
            // refuses a bare keyword, so the inner `not` needs brackets.
            parenthesise(body, matches!(prec, Prec::InNot))
        }
        Expr::Leaf(l) => leaf(l),
    }
}

fn parenthesise(body: String, needed: bool) -> String {
    if needed { format!("({body})") } else { body }
}

fn print_match_term(term: &MatchTerm) -> String {
    match term {
        MatchTerm::FreeText(s) => quote_if_needed(s),
        MatchTerm::Field(field, op, value) => print_field(field.name(), *op, value),
        MatchTerm::Roster { quant, pred } => print_roster(*quant, pred),
    }
}

fn print_roster_term(term: &RosterTerm) -> String {
    print_field(term.field.name(), term.op, &term.value)
}

fn print_field(name: &str, op: Op, value: &Value) -> String {
    if op.is_nullary() {
        // The space is what `trailing_nullary_op_len` reads the tail back by.
        return format!("{name} {}", op.as_token());
    }
    format!("{name}{}{}", op.as_token(), print_value(value))
}

fn print_roster(quant: Quant, pred: &RosterExpr) -> String {
    if let Some(sugar) = try_print_sugar(quant, pred) {
        return sugar;
    }
    let body = print_expr(pred, Prec::Top, &print_roster_term);
    match quant {
        Quant::Any => format!("any({body})"),
        Quant::None => format!("none({body})"),
        Quant::Count(op, n) => format!("count({body}){}{n}", cmp_token_str(op)),
    }
}

/// Recognise the shapes `scope_prefix` expands to and print them back as sugar,
/// so `self.damage>=100000` does not round trip into
/// `any(relation=self and damage>=100000)`.
fn try_print_sugar(quant: Quant, pred: &RosterExpr) -> Option<String> {
    if quant != Quant::Any {
        return None;
    }
    let (scope, inner) = match pred {
        Expr::All(cs) if cs.len() == 2 => {
            let Expr::Leaf(first) = &cs[0] else { return None };
            let Expr::Leaf(inner) = &cs[1] else { return None };
            let scope = match (first.field, &first.value, first.op) {
                (RosterField::Relation, Value::Relation(VehicleRelation::SelfPlayer), Op::Is) => "self",
                (RosterField::Relation, Value::Relation(VehicleRelation::Ally), Op::Is) => "ally",
                (RosterField::Relation, Value::Relation(VehicleRelation::Enemy), Op::Is) => "enemy",
                (RosterField::Division, Value::Division(DivisionScope::Mine), Op::Is) => "div",
                _ => return None,
            };
            (scope, inner)
        }
        Expr::Leaf(inner) => ("anyone", inner),
        _ => return None,
    };
    Some(format!("{scope}.{}", print_roster_term(inner)))
}

fn cmp_token_str(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "=",
        CmpOp::Ne => "!=",
        CmpOp::Gt => ">",
        CmpOp::Ge => ">=",
        CmpOp::Lt => "<",
        CmpOp::Le => "<=",
    }
}

fn print_value(value: &Value) -> String {
    match value {
        Value::Text(s) => quote_if_needed(s),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Outcome(o) => o.as_db_str().to_string(),
        Value::Relation(r) => r.as_db_str().to_string(),
        Value::Division(d) => d.as_token().to_string(),
        Value::Class(c) => c.as_db_str().to_ascii_lowercase(),
        Value::Ship(s) => s.raw().to_string(),
        Value::Account(a) => a.raw().to_string(),
        Value::Source(s) => s.0.to_string(),
        Value::Timestamp(t) => print_timestamp(*t),
        Value::NoOperand => String::new(),
    }
}

/// A timestamp prints as a bare date when it lands exactly on midnight UTC,
/// which is both what `date>=2026-01-01` means and what a person would type.
/// A relative date resolves to whatever second the parse happened at, so
/// anything else prints the full RFC 3339 instant; `parse_date` reads both.
fn print_timestamp(t: Timestamp) -> String {
    let zoned = t.to_zoned(jiff::tz::TimeZone::UTC);
    if zoned.time() == jiff::civil::Time::midnight() { zoned.date().to_string() } else { t.to_string() }
}

/// Quote a value or bare word the parser would not otherwise read back
/// unchanged.
///
/// Every condition is read from the parser rather than restated:
/// `is_term_boundary` ends a term at whitespace, `(`, `)`, and `|`;
/// `split_term` splits on `OPERATOR_TOKENS` and `NULLARY_TOKENS`; `unary` reads
/// a leading `-` as `not`; `term_text` refuses a bare `and`, `or`, or `not`;
/// and an empty word has no bare spelling at all. Quoting answers all of them
/// because `word_end` and `find_unquoted` both treat a quoted run as opaque.
fn quote_if_needed(s: &str) -> String {
    let boundary = s.chars().any(|c| is_term_boundary(c) || c == '"');
    let operator = OPERATOR_TOKENS.iter().chain(NULLARY_TOKENS.iter()).any(|token| s.contains(token));
    let keyword = matches!(s.to_ascii_lowercase().as_str(), "and" | "or" | "not");
    if s.is_empty() || boundary || operator || keyword || s.starts_with('-') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::query_ast::CmpOp;
    use crate::index::query_ast::DivisionScope;
    use crate::index::query_ast::MatchField;
    use crate::index::query_ast::MatchTerm;
    use crate::index::query_ast::Op;
    use crate::index::query_ast::Quant;
    use crate::index::query_ast::RosterField;
    use crate::index::query_ast::RosterTerm;
    use crate::index::query_ast::ShipClass;
    use crate::index::query_ast::Value;
    use crate::index::rows::MatchOutcome;
    use crate::index::rows::VehicleRelation;

    fn one(input: &str) -> MatchTerm {
        match parse_query(input).unwrap() {
            Expr::Leaf(t) => t,
            other => panic!("expected a single leaf, got {other:?}"),
        }
    }

    /// Parse against a fixed instant, so a relative date resolves the same way
    /// on every run.
    fn at(input: &str) -> MatchExpr {
        let now = Timestamp::from_second(1_800_000_000).unwrap();
        parse_query_at(input, now).unwrap()
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
        // Every `alt` branch in `primary` backtracks here, so every label must
        // survive rather than the later one replacing the earlier.
        let err = parse_query("and").unwrap_err();
        match &err.kind {
            ParseErrorKind::Unexpected { expected } => {
                assert_eq!(*expected, vec!["a parenthesised group", "a quantifier", "a filter term"]);
            }
            other => panic!("got {other:?}"),
        }

        // Dying at end-of-input carries no label, so the fallback set is used.
        let err = parse_query("outcome:win and").unwrap_err();
        match &err.kind {
            ParseErrorKind::Unexpected { expected } => {
                assert_eq!(*expected, vec!["a filter term", "a parenthesised group", "a quantifier", "not"]);
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

    #[test]
    fn parses_an_explicit_any_quantifier() {
        match at("any(tier=10)") {
            Expr::Leaf(MatchTerm::Roster { quant, pred }) => {
                assert_eq!(quant, Quant::Any);
                assert_eq!(
                    pred,
                    Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) })
                );
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_none_and_count_quantifiers() {
        assert!(matches!(at("none(class:cv)"), Expr::Leaf(MatchTerm::Roster { quant: Quant::None, .. })));
        match at("count(relation:enemy and damage>100k)>=3") {
            Expr::Leaf(MatchTerm::Roster { quant: Quant::Count(op, n), .. }) => {
                assert_eq!(op, CmpOp::Ge);
                assert_eq!(n, 3);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn roster_predicates_nest_with_booleans() {
        match at("any(relation:enemy and (tier=9 or tier=10))") {
            Expr::Leaf(MatchTerm::Roster { pred: Expr::All(cs), .. }) => {
                assert_eq!(cs.len(), 2);
                assert!(matches!(cs[1], Expr::Any(ref inner) if inner.len() == 2), "got {:?}", cs[1]);
            }
            other => panic!("got {other:?}"),
        }
    }

    /// A quantifier keyword only starts a quantifier when a predicate body
    /// follows it, so the words themselves stay usable as free text.
    #[test]
    fn a_quantifier_keyword_without_a_body_is_free_text() {
        assert_eq!(one("any"), MatchTerm::FreeText("any".into()));
        assert_eq!(one("count"), MatchTerm::FreeText("count".into()));
    }

    #[test]
    fn scope_sugar_expands_to_a_single_term_any() {
        assert_eq!(
            at("self.dmg>=100k"),
            Expr::Leaf(MatchTerm::Roster {
                quant: Quant::Any,
                pred: Expr::All(vec![
                    Expr::Leaf(RosterTerm {
                        field: RosterField::Relation,
                        op: Op::Is,
                        value: Value::Relation(VehicleRelation::SelfPlayer),
                    }),
                    Expr::Leaf(RosterTerm { field: RosterField::Damage, op: Op::Ge, value: Value::Int(100_000) }),
                ]),
            })
        );
    }

    /// The sugar has to be exactly an abbreviation. Task 9's printer prints the
    /// general form, so any divergence here breaks `parse(print(x)) == x`.
    #[test]
    fn scope_sugar_is_identical_to_the_general_form() {
        assert_eq!(at("self.dmg>=100k"), at("any(relation:self and damage>=100000)"));
        assert_eq!(at("enemy.class:dd"), at("any(relation:enemy and class:dd)"));
        assert_eq!(at("anyone.pr<800"), at("any(pr<800)"));
    }

    #[test]
    fn every_scope_prefix_resolves() {
        for (prefix, expected) in
            [("self", VehicleRelation::SelfPlayer), ("ally", VehicleRelation::Ally), ("enemy", VehicleRelation::Enemy)]
        {
            match at(&format!("{prefix}.tier=10")) {
                Expr::Leaf(MatchTerm::Roster { pred: Expr::All(cs), .. }) => {
                    assert_eq!(
                        cs[0],
                        Expr::Leaf(RosterTerm {
                            field: RosterField::Relation,
                            op: Op::Is,
                            value: Value::Relation(expected),
                        })
                    );
                }
                other => panic!("{prefix} gave {other:?}"),
            }
        }
    }

    #[test]
    fn div_scope_expands_to_division_mine() {
        match at("div.test-ship:true") {
            Expr::Leaf(MatchTerm::Roster { pred: Expr::All(cs), .. }) => {
                assert_eq!(
                    cs[0],
                    Expr::Leaf(RosterTerm {
                        field: RosterField::Division,
                        op: Op::Is,
                        value: Value::Division(DivisionScope::Mine),
                    })
                );
                assert_eq!(
                    cs[1],
                    Expr::Leaf(RosterTerm { field: RosterField::TestShip, op: Op::Is, value: Value::Bool(true) })
                );
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn anyone_scope_adds_no_relation_constraint() {
        match at("anyone.pr<800") {
            Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred }) => {
                assert_eq!(
                    pred,
                    Expr::Leaf(RosterTerm { field: RosterField::Pr, op: Op::Lt, value: Value::Float(800.0) })
                );
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn class_values_accept_abbreviations() {
        match at("any(class:cv)") {
            Expr::Leaf(MatchTerm::Roster { pred: Expr::Leaf(t), .. }) => {
                assert_eq!(t.value, Value::Class(ShipClass::AirCarrier));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn relative_dates_resolve_against_the_supplied_now() {
        let now = Timestamp::from_second(1_800_000_000).unwrap();
        match parse_query_at("date>=-30d", now).unwrap() {
            Expr::Leaf(MatchTerm::Field(MatchField::Date, Op::Ge, Value::Timestamp(t))) => {
                assert_eq!(t.as_second(), 1_800_000_000 - 30 * 86_400);
            }
            other => panic!("got {other:?}"),
        }
        match parse_query_at("date>=-6h", now).unwrap() {
            Expr::Leaf(MatchTerm::Field(_, _, Value::Timestamp(t))) => {
                assert_eq!(t.as_second(), 1_800_000_000 - 6 * 3_600);
            }
            other => panic!("got {other:?}"),
        }
    }

    /// The relative-date suffix is one character, so splitting the digits off
    /// must not assume the last byte is the last character.
    #[test]
    fn a_relative_date_with_a_multi_byte_suffix_is_rejected_not_a_panic() {
        assert!(parse_query_at("date>=-30\u{e9}", Timestamp::from_second(1_800_000_000).unwrap()).is_err());
        assert!(parse_query_at("date>=-\u{e9}", Timestamp::from_second(1_800_000_000).unwrap()).is_err());
    }

    /// A huge offset must not overflow the seconds arithmetic.
    #[test]
    fn an_absurd_relative_date_is_rejected() {
        let now = Timestamp::from_second(1_800_000_000).unwrap();
        assert!(parse_query_at("date>=-99999999999999999999d", now).is_err());
        assert!(parse_query_at("date>=-9000000000000d", now).is_err());
    }

    #[test]
    fn the_parse_time_now_is_cleared_when_the_parse_ends() {
        let long_ago = Timestamp::from_second(1_000_000_000).unwrap();
        assert!(parse_query_at("date>=-1d", long_ago).is_ok());
        assert!(NOW.with(|cell| cell.get()).is_none(), "the guard must clear the thread local on success");
        assert!(parse_query_at("date>=-1d outcom:win", long_ago).is_err());
        assert!(NOW.with(|cell| cell.get()).is_none(), "the guard must clear it on the error path too");
    }

    #[test]
    fn the_bare_entry_point_resolves_relative_dates_against_the_real_clock() {
        match parse_query("date>=-1d").unwrap() {
            Expr::Leaf(MatchTerm::Field(_, _, Value::Timestamp(t))) => {
                let expected = Timestamp::now().as_second() - 86_400;
                assert!((t.as_second() - expected).abs() < 60, "got {t}, expected about {expected}");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_roster_field_reports_its_span_inside_the_quantifier() {
        let err = parse_query("any(daamage>100)").unwrap_err();
        match &err.kind {
            ParseErrorKind::UnknownField { name, .. } => assert_eq!(name, "daamage"),
            other => panic!("got {other:?}"),
        }
        assert_eq!(err.span, 4..11, "span must be absolute, not relative to the quantifier body");
    }

    /// A sugared term must underline exactly what the general form underlines:
    /// the field name alone, or the value alone. The scope prefix shifts the
    /// field name along, and nothing shifts the value.
    #[test]
    fn a_scope_sugar_error_underlines_the_field_or_the_value_alone() {
        let err = parse_query("map:ocean self.daamage>1").unwrap_err();
        match &err.kind {
            ParseErrorKind::UnknownField { name, suggestion } => {
                assert_eq!(name, "daamage");
                assert_eq!(*suggestion, Some("damage"));
            }
            other => panic!("got {other:?}"),
        }
        assert_eq!(err.span, 15..22, "span must cover the field name, past the scope prefix");

        let err = parse_query("enemy.class:banana").unwrap_err();
        match &err.kind {
            ParseErrorKind::BadValue { field, allowed } => {
                assert_eq!(*field, "class");
                let allowed = allowed.as_ref().expect("class is an enum, so its values are enumerable");
                assert!(allowed.iter().any(|a| a == "destroyer"), "got {allowed:?}");
            }
            other => panic!("got {other:?}"),
        }
        assert_eq!(err.span, 12..18, "span must cover the bad value alone");

        // The operator error reports the key, which for a sugared term is the
        // field name rather than the whole `scope.field`.
        let err = parse_query("self.tier is-set").unwrap_err();
        match &err.kind {
            ParseErrorKind::BadOperator { field, .. } => assert_eq!(*field, "tier"),
            other => panic!("got {other:?}"),
        }
        assert_eq!(err.span, 5..9);
    }

    /// A quantifier the user is halfway through typing must report the bracket
    /// it is missing, not a bogus unknown-field error from the body being
    /// re-read as a match-level group.
    #[test]
    fn a_half_typed_quantifier_reports_the_unclosed_bracket() {
        for (input, at) in [("any(", 3), ("any(tier=10", 3), ("none(class:cv", 4), ("count(tier=10", 5)] {
            let err = parse_query(input).unwrap_err();
            assert!(matches!(err.kind, ParseErrorKind::Unbalanced), "{input:?} gave {:?}", err.kind);
            assert_eq!(err.span, at..at + 1, "{input:?} must point at the unclosed paren");
        }
        // A closed body still diagnoses what is wrong inside it.
        let err = parse_query("any(daamage>100)").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::UnknownField { .. }), "got {:?}", err.kind);
    }

    /// `count(<roster predicate>)` is unambiguously a quantifier, so a missing
    /// comparison must say so rather than backtracking into a reading where
    /// `count` is free text and the body is a match-level group.
    #[test]
    fn count_without_a_comparison_is_an_error() {
        for input in ["count(tier=10)", "count(tier=10)>=", "count(tier=10)>=x"] {
            let err = parse_query(input).unwrap_err();
            match &err.kind {
                ParseErrorKind::Unexpected { expected } => {
                    assert_eq!(*expected, vec!["a comparison after count(...), such as >=3"], "{input:?}");
                }
                other => panic!("{input:?} gave {other:?}"),
            }
            assert_eq!(err.span, 0..14, "{input:?} must underline the count(...) that lacks its comparison");
        }
    }

    fn fixed_now() -> Timestamp {
        Timestamp::from_second(1_800_000_000).unwrap()
    }

    fn round_trip(src: &str) {
        let parsed = parse_query_at(src, fixed_now()).unwrap_or_else(|e| panic!("{src:?} did not parse: {e}"));
        let printed = print_query(&parsed);
        let reparsed = parse_query_at(&printed, fixed_now())
            .unwrap_or_else(|e| panic!("printed form did not reparse\n  src: {src}\n  printed: {printed}\n  err: {e}"));
        assert_eq!(parsed, reparsed, "round trip changed the tree\n  src: {src}\n  printed: {printed}");
    }

    /// The other half of the property. The widget builds trees the parser has no
    /// way to produce, so those have to survive printing too.
    fn round_trip_tree(expr: MatchExpr) {
        let printed = print_query(&expr);
        let reparsed = parse_query_at(&printed, fixed_now())
            .unwrap_or_else(|e| panic!("printed form did not reparse\n  printed: {printed}\n  err: {e}"));
        assert_eq!(expr, reparsed, "round trip changed the tree\n  printed: {printed}");
    }

    #[test]
    fn every_supported_shape_round_trips() {
        for src in [
            "",
            "outcome:win",
            "map:ocean",
            "map:\"new dawn\"",
            "map:th\u{e9}",
            "build>=1234",
            "build>=-5",
            "build is-set",
            "build is-not-set",
            "date>=2026-01-01",
            "date>=-30d",
            "group=5",
            "results-available:true",
            "yamato",
            "outcome:win map:ocean",
            "outcome:win and map:ocean",
            "outcome:win or map:ocean",
            "outcome:win and (map:ocean or map:north)",
            "outcome:win and (map:ocean and map:north)",
            "outcome:win or (map:ocean or map:north)",
            "(outcome:win or map:ocean) and (build>1000 or build<500)",
            "not outcome:win",
            "not (map:ocean or map:north)",
            "not (not outcome:win)",
            "any(tier=10)",
            "none(class:cv)",
            "none(division:none)",
            "any(not class:cv)",
            "any(damage is-set)",
            "any(account=12345)",
            "any(ship=4288851920)",
            "count(relation:enemy and damage>100000)>=3",
            "count(tier=10)!=0",
            "any(relation:enemy and (tier=9 or tier=10))",
            "none(relation:enemy and (tier=9 or tier=10))",
            "any(division:mine and test-ship:true)",
            "any(tier=10 and kills>=3)",
            "self.damage>=100000",
            "self.damage is-set",
            "ally.tier=10",
            "enemy.tier=10",
            "div.test-ship:true",
            "anyone.pr<800",
            "anyone.pr>=-1.5",
            "anyone.survived:no",
            "anyone.name:\"new player\"",
            "outcome:win and any(division:mine and test-ship:true) and not none(class:cv)",
        ] {
            round_trip(src);
        }
    }

    /// Every character the grammar gives a meaning to has to survive inside a
    /// value or a bare word, which is what the quoting rule is for.
    #[test]
    fn metacharacters_round_trip_inside_a_value_and_a_bare_word() {
        for src in [
            "map:\"a b\"",
            "map:\"a|b\"",
            "map:\"a(b\"",
            "map:\"a)b\"",
            "map:\"a:b\"",
            "map:\"a=b\"",
            "map:\"a>b\"",
            "map:\"a<b\"",
            "map:\"a!b\"",
            "map:\"this-set\"",
            "map:\"and\"",
            "map:\"\"",
            // A quote can be typed bare in the middle of a value, and the run it
            // opens then swallows the following space. Both shapes have to come
            // back as one leaf, not two.
            "map:a\"b c\"",
            "map:\"a\"\" b\"",
            "\"a b\"",
            "\"a|b\"",
            "\"a:b\"",
            "\"a=b\"",
            "\"a>b\"",
            "\"this-set\"",
            "\"and\"",
            "\"or\"",
            "\"not\"",
            "\"-foo\"",
            "\"\"",
            "a\"b c\"",
        ] {
            round_trip(src);
        }
    }

    /// The quoting rule reads the parser's own token arrays, so an operator
    /// added to either one is covered here without anybody remembering to.
    #[test]
    fn every_operator_token_survives_inside_a_value_and_a_bare_word() {
        for token in OPERATOR_TOKENS.into_iter().chain(NULLARY_TOKENS) {
            let text = format!("a{token}b");
            round_trip_tree(Expr::Leaf(MatchTerm::FreeText(text.clone())));
            round_trip_tree(Expr::Leaf(MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text(text))));
        }
    }

    /// A NaN is not equal to itself, so letting one into the tree would break
    /// the round-trip property from text a user can type.
    #[test]
    fn a_not_a_number_float_is_refused_but_infinity_is_not() {
        for input in ["anyone.pr<nan", "anyone.pr<NaN", "any(pr>=nan)"] {
            let err = parse_query(input).unwrap_err();
            assert!(matches!(err.kind, ParseErrorKind::BadValue { .. }), "{input:?} gave {:?}", err.kind);
        }
        round_trip("anyone.pr<inf");
    }

    /// A tree the parser refuses to build from text: the widget can still make
    /// one, and the printer is what has to make it representable again.
    #[test]
    fn a_programmatic_tree_with_awkward_text_round_trips() {
        let awkward = [
            "and", "or", "not", "-foo", "", "a b", "a|b", "a:b", "a=b", "a!b", "is-set", "this-set", "(x)", "a\"b",
            "a\" b", "\"", "\"\"", "\" \"", "a\"\"b", " ", "--", "not ", "a\tb",
        ];
        for text in awkward {
            round_trip_tree(Expr::Leaf(MatchTerm::FreeText(text.into())));
            round_trip_tree(Expr::Leaf(MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text(text.into()))));
        }
    }

    /// A quoted run is opaque, so an operator inside it belongs to the value
    /// rather than splitting the term.
    #[test]
    fn an_operator_inside_quotes_belongs_to_the_value() {
        assert_eq!(one("map:\"a>b\""), MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("a>b".into())));
        assert_eq!(one("map:\"a=b\""), MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("a=b".into())));
        assert_eq!(
            one("map:\"this-set\""),
            MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("this-set".into()))
        );
        // The operator that does the splitting is still found outside the quotes.
        assert_eq!(one("map=\"a>b\""), MatchTerm::Field(MatchField::Map, Op::Equals, Value::Text("a>b".into())));
    }

    /// A quote is doubled rather than backslash-escaped, because `word_end`
    /// finds the end of a term by counting quotes: a doubled pair toggles off
    /// and straight back on, so whitespace after it stays inside the run.
    #[test]
    fn a_doubled_quote_is_one_literal_quote() {
        let text = |s: &str| MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text(s.into()));
        assert_eq!(one("map:\"a\"\" b\""), text("a\" b"));
        assert_eq!(one("map:\"\"\"\""), text("\""));
        assert_eq!(one("map:\"a\"\"b\""), text("a\"b"));
        // An explicit empty string is a value a text chip can hold, so it has to
        // read back. An operator with nothing after it is still an error.
        assert_eq!(one("map:\"\""), text(""));
        assert!(parse_query("map:").is_err());
        assert!(parse_query("build>=").is_err());
    }

    #[test]
    fn scope_sugar_is_printed_back_as_sugar() {
        let parsed = parse_query_at("self.damage>=100000", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "self.damage>=100000");
        let parsed = parse_query_at("any(relation:self and damage>=100000)", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "self.damage>=100000");
        // Every scope has to be pinned by an exact string. Losing an arm still
        // round trips through the general form, so the corpus cannot see it.
        let parsed = parse_query_at("ally.tier=10", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "ally.tier=10");
        let parsed = parse_query_at("any(relation:ally and kills>=2)", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "ally.kills>=2");
        let parsed = parse_query_at("enemy.tier=10", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "enemy.tier=10");
        let parsed = parse_query_at("any(relation:enemy and damage>100000)", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "enemy.damage>100000");
        let parsed = parse_query_at("div.test-ship:true", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "div.test-ship=true");
        let parsed = parse_query_at("anyone.pr<800", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "anyone.pr<800");
        let parsed = parse_query_at("self.damage is-set", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "self.damage is-set");
    }

    #[test]
    fn a_general_roster_form_that_is_not_sugar_prints_in_full() {
        let parsed = parse_query_at("any(tier=10 and kills>=3)", fixed_now()).unwrap();
        let printed = print_query(&parsed);
        assert_eq!(printed, "any(tier=10 and kills>=3)");
        round_trip(&printed);

        let parsed = parse_query_at("count(relation:enemy and damage>100k)>=3", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "count(relation=enemy and damage>100000)>=3");

        let parsed = parse_query_at("none(class:cv)", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "none(class=aircarrier)");
    }

    /// Precedence is only half of it: a nested group of the same operator has to
    /// keep its parentheses, or the tree flattens and stops matching.
    #[test]
    fn nested_groups_keep_the_parentheses_that_preserve_the_tree() {
        for (src, expected) in [
            ("outcome:win and (map:ocean and map:north)", "outcome=win and (map:ocean and map:north)"),
            ("outcome:win or (map:ocean or map:north)", "outcome=win or (map:ocean or map:north)"),
            ("outcome:win and (map:ocean or map:north)", "outcome=win and (map:ocean or map:north)"),
            ("outcome:win or map:ocean and map:north", "outcome=win or map:ocean and map:north"),
            ("not (map:ocean and map:north)", "not (map:ocean and map:north)"),
            ("not (not outcome:win)", "not (not outcome=win)"),
        ] {
            assert_eq!(print_query(&parse_query_at(src, fixed_now()).unwrap()), expected, "{src:?}");
        }
    }

    #[test]
    fn a_relative_date_prints_as_an_absolute_one() {
        let parsed = parse_query_at("date>=-30d", fixed_now()).unwrap();
        let printed = print_query(&parsed);
        assert!(!printed.contains("-30d"), "relative dates must not survive printing: {printed}");
        // Reparsing with a different "now" must give the same tree.
        let later = Timestamp::from_second(1_900_000_000).unwrap();
        assert_eq!(parse_query_at(&printed, later).unwrap(), parsed);
    }

    /// A relative date lands on whatever second the parse happened at, so the
    /// bare `YYYY-MM-DD` form would silently truncate it.
    #[test]
    fn a_timestamp_off_midnight_prints_its_time_of_day() {
        let parsed = parse_query_at("date>=-6h", fixed_now()).unwrap();
        let printed = print_query(&parsed);
        assert!(printed.ends_with("T02:00:00Z"), "got {printed}");
        assert_eq!(print_query(&parse_query_at("date>=2026-01-01", fixed_now()).unwrap()), "date>=2026-01-01");
    }

    #[test]
    fn values_needing_quotes_are_quoted() {
        let parsed = parse_query_at("map:\"new dawn\"", fixed_now()).unwrap();
        assert_eq!(print_query(&parsed), "map:\"new dawn\"");
    }
}
