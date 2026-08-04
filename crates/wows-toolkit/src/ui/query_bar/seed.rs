//! The queries other parts of the app hand to the Search tab.
//!
//! Every one of these is a tree built in code rather than parsed, so nothing
//! checks that its operators are ones the field accepts. `Op` spells equality
//! three ways and all three print as `=`; the parser picks between them from the
//! field's `ValueKind`, so a term built with the wrong one prints fine and
//! reparses into a different tree. Building them here, through `seed_op`, is
//! what keeps that from being a per-call-site decision.

use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;

use crate::db::index::query_ast::Expr;
use crate::db::index::query_ast::MatchExpr;
use crate::db::index::query_ast::MatchField;
use crate::db::index::query_ast::MatchTerm;
use crate::db::index::query_ast::Op;
use crate::db::index::query_ast::Quant;
use crate::db::index::query_ast::RosterExpr;
use crate::db::index::query_ast::RosterField;
use crate::db::index::query_ast::RosterTerm;
use crate::db::index::query_ast::Value;
use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::SourceId;
use crate::db::index::rows::VehicleRelation;

/// The comparison a seeded term wants, before the field's own `allowed_ops`
/// decides how to spell it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Wanted {
    Equality,
    Substring,
}

/// The operator to build a term with, taken from the field's own `allowed_ops`.
///
/// The preference list is tried first and the field's own list is the fallback,
/// so a field whose operator set changes yields something the field accepts
/// rather than a term that prints one way and reparses another. There is no
/// hand-picked last resort: naming an `Op` here is exactly what this module
/// exists to stop, and chaining the field's own list already makes the fallback
/// its first entry.
///
/// Also the chooser `select::reconcile_term` goes through when a filter change
/// leaves a term carrying an operator its new field disallows, so there is one
/// preference list rather than one per editing path.
pub(crate) fn seed_op(allowed: &'static [Op], wanted: Wanted) -> Op {
    let preferred: &[Op] = match wanted {
        Wanted::Equality => &[Op::Is, Op::Equals, Op::Eq],
        Wanted::Substring => &[Op::Contains],
    };
    preferred
        .iter()
        .chain(allowed)
        .copied()
        .find(|op| allowed.contains(op))
        .expect("every field allows at least one operator")
}

fn match_term(field: MatchField, wanted: Wanted, value: Value) -> MatchExpr {
    Expr::Leaf(MatchTerm::Field(field, seed_op(field.allowed_ops(), wanted), value))
}

fn roster_term(field: RosterField, wanted: Wanted, value: Value) -> RosterExpr {
    Expr::Leaf(RosterTerm { field, op: seed_op(field.allowed_ops(), wanted), value })
}

/// "at least one roster row satisfies `pred`".
fn any_roster(pred: RosterExpr) -> MatchExpr {
    Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred })
}

/// The perspective player's own roster row.
fn is_self() -> RosterExpr {
    roster_term(RosterField::Relation, Wanted::Equality, Value::Relation(VehicleRelation::SelfPlayer))
}

/// Every match indexed under `source` and nothing else.
pub fn source_scoped(source: SourceId) -> MatchExpr {
    match_term(MatchField::Group, Wanted::Equality, Value::Source(source))
}

/// Every match the user played in `ship`.
///
/// Goes through the roster rather than `replay_record.self_ship_id` because the
/// AST has no match-level self columns: the self row is the roster row whose
/// relation is `self`, and every stat is then uniformly available for it.
pub fn my_matches_in_ship(ship: GameParamId) -> MatchExpr {
    any_roster(Expr::All(vec![is_self(), roster_term(RosterField::Ship, Wanted::Equality, Value::Ship(ship))]))
}

/// Every match `account` appeared in, on either side.
pub fn matches_with_player(account: AccountId) -> MatchExpr {
    any_roster(roster_term(RosterField::Account, Wanted::Equality, Value::Account(account)))
}

/// Every match in which someone's clan tag or name contains `tag`. The index has
/// no clan-only field the user reaches from a tag alone, so a tag that occurs
/// inside a player's name matches too.
pub fn matches_mentioning_clan(tag: &str) -> MatchExpr {
    any_roster(Expr::Any(vec![
        roster_term(RosterField::Clan, Wanted::Substring, Value::Text(tag.to_owned())),
        roster_term(RosterField::Name, Wanted::Substring, Value::Text(tag.to_owned())),
    ]))
}

/// Losses in which the user's own ship sank.
pub fn games_i_died_in() -> MatchExpr {
    Expr::All(vec![
        match_term(MatchField::Outcome, Wanted::Equality, Value::Outcome(MatchOutcome::Loss)),
        any_roster(Expr::All(vec![
            is_self(),
            roster_term(RosterField::Survived, Wanted::Equality, Value::Bool(false)),
        ])),
    ])
}

pub fn games_i_won() -> MatchExpr {
    match_term(MatchField::Outcome, Wanted::Equality, Value::Outcome(MatchOutcome::Win))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::index::query_text::parse_query;
    use crate::db::index::query_text::print_query;

    fn every_seed() -> Vec<(&'static str, MatchExpr)> {
        vec![
            ("source_scoped", source_scoped(SourceId(7))),
            ("my_matches_in_ship", my_matches_in_ship(GameParamId::from(4_179_530_192_u64))),
            ("matches_with_player", matches_with_player(AccountId(1_234_567))),
            ("matches_mentioning_clan", matches_mentioning_clan("PANDA")),
            ("games_i_died_in", games_i_died_in()),
            ("games_i_won", games_i_won()),
        ]
    }

    /// The reason this module exists. A seeded tree that prints as text the
    /// grammar reads back differently is invisible until the first edit, and
    /// there is no compile-time protection against it.
    #[test]
    fn every_seeded_expression_round_trips() {
        for (name, expr) in every_seed() {
            let printed = print_query(&expr);
            let reparsed = parse_query(&printed).unwrap_or_else(|e| panic!("{name} printed {printed:?}: {e}"));
            assert_eq!(reparsed, expr, "{name} printed {printed:?}");
        }
    }

    /// Every operator in a seeded tree has to be one its own field allows, or
    /// the round trip above only happens to hold for the values chosen here.
    #[test]
    fn every_seeded_operator_is_one_its_field_allows() {
        fn check_match(expr: &MatchExpr, name: &str) {
            match expr {
                Expr::Leaf(MatchTerm::Field(field, op, _)) => {
                    assert!(field.allowed_ops().contains(op), "{name}: {field:?} does not allow {op:?}");
                }
                Expr::Leaf(MatchTerm::Roster { pred, .. }) => check_roster(pred, name),
                Expr::Leaf(MatchTerm::FreeText(_)) => {}
                other => other.children().iter().for_each(|c| check_match(c, name)),
            }
        }
        fn check_roster(expr: &RosterExpr, name: &str) {
            match expr {
                Expr::Leaf(term) => assert!(
                    term.field.allowed_ops().contains(&term.op),
                    "{name}: {:?} does not allow {:?}",
                    term.field,
                    term.op
                ),
                other => other.children().iter().for_each(|c| check_roster(c, name)),
            }
        }
        for (name, expr) in every_seed() {
            check_match(&expr, name);
        }
    }

    /// The trap this guards: a text field's equality is `Op::Equals` while an
    /// enum field's is `Op::Is`, and both print as `=`. Asking for equality by
    /// class rather than by variant is what keeps the two apart.
    #[test]
    fn seed_op_spells_equality_the_way_each_field_does() {
        assert_eq!(seed_op(MatchField::Outcome.allowed_ops(), Wanted::Equality), Op::Is);
        assert_eq!(seed_op(MatchField::Map.allowed_ops(), Wanted::Equality), Op::Equals);
        assert_eq!(seed_op(MatchField::Build.allowed_ops(), Wanted::Equality), Op::Eq);
        assert_eq!(seed_op(RosterField::Name.allowed_ops(), Wanted::Substring), Op::Contains);
        // A field with no substring form falls back to something it does allow
        // rather than to an operator it would reject.
        let fallback = seed_op(MatchField::Outcome.allowed_ops(), Wanted::Substring);
        assert!(MatchField::Outcome.allowed_ops().contains(&fallback));
    }

    /// Seeding replaces the query outright, so a seeded tree must already be in
    /// the shape the bar keeps its own trees in: a one-child group would print
    /// as a bare term and lose its brackets on the next reload.
    #[test]
    fn every_seeded_expression_is_already_canonical() {
        for (name, expr) in every_seed() {
            let mut canonical = expr.clone();
            crate::ui::query_bar::select::canonicalise(&mut canonical);
            assert_eq!(canonical, expr, "{name} is not canonical as built");
        }
    }
}
