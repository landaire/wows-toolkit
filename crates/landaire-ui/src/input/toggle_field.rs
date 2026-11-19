//! A switch or a checkbox with the text that names it beside it, where a press on the text turns
//! the control as a press on the control does.

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::view;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::LabelProps;
use zgui_ui::SwitchProps;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Controllable;

/// What the field rules are installed under.
const SHEET: &str = "landaire-ui-toggle-field";

const CSS: &str = css!(
    ".lui-toggle-field {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
    }
    .lui-toggle-field .lui-toggle-field__label {
        cursor: default;
        font-weight: 400;
    }
    .lui-toggle-field-disabled .lui-toggle-field__label {
        opacity: 0.5;
    }"
);

/// A switch named by the text beside it. A press on the text turns it as a press on the switch
/// does. While disabled the text is dimmed and a press on it does nothing.
#[component]
pub fn SwitchField(
    /// Whether it is on, when the caller holds it.
    #[prop(into, optional)]
    checked: Binding<bool>,
    /// Whether it starts on, when the field owns that itself.
    #[prop(default = false)]
    default_checked: bool,
    /// Told whenever it changes, whoever owns it.
    #[prop(optional)]
    on_change: Option<UnsyncCallback<bool>>,
    /// Whether it can be operated.
    #[prop(into, default = Signal::stored_local(false))]
    disabled: Signal<bool, LocalStorage>,
    /// The text beside it, which names it for a reader.
    #[prop(into)]
    label: String,
    /// Classes merged after the field's own, on the row that holds the switch and its text.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else, forwarded to the switch.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let held = Controllable::new(checked, default_checked, on_change);
    let control = NodeRef::new();
    let name = NodeRef::new();

    let to_switch = Binding::controlled(held.signal(), move |on| held.set(on));
    let toggle = move |_: &mut EventCx<'_, events::Click>| {
        if !disabled.get_untracked() {
            held.toggle();
        }
    };

    view! {
        row(class = "lui-toggle-field", class:lui-toggle-field-disabled = move || disabled.get(), class = class) {
            Switch(node_ref = control, checked = to_switch, disabled = disabled, labelled_by = name, {..attrs})
            Label(class = "lui-toggle-field__label", node_ref = name, control = control, on:click = toggle) {{label}}
        }
    }
}

/// A checkbox named by the text beside it. A press on the text turns it as a press on the
/// checkbox does, a mixed one to checked. While disabled the text is dimmed and a press on it does
/// nothing.
#[component]
pub fn CheckboxField(
    /// What it is, when the caller holds it.
    #[prop(into, optional)]
    checked: Binding<Checked>,
    /// What it starts as, when the field owns it itself.
    #[prop(into, default = Checked::No)]
    default_checked: Checked,
    /// Told whenever it changes, whoever owns it.
    #[prop(optional)]
    on_change: Option<UnsyncCallback<Checked>>,
    /// Whether it can be operated.
    #[prop(into, default = Signal::stored_local(false))]
    disabled: Signal<bool, LocalStorage>,
    /// The text beside it, which names it for a reader.
    #[prop(into)]
    label: String,
    /// Classes merged after the field's own, on the row that holds the checkbox and its text.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else, forwarded to the checkbox.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let held = Controllable::new(checked, default_checked, on_change);
    let control = NodeRef::new();
    let name = NodeRef::new();

    let to_checkbox = Binding::controlled(held.signal(), move |next| held.set(next));
    let toggle = move |_: &mut EventCx<'_, events::Click>| {
        if !disabled.get_untracked() {
            held.set(held.get_untracked().toggled());
        }
    };

    view! {
        row(class = "lui-toggle-field", class:lui-toggle-field-disabled = move || disabled.get(), class = class) {
            Checkbox(node_ref = control, checked = to_checkbox, disabled = disabled, labelled_by = name, {..attrs})
            Label(class = "lui-toggle-field__label", node_ref = name, control = control, on:click = toggle) {{label}}
        }
    }
}
