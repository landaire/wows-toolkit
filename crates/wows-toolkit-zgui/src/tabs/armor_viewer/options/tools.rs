//! The options rail's tool buttons and the viewport context menu's toggles.

use std::rc::Rc;
use std::sync::Arc;

use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::MenuCheckboxItemProps;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuSeparatorProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui::prelude::Binding;

use crate::icons::AppIconProps;
use crate::shell::Shell;
use crate::tabs::armor_viewer::options::OptionSection;
use crate::tabs::armor_viewer::options::SectionCx;
use crate::tabs::armor_viewer::options::filter::MarkedLabel;
use crate::tabs::armor_viewer::options::filter::RailFilter;
use crate::tabs::armor_viewer::panels::export::ExportRequest;
use crate::tabs::armor_viewer::panels::export::open_export_dialog;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::tabs::armor_viewer::viewport::SceneFeatures;
use crate::tabs::armor_viewer::workspace::ArmorWorkspace;

const SHEET: &str = "wt-armor-tools";

const CSS: &str = css!(
    ".wt-armor-tools {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 4px;
    }
    .wt-armor-tool { display: flex; min-width: 0; border-radius: 2px; }
    .wt-armor-tool__button { width: 100%; min-width: 0; justify-content: flex-start; }
    .wt-armor-tool__button[data-selected] {
        background-color: var(--lui-color-selection);
        border-color: var(--lui-color-accent);
        color: var(--lui-color-text-bright);
    }
    .wt-armor-tool__label {
        min-width: 0;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
    }
    .wt-armor-choice[data-selected] {
        background-color: var(--lui-color-selection);
        border-color: var(--lui-color-accent);
        color: var(--lui-color-text-bright);
    }"
);

/// The options rail's tool rows for the active viewport: the view toggles, then the splash tools
/// and the export.
pub fn tools(cx: &SectionCx) -> Option<AnyView> {
    install_stylesheet(SHEET, CSS);
    let viewport = cx.viewport;
    let filter = &cx.filter;

    // An open model keeps only the tools it has anything for; with nothing open every tool is
    // drawn, disabled.
    let features = cx.features;
    let armor = features.is_none_or(|features| features.armor);
    let ship = features.is_none_or(|features| features.ship);
    let has_splash_data = viewport.shown_untracked().is_some_and(|armor| armor.splash_data.is_some());

    let tools: Vec<AnyView> = [
        armor.then(|| hidden_plates_button(viewport, filter)).flatten(),
        armor.then(|| gaps_button(viewport, cx)).flatten(),
        ship.then(|| splash_button(viewport, cx, has_splash_data)).flatten(),
        ship.then(|| splash_boxes_button(viewport, filter)).flatten(),
        export_button(cx.workspace, viewport, filter, features),
    ]
    .into_iter()
    .flatten()
    .collect();
    if tools.is_empty() {
        return None;
    }

    // Two even columns rather than wrapped rows: the labels differ in length, and wrapping leaves a
    // ragged edge that moves as the rail is resized.
    Some(AnyView::new(view! {
        box(class = "wt-armor-tools", attr:data-testid = "armor-options-tools") {{tools}}
    }))
}

/// The viewport context menu's toggles, read from `viewport` and the workspace as the menu opens.
pub fn quick_toggles(workspace: ArmorWorkspace, viewport: ArmorViewportState) -> Vec<AnyView> {
    let features = viewport.scene_features();
    let armor = features.is_none_or(|features| features.armor);
    let ship = features.is_none_or(|features| features.ship);
    let no_armor = Signal::derive_local(move || !viewport.has_armor());
    let no_splash_data = Signal::derive_local(move || !viewport.splash_state().has_data);

    let mut items: Vec<AnyView> = Vec::new();
    if armor {
        items.push(toggle_item(
            t!("ui.armor.show_hidden").into_owned(),
            Signal::derive_local(move || viewport.show_hidden_only()),
            no_armor,
            move |on| viewport.set_show_hidden_only(on),
        ));
        items.push(toggle_item(
            t!("ui.armor.gaps").into_owned(),
            Signal::derive_local(move || viewport.gaps().shown),
            no_armor,
            move |on| viewport.set_show_gaps(on),
        ));
    }
    if ship {
        items.push(toggle_item(
            t!("ui.armor.splash_mode").into_owned(),
            Signal::derive_local(move || viewport.splash_state().shown),
            no_splash_data,
            move |on| viewport.set_splash_mode(on),
        ));
        items.push(toggle_item(
            t!("ui.armor.splash_boxes").into_owned(),
            Signal::derive_local(move || viewport.splash_state().boxes_shown),
            no_splash_data,
            move |on| viewport.set_show_splash_boxes(on),
        ));
    }

    if armor {
        let pen_check = t!("ui.armor.pen_check").into_owned();
        items.push(AnyView::new(view! {
            MenuItem(on_select = UnsyncCallback::new(move |()| workspace.open_section(OptionSection::PenCheck))) {
                {pen_check}
            }
        }));
    }
    items.push(toggle_item(
        t!("ui.armor.options_panel").into_owned(),
        Signal::derive_local(move || workspace.options_open.get()),
        false,
        move |_| workspace.toggle_options(),
    ));

    items.push(AnyView::new(view! { MenuSeparator() }));
    let export = t!("ui.armor.export_model").into_owned();
    let shell = Shell::current();
    items.push(AnyView::new(view! {
        MenuItem(disabled = no_armor, on_select = UnsyncCallback::new(move |()| open_export(shell, workspace, viewport))) {
            {export}
        }
    }));

    items
}

/// One ticked menu entry; choosing it asks for the opposite of `checked`.
fn toggle_item(
    label: String,
    checked: Signal<bool, LocalStorage>,
    disabled: impl Into<Signal<bool, LocalStorage>>,
    apply: impl Fn(bool) + 'static,
) -> AnyView {
    let disabled: Signal<bool, LocalStorage> = disabled.into();

    AnyView::new(view! {
        MenuCheckboxItem(checked = Binding::controlled(checked, apply), disabled = disabled) {
            {label}
        }
    })
}

/// Whether `filter` keeps a tool, by its label or by the tooltip that says what it does.
fn keeps_tool(filter: &RailFilter, label: &str, tooltip: &str) -> bool {
    filter.keeps(label) || filter.keeps(tooltip)
}

/// A label with the count a tool carries while it is on.
fn counted_label(name: &str, count: Option<usize>) -> String {
    match count {
        Some(count) => format!("{name} ({count})"),
        None => name.to_owned(),
    }
}

/// What a tool button shows and does.
struct Tool {
    test_id: &'static str,
    glyph: Signal<Glyph, LocalStorage>,
    /// The label, marked where `filter` matches it.
    label: Signal<String, LocalStorage>,
    filter: RailFilter,
    tooltip: String,
    /// `None` for a command rather than a mode.
    selected: Option<Signal<bool, LocalStorage>>,
    disabled: Signal<bool, LocalStorage>,
    on_click: Box<dyn Fn()>,
}

/// A tool button. A mode reads as selected while it is on. Its label is shortened rather than cut
/// off where a large zoom leaves its column too narrow; the tooltip says what it does.
fn tool_button(tool: Tool) -> AnyView {
    let Tool { test_id, glyph, label, filter, tooltip, selected, disabled, on_click } = tool;
    // A mode is wrapped by the id its selection is read from, and its button carries a suffixed
    // one; a command's button carries the id itself.
    let (wrapper_id, button_id) = match selected {
        Some(_) => (Some(test_id.to_owned()), format!("{test_id}-button")),
        None => (None, test_id.to_owned()),
    };
    let selected = selected.unwrap_or_else(|| Signal::stored_local(false));
    let shown_label = move || filter.marked(label.get()).view();

    AnyView::new(view! {
        box(
            class = "wt-armor-tool",
            attr:data-testid = wrapper_id,
            attr:data-selected = move || selected.get().then(String::new),
            a11y:selected = move || selected.get()
        ) {
            Tooltip {
                TooltipTrigger(class = "wt-armor-tool") {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Xs,
                        class = "wt-armor-tool__button",
                        disabled = disabled,
                        attr:data-testid = button_id,
                        attr:data-selected = move || selected.get().then(String::new),
                        a11y:toggled_on = move || selected.get(),
                        on:click = move |_| on_click()
                    ) {
                        AppIcon(glyph = glyph)
                        box(class = "wt-armor-tool__label", attr:data-testid = "armor-tool-label") {{shown_label}}
                    }
                }
                TooltipContent {{tooltip.clone()}}
            }
        }
    })
}

/// Rail toggle for "Show Hidden": the plates that are part of the combat model but that the game's
/// own armor viewer never draws. A mode rather than an addition, so it reads as selected while on.
fn hidden_plates_button(viewport: ArmorViewportState, filter: &RailFilter) -> Option<AnyView> {
    let tooltip = t!("ui.armor.show_hidden_tooltip").into_owned();
    let label = t!("ui.armor.show_hidden").into_owned();
    if !keeps_tool(filter, &label, &tooltip) {
        return None;
    }
    let showing = Signal::derive_local(move || viewport.show_hidden_only());

    Some(tool_button(Tool {
        test_id: "armor-show-hidden",
        glyph: Signal::stored_local(Glyph::EyeOff),
        label: Signal::stored_local(label),
        filter: filter.clone(),
        tooltip,
        selected: Some(showing),
        disabled: Signal::derive_local(move || !viewport.has_armor()),
        on_click: Box::new(move || viewport.set_show_hidden_only(!viewport.show_hidden_only())),
    }))
}

/// Rail toggle for gap detection, which carries its own count: the number is the answer, so it is
/// on the control rather than behind a hover.
fn gaps_button(viewport: ArmorViewportState, cx: &SectionCx) -> Option<AnyView> {
    let tooltip = t!("ui.armor.gaps_tooltip").into_owned();
    let name = t!("ui.armor.gaps").into_owned();
    let count_now = cx.matching.and_then(|state| state.gaps);
    if !keeps_tool(&cx.filter, &counted_label(&name, count_now), &tooltip) {
        return None;
    }
    let label = Signal::derive_local(move || {
        let gaps = viewport.gaps();
        counted_label(&name, gaps.shown.then_some(gaps.count))
    });
    // A hull with openings is worth a warning tone; one with none, having been looked at, is worth
    // saying so quietly.
    let glyph = Signal::derive_local(move || {
        let gaps = viewport.gaps();
        if gaps.shown && gaps.count > 0 { Glyph::TriangleAlert } else { Glyph::EyeOff }
    });

    Some(tool_button(Tool {
        test_id: "armor-show-gaps",
        glyph,
        label,
        filter: cx.filter.clone(),
        tooltip,
        selected: Some(Signal::derive_local(move || viewport.gaps().shown)),
        disabled: Signal::derive_local(move || !viewport.has_armor()),
        on_click: Box::new(move || viewport.set_show_gaps(!viewport.gaps().shown)),
    }))
}

/// Rail toggle for splash mode, carrying how many zones the last burst reached: that number is the
/// answer the mode exists to give. Refused on a hull that ships no splash file; the tooltip says so.
fn splash_button(viewport: ArmorViewportState, cx: &SectionCx, has_data: bool) -> Option<AnyView> {
    let tooltip = if has_data {
        t!("ui.armor.splash_tooltip_mode").into_owned()
    } else {
        t!("ui.armor.splash_no_data").into_owned()
    };
    let name = t!("ui.armor.splash_mode").into_owned();
    let count_now = cx.matching.and_then(|state| state.splash);
    if !keeps_tool(&cx.filter, &counted_label(&name, count_now), &tooltip) {
        return None;
    }
    let label = Signal::derive_local(move || {
        let state = viewport.splash_state();
        counted_label(&name, state.shown.then_some(state.zones_reached))
    });

    Some(tool_button(Tool {
        test_id: "armor-splash",
        glyph: Signal::stored_local(Glyph::Bomb),
        label,
        filter: cx.filter.clone(),
        tooltip,
        selected: Some(Signal::derive_local(move || viewport.splash_state().shown)),
        disabled: Signal::derive_local(move || !viewport.splash_state().has_data),
        on_click: Box::new(move || viewport.set_splash_mode(!viewport.splash_state().shown)),
    }))
}

/// Rail toggle for the splash-box outlines, which stand on their own: they are worth seeing while
/// placing a burst and after it.
fn splash_boxes_button(viewport: ArmorViewportState, filter: &RailFilter) -> Option<AnyView> {
    let tooltip = t!("ui.armor.splash_tooltip").into_owned();
    let label = t!("ui.armor.splash_boxes").into_owned();
    if !keeps_tool(filter, &label, &tooltip) {
        return None;
    }

    Some(tool_button(Tool {
        test_id: "armor-splash-boxes",
        glyph: Signal::stored_local(Glyph::Box),
        label: Signal::stored_local(label),
        filter: filter.clone(),
        tooltip,
        selected: Some(Signal::derive_local(move || viewport.splash_state().boxes_shown)),
        disabled: Signal::derive_local(move || !viewport.splash_state().has_data),
        on_click: Box::new(move || viewport.set_show_splash_boxes(!viewport.splash_state().boxes_shown)),
    }))
}

/// Opens the export for the scene `viewport` shows: the confirm panel over the viewport for a ship,
/// the export dialog for a model, which asks what the file should contain.
fn open_export(shell: Shell, workspace: ArmorWorkspace, viewport: ArmorViewportState) {
    let Some(request) = viewport.open_export_confirm() else {
        return;
    };
    let (Some(bundle), Some(models)) =
        (workspace.library.ready_untracked(), workspace.library.models_ready_untracked())
    else {
        tracing::warn!("model viewer: export requested before the model library finished loading");
        return;
    };

    open_export_dialog(shell, ExportRequest::Model { request, models }, Arc::clone(&bundle.assets));
}

/// Rail trigger for the export of the scene shown in this pane, at its current hull, LOD and
/// module selection. Gated on a scene being loaded.
fn export_button(
    workspace: ArmorWorkspace,
    viewport: ArmorViewportState,
    filter: &RailFilter,
    features: Option<SceneFeatures>,
) -> Option<AnyView> {
    let tooltip = match features {
        Some(SceneFeatures { ship: false, .. }) => t!("ui.model_view.export_tooltip").into_owned(),
        Some(_) | None => t!("ui.armor.export_tooltip").into_owned(),
    };
    let label = t!("ui.replay.section_export").into_owned();
    if !keeps_tool(filter, &label, &tooltip) {
        return None;
    }
    let shell = Shell::current();

    Some(tool_button(Tool {
        test_id: "armor-export-trigger",
        glyph: Signal::stored_local(Glyph::Download),
        label: Signal::stored_local(label),
        filter: filter.clone(),
        tooltip,
        selected: None,
        disabled: Signal::derive_local(move || !viewport.has_armor()),
        on_click: Box::new(move || open_export(shell, workspace, viewport)),
    }))
}

/// One of a row of choices, which reads as selected while it is the one chosen.
pub(super) fn choice_button(
    test_id: String,
    label: MarkedLabel,
    selected: Signal<bool, LocalStorage>,
    disabled: impl Into<Signal<bool, LocalStorage>>,
    tooltip: Option<String>,
    on_click: impl Fn() + 'static,
) -> AnyView {
    install_stylesheet(SHEET, CSS);
    let disabled: Signal<bool, LocalStorage> = disabled.into();
    let on_click = Rc::new(on_click);
    let button = move || {
        let on_click = Rc::clone(&on_click);
        view! {
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Xs,
                class = "wt-armor-choice",
                disabled = disabled,
                attr:data-testid = test_id.clone(),
                attr:data-selected = move || selected.get().then(String::new),
                a11y:selected = move || selected.get(),
                a11y:toggled_on = move || selected.get(),
                on:click = move |_| on_click()
            ) {{label.view()}}
        }
    };
    let Some(tooltip) = tooltip else {
        return AnyView::new(button());
    };

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {{button()}}
            TooltipContent {{tooltip.clone()}}
        }
    })
}
