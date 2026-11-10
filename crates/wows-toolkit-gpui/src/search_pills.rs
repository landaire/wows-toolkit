//! The Search tab's query bar: the parsed query drawn as pills.
//!
//! What each pill reads as, what its segments offer and what its menu does
//! to the query come from `wows_toolkit_viewmodel::query_bar`, shared with
//! the egui and NSH bars; this module only draws them, so the bars cannot
//! disagree about how a query reads.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use rust_i18n::t;
use wows_toolkit_config::index::query_ast::MatchExpr;
use wows_toolkit_config::index::query_ast::OperatorPreferences;
use wows_toolkit_viewmodel::query_bar::label::NameCache;
use wows_toolkit_viewmodel::query_bar::label::SegmentRole;
use wows_toolkit_viewmodel::query_bar::pills;
use wows_toolkit_viewmodel::query_bar::pills::EditablePart;
use wows_toolkit_viewmodel::query_bar::pills::StructuralEdit;
use wows_toolkit_viewmodel::query_bar::select;
use wows_toolkit_viewmodel::query_bar::select::Selection;
use wows_toolkit_viewmodel::query_bar::tokens;
use wows_toolkit_viewmodel::query_bar::tokens::NodePath;
use wows_toolkit_viewmodel::query_bar::tokens::TokenKind;

/// Element ids are one range per pill, so a segment's id cannot collide with
/// the next pill's. No term renders more than a field, an operator and a
/// value.
const SEGMENTS_PER_PILL: usize = 8;

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
    operator_preferences: &OperatorPreferences,
    cx: &App,
    on_choice: impl Fn(String, &mut Window, &mut App) + Clone + 'static,
    on_structure: impl Fn(NodePath, StructuralEdit, &mut Window, &mut App) + Clone + 'static,
    editing: Option<(&NodePath, &Entity<InputState>)>,
    on_edit: impl Fn(NodePath, &mut Window, &mut App) + Clone + 'static,
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
                    .rounded(px(10.))
                    .bg(theme.muted)
                    .border_1()
                    .border_color(if chosen { accent } else { border })
                    .when(chosen, |this| this.bg(accent.opacity(0.2)));
                for (slot, segment) in segments.iter().enumerate() {
                    // The field and the operator are chrome around the value,
                    // which is the part the reader is looking for.
                    let part = EditablePart::of(segment.role);
                    let offered = pills::choices(expr, &token.path, part, operator_preferences);
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
                        if matches!(segment.role, SegmentRole::Value)
                            && select::segment_path(expr, &token.path).is_some()
                        {
                            if let Some((_, input)) = editing.filter(|(path, _)| *path == &token.path) {
                                pill = pill.child(div().w(px(140.)).child(Input::new(input).small().appearance(false)));
                            } else {
                                let edit = on_edit.clone();
                                let path = token.path.clone();
                                pill = pill.child(
                                    Button::new(("search-pill-segment", id))
                                        .label(segment.text.clone())
                                        .ghost()
                                        .compact()
                                        .tooltip("Edit value")
                                        .on_click(move |_event, window, cx| edit(path.clone(), window, cx)),
                                );
                            }
                        } else {
                            pill = pill.child(div().px_1().text_xs().child(segment.text.clone()));
                        }
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
    let state = pills::pill_menu(expr, path, selection);

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
            t!(if state.selected { "ui.search.pill_deselect" } else { "ui.search.pill_select" }).into_owned(),
            StructuralEdit::ToggleSelected,
            true,
        ))
        .separator()
        .item(item(
            t!("ui.search.pill_group_all").into_owned(),
            StructuralEdit::Group { is_or: false },
            state.can_group,
        ))
        .item(item(t!("ui.search.pill_group_any").into_owned(), StructuralEdit::Group { is_or: true }, state.can_group))
        .item(item(t!("ui.search.pill_ungroup").into_owned(), StructuralEdit::Ungroup, state.can_ungroup))
        .separator()
        .item(item(t!("ui.search.pill_negate").into_owned(), StructuralEdit::Negate, true))
        .item(item(t!("ui.search.pill_flip_connector").into_owned(), StructuralEdit::FlipConnector, true))
        .separator()
        .item(item(t!("ui.search.pill_delete").into_owned(), StructuralEdit::Delete, state.can_delete))
    })
}

