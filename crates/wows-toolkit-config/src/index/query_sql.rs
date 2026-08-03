//! Compiles a `MatchExpr` into a `sqlx` WHERE clause.
//!
//! Column names always come from a closed match on the field enums and are
//! never interpolated from user text; every value is bound.

use sqlx::QueryBuilder;
use sqlx::Sqlite;

use super::query_ast::CmpOp;
use super::query_ast::DivisionScope;
use super::query_ast::Expr;
use super::query_ast::MapCatalog;
use super::query_ast::MatchExpr;
use super::query_ast::MatchField;
use super::query_ast::MatchTerm;
use super::query_ast::Op;
use super::query_ast::Quant;
use super::query_ast::RosterExpr;
use super::query_ast::RosterTerm;
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

/// Renders a leaf. The match level captures its `CompileCtx` here so the tree
/// walker itself stays generic over what a leaf is.
type PushLeaf<'a, L> = dyn FnMut(&mut QueryBuilder<'_, Sqlite>, &L) + 'a;

/// Render a boolean tree, delegating leaves to `leaf`. Shared by the match and
/// the roster level, the same way `print_expr` is shared by the printer.
fn push_expr<L>(qb: &mut QueryBuilder<'_, Sqlite>, expr: &Expr<L>, leaf: &mut PushLeaf<'_, L>) {
    match expr {
        Expr::All(cs) if cs.is_empty() => {
            qb.push("1=1");
        }
        Expr::Any(cs) if cs.is_empty() => {
            qb.push("1=0");
        }
        Expr::All(cs) => push_joined(qb, cs, " AND ", leaf),
        Expr::Any(cs) => push_joined(qb, cs, " OR ", leaf),
        Expr::Not(inner) => {
            qb.push("NOT (");
            push_expr(qb, inner, leaf);
            qb.push(")");
        }
        Expr::Leaf(l) => leaf(qb, l),
    }
}

fn push_joined<L>(qb: &mut QueryBuilder<'_, Sqlite>, cs: &[Expr<L>], join: &str, leaf: &mut PushLeaf<'_, L>) {
    qb.push("(");
    for (i, c) in cs.iter().enumerate() {
        if i > 0 {
            qb.push(join);
        }
        push_expr(qb, c, leaf);
    }
    qb.push(")");
}

pub fn push_match_expr(qb: &mut QueryBuilder<'_, Sqlite>, expr: &MatchExpr, ctx: &CompileCtx<'_>) {
    push_expr(qb, expr, &mut |qb, term| push_match_term(qb, term, ctx));
}

fn push_match_term(qb: &mut QueryBuilder<'_, Sqlite>, term: &MatchTerm, ctx: &CompileCtx<'_>) {
    match term {
        // A nullary op (IsSet/IsNotSet) on Map must go through the same
        // nullary-first ordering as every other field, not the catalogue path.
        MatchTerm::Field(MatchField::Map, op, Value::Text(needle)) if !op.is_nullary() => {
            push_map(qb, *op, needle, ctx)
        }
        MatchTerm::Field(field, op, value) => push_field(qb, *field, *op, value),
        MatchTerm::Roster { quant, pred } => push_roster(qb, *quant, pred),
        MatchTerm::FreeText(needle) => push_free_text(qb, needle),
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

/// A `map` term compares the raw space name the column holds and, when the
/// catalogue is loaded, the display names the user actually reads.
///
/// Both halves answer the operator that was given. A contains resolves the
/// catalogue by substring; anything else by an exact display name. A negated
/// operator joins the halves with AND rather than OR, because the negation of
/// "the raw name matches or the display name does" is "neither does": an OR
/// would be satisfied by the raw name alone and turn the term into its opposite.
fn push_map(qb: &mut QueryBuilder<'_, Sqlite>, op: Op, needle: &str, ctx: &CompileCtx<'_>) {
    let raws = match op {
        Op::Contains => ctx.maps.raw_names_matching(needle),
        _ => ctx.maps.raw_names_named(needle),
    };
    if raws.is_empty() {
        push_text(qb, "m.map", op, needle);
        return;
    }
    let negated = matches!(op, Op::NotEquals | Op::Ne | Op::IsNot);
    qb.push("(");
    push_text(qb, "m.map", op, needle);
    qb.push(if negated { " AND m.map NOT IN (" } else { " OR m.map IN (" });
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

/// A bare word. `m.map` is on the outer row, so it compares directly; the three
/// roster columns need their own arena-correlated EXISTS.
fn push_free_text(qb: &mut QueryBuilder<'_, Sqlite>, needle: &str) {
    qb.push("(");
    push_text(qb, "m.map", Op::Contains, needle);
    qb.push(" OR EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND (");
    push_text(qb, "v.player_name", Op::Contains, needle);
    qb.push(" OR ");
    push_text(qb, "v.clan", Op::Contains, needle);
    qb.push(" OR ");
    push_text(qb, "v.ship_name", Op::Contains, needle);
    qb.push(")))");
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

fn push_roster(qb: &mut QueryBuilder<'_, Sqlite>, quant: Quant, pred: &RosterExpr) {
    match quant {
        Quant::Any => {
            qb.push("EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND ");
            push_roster_expr(qb, pred);
            qb.push(")");
        }
        Quant::None => {
            qb.push("NOT EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND ");
            push_roster_expr(qb, pred);
            qb.push(")");
        }
        Quant::Count(op, n) => {
            qb.push("(SELECT COUNT(*) FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND ");
            push_roster_expr(qb, pred);
            qb.push(format!(") {} ", op.as_sql())).push_bind(i64::from(n));
        }
    }
}

pub fn push_roster_expr(qb: &mut QueryBuilder<'_, Sqlite>, expr: &RosterExpr) {
    push_expr(qb, expr, &mut push_roster_term);
}

/// Dispatch is on `(field.value_kind(), value)`, not on the value alone: a
/// `RosterField::Tier` term carrying a `Value::Text` must fall through to the
/// mismatch arm rather than reach the text comparison for an integer column.
/// Task 2's `push_field` pairs the same way.
fn push_roster_term(qb: &mut QueryBuilder<'_, Sqlite>, term: &RosterTerm) {
    let col = format!("v.{}", term.field.column());
    if term.op.is_nullary() {
        let sql = if matches!(term.op, Op::IsSet) { "IS NOT NULL" } else { "IS NULL" };
        qb.push(format!("{col} {sql}"));
        return;
    }
    match (term.field.value_kind(), &term.value) {
        (ValueKind::Division, Value::Division(scope)) => push_division(qb, term.op, *scope),
        (ValueKind::Text, Value::Text(s)) => push_text(qb, &col, term.op, s),
        (ValueKind::Int, Value::Int(n)) => push_num_i64(qb, &col, term.op, *n),
        (ValueKind::Float, Value::Float(f)) => push_num_f64(qb, &col, term.op, *f),
        (ValueKind::Bool, Value::Bool(b)) => push_eq_bool(qb, &col, term.op, *b),
        (ValueKind::Class, Value::Class(c)) => push_eq_str(qb, &col, term.op, c.as_db_str()),
        (ValueKind::Relation, Value::Relation(r)) => push_eq_str(qb, &col, term.op, r.as_db_str()),
        (ValueKind::Account, Value::Account(a)) => push_num_i64(qb, &col, term.op, a.raw()),
        (ValueKind::Ship, Value::Ship(s)) => push_num_i64(qb, &col, term.op, s.raw() as i64),
        // A value that does not fit the field is a bug upstream. Narrow, do not widen.
        _ => {
            qb.push("1=0");
        }
    }
}

/// `Mine` correlates to the perspective player's own division. The
/// `relation IN ('self', 'ally')` clause assumes nothing about whether
/// `division_id` (the server's prebattle id) can collide across teams: a
/// division is always same-team, so the constraint cannot exclude a correct
/// row. When the self row has a NULL `division_id` (the player was solo) the
/// equality yields NULL for every row and the quantifier is false, which is
/// correct.
fn push_division(qb: &mut QueryBuilder<'_, Sqlite>, op: Op, scope: DivisionScope) {
    let negated = matches!(op, Op::IsNot | Op::NotEquals | Op::Ne);
    if negated {
        qb.push("NOT (");
    }
    match scope {
        DivisionScope::Mine => {
            qb.push(
                "(v.division_id IS NOT NULL AND v.relation IN ('self', 'ally') \
                 AND v.division_id = (SELECT s.division_id FROM indexed_vehicle s \
                 WHERE s.arena_id = m.arena_id AND s.relation = 'self'))",
            );
        }
        DivisionScope::Any => {
            qb.push("v.division_id IS NOT NULL");
        }
        DivisionScope::None => {
            qb.push("v.division_id IS NULL");
        }
    }
    if negated {
        qb.push(")");
    }
}

fn push_num_f64(qb: &mut QueryBuilder<'_, Sqlite>, col: &str, op: Op, n: f64) {
    qb.push(format!("{col} {} ", num_cmp(op).as_sql())).push_bind(n);
}

#[cfg(test)]
mod tests {
    use sqlx::QueryBuilder;
    use sqlx::Sqlite;

    use super::*;
    use crate::index::query_ast::CmpOp;
    use crate::index::query_ast::DivisionScope;
    use crate::index::query_ast::MatchField;
    use crate::index::query_ast::MatchTerm;
    use crate::index::query_ast::Op;
    use crate::index::query_ast::Quant;
    use crate::index::query_ast::RosterExpr;
    use crate::index::query_ast::RosterField;
    use crate::index::query_ast::RosterTerm;
    use crate::index::query_ast::ShipClass;
    use crate::index::query_ast::Value;
    use crate::index::rows::MatchOutcome;
    use crate::index::rows::VehicleRelation;
    use wows_core::game_types::AccountId;
    use wows_core::game_types::GameParamId;

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

    fn sql_with(expr: &MatchExpr, maps: &MapCatalog) -> String {
        let ctx = CompileCtx { maps };
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("");
        push_match_expr(&mut qb, expr, &ctx);
        qb.sql().to_string()
    }

    fn two_oceans() -> MapCatalog {
        MapCatalog::from_pairs(vec![
            ("spaces/13_OC_new_dawn".into(), "Ocean".into()),
            ("spaces/40_okinawa".into(), "Ocean Rift".into()),
        ])
    }

    #[test]
    fn map_with_a_catalogue_unions_an_in_list_with_the_raw_match() {
        let maps = MapCatalog::from_pairs(vec![("spaces/13_OC_new_dawn".into(), "Ocean".into())]);
        let sql = sql_with(&leaf(MatchField::Map, Op::Contains, Value::Text("ocean".into())), &maps);
        assert!(sql.contains("m.map IN ("), "got {sql}");
        assert!(sql.contains(" OR "), "got {sql}");
        assert!(sql.contains("LOWER(m.map)"), "got {sql}");
    }

    /// The negation of "the raw name matches or its display name does" is
    /// "neither does". An OR here is satisfied by the raw name alone, which
    /// returns exactly the rows the term asked to exclude.
    #[test]
    fn a_negated_map_term_intersects_the_catalogue_instead_of_unioning_it() {
        let maps = MapCatalog::from_pairs(vec![("spaces/13_OC_new_dawn".into(), "Ocean".into())]);
        let sql = sql_with(&leaf(MatchField::Map, Op::NotEquals, Value::Text("ocean".into())), &maps);
        assert_eq!(sql, "(LOWER(m.map) <> LOWER(?) AND m.map NOT IN (?))");
    }

    #[test]
    fn the_catalogue_half_resolves_by_substring_only_for_contains() {
        let maps = two_oceans();
        // Two display names contain "ocean", so a contains term lists both.
        let contains = sql_with(&leaf(MatchField::Map, Op::Contains, Value::Text("ocean".into())), &maps);
        assert!(contains.contains("m.map IN (?, ?)"), "got {contains}");
        // Only one is named "ocean", so an equality lists that one.
        let equals = sql_with(&leaf(MatchField::Map, Op::Equals, Value::Text("ocean".into())), &maps);
        assert!(equals.contains("m.map IN (?)"), "got {equals}");
        let not_equals = sql_with(&leaf(MatchField::Map, Op::NotEquals, Value::Text("ocean".into())), &maps);
        assert!(not_equals.contains("m.map NOT IN (?)"), "got {not_equals}");
    }

    #[test]
    fn map_with_a_nullary_op_narrows_instead_of_rendering_a_text_comparison() {
        // Map's allowed_ops is TEXT_OPS, so this term is malformed upstream.
        // It must go through the same nullary-first ordering as every other
        // field, not the catalogue path, and never render a live comparison
        // against the stray value.
        let sql = sql_for(&leaf(MatchField::Map, Op::IsSet, Value::Text("ocean".into())));
        assert_eq!(sql, "m.map IS NOT NULL");
    }

    #[test]
    fn a_term_whose_value_does_not_match_its_field_compiles_to_a_false_predicate() {
        // The parser and the UI both prevent this; a mismatch reaching here is a
        // bug, and must not silently widen the result set.
        let sql = sql_for(&leaf(MatchField::Date, Op::Ge, Value::Text("nonsense".into())));
        assert_eq!(sql, "1=0");
    }

    fn roster(quant: Quant, pred: RosterExpr) -> MatchExpr {
        Expr::Leaf(MatchTerm::Roster { quant, pred })
    }

    fn rleaf(field: RosterField, op: Op, value: Value) -> RosterExpr {
        Expr::Leaf(RosterTerm { field, op, value })
    }

    #[test]
    fn any_compiles_to_exists_correlated_on_arena() {
        let sql = sql_for(&roster(Quant::Any, rleaf(RosterField::Tier, Op::Eq, Value::Int(10))));
        assert!(
            sql.starts_with("EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND "),
            "got {sql}"
        );
        assert!(sql.contains("v.tier ="), "got {sql}");
    }

    #[test]
    fn none_compiles_to_not_exists() {
        let sql = sql_for(&roster(Quant::None, rleaf(RosterField::Class, Op::Is, Value::Class(ShipClass::AirCarrier))));
        assert!(sql.starts_with("NOT EXISTS ("), "got {sql}");
        assert!(sql.contains("v.species"), "got {sql}");
    }

    #[test]
    fn count_compiles_to_a_correlated_aggregate_comparison() {
        let sql = sql_for(&roster(
            Quant::Count(CmpOp::Ge, 3),
            Expr::All(vec![
                rleaf(RosterField::Relation, Op::Is, Value::Relation(VehicleRelation::Enemy)),
                rleaf(RosterField::Damage, Op::Gt, Value::Int(100_000)),
            ]),
        ));
        assert!(sql.contains("SELECT COUNT(*) FROM indexed_vehicle v"), "got {sql}");
        assert!(sql.contains("v.arena_id = m.arena_id"), "got {sql}");
        assert!(sql.contains(") >= "), "got {sql}");
        assert!(sql.contains("v.relation"), "got {sql}");
        assert!(sql.contains("v.damage >"), "got {sql}");
    }

    #[test]
    fn division_mine_correlates_to_the_self_rows_division_and_stays_same_team() {
        let sql =
            sql_for(&roster(Quant::Any, rleaf(RosterField::Division, Op::Is, Value::Division(DivisionScope::Mine))));
        assert!(sql.contains("v.division_id IS NOT NULL"), "got {sql}");
        assert!(sql.contains("v.relation IN ('self', 'ally')"), "got {sql}");
        assert!(sql.contains("SELECT s.division_id FROM indexed_vehicle s"), "got {sql}");
        assert!(sql.contains("s.arena_id = m.arena_id"), "got {sql}");
        assert!(sql.contains("s.relation = 'self'"), "got {sql}");
    }

    #[test]
    fn division_any_and_none_are_plain_null_checks() {
        let any =
            sql_for(&roster(Quant::Any, rleaf(RosterField::Division, Op::Is, Value::Division(DivisionScope::Any))));
        assert!(any.contains("v.division_id IS NOT NULL"), "got {any}");
        assert!(!any.contains("s.division_id"), "Any must not correlate to the self row: {any}");

        let none =
            sql_for(&roster(Quant::Any, rleaf(RosterField::Division, Op::Is, Value::Division(DivisionScope::None))));
        assert!(none.contains("v.division_id IS NULL"), "got {none}");
    }

    #[test]
    fn roster_predicates_nest_with_and_or_and_not() {
        let sql = sql_for(&roster(
            Quant::Any,
            Expr::All(vec![
                rleaf(RosterField::Division, Op::Is, Value::Division(DivisionScope::Mine)),
                Expr::Not(Box::new(rleaf(RosterField::TestShip, Op::Is, Value::Bool(true)))),
                Expr::Any(vec![
                    rleaf(RosterField::Tier, Op::Eq, Value::Int(9)),
                    rleaf(RosterField::Tier, Op::Eq, Value::Int(10)),
                ]),
            ]),
        ));
        assert!(sql.contains(" AND "), "got {sql}");
        assert!(sql.contains(" OR "), "got {sql}");
        assert!(sql.contains("NOT ("), "got {sql}");
    }

    #[test]
    fn roster_is_set_needs_no_bind() {
        let sql = sql_for(&roster(Quant::Any, rleaf(RosterField::Damage, Op::IsSet, Value::NoOperand)));
        assert!(sql.contains("v.damage IS NOT NULL"), "got {sql}");
    }

    #[test]
    fn a_roster_term_whose_value_does_not_match_its_field_compiles_to_a_false_predicate() {
        // tier is an integer column; a text value is an upstream bug and must
        // narrow the result set, not reach the text comparison.
        let sql = sql_for(&roster(Quant::Any, rleaf(RosterField::Tier, Op::Eq, Value::Text("ten".into()))));
        assert!(sql.contains("1=0"), "got {sql}");
        assert!(!sql.contains("LOWER(v.tier)"), "got {sql}");
    }

    #[test]
    fn account_and_ship_bind_their_newtype_inner_values() {
        let acct = sql_for(&roster(Quant::Any, rleaf(RosterField::Account, Op::Is, Value::Account(AccountId(7)))));
        assert!(acct.contains("v.account_id ="), "got {acct}");
        let ship =
            sql_for(&roster(Quant::Any, rleaf(RosterField::Ship, Op::Is, Value::Ship(GameParamId::from(999u64)))));
        assert!(ship.contains("v.ship_id ="), "got {ship}");
    }

    #[test]
    fn free_text_searches_map_player_clan_and_ship_name() {
        let sql = sql_for(&Expr::Leaf(MatchTerm::FreeText("yamato".into())));
        assert_eq!(
            sql,
            "(LOWER(m.map) LIKE '%' || LOWER(?) || '%' OR EXISTS (SELECT 1 FROM indexed_vehicle v WHERE v.arena_id = m.arena_id AND (LOWER(v.player_name) LIKE '%' || LOWER(?) || '%' OR LOWER(v.clan) LIKE '%' || LOWER(?) || '%' OR LOWER(v.ship_name) LIKE '%' || LOWER(?) || '%')))"
        );
        assert!(!sql.contains("yamato"), "needle was inlined into SQL: {sql}");
    }
}
