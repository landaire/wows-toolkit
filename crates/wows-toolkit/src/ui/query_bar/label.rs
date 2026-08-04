//! Human-readable pill text for one query term: `MatchField::Outcome, Op::Is,
//! Value::Outcome(Win)` reads as "Outcome is Win", not as grammar.

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
// Reached by the test module's `sample_value` through `use super::*`.
#[cfg(test)]
use crate::db::index::query_ast::ValueKind;
use crate::db::index::rows::IndexSource;
use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::VehicleRelation;
use crate::ui::query_bar::tokens::NodePath;

/// Display names for ids that only appear as numbers in the tree. Filled by the
/// Search tab from the index; an id that is absent renders as `#<id>`, matching
/// what the tab did before this widget existed.
#[derive(Default)]
pub struct NameCache {
    pub ships: HashMap<GameParamId, String>,
    pub players: HashMap<AccountId, String>,
    pub sources: Vec<IndexSource>,
    /// The app's active locale, same value the Search tab reads from
    /// `settings.app.locale` and threads in here every frame. `None` (the
    /// default) formats numbers as en-US, matching `separate_number`'s own default.
    pub locale: Option<String>,
}

/// Which part of a term a segment renders. Each role carries its own click
/// target and its own autocomplete source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentRole {
    /// The field, including any scope prefix.
    Filter,
    Operator,
    /// Absent for a nullary operator.
    Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PillSegment {
    pub role: SegmentRole,
    pub text: String,
}

/// Human text for one match-level term, as a join of its segments.
/// `tokens.rs` carries `pill_segments` through the token stream directly
/// rather than calling this, so it has no production caller; kept for the
/// tests in this module that pin real display strings against it.
#[cfg(test)]
pub fn pill_text(term: &MatchTerm, cache: &NameCache) -> String {
    join_segments(&pill_segments(term, cache))
}

/// One segment per clickable part of a match-level term.
pub fn pill_segments(term: &MatchTerm, cache: &NameCache) -> Vec<PillSegment> {
    match term {
        MatchTerm::Field(field, op, value) => field_op_value_segments(match_field_label(*field), *op, value, cache),
        MatchTerm::Roster { quant, pred } => roster_term_segments(*quant, pred, cache),
        MatchTerm::FreeText(s) => {
            vec![PillSegment { role: SegmentRole::Value, text: t!("ui.search.free_text", text = s).into() }]
        }
    }
}

/// Human text for one roster-level term, as a join of its segments.
pub fn roster_pill_text(term: &RosterTerm, cache: &NameCache) -> String {
    join_segments(&roster_segments(term, cache))
}

/// One segment per clickable part of a roster-level term.
pub fn roster_segments(term: &RosterTerm, cache: &NameCache) -> Vec<PillSegment> {
    field_op_value_segments(roster_field_label(term.field), term.op, &term.value, cache)
}

fn join_segments(segments: &[PillSegment]) -> String {
    segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ")
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

/// Filter, operator segments for a comparison op, plus a value segment unless
/// the op is nullary (`Op::IsSet` / `Op::IsNotSet`), which has no right-hand
/// side.
fn field_op_value_segments(field_label: String, op: Op, value: &Value, cache: &NameCache) -> Vec<PillSegment> {
    let mut segments = vec![
        PillSegment { role: SegmentRole::Filter, text: field_label },
        PillSegment { role: SegmentRole::Operator, text: op_label(op) },
    ];
    if !op.is_nullary() {
        segments.push(PillSegment { role: SegmentRole::Value, text: value_text(value, cache) });
    }
    segments
}

/// Segments for a roster quantifier term. A sugar-shaped predicate (see
/// `sugar_shape`) collapses to one pill: a compound filter segment (scope and
/// field together, e.g. "Enemy ship") plus the inner term's operator and
/// value, matching what `roster_sugar_pill_text` renders as a single string.
/// Any other shape is not reached in practice -- `tokens.rs` renders it as a
/// bracketed group of per-leaf pills via `roster_segments` instead -- but
/// still needs a total, non-empty rendering here so `pill_text` keeps working
/// for it.
fn roster_term_segments(quant: Quant, pred: &Expr<RosterTerm>, cache: &NameCache) -> Vec<PillSegment> {
    match sugar_shape(quant, pred) {
        Some((scope_label, inner)) => {
            let field_label = roster_field_label(inner.field);
            let filter_text = match scope_label {
                Some(scope) => format!("{scope} {}", lowercase_first(&field_label)),
                None => format!("{} {field_label}", quant_prefix(quant)),
            };
            let mut segments = vec![PillSegment { role: SegmentRole::Filter, text: filter_text }];
            segments.extend(field_op_value_segments_tail(inner.op, &inner.value, cache));
            segments
        }
        None => vec![PillSegment {
            role: SegmentRole::Filter,
            text: format!("{} {}", quant_prefix(quant), roster_expr_text(pred, cache)),
        }],
    }
}

/// The operator (and, unless nullary, value) segments of `field_op_value_segments`,
/// without its filter segment. Used where the filter segment is built
/// separately, as for a sugar-shaped roster term's compound filter.
fn field_op_value_segments_tail(op: Op, value: &Value, cache: &NameCache) -> Vec<PillSegment> {
    let mut segments = vec![PillSegment { role: SegmentRole::Operator, text: op_label(op) }];
    if !op.is_nullary() {
        segments.push(PillSegment { role: SegmentRole::Value, text: value_text(value, cache) });
    }
    segments
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

pub(crate) fn match_field_label(field: MatchField) -> String {
    match field {
        MatchField::Map => t!("ui.search.field.map"),
        MatchField::GameType => t!("ui.search.field.game_type"),
        MatchField::GameMode => t!("ui.search.field.game_mode"),
        MatchField::MatchGroup => t!("ui.search.field.match_group"),
        MatchField::Date => t!("ui.search.field.date"),
        MatchField::Build => t!("ui.search.field.build"),
        MatchField::Outcome => t!("ui.search.field.outcome"),
        MatchField::Group => t!("ui.search.field.group"),
        MatchField::ResultsAvailable => t!("ui.search.field.results_available"),
    }
    .into()
}

pub(crate) fn roster_field_label(field: RosterField) -> String {
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

pub(crate) fn op_label(op: Op) -> String {
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
/// companion to a nullary op) is never reached: callers only reach this when
/// `op.is_nullary()` is false.
fn value_text(value: &Value, cache: &NameCache) -> String {
    match value {
        Value::Text(s) => s.clone(),
        Value::Int(n) => crate::util::formatting::separate_number(*n, cache.locale.as_deref()),
        // Matches the existing PR-formatting convention (`replay_parser/mod.rs`,
        // `stats_tab.rs`): rounded to a whole number, no thousands grouping.
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

/// Recognises the shapes `query_text::try_print_sugar` treats as sugar: `Any`
/// over a bare `Leaf` (returned as `(None, leaf)`), or an `All` of exactly two
/// where the first is a scope conjunct (self/ally/enemy relation, or
/// division-mine, each via `Op::Is`) and the second is a single `Leaf`
/// (returned as `(Some(scope_label), leaf)`). Returns `None` for any other
/// shape; callers render those as a bracketed `QuantOpen`/tokens/`QuantClose`
/// group instead.
///
/// Keep this in sync with `wows-toolkit-config`'s `query_text::try_print_sugar`:
/// any shape that prints as sugar there must render compactly here, or a
/// bracketed group would show text whose one-line form the user never sees.
fn sugar_shape(quant: Quant, pred: &Expr<RosterTerm>) -> Option<(Option<String>, &RosterTerm)> {
    if quant != Quant::Any {
        return None;
    }
    match pred {
        Expr::All(cs) if cs.len() == 2 => {
            let Expr::Leaf(first) = &cs[0] else { return None };
            let Expr::Leaf(inner) = &cs[1] else { return None };
            let scope_label: String = match (first.field, &first.value, first.op) {
                (RosterField::Relation, Value::Relation(VehicleRelation::SelfPlayer), Op::Is) => {
                    t!("ui.search.scope_self").into()
                }
                (RosterField::Relation, Value::Relation(VehicleRelation::Ally), Op::Is) => {
                    t!("ui.search.scope_ally").into()
                }
                (RosterField::Relation, Value::Relation(VehicleRelation::Enemy), Op::Is) => {
                    t!("ui.search.scope_enemy").into()
                }
                (RosterField::Division, Value::Division(DivisionScope::Mine), Op::Is) => {
                    t!("ui.search.scope_div").into()
                }
                _ => return None,
            };
            Some((Some(scope_label), inner))
        }
        Expr::Leaf(inner) => Some((None, inner)),
        _ => None,
    }
}

/// Where inside a sugar-shaped predicate the term a collapsed pill renders
/// lives, as a path relative to the predicate root. `None` for a shape that
/// does not collapse.
///
/// Gated on `sugar_shape` so the two cannot disagree about which shapes
/// collapse at all, and the index then follows from that function's two
/// accepting arms: a bare leaf is the predicate root itself, and the
/// two-conjunct scope form renders its second child.
/// `the_sugar_inner_path_resolves_to_the_term_sugar_shape_renders` pins the
/// pair against each other.
///
/// Arms are named rather than caught: a third shape added to `sugar_shape`
/// must state its own index here, instead of inheriting the scope form's and
/// silently retargeting every click on the pill it collapses.
pub(crate) fn sugar_inner_path(quant: Quant, pred: &Expr<RosterTerm>) -> Option<NodePath> {
    sugar_shape(quant, pred)?;
    match pred {
        Expr::Leaf(_) => Some(Vec::new()),
        Expr::All(_) => Some(vec![1]),
        Expr::Any(_) | Expr::Not(_) => None,
    }
}

/// Compact "Enemy ship is Yamato" form for a roster quantifier whose predicate
/// is sugar-shaped (see `sugar_shape`). Returns `None` for any other shape;
/// the caller renders those as a bracketed `QuantOpen`/tokens/`QuantClose`
/// group instead.
pub fn roster_sugar_pill_text(quant: Quant, pred: &Expr<RosterTerm>, cache: &NameCache) -> Option<String> {
    let (scope_label, inner) = sugar_shape(quant, pred)?;
    match scope_label {
        Some(scope_label) => Some(format!("{scope_label} {}", lowercase_first(&roster_pill_text(inner, cache)))),
        None => Some(format!("{} {}", quant_prefix(Quant::Any), roster_pill_text(inner, cache))),
    }
}

/// Lowercases the first character so a scope word can lead a compact pill's
/// sentence while the field name that follows reads as a continuation of it
/// ("Enemy ship is Yamato"), not as its own capitalized clause.
fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub(crate) fn bool_label(b: bool) -> String {
    if b { t!("ui.search.bool_true") } else { t!("ui.search.bool_false") }.into()
}

pub(crate) fn outcome_label(o: MatchOutcome) -> String {
    match o {
        MatchOutcome::Win => t!("ui.search.outcome_win"),
        MatchOutcome::Loss => t!("ui.search.outcome_loss"),
        MatchOutcome::Draw => t!("ui.search.outcome_draw"),
        MatchOutcome::Unknown => t!("ui.search.outcome_unknown"),
    }
    .into()
}

pub(crate) fn relation_label(r: VehicleRelation) -> String {
    match r {
        VehicleRelation::SelfPlayer => t!("ui.search.value.relation_self"),
        VehicleRelation::Ally => t!("ui.search.value.relation_ally"),
        VehicleRelation::Enemy => t!("ui.search.value.relation_enemy"),
    }
    .into()
}

pub(crate) fn division_label(d: DivisionScope) -> String {
    match d {
        DivisionScope::Mine => t!("ui.search.value.division_mine"),
        DivisionScope::Any => t!("ui.search.value.division_any"),
        DivisionScope::None => t!("ui.search.value.division_none"),
    }
    .into()
}

pub(crate) fn class_label(c: ShipClass) -> String {
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
        assert_eq!(pill_text(&term, &cache), "Result is Win");
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
    fn a_roster_quantifier_over_one_leaf_reads_as_quant_then_term() {
        let cache = NameCache::default();
        let pred = Expr::Leaf(RosterTerm {
            field: RosterField::Relation,
            op: Op::Is,
            value: Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
        });
        let term = MatchTerm::Roster { quant: Quant::Any, pred };
        assert_eq!(pill_text(&term, &cache), "any Relation is Enemy");
    }

    #[test]
    fn a_roster_quantifier_over_all_joins_leaves_with_and() {
        // Neither leaf is a scope conjunct (see `a_self_scoped_sugar_reads_as_a_compact_pill`
        // and friends for that), so this predicate is not sugar-shaped and
        // `pill_text` renders it fully expanded, joined with "and".
        let mut cache = NameCache::default();
        cache.ships.insert(GameParamId::from(1u64), "Yamato".into());
        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(5) }),
            Expr::Leaf(RosterTerm {
                field: RosterField::Ship,
                op: Op::Is,
                value: Value::Ship(GameParamId::from(1u64)),
            }),
        ]);
        let term = MatchTerm::Roster { quant: Quant::Any, pred };
        assert_eq!(pill_text(&term, &cache), "any Tier = 5 and Ship is Yamato");
    }

    #[test]
    fn a_roster_quantifier_over_any_joins_leaves_with_or() {
        let cache = NameCache::default();
        let pred = Expr::Any(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
            }),
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(crate::db::index::rows::VehicleRelation::Ally),
            }),
        ]);
        let term = MatchTerm::Roster { quant: Quant::None, pred };
        assert_eq!(pill_text(&term, &cache), "no Relation is Enemy or Relation is Ally");
    }

    #[test]
    fn a_roster_quantifier_over_a_negation_prefixes_not() {
        let cache = NameCache::default();
        let pred = Expr::Not(Box::new(Expr::Leaf(RosterTerm {
            field: RosterField::Relation,
            op: Op::Is,
            value: Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
        })));
        let term = MatchTerm::Roster { quant: Quant::Any, pred };
        assert_eq!(pill_text(&term, &cache), "any not (Relation is Enemy)");
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

    fn scoped_ship_pred(scope: (RosterField, Op, Value)) -> Expr<RosterTerm> {
        Expr::All(vec![
            Expr::Leaf(RosterTerm { field: scope.0, op: scope.1, value: scope.2 }),
            Expr::Leaf(RosterTerm {
                field: RosterField::Ship,
                op: Op::Is,
                value: Value::Ship(GameParamId::from(1u64)),
            }),
        ])
    }

    #[test]
    fn a_self_scoped_sugar_reads_as_a_compact_pill() {
        let mut cache = NameCache::default();
        cache.ships.insert(GameParamId::from(1u64), "Yamato".into());
        let pred = scoped_ship_pred((
            RosterField::Relation,
            Op::Is,
            Value::Relation(crate::db::index::rows::VehicleRelation::SelfPlayer),
        ));
        assert_eq!(roster_sugar_pill_text(Quant::Any, &pred, &cache), Some("Self ship is Yamato".to_string()));
    }

    #[test]
    fn an_ally_scoped_sugar_reads_as_a_compact_pill() {
        let mut cache = NameCache::default();
        cache.ships.insert(GameParamId::from(1u64), "Yamato".into());
        let pred = scoped_ship_pred((
            RosterField::Relation,
            Op::Is,
            Value::Relation(crate::db::index::rows::VehicleRelation::Ally),
        ));
        assert_eq!(roster_sugar_pill_text(Quant::Any, &pred, &cache), Some("Ally ship is Yamato".to_string()));
    }

    #[test]
    fn an_enemy_scoped_sugar_reads_as_a_compact_pill() {
        let mut cache = NameCache::default();
        cache.ships.insert(GameParamId::from(1u64), "Yamato".into());
        let pred = scoped_ship_pred((
            RosterField::Relation,
            Op::Is,
            Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
        ));
        assert_eq!(roster_sugar_pill_text(Quant::Any, &pred, &cache), Some("Enemy ship is Yamato".to_string()));
    }

    #[test]
    fn a_division_scoped_sugar_reads_as_a_compact_pill() {
        let mut cache = NameCache::default();
        cache.ships.insert(GameParamId::from(1u64), "Yamato".into());
        let pred = scoped_ship_pred((RosterField::Division, Op::Is, Value::Division(DivisionScope::Mine)));
        assert_eq!(roster_sugar_pill_text(Quant::Any, &pred, &cache), Some("Div ship is Yamato".to_string()));
    }

    #[test]
    fn a_bare_leaf_sugar_reads_like_the_general_quantifier_form() {
        let cache = NameCache::default();
        let pred = Expr::Leaf(RosterTerm {
            field: RosterField::Relation,
            op: Op::Is,
            value: Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
        });
        assert_eq!(roster_sugar_pill_text(Quant::Any, &pred, &cache), Some("any Relation is Enemy".to_string()));
    }

    #[test]
    fn a_non_any_quantifier_is_never_sugar() {
        let cache = NameCache::default();
        let pred = Expr::Leaf(RosterTerm {
            field: RosterField::Relation,
            op: Op::Is,
            value: Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
        });
        assert_eq!(roster_sugar_pill_text(Quant::Count(CmpOp::Ge, 2), &pred, &cache), None);
        assert_eq!(roster_sugar_pill_text(Quant::None, &pred, &cache), None);
    }

    #[test]
    fn a_scope_conjunct_built_with_the_wrong_op_is_not_sugar() {
        // `try_print_sugar` matches `Op::Is` specifically; `Op::Equals` prints
        // (and here renders) as the general bracketed form instead.
        let cache = NameCache::default();
        let pred = scoped_ship_pred((
            RosterField::Relation,
            Op::Equals,
            Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
        ));
        assert_eq!(roster_sugar_pill_text(Quant::Any, &pred, &cache), None);
    }

    #[test]
    fn a_three_way_all_is_not_sugar() {
        let cache = NameCache::default();
        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
            }),
            Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) }),
            Expr::Leaf(RosterTerm {
                field: RosterField::Ship,
                op: Op::Is,
                value: Value::Ship(GameParamId::from(1u64)),
            }),
        ]);
        assert_eq!(roster_sugar_pill_text(Quant::Any, &pred, &cache), None);
    }

    fn seg(role: SegmentRole, text: &str) -> PillSegment {
        PillSegment { role, text: text.into() }
    }

    #[test]
    fn a_match_field_term_yields_filter_operator_value() {
        let cache = NameCache::default();
        let term = MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win));
        assert_eq!(
            pill_segments(&term, &cache),
            vec![seg(SegmentRole::Filter, "Result"), seg(SegmentRole::Operator, "is"), seg(SegmentRole::Value, "Win")]
        );
    }

    #[test]
    fn a_nullary_operator_yields_two_segments_not_three() {
        let cache = NameCache::default();
        let term = MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand);
        let segs = pill_segments(&term, &cache);
        assert_eq!(segs.len(), 2, "got {segs:?}");
        assert!(segs.iter().all(|s| s.role != SegmentRole::Value));
    }

    #[test]
    fn joining_segments_reproduces_each_sample_terms_literal_text() {
        // One pinned literal per `sample_terms()` entry, in the order that
        // function returns them: plain field, nullary operator, sugar-shaped
        // roster, non-sugar roster, free text. Asserting the join against
        // `pill_text` (which is `join_segments(pill_segments(..))`) would
        // exercise the same code on both sides and could never catch a
        // separator or wording drift; only real strings pin the display form.
        let cache = NameCache::default();
        let expected = [
            "Result is Win",
            "Build is set",
            "Enemy ship is #1",
            "no Relation is Enemy or Relation is Ally",
            "contains \"yamato\"",
        ];
        for (term, want) in sample_terms().into_iter().zip(expected) {
            let joined = pill_segments(&term, &cache).iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
            assert_eq!(joined, want, "{term:?}");
        }
    }

    #[test]
    fn a_sugar_shaped_roster_term_keeps_a_compound_filter_segment() {
        let cache = NameCache::default();
        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(VehicleRelation::Enemy),
            }),
            Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) }),
        ]);
        let term = MatchTerm::Roster { quant: Quant::Any, pred };
        let segs = pill_segments(&term, &cache);
        assert_eq!(segs.len(), 3, "sugar stays one pill with three segments: {segs:?}");
        assert_eq!(segs[0].role, SegmentRole::Filter);
        assert!(segs[0].text.to_lowercase().contains("enemy"), "got {:?}", segs[0].text);
    }

    #[test]
    fn every_field_and_allowed_operator_yields_a_non_empty_segment_each() {
        // A missing arm must not render an empty segment, which would paint an
        // unclickable zero-width target.
        let cache = NameCache::default();
        for f in MatchField::ALL {
            for &op in f.allowed_ops() {
                let value = if op.is_nullary() { Value::NoOperand } else { sample_value(f.value_kind()) };
                for s in pill_segments(&MatchTerm::Field(f, op, value.clone()), &cache) {
                    assert!(!s.text.trim().is_empty(), "{f:?} {op:?} {:?} empty", s.role);
                }
            }
        }
        for f in RosterField::ALL {
            for &op in f.allowed_ops() {
                let value = if op.is_nullary() { Value::NoOperand } else { sample_value(f.value_kind()) };
                let t = RosterTerm { field: f, op, value };
                for s in roster_segments(&t, &cache) {
                    assert!(!s.text.trim().is_empty(), "{f:?} {op:?} {:?} empty", s.role);
                }
            }
        }
    }

    #[test]
    fn free_text_is_a_single_value_segment() {
        let cache = NameCache::default();
        let segs = pill_segments(&MatchTerm::FreeText("yamato".into()), &cache);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].role, SegmentRole::Value);
    }

    /// One representative `MatchTerm` for each shape `pill_segments` must
    /// handle: a plain field, a nullary operator, a sugar-shaped roster term,
    /// a non-sugar roster term, and free text.
    fn sample_terms() -> Vec<MatchTerm> {
        vec![
            MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win)),
            MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand),
            MatchTerm::Roster {
                quant: Quant::Any,
                pred: Expr::All(vec![
                    Expr::Leaf(RosterTerm {
                        field: RosterField::Relation,
                        op: Op::Is,
                        value: Value::Relation(VehicleRelation::Enemy),
                    }),
                    Expr::Leaf(RosterTerm {
                        field: RosterField::Ship,
                        op: Op::Is,
                        value: Value::Ship(GameParamId::from(1u64)),
                    }),
                ]),
            },
            MatchTerm::Roster {
                quant: Quant::None,
                pred: Expr::Any(vec![
                    Expr::Leaf(RosterTerm {
                        field: RosterField::Relation,
                        op: Op::Is,
                        value: Value::Relation(VehicleRelation::Enemy),
                    }),
                    Expr::Leaf(RosterTerm {
                        field: RosterField::Relation,
                        op: Op::Is,
                        value: Value::Relation(VehicleRelation::Ally),
                    }),
                ]),
            },
            MatchTerm::FreeText("yamato".into()),
        ]
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

    /// `sugar_inner_path` names an index while `sugar_shape` names a term, and
    /// they have to be the same term: a segment edit on a collapsed pill
    /// rewrites whatever the index reaches, and a pill that draws one term
    /// while its clicks retarget another is worse than no click at all.
    #[test]
    fn the_sugar_inner_path_resolves_to_the_term_sugar_shape_renders() {
        use crate::db::index::query_ast::Op;
        use crate::db::index::query_ast::RosterField;

        let tier = RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) };
        let scopes = [
            (RosterField::Relation, Value::Relation(VehicleRelation::SelfPlayer)),
            (RosterField::Relation, Value::Relation(VehicleRelation::Ally)),
            (RosterField::Relation, Value::Relation(VehicleRelation::Enemy)),
            (RosterField::Division, Value::Division(DivisionScope::Mine)),
        ];
        let mut preds: Vec<Expr<RosterTerm>> = vec![Expr::Leaf(tier.clone())];
        for (field, value) in scopes {
            preds.push(Expr::All(vec![Expr::Leaf(RosterTerm { field, op: Op::Is, value }), Expr::Leaf(tier.clone())]));
        }

        for pred in &preds {
            let (_, inner) = sugar_shape(Quant::Any, pred).expect("the fixture is sugar-shaped");
            let path = sugar_inner_path(Quant::Any, pred).expect("a sugar shape has an inner path");
            let mut reached = pred;
            for step in &path {
                match reached {
                    Expr::All(children) | Expr::Any(children) => reached = &children[*step],
                    other => panic!("path {path:?} left the tree at {other:?}"),
                }
            }
            match reached {
                Expr::Leaf(term) => assert_eq!(term, inner, "path {path:?} reached the wrong term"),
                other => panic!("path {path:?} reached {other:?}, not a leaf"),
            }
        }

        // A shape that does not collapse has no inner term to point at.
        let not_sugar = Expr::Leaf(tier.clone());
        assert!(sugar_inner_path(Quant::None, &not_sugar).is_none());
    }
}
