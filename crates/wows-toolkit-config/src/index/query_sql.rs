//! Compiles a `MatchExpr` into a `sqlx` WHERE clause.
//!
//! Column names always come from a closed match on the field enums and are
//! never interpolated from user text; every value is bound.

use sqlx::QueryBuilder;
use sqlx::Sqlite;

use super::query_ast::CmpOp;
use super::query_ast::Expr;
use super::query_ast::MapCatalog;
use super::query_ast::MatchExpr;
use super::query_ast::MatchField;
use super::query_ast::MatchTerm;
use super::query_ast::Op;
use super::query_ast::Value;
use super::query_ast::ValueKind;

/// Compilation inputs that are not part of the query itself.
#[derive(Debug, Clone, Copy)]
pub struct CompileCtx<'a> {
    pub maps: &'a MapCatalog,
}

impl Default for CompileCtx<'_> {
    fn default() -> Self {
        const EMPTY: &MapCatalog = &MapCatalog::const_empty();
        CompileCtx { maps: EMPTY }
    }
}

pub fn push_match_expr(qb: &mut QueryBuilder<'_, Sqlite>, expr: &MatchExpr, ctx: &CompileCtx<'_>) {
    match expr {
        Expr::All(cs) if cs.is_empty() => {
            qb.push("1=1");
        }
        Expr::Any(cs) if cs.is_empty() => {
            qb.push("1=0");
        }
        Expr::All(cs) => push_joined(qb, cs, " AND ", ctx),
        Expr::Any(cs) => push_joined(qb, cs, " OR ", ctx),
        Expr::Not(inner) => {
            qb.push("NOT (");
            push_match_expr(qb, inner, ctx);
            qb.push(")");
        }
        Expr::Leaf(term) => push_match_term(qb, term, ctx),
    }
}

fn push_joined(qb: &mut QueryBuilder<'_, Sqlite>, cs: &[MatchExpr], join: &str, ctx: &CompileCtx<'_>) {
    qb.push("(");
    for (i, c) in cs.iter().enumerate() {
        if i > 0 {
            qb.push(join);
        }
        push_match_expr(qb, c, ctx);
    }
    qb.push(")");
}

fn push_match_term(qb: &mut QueryBuilder<'_, Sqlite>, term: &MatchTerm, ctx: &CompileCtx<'_>) {
    match term {
        MatchTerm::Field(MatchField::Map, op, Value::Text(needle)) => push_map(qb, *op, needle, ctx),
        MatchTerm::Field(field, op, value) => push_field(qb, *field, *op, value),
        // Task 3 replaces this arm.
        MatchTerm::Roster { .. } => {
            qb.push("1=0");
        }
        // Task 4 replaces this arm.
        MatchTerm::FreeText(_) => {
            qb.push("1=0");
        }
    }
}

/// The column each match field reads, qualified by the alias the outer query
/// uses (`m` for `indexed_match`, `r` for `replay_record`).
fn match_column(field: MatchField) -> &'static str {
    match field {
        MatchField::Map => "m.map",
        MatchField::GameType => "m.game_type",
        MatchField::GameMode => "m.game_mode",
        MatchField::MatchGroup => "m.match_group",
        MatchField::Date => "m.timestamp",
        MatchField::Build => "m.version_build",
        MatchField::Outcome => "r.outcome",
        MatchField::Group => "r.source_id",
        MatchField::ResultsAvailable => "r.results_available",
    }
}

fn push_field(qb: &mut QueryBuilder<'_, Sqlite>, field: MatchField, op: Op, value: &Value) {
    let col = match_column(field);
    if op.is_nullary() {
        let sql = if matches!(op, Op::IsSet) { "IS NOT NULL" } else { "IS NULL" };
        qb.push(format!("{col} {sql}"));
        return;
    }
    // Dispatch on both the field's declared kind and the value's actual
    // variant: a value that does not fit its field is a bug upstream, and
    // must narrow the result set, not widen it.
    match (field.value_kind(), value) {
        (ValueKind::Text, Value::Text(s)) => push_text(qb, col, op, s),
        (ValueKind::Int, Value::Int(n)) => push_num_i64(qb, col, op, *n),
        (ValueKind::Timestamp, Value::Timestamp(t)) => push_num_i64(qb, col, op, t.as_second()),
        (ValueKind::Outcome, Value::Outcome(o)) => push_eq_str(qb, col, op, o.as_db_str()),
        (ValueKind::Source, Value::Source(s)) => push_num_i64(qb, col, op, s.0),
        (ValueKind::Bool, Value::Bool(b)) => push_eq_bool(qb, col, op, *b),
        _ => {
            qb.push("1=0");
        }
    }
}

fn push_map(qb: &mut QueryBuilder<'_, Sqlite>, op: Op, needle: &str, ctx: &CompileCtx<'_>) {
    let raws = ctx.maps.raw_names_matching(needle);
    if raws.is_empty() {
        push_text(qb, "m.map", op, needle);
        return;
    }
    qb.push("(");
    push_text(qb, "m.map", op, needle);
    qb.push(" OR m.map IN (");
    for (i, raw) in raws.iter().enumerate() {
        if i > 0 {
            qb.push(", ");
        }
        qb.push_bind(raw.to_string());
    }
    qb.push("))");
}

fn push_text(qb: &mut QueryBuilder<'_, Sqlite>, col: &str, op: Op, s: &str) {
    match op {
        Op::Contains => {
            qb.push(format!("LOWER({col}) LIKE '%' || LOWER(")).push_bind(s.to_string()).push(") || '%'");
        }
        Op::NotEquals | Op::Ne | Op::IsNot => {
            qb.push(format!("LOWER({col}) <> LOWER(")).push_bind(s.to_string()).push(")");
        }
        _ => {
            qb.push(format!("LOWER({col}) = LOWER(")).push_bind(s.to_string()).push(")");
        }
    }
}

/// The comparison an `Op` makes when applied to a numeric column.
fn num_cmp(op: Op) -> CmpOp {
    match op {
        Op::Ne | Op::NotEquals | Op::IsNot => CmpOp::Ne,
        Op::Gt => CmpOp::Gt,
        Op::Ge => CmpOp::Ge,
        Op::Lt => CmpOp::Lt,
        Op::Le => CmpOp::Le,
        _ => CmpOp::Eq,
    }
}

fn push_num_i64(qb: &mut QueryBuilder<'_, Sqlite>, col: &str, op: Op, n: i64) {
    qb.push(format!("{col} {} ", num_cmp(op).as_sql())).push_bind(n);
}

fn push_eq_str(qb: &mut QueryBuilder<'_, Sqlite>, col: &str, op: Op, val: &str) {
    let sql = if matches!(op, Op::IsNot | Op::NotEquals | Op::Ne) { "<>" } else { "=" };
    qb.push(format!("{col} {sql} ")).push_bind(val.to_string());
}

fn push_eq_bool(qb: &mut QueryBuilder<'_, Sqlite>, col: &str, op: Op, b: bool) {
    let sql = if matches!(op, Op::IsNot | Op::NotEquals | Op::Ne) { "<>" } else { "=" };
    qb.push(format!("{col} {sql} ")).push_bind(b);
}

#[cfg(test)]
mod tests {
    use sqlx::QueryBuilder;
    use sqlx::Sqlite;

    use super::*;
    use crate::index::query_ast::MatchField;
    use crate::index::query_ast::MatchTerm;
    use crate::index::query_ast::Op;
    use crate::index::query_ast::Value;
    use crate::index::rows::MatchOutcome;

    fn sql_for(expr: &MatchExpr) -> String {
        let ctx = CompileCtx::default();
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("");
        push_match_expr(&mut qb, expr, &ctx);
        qb.sql().to_string()
    }

    fn leaf(f: MatchField, op: Op, v: Value) -> MatchExpr {
        Expr::Leaf(MatchTerm::Field(f, op, v))
    }

    #[test]
    fn empty_all_matches_everything_and_empty_any_matches_nothing() {
        assert_eq!(sql_for(&Expr::All(vec![])), "1=1");
        assert_eq!(sql_for(&Expr::Any(vec![])), "1=0");
    }

    #[test]
    fn conjunction_and_disjunction_are_parenthesised() {
        let a = leaf(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win));
        let b = leaf(MatchField::Build, Op::Ge, Value::Int(1234));
        let and = sql_for(&Expr::All(vec![a.clone(), b.clone()]));
        assert!(and.starts_with('(') && and.ends_with(')'), "got {and}");
        assert!(and.contains(" AND "), "got {and}");
        let or = sql_for(&Expr::Any(vec![a, b]));
        assert!(or.contains(" OR "), "got {or}");
    }

    #[test]
    fn negation_wraps_in_not() {
        let inner = leaf(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Loss));
        let sql = sql_for(&Expr::Not(Box::new(inner)));
        assert!(sql.starts_with("NOT ("), "got {sql}");
    }

    #[test]
    fn text_contains_is_case_insensitive_and_bound() {
        let sql = sql_for(&leaf(MatchField::GameType, Op::Contains, Value::Text("pvp".into())));
        assert!(sql.contains("LOWER(m.game_type)"), "got {sql}");
        assert!(sql.contains("LIKE"), "got {sql}");
        // The needle must be a bind placeholder, never inlined.
        assert!(!sql.contains("pvp"), "value was inlined into SQL: {sql}");
    }

    #[test]
    fn date_compares_against_the_timestamp_column() {
        let ts = jiff::Timestamp::from_second(1_700_000_000).unwrap();
        let sql = sql_for(&leaf(MatchField::Date, Op::Ge, Value::Timestamp(ts)));
        assert!(sql.contains("m.timestamp >="), "got {sql}");
    }

    #[test]
    fn is_set_and_is_not_set_compile_to_null_checks_with_no_bind() {
        let set = sql_for(&leaf(MatchField::Build, Op::IsSet, Value::NoOperand));
        assert_eq!(set, "m.version_build IS NOT NULL");
        let unset = sql_for(&leaf(MatchField::Build, Op::IsNotSet, Value::NoOperand));
        assert_eq!(unset, "m.version_build IS NULL");
    }

    #[test]
    fn map_without_a_catalogue_falls_back_to_the_raw_column() {
        let sql = sql_for(&leaf(MatchField::Map, Op::Contains, Value::Text("ocean".into())));
        assert!(sql.contains("LOWER(m.map)"), "got {sql}");
        assert!(!sql.contains(" IN ("), "empty catalogue must not emit an IN list: {sql}");
    }

    #[test]
    fn map_with_a_catalogue_unions_an_in_list_with_the_raw_match() {
        let maps = MapCatalog::from_pairs(vec![("spaces/13_OC_new_dawn".into(), "Ocean".into())]);
        let ctx = CompileCtx { maps: &maps };
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("");
        push_match_expr(&mut qb, &leaf(MatchField::Map, Op::Contains, Value::Text("ocean".into())), &ctx);
        let sql = qb.sql().to_string();
        assert!(sql.contains("m.map IN ("), "got {sql}");
        assert!(sql.contains(" OR "), "got {sql}");
        assert!(sql.contains("LOWER(m.map)"), "got {sql}");
    }

    #[test]
    fn a_term_whose_value_does_not_match_its_field_compiles_to_a_false_predicate() {
        // The parser and the UI both prevent this; a mismatch reaching here is a
        // bug, and must not silently widen the result set.
        let sql = sql_for(&leaf(MatchField::Date, Op::Ge, Value::Text("nonsense".into())));
        assert_eq!(sql, "1=0");
    }
}
