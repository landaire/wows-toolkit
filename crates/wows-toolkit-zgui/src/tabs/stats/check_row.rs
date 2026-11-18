//! A checkbox beside the words naming it, as the stats menus list their choices.

use landaire_ui::MarkedTextProps;
use zgui::component;
use zgui::css;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::install_stylesheet;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::LabelProps;
use zgui_ui_primitives::Binding;

use crate::text_marks::Marks;

const SHEET: &str = "wt-stats-check-row";

const CSS: &str = css!(
    ".wt-check-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
    }"
);

/// A checkbox and the words naming it, marked where `marks` says, either of which toggles it.
#[component]
pub fn CheckRow(
    #[prop(into)] checked: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(into, default = Signal::stored_local(Marks::default()))] marks: Signal<Marks, LocalStorage>,
    change: UnsyncCallback<bool>,
    #[prop(into)] test_id: String,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let name = NodeRef::new();
    let control = NodeRef::new();
    let binding = Binding::controlled(
        Signal::derive_local(move || if checked.get() { Checked::Yes } else { Checked::No }),
        move |next: Checked| change.run(next == Checked::Yes),
    );

    view! {
        row(class = "wt-check-row") {
            Checkbox(node_ref = control, checked = binding, disabled = disabled, labelled_by = name, attr:data-testid = test_id)
            Label(node_ref = name, control = control, on:click = move |_| {
                if !disabled.get_untracked() {
                    change.run(!checked.get_untracked());
                }
            }) {{move || view! { MarkedText(text = label.get(), ranges = marks.get()) }}}
        }
    }
}
