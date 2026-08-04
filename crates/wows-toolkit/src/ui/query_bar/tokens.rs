//! Flattens a `MatchExpr` into a linear token stream. Layout, hit-testing, and
//! selection all work on the stream rather than recursing the tree, so bracket
//! nesting is expressed by `depth` and by paired open/close tokens.

use crate::db::index::query_ast::Expr;
use crate::db::index::query_ast::MatchExpr;
use crate::db::index::query_ast::MatchTerm;
use crate::db::index::query_ast::RosterTerm;
use crate::ui::query_bar::label;
use crate::ui::query_bar::label::NameCache;
use crate::ui::query_bar::label::PillSegment;

/// Where a node sits in the tree: the child index at each level from the root.
/// An empty path is the root. `Not`'s single operand is index 0.
///
/// A path that continues past a `MatchTerm::Roster` leaf addresses a node
/// inside the roster predicate, which has no representation in `MatchExpr`.
/// `select::split_at_leaf` resolves such a path in two halves -- the match tree
/// down to the `Roster` leaf, then the remainder over the predicate -- which is
/// how every edit reaches a term inside a quantifier.
pub type NodePath = Vec<usize>;

/// Callback that turns one leaf into its token(s) and pushes them to `out`.
/// A trait alias would fit better but those are unstable; this `dyn` alias
/// keeps `emit_expr`'s and `emit_conjunction`'s signatures readable.
type EmitLeaf<'a, L> = dyn FnMut(&L, &NodePath, usize, &mut Vec<Token>) + 'a;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub path: NodePath,
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Pill {
        segments: Vec<PillSegment>,
    },
    /// The `and` / `or` word between two siblings.
    Connector {
        is_or: bool,
    },
    /// `not` in front of the node at `path`.
    NotPrefix,
    GroupOpen {
        is_or: bool,
    },
    GroupClose,
    /// A roster quantifier's opening bracket, carrying its rendered prefix.
    QuantOpen {
        prefix: String,
    },
    QuantClose,
    /// The text-entry caret. Always exactly one, always last.
    Caret,
}

pub fn tokenize(expr: &MatchExpr, cache: &NameCache) -> Vec<Token> {
    let mut out = Vec::new();
    let mut path = Vec::new();
    emit_expr(expr, &mut path, 0, true, &mut out, &mut |term, path, depth, out| {
        emit_match_leaf(term, path, depth, cache, out);
    });
    out.push(Token { kind: TokenKind::Caret, path: Vec::new(), depth: 0 });
    out
}

/// Puts the caret in the slot of the pill at `path`, so a term being edited as
/// text is typed where its pill was rather than at the end of the bar.
///
/// The caret is moved, not added: there is exactly one in the stream, before and
/// after. It inherits the pill's path and depth, so it indents to the same
/// nesting level and any bracket around the pill still closes over it.
///
/// A path that names no pill leaves the stream alone, which is what keeps the
/// caret at the end while an edit's anchor is gone.
pub fn move_caret_to(tokens: &mut Vec<Token>, path: &NodePath) {
    let Some(slot) =
        tokens.iter().position(|token| matches!(token.kind, TokenKind::Pill { .. }) && token.path == *path)
    else {
        return;
    };
    let Some(caret) = tokens.iter().position(|token| matches!(token.kind, TokenKind::Caret)) else {
        return;
    };
    let depth = tokens[slot].depth;
    // The caret is emitted last, so removing it never shifts the slot.
    tokens.remove(caret);
    tokens[slot] = Token { kind: TokenKind::Caret, path: path.clone(), depth };
}

/// Emits one node's tokens. `top_level` suppresses the bracket an `All`/`Any`
/// would otherwise get: true only for the tree's root and for a roster
/// predicate directly under its `QuantOpen`/`QuantClose`, both of which are
/// already delimited by something other than a `GroupOpen`/`GroupClose` pair.
fn emit_expr<L>(
    expr: &Expr<L>,
    path: &mut NodePath,
    depth: usize,
    top_level: bool,
    out: &mut Vec<Token>,
    emit_leaf: &mut EmitLeaf<'_, L>,
) {
    match expr {
        Expr::All(cs) => emit_conjunction(cs, false, path, depth, top_level, out, emit_leaf),
        Expr::Any(cs) => emit_conjunction(cs, true, path, depth, top_level, out, emit_leaf),
        Expr::Not(inner) => {
            out.push(Token { kind: TokenKind::NotPrefix, path: path.clone(), depth });
            path.push(0);
            emit_expr(inner, path, depth, false, out, emit_leaf);
            path.pop();
        }
        Expr::Leaf(term) => emit_leaf(term, path, depth, out),
    }
}

fn emit_conjunction<L>(
    children: &[Expr<L>],
    is_or: bool,
    path: &mut NodePath,
    depth: usize,
    top_level: bool,
    out: &mut Vec<Token>,
    emit_leaf: &mut EmitLeaf<'_, L>,
) {
    let inner_depth = if top_level { depth } else { depth + 1 };
    if !top_level {
        out.push(Token { kind: TokenKind::GroupOpen { is_or }, path: path.clone(), depth: inner_depth });
    }
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            out.push(Token { kind: TokenKind::Connector { is_or }, path: path.clone(), depth: inner_depth });
        }
        path.push(i);
        emit_expr(child, path, inner_depth, false, out, emit_leaf);
        path.pop();
    }
    if !top_level {
        out.push(Token { kind: TokenKind::GroupClose, path: path.clone(), depth: inner_depth });
    }
}

fn emit_match_leaf(term: &MatchTerm, path: &NodePath, depth: usize, cache: &NameCache, out: &mut Vec<Token>) {
    let MatchTerm::Roster { quant, pred } = term else {
        out.push(Token {
            kind: TokenKind::Pill { segments: label::pill_segments(term, cache) },
            path: path.clone(),
            depth,
        });
        return;
    };
    // `roster_sugar_pill_text` and `label::sugar_shape` (which it wraps) are
    // kept in step with `wows-toolkit-config`'s `query_text::try_print_sugar`;
    // see `sugar_shape`'s doc comment for why that matters. This only asks
    // whether the shape is sugar; `pill_segments` (which recognises the same
    // shape) supplies the actual segments for the collapsed pill.
    if label::roster_sugar_pill_text(*quant, pred, cache).is_some() {
        out.push(Token {
            kind: TokenKind::Pill { segments: label::pill_segments(term, cache) },
            path: path.clone(),
            depth,
        });
        return;
    }
    let inner_depth = depth + 1;
    out.push(Token {
        kind: TokenKind::QuantOpen { prefix: label::quant_prefix(*quant) },
        path: path.clone(),
        depth: inner_depth,
    });
    let mut roster_path = path.clone();
    emit_expr(pred, &mut roster_path, inner_depth, true, out, &mut |rterm: &RosterTerm, p, d, out| {
        out.push(Token {
            kind: TokenKind::Pill { segments: label::roster_segments(rterm, cache) },
            path: p.clone(),
            depth: d,
        });
    });
    out.push(Token { kind: TokenKind::QuantClose, path: path.clone(), depth: inner_depth });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::index::query_ast::CmpOp;
    use crate::db::index::query_ast::Expr;
    use crate::db::index::query_ast::MatchField;
    use crate::db::index::query_ast::Op;
    use crate::db::index::query_ast::Quant;
    use crate::db::index::query_ast::RosterField;
    use crate::db::index::query_ast::Value;
    use crate::db::index::rows::MatchOutcome;
    use crate::db::index::rows::VehicleRelation;
    use crate::ui::query_bar::label::NameCache;
    use crate::ui::query_bar::label::SegmentRole;
    use crate::ui::query_bar::select;

    fn win() -> MatchExpr {
        Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win)))
    }
    fn build_ge(n: i64) -> MatchExpr {
        Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Ge, Value::Int(n)))
    }

    /// A term edited as text is typed where its pill was, which means the pill
    /// stops being drawn and the caret takes its place in the stream. Both
    /// halves matter: leaving the pill would show the term twice, and adding a
    /// second caret would give the bar two text fields.
    #[test]
    fn moving_the_caret_onto_a_pill_replaces_it_rather_than_joining_it() {
        let expr = Expr::All(vec![win(), build_ge(3), build_ge(4)]);
        let mut toks = tokenize(&expr, &NameCache::default());
        let before = toks.len();
        move_caret_to(&mut toks, &vec![1]);

        assert_eq!(toks.len(), before - 1, "the caret was added rather than moved: {toks:#?}");
        assert_eq!(toks.iter().filter(|t| matches!(t.kind, TokenKind::Caret)).count(), 1, "{toks:#?}");
        let caret = toks.iter().find(|t| matches!(t.kind, TokenKind::Caret)).expect("the caret");
        assert_eq!(caret.path, vec![1], "the caret must stand where the pill stood");
        assert!(
            !toks.iter().any(|t| matches!(t.kind, TokenKind::Pill { .. }) && t.path == vec![1]),
            "the pill is still drawn beside the editor for it: {toks:#?}"
        );
        // The pills on either side are untouched, so the caret really took one
        // slot rather than the stream being rebuilt around it.
        assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Pill { .. }) && t.path == vec![0]));
        assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Pill { .. }) && t.path == vec![2]));
    }

    /// A nested pill's caret has to indent the way the pill did, or the term
    /// jumps out of the bracket it is being edited inside.
    #[test]
    fn a_moved_caret_keeps_the_pills_depth() {
        let expr = Expr::All(vec![win(), Expr::Any(vec![build_ge(3), build_ge(4)])]);
        let mut toks = tokenize(&expr, &NameCache::default());
        let depth = toks
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Pill { .. }) && t.path == vec![1, 0])
            .expect("the nested pill")
            .depth;
        assert!(depth > 0, "the fixture must nest the pill");

        move_caret_to(&mut toks, &vec![1, 0]);
        let caret = toks.iter().find(|t| matches!(t.kind, TokenKind::Caret)).expect("the caret");
        assert_eq!(caret.depth, depth);
    }

    /// A path naming no pill leaves the stream alone, so the caret stays where
    /// it always was rather than disappearing.
    #[test]
    fn moving_the_caret_onto_nothing_leaves_the_stream_alone() {
        let expr = Expr::All(vec![win(), build_ge(3)]);
        let toks = tokenize(&expr, &NameCache::default());
        let mut moved = toks.clone();
        move_caret_to(&mut moved, &vec![9]);
        assert_eq!(moved, toks);
    }

    #[test]
    fn a_single_leaf_is_one_pill_plus_the_caret() {
        let toks = tokenize(&win(), &NameCache::default());
        assert_eq!(toks.len(), 2, "got {toks:#?}");
        assert!(matches!(toks[0].kind, TokenKind::Pill { .. }));
        assert!(matches!(toks[1].kind, TokenKind::Caret));
    }

    #[test]
    fn the_caret_is_always_last_and_always_present() {
        for expr in [Expr::All(vec![]), win(), Expr::All(vec![win(), build_ge(1)])] {
            let toks = tokenize(&expr, &NameCache::default());
            assert!(matches!(toks.last().unwrap().kind, TokenKind::Caret), "{expr:?}");
            assert_eq!(toks.iter().filter(|t| matches!(t.kind, TokenKind::Caret)).count(), 1);
        }
    }

    #[test]
    fn an_empty_query_is_just_the_caret() {
        assert_eq!(tokenize(&Expr::All(vec![]), &NameCache::default()).len(), 1);
    }

    #[test]
    fn a_conjunction_puts_a_connector_between_pills_but_not_around_them() {
        let toks = tokenize(&Expr::All(vec![win(), build_ge(1)]), &NameCache::default());
        let kinds: Vec<_> = toks.iter().map(|t| std::mem::discriminant(&t.kind)).collect();
        assert_eq!(kinds.len(), 4, "pill, connector, pill, caret: got {toks:#?}");
        assert!(matches!(toks[1].kind, TokenKind::Connector { .. }));
    }

    #[test]
    fn a_nested_group_is_bracketed_and_its_depth_increases() {
        let inner = Expr::Any(vec![win(), build_ge(1)]);
        let toks = tokenize(&Expr::All(vec![win(), inner]), &NameCache::default());
        let open = toks.iter().position(|t| matches!(t.kind, TokenKind::GroupOpen { .. })).expect("an open bracket");
        // `GroupClose` is a unit variant; the `{ .. }` is pinned verbatim from
        // the task brief's test text, not left in by oversight.
        #[allow(clippy::unneeded_struct_pattern)]
        let close = toks.iter().rposition(|t| matches!(t.kind, TokenKind::GroupClose { .. })).expect("a close bracket");
        assert!(open < close);
        assert!(toks[open].depth > toks[0].depth, "the group's contents nest deeper than its siblings");
    }

    #[test]
    fn a_negation_is_marked_and_wraps_its_operand() {
        let toks = tokenize(&Expr::Not(Box::new(win())), &NameCache::default());
        // `NotPrefix` is a unit variant; the `{ .. }` is pinned verbatim from
        // the task brief's test text, not left in by oversight.
        #[allow(clippy::unneeded_struct_pattern)]
        let has_not_prefix = toks.iter().any(|t| matches!(t.kind, TokenKind::NotPrefix { .. }));
        assert!(has_not_prefix, "got {toks:#?}");
    }

    // `Quant::Any` over a bare `Leaf` is a sugar shape (see
    // `label::roster_sugar_pill_text`), so this uses `Quant::Count` to stay a
    // genuine non-sugar case that must still render bracketed.
    #[test]
    fn a_roster_quantifier_renders_as_a_bracketed_group_with_a_prefix() {
        let pred = Expr::Leaf(RosterTerm {
            field: crate::db::index::query_ast::RosterField::Tier,
            op: Op::Eq,
            value: Value::Int(10),
        });
        let expr: MatchExpr =
            Expr::Leaf(MatchTerm::Roster { quant: Quant::Count(crate::db::index::query_ast::CmpOp::Ge, 2), pred });
        let toks = tokenize(&expr, &NameCache::default());
        assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::QuantOpen { .. })), "got {toks:#?}");
    }

    #[test]
    fn a_sugar_shaped_roster_renders_as_one_pill_with_no_quant_open() {
        let pred = Expr::Leaf(RosterTerm {
            field: crate::db::index::query_ast::RosterField::Relation,
            op: Op::Is,
            value: Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
        });
        let expr: MatchExpr = Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred });
        let toks = tokenize(&expr, &NameCache::default());
        assert_eq!(toks.len(), 2, "one pill plus the caret: got {toks:#?}");
        assert!(matches!(toks[0].kind, TokenKind::Pill { .. }));
        assert!(!toks.iter().any(|t| matches!(t.kind, TokenKind::QuantOpen { .. })), "got {toks:#?}");
    }

    #[test]
    fn a_non_sugar_roster_predicate_still_renders_bracketed() {
        // `try_print_sugar` only recognises `Expr::All` of exactly two; an
        // `Any` of two relation leaves is never sugar, however its leaves read.
        let pred = Expr::Any(vec![
            Expr::Leaf(RosterTerm {
                field: crate::db::index::query_ast::RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(crate::db::index::rows::VehicleRelation::Enemy),
            }),
            Expr::Leaf(RosterTerm {
                field: crate::db::index::query_ast::RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(crate::db::index::rows::VehicleRelation::Ally),
            }),
        ]);
        let expr: MatchExpr = Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred });
        let toks = tokenize(&expr, &NameCache::default());
        assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::QuantOpen { .. })), "got {toks:#?}");
        assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::QuantClose)), "got {toks:#?}");
    }

    /// Every tree shape whose pills the resolvers have to reach: the match
    /// level, then all four quantifiers against four predicate shapes.
    ///
    /// The roster half is a cross product rather than hand-picked cases,
    /// because the failure this sweep exists to catch is shape-specific and one
    /// instance of it says nothing about the rest. Every roster tree is nested
    /// under a sibling so its pill paths are non-empty, which is what stops a
    /// resolver that ignored the path from passing by accident.
    fn resolver_fixtures() -> Vec<MatchExpr> {
        let tier = || Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) });
        let kills = || Expr::Leaf(RosterTerm { field: RosterField::Kills, op: Op::Ge, value: Value::Int(2) });
        let scope = || {
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(VehicleRelation::Enemy),
            })
        };
        // A bare leaf, the scope-conjunct form sugar collapses, a plain `All`
        // of two, and a negation. The first two are the shapes that leave the
        // resolver an empty tail; the last two leave it a real one.
        let preds =
            [tier(), Expr::All(vec![scope(), tier()]), Expr::All(vec![tier(), kills()]), Expr::Not(Box::new(tier()))];
        let quants = [Quant::Any, Quant::None, Quant::Count(CmpOp::Ge, 2), Quant::Count(CmpOp::Lt, 1)];

        let mut out = vec![Expr::All(vec![win(), Expr::Any(vec![build_ge(1), Expr::Not(Box::new(win()))])])];
        for quant in quants {
            for pred in &preds {
                out.push(Expr::All(vec![win(), Expr::Leaf(MatchTerm::Roster { quant, pred: pred.clone() })]));
            }
        }
        out
    }

    /// Asserted through the resolvers editing actually uses -- `segment_path`
    /// and `term_at` for a pill, `addresses_match_node` for the chrome -- not
    /// through a second walker of this module's own. A path that resolves under
    /// one walker and not the other is exactly the failure worth catching, and
    /// only the production pair can catch it.
    ///
    /// Chrome drawn inside a quantifier's bracket names no match-level node by
    /// design, which is what keeps the toolbar from offering edits it cannot
    /// make (`a_roster_internal_pill_is_drawn_but_not_selectable`), so the
    /// chrome assertion is bounded to the match level. Bracket depth is counted
    /// off the token stream itself rather than asked of a resolver, so the
    /// assertion does not answer its own question.
    #[test]
    fn every_token_path_resolves_under_the_resolvers_editing_uses() {
        for expr in resolver_fixtures() {
            let toks = tokenize(&expr, &NameCache::default());
            let mut pills = 0;
            let mut depth = 0usize;
            for t in &toks {
                if matches!(t.kind, TokenKind::QuantClose) {
                    depth -= 1;
                }
                match &t.kind {
                    TokenKind::Caret => {}
                    TokenKind::Pill { .. } => {
                        pills += 1;
                        let path = select::segment_path(&expr, &t.path)
                            .unwrap_or_else(|| panic!("pill path {:?} addresses no term in {expr:?}", t.path));
                        assert!(select::term_at(&expr, &path).is_some(), "path {path:?} names no term in {expr:?}");
                    }
                    _ if depth > 0 => {}
                    _ => assert!(
                        select::addresses_match_node(&expr, &t.path),
                        "path {:?} names no node in {expr:?}",
                        t.path
                    ),
                }
                if matches!(t.kind, TokenKind::QuantOpen { .. }) {
                    depth += 1;
                }
            }
            assert!(pills > 0, "a fixture with no pill proves nothing: {expr:?}");
        }
    }

    #[test]
    fn a_sugar_collapsed_pills_path_resolves_to_the_term_it_draws() {
        // The sugar branch pushes its `Pill` at the quantifier's own
        // (unextended) path, so the segment resolver has to walk on into the
        // predicate to reach the term the pill actually renders. A regression
        // here would retarget every click on the pill.
        //
        // The roster term is nested under a sibling (`Expr::All([win(), ..])`)
        // rather than placed at the tree's root: at the root the pill's path is
        // `[]`, and a resolver that ignored the path entirely would still land
        // on the right node, so the assertion would hold no matter what path
        // the sugar branch computed.
        let inner =
            RosterTerm { field: RosterField::Relation, op: Op::Is, value: Value::Relation(VehicleRelation::Enemy) };
        let roster_leaf: MatchExpr =
            Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred: Expr::Leaf(inner.clone()) });
        let expr = Expr::All(vec![win(), roster_leaf]);
        let toks = tokenize(&expr, &NameCache::default());
        let pill = toks
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Pill { .. }) && t.path == vec![1])
            .unwrap_or_else(|| panic!("expected the roster pill at path [1], got {toks:#?}"));

        let path = select::segment_path(&expr, &pill.path).expect("a sugar-collapsed pill addresses a term");
        let (field, op, value) = select::term_at(&expr, &path).expect("the term the pill draws");
        assert_eq!(field, crate::ui::query_bar::suggest::TermField::Roster(inner.field));
        assert_eq!(op, inner.op);
        assert_eq!(*value, inner.value);
    }

    /// A bracketed quantifier whose predicate is a single leaf draws its one
    /// inner pill on the quantifier's own path, so the tail is empty exactly as
    /// it is for a sugar-collapsed pill even though the shape is not sugar.
    /// That pill must still address the leaf, or every segment on it would be
    /// inert.
    #[test]
    fn a_bracketed_quantifier_over_one_leaf_still_addresses_that_leaf() {
        let inner = RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) };
        let roster_leaf: MatchExpr =
            Expr::Leaf(MatchTerm::Roster { quant: Quant::Count(CmpOp::Ge, 2), pred: Expr::Leaf(inner.clone()) });
        let expr = Expr::All(vec![win(), roster_leaf]);
        let toks = tokenize(&expr, &NameCache::default());
        assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::QuantOpen { .. })), "the fixture must be bracketed");
        let roster_pill = toks
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Pill { .. }) && t.path.starts_with(&[1]))
            .expect("a roster pill token");

        let path = select::segment_path(&expr, &roster_pill.path).expect("the inner pill addresses its own term");
        let (field, op, value) = select::term_at(&expr, &path).expect("the term the pill draws");
        assert_eq!(field, crate::ui::query_bar::suggest::TermField::Roster(inner.field));
        assert_eq!(op, inner.op);
        assert_eq!(*value, inner.value);
    }

    #[test]
    fn paths_are_stable_across_two_tokenizations_of_the_same_tree() {
        let expr = Expr::All(vec![win(), build_ge(1)]);
        let a = tokenize(&expr, &NameCache::default());
        let b = tokenize(&expr, &NameCache::default());
        let pa: Vec<_> = a.iter().map(|t| t.path.clone()).collect();
        let pb: Vec<_> = b.iter().map(|t| t.path.clone()).collect();
        assert_eq!(pa, pb);
    }

    #[test]
    fn a_pill_token_carries_its_segments() {
        let toks = tokenize(&win(), &NameCache::default());
        match &toks[0].kind {
            TokenKind::Pill { segments } => {
                assert_eq!(segments.len(), 3, "got {segments:?}");
                assert_eq!(segments[0].role, SegmentRole::Filter);
                assert_eq!(segments[1].role, SegmentRole::Operator);
                assert_eq!(segments[2].role, SegmentRole::Value);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn a_sugar_shaped_roster_is_still_one_pill_with_segments() {
        let pred = Expr::Leaf(RosterTerm {
            field: RosterField::Relation,
            op: Op::Is,
            value: Value::Relation(VehicleRelation::Enemy),
        });
        let expr: MatchExpr = Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred });
        let toks = tokenize(&expr, &NameCache::default());
        assert!(!toks.iter().any(|t| matches!(t.kind, TokenKind::QuantOpen { .. })), "got {toks:#?}");
        let pills: Vec<_> = toks.iter().filter(|t| matches!(t.kind, TokenKind::Pill { .. })).collect();
        assert_eq!(pills.len(), 1);
    }

    #[test]
    fn a_non_sugar_roster_still_brackets_and_its_inner_pills_are_segmented() {
        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(VehicleRelation::Enemy),
            }),
            Expr::Leaf(RosterTerm { field: RosterField::Damage, op: Op::Gt, value: Value::Int(100_000) }),
        ]);
        let expr: MatchExpr = Expr::Leaf(MatchTerm::Roster { quant: Quant::Count(CmpOp::Ge, 2), pred });
        let toks = tokenize(&expr, &NameCache::default());
        assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::QuantOpen { .. })), "got {toks:#?}");
        for t in &toks {
            if let TokenKind::Pill { segments } = &t.kind {
                assert!(segments.len() >= 2, "inner pill not segmented: {segments:?}");
            }
        }
    }
}
