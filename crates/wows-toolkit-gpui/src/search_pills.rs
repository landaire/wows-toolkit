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
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use wows_toolkit_config::index::query_ast::MatchExpr;
use wows_toolkit_viewmodel::query_bar::label::NameCache;
use wows_toolkit_viewmodel::query_bar::label::SegmentRole;
use wows_toolkit_viewmodel::query_bar::suggest;
use wows_toolkit_viewmodel::query_bar::tokens;
use wows_toolkit_viewmodel::query_bar::tokens::TokenKind;

/// How many completions the dropdown offers at once. The egui bar shows the
/// same number before it scrolls.
pub const MAX_SUGGESTIONS: usize = 8;

/// The parsed query as a row of pills.
///
/// `None` when the query is empty or does not parse: there is nothing to read
/// back, and the bar says so in its own way rather than drawing half a query.
pub fn pill_strip(expr: &MatchExpr, cache: &NameCache, cx: &App) -> Option<AnyElement> {
    let stream = tokens::tokenize(expr, cache);
    if stream.is_empty() {
        return None;
    }

    let theme = cx.theme();
    let (border, accent, muted) = (theme.border, theme.accent, theme.foreground);

    let mut row = h_flex().flex_wrap().gap_1().items_center();
    for (index, token) in stream.iter().enumerate() {
        row = match &token.kind {
            TokenKind::Pill { segments } => {
                let mut pill = h_flex()
                    .id(("search-pill", index))
                    .gap_0p5()
                    .items_center()
                    .px_1p5()
                    .py(px(1.))
                    .rounded_sm()
                    .border_1()
                    .border_color(border);
                for segment in segments {
                    // The field and the operator are chrome around the value,
                    // which is the part the reader is looking for.
                    let dimmed = !matches!(segment.role, SegmentRole::Value);
                    pill = pill.child(
                        div()
                            .text_xs()
                            .when(dimmed, |this| this.opacity(0.7))
                            .when(!dimmed, |this| this.font_weight(FontWeight::MEDIUM))
                            .child(segment.text.clone()),
                    );
                }
                row.child(pill)
            }
            TokenKind::Connector { is_or } => {
                row.child(div().text_xs().opacity(0.6).child(if *is_or { "or" } else { "and" }))
            }
            TokenKind::NotPrefix => row.child(div().text_xs().text_color(accent).child("not")),
            TokenKind::GroupOpen { .. } => row.child(div().text_xs().opacity(0.5).child("(")),
            TokenKind::GroupClose => row.child(div().text_xs().opacity(0.5).child(")")),
            TokenKind::QuantOpen { prefix } => row.child(
                h_flex()
                    .gap_0p5()
                    .child(div().text_xs().text_color(muted).child(prefix.clone()))
                    .child(div().text_xs().opacity(0.5).child("[")),
            ),
            TokenKind::QuantClose => row.child(div().text_xs().opacity(0.5).child("]")),
            // The caret is the text input itself here, not a token to draw.
            TokenKind::Caret => row,
        };
    }

    Some(row.into_any_element())
}

/// One completion the bar is offering.
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
    let prefix = &query[..suggest::active_fragment_start(query)];
    let all = suggest::static_suggestions();

    suggest::rank(fragment, &all)
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|index| {
            let suggestion = &all[index];
            Completion {
                label: suggestion.label.clone(),
                context: category_label(suggestion.context),
                replacement: format!("{prefix}{}", suggestion.label),
            }
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
    /// vocabulary changes.
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

    /// Every category renders, so a new one cannot reach the screen as an
    /// empty breadcrumb.
    #[test]
    fn every_suggestion_carries_a_breadcrumb() {
        for suggestion in suggest::static_suggestions() {
            assert!(!super::category_label(suggestion.context).is_empty());
        }
    }
}
