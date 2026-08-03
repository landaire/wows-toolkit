//! Human-readable pill text for one query term: `MatchField::Outcome, Op::Is,
//! Value::Outcome(Win)` reads as "Outcome is Win", not as grammar.

// Consumed by later query-bar tasks (the pill widget and filter builder); no
// call site in this crate yet.
#![allow(dead_code)]

use std::collections::HashMap;

use rust_i18n::t;
use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;

use crate::db::index::query_ast::CmpOp;
use crate::db::index::query_ast::DivisionScope;
use crate::db::index::query_ast::Expr;
use crate::db::index::query_ast::MatchField;
use crate::db::index::query_ast::MatchTerm;
use crate::db::index::query_ast::Op;
use crate::db::index::query_ast::Quant;
use crate::db::index::query_ast::RosterField;
use crate::db::index::query_ast::RosterTerm;
use crate::db::index::query_ast::ShipClass;
use crate::db::index::query_ast::Value;
// Only used by the test module's `sample_value`, reached through `use super::*`.
#[allow(unused_imports)]
use crate::db::index::query_ast::ValueKind;
use crate::db::index::rows::IndexSource;
use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::VehicleRelation;

/// Display names for ids that only appear as numbers in the tree. Filled by the
/// Search tab from the index; an id that is absent renders as `#<id>`, matching
/// what the tab did before this widget existed.
#[derive(Default)]
pub struct NameCache {
    pub ships: HashMap<GameParamId, String>,
    pub players: HashMap<AccountId, String>,
    pub sources: Vec<IndexSource>,
}

/// Human text for one match-level term.
pub fn pill_text(term: &MatchTerm, cache: &NameCache) -> String {
    match term {
        MatchTerm::Field(field, op, value) => field_op_value_text(match_field_label(*field), *op, value, cache),
        MatchTerm::Roster { quant, pred } => {
            format!("{} {}", quant_prefix(*quant), roster_expr_text(pred, cache))
        }
        MatchTerm::FreeText(s) => t!("ui.search.free_text", text = s).into(),
    }
}

/// Human text for one roster-level term.
pub fn roster_pill_text(term: &RosterTerm, cache: &NameCache) -> String {
    field_op_value_text(roster_field_label(term.field), term.op, &term.value, cache)
}

/// English prefix for how many roster rows must satisfy a predicate: "any",
/// "no", "3 or more", "exactly 2", and so on for the other `CmpOp`s.
pub fn quant_prefix(quant: Quant) -> String {
    match quant {
        Quant::Any => t!("ui.search.quant_any").into(),
        Quant::None => t!("ui.search.quant_none").into(),
        Quant::Count(CmpOp::Eq, n) => t!("ui.search.quant_eq", n = n).into(),
        Quant::Count(CmpOp::Ne, n) => t!("ui.search.quant_ne", n = n).into(),
        Quant::Count(CmpOp::Gt, n) => t!("ui.search.quant_gt", n = n).into(),
        Quant::Count(CmpOp::Ge, n) => t!("ui.search.quant_ge", n = n).into(),
        Quant::Count(CmpOp::Lt, n) => t!("ui.search.quant_lt", n = n).into(),
        Quant::Count(CmpOp::Le, n) => t!("ui.search.quant_le", n = n).into(),
    }
}

/// "<field> <op> <value>" for a comparison op, or "<field> <op>" alone for a
/// nullary one (`Op::IsSet` / `Op::IsNotSet`), which has no right-hand side.
fn field_op_value_text(field_label: String, op: Op, value: &Value, cache: &NameCache) -> String {
    let op_text = op_label(op);
    if op.is_nullary() {
        format!("{field_label} {op_text}")
    } else {
        format!("{field_label} {op_text} {}", value_text(value, cache))
    }
}

/// A roster predicate tree rendered as English, joining `All`/`Any` children
/// with "and"/"or" and prefixing a `Not` with "not".
fn roster_expr_text(expr: &Expr<RosterTerm>, cache: &NameCache) -> String {
    match expr {
        Expr::All(children) => children.iter().map(|c| roster_expr_text(c, cache)).collect::<Vec<_>>().join(" and "),
        Expr::Any(children) => children.iter().map(|c| roster_expr_text(c, cache)).collect::<Vec<_>>().join(" or "),
        Expr::Not(inner) => format!("not ({})", roster_expr_text(inner, cache)),
        Expr::Leaf(term) => roster_pill_text(term, cache),
    }
}

fn match_field_label(field: MatchField) -> String {
    match field {
        MatchField::Map => t!("ui.search.field.map"),
        MatchField::GameType => t!("ui.search.field.game_type"),
        MatchField::GameMode => t!("ui.search.field.game_mode"),
        MatchField::MatchGroup => t!("ui.search.field.match_group"),
        MatchField::Date => t!("ui.search.field.date"),
        MatchField::Build => t!("ui.search.field.build"),
        MatchField::Outcome => t!("ui.search.field.match_outcome"),
        MatchField::Group => t!("ui.search.field.group"),
        MatchField::ResultsAvailable => t!("ui.search.field.results_available"),
    }
    .into()
}

fn roster_field_label(field: RosterField) -> String {
    match field {
        RosterField::Relation => t!("ui.search.field.relation"),
        RosterField::Division => t!("ui.search.field.division"),
        RosterField::Account => t!("ui.search.field.player_present"),
        RosterField::Name => t!("ui.search.field.name"),
        RosterField::Clan => t!("ui.search.field.clan"),
        RosterField::Realm => t!("ui.search.field.realm"),
        RosterField::Ship => t!("ui.search.field.ship"),
        RosterField::ShipIndex => t!("ui.search.field.ship_index"),
        RosterField::Nation => t!("ui.search.field.nation"),
        RosterField::Class => t!("ui.search.field.class"),
        RosterField::Tier => t!("ui.search.field.tier"),
        RosterField::TestShip => t!("ui.search.field.test_ship"),
        RosterField::Damage => t!("ui.search.field.stat_damage"),
        RosterField::Kills => t!("ui.search.field.stat_kills"),
        RosterField::Spotting => t!("ui.search.field.stat_spotting"),
        RosterField::Potential => t!("ui.search.field.stat_potential"),
        RosterField::Received => t!("ui.search.field.stat_received"),
        RosterField::Pr => t!("ui.search.field.stat_pr"),
        RosterField::Survived => t!("ui.search.field.stat_survived"),
        RosterField::Disconnected => t!("ui.search.field.stat_disconnected"),
        RosterField::StreamSniper => t!("ui.search.field.stream_sniper"),
        RosterField::SniperLogin => t!("ui.search.field.sniper_login"),
    }
    .into()
}

fn op_label(op: Op) -> String {
    match op {
        Op::Contains => t!("ui.search.op.contains"),
        Op::Equals => t!("ui.search.op.equals"),
        Op::NotEquals => t!("ui.search.op.not_equals"),
        Op::Eq => t!("ui.search.op.eq"),
        Op::Ne => t!("ui.search.op.ne"),
        Op::Gt => t!("ui.search.op.gt"),
        Op::Ge => t!("ui.search.op.ge"),
        Op::Lt => t!("ui.search.op.lt"),
        Op::Le => t!("ui.search.op.le"),
        Op::Is => t!("ui.search.op.is"),
        Op::IsNot => t!("ui.search.op.is_not"),
        Op::IsSet => t!("ui.search.op.is_set"),
        Op::IsNotSet => t!("ui.search.op.is_not_set"),
    }
    .into()
}

/// Text for one term's right-hand value. Ship/account ids resolve through
/// `cache`, falling back to `#<raw>` when unresolved. `Value::NoOperand` (the
/// companion to a nullary op) is never reached: `field_op_value_text` only
/// calls this when `op.is_nullary()` is false.
fn value_text(value: &Value, cache: &NameCache) -> String {
    match value {
        Value::Text(s) => s.clone(),
        Value::Int(n) => crate::util::formatting::separate_number(*n, None),
        Value::Float(f) => format!("{f:.0}"),
        Value::Bool(b) => bool_label(*b),
        Value::Outcome(o) => outcome_label(*o),
        Value::Relation(r) => relation_label(*r),
        Value::Division(d) => division_label(*d),
        Value::Class(c) => class_label(*c),
        Value::Ship(id) => cache.ships.get(id).cloned().unwrap_or_else(|| format!("#{}", id.raw())),
        Value::Account(a) => cache.players.get(a).cloned().unwrap_or_else(|| format!("#{}", a.raw())),
        Value::Source(s) => cache
            .sources
            .iter()
            .find(|src| src.id == *s)
            .map(|src| src.name.clone())
            .unwrap_or_else(|| format!("#{}", s.0)),
        Value::Timestamp(ts) => ts.strftime("%Y-%m-%d").to_string(),
        Value::NoOperand => String::new(),
    }
}

fn bool_label(b: bool) -> String {
    if b { t!("ui.search.bool_true") } else { t!("ui.search.bool_false") }.into()
}

fn outcome_label(o: MatchOutcome) -> String {
    match o {
        MatchOutcome::Win => t!("ui.search.outcome_win"),
        MatchOutcome::Loss => t!("ui.search.outcome_loss"),
        MatchOutcome::Draw => t!("ui.search.outcome_draw"),
        MatchOutcome::Unknown => t!("ui.search.outcome_unknown"),
    }
    .into()
}

fn relation_label(r: VehicleRelation) -> String {
    match r {
        VehicleRelation::SelfPlayer => t!("ui.search.value.relation_self"),
        VehicleRelation::Ally => t!("ui.search.value.relation_ally"),
        VehicleRelation::Enemy => t!("ui.search.value.relation_enemy"),
    }
    .into()
}

fn division_label(d: DivisionScope) -> String {
    match d {
        DivisionScope::Mine => t!("ui.search.value.division_mine"),
        DivisionScope::Any => t!("ui.search.value.division_any"),
        DivisionScope::None => t!("ui.search.value.division_none"),
    }
    .into()
}

fn class_label(c: ShipClass) -> String {
    match c {
        ShipClass::AirCarrier => t!("ui.search.value.class_air_carrier"),
        ShipClass::Battleship => t!("ui.search.value.class_battleship"),
        ShipClass::Cruiser => t!("ui.search.value.class_cruiser"),
        ShipClass::Destroyer => t!("ui.search.value.class_destroyer"),
        ShipClass::Submarine => t!("ui.search.value.class_submarine"),
        ShipClass::Auxiliary => t!("ui.search.value.class_auxiliary"),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::index::query_ast::MatchField;
    use crate::db::index::query_ast::Op;
    use crate::db::index::query_ast::RosterField;
    use crate::db::index::query_ast::Value;
    use crate::db::index::rows::MatchOutcome;
    use wows_replays::types::GameParamId;

    #[test]
    fn a_match_field_pill_reads_as_field_op_value() {
        let cache = NameCache::default();
        let term = MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win));
        assert_eq!(pill_text(&term, &cache), "Outcome is Win");
    }

    #[test]
    fn a_nullary_op_pill_has_no_value() {
        let cache = NameCache::default();
        let term = MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand);
        assert_eq!(pill_text(&term, &cache), "Build is set");
    }

    #[test]
    fn an_unresolved_ship_id_falls_back_to_a_hash_id() {
        let cache = NameCache::default();
        let t =
            RosterTerm { field: RosterField::Ship, op: Op::Is, value: Value::Ship(GameParamId::from(4273766384u64)) };
        assert!(roster_pill_text(&t, &cache).contains("#4273766384"), "got {}", roster_pill_text(&t, &cache));
    }

    #[test]
    fn a_resolved_ship_id_shows_its_name() {
        let mut cache = NameCache::default();
        cache.ships.insert(GameParamId::from(1u64), "Yamato".into());
        let t = RosterTerm { field: RosterField::Ship, op: Op::Is, value: Value::Ship(GameParamId::from(1u64)) };
        assert_eq!(roster_pill_text(&t, &cache), "Ship is Yamato");
    }

    #[test]
    fn quantifier_prefixes_read_as_english() {
        assert_eq!(quant_prefix(Quant::Any), "any");
        assert_eq!(quant_prefix(Quant::None), "no");
        assert_eq!(quant_prefix(Quant::Count(CmpOp::Ge, 3)), "3 or more");
        assert_eq!(quant_prefix(Quant::Count(CmpOp::Eq, 2)), "exactly 2");
    }

    #[test]
    fn free_text_reads_as_a_plain_search() {
        let cache = NameCache::default();
        assert_eq!(pill_text(&MatchTerm::FreeText("yamato".into()), &cache), "contains \"yamato\"");
    }

    #[test]
    fn every_field_and_op_pair_produces_non_empty_text() {
        // A missing arm must not silently render an empty pill.
        let cache = NameCache::default();
        for f in MatchField::ALL {
            for &op in f.allowed_ops() {
                let value = if op.is_nullary() { Value::NoOperand } else { sample_value(f.value_kind()) };
                let text = pill_text(&MatchTerm::Field(f, op, value), &cache);
                assert!(!text.trim().is_empty(), "{f:?} {op:?} rendered empty");
            }
        }
        for f in RosterField::ALL {
            for &op in f.allowed_ops() {
                let value = if op.is_nullary() { Value::NoOperand } else { sample_value(f.value_kind()) };
                let t = RosterTerm { field: f, op, value };
                assert!(!roster_pill_text(&t, &cache).trim().is_empty(), "{f:?} {op:?} rendered empty");
            }
        }
    }

    fn sample_value(kind: ValueKind) -> Value {
        match kind {
            ValueKind::Text => Value::Text("x".into()),
            ValueKind::Int => Value::Int(1),
            ValueKind::Float => Value::Float(1.0),
            ValueKind::Bool => Value::Bool(true),
            ValueKind::Outcome => Value::Outcome(MatchOutcome::Win),
            ValueKind::Relation => Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
            ValueKind::Division => Value::Division(DivisionScope::Mine),
            ValueKind::Class => Value::Class(ShipClass::Destroyer),
            ValueKind::Ship => Value::Ship(GameParamId::from(1u64)),
            ValueKind::Account => Value::Account(wows_replays::types::AccountId(1)),
            ValueKind::Source => Value::Source(crate::db::index::rows::SourceId(1)),
            ValueKind::Timestamp => Value::Timestamp(jiff::Timestamp::from_second(0).unwrap()),
        }
    }
}
