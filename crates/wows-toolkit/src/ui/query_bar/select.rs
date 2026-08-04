//! Selection over the token stream's node paths, and the tree edits a selection
//! authorises. Pure so the boolean-editing rules are testable without egui.

use jiff::Timestamp;
use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;
use wowsunpack::game_types::GameMode;

use crate::db::index::query_ast::DivisionScope;
use crate::db::index::query_ast::Expr;
use crate::db::index::query_ast::MatchExpr;
use crate::db::index::query_ast::MatchTerm;
use crate::db::index::query_ast::Op;
use crate::db::index::query_ast::Quant;
use crate::db::index::query_ast::ShipClass;
use crate::db::index::query_ast::Value;
use crate::db::index::query_ast::ValueKind;
use crate::db::index::query_text;
use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::SourceId;
use crate::db::index::rows::VehicleRelation;
use crate::ui::query_bar::label;
use crate::ui::query_bar::seed;
use crate::ui::query_bar::suggest;
use crate::ui::query_bar::suggest::Scope;
use crate::ui::query_bar::suggest::TermField;
use crate::ui::query_bar::tokens::NodePath;
use crate::ui::query_bar::tokens::Token;
use crate::ui::query_bar::tokens::TokenKind;

/// The nodes the user has selected, by path. Order is not significant.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selection {
    pub nodes: Vec<NodePath>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn contains(&self, path: &[usize]) -> bool {
        self.nodes.iter().any(|p| p.as_slice() == path)
    }

    pub fn toggle(&mut self, path: NodePath) {
        if let Some(i) = self.nodes.iter().position(|p| *p == path) {
            self.nodes.remove(i);
        } else {
            self.nodes.push(path);
        }
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    /// Discards whatever was selected and selects `path` alone.
    pub fn set_one(&mut self, path: NodePath) {
        self.nodes.clear();
        self.nodes.push(path);
    }

    /// Discards whatever was selected and selects exactly `paths`.
    pub fn set_many(&mut self, paths: Vec<NodePath>) {
        self.nodes = paths;
    }

    /// Drops selected paths that no longer name a token, which is every path
    /// below a node an edit removed or reshaped.
    pub fn retain_present(&mut self, tokens: &[Token]) {
        self.nodes.retain(|p| tokens.iter().any(|t| t.path == *p));
    }
}

/// Every pill's node path, in stream order. Pills are the unit the caret steps
/// through and the unit the toolbar acts on; a bracket or a connector names the
/// same node its contents sit under, so including those would offer one node
/// twice.
pub fn pill_paths(tokens: &[Token]) -> Vec<NodePath> {
    tokens.iter().filter(|t| matches!(t.kind, TokenKind::Pill { .. })).map(|t| t.path.clone()).collect()
}

/// The pills the selection may act on: those whose path names a node of the
/// match tree. A pill inside a roster predicate draws inside its quantifier's
/// bracket but has no match-level node to group, negate, or delete, so letting
/// the caret step onto it would offer edits that quietly do nothing.
pub fn selectable_paths(expr: &MatchExpr, tokens: &[Token]) -> Vec<NodePath> {
    pill_paths(tokens).into_iter().filter(|p| addresses_match_node(expr, p)).collect()
}

/// True when `path` names a node of the `MatchExpr` itself. A path that
/// continues past a leaf into a roster predicate does not: that predicate is a
/// separate tree, and no match-level edit can address anything inside it.
pub fn addresses_match_node(expr: &MatchExpr, path: &[usize]) -> bool {
    match split_at_leaf(expr, path) {
        Some((_, rest)) => rest.is_empty(),
        None => expr_at(expr, path).is_some(),
    }
}

/// The pill one step from `anchor` in stream order. With no anchor, stepping
/// back lands on the last pill (the caret sits after it) and stepping forward
/// goes nowhere, since there is nothing after the caret. `None` when there is
/// no such pill.
pub fn step(paths: &[NodePath], anchor: Option<&[usize]>, back: bool) -> Option<NodePath> {
    let Some(anchor) = anchor else {
        return if back { paths.last().cloned() } else { None };
    };
    let at = paths.iter().position(|p| p.as_slice() == anchor)?;
    let next = if back { at.checked_sub(1)? } else { at + 1 };
    paths.get(next).cloned()
}

/// The inclusive run of pills between `anchor` and `target`, in stream order.
/// Empty when either path is not a pill in this stream.
pub fn range(paths: &[NodePath], anchor: &[usize], target: &[usize]) -> Vec<NodePath> {
    let (Some(a), Some(b)) =
        (paths.iter().position(|p| p.as_slice() == anchor), paths.iter().position(|p| p.as_slice() == target))
    else {
        return Vec::new();
    };
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    paths[lo..=hi].to_vec()
}

/// Adds `node` as a new top-level conjunct. Only an `All` root absorbs it
/// directly; any other root is first wrapped in a new `All`, since pushing into
/// an `Any` root would OR the new filter into the query instead of narrowing it.
pub fn append_top_level(expr: &mut MatchExpr, node: MatchExpr) {
    match expr {
        Expr::All(cs) => cs.push(node),
        other => {
            let taken = std::mem::replace(other, Expr::All(Vec::new()));
            *other = Expr::All(vec![taken, node]);
        }
    }
}

/// The path of the node `append_top_level` last added, read after
/// `canonicalise` has run over the tree.
///
/// `append_top_level` always leaves an `All` root and pushes onto its end, and
/// canonicalising a conjunction only drops children that collapsed away and
/// replaces a one-child node with that child. So the appended node is either the
/// whole tree, when it was the only survivor, or still the last child of the
/// root.
pub fn last_appended_path(expr: &MatchExpr) -> NodePath {
    match expr {
        Expr::All(cs) | Expr::Any(cs) if !cs.is_empty() => vec![cs.len() - 1],
        _ => Vec::new(),
    }
}

/// A fresh term for `field`, ready to be appended: the field's leading allowed
/// operator and an in-kind placeholder value, under `scope` when the field is a
/// roster one. `None` for a field with no grammar spelling, which is the same
/// refusal the two prefix functions already make.
///
/// Built by parsing the grammar text that spells the term, so a term minted by
/// picking a field and one typed by hand cannot differ. That is also what keeps
/// a scope's expansion -- `enemy.` is a relation constraint standing beside the
/// term, not a part of it -- in the parser alone rather than restated here.
///
/// The value is `placeholder_value`'s arbitrary in-kind value, so the caller
/// carries the obligation that function names: open the value editor on the new
/// term, because nothing about the placeholder is a choice the user made.
pub fn new_term(field: TermField, scope: Option<Scope>) -> Option<MatchExpr> {
    let prefix = match (field, scope) {
        (TermField::Match(f), _) => suggest::match_field_prefix(f),
        (TermField::Roster(f), Some(scope)) => suggest::roster_field_prefix(f, scope),
        (TermField::Roster(_), None) => None,
    }?;
    let literal = query_text::print_value(&placeholder_value(field.value_kind()));
    query_text::parse_query(&format!("{prefix}{literal}")).ok().filter(|parsed| !parsed.is_empty_all())
}

/// Adds a freshly parsed query to the tree. An `All` root is spliced rather
/// than nested: two terms typed into the caret mean two filters, not one
/// bracketed group.
pub fn append_query(expr: &mut MatchExpr, parsed: MatchExpr) {
    match parsed {
        Expr::All(cs) => {
            for child in cs {
                append_top_level(expr, child);
            }
        }
        other => append_top_level(expr, other),
    }
}

/// Switches the `All`/`Any` node at `path` to the other connector, keeping its
/// children in place. A node that is neither is left alone.
pub fn set_connector(expr: &mut MatchExpr, path: &[usize], is_or: bool) {
    let Some(node) = node_at_mut(expr, path) else {
        return;
    };
    let children = match node {
        Expr::All(cs) | Expr::Any(cs) => std::mem::take(cs),
        _ => return,
    };
    *node = if is_or { Expr::Any(children) } else { Expr::All(children) };
}

/// The operators a pill's term allows, and the one it currently carries.
/// `None` for a pill that renders more than one term (a sugar-collapsed roster
/// quantifier) or none at all, neither of which has a single operator to offer.
pub fn term_op_at(expr: &MatchExpr, path: &[usize]) -> Option<(&'static [Op], Op)> {
    let (term, rest) = split_at_leaf(expr, path)?;
    match term {
        MatchTerm::Field(field, op, _) if rest.is_empty() => Some((field.allowed_ops(), *op)),
        MatchTerm::Roster { pred, .. } => match expr_at(pred, rest)? {
            Expr::Leaf(roster) => Some((roster.field.allowed_ops(), roster.op)),
            _ => None,
        },
        _ => None,
    }
}

/// The field, operator, and value of the term a segment path names. `None` for
/// a path that names no single term, which is every path that stops on a roster
/// quantifier whose predicate has more than one leaf. `segment_path` is what
/// turns a pill's own path into one this accepts.
pub fn term_at<'a>(expr: &'a MatchExpr, path: &[usize]) -> Option<(TermField, Op, &'a Value)> {
    let (term, rest) = split_at_leaf(expr, path)?;
    match term {
        MatchTerm::Field(field, op, value) if rest.is_empty() => Some((TermField::Match(*field), *op, value)),
        MatchTerm::Roster { pred, .. } => match expr_at(pred, rest)? {
            Expr::Leaf(roster) => Some((TermField::Roster(roster.field), roster.op, &roster.value)),
            _ => None,
        },
        _ => None,
    }
}

/// The node a path names, over the match tree alone. `None` for a path that
/// walks past a `Roster` leaf into its predicate, which is a different tree and
/// carries no `MatchExpr` to hand back.
pub fn node_at<'a>(expr: &'a MatchExpr, path: &[usize]) -> Option<&'a MatchExpr> {
    expr_at(expr, path)
}

/// Replaces the node at `path` with `node`, but only while `expected` is still
/// what sits there. Reports whether the write took.
///
/// A numeric path names a position, not a node, so any rewrite between the
/// moment a path is taken and the moment it is used can move a different node
/// under it. Matching against the node the caller started from is what makes the
/// write land on that node or on nothing at all.
///
/// The root is a node like any other here: an empty `path` replaces the whole
/// tree, with no parent lookup to be absent at.
pub fn replace_node(expr: &mut MatchExpr, path: &[usize], expected: &MatchExpr, node: MatchExpr) -> bool {
    let Some(slot) = node_at_mut(expr, path) else {
        return false;
    };
    if slot != expected {
        return false;
    }
    *slot = node;
    true
}

/// What a value written back through `commit_value` is anchored to: the path of
/// the `MatchExpr` leaf carrying the term, the node sitting at that path now,
/// and the tail addressing the term inside that leaf's roster predicate.
///
/// The leaf rather than the term itself, because only a whole `MatchExpr` can be
/// gated on: `replace_node` compares what it is about to overwrite against the
/// node the caller started from, and a `RosterTerm` buried in a predicate is not
/// a node the match tree can name.
pub fn leaf_seed(expr: &MatchExpr, path: &[usize]) -> Option<(NodePath, MatchExpr, NodePath)> {
    let (_, rest) = split_at_leaf(expr, path)?;
    // `split_at_leaf` walks `path` from the front and hands back an unconsumed
    // suffix of it, so what it consumed is the prefix of that length.
    let leaf = path[..path.len() - rest.len()].to_vec();
    let seed = node_at(expr, &leaf)?.clone();
    Some((leaf, seed, rest.to_vec()))
}

/// Writes `value` onto the term `tail` names inside `seed`, and writes the
/// result back only while `seed` is still the node at `leaf`.
///
/// The edit is applied to a clone of the seed rather than to a position in the
/// live tree, so nothing here re-resolves an index that a rewrite could have
/// moved a different node under; the one index used is `leaf`, and
/// `replace_node` refuses it unless it still names the node the caller opened.
pub fn commit_value(expr: &mut MatchExpr, leaf: &[usize], seed: &MatchExpr, tail: &[usize], value: Value) -> bool {
    let mut updated = seed.clone();
    if !set_value(&mut updated, tail, value) {
        return false;
    }
    replace_node(expr, leaf, seed, updated)
}

/// The path a segment edit acts on, given the path of the pill that was
/// clicked. `None` for a pill whose segments address no editable term.
///
/// Two kinds of pill leave the tail empty while naming a `Roster` leaf, which
/// carries no single field of its own, and both are redirected into the
/// predicate. A sugar-collapsed pill draws the quantifier's inner term ("Enemy
/// ship is Yamato"); a bracketed quantifier over a one-leaf predicate draws its
/// single inner pill on the quantifier's own path, and that leaf is the
/// predicate root.
///
/// `MatchTerm::FreeText` has no field or operator at all and yields `None`,
/// which is what keeps its one segment from being a click that does nothing.
pub fn segment_path(expr: &MatchExpr, path: &NodePath) -> Option<NodePath> {
    let (term, rest) = split_at_leaf(expr, path)?;
    match term {
        MatchTerm::Field(..) if rest.is_empty() => Some(path.clone()),
        MatchTerm::Roster { quant, pred } if rest.is_empty() => {
            let inner =
                label::sugar_inner_path(*quant, pred).or_else(|| matches!(pred, Expr::Leaf(_)).then(Vec::new))?;
            let mut extended = path.clone();
            extended.extend(inner);
            Some(extended)
        }
        MatchTerm::Roster { pred, .. } => matches!(expr_at(pred, rest), Some(Expr::Leaf(_))).then(|| path.clone()),
        _ => None,
    }
}

/// Whether `set_op` would take, so an operator menu can disable an entry rather
/// than offer one that silently does nothing.
pub fn can_set_op(expr: &MatchExpr, path: &[usize], op: Op) -> bool {
    let Some((allowed, current)) = term_op_at(expr, path) else {
        return false;
    };
    allowed.contains(&op) && (op.is_nullary() || !current.is_nullary())
}

/// Replaces the operator of the term at `path`, reporting whether it took.
///
/// Refuses an operator the field does not allow: `Op` spells equals three ways
/// and all three print identically, so an unchecked assignment yields a tree
/// that reparses into a different one. Also refuses moving from a nullary
/// operator to one that takes an operand, because the term has no value left to
/// compare and inventing a placeholder is exactly the sentinel the model avoids.
pub fn set_op(expr: &mut MatchExpr, path: &[usize], op: Op) -> bool {
    let Some((term, rest)) = split_at_leaf_mut(expr, path) else {
        return false;
    };
    match term {
        MatchTerm::Field(field, current, value) if rest.is_empty() => apply_op(field.allowed_ops(), current, value, op),
        MatchTerm::Roster { pred, .. } => match expr_at_mut(pred, rest) {
            Some(Expr::Leaf(roster)) => apply_op(roster.field.allowed_ops(), &mut roster.op, &mut roster.value, op),
            _ => false,
        },
        _ => false,
    }
}

/// Replaces the value of the term at `path`, reporting whether it took.
///
/// Refuses a value whose kind the field does not carry, so a picked literal
/// that parsed against the wrong field cannot land in the tree, and refuses any
/// value at all on a nullary operator, which has no right-hand side to hold it.
pub fn set_value(expr: &mut MatchExpr, path: &[usize], value: Value) -> bool {
    let Some((term, rest)) = split_at_leaf_mut(expr, path) else {
        return false;
    };
    match term {
        MatchTerm::Field(field, op, current) if rest.is_empty() => apply_value(field.value_kind(), *op, current, value),
        MatchTerm::Roster { pred, .. } => match expr_at_mut(pred, rest) {
            Some(Expr::Leaf(roster)) => apply_value(roster.field.value_kind(), roster.op, &mut roster.value, value),
            _ => false,
        },
        _ => false,
    }
}

fn apply_value(kind: ValueKind, op: Op, current: &mut Value, value: Value) -> bool {
    if op.is_nullary() || value_kind_of(&value) != Some(kind) {
        return false;
    }
    *current = value;
    true
}

fn apply_op(allowed: &[Op], current: &mut Op, value: &mut Value, op: Op) -> bool {
    if !allowed.contains(&op) {
        return false;
    }
    if op.is_nullary() {
        *current = op;
        *value = Value::NoOperand;
        return true;
    }
    if current.is_nullary() {
        return false;
    }
    *current = op;
    true
}

/// Changes the field of the term at `path`, re-deriving its operator and
/// value so the term stays legal for the new field. Reports whether `path`
/// addressed a field term of the kind `field` names: `false` both when
/// `path` addresses no field term at all and when it addresses one whose
/// kind (`Match` vs `Roster`) does not match `field`'s. This is `true` even
/// when the new field is the same as the old one (nothing to re-derive), so
/// a caller does not report an edit that did not happen -- it is not a
/// report that anything changed.
///
/// The operator is kept when the new field still allows it; otherwise it is
/// replaced by that field's own equality spelling, chosen by class through
/// `seed::seed_op` rather than by naming an `Op` literal here -- `Op` spells
/// equality three ways and all three print identically, so picking the wrong
/// one silently reparses into a different tree.
///
/// The value is kept when it already matches the new field's `ValueKind`;
/// otherwise it is replaced by `placeholder_value(kind)`. That placeholder is
/// an arbitrary in-kind value, not a marker: nothing here or downstream may
/// read it as "the user has not chosen a value yet".
///
/// A caller that needs to know whether the value was reset compares the
/// `Value` itself, before and after. Comparing the two fields' declared
/// `value_kind()`s is **not** equivalent and misses real cases: a nullary
/// operator carries `Value::NoOperand`, which belongs to no kind at all, so
/// retargeting `realm is set` at `name` -- both `Text` -- still replaces the
/// value here.
pub fn set_field(expr: &mut MatchExpr, path: &NodePath, field: TermField) -> bool {
    let Some((term, rest)) = split_at_leaf_mut(expr, path) else {
        return false;
    };
    match (term, field) {
        (MatchTerm::Field(current, op, value), TermField::Match(new_field)) if rest.is_empty() => {
            *current = new_field;
            reconcile_term(op, value, new_field.allowed_ops(), new_field.value_kind());
            true
        }
        (MatchTerm::Roster { pred, .. }, TermField::Roster(new_field)) => match expr_at_mut(pred, rest) {
            Some(Expr::Leaf(roster)) => {
                roster.field = new_field;
                reconcile_term(&mut roster.op, &mut roster.value, new_field.allowed_ops(), new_field.value_kind());
                true
            }
            _ => false,
        },
        _ => false,
    }
}

/// Brings an operator/value pair back into a field's own rules after the
/// field itself changed underneath them.
///
/// The replacement operator comes from `seed::seed_op`, the same chooser the
/// app's built-in queries go through, rather than from a second copy of the
/// preference list: `Op` spells equality three ways and all three print
/// identically, so two lists that drifted would build a term that reparses into
/// a different tree.
fn reconcile_term(op: &mut Op, value: &mut Value, allowed: &'static [Op], kind: ValueKind) {
    if !allowed.contains(op) {
        *op = seed::seed_op(allowed, seed::Wanted::Equality);
    }
    if op.is_nullary() {
        *value = Value::NoOperand;
    } else if value_kind_of(value) != Some(kind) {
        *value = placeholder_value(kind);
    }
}

/// The `ValueKind` a `Value` carries, or `None` for `NoOperand`, which is not
/// in any kind's family.
fn value_kind_of(value: &Value) -> Option<ValueKind> {
    match value {
        Value::Text(_) => Some(ValueKind::Text),
        Value::Int(_) => Some(ValueKind::Int),
        Value::Float(_) => Some(ValueKind::Float),
        Value::Bool(_) => Some(ValueKind::Bool),
        Value::Outcome(_) => Some(ValueKind::Outcome),
        Value::Relation(_) => Some(ValueKind::Relation),
        Value::Division(_) => Some(ValueKind::Division),
        Value::Class(_) => Some(ValueKind::Class),
        Value::Ship(_) => Some(ValueKind::Ship),
        Value::Account(_) => Some(ValueKind::Account),
        Value::Source(_) => Some(ValueKind::Source),
        Value::Timestamp(_) => Some(ValueKind::Timestamp),
        Value::GameMode(_) => Some(ValueKind::GameMode),
        Value::NoOperand => None,
    }
}

/// A deterministic placeholder for a value of `kind`, used only to replace a
/// value whose kind no longer matches its field. Empty text, zero for a
/// number or an ID, the epoch for a timestamp, and the first declared variant
/// for an enum -- except `GameMode`, whose placeholder is the first *offerable*
/// variant (`GameMode::Invalid` cannot be offered; see `GameMode::is_offerable`),
/// so the placeholder stays a value the dropdown can also produce. This is an
/// arbitrary in-kind value, never a sentinel for "absent" -- `Outcome`'s
/// placeholder is `Win`, a value a user can also choose on purpose, so
/// nothing may treat it as meaning "unchosen".
///
/// `suggest::no_preset_carries_a_placeholder_value` asserts the opposite for
/// a preset's `Value::Ship`/`Value::Account`: no zero id there. The two do
/// not contradict. A preset is a complete, shipped tree, so a zero id in one
/// would be a resting bug the user could commit unchanged. A `Ship` or
/// `Account` placeholder minted here exists only inside a live edit, and
/// `QueryBar::commit_field_change` opens the value segment's editor whenever
/// the `Value` it snapshotted before the retarget is not the one that came
/// back, which is exactly when a placeholder was minted.
///
/// That caller compares the `Value`, not the two fields' declared
/// `value_kind()`s. The kind comparison looks equivalent and is not: it misses
/// every retarget off a nullary operator, whose `Value::NoOperand` belongs to
/// no kind, and a caller written that way commits a placeholder unseen.
fn placeholder_value(kind: ValueKind) -> Value {
    match kind {
        ValueKind::Text => Value::Text(String::new()),
        ValueKind::Int => Value::Int(0),
        ValueKind::Float => Value::Float(0.0),
        ValueKind::Bool => Value::Bool(false),
        ValueKind::Outcome => Value::Outcome(MatchOutcome::Win),
        ValueKind::Relation => Value::Relation(VehicleRelation::SelfPlayer),
        ValueKind::Division => Value::Division(DivisionScope::ALL[0]),
        ValueKind::Class => Value::Class(ShipClass::ALL[0]),
        ValueKind::Ship => Value::Ship(GameParamId::from(0u64)),
        ValueKind::Account => Value::Account(AccountId(0)),
        ValueKind::Source => Value::Source(SourceId(0)),
        ValueKind::Timestamp => Value::Timestamp(Timestamp::from_second(0).unwrap()),
        ValueKind::GameMode => Value::GameMode(
            GameMode::ALL.into_iter().find(|m| m.is_offerable()).expect("at least one mode is offerable"),
        ),
    }
}

/// Resolves the `MatchExpr` part of a token path down to its leaf term, and
/// hands back the unconsumed tail, which addresses a node inside that leaf's
/// roster predicate.
fn split_at_leaf<'a, 'p>(expr: &'a MatchExpr, path: &'p [usize]) -> Option<(&'a MatchTerm, &'p [usize])> {
    match expr {
        Expr::Leaf(term) => Some((term, path)),
        Expr::All(cs) | Expr::Any(cs) => {
            let (&i, rest) = path.split_first()?;
            split_at_leaf(cs.get(i)?, rest)
        }
        Expr::Not(inner) => {
            let (&i, rest) = path.split_first()?;
            if i != 0 {
                return None;
            }
            split_at_leaf(inner, rest)
        }
    }
}

fn split_at_leaf_mut<'a, 'p>(expr: &'a mut MatchExpr, path: &'p [usize]) -> Option<(&'a mut MatchTerm, &'p [usize])> {
    match expr {
        Expr::Leaf(term) => Some((term, path)),
        Expr::All(cs) | Expr::Any(cs) => {
            let (&i, rest) = path.split_first()?;
            split_at_leaf_mut(cs.get_mut(i)?, rest)
        }
        Expr::Not(inner) => {
            let (&i, rest) = path.split_first()?;
            if i != 0 {
                return None;
            }
            split_at_leaf_mut(inner, rest)
        }
    }
}

/// Resolves a node path over either tree level, so a roster predicate resolves
/// the same way the match level does. The resolver every edit goes through:
/// `split_at_leaf` stops at the `MatchTerm` and hands the unconsumed tail here,
/// which is how a path that continues into a roster predicate keeps walking.
fn expr_at<'a, L>(expr: &'a Expr<L>, path: &[usize]) -> Option<&'a Expr<L>> {
    let Some((&i, rest)) = path.split_first() else {
        return Some(expr);
    };
    match expr {
        Expr::All(cs) | Expr::Any(cs) => cs.get(i).and_then(|c| expr_at(c, rest)),
        Expr::Not(inner) if i == 0 => expr_at(inner, rest),
        _ => None,
    }
}

fn expr_at_mut<'a, L>(expr: &'a mut Expr<L>, path: &[usize]) -> Option<&'a mut Expr<L>> {
    let Some((&i, rest)) = path.split_first() else {
        return Some(expr);
    };
    match expr {
        Expr::All(cs) | Expr::Any(cs) => cs.get_mut(i).and_then(|c| expr_at_mut(c, rest)),
        Expr::Not(inner) if i == 0 => expr_at_mut(inner, rest),
        _ => None,
    }
}

/// The parent path and child index a node path names, when it is not the root.
fn parent_and_index(path: &[usize]) -> Option<(&[usize], usize)> {
    let (&last, rest) = path.split_last()?;
    Some((rest, last))
}

/// The children vector of the `All`/`Any` node at `parent_path`, if that node
/// exists and is a conjunction/disjunction.
fn children_mut<'a>(expr: &'a mut MatchExpr, parent_path: &[usize]) -> Option<&'a mut Vec<MatchExpr>> {
    let node = node_at_mut(expr, parent_path)?;
    match node {
        Expr::All(cs) | Expr::Any(cs) => Some(cs),
        _ => None,
    }
}

fn node_at_mut<'a>(expr: &'a mut MatchExpr, path: &[usize]) -> Option<&'a mut MatchExpr> {
    let Some((&i, rest)) = path.split_first() else {
        return Some(expr);
    };
    match expr {
        Expr::All(cs) | Expr::Any(cs) => cs.get_mut(i).and_then(|c| node_at_mut(c, rest)),
        Expr::Not(inner) if i == 0 => node_at_mut(inner, rest),
        _ => None,
    }
}

/// True when every selected node shares one parent, and there are at least
/// two of them. A parent is always uniform (`All` or `Any`), so the selected
/// nodes can be moved together into a new group without changing what any of
/// them mean.
pub fn can_group(_expr: &MatchExpr, sel: &Selection) -> bool {
    if sel.nodes.len() < 2 {
        return false;
    }
    let mut parents = sel.nodes.iter().filter_map(|p| parent_and_index(p).map(|(parent, _)| parent));
    let Some(first) = parents.next() else {
        return false;
    };
    parents.all(|p| p == first)
}

/// Wraps the selected siblings in a new `All`/`Any` node in place of their old
/// position. Assumes `can_group` returned true for this selection.
pub fn group(expr: &mut MatchExpr, sel: &Selection, is_or: bool) {
    if sel.nodes.is_empty() {
        return;
    }
    let Some((parent_path, _)) = parent_and_index(&sel.nodes[0]) else {
        return;
    };
    let mut indices: Vec<usize> = sel.nodes.iter().filter_map(|p| parent_and_index(p).map(|(_, i)| i)).collect();
    indices.sort_unstable();
    indices.dedup();

    // The first path had a parent, so at least one index survived; taking it as
    // an option rather than by subscript keeps that reasoning off a panic path.
    let Some(&insert_at) = indices.first() else {
        return;
    };
    let Some(children) = children_mut(expr, parent_path) else {
        return;
    };
    let mut taken = Vec::with_capacity(indices.len());
    for &i in indices.iter().rev() {
        taken.push(children.remove(i));
    }
    taken.reverse();
    let new_node = if is_or { Expr::Any(taken) } else { Expr::All(taken) };
    children.insert(insert_at, new_node);
}

/// Splices the children of the `All`/`Any` node at `path` into its parent, in
/// place of that node. Reports whether it took, so a caller does not report an
/// edit that did not happen.
///
/// The root is refused: it has no parent to splice into, and the printer
/// already suppresses its brackets, so there is no group there to dissolve.
/// A node whose parent is a `Not` is refused for the same reason in reverse --
/// a `Not` holds exactly one operand and cannot absorb several.
pub fn ungroup(expr: &mut MatchExpr, path: &NodePath) -> bool {
    let Some((parent_path, index)) = parent_and_index(path) else {
        return false;
    };
    // Checked before the children are taken, so a parent that cannot absorb
    // them leaves the group exactly as it was rather than emptied.
    if children_mut(expr, parent_path).is_none() {
        return false;
    }
    let Some(node) = node_at_mut(expr, path) else {
        return false;
    };
    let taken = match node {
        Expr::All(cs) | Expr::Any(cs) => std::mem::take(cs),
        _ => return false,
    };
    let Some(children) = children_mut(expr, parent_path) else {
        return false;
    };
    children.remove(index);
    for (offset, child) in taken.into_iter().enumerate() {
        children.insert(index + offset, child);
    }
    true
}

/// Negates the node at `path` in place: flips a leaf's operator or quantifier
/// when an inverse exists that the field still allows, unwraps an existing
/// `Not`, and otherwise wraps the node in `Not`.
///
/// Resolved through `node_at_mut` rather than through the parent's children,
/// because every branch here rewrites the node itself and needs no sibling.
/// That is also what lets the root be negated: a canonical one-pill query is a
/// bare `Leaf` whose path is empty, and a query that is only a `Not` gives its
/// pill the path `[0]`, neither of which names a child of an `All`/`Any`.
pub fn negate(expr: &mut MatchExpr, path: &NodePath) {
    let Some(node) = node_at_mut(expr, path) else {
        return;
    };

    match node {
        Expr::Not(inner) => {
            let inner = std::mem::replace(inner.as_mut(), Expr::All(Vec::new()));
            *node = inner;
        }
        Expr::Leaf(MatchTerm::Field(field, op, _)) => {
            if let Some(inv) = op.inverse()
                && field.allowed_ops().contains(&inv)
            {
                *op = inv;
                return;
            }
            let taken = std::mem::replace(node, Expr::All(Vec::new()));
            *node = Expr::Not(Box::new(taken));
        }
        Expr::Leaf(MatchTerm::Roster { quant, .. }) => {
            let inverted: Quant = quant.inverse();
            *quant = inverted;
        }
        _ => {
            let taken = std::mem::replace(node, Expr::All(Vec::new()));
            *node = Expr::Not(Box::new(taken));
        }
    }
}

/// Removes every selected node. Removes in descending path order (deepest,
/// highest index first) so earlier removals never shift the index of a later
/// one still to be removed. The root sorts last under that order, so a
/// selection that names it takes effect after the rest and leaves the same
/// empty query either way.
pub fn delete(expr: &mut MatchExpr, sel: &Selection) {
    let mut paths = sel.nodes.clone();
    paths.sort_by(|a, b| b.cmp(a));
    for path in paths {
        remove_node(expr, &path);
    }
}

/// Detaches the node at `path` from whatever holds it.
///
/// The root is held by nothing, so removing it leaves the canonical empty
/// query. A `Not`'s only operand cannot simply be dropped, since that would
/// leave a negation of nothing; it becomes an empty conjunction, which
/// `canonicalise` then drops along with the `Not` above it. Both matter for a
/// canonical tree: a one-pill query is a bare `Leaf` at the root, and
/// `-outcome:win` puts its only pill under a root `Not`.
fn remove_node(expr: &mut MatchExpr, path: &[usize]) {
    let Some((parent_path, index)) = parent_and_index(path) else {
        *expr = Expr::All(Vec::new());
        return;
    };
    let Some(parent) = node_at_mut(expr, parent_path) else {
        return;
    };
    match parent {
        Expr::All(cs) | Expr::Any(cs) => {
            if index < cs.len() {
                cs.remove(index);
            }
        }
        Expr::Not(inner) if index == 0 => **inner = Expr::All(Vec::new()),
        Expr::Not(_) | Expr::Leaf(_) => {}
    }
}

/// Enforces the two invariants Plan A's printer requires: no `All`/`Any` with
/// exactly one child (it would print as a bare term and reparse as `Leaf`,
/// silently losing the group -- this applies at the root too, since the
/// printer suppresses the root's own brackets the same way), and no
/// `All`/`Any` with zero children except a root left as the canonical empty
/// query. Bottom-up and idempotent: re-running it on its own output is a
/// no-op, since every remaining `All`/`Any` already has zero (root only) or at
/// least two children.
///
/// This deliberately does not preserve `Expr`'s own boolean semantics for an
/// emptied `Any`: per its doc comment, `Any([])` denotes "matches nothing", so
/// `All([Any([]), x])` is formally "nothing AND x", i.e. "matches nothing" --
/// yet this function reduces it to `x`, not to the canonical false shape.
/// That is intentional. In an interactive builder, deleting the last pill out
/// of an OR-subgroup means "remove this group", not "poison the query to
/// match zero rows": `win and (map:ocean or map:north)` with both map pills
/// deleted should read as `win`, not silently become unsatisfiable. The same
/// reasoning covers the root: a fully emptied tree canonicalises to
/// `All(vec![])`, which matches everything, because a cleared query bar means
/// "no filter", not "no results". Because every edit function in this module
/// calls this afterward, an empty `Any` never reaches `query_sql`, where it
/// would otherwise compile to the literal `1=0`.
pub fn canonicalise(expr: &mut MatchExpr) {
    let taken = std::mem::replace(expr, Expr::All(Vec::new()));
    *expr = canonicalise_node(taken).unwrap_or(Expr::All(Vec::new()));
}

/// Canonicalises one node bottom-up. Returns `None` when the node collapsed
/// to nothing (an empty `All`/`Any`), for the caller to drop from its own
/// children; the top-level `canonicalise` substitutes the canonical empty
/// query when the whole tree collapses this way.
fn canonicalise_node(expr: MatchExpr) -> Option<MatchExpr> {
    match expr {
        Expr::Leaf(l) => Some(Expr::Leaf(l)),
        Expr::Not(inner) => Some(Expr::Not(Box::new(canonicalise_node(*inner)?))),
        Expr::All(cs) => canonicalise_conjunction(cs, false),
        Expr::Any(cs) => canonicalise_conjunction(cs, true),
    }
}

/// Canonicalises every child, drops any that collapsed away entirely, and
/// then collapses this node itself to its sole surviving child when exactly
/// one remains -- whether it started that way or was only reduced to it by a
/// sibling collapsing away. A node with zero surviving children returns
/// `None` for the caller to drop.
fn canonicalise_conjunction(cs: Vec<MatchExpr>, is_or: bool) -> Option<MatchExpr> {
    let mut out: Vec<MatchExpr> = cs.into_iter().filter_map(canonicalise_node).collect();
    match out.len() {
        0 => None,
        1 => out.pop(),
        _ => Some(if is_or { Expr::Any(out) } else { Expr::All(out) }),
    }
}

/// Drops terms that constrain nothing, and canonicalises what is left.
///
/// An empty-text `Contains` compiles to `LIKE '%'`, which matches every
/// non-NULL row, so a term the user has not finished would *widen* the search
/// rather than narrow it. The Search tab compiles this rather than the tree it
/// shows, so a half-written term costs nothing until it says something.
///
/// This is not a logic-preserving rewrite and does not try to be. `not map:""`
/// formally matches nothing, because the term it negates matches everything;
/// here the whole thing is dropped, on the reading that an unfinished term is
/// one the user has not asked for yet rather than one whose vacuous truth value
/// should be honoured. `canonicalise` makes the same choice for an emptied
/// `Any`, for the same reason.
///
/// Roster predicates are left alone. A quantifier over an emptied predicate is
/// a different assertion, not a no-op -- `no(...)` over nothing asks whether the
/// roster is empty -- so there is no removal here that is safe by inspection.
pub fn prune_empty(expr: &MatchExpr) -> MatchExpr {
    let mut out = prune_node(expr).unwrap_or_default();
    canonicalise(&mut out);
    out
}

/// `None` when nothing under this node constrains anything.
fn prune_node(expr: &MatchExpr) -> Option<MatchExpr> {
    match expr {
        Expr::Leaf(term) => (!is_vacuous(term)).then(|| expr.clone()),
        Expr::Not(inner) => prune_node(inner).map(|kept| Expr::Not(Box::new(kept))),
        Expr::All(children) => prune_children(children).map(Expr::All),
        Expr::Any(children) => prune_children(children).map(Expr::Any),
    }
}

fn prune_children(children: &[MatchExpr]) -> Option<Vec<MatchExpr>> {
    let kept: Vec<MatchExpr> = children.iter().filter_map(prune_node).collect();
    (!kept.is_empty()).then_some(kept)
}

/// True for a term whose SQL would match every row it is asked about.
fn is_vacuous(term: &MatchTerm) -> bool {
    match term {
        MatchTerm::Field(_, Op::Contains, Value::Text(s)) => s.is_empty(),
        MatchTerm::FreeText(s) => s.is_empty(),
        MatchTerm::Field(..) | MatchTerm::Roster { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::index::query_ast::Expr;
    use crate::db::index::query_ast::MatchField;
    use crate::db::index::query_ast::MatchTerm;
    use crate::db::index::query_ast::Op;
    use crate::db::index::query_ast::Value;
    use crate::db::index::rows::MatchOutcome;

    fn leaf_term(n: i64) -> MatchTerm {
        MatchTerm::Field(MatchField::Build, Op::Eq, Value::Int(n))
    }
    fn leaf(n: i64) -> MatchExpr {
        Expr::Leaf(leaf_term(n))
    }
    fn sel(paths: &[&[usize]]) -> Selection {
        Selection { nodes: paths.iter().map(|p| p.to_vec()).collect() }
    }

    #[test]
    fn grouping_siblings_wraps_them_in_a_new_node() {
        let mut e = Expr::All(vec![leaf(1), leaf(2), leaf(3)]);
        assert!(can_group(&e, &sel(&[&[0], &[1]])));
        group(&mut e, &sel(&[&[0], &[1]]), true);
        match &e {
            Expr::All(cs) => {
                assert_eq!(cs.len(), 2);
                assert!(matches!(cs[0], Expr::Any(ref inner) if inner.len() == 2), "got {:?}", cs[0]);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn grouping_non_contiguous_siblings_is_allowed_and_moves_them_together() {
        let mut e = Expr::All(vec![leaf(1), leaf(2), leaf(3)]);
        assert!(can_group(&e, &sel(&[&[0], &[2]])));
        group(&mut e, &sel(&[&[0], &[2]]), false);
        match &e {
            Expr::All(cs) => {
                assert_eq!(cs.len(), 2);
                let grouped = cs.iter().find(|c| matches!(c, Expr::All(inner) if inner.len() == 2)).expect("the group");
                match grouped {
                    Expr::All(inner) => {
                        assert_eq!(inner[0], leaf(1));
                        assert_eq!(inner[1], leaf(3));
                    }
                    other => panic!("got {other:?}"),
                }
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn grouping_across_different_parents_is_refused() {
        let e = Expr::All(vec![leaf(1), Expr::Any(vec![leaf(2), leaf(3)])]);
        assert!(!can_group(&e, &sel(&[&[0], &[1, 0]])));
    }

    #[test]
    fn grouping_fewer_than_two_nodes_is_refused() {
        let e = Expr::All(vec![leaf(1), leaf(2)]);
        assert!(!can_group(&e, &sel(&[&[0]])));
        assert!(!can_group(&e, &sel(&[])));
    }

    #[test]
    fn ungrouping_splices_children_into_the_parent_in_place() {
        let mut e = Expr::All(vec![leaf(1), Expr::Any(vec![leaf(2), leaf(3)]), leaf(4)]);
        assert!(ungroup(&mut e, &vec![1]));
        match &e {
            Expr::All(cs) => {
                assert_eq!(cs.len(), 4);
                assert_eq!(cs[1], leaf(2));
                assert_eq!(cs[2], leaf(3));
                assert_eq!(cs[3], leaf(4));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn negating_a_leaf_with_an_inverse_flips_its_operator_instead_of_wrapping() {
        let mut e: MatchExpr = Expr::All(vec![Expr::Leaf(MatchTerm::Field(
            MatchField::Outcome,
            Op::Is,
            Value::Outcome(MatchOutcome::Win),
        ))]);
        negate(&mut e, &vec![0]);
        match &e {
            Expr::All(cs) => match &cs[0] {
                Expr::Leaf(MatchTerm::Field(_, op, _)) => assert_eq!(*op, Op::IsNot),
                other => panic!("expected an operator flip, got {other:?}"),
            },
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn negating_a_group_wraps_it_in_not() {
        let mut e = Expr::All(vec![Expr::Any(vec![leaf(1), leaf(2)])]);
        negate(&mut e, &vec![0]);
        match &e {
            Expr::All(cs) => assert!(matches!(cs[0], Expr::Not(_)), "got {:?}", cs[0]),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn negating_twice_returns_the_original_tree() {
        for mut e in [Expr::All(vec![leaf(1)]), Expr::All(vec![Expr::Any(vec![leaf(1), leaf(2)])])] {
            let before = e.clone();
            negate(&mut e, &vec![0]);
            negate(&mut e, &vec![0]);
            assert_eq!(e, before);
        }
    }

    #[test]
    fn negating_a_quantifier_flips_it_rather_than_wrapping() {
        let pred = Expr::Leaf(crate::db::index::query_ast::RosterTerm {
            field: crate::db::index::query_ast::RosterField::Tier,
            op: Op::Eq,
            value: Value::Int(10),
        });
        let mut e: MatchExpr = Expr::All(vec![Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred })]);
        negate(&mut e, &vec![0]);
        match &e {
            Expr::All(cs) => match &cs[0] {
                Expr::Leaf(MatchTerm::Roster { quant, .. }) => assert_eq!(*quant, Quant::None),
                other => panic!("got {other:?}"),
            },
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn deleting_removes_every_selected_node_even_when_indices_shift() {
        let mut e = Expr::All(vec![leaf(1), leaf(2), leaf(3), leaf(4)]);
        delete(&mut e, &sel(&[&[0], &[2]]));
        match &e {
            Expr::All(cs) => {
                assert_eq!(cs.len(), 2);
                assert_eq!(cs[0], leaf(2));
                assert_eq!(cs[1], leaf(4));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn canonicalise_collapses_a_single_child_group() {
        // Plan A's printer renders All([x]) as bare x, which reparses to Leaf(x),
        // so a one-condition group must never reach the printer.
        let mut e = Expr::All(vec![Expr::Any(vec![leaf(1)]), leaf(2)]);
        canonicalise(&mut e);
        match &e {
            Expr::All(cs) => assert_eq!(cs[0], leaf(1)),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn canonicalise_removes_an_empty_group_but_keeps_an_empty_root() {
        let mut e = Expr::All(vec![Expr::Any(vec![]), leaf(1)]);
        canonicalise(&mut e);
        assert_eq!(e, leaf(1));

        let mut root: MatchExpr = Expr::All(vec![]);
        canonicalise(&mut root);
        assert_eq!(root, Expr::All(vec![]));
    }

    #[test]
    fn canonicalise_is_idempotent() {
        for mut e in [
            Expr::All(vec![Expr::Any(vec![]), leaf(1)]),
            Expr::All(vec![Expr::Any(vec![leaf(1)]), leaf(2)]),
            Expr::All(vec![leaf(1)]),
            Expr::All(vec![Expr::Any(vec![Expr::All(vec![leaf(1)])])]),
            Expr::All(vec![leaf(1), leaf(2)]),
            Expr::All(vec![]),
        ] {
            canonicalise(&mut e);
            let once = e.clone();
            canonicalise(&mut e);
            assert_eq!(e, once, "canonicalise changed on a second call");
        }
    }

    /// The shape the bar actually holds a one-pill query in. `canonicalise`
    /// collapses a single-child root, so `All([x])` is a tree the widget never
    /// has and a fixture built from one cannot exercise the path the toolbar,
    /// the right-click menu, and Backspace all take.
    #[test]
    fn deleting_the_only_pill_leaves_the_canonical_empty_query() {
        let mut e = Expr::Leaf(leaf_term(1));
        delete(&mut e, &sel(&[&[]]));
        canonicalise(&mut e);
        assert!(e.is_empty_all(), "got {e:?}");
    }

    /// `-outcome:win` canonicalises to a root `Not`, whose only pill is at
    /// `[0]`. That path has a parent, but the parent is a `Not` and holds no
    /// children vector to remove from.
    #[test]
    fn deleting_the_only_pill_under_a_root_not_leaves_the_canonical_empty_query() {
        let mut e: MatchExpr = Expr::Not(Box::new(Expr::Leaf(leaf_term(1))));
        delete(&mut e, &sel(&[&[0]]));
        canonicalise(&mut e);
        assert!(e.is_empty_all(), "got {e:?}");
    }

    /// A negation nested inside a query loses only its own operand, not the
    /// siblings around it.
    #[test]
    fn deleting_a_nested_negations_operand_keeps_its_siblings() {
        let mut e = Expr::All(vec![leaf(1), Expr::Not(Box::new(leaf(2)))]);
        delete(&mut e, &sel(&[&[1, 0]]));
        canonicalise(&mut e);
        assert_eq!(e, leaf(1));
    }

    #[test]
    fn negating_the_root_leaf_flips_its_operator_in_place() {
        let mut e = Expr::Leaf(leaf_term(1));
        negate(&mut e, &vec![]);
        assert_eq!(e, Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Ne, Value::Int(1))));
        negate(&mut e, &vec![]);
        assert_eq!(e, Expr::Leaf(leaf_term(1)), "negating twice returns the original");
    }

    #[test]
    fn negating_the_root_group_wraps_and_unwraps_it_in_place() {
        let before: MatchExpr = Expr::Any(vec![leaf(1), leaf(2)]);
        let mut e = before.clone();
        negate(&mut e, &vec![]);
        assert_eq!(e, Expr::Not(Box::new(before.clone())));
        negate(&mut e, &vec![]);
        assert_eq!(e, before);
    }

    /// The pill of `-outcome:win` sits at `[0]`, under a root that is a `Not`
    /// rather than an `All`.
    #[test]
    fn negating_the_only_pill_under_a_root_not_flips_it() {
        let mut e: MatchExpr = Expr::Not(Box::new(Expr::Leaf(leaf_term(1))));
        negate(&mut e, &vec![0]);
        assert_eq!(e, Expr::Not(Box::new(Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Ne, Value::Int(1))))));
    }

    /// The root has no parent to splice into and the printer already suppresses
    /// its brackets, so there is nothing there to dissolve. Refusing has to be
    /// reported, or the bar re-queries and drops the selection as if it acted.
    #[test]
    fn ungrouping_the_root_is_refused_and_says_so() {
        let mut e = Expr::All(vec![leaf(1), leaf(2)]);
        let before = e.clone();
        assert!(!ungroup(&mut e, &vec![]));
        assert_eq!(e, before);
    }

    /// A `Not` holds exactly one operand and cannot absorb a group's children,
    /// so the group has to survive the refusal intact rather than be emptied.
    #[test]
    fn ungrouping_a_group_under_a_negation_is_refused_without_losing_its_children() {
        let mut e: MatchExpr = Expr::Not(Box::new(Expr::Any(vec![leaf(1), leaf(2)])));
        let before = e.clone();
        assert!(!ungroup(&mut e, &vec![0]));
        assert_eq!(e, before);
    }

    #[test]
    fn pill_paths_lists_only_pills_in_stream_order() {
        use crate::ui::query_bar::label::NameCache;
        use crate::ui::query_bar::tokens::tokenize;
        let e = Expr::All(vec![leaf(1), Expr::Any(vec![leaf(2), leaf(3)])]);
        let toks = tokenize(&e, &NameCache::default());
        assert_eq!(pill_paths(&toks), vec![vec![0], vec![1, 0], vec![1, 1]]);
    }

    #[test]
    fn a_roster_internal_pill_is_drawn_but_not_selectable() {
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;
        use crate::ui::query_bar::label::NameCache;
        use crate::ui::query_bar::tokens::tokenize;
        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) }),
            Expr::Leaf(RosterTerm { field: RosterField::Kills, op: Op::Ge, value: Value::Int(2) }),
        ]);
        let e: MatchExpr = Expr::All(vec![leaf(1), Expr::Leaf(MatchTerm::Roster { quant: Quant::None, pred })]);
        let toks = tokenize(&e, &NameCache::default());
        assert_eq!(pill_paths(&toks), vec![vec![0], vec![1, 0], vec![1, 1]], "both roster conjuncts draw as pills");
        assert_eq!(selectable_paths(&e, &toks), vec![vec![0]], "only the match-level pill can be selected");
        assert!(addresses_match_node(&e, &[1]), "the quantifier leaf itself is a match node");
        assert!(!addresses_match_node(&e, &[1, 0]));
    }

    #[test]
    fn an_interior_group_node_addresses_a_match_node() {
        let e = Expr::All(vec![leaf(1), Expr::Any(vec![leaf(2), leaf(3)])]);
        assert!(addresses_match_node(&e, &[]));
        assert!(addresses_match_node(&e, &[1]));
        assert!(addresses_match_node(&e, &[1, 0]));
        assert!(!addresses_match_node(&e, &[9]));
    }

    #[test]
    fn stepping_back_from_nothing_lands_on_the_last_pill_and_forward_goes_nowhere() {
        let paths = vec![vec![0], vec![1], vec![2]];
        assert_eq!(step(&paths, None, true), Some(vec![2]));
        assert_eq!(step(&paths, None, false), None);
    }

    #[test]
    fn stepping_stops_at_each_end_rather_than_wrapping() {
        let paths = vec![vec![0], vec![1]];
        assert_eq!(step(&paths, Some(&[0]), true), None);
        assert_eq!(step(&paths, Some(&[1]), false), None);
        assert_eq!(step(&paths, Some(&[0]), false), Some(vec![1]));
        assert_eq!(step(&paths, Some(&[1]), true), Some(vec![0]));
    }

    #[test]
    fn stepping_from_a_path_that_is_not_a_pill_goes_nowhere() {
        let paths = vec![vec![0], vec![1]];
        assert_eq!(step(&paths, Some(&[7]), true), None);
    }

    #[test]
    fn a_range_is_inclusive_and_independent_of_which_end_came_first() {
        let paths = vec![vec![0], vec![1], vec![2], vec![3]];
        assert_eq!(range(&paths, &[1], &[3]), vec![vec![1], vec![2], vec![3]]);
        assert_eq!(range(&paths, &[3], &[1]), vec![vec![1], vec![2], vec![3]]);
        assert_eq!(range(&paths, &[2], &[2]), vec![vec![2]]);
        assert!(range(&paths, &[2], &[9]).is_empty());
    }

    #[test]
    fn appending_to_an_all_root_pushes_a_sibling() {
        let mut e = Expr::All(vec![leaf(1)]);
        append_top_level(&mut e, leaf(2));
        assert_eq!(e, Expr::All(vec![leaf(1), leaf(2)]));
    }

    #[test]
    fn appending_to_an_any_root_wraps_rather_than_widening_the_or() {
        // Pushing into the `Any` would OR the new filter in, so a query that
        // was `a or b` would become `a or b or c` instead of `(a or b) and c`.
        let mut e = Expr::Any(vec![leaf(1), leaf(2)]);
        append_top_level(&mut e, leaf(3));
        assert_eq!(e, Expr::All(vec![Expr::Any(vec![leaf(1), leaf(2)]), leaf(3)]));
    }

    #[test]
    fn appending_to_a_leaf_root_wraps_both_in_a_conjunction() {
        let mut e = leaf(1);
        append_top_level(&mut e, leaf(2));
        assert_eq!(e, Expr::All(vec![leaf(1), leaf(2)]));
    }

    #[test]
    fn appending_a_parsed_conjunction_splices_its_children() {
        let mut e = Expr::All(vec![leaf(1)]);
        append_query(&mut e, Expr::All(vec![leaf(2), leaf(3)]));
        assert_eq!(e, Expr::All(vec![leaf(1), leaf(2), leaf(3)]));
    }

    #[test]
    fn appending_a_parsed_disjunction_keeps_it_as_one_group() {
        let mut e = Expr::All(vec![leaf(1)]);
        append_query(&mut e, Expr::Any(vec![leaf(2), leaf(3)]));
        assert_eq!(e, Expr::All(vec![leaf(1), Expr::Any(vec![leaf(2), leaf(3)])]));
    }

    #[test]
    fn setting_a_connector_swaps_the_node_kind_and_keeps_its_children() {
        let mut e = Expr::All(vec![leaf(1), Expr::All(vec![leaf(2), leaf(3)])]);
        set_connector(&mut e, &[1], true);
        assert_eq!(e, Expr::All(vec![leaf(1), Expr::Any(vec![leaf(2), leaf(3)])]));
        set_connector(&mut e, &[1], false);
        assert_eq!(e, Expr::All(vec![leaf(1), Expr::All(vec![leaf(2), leaf(3)])]));
    }

    #[test]
    fn setting_a_connector_on_a_leaf_changes_nothing() {
        let mut e = Expr::All(vec![leaf(1)]);
        let before = e.clone();
        set_connector(&mut e, &[0], true);
        assert_eq!(e, before);
    }

    /// `term_op_at` is what feeds the operator segment's dropdown and what
    /// `can_set_op` greys its rows with, so this is the source the offered set
    /// is read from.
    #[test]
    fn a_terms_offered_operators_are_only_the_ones_its_field_allows() {
        let e = Expr::All(vec![leaf(1)]);
        let (allowed, current) = term_op_at(&e, &[0]).expect("a field term");
        assert_eq!(allowed, MatchField::Build.allowed_ops());
        assert_eq!(current, Op::Eq);
        assert!(!allowed.contains(&Op::Contains), "Build is numeric");
    }

    #[test]
    fn setting_an_operator_the_field_forbids_changes_nothing() {
        // `Contains` prints as `:` and `Eq` as `=`; accepting it here would
        // give a tree that reparses into a different one.
        let mut e = Expr::All(vec![leaf(1)]);
        let before = e.clone();
        assert!(!set_op(&mut e, &[0], Op::Contains));
        assert_eq!(e, before);
        assert!(!can_set_op(&e, &[0], Op::Contains));
    }

    #[test]
    fn setting_an_allowed_operator_replaces_it_in_place() {
        let mut e = Expr::All(vec![leaf(1)]);
        assert!(set_op(&mut e, &[0], Op::Ge));
        assert_eq!(e, Expr::All(vec![Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Ge, Value::Int(1)))]));
    }

    #[test]
    fn switching_to_a_nullary_operator_drops_the_operand() {
        let mut e = Expr::All(vec![leaf(1)]);
        assert!(set_op(&mut e, &[0], Op::IsSet));
        assert_eq!(e, Expr::All(vec![Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand))]));
    }

    #[test]
    fn switching_back_off_a_nullary_operator_is_refused() {
        // There is no operand to compare against and no placeholder is allowed.
        let mut e = Expr::All(vec![Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand))]);
        let before = e.clone();
        assert!(!can_set_op(&e, &[0], Op::Eq));
        assert!(!set_op(&mut e, &[0], Op::Eq));
        assert_eq!(e, before);
    }

    #[test]
    fn can_set_op_agrees_with_set_op_for_every_operator() {
        for start in [
            Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Eq, Value::Int(1))),
            Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand)),
            Expr::Leaf(MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("x".into()))),
            Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win))),
        ] {
            let e = Expr::All(vec![start.clone()]);
            let (_, current) = term_op_at(&e, &[0]).expect("a field term");
            for op in Op::ALL {
                let predicted = can_set_op(&e, &[0], op);
                let mut applied = e.clone();
                assert_eq!(set_op(&mut applied, &[0], op), predicted, "{start:?} to {op:?}");
                // Re-setting the operator a term already carries takes but is a
                // no-op, so only a refusal and a genuine change are pinned here.
                if !predicted {
                    assert_eq!(applied, e, "{start:?} to {op:?} was refused yet changed the tree");
                } else if op != current {
                    assert_ne!(applied, e, "{start:?} to {op:?} took yet changed nothing");
                }
            }
        }
    }

    #[test]
    fn every_operator_the_menu_offers_survives_a_print_parse_round_trip() {
        use crate::db::index::query_text::parse_query;
        use crate::db::index::query_text::print_query;
        for field in MatchField::ALL {
            for &op in field.allowed_ops() {
                let mut e = Expr::All(vec![
                    Expr::Leaf(MatchTerm::Field(field, field.allowed_ops()[0], sample_value(field.value_kind()))),
                    leaf(7),
                ]);
                if !set_op(&mut e, &[0], op) {
                    continue;
                }
                let printed = print_query(&e);
                let reparsed =
                    parse_query(&printed).unwrap_or_else(|err| panic!("{field:?} {op:?} -> {printed}: {err}"));
                assert_eq!(reparsed, e, "{field:?} {op:?} printed {printed}");
            }
        }
    }

    #[test]
    fn a_roster_predicates_own_leaf_is_reachable_by_its_token_path() {
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;
        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) }),
            Expr::Leaf(RosterTerm { field: RosterField::Kills, op: Op::Ge, value: Value::Int(2) }),
        ]);
        let mut e: MatchExpr = Expr::All(vec![
            leaf(1),
            Expr::Leaf(MatchTerm::Roster { quant: crate::db::index::query_ast::Quant::None, pred }),
        ]);
        // The second roster conjunct's token path is the Roster leaf's path
        // followed by its index inside the predicate.
        assert!(set_op(&mut e, &[1, 1], Op::Lt));
        match &e {
            Expr::All(cs) => match &cs[1] {
                Expr::Leaf(MatchTerm::Roster { pred, .. }) => match &pred.children()[1] {
                    Expr::Leaf(term) => assert_eq!(term.op, Op::Lt),
                    other => panic!("got {other:?}"),
                },
                other => panic!("got {other:?}"),
            },
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn a_sugar_collapsed_roster_pill_offers_no_operator() {
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;
        use crate::db::index::rows::VehicleRelation;
        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(VehicleRelation::Enemy),
            }),
            Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) }),
        ]);
        // The whole quantifier renders as one pill, so its path names two terms
        // at once and there is no single operator to offer.
        let e: MatchExpr = Expr::All(vec![leaf(1), Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred })]);
        assert_eq!(term_op_at(&e, &[1]), None);
    }

    #[test]
    fn changing_the_field_keeps_a_compatible_operator() {
        // Map and GameType are both text, so Contains survives.
        let mut e: MatchExpr = Expr::Leaf(MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("ocean".into())));
        assert!(set_field(&mut e, &vec![], TermField::Match(MatchField::GameType)));
        match &e {
            Expr::Leaf(MatchTerm::Field(f, op, v)) => {
                assert_eq!(*f, MatchField::GameType);
                assert_eq!(*op, Op::Contains);
                assert_eq!(*v, Value::Text("ocean".into()));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn changing_the_field_replaces_an_operator_the_new_field_disallows() {
        let mut e: MatchExpr = Expr::Leaf(MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("ocean".into())));
        assert!(set_field(&mut e, &vec![], TermField::Match(MatchField::Outcome)));
        match &e {
            Expr::Leaf(MatchTerm::Field(f, op, _)) => {
                assert_eq!(*f, MatchField::Outcome);
                assert!(f.allowed_ops().contains(op), "{op:?} not allowed for {f:?}");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn changing_the_field_clears_a_value_of_the_wrong_kind() {
        let mut e: MatchExpr = Expr::Leaf(MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("ocean".into())));
        set_field(&mut e, &vec![], TermField::Match(MatchField::Build));
        match &e {
            Expr::Leaf(MatchTerm::Field(_, _, v)) => {
                assert_ne!(*v, Value::Text("ocean".into()), "a text value must not survive onto an int field");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn every_field_pair_leaves_a_legal_term_that_round_trips() {
        // The exhaustive form of the operator trap: no field change may produce
        // a term that prints and reparses into something else.
        use crate::db::index::query_text::parse_query;
        use crate::db::index::query_text::print_query;
        // Seeded from every operator `from` allows, not just its first: index 0
        // is always the equality-flavoured spelling for every op set, so
        // seeding from it alone would never exercise `reconcile_term`'s
        // "keep the operator" branch with a comparison or negation operator.
        for from in MatchField::ALL {
            for &op in from.allowed_ops() {
                for to in MatchField::ALL {
                    let value = if op.is_nullary() { Value::NoOperand } else { sample_value(from.value_kind()) };
                    let mut e: MatchExpr = Expr::Leaf(MatchTerm::Field(from, op, value));
                    assert!(
                        set_field(&mut e, &vec![], TermField::Match(to)),
                        "{from:?}/{op:?} -> {to:?} was not addressed as a field term"
                    );
                    match &e {
                        Expr::Leaf(MatchTerm::Field(f, op, _)) => {
                            assert_eq!(*f, to, "{from:?}/{op:?} -> {to:?} did not change the field");
                            assert!(f.allowed_ops().contains(op), "{from:?}/{op:?} -> {to:?} gave illegal {op:?}");
                        }
                        other => panic!("{from:?}/{op:?} -> {to:?} gave {other:?}"),
                    }
                    let printed = print_query(&e);
                    assert_eq!(parse_query(&printed).unwrap(), e, "{from:?}/{op:?} -> {to:?} printed {printed:?}");
                }
            }
        }
    }

    #[test]
    fn every_roster_field_pair_leaves_a_legal_term_that_round_trips() {
        // The roster-level mirror of the exhaustive match-field test above:
        // `TermField::Roster` is the other half of `set_field` and the only
        // route to `ValueKind::Float`, `Relation`, `Division`, `Class`,
        // `Ship`, and `Account`. Covers both a root-path roster pill (the
        // roster analogue of the match-level root-leaf trap: the whole tree
        // is one `Leaf(MatchTerm::Roster { .. })` with no parent to look up)
        // and a nested one, addressed one step into a compound predicate
        // alongside a sibling that must survive untouched.
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;
        use crate::db::index::query_text::parse_query;
        use crate::db::index::query_text::print_query;

        // Seeded from every operator `from` allows, not just its first, for
        // the same reason as the match-level test above.
        for from in RosterField::ALL {
            for &op in from.allowed_ops() {
                for to in RosterField::ALL {
                    let value = if op.is_nullary() { Value::NoOperand } else { sample_value(from.value_kind()) };

                    let mut root: MatchExpr = Expr::Leaf(MatchTerm::Roster {
                        quant: Quant::Any,
                        pred: Expr::Leaf(RosterTerm { field: from, op, value: value.clone() }),
                    });
                    assert!(
                        set_field(&mut root, &vec![], TermField::Roster(to)),
                        "{from:?}/{op:?} -> {to:?} at the root was not addressed as a field term"
                    );
                    match &root {
                        Expr::Leaf(MatchTerm::Roster { pred: Expr::Leaf(term), .. }) => {
                            assert_eq!(
                                term.field, to,
                                "{from:?}/{op:?} -> {to:?} at the root did not change the field"
                            );
                            assert!(
                                term.field.allowed_ops().contains(&term.op),
                                "{from:?}/{op:?} -> {to:?} at the root gave illegal {:?}",
                                term.op
                            );
                        }
                        other => panic!("{from:?}/{op:?} -> {to:?} at the root gave {other:?}"),
                    }
                    let printed = print_query(&root);
                    assert_eq!(
                        parse_query(&printed).unwrap(),
                        root,
                        "{from:?}/{op:?} -> {to:?} at the root printed {printed:?}"
                    );

                    let sibling = RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) };
                    let mut nested: MatchExpr = Expr::Leaf(MatchTerm::Roster {
                        quant: Quant::Any,
                        pred: Expr::All(vec![
                            Expr::Leaf(sibling.clone()),
                            Expr::Leaf(RosterTerm { field: from, op, value }),
                        ]),
                    });
                    assert!(
                        set_field(&mut nested, &vec![1], TermField::Roster(to)),
                        "{from:?}/{op:?} -> {to:?} nested was not addressed as a field term"
                    );
                    match &nested {
                        Expr::Leaf(MatchTerm::Roster { pred: Expr::All(cs), .. }) => {
                            assert_eq!(
                                cs[0],
                                Expr::Leaf(sibling),
                                "{from:?}/{op:?} -> {to:?} nested changed its sibling"
                            );
                            match &cs[1] {
                                Expr::Leaf(term) => {
                                    assert_eq!(
                                        term.field, to,
                                        "{from:?}/{op:?} -> {to:?} nested did not change the field"
                                    );
                                    assert!(
                                        term.field.allowed_ops().contains(&term.op),
                                        "{from:?}/{op:?} -> {to:?} nested gave illegal {:?}",
                                        term.op
                                    );
                                }
                                other => panic!("{from:?}/{op:?} -> {to:?} nested gave {other:?}"),
                            }
                        }
                        other => panic!("{from:?}/{op:?} -> {to:?} nested gave {other:?}"),
                    }
                    let printed = print_query(&nested);
                    assert_eq!(
                        parse_query(&printed).unwrap(),
                        nested,
                        "{from:?}/{op:?} -> {to:?} nested printed {printed:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn changing_the_field_off_a_nullary_operator_keeps_the_nullary_invariant() {
        // Build's IsSet is the only nullary operator any MatchField allows.
        // Every other field forbids it, so the operator is replaced and the
        // value must stop being NoOperand; retargeting to Build itself keeps
        // both. Looping over every field pins the invariant both ways:
        // nullary implies NoOperand, non-nullary must not carry it.
        for to in MatchField::ALL {
            let mut e: MatchExpr = Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand));
            assert!(
                set_field(&mut e, &vec![], TermField::Match(to)),
                "Build -> {to:?} was not addressed as a field term"
            );
            match &e {
                Expr::Leaf(MatchTerm::Field(f, op, v)) => {
                    assert_eq!(*f, to);
                    assert!(f.allowed_ops().contains(op), "{to:?} does not allow {op:?}");
                    if op.is_nullary() {
                        assert_eq!(*v, Value::NoOperand, "{to:?}: a nullary operator must carry NoOperand");
                    } else {
                        assert_ne!(*v, Value::NoOperand, "{to:?}: {op:?} is not nullary but kept NoOperand");
                    }
                }
                other => panic!("got {other:?}"),
            }
        }
    }

    /// The sibling assertion alone holds against a `set_field` that does
    /// nothing at all, so the target's own change is asserted with it.
    #[test]
    fn changing_the_field_of_a_nested_pill_leaves_its_siblings_alone() {
        let mut e = Expr::All(vec![
            Expr::Leaf(MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text("ocean".into()))),
            Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Ge, Value::Int(1234))),
        ]);
        let before = e.children()[1].clone();
        assert!(set_field(&mut e, &vec![0], TermField::Match(MatchField::Outcome)), "the path addresses a field term");
        assert_eq!(
            term_at(&e, &[0]).expect("the retargeted term").0,
            TermField::Match(MatchField::Outcome),
            "the target must actually move"
        );
        assert_eq!(e.children()[1], before);
    }

    fn sample_value(kind: crate::db::index::query_ast::ValueKind) -> Value {
        use crate::db::index::query_ast::ValueKind;
        match kind {
            ValueKind::Text => Value::Text("x".into()),
            ValueKind::Int => Value::Int(1),
            ValueKind::Float => Value::Float(1.0),
            ValueKind::Bool => Value::Bool(true),
            ValueKind::Outcome => Value::Outcome(MatchOutcome::Win),
            ValueKind::Relation => Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
            ValueKind::Division => Value::Division(crate::db::index::query_ast::DivisionScope::Mine),
            ValueKind::Class => Value::Class(crate::db::index::query_ast::ShipClass::Destroyer),
            ValueKind::Ship => Value::Ship(wows_replays::types::GameParamId::from(1u64)),
            ValueKind::Account => Value::Account(wows_replays::types::AccountId(1)),
            ValueKind::Source => Value::Source(crate::db::index::rows::SourceId(1)),
            ValueKind::Timestamp => Value::Timestamp(jiff::Timestamp::from_second(0).unwrap()),
            ValueKind::GameMode => Value::GameMode(GameMode::ArmsRace),
        }
    }

    /// A retarget that mints a placeholder must mint one the dropdown could
    /// also have produced: a value the user could reach some other way, not a
    /// value that exists only inside this function. `GameMode`'s placeholder
    /// (`is_offerable`'s first hit, not `ALL[0]`) is what motivates this test;
    /// checked across every enum kind so a future kind cannot reintroduce the
    /// same gap.
    #[test]
    fn the_placeholder_for_every_enum_kind_is_something_the_dropdown_offers() {
        use crate::db::index::query_ast::ValueKind;
        use crate::db::index::query_text::parse_roster_value;
        use crate::ui::query_bar::suggest::enum_values;

        for kind in [
            ValueKind::Outcome,
            ValueKind::Bool,
            ValueKind::Relation,
            ValueKind::Division,
            ValueKind::Class,
            ValueKind::GameMode,
        ] {
            let options = enum_values(kind).unwrap_or_else(|| panic!("{kind:?} should be enumerable"));
            let placeholder = placeholder_value(kind);
            let offered: Vec<Value> = options
                .iter()
                .map(|o| parse_roster_value(kind, &o.token).unwrap_or_else(|| panic!("{kind:?} token {:?}", o.token)))
                .collect();
            assert!(
                offered.contains(&placeholder),
                "{kind:?} placeholder {placeholder:?} is not among the dropdown's own values: {offered:?}"
            );
        }
    }

    #[test]
    fn every_edit_leaves_a_tree_that_survives_a_print_parse_round_trip() {
        use crate::db::index::query_text::parse_query;
        use crate::db::index::query_text::print_query;
        let mut e = Expr::All(vec![leaf(1), leaf(2), leaf(3)]);
        // Exercises all four edit functions, and both `negate` branches: edit
        // 1 flips build=3's `Eq` to its allowed inverse `Ne` (the branch a
        // group negation never reaches), and edit 4 wraps a group in `Not`.
        for edit in 0..6 {
            match edit {
                0 => group(&mut e, &sel(&[&[0], &[1]]), true),
                1 => negate(&mut e, &vec![1]),
                2 => assert!(ungroup(&mut e, &vec![0]), "edit 2 should have ungrouped"),
                3 => group(&mut e, &sel(&[&[0], &[1]]), false),
                4 => negate(&mut e, &vec![0]),
                _ => delete(&mut e, &sel(&[&[1]])),
            }
            canonicalise(&mut e);
            let printed = print_query(&e);
            let reparsed = parse_query(&printed).unwrap_or_else(|err| panic!("edit {edit} printed {printed:?}: {err}"));
            assert_eq!(reparsed, e, "edit {edit} printed {printed:?}");
        }
    }

    fn empty_map() -> MatchExpr {
        Expr::Leaf(MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text(String::new())))
    }
    fn map_named(name: &str) -> MatchExpr {
        Expr::Leaf(MatchTerm::Field(MatchField::Map, Op::Contains, Value::Text(name.to_owned())))
    }

    #[test]
    fn pruning_drops_an_empty_contains_and_keeps_the_rest() {
        let expr = Expr::All(vec![leaf(1), empty_map(), map_named("ocean")]);
        assert_eq!(prune_empty(&expr), Expr::All(vec![leaf(1), map_named("ocean")]));
    }

    #[test]
    fn pruning_drops_an_empty_free_text_but_keeps_a_non_empty_one() {
        let empty: MatchExpr = Expr::Leaf(MatchTerm::FreeText(String::new()));
        let typed: MatchExpr = Expr::Leaf(MatchTerm::FreeText("yamato".into()));
        assert_eq!(prune_empty(&Expr::All(vec![empty.clone(), typed.clone()])), typed);
        assert_eq!(prune_empty(&empty), MatchExpr::default());
    }

    /// A non-empty text value is a real filter even when it is only one
    /// character, and a `Contains` is not the only operator over text: an
    /// `Equals` against the empty string asks for rows whose column is empty,
    /// which is a constraint and must survive.
    #[test]
    fn pruning_keeps_terms_that_still_say_something() {
        let one_char = map_named("o");
        assert_eq!(prune_empty(&one_char), one_char);
        let equals_empty: MatchExpr =
            Expr::Leaf(MatchTerm::Field(MatchField::Map, Op::Equals, Value::Text(String::new())));
        assert_eq!(prune_empty(&equals_empty), equals_empty);
    }

    /// The point of pruning: an empty term must not be able to widen the query
    /// it sits in. Both branches of an OR going empty leaves nothing, and the
    /// tree that survives is the one the other terms describe.
    #[test]
    fn pruning_never_widens_the_surviving_query() {
        let expr = Expr::All(vec![leaf(1), Expr::Any(vec![empty_map(), map_named("ocean")])]);
        assert_eq!(prune_empty(&expr), Expr::All(vec![leaf(1), map_named("ocean")]));

        let both_empty = Expr::All(vec![leaf(1), Expr::Any(vec![empty_map(), empty_map()])]);
        assert_eq!(prune_empty(&both_empty), leaf(1));
    }

    /// An unfinished term is one the user has not asked for, so negating it
    /// does not turn the query unsatisfiable.
    #[test]
    fn pruning_removes_a_negated_empty_term_rather_than_matching_nothing() {
        let expr = Expr::All(vec![leaf(1), Expr::Not(Box::new(empty_map()))]);
        assert_eq!(prune_empty(&expr), leaf(1));
        assert_eq!(prune_empty(&Expr::Not(Box::new(empty_map()))), MatchExpr::default());
    }

    /// A roster predicate's quantifier changes what an emptied predicate
    /// asserts, so nothing inside one is dropped.
    #[test]
    fn pruning_leaves_roster_predicates_alone() {
        use crate::db::index::query_ast::Quant;
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;

        let roster: MatchExpr = Expr::Leaf(MatchTerm::Roster {
            quant: Quant::None,
            pred: Expr::Leaf(RosterTerm {
                field: RosterField::Name,
                op: Op::Contains,
                value: Value::Text(String::new()),
            }),
        });
        assert_eq!(prune_empty(&roster), roster);
    }

    /// One segment edit, in the three shapes `mod.rs`'s routing applies.
    #[derive(Debug, Clone)]
    enum Edit {
        Field(TermField),
        Operator(Op),
        Literal(Value),
    }

    fn apply_edit(expr: &mut MatchExpr, path: &NodePath, edit: &Edit) -> bool {
        match edit {
            Edit::Field(field) => set_field(expr, path, *field),
            Edit::Operator(op) => set_op(expr, path, *op),
            Edit::Literal(value) => set_value(expr, path, value.clone()),
        }
    }

    /// (query text, starting tree, path, edit) quadruples where the edit must
    /// land on exactly the tree the text parses to.
    ///
    /// A scope is not a field: in the grammar it is a `RosterField::Relation`
    /// conjunct sitting beside the term it scopes. *Introducing or removing*
    /// one is therefore a tree-shape change no `set_field` can express, and
    /// there is no segment edit for it to converge with. *Retargeting* an
    /// existing one is expressible, and both halves of it are covered.
    ///
    /// The scope conjunct is at `[0]` and the term it scopes at `[1]`. Both
    /// cases below use the sugar-shaped tree, which renders as one collapsed
    /// pill whose `segment_path` is `[1]`, so no click on *that* tree reaches
    /// `[0]` at all -- a scope conjunct is only click-reachable on a genuinely
    /// bracketed roster, which this fixture is not. The `[0]` case is kept
    /// because it is the only one exercising the sugar printer's scope round
    /// trip; the `[1]` cases are here because they are what a click on the
    /// collapsed pill actually produces.
    fn convergence_cases() -> Vec<(&'static str, MatchExpr, NodePath, Edit)> {
        use crate::db::index::query_ast::Quant;
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;
        use crate::db::index::rows::VehicleRelation;

        let roster = |field: RosterField, op: Op, value: Value| -> MatchExpr {
            Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred: Expr::Leaf(RosterTerm { field, op, value }) })
        };
        let scoped = |relation: VehicleRelation| -> MatchExpr {
            Expr::Leaf(MatchTerm::Roster {
                quant: Quant::Any,
                pred: Expr::All(vec![
                    Expr::Leaf(RosterTerm {
                        field: RosterField::Relation,
                        op: Op::Is,
                        value: Value::Relation(relation),
                    }),
                    Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) }),
                ]),
            })
        };
        vec![
            (
                "date>=1970-01-01",
                Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Ge, Value::Int(5))),
                vec![],
                Edit::Field(TermField::Match(MatchField::Date)),
            ),
            (
                "anyone.damage=10",
                roster(RosterField::Tier, Op::Eq, Value::Int(10)),
                vec![],
                Edit::Field(TermField::Roster(RosterField::Damage)),
            ),
            (
                "build<5",
                Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Ge, Value::Int(5))),
                vec![],
                Edit::Operator(Op::Lt),
            ),
            (
                "outcome=loss",
                Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win))),
                vec![],
                Edit::Literal(Value::Outcome(MatchOutcome::Loss)),
            ),
            (
                "ally.tier=10",
                scoped(VehicleRelation::Enemy),
                vec![0],
                Edit::Literal(Value::Relation(VehicleRelation::Ally)),
            ),
            ("enemy.tier=20", scoped(VehicleRelation::Enemy), vec![1], Edit::Literal(Value::Int(20))),
            (
                "enemy.damage=10",
                scoped(VehicleRelation::Enemy),
                vec![1],
                Edit::Field(TermField::Roster(RosterField::Damage)),
            ),
        ]
    }

    /// The property that keeps the two editing paths honest, and the reason
    /// both can be first-class: whatever a segment edit builds, typing the
    /// equivalent text builds the same tree.
    #[test]
    fn editing_a_segment_and_typing_the_text_produce_the_same_tree() {
        use crate::db::index::query_text::ZoneGuard;
        use crate::db::index::query_text::parse_query;
        // The `date` case types a bare date, which the parser reads as local
        // midnight, against the epoch `placeholder_value` mints. The two are
        // the same instant in UTC alone, so the zone is pinned rather than left
        // to be whichever one the machine running the test sits in.
        let _zone = ZoneGuard::set(jiff::tz::TimeZone::UTC);
        for (typed, start, path, edit) in convergence_cases() {
            let mut edited = start;
            assert!(apply_edit(&mut edited, &path, &edit), "the edit {edit:?} was refused");
            canonicalise(&mut edited);
            assert_eq!(edited, parse_query(typed).expect("the case text parses"), "typing {typed:?} diverged");
        }
    }

    /// The bug `edit_as_text` had: it printed the leaf under a `not`, deleted
    /// the leaf, and left the caret holding text that reparsed without the
    /// negation. A segment edit rewrites the term in place, so the enclosing
    /// node cannot be lost -- and this is the property the retired refusal test
    /// was ultimately protecting.
    #[test]
    fn changing_a_value_under_a_not_keeps_the_negation() {
        let win = Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win)));
        let mut e = Expr::Not(Box::new(win));
        assert_eq!(segment_path(&e, &vec![0]), Some(vec![0]));
        assert!(set_value(&mut e, &[0], Value::Outcome(MatchOutcome::Loss)));
        canonicalise(&mut e);
        match &e {
            Expr::Not(inner) => assert_eq!(
                **inner,
                Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Loss)))
            ),
            other => panic!("the negation was lost: {other:?}"),
        }
    }

    /// The direct replacement for the retired `edit_as_text` refusal. That test
    /// asserted a roster-internal pill could not be lifted into the caret,
    /// because the printed text would be the whole quantifier while the removal
    /// addressed a node the match tree does not have. Segment edits do not
    /// print or remove anything, so the refusal is meaningless; what has to
    /// hold instead is that the edit reaches the inner term and touches nothing
    /// around it.
    #[test]
    fn a_segment_edit_on_a_roster_internal_pill_changes_only_that_term() {
        use crate::db::index::query_ast::Quant;
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;

        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) }),
            Expr::Leaf(RosterTerm { field: RosterField::Kills, op: Op::Ge, value: Value::Int(2) }),
        ]);
        let win = Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win)));
        let mut e = Expr::All(vec![win.clone(), Expr::Leaf(MatchTerm::Roster { quant: Quant::None, pred })]);

        let path = segment_path(&e, &vec![1, 0]).expect("a roster-internal pill addresses its own term");
        assert_eq!(path, vec![1, 0]);
        assert!(set_value(&mut e, &path, Value::Int(8)));

        let Expr::All(children) = &e else { panic!("got {e:?}") };
        assert_eq!(children[0], win, "the sibling term must not move");
        let Expr::Leaf(MatchTerm::Roster { quant, pred }) = &children[1] else { panic!("got {:?}", children[1]) };
        assert_eq!(*quant, Quant::None, "the quantifier must survive");
        let Expr::All(inner) = pred else { panic!("got {pred:?}") };
        assert_eq!(inner[0], Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(8) }));
        assert_eq!(inner[1], Expr::Leaf(RosterTerm { field: RosterField::Kills, op: Op::Ge, value: Value::Int(2) }));
    }

    /// A sugar-collapsed pill draws the quantifier's inner term, so its
    /// segments have to address that term rather than the `Roster` leaf, which
    /// carries no single field. Without the redirection every segment on a
    /// collapsed pill would be a click that does nothing.
    #[test]
    fn a_sugar_collapsed_pill_addresses_the_term_it_draws() {
        use crate::db::index::query_ast::Quant;
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;
        use crate::db::index::rows::VehicleRelation;

        let scope = Expr::Leaf(RosterTerm {
            field: RosterField::Relation,
            op: Op::Is,
            value: Value::Relation(VehicleRelation::Enemy),
        });
        let inner = Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) });
        let e: MatchExpr = Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred: Expr::All(vec![scope, inner]) });

        assert_eq!(segment_path(&e, &vec![]), Some(vec![1]), "the scope conjunct is not what the pill draws");
        let (field, op, value) = term_at(&e, &[1]).expect("the inner term");
        assert_eq!(field, TermField::Roster(RosterField::Tier));
        assert_eq!(op, Op::Eq);
        assert_eq!(*value, Value::Int(10));
    }

    /// Free text has no field and no operator, so it has nothing a segment can
    /// edit. Returning `None` is what keeps its one segment from registering a
    /// click target that could not act on it.
    #[test]
    fn a_free_text_pill_has_no_segment_target() {
        let e: MatchExpr = Expr::Leaf(MatchTerm::FreeText("yamato".into()));
        assert_eq!(segment_path(&e, &vec![]), None);
        assert!(term_at(&e, &[]).is_none());
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_refused() {
        let mut e = Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win)));
        let before = e.clone();
        assert!(!set_value(&mut e, &[], Value::Int(3)), "an Int cannot land on an Outcome field");
        assert_eq!(e, before);
    }

    /// A nullary operator draws no value segment, so nothing should be able to
    /// put a value back on one without going through the operator first.
    #[test]
    fn a_value_on_a_nullary_operator_is_refused() {
        let mut e = Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand));
        let before = e.clone();
        assert!(!set_value(&mut e, &[], Value::Int(3)));
        assert_eq!(e, before);
    }

    #[test]
    fn replace_node_writes_only_while_the_expected_node_is_still_there() {
        let mut e = Expr::All(vec![leaf(1), leaf(2), leaf(3)]);
        assert!(replace_node(&mut e, &[1], &leaf(2), leaf(99)));
        assert_eq!(e, Expr::All(vec![leaf(1), leaf(99), leaf(3)]));
    }

    /// The gate a caller depends on to write a value back safely: a numeric path
    /// names a position, not a node, so a rewrite between reading the path and
    /// writing through it can move a different node there. The write must land
    /// on the node the caller started from, or on nothing.
    #[test]
    fn replace_node_refuses_a_path_whose_node_moved_since_it_was_taken() {
        let mut e = Expr::All(vec![leaf(1), leaf(2), leaf(3), leaf(4)]);
        let seed = leaf(3);
        assert_eq!(node_at(&e, &[2]), Some(&seed), "the fixture must start with the seed at the path under test");

        // A different node is deleted ahead of the path, sliding every term
        // after it up one: the path still resolves, but to a term the caller
        // never saw.
        delete(&mut e, &sel(&[&[0]]));
        assert_eq!(node_at(&e, &[2]), Some(&leaf(4)), "the fixture must leave a different node under the path");

        assert!(!replace_node(&mut e, &[2], &seed, leaf(99)), "the write landed on a node it never opened");
        assert_eq!(e, Expr::All(vec![leaf(2), leaf(3), leaf(4)]));
    }

    #[test]
    fn replace_node_can_replace_the_whole_tree_at_the_root() {
        let mut e = leaf(1);
        assert!(replace_node(&mut e, &[], &leaf(1), leaf(2)));
        assert_eq!(e, leaf(2));
    }

    /// A term inside a roster predicate is not a node the match tree can name,
    /// so the anchor a value edit writes back through is the leaf carrying it
    /// plus the tail that reaches the term inside that leaf.
    #[test]
    fn leaf_seed_splits_a_roster_term_path_into_the_leaf_and_the_tail_inside_it() {
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;
        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) }),
            Expr::Leaf(RosterTerm { field: RosterField::Kills, op: Op::Ge, value: Value::Int(2) }),
        ]);
        let roster: MatchExpr = Expr::Leaf(MatchTerm::Roster { quant: Quant::None, pred });
        let e: MatchExpr = Expr::All(vec![leaf(1), roster.clone()]);

        let (leaf_path, seed, tail) = leaf_seed(&e, &[1, 1]).expect("the roster term resolves");
        assert_eq!(leaf_path, vec![1], "the anchor stops at the match-level leaf");
        assert_eq!(seed, roster, "the seed is the whole leaf, which is what can be gated on");
        assert_eq!(tail, vec![1], "the tail addresses the term inside the predicate");

        let (leaf_path, seed, tail) = leaf_seed(&e, &[0]).expect("the field term resolves");
        assert_eq!((leaf_path, seed, tail), (vec![0], leaf(1), Vec::new()), "a field term has no tail");
    }

    /// `canonicalise` collapses a one-condition tree to a bare leaf, so the term
    /// a box was opened on can end up at the root path. The anchor has to name
    /// it there directly rather than through a parent that does not exist.
    #[test]
    fn leaf_seed_anchors_a_lone_term_at_the_root() {
        let e = leaf(7);
        let (leaf_path, seed, tail) = leaf_seed(&e, &[]).expect("the root term resolves");
        assert_eq!((leaf_path, seed, tail), (Vec::new(), leaf(7), Vec::new()));
    }

    /// The two things the write has to get right at once: the term it lands on
    /// is the one the tail names inside the predicate, and everything standing
    /// around the leaf -- here a `not` -- is untouched, because the leaf is
    /// swapped rather than the node the pill reads as.
    #[test]
    fn commit_value_writes_a_roster_term_through_its_leaf() {
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;
        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Ge, value: Value::Int(8) }),
            Expr::Leaf(RosterTerm { field: RosterField::Kills, op: Op::Ge, value: Value::Int(2) }),
        ]);
        let mut e: MatchExpr = Expr::Not(Box::new(Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred })));

        let (leaf_path, seed, tail) = leaf_seed(&e, &[0, 1]).expect("the second roster term resolves");
        assert_eq!(tail, vec![1], "the fixture must exercise a non-empty tail");
        assert!(commit_value(&mut e, &leaf_path, &seed, &tail, Value::Int(5)));

        assert!(matches!(e, Expr::Not(_)), "the negation was replaced rather than written under: {e:?}");
        assert_eq!(
            term_at(&e, &[0, 1]).map(|(_, _, value)| value.clone()),
            Some(Value::Int(5)),
            "the tail's own term must take the value"
        );
        assert_eq!(
            term_at(&e, &[0, 0]).map(|(_, _, value)| value.clone()),
            Some(Value::Int(8)),
            "its sibling in the predicate must be untouched"
        );
    }

    /// The same staleness `replace_node` guards, driven the way a value commit
    /// reaches it: the leaf a box opened over is deleted while the box is open,
    /// another term slides into its path, and the typed value must land on
    /// nothing rather than on the term that inherited the position.
    #[test]
    fn commit_value_refuses_a_leaf_that_moved_while_the_box_was_open() {
        let mut e = Expr::All(vec![leaf(1), leaf(2), leaf(3)]);
        let (leaf_path, seed, tail) = leaf_seed(&e, &[1]).expect("the second term resolves");

        delete(&mut e, &sel(&[&[0]]));
        assert_eq!(node_at(&e, &leaf_path), Some(&leaf(3)), "the fixture must slide a different term under the path");

        assert!(!commit_value(&mut e, &leaf_path, &seed, &tail, Value::Int(99)), "the write landed on another term");
        assert_eq!(e, Expr::All(vec![leaf(2), leaf(3)]), "a refused write must leave the tree alone");
    }
}
