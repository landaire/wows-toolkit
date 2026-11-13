//! The query bar: the committed query drawn as pills, the box the next term is typed in, and the
//! completions or calendar hanging under it.

use landaire_ui::EscapeClears;
use landaire_ui::LoadingLineProps;
use landaire_ui::MarkedTextProps;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_config::index::query_ast::MatchExpr;
use wows_toolkit_config::index::query_ast::OperatorPreferences;
use wows_toolkit_viewmodel::query_bar::label::NameCache;
use wows_toolkit_viewmodel::query_bar::label::SegmentRole;
use wows_toolkit_viewmodel::query_bar::picker;
use wows_toolkit_viewmodel::query_bar::picker::Choice;
use wows_toolkit_viewmodel::query_bar::picker::EditablePart;
use wows_toolkit_viewmodel::query_bar::picker::PillMenuState;
use wows_toolkit_viewmodel::query_bar::picker::StructuralEdit;
use wows_toolkit_viewmodel::query_bar::select;
use wows_toolkit_viewmodel::query_bar::select::Selection;
use wows_toolkit_viewmodel::query_bar::suggest;
use wows_toolkit_viewmodel::query_bar::tokens;
use wows_toolkit_viewmodel::query_bar::tokens::NodePath;
use wows_toolkit_viewmodel::query_bar::tokens::TokenKind;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui_ui::AnchoredSurfaceProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CalendarProps;
use zgui_ui::ContextMenuContentProps;
use zgui_ui::ContextMenuProps;
use zgui_ui::ContextMenuTriggerProps;
use zgui_ui::Date;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::InputProps;
use zgui_ui::MenuCheckboxItemProps;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuSeparatorProps;
use zgui_ui::OverlayState;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::ui::SEARCH;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::tabs::search::model::Offer;
use crate::tabs::search::model::Search;

const SHEET: &str = "wt-search-bar";

const CSS: &str = css!(
    ".wt-search-bar {
        display: flex;
        flex-direction: column;
        flex: none;
        gap: 4px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--lui-color-border);
        background-color: var(--lui-color-panel);
    }
    .wt-search-entry {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
    }
    .wt-search-pills {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        flex: 1 1 auto;
        align-items: center;
        gap: 4px;
        min-width: 0;
        padding: 2px 8px;
        border: 1px solid var(--lui-color-border);
        border-radius: 6px;
    }
    .wt-search-caret { flex: 1 1 120px; min-width: 120px; }
    .wt-search-caret .zui-input {
        border-width: 0;
        background-color: transparent;
        box-shadow: none;
        height: 24px;
        padding: 0 2px;
    }
    .wt-search-pill {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 2px;
        padding: 1px 6px;
        border-radius: 10px;
        background-color: var(--lui-color-widget);
        border: 1px solid var(--lui-color-border);
    }
    .wt-search-pill[data-chosen] {
        border-color: var(--lui-color-accent);
        background-color: color-mix(in srgb, var(--lui-color-accent) 20%, transparent);
    }
    .wt-search-pill .zui-button { height: 20px; padding: 0 4px; }
    .wt-search-pill__divider {
        flex: none;
        width: 1px;
        height: 14px;
        background-color: var(--lui-color-border);
    }
    .wt-search-pill__text { padding: 0 4px; font-size: var(--zui-type-size-xs); }
    .wt-search-pill__value .zui-input { width: 140px; height: 20px; }
    .wt-search-token { font-size: var(--zui-type-size-xs); color: var(--lui-color-text-dim); }
    .wt-search-token--not { color: var(--lui-color-accent); }
    .wt-search-token--bracket { color: var(--lui-color-text-faint); }
    .wt-search-value-error { font-size: var(--zui-type-size-xs); color: var(--lui-color-accent); }
    .wt-search-completions {
        display: flex;
        flex-direction: column;
        gap: 1px;
        padding: 4px;
        min-width: 260px;
        max-width: 460px;
        max-height: 260px;
        overflow-y: auto;
    }
    .wt-search-completion {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
        padding: 4px 8px;
        border-radius: 4px;
        cursor: pointer;
    }
    .wt-search-completion:hover, .wt-search-completion[data-highlighted] {
        background-color: var(--lui-color-selection);
    }
    .wt-search-completion__label { font-size: var(--zui-type-size-sm); }
    .wt-search-completion__context { font-size: var(--zui-type-size-xs); color: var(--lui-color-text-dim); }
    .wt-search-looking-up { padding: 4px 8px; }
    .wt-search-calendar { padding: 4px; }
    .wt-search-parse-error {
        display: flex;
        flex-direction: column;
        gap: 1px;
        padding: 0 20px;
        font-size: var(--zui-type-size-xs);
    }
    .wt-search-parse-error__message { color: var(--wt-color-warn); }
    .wt-search-parse-error__text { display: flex; flex-direction: row; font-family: monospace; }
    .wt-search-parse-error__around { color: var(--lui-color-text-dim); white-space: pre; }
    .wt-search-parse-error__marked {
        color: var(--wt-color-warn);
        text-decoration: underline;
        white-space: pre;
    }"
);

/// The bar, above the results.
#[component]
pub(crate) fn QueryBar(search: Search) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let pills = NodeRef::new();
    let input = NodeRef::new();
    let focused = StoredValue::new(false);

    let typing = RenderEffect::new(move |_| {
        let text = search.pending.get();
        search.typed(&text, focused.get_value());
    });
    on_cleanup_local(move || drop(typing));

    // With the completions open, Escape closes them; with them closed, it empties the text.
    let escape =
        EscapeClears::with(Signal::derive_local(move || search.pending.with(|text| !text.is_empty())), move || {
            search.clear_typed()
        });
    let on_key = move |ev: &mut EventCx<'_, events::KeyDown>| {
        if handle_key(search, input, &ev.key, ev.modifiers) {
            ev.prevent_default();
            ev.stop_propagation();
            return;
        }
        escape.key_down(ev);
    };
    let strip = move || {
        let expr = search.reading.get()?;
        let selection = search.selection.get();
        let preferences = search.operator_preferences.get();
        let editing = search.value_edit.with(|edit| edit.as_ref().map(|edit| edit.path.clone()));
        Some(search.names.with(|names| pill_strip(search, &expr, names, &selection, &preferences, editing.as_ref())))
    };
    let parse_error = move || match search.status.get() {
        crate::tabs::search::model::Status::Invalid { reason, span, text } => {
            Some(parse_error_strip(&reason, span, &text))
        }
        _ => None,
    };

    view! {
        box(class = "wt-search-bar", attr:data-testid = "search-bar") {
            row(class = "wt-search-entry") {
                Icon(icon = SEARCH)
                row(class = "wt-search-pills", node_ref = pills, attr:data-testid = "search-pills") {
                    {strip}
                    box(
                        class = "wt-search-caret",
                        on:key_down:capture = on_key,
                        on:focus_in = move |_| focused.set_value(true),
                        on:focus_out = move |_| {
                            focused.set_value(false);
                            // The calendar is worked with the pointer, which takes the caret.
                            if !search.pending.with_untracked(|text| suggest::date_value_at_caret(text).is_some()) {
                                search.close_completions();
                            }
                        }
                    ) {
                        Input(
                            node_ref = input,
                            value = search.pending,
                            placeholder = "Add filter".to_owned(),
                            label = t!("ui.tabs.search").into_owned(),
                            attr:data-testid = "search-query"
                        )
                    }
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    on:click = move |_| search.commit_typed(),
                    attr:data-testid = "search-run"
                ) {
                    {t!("ui.tabs.search").into_owned()}
                }
            }
            Show(when = move || search.value_error.get()) {
                text(class = "wt-search-value-error") {
                    "Invalid filter value. Correct the value or press Escape to cancel."
                }
            }
            {parse_error}
            Dropdown(search = search, anchor = pills)
        }
    }
}

/// Keyboard on the bar: the dropdown's rows are walked with the arrows and taken with Enter; with
/// no dropdown the arrows walk the history. At the start of the text, Left steps onto the pills.
/// `true` when the key was the bar's rather than the text box's.
fn handle_key(search: Search, input: NodeRef, key: &Key, modifiers: zgui::vocab::Modifiers) -> bool {
    if search.value_edit.with_untracked(Option::is_some) {
        return false;
    }
    if modifiers.primary()
        && let Key::Character(text) = key
    {
        match text.to_ascii_lowercase().as_str() {
            "z" if !modifiers.shift() => {
                search.undo_edit();
                return true;
            }
            "y" | "z" => {
                search.redo_edit();
                return true;
            }
            _ => {}
        }
    }
    let Key::Named(named) = key else { return false };
    let open = search.completions_open.get_untracked();
    let caret = input.selection();
    let text_len = search.pending.with_untracked(String::len);
    let at_start = caret.as_ref().is_some_and(|caret| caret.start == 0 && caret.end == 0);
    let at_end = caret.as_ref().is_some_and(|caret| caret.start == text_len && caret.end == text_len);
    match named {
        NamedKey::ArrowDown if open => search.move_cursor(1, search.offered_untracked().len()),
        NamedKey::ArrowUp if open => search.move_cursor(-1, search.offered_untracked().len()),
        NamedKey::ArrowUp => search.walk_history(1),
        NamedKey::ArrowDown => search.walk_history(-1),
        NamedKey::Escape if open => search.close_completions(),
        NamedKey::ArrowLeft if at_start => search.step_pill_selection(true, modifiers.shift()),
        NamedKey::ArrowRight if at_end && search.has_pill_selection() => {
            search.step_pill_selection(false, modifiers.shift());
        }
        NamedKey::Delete if search.has_pill_selection() => search.delete_at_caret(),
        NamedKey::Backspace if text_len == 0 => search.delete_at_caret(),
        NamedKey::Enter => match search.cursor.get_untracked() {
            Some(at) => {
                let Some(taken) = search.offered_untracked().get(at).map(|row| row.replacement.clone()) else {
                    return false;
                };
                search.take_completion(taken, Offer::WhatFollows);
            }
            None => search.press_enter(),
        },
        _ => return false,
    }
    true
}

/// The parsed query as a row of pills, with the connectors, negations and brackets between them.
fn pill_strip(
    search: Search,
    expr: &MatchExpr,
    names: &NameCache,
    selection: &Selection,
    preferences: &OperatorPreferences,
    editing: Option<&NodePath>,
) -> Vec<AnyView> {
    tokens::tokenize(expr, names)
        .into_iter()
        .enumerate()
        .map(|(index, token)| match token.kind {
            TokenKind::Pill { segments } => {
                let mut cells = Vec::new();
                for (slot, segment) in segments.into_iter().enumerate() {
                    if slot > 0 {
                        cells.push(AnyView::new(view! { box(class = "wt-search-pill__divider") {} }));
                    }
                    let part = EditablePart::of(segment.role);
                    let offered = picker::choices_with_operator_preferences(expr, &token.path, part, preferences);
                    cells.push(segment_view(search, expr, &token.path, segment.role, segment.text, offered, editing));
                }
                let chosen = selection.contains(&token.path);
                let menu = PillMenuState::new(expr, &token.path, selection);
                pill_with_menu(search, index, token.path, chosen, menu, cells)
            }
            TokenKind::Connector { is_or } => {
                AnyView::new(view! { text(class = "wt-search-token") {{if is_or { "or" } else { "and" }}} })
            }
            TokenKind::NotPrefix => {
                AnyView::new(view! { text(class = "wt-search-token wt-search-token--not") {"not"} })
            }
            TokenKind::GroupOpen { .. } => {
                AnyView::new(view! { text(class = "wt-search-token wt-search-token--bracket") {"("} })
            }
            TokenKind::GroupClose => {
                AnyView::new(view! { text(class = "wt-search-token wt-search-token--bracket") {")"} })
            }
            TokenKind::QuantOpen { prefix } => AnyView::new(view! {
                row {
                    text(class = "wt-search-token") {{prefix}}
                    text(class = "wt-search-token wt-search-token--bracket") {"["}
                }
            }),
            TokenKind::QuantClose => {
                AnyView::new(view! { text(class = "wt-search-token wt-search-token--bracket") {"]"} })
            }
            // The box the next term is typed in sits after the pills, where it keeps its focus
            // while the pills are built again.
            TokenKind::Caret => AnyView::new(()),
        })
        .collect()
}

/// One segment of a pill: a picker where there is something to pick, a button that retypes a
/// free value, or plain text.
fn segment_view(
    search: Search,
    expr: &MatchExpr,
    path: &NodePath,
    role: SegmentRole,
    label: String,
    offered: Vec<Choice>,
    editing: Option<&NodePath>,
) -> AnyView {
    if !offered.is_empty() {
        return AnyView::new(view! {
            DropdownMenu {
                DropdownMenuTrigger(variant = ButtonVariant::Ghost, size = ButtonSize::Xs, attr:data-testid = "search-pill-segment") {
                    {label}
                }
                DropdownMenuContent {
                    {offered
                        .clone()
                        .into_iter()
                        .map(|choice| {
                            let taken = choice.taken.clone();
                            AnyView::new(view! {
                                MenuCheckboxItem(
                                    default_checked = choice.current,
                                    on_change = UnsyncCallback::new(move |_| search.take_choice(taken.clone()))
                                ) {
                                    {choice.label}
                                }
                            })
                        })
                        .collect::<Vec<_>>()}
                }
            }
        });
    }

    if matches!(role, SegmentRole::Value) && select::segment_path(expr, path).is_some() {
        if editing == Some(path) {
            return AnyView::new(view! { ValueEditor(search = search) });
        }
        let path = path.clone();
        return AnyView::new(view! {
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::Xs,
                        on:click = move |_| search.begin_value_edit(path.clone()),
                        attr:data-testid = "search-pill-value"
                    ) {
                        {label}
                    }
                }
                TooltipContent {{t!("ui.search.edit_value").into_owned()}}
            }
        });
    }

    AnyView::new(view! { text(class = "wt-search-pill__text") {{label}} })
}

/// A drawn pill inside the menu that acts on the query's shape. Every item stays listed, greyed
/// out where it does not apply.
fn pill_with_menu(
    search: Search,
    index: usize,
    path: NodePath,
    chosen: bool,
    menu: PillMenuState,
    cells: Vec<AnyView>,
) -> AnyView {
    AnyView::new(view! {
        ContextMenu {
            ContextMenuTrigger {
                row(
                    class = "wt-search-pill",
                    attr:data-chosen = chosen.then(String::new),
                    attr:data-testid = format!("search-pill-{index}")
                ) {
                    {cells}
                }
            }
            ContextMenuContent {
                {pill_menu_items(search, &path, menu)}
            }
        }
    })
}

fn pill_menu_items(search: Search, path: &NodePath, menu: PillMenuState) -> Vec<AnyView> {
    let item = |label: String, edit: StructuralEdit, enabled: bool| {
        let path = path.clone();
        AnyView::new(view! {
            MenuItem(
                disabled = !enabled,
                on_select = UnsyncCallback::new(move |()| search.apply_structural_edit(path.clone(), edit))
            ) {
                {label}
            }
        })
    };
    let separator = || AnyView::new(view! { MenuSeparator() });
    let select_label = t!(if menu.selected { "ui.search.pill_deselect" } else { "ui.search.pill_select" }).into_owned();
    vec![
        item(select_label, StructuralEdit::ToggleSelected, true),
        separator(),
        item(t!("ui.search.pill_group_all").into_owned(), StructuralEdit::Group { is_or: false }, menu.can_group),
        item(t!("ui.search.pill_group_any").into_owned(), StructuralEdit::Group { is_or: true }, menu.can_group),
        item(t!("ui.search.pill_ungroup").into_owned(), StructuralEdit::Ungroup, menu.can_ungroup),
        separator(),
        item(t!("ui.search.pill_negate").into_owned(), StructuralEdit::Negate, true),
        item(t!("ui.search.pill_flip_connector").into_owned(), StructuralEdit::FlipConnector, true),
        separator(),
        item(t!("ui.search.pill_delete").into_owned(), StructuralEdit::Delete, menu.can_delete),
    ]
}

/// The box a free value is retyped in, inside its pill. Enter or leaving the box writes it;
/// Escape cancels.
#[component]
fn ValueEditor(search: Search) -> impl IntoView {
    let input = NodeRef::new();
    let focusing = RenderEffect::new(move |focused: Option<bool>| {
        if focused == Some(true) {
            return true;
        }
        let bound = input.get().is_some();
        if bound {
            input.focus();
        }
        bound
    });
    on_cleanup_local(move || drop(focusing));

    view! {
        box(
            class = "wt-search-pill__value",
            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| match ev.key {
                Key::Named(NamedKey::Enter) => {
                    search.commit_value_edit();
                    ev.stop_propagation();
                }
                Key::Named(NamedKey::Escape) => {
                    search.cancel_value_edit();
                    ev.stop_propagation();
                }
                _ => {}
            },
            on:focus_out = move |_| search.commit_value_edit()
        ) {
            Input(
                node_ref = input,
                value = search.value_text,
                on_change = UnsyncCallback::new(move |_| search.value_error.set(false)),
                invalid = Signal::derive_local(move || search.value_error.get()),
                attr:data-testid = "search-value-input"
            )
        }
    }
}

/// The completions, or for a date field a calendar, hung under the bar while the reader types.
#[component]
fn Dropdown(search: Search, anchor: NodeRef) -> impl IntoView {
    let calendar = Memo::new(move |_| search.pending.with(|text| suggest::date_value_at_caret(text).is_some()));
    let shown = Memo::new(move |_| {
        search.completions_open.get()
            && (calendar.get() || search.value_lookup_running.get() || !search.offered().is_empty())
    });
    let placement: Signal<Placement, LocalStorage> = Signal::stored_local(Placement::new(Side::Bottom, Align::Start));

    move || {
        if !shown.get() {
            return None;
        }
        let state = OverlayState::uncontrolled(
            true,
            Some(UnsyncCallback::new(move |open: bool| {
                if !open {
                    search.close_completions();
                }
            })),
        )
        .anchored_to(anchor);
        let body = move || {
            if calendar.get() {
                AnyView::new(view! {
                    box(class = "wt-search-calendar", attr:data-testid = "search-calendar") {
                        Calendar(on_change = UnsyncCallback::new(move |day: Option<Date>| {
                            if let Some(day) = day {
                                search.take_date(&day.to_iso());
                            }
                        }))
                    }
                })
            } else {
                AnyView::new(view! { CompletionRows(search = search) })
            }
        };
        Some(AnyView::new(view! {
            AnchoredSurface(state = state, placement = placement, class = "wt-search-dropdown") {
                {body}
            }
        }))
    }
}

/// The completion rows, best first, with a line while the index is still being asked.
#[component]
fn CompletionRows(search: Search) -> impl IntoView {
    let rows = move || {
        let cursor = search.cursor.get();
        search
            .offered()
            .into_iter()
            .enumerate()
            .map(|(index, completion)| {
                let replacement = completion.replacement.clone();
                AnyView::new(view! {
                    row(
                        class = "wt-search-completion",
                        attr:data-highlighted = (cursor == Some(index)).then(String::new),
                        attr:data-testid = format!("search-completion-{index}"),
                        a11y:label = completion.label.clone(),
                        on:click = move |_| search.take_completion(replacement.clone(), Offer::WhatFollows)
                    ) {
                        MarkedText(
                            class = "wt-search-completion__label",
                            text = completion.label,
                            ranges = completion.marks.into()
                        )
                        text(class = "wt-search-completion__context") {{completion.context}}
                    }
                })
            })
            .collect::<Vec<_>>()
    };

    view! {
        box(
            class = "wt-search-completions",
            // A press on a row keeps the caret in the box, so its leaving does not close the
            // dropdown before the row is taken.
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.prevent_default(),
            attr:data-testid = "search-completions"
        ) {
            Show(when = move || search.value_lookup_running.get()) {
                LoadingLine(label = t!("ui.search.looking_up_values").into_owned(), class = "wt-search-looking-up")
            }
            {rows}
        }
    }
}

/// The parse error: the message, and the query with the run the parser objected to marked. An
/// empty or out-of-range span marks nothing.
fn parse_error_strip(reason: &str, span: std::ops::Range<usize>, text: &str) -> AnyView {
    let message = t!("ui.search.parse_failed", reason = reason).into_owned();
    let marked = text.get(span.clone()).filter(|marked| !marked.is_empty()).map(|marked| {
        let before = text[..span.start].to_string();
        let after = text[span.end..].to_string();
        let marked = marked.to_string();
        view! {
            row(class = "wt-search-parse-error__text") {
                text(class = "wt-search-parse-error__around") {{before}}
                text(class = "wt-search-parse-error__marked") {{marked}}
                text(class = "wt-search-parse-error__around") {{after}}
            }
        }
    });

    AnyView::new(view! {
        box(class = "wt-search-parse-error", a11y:label = message.clone(), attr:data-testid = "search-parse-error") {
            text(class = "wt-search-parse-error__message") {{message}}
            {marked}
        }
    })
}
