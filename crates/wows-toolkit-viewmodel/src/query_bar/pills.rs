//! What a drawn pill offers: the picks behind each of its segments, the edits
//! its menu makes to the query's shape, and the completions the caret
//! fragment offers.
//!
//! Every front end draws these its own way; what each pick or edit does to
//! the query is decided here once.

use rust_i18n::t;
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
use crate::query_bar::suggest::ValueOption;
use crate::query_bar::tokens::NodePath;

/// How many completions the dropdown offers at once. The egui bar shows the
/// same number before it scrolls.
pub const MAX_SUGGESTIONS: usize = 8;

/// How many structural edits can be stepped back through. Deep enough to
/// cover a session's worth of reshaping, bounded so a long one does not grow
/// without limit.
pub const UNDO_DEPTH: usize = 64;

/// Which part of a term a picker is editing.
///
/// The value segment only opens a picker for a field whose values enumerate;
/// a free number or a name is typed in the bar, as it is in the egui one.
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
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub label: String,
    pub taken: String,
    /// Whether this is what the term already says.
    pub current: bool,
}

/// An operator a pill's operator segment may be changed to.
#[derive(Debug, Clone, PartialEq)]
pub struct OperatorChoice {
    pub op: Op,
    pub label: String,
}

/// What a pill's own menu can do to the query's shape.
///
/// The edits themselves are [`select`]'s, shared with the egui bar; this only
/// names which one was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructuralEdit {
    /// Add this pill to the selection, or take it out again.
    ToggleSelected,
    /// Wrap the selection in a group of its own.
    Group { is_or: bool },
    /// Dissolve the group this pill is in, lifting its members into the
    /// group above.
    Ungroup,
    /// Put a `not` in front of this pill, or take the one that is there off.
    Negate,
    /// Drop the selection from the query.
    Delete,
    /// Swap the connector joining this pill to its siblings.
    FlipConnector,
}

/// Which of a pill menu's items apply. Every item is listed whether or not it
/// applies: a menu whose items move around is harder to learn than one whose
/// items grey out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PillMenu {
    pub selected: bool,
    pub can_group: bool,
    pub can_ungroup: bool,
    pub can_delete: bool,
}

/// One completion the bar is offering.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    /// What the row shows.
    pub label: String,
    /// The breadcrumb after it, already translated.
    pub context: String,
    /// The query text this completion produces when taken.
    pub replacement: String,
}

/// The operators the term at `path` accepts, and which one it currently
/// carries.
///
/// `None` for a path that names no single term, which is every path stopping
/// on a roster quantifier with more than one leaf: there is no one operator
/// to change.
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
/// `None` when the edit does not apply, which is what `can_set_op` refuses:
/// the bar then leaves the text alone rather than writing a query that says
/// something else.
pub fn with_operator(expr: &MatchExpr, path: &[usize], op: Op) -> Option<String> {
    let mut edited = expr.clone();

    select::set_op(&mut edited, path, op).then(|| query_text::print_query(&edited))
}

/// What the picker for `part` of the term at `path` offers.
///
/// Empty when that part has nothing to pick from, which is a value that is
/// typed rather than chosen; the bar then opens no picker at all rather than
/// an empty one.
pub fn choices(
    expr: &MatchExpr,
    path: &[usize],
    part: EditablePart,
    operator_preferences: &OperatorPreferences,
) -> Vec<Choice> {
    match part {
        EditablePart::Operator => operator_segment_choices(expr, path),
        EditablePart::Field => field_choices(expr, path, operator_preferences),
        EditablePart::Value => value_choices(expr, path),
    }
}

/// The menu a pill at `path` opens, given what is selected.
pub fn pill_menu(expr: &MatchExpr, path: &[usize], selection: &Selection) -> PillMenu {
    PillMenu {
        selected: selection.contains(path),
        can_group: select::can_group(expr, selection) && !selection.is_empty(),
        can_ungroup: path.len() > 1,
        can_delete: !selection.is_empty(),
    }
}

/// `expr` after a pill menu's `edit`, asked for from the pill at `path`.
///
/// The edit acts on the selection where there is one and on the pill it was
/// asked from where there is not, so a right-click on an unselected pill
/// still does the obvious thing. `None` for [`StructuralEdit::ToggleSelected`],
/// which changes the selection rather than the query.
pub fn apply_structural_edit(
    expr: &MatchExpr,
    path: &NodePath,
    edit: StructuralEdit,
    selection: &Selection,
) -> Option<MatchExpr> {
    let target = if selection.is_empty() {
        let mut one = Selection::default();
        one.set_one(path.clone());
        one
    } else {
        selection.clone()
    };

    let mut edited = expr.clone();
    match edit {
        StructuralEdit::ToggleSelected => return None,
        StructuralEdit::Group { is_or } => select::group(&mut edited, &target, is_or),
        StructuralEdit::Ungroup => {
            select::ungroup(&mut edited, path);
        }
        StructuralEdit::Negate => select::negate(&mut edited, path),
        StructuralEdit::Delete => select::delete(&mut edited, &target),
        StructuralEdit::FlipConnector => {
            let is_or = matches!(select::node_at(&edited, &parent_of(path)), Some(Expr::Any(_)));
            select::set_connector(&mut edited, path, !is_or);
        }
    }
    select::canonicalise(&mut edited);

    Some(edited)
}

/// The completions for the fragment the caret sits in, best first.
///
/// `query` is the whole bar text; the fragment is the part after the last
/// boundary, which is what [`suggest::active_fragment`] decides. An empty
/// fragment offers everything, which is how the egui bar opens its dropdown
/// on a fresh query.
pub fn completions(query: &str) -> Vec<Completion> {
    let fragment = suggest::active_fragment(query);
    let all = suggest::static_suggestions();
    // The bar keeps no record of which operator a field was last given, so a
    // field is offered with the leading one it allows.
    let prefs = OperatorPreferences::default();

    suggest::rank(fragment, &all)
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .filter_map(|index| {
            let suggestion = &all[index];
            // A suggestion is read as a phrase and written as grammar: the
            // label says "Enemy ship", the query takes `enemy.ship:`.
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

/// The index's values for the caret's field as completions of `query`.
pub fn value_completions(query: &str, options: &[ValueOption]) -> Vec<Completion> {
    options
        .iter()
        .map(|option| Completion {
            label: option.label.clone(),
            context: t!("ui.search.value_context").into_owned(),
            replacement: suggest::replace_active_value(query, &option.token),
        })
        .collect()
}

/// The breadcrumb text for a suggestion's category.
///
/// The category is typed rather than a string precisely so this match is
/// exhaustive: a new one cannot reach the screen untranslated.
pub fn category_label(category: suggest::SuggestionCategory) -> String {
    match category {
        suggest::SuggestionCategory::Preset => t!("ui.search.context_preset").into_owned(),
        suggest::SuggestionCategory::Match => t!("ui.search.context_match").into_owned(),
        suggest::SuggestionCategory::Roster => t!("ui.search.context_player").into_owned(),
    }
}

/// The path of `path`'s parent. The root's parent is the root.
fn parent_of(path: &[usize]) -> NodePath {
    path.iter().copied().take(path.len().saturating_sub(1)).collect()
}

fn operator_segment_choices(expr: &MatchExpr, path: &[usize]) -> Vec<Choice> {
    let Some((offered, current)) = operator_choices(expr, path) else { return Vec::new() };

    offered
        .into_iter()
        .filter_map(|choice| {
            Some(Choice {
                label: choice.label,
                taken: with_operator(expr, path, choice.op)?,
                current: choice.op == current,
            })
        })
        .collect()
}

/// Every match-level field this term could be instead.
///
/// Roster terms are left alone: their field list is a different one and
/// changing it reshapes the quantifier around it, which the bar does not do
/// yet.
fn field_choices(expr: &MatchExpr, path: &[usize], operator_preferences: &OperatorPreferences) -> Vec<Choice> {
    let Some((current, _, _)) = select::term_at(expr, path) else { return Vec::new() };
    if !matches!(current, TermField::Match(_)) {
        return Vec::new();
    }

    // The operator a field was last given, so switching to it lands on the
    // same operator the user chose before rather than the field's default.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query_bar::label::NameCache;
    use crate::query_bar::tokens;

    fn first_pill(expr: &MatchExpr) -> NodePath {
        select::pill_paths(&tokens::tokenize(expr, &NameCache::default())).first().cloned().expect("one pill")
    }

    /// The fragment under the caret is what gets completed; the finished
    /// part of the query in front of it survives.
    ///
    /// The needle is taken from a real suggestion rather than written here,
    /// so the test does not quietly stop exercising anything when the
    /// vocabulary changes. What lands in the bar is the grammar the
    /// suggestion stands for, not the phrase it is read as.
    #[test]
    fn a_completion_replaces_only_the_fragment_under_the_caret() {
        let first = suggest::static_suggestions().first().expect("there are suggestions").label.clone();
        let needle: String = first.chars().take(3).collect();

        let offered = completions(&format!("map:ocean {needle}"));

        assert!(!offered.is_empty(), "a fragment of a real label offers it back: {needle:?}");
        for completion in &offered {
            assert!(
                completion.replacement.starts_with("map:ocean "),
                "the finished part of the query survives: {:?}",
                completion.replacement
            );
            assert!(
                !completion.replacement.contains(&format!("{needle}{needle}")),
                "the fragment is replaced, not appended to"
            );
        }
    }

    #[test]
    fn no_more_than_the_dropdown_can_show_are_offered() {
        assert!(completions("").len() <= MAX_SUGGESTIONS);
    }

    /// The operator picker offers what the term accepts, and taking one
    /// rewrites only that operator.
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

    /// The field segment offers every match field, and taking one keeps the
    /// term rather than starting a new query.
    #[test]
    fn a_field_can_be_changed_from_its_pill() {
        let expr = query_text::parse_query("map:ocean").expect("the fixture parses");
        let path = first_pill(&expr);

        let offered = choices(&expr, &path, EditablePart::Field, &OperatorPreferences::default());
        assert!(offered.len() > 1, "there are other fields to choose");
        assert_eq!(offered.iter().filter(|choice| choice.current).count(), 1, "exactly one is the current field");

        let other = offered.iter().find(|choice| !choice.current).expect("another field");
        let reparsed = query_text::parse_query(&other.taken).expect("what it prints, it reads back");
        let (field, _, _) = select::term_at(&reparsed, &path).expect("still one term");
        assert_ne!(
            format!("{field:?}"),
            format!("{:?}", select::term_at(&expr, &path).expect("a term").0),
            "the field changed"
        );
    }

    /// A field whose values enumerate offers them; one that is typed offers
    /// nothing, so the bar opens no picker for it.
    #[test]
    fn only_an_enumerable_value_offers_a_picker() {
        let prefs = OperatorPreferences::default();

        let enumerated = query_text::parse_query("outcome=win").expect("the fixture parses");
        let path = first_pill(&enumerated);
        let offered = choices(&enumerated, &path, EditablePart::Value, &prefs);
        assert!(offered.len() > 1, "every outcome is offered");
        assert!(offered.iter().any(|choice| choice.current), "one of them is the current value");
        let other = offered.iter().find(|choice| !choice.current).expect("another outcome");
        assert!(
            query_text::parse_query(&other.taken).is_ok(),
            "taking it leaves a query that parses: {:?}",
            other.taken
        );

        let typed = query_text::parse_query("build>9000000").expect("the fixture parses");
        let path = first_pill(&typed);
        assert!(choices(&typed, &path, EditablePart::Value, &prefs).is_empty(), "a free number offers no picker");
    }

    /// Every category renders, so a new one cannot reach the screen as an
    /// empty breadcrumb.
    #[test]
    fn every_suggestion_carries_a_breadcrumb() {
        for suggestion in suggest::static_suggestions() {
            assert!(!category_label(suggestion.context).is_empty());
        }
    }

    /// Negating twice is the query it started from, whichever way the shared
    /// transform expresses the first negation.
    #[test]
    fn negating_a_pill_twice_is_the_identity() {
        let expr = query_text::parse_query("outcome=win").expect("the fixture parses");
        let selection = Selection::default();

        let negated = apply_structural_edit(&expr, &first_pill(&expr), StructuralEdit::Negate, &selection)
            .expect("negate edits the query");
        assert_ne!(query_text::print_query(&negated), "outcome=win");

        let back = apply_structural_edit(&negated, &first_pill(&negated), StructuralEdit::Negate, &selection)
            .expect("negate edits the query");
        assert_eq!(query_text::print_query(&back), "outcome=win");
    }

    /// Delete acts on the pill the menu was opened from when nothing is
    /// selected, and on the selection when something is.
    #[test]
    fn delete_takes_the_selection_or_the_pill_it_was_asked_from() {
        let expr = query_text::parse_query("outcome=win map:ocean").expect("the fixture parses");
        let paths = select::pill_paths(&tokens::tokenize(&expr, &NameCache::default()));
        assert_eq!(paths.len(), 2);

        let one = apply_structural_edit(&expr, &paths[0], StructuralEdit::Delete, &Selection::default())
            .expect("delete edits the query");
        assert_eq!(query_text::print_query(&one), "map:ocean");

        let mut selected = Selection::default();
        selected.set_many(paths.clone());
        let none = apply_structural_edit(&expr, &paths[0], StructuralEdit::Delete, &selected)
            .expect("delete edits the query");
        assert_eq!(query_text::print_query(&none), "");
    }

    #[test]
    fn toggling_the_selection_does_not_edit_the_query() {
        let expr = query_text::parse_query("outcome=win").expect("the fixture parses");

        assert!(
            apply_structural_edit(&expr, &first_pill(&expr), StructuralEdit::ToggleSelected, &Selection::default())
                .is_none()
        );
    }
}
