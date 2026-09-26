//! The Search tab's query bar: the parsed query drawn as pills, and the
//! completions the caret fragment offers.
//!
//! Both come from `wows_toolkit_viewmodel::query_bar`, which is the egui app's
//! own pill and completion logic. That crate decides what a term reads as and
//! what may follow the caret; this module only draws the result, so the two
//! bars cannot disagree about how a query reads.
//!
//! The egui bar additionally edits in place -- clicking a pill segment opens
//! a picker for it. Here a pill is a reading of the query, not a handle on
//! it; the text is still edited in the input beside it.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use rust_i18n::t;
use wows_toolkit_config::index::query_ast::MatchExpr;
use wows_toolkit_config::index::query_ast::MatchField;
use wows_toolkit_config::index::query_ast::Op;
use wows_toolkit_config::index::query_ast::OperatorPreferences;
use wows_toolkit_config::index::query_text;
use wows_toolkit_viewmodel::query_bar::label;
use wows_toolkit_viewmodel::query_bar::label::NameCache;
use wows_toolkit_viewmodel::query_bar::label::SegmentRole;
use wows_toolkit_viewmodel::query_bar::select;
use wows_toolkit_viewmodel::query_bar::select::Selection;
use wows_toolkit_viewmodel::query_bar::suggest;
use wows_toolkit_viewmodel::query_bar::suggest::SuggestionKind;
use wows_toolkit_viewmodel::query_bar::suggest::TermField;
use wows_toolkit_viewmodel::query_bar::tokens;
use wows_toolkit_viewmodel::query_bar::tokens::NodePath;
use wows_toolkit_viewmodel::query_bar::tokens::TokenKind;

/// How many completions the dropdown offers at once. The egui bar shows the
/// same number before it scrolls.
pub const MAX_SUGGESTIONS: usize = 8;

/// Element ids are one range per pill, so a segment's id cannot collide with
/// the next pill's. No term renders more than a field, an operator and a
/// value.
const SEGMENTS_PER_PILL: usize = 8;

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
    fn of(role: SegmentRole) -> Self {
        match role {
            SegmentRole::Filter => Self::Field,
            SegmentRole::Operator => Self::Operator,
            SegmentRole::Value => Self::Value,
        }
    }
}

/// One choice a picker offers, and the query text taking it produces.
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
pub fn choices(expr: &MatchExpr, path: &[usize], part: EditablePart) -> Vec<Choice> {
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
        EditablePart::Field => field_choices(expr, path),
        EditablePart::Value => value_choices(expr, path),
    }
}

/// Every match-level field this term could be instead.
///
/// Roster terms are left alone: their field list is a different one and
/// changing it reshapes the quantifier around it, which the bar does not do
/// yet.
fn field_choices(expr: &MatchExpr, path: &[usize]) -> Vec<Choice> {
    let Some((current, _, _)) = select::term_at(expr, path) else { return Vec::new() };
    if !matches!(current, TermField::Match(_)) {
        return Vec::new();
    }

    // The operator a field was last given, so switching to it lands on the
    // same operator the user chose before rather than the field's default.
    let mut prefs = OperatorPreferences::default();
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
    let Some((field, op, current)) = select::term_at(expr, path) else { return Vec::new() };
    let Some(offered) = query_text::enumerable_values(field.value_kind()) else { return Vec::new() };
    let _ = op;

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

/// The parsed query as a row of pills.
///
/// `None` when the query is empty or does not parse: there is nothing to read
/// back, and the bar says so in its own way rather than drawing half a query.
///
/// `on_segment` is handed the path of a clicked pill and which part of it was
/// clicked, together with the app it was clicked in, which is what opens the
/// picker for it.
/// `caret` is the text box the reader types into, placed where the token
/// stream says the caret goes: after the pills, or in the slot of the pill
/// being retyped. The egui bar puts a real editor in that slot for the same
/// reason -- one bar, rather than a box with its own reading underneath it.
#[allow(clippy::too_many_arguments)]
pub fn pill_strip(
    expr: &MatchExpr,
    cache: &NameCache,
    selection: &Selection,
    cx: &App,
    on_choice: impl Fn(String, &mut Window, &mut App) + Clone + 'static,
    on_structure: impl Fn(NodePath, StructuralEdit, &mut Window, &mut App) + Clone + 'static,
    caret: AnyElement,
) -> Option<AnyElement> {
    let stream = tokens::tokenize(expr, cache);

    let theme = cx.theme();
    let (border, accent, muted) = (theme.border, theme.accent, theme.foreground);

    let mut caret = Some(caret);
    let mut row = h_flex().flex_wrap().gap_1().items_center();
    for (index, token) in stream.iter().enumerate() {
        row = match &token.kind {
            TokenKind::Pill { segments } => {
                // A selected pill reads as selected, which is what says what
                // a group or a delete is about to act on.
                let chosen = selection.contains(&token.path);
                let mut pill = h_flex()
                    .id(("search-pill", index))
                    .gap_0p5()
                    .items_center()
                    .px_1p5()
                    .py(px(1.))
                    .rounded_sm()
                    .border_1()
                    .border_color(if chosen { accent } else { border })
                    .when(chosen, |this| this.bg(accent.opacity(0.2)));
                for (slot, segment) in segments.iter().enumerate() {
                    // The field and the operator are chrome around the value,
                    // which is the part the reader is looking for.
                    let dimmed = !matches!(segment.role, SegmentRole::Value);
                    let part = EditablePart::of(segment.role);
                    let offered = choices(expr, &token.path, part);
                    let id = index * SEGMENTS_PER_PILL + slot;

                    // A divider between the cells, as the egui bar draws
                    // them: one run of words is hard to tell the parts of.
                    if slot > 0 {
                        pill = pill.child(div().flex_none().w(px(1.)).h(px(14.)).bg(border));
                    }

                    // A segment offers a pick only where there is something
                    // to pick; a free value is typed in the bar instead, as
                    // it is in the egui one.
                    if offered.is_empty() {
                        pill = pill.child(
                            div()
                                .id(("search-pill-segment", id))
                                .test_support()
                                .px_1()
                                .text_xs()
                                .when(dimmed, |this| this.text_color(crate::theme::text_dim()))
                                .when(!dimmed, |this| this.font_weight(FontWeight::MEDIUM))
                                .child(segment.text.clone()),
                        );
                        continue;
                    }

                    let take = on_choice.clone();
                    pill = pill.child(
                        Button::new(("search-pill-segment", id))
                            .label(segment.text.clone())
                            .ghost()
                            .compact()
                            .dropdown_menu(move |menu, _window, _cx| {
                                offered.iter().fold(menu, |menu, choice| {
                                    let take = take.clone();
                                    let taken = choice.taken.clone();
                                    menu.item(
                                        PopupMenuItem::new(choice.label.clone())
                                            .checked(choice.current)
                                            .on_click(move |_event, window, cx| take(taken.clone(), window, cx)),
                                    )
                                })
                            }),
                    );
                }
                row.child(pill_with_menu(pill, index, &token.path, expr, selection, on_structure.clone()))
            }
            TokenKind::Connector { is_or } => {
                row.child(div().text_xs().text_color(crate::theme::text_dim()).child(if *is_or { "or" } else { "and" }))
            }
            TokenKind::NotPrefix => row.child(div().text_xs().text_color(accent).child("not")),
            TokenKind::GroupOpen { .. } => row.child(div().text_xs().text_color(crate::theme::text_faint()).child("(")),
            TokenKind::GroupClose => row.child(div().text_xs().text_color(crate::theme::text_faint()).child(")")),
            TokenKind::QuantOpen { prefix } => row.child(
                h_flex()
                    .gap_0p5()
                    .child(div().text_xs().text_color(muted).child(prefix.clone()))
                    .child(div().text_xs().text_color(crate::theme::text_faint()).child("[")),
            ),
            TokenKind::QuantClose => row.child(div().text_xs().text_color(crate::theme::text_faint()).child("]")),
            TokenKind::Caret => row.child(caret.take().unwrap_or_else(|| div().into_any_element())),
        };
    }

    Some(row.into_any_element())
}

/// What a pill's own menu can do to the query's shape.
///
/// The edits themselves are `wows_toolkit_viewmodel::query_bar::select`,
/// shared with the egui bar; this only names which one was asked for.
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

/// Wraps a drawn pill in the menu that acts on the query's shape.
///
/// Everything it offers is refused rather than hidden when it does not
/// apply: a menu whose items move around is harder to learn than one whose
/// items grey out.
fn pill_with_menu(
    pill: impl IntoElement,
    index: usize,
    path: &NodePath,
    expr: &MatchExpr,
    selection: &Selection,
    on_structure: impl Fn(NodePath, StructuralEdit, &mut Window, &mut App) + Clone + 'static,
) -> impl IntoElement {
    let path = path.clone();
    let selected = selection.contains(&path);
    let can_group = select::can_group(expr, selection) && !selection.is_empty();
    let can_ungroup = path.len() > 1;
    let can_delete = !selection.is_empty();

    let item = {
        let path = path.clone();
        let on_structure = on_structure.clone();
        move |label: String, edit: StructuralEdit, enabled: bool| {
            let path = path.clone();
            let on_structure = on_structure.clone();
            PopupMenuItem::new(label).disabled(!enabled).on_click(move |_event, window, cx: &mut App| {
                on_structure(path.clone(), edit, window, cx);
            })
        }
    };

    div().id(("search-pill-menu", index)).child(pill).context_menu(move |menu, _window, _cx| {
        menu.item(item(
            t!(if selected { "ui.search.pill_deselect" } else { "ui.search.pill_select" }).into_owned(),
            StructuralEdit::ToggleSelected,
            true,
        ))
        .separator()
        .item(item(t!("ui.search.pill_group_all").into_owned(), StructuralEdit::Group { is_or: false }, can_group))
        .item(item(t!("ui.search.pill_group_any").into_owned(), StructuralEdit::Group { is_or: true }, can_group))
        .item(item(t!("ui.search.pill_ungroup").into_owned(), StructuralEdit::Ungroup, can_ungroup))
        .separator()
        .item(item(t!("ui.search.pill_negate").into_owned(), StructuralEdit::Negate, true))
        .item(item(t!("ui.search.pill_flip_connector").into_owned(), StructuralEdit::FlipConnector, true))
        .separator()
        .item(item(t!("ui.search.pill_delete").into_owned(), StructuralEdit::Delete, can_delete))
    })
}

/// One completion the bar is offering.
#[derive(Clone)]
pub struct Completion {
    /// What the row shows.
    pub label: String,
    /// The breadcrumb after it, already translated.
    pub context: String,
    /// The query text this completion produces when taken.
    pub replacement: String,
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

/// The breadcrumb text for a suggestion's category.
///
/// The category is typed rather than a string precisely so this match is
/// exhaustive: a new one cannot reach the screen untranslated.
fn category_label(category: suggest::SuggestionCategory) -> String {
    match category {
        suggest::SuggestionCategory::Preset => "preset".to_string(),
        suggest::SuggestionCategory::Match => "match".to_string(),
        suggest::SuggestionCategory::Roster => "player".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::completions;
    use wows_toolkit_viewmodel::query_bar::suggest;

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
        assert!(completions("").len() <= super::MAX_SUGGESTIONS);
    }

    /// The operator picker offers what the term accepts, and taking one
    /// rewrites only that operator.
    #[test]
    fn an_operator_can_be_changed_from_its_pill() {
        use wows_toolkit_config::index::query_text;
        use wows_toolkit_viewmodel::query_bar::label::NameCache;
        use wows_toolkit_viewmodel::query_bar::select;
        use wows_toolkit_viewmodel::query_bar::tokens;

        let expr = query_text::parse_query("build>9000000").expect("the fixture parses");
        let cache = NameCache::default();
        let path = select::pill_paths(&tokens::tokenize(&expr, &cache)).first().cloned().expect("one pill");

        let (choices, current) = super::operator_choices(&expr, &path).expect("a single term has an operator");
        assert!(choices.len() > 1, "there is something to change it to");
        assert!(choices.iter().all(|choice| !choice.label.is_empty()), "every choice reads as something");

        let other = choices.iter().find(|choice| choice.op != current).expect("another operator");
        let rewritten = super::with_operator(&expr, &path, other.op).expect("the edit applies");

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
        use wows_toolkit_config::index::query_text;
        use wows_toolkit_viewmodel::query_bar::label::NameCache;
        use wows_toolkit_viewmodel::query_bar::select;
        use wows_toolkit_viewmodel::query_bar::tokens;

        let expr = query_text::parse_query("map:ocean").expect("the fixture parses");
        let path =
            select::pill_paths(&tokens::tokenize(&expr, &NameCache::default())).first().cloned().expect("one pill");

        let offered = super::choices(&expr, &path, super::EditablePart::Field);
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
        use wows_toolkit_config::index::query_text;
        use wows_toolkit_viewmodel::query_bar::label::NameCache;
        use wows_toolkit_viewmodel::query_bar::select;
        use wows_toolkit_viewmodel::query_bar::tokens;

        let path_of = |expr: &_| {
            select::pill_paths(&tokens::tokenize(expr, &NameCache::default())).first().cloned().expect("one pill")
        };

        // `outcome` is an enumeration.
        let enumerated = query_text::parse_query("outcome=win").expect("the fixture parses");
        let path = path_of(&enumerated);
        let offered = super::choices(&enumerated, &path, super::EditablePart::Value);
        assert!(offered.len() > 1, "every outcome is offered");
        assert!(offered.iter().any(|choice| choice.current), "one of them is the current value");
        let other = offered.iter().find(|choice| !choice.current).expect("another outcome");
        assert!(
            query_text::parse_query(&other.taken).is_ok(),
            "taking it leaves a query that parses: {:?}",
            other.taken
        );

        // A build number is typed, not chosen.
        let typed = query_text::parse_query("build>9000000").expect("the fixture parses");
        let path = path_of(&typed);
        assert!(super::choices(&typed, &path, super::EditablePart::Value).is_empty(), "a free number offers no picker");
    }

    /// Every category renders, so a new one cannot reach the screen as an
    /// empty breadcrumb.
    #[test]
    fn every_suggestion_carries_a_breadcrumb() {
        for suggestion in suggest::static_suggestions() {
            assert!(!super::category_label(suggestion.context).is_empty());
        }
    }
}
