//! Selection over the token stream's node paths, and the tree edits a selection
//! authorises. Pure so the boolean-editing rules are testable without egui.

// Consumed by later query-bar tasks (hit-testing, the filter builder); no call
// site in this crate yet.
#![allow(dead_code)]

use crate::db::index::query_ast::Expr;
use crate::db::index::query_ast::MatchExpr;
use crate::db::index::query_ast::MatchTerm;
use crate::db::index::query_ast::Quant;
use crate::ui::query_bar::tokens::NodePath;

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

    let Some(children) = children_mut(expr, parent_path) else {
        return;
    };
    let insert_at = indices[0];
    let mut taken = Vec::with_capacity(indices.len());
    for &i in indices.iter().rev() {
        taken.push(children.remove(i));
    }
    taken.reverse();
    let new_node = if is_or { Expr::Any(taken) } else { Expr::All(taken) };
    children.insert(insert_at, new_node);
}

/// Splices the children of the `All`/`Any` node at `path` into its parent, in
/// place of that node.
pub fn ungroup(expr: &mut MatchExpr, path: &NodePath) {
    let Some((parent_path, index)) = parent_and_index(path) else {
        return;
    };
    let Some(node) = node_at_mut(expr, path) else {
        return;
    };
    let taken = match node {
        Expr::All(cs) | Expr::Any(cs) => std::mem::take(cs),
        _ => return,
    };
    let Some(children) = children_mut(expr, parent_path) else {
        return;
    };
    children.remove(index);
    for (offset, child) in taken.into_iter().enumerate() {
        children.insert(index + offset, child);
    }
}

/// Negates the node at `path` in place: flips a leaf's operator or quantifier
/// when an inverse exists that the field still allows, unwraps an existing
/// `Not`, and otherwise wraps the node in `Not`.
pub fn negate(expr: &mut MatchExpr, path: &NodePath) {
    let Some((parent_path, index)) = parent_and_index(path) else {
        return;
    };
    let Some(children) = children_mut(expr, parent_path) else {
        return;
    };
    let Some(node) = children.get_mut(index) else {
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
/// one still to be removed.
pub fn delete(expr: &mut MatchExpr, sel: &Selection) {
    let mut paths = sel.nodes.clone();
    paths.sort_by(|a, b| b.cmp(a));
    for path in paths {
        let Some((parent_path, index)) = parent_and_index(&path) else {
            continue;
        };
        if let Some(children) = children_mut(expr, parent_path)
            && index < children.len()
        {
            children.remove(index);
        }
    }
}

/// Enforces the two invariants Plan A's printer requires: no `All`/`Any` with
/// exactly one child (it would print as a bare term and reparse as `Leaf`),
/// and no `All`/`Any` with zero children except a root left as the canonical
/// empty query. Bottom-up.
///
/// Not idempotent in general: a node that already has exactly one child when
/// this runs is collapsed into that child (see `canonicalise_conjunction`), so
/// re-running it on its own output can collapse further. Every edit function
/// in this module produces at least two children wherever it introduces a new
/// `All`/`Any`, so a single call after each edit is enough in practice.
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

/// A node with exactly one child, before recursing into it, is itself the
/// spurious wrapper the printer cannot round trip (`All([x])` prints as bare
/// `x`), so it always collapses to that child's canonicalised form.
///
/// A node with two or more children that loses siblings to recursive
/// canonicalisation (an operand that canonicalised away entirely) keeps its
/// own wrapper even if only one child survives: it was a genuine multi-operand
/// group as authored, not the printer's spurious shape, so there is nothing to
/// fix by unwrapping it. `canonicalise_removes_an_empty_group_but_keeps_an_empty_root`
/// pins exactly this: `All([Any([]), x])` canonicalises to `All([x])`, not `x`.
fn canonicalise_conjunction(cs: Vec<MatchExpr>, is_or: bool) -> Option<MatchExpr> {
    if cs.len() == 1 {
        let mut it = cs.into_iter();
        return canonicalise_node(it.next().expect("len checked above"));
    }
    let out: Vec<MatchExpr> = cs.into_iter().filter_map(canonicalise_node).collect();
    if out.is_empty() { None } else { Some(if is_or { Expr::Any(out) } else { Expr::All(out) }) }
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

    fn leaf(n: i64) -> MatchExpr {
        Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Eq, Value::Int(n)))
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
        ungroup(&mut e, &vec![1]);
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
        assert_eq!(e, Expr::All(vec![leaf(1)]));

        let mut root: MatchExpr = Expr::All(vec![]);
        canonicalise(&mut root);
        assert_eq!(root, Expr::All(vec![]));
    }

    #[test]
    fn deleting_the_last_child_leaves_the_canonical_empty_query() {
        let mut e = Expr::All(vec![leaf(1)]);
        delete(&mut e, &sel(&[&[0]]));
        canonicalise(&mut e);
        assert!(e.is_empty_all());
    }

    #[test]
    fn every_edit_leaves_a_tree_that_survives_a_print_parse_round_trip() {
        use crate::db::index::query_text::parse_query;
        use crate::db::index::query_text::print_query;
        let mut e = Expr::All(vec![leaf(1), leaf(2), leaf(3)]);
        for edit in 0..3 {
            match edit {
                0 => group(&mut e, &sel(&[&[0], &[1]]), true),
                1 => negate(&mut e, &vec![0]),
                _ => delete(&mut e, &sel(&[&[1]])),
            }
            canonicalise(&mut e);
            let printed = print_query(&e);
            let reparsed = parse_query(&printed).unwrap_or_else(|err| panic!("edit {edit} printed {printed:?}: {err}"));
            assert_eq!(reparsed, e, "edit {edit} printed {printed:?}");
        }
    }
}
