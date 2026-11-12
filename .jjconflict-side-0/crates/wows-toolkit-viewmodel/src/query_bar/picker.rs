//! What a query bar's pills and completions offer: the choices behind a pill's field, operator and
//! value segments, the edits its menu makes to the query's shape, and the completions for the
//! fragment under the caret.
//!
//! The bar draws these; deciding them here keeps the front ends' bars from disagreeing about what
//! a query may be changed to.

use wows_toolkit_config::index::query_ast::Expr;
use wows_toolkit_config::index::query_ast::MatchExpr;
use wows_toolkit_config::index::query_ast::MatchField;
use wows_toolkit_config::index::query_ast::Op;
use wows_toolkit_config::index::query_ast::OperatorPreferences;
use wows_toolkit_config::index::query_text;

use crate::query_bar::label;
use crate::query_bar::label::SegmentRole;
use crate::query_bar::select;
use crate::query_bar::select::Selection;
use crate::query_bar::suggest;
use crate::query_bar::suggest::SuggestionKind;
use crate::query_bar::suggest::TermField;
use crate::query_bar::tokens::NodePath;

/// How many completions the dropdown offers at once. The egui bar shows the same number before
/// it scrolls.
pub const MAX_SUGGESTIONS: usize = 8;

/// Which part of a term a picker is editing.
///
/// The value segment only opens a picker for a field whose values enumerate; a free number or a
/// name is typed in the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditablePart {
    Field,
    Operator,
    Value,
}

impl EditablePart {
    pub fn of(role: SegmentRole) -> Self {
        match role {
            SegmentRole::Filter => Self::Field,
            SegmentRole::Operator => Self::Operator,
            SegmentRole::Value => Self::Value,
        }
    }
}

/// One choice a picker offers, and the query text taking it produces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub taken: String,
    /// Whether this is what the term already says.
    pub current: bool,
}

/// An operator a pill's operator segment may be changed to.
pub struct OperatorChoice {
    pub op: Op,
    pub label: String,
}

/// The operators the term at `path` accepts, and which one it currently carries.
///
/// `None` for a path that names no single term, which is every path stopping on a roster
/// quantifier with more than one leaf: there is no one operator to change.
pub fn operator_choices(expr: &MatchExpr, path: &[usize]) -> Option<(Vec<OperatorChoice>, Op)> {
    let (allowed, current) = select::term_op_at(expr, path)?;
    let choices = allowed
        .iter()
        .copied()
        .filter(|op| select::can_set_op(expr, path, *op))
        .map(|op| OperatorChoice { op, label: label::op_label(op) })
        .collect();
    Some((choices, current))
}

/// `expr` with the operator at `path` changed, printed back as query text.
///
/// `None` when the edit does not apply, which is what `can_set_op` refuses: the bar then leaves
/// the text alone rather than writing a query that says something else.
pub fn with_operator(expr: &MatchExpr, path: &[usize], op: Op) -> Option<String> {
    let mut edited = expr.clone();
    select::set_op(&mut edited, path, op).then(|| query_text::print_query(&edited))
}

/// What the picker for `part` of the term at `path` offers, with no operator remembered per
/// field.
pub fn choices(expr: &MatchExpr, path: &[usize], part: EditablePart) -> Vec<Choice> {
    choices_with_operator_preferences(expr, path, part, &OperatorPreferences::default())
}

/// What the picker for `part` of the term at `path` offers.
///
/// Empty when that part has nothing to pick from, which is a value that is typed rather than
/// chosen; the bar then opens no picker at all rather than an empty one.
pub fn choices_with_operator_preferences(
    expr: &MatchExpr,
    path: &[usize],
    part: EditablePart,
    operator_preferences: &OperatorPreferences,
) -> Vec<Choice> {
    match part {
        EditablePart::Operator => operator_choices(expr, path)
            .map(|(choices, current)| {
                choices
                    .into_iter()
                    .filter_map(|choice| {
                        Some(Choice {
                            label: choice.label,
                            taken: with_operator(expr, path, choice.op)?,
                            current: choice.op == current,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        EditablePart::Field => field_choices(expr, path, operator_preferences),
        EditablePart::Value => value_choices(expr, path),
    }
}

/// Every match-level field this term could be instead.
///
/// Roster terms are left alone: their field list is a different one and changing it reshapes the
/// quantifier around it.
fn field_choices(expr: &MatchExpr, path: &[usize], operator_preferences: &OperatorPreferences) -> Vec<Choice> {
    let Some((current, _, _)) = select::term_at(expr, path) else { return Vec::new() };
    if !matches!(current, TermField::Match(_)) {
        return Vec::new();
    }

    // The operator a field was last given, so switching to it lands on the same operator the user
    // chose before rather than the field's default.
    let mut prefs = operator_preferences.clone();
    select::record_operators(expr, &mut prefs);

    MatchField::ALL
        .iter()
        .copied()
        .filter_map(|field| {
            let mut edited = expr.clone();
            select::set_field(&mut edited, &path.to_vec(), TermField::Match(field), &prefs).then(|| Choice {
                label: label::match_field_label(field),
                taken: query_text::print_query(&edited),
                current: current == TermField::Match(field),
            })
        })
        .collect()
}

/// The values this term's field enumerates, when it enumerates any.
fn value_choices(expr: &MatchExpr, path: &[usize]) -> Vec<Choice> {
    let Some((field, _, current)) = select::term_at(expr, path) else { return Vec::new() };
    let Some(offered) = query_text::enumerable_values(field.value_kind()) else { return Vec::new() };

    let current_text = query_text::print_value(current);
    offered
        .into_iter()
        .filter_map(|raw| {
            let value = query_text::parse_roster_value(field.value_kind(), &raw)?;
            let is_current = query_text::print_value(&value) == current_text;
            let mut edited = expr.clone();
            select::set_value(&mut edited, path, value).then(|| Choice {
                label: raw,
                taken: query_text::print_query(&edited),
                current: is_current,
            })
        })
        .collect()
}

/// What a pill's own menu can do to the query's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructuralEdit {
    /// Add this pill to the selection, or take it out again.
    ToggleSelected,
    /// Wrap the selection in a group of its own.
    Group { is_or: bool },
    /// Dissolve the group this pill is in, lifting its members into the group above.
    Ungroup,
    /// Put a `not` in front of this pill, or take the one that is there off.
    Negate,
    /// Drop the selection from the query.
    Delete,
    /// Swap the connector joining this pill to its siblings.
    FlipConnector,
}

/// The path of `path`'s parent. The root's parent is the root.
pub fn parent_of(path: &[usize]) -> NodePath {
    path.iter().copied().take(path.len().saturating_sub(1)).collect()
}

/// `expr` after `edit`, made from the pill at `path`'s menu, canonicalised.
///
/// The edit acts on `selection` where there is one and on the pill at `path` where there is not,
/// so a right-click on an unselected pill still does the obvious thing.
/// [`StructuralEdit::ToggleSelected`] changes the selection rather than the query and is
/// returned unchanged.
pub fn apply_structural_edit(
    mut expr: MatchExpr,
    path: &NodePath,
    edit: StructuralEdit,
    selection: &Selection,
) -> MatchExpr {
    let target = if selection.is_empty() {
        let mut one = Selection::default();
        one.set_one(path.clone());
        one
    } else {
        selection.clone()
    };

    match edit {
        StructuralEdit::ToggleSelected => return expr,
        StructuralEdit::Group { is_or } => select::group(&mut expr, &target, is_or),
        StructuralEdit::Ungroup => {
            select::ungroup(&mut expr, path);
        }
        StructuralEdit::Negate => select::negate(&mut expr, path),
        StructuralEdit::Delete => select::delete(&mut expr, &target),
        StructuralEdit::FlipConnector => {
            let is_or = matches!(select::node_at(&expr, &parent_of(path)), Some(Expr::Any(_)));
            select::set_connector(&mut expr, path, !is_or);
        }
    }
    select::canonicalise(&mut expr);
    expr
}

/// Which of a pill menu's edits apply, for the pill at `path`.
///
/// Everything the menu offers is refused rather than hidden when it does not apply: a menu whose
/// items move around is harder to learn than one whose items grey out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PillMenuState {
    pub selected: bool,
    pub can_group: bool,
    pub can_ungroup: bool,
    pub can_delete: bool,
}

impl PillMenuState {
    pub fn new(expr: &MatchExpr, path: &[usize], selection: &Selection) -> Self {
        Self {
            selected: selection.contains(path),
            can_group: select::can_group(expr, selection) && !selection.is_empty(),
            can_ungroup: path.len() > 1,
            can_delete: !selection.is_empty(),
        }
    }
}

/// One completion the bar is offering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// What the row shows.
    pub label: String,
    /// The breadcrumb after it.
    pub context: String,
    /// The query text this completion produces when taken.
    pub replacement: String,
}

/// The completions for the fragment the caret sits in, best first.
///
/// `query` is the whole bar text; the fragment is the part after the last boundary, which is what
/// [`suggest::active_fragment`] decides. An empty fragment offers everything, which is how the bar
/// opens its dropdown on a fresh query.
pub fn completions(query: &str) -> Vec<Completion> {
    let fragment = suggest::active_fragment(query);
    let all = suggest::static_suggestions();
    // The bar keeps no record of which operator a field was last given, so a field is offered with
    // the leading one it allows.
    let prefs = OperatorPreferences::default();

    suggest::rank(fragment, &all)
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .filter_map(|index| {
            let suggestion = &all[index];
            // A suggestion is read as a phrase and written as grammar: the label says "Enemy
            // ship", the query takes `enemy.ship:`.
            let taken = match &suggestion.kind {
                SuggestionKind::MatchField(field) => suggest::match_field_prefix(*field, &prefs)?,
                SuggestionKind::RosterField { field, scope } => {
                    suggest::roster_field_prefix(*field, scope.unwrap_or(suggest::Scope::Anyone), &prefs)?
                }
                SuggestionKind::Preset(key) => {
                    let preset = suggest::PRESETS.iter().find(|preset| preset.key == *key)?;
                    query_text::print_query(&(preset.build)())
                }
            };
            Some(Completion {
                label: suggestion.label.clone(),
                context: category_label(suggestion.context),
                replacement: suggest::replace_active_fragment(query, &taken),
            })
        })
        .collect()
}

/// The values the index answered for the caret's field, as completions of `text`.
pub fn value_completions(text: &str, options: &[suggest::ValueOption]) -> Vec<Completion> {
    options
        .iter()
        .map(|option| Completion {
            label: option.label.clone(),
            context: "value".to_string(),
            replacement: suggest::replace_active_value(text, &option.token),
        })
        .collect()
}

/// The breadcrumb text for a suggestion's category.
///
/// The category is typed rather than a string so this match is exhaustive: a new one cannot reach
/// the screen unnamed.
fn category_label(category: suggest::SuggestionCategory) -> String {
    match category {
        suggest::SuggestionCategory::Preset => "preset".to_string(),
        suggest::SuggestionCategory::Match => "match".to_string(),
        suggest::SuggestionCategory::Roster => "player".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use wows_toolkit_config::index::query_text;

    use super::EditablePart;
    use super::StructuralEdit;
    use super::apply_structural_edit;
    use super::choices;
    use super::completions;
    use super::operator_choices;
    use super::with_operator;
    use crate::query_bar::label::NameCache;
    use crate::query_bar::select;
    use crate::query_bar::select::Selection;
    use crate::query_bar::suggest;
    use crate::query_bar::tokens;

    fn first_pill(expr: &wows_toolkit_config::index::query_ast::MatchExpr) -> Vec<usize> {
        select::pill_paths(&tokens::tokenize(expr, &NameCache::default())).first().cloned().expect("one pill")
    }

    /// The fragment under the caret is what gets completed; the finished part of the query in front
    /// of it survives. The needle comes from a real suggestion so the test keeps exercising
    /// something when the vocabulary changes.
    #[test]
    fn a_completion_replaces_only_the_fragment_under_the_caret() {
        let first = suggest::static_suggestions().first().expect("there are suggestions").label.clone();
        let needle: String = first.chars().take(3).collect();

        let offered = completions(&format!("map:ocean {needle}"));

        assert!(!offered.is_empty(), "a fragment of a real label offers it back: {needle:?}");
        for completion in &offered {
            assert!(completion.replacement.starts_with("map:ocean "), "{:?}", completion.replacement);
            assert!(!completion.replacement.contains(&format!("{needle}{needle}")), "replaced, not appended to");
        }
    }

    #[test]
    fn no_more_than_the_dropdown_can_show_are_offered() {
        assert!(completions("").len() <= super::MAX_SUGGESTIONS);
    }

    /// The operator picker offers what the term accepts, and taking one rewrites only that
    /// operator.
    #[test]
    fn an_operator_can_be_changed_from_its_pill() {
        let expr = query_text::parse_query("build>9000000").expect("the fixture parses");
        let path = first_pill(&expr);

        let (offered, current) = operator_choices(&expr, &path).expect("a single term has an operator");
        assert!(offered.len() > 1, "there is something to change it to");
        assert!(offered.iter().all(|choice| !choice.label.is_empty()), "every choice reads as something");

        let other = offered.iter().find(|choice| choice.op != current).expect("another operator");
        let rewritten = with_operator(&expr, &path, other.op).expect("the edit applies");

        assert_ne!(rewritten, query_text::print_query(&expr), "the query changed");
        assert!(rewritten.contains("build"), "and it is still the same term");
        let reparsed = query_text::parse_query(&rewritten).expect("what it prints, it can read back");
        let (_, op, _) = select::term_at(&reparsed, &path).expect("still one term");
        assert_eq!(op, other.op, "the operator is the one taken");
    }

    /// The field segment offers every match field, and taking one keeps the term.
    #[test]
    fn a_field_can_be_changed_from_its_pill() {
        let expr = query_text::parse_query("map:ocean").expect("the fixture parses");
        let path = first_pill(&expr);

        let offered = choices(&expr, &path, EditablePart::Field);
        assert!(offered.len() > 1, "there are other fields to choose");
        assert_eq!(offered.iter().filter(|choice| choice.current).count(), 1, "exactly one is the current field");

        let other = offered.iter().find(|choice| !choice.current).expect("another field");
        let reparsed = query_text::parse_query(&other.taken).expect("what it prints, it reads back");
        let (field, _, _) = select::term_at(&reparsed, &path).expect("still one term");
        let (before, _, _) = select::term_at(&expr, &path).expect("a term");
        assert_ne!(field, before, "the field changed");
    }

    /// A field whose values enumerate offers them; one that is typed offers nothing, so the bar
    /// opens no picker for it.
    #[test]
    fn only_an_enumerable_value_offers_a_picker() {
        let enumerated = query_text::parse_query("outcome=win").expect("the fixture parses");
        let path = first_pill(&enumerated);
        let offered = choices(&enumerated, &path, EditablePart::Value);
        assert!(offered.len() > 1, "every outcome is offered");
        assert!(offered.iter().any(|choice| choice.current), "one of them is the current value");
        let other = offered.iter().find(|choice| !choice.current).expect("another outcome");
        assert!(query_text::parse_query(&other.taken).is_ok(), "taking it parses: {:?}", other.taken);

        let typed = query_text::parse_query("build>9000000").expect("the fixture parses");
        let path = first_pill(&typed);
        assert!(choices(&typed, &path, EditablePart::Value).is_empty(), "a free number offers no picker");
    }

    #[test]
    fn every_suggestion_carries_a_breadcrumb() {
        for suggestion in suggest::static_suggestions() {
            assert!(!super::category_label(suggestion.context).is_empty());
        }
    }

    /// Deleting the only selected pill of two leaves the other, and negating one wraps it.
    #[test]
    fn a_pill_menu_edit_reshapes_the_query() {
        let expr = query_text::parse_query("map:ocean outcome=win").expect("the fixture parses");
        let paths = select::pill_paths(&tokens::tokenize(&expr, &NameCache::default()));
        let mut selection = Selection::default();
        selection.set_one(paths[0].clone());

        let deleted = apply_structural_edit(expr.clone(), &paths[0], StructuralEdit::Delete, &selection);
        let printed = query_text::print_query(&deleted);
        assert!(!printed.contains("ocean") && printed.contains("win"), "{printed}");

        let negated = apply_structural_edit(expr.clone(), &paths[1], StructuralEdit::Negate, &Selection::default());
        let printed = query_text::print_query(&negated);
        assert_ne!(printed, query_text::print_query(&expr), "negating changes the query");
        let twice = apply_structural_edit(negated, &paths[1], StructuralEdit::Negate, &Selection::default());
        assert_eq!(query_text::print_query(&twice), query_text::print_query(&expr), "and negating again undoes it");
    }
}
