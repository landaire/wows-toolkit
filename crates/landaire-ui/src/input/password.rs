//! A password field with a button that shows and hides what was typed.

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::install_stylesheet;
use zgui_ui::InputGroupAddonAlign;
use zgui_ui::InputGroupAddonProps;
use zgui_ui::InputGroupButtonProps;
use zgui_ui::InputGroupButtonSize;
use zgui_ui::InputGroupInputProps;
use zgui_ui::InputGroupProps;
use zgui_ui_icons::IconData;
use zgui_ui_icons::IconProps;
use zgui_ui_primitives::Binding;

/// What the password field rules are installed under.
const SHEET: &str = "landaire-ui-password-input";

const CSS: &str = css!(
    ".lui-password-input {
        width: 100%;
    }"
);

/// The outline of an almond with a round pupil in it, for showing a password.
const EYE: IconData = IconData::new(
    "eye",
    24.0,
    "M2 12 C4.5 7.5 8 5 12 5 C16 5 19.5 7.5 22 12 C19.5 16.5 16 19 12 19 C8 19 4.5 16.5 2 12 Z \
     M4.3 12 C6.4 15.3 9 17 12 17 C15 17 17.6 15.3 19.7 12 C17.6 8.7 15 7 12 7 C9 7 6.4 8.7 4.3 12 Z \
     M12 9 C13.657 9 15 10.343 15 12 C15 13.657 13.657 15 12 15 C10.343 15 9 13.657 9 12 \
     C9 10.343 10.343 9 12 9 Z",
);

/// [`EYE`] struck through from corner to corner, for hiding a password. The bar is wound as the
/// almond's outside is, so it stays filled where it crosses the outline.
const EYE_OFF: IconData = IconData::new(
    "eye-off",
    24.0,
    "M2 12 C4.5 7.5 8 5 12 5 C16 5 19.5 7.5 22 12 C19.5 16.5 16 19 12 19 C8 19 4.5 16.5 2 12 Z \
     M4.3 12 C6.4 15.3 9 17 12 17 C15 17 17.6 15.3 19.7 12 C17.6 8.7 15 7 12 7 C9 7 6.4 8.7 4.3 12 Z \
     M12 9 C13.657 9 15 10.343 15 12 C15 13.657 13.657 15 12 15 C10.343 15 9 13.657 9 12 \
     C9 10.343 10.343 9 12 9 Z \
     M3.7 2.3 L21.7 20.3 L20.3 21.7 L2.3 3.7 Z",
);

/// The names a [`PasswordInput`]'s button is read out as.
#[derive(Clone, Copy, Debug)]
pub struct PasswordInputStrings {
    /// While the text is masked.
    pub show: Signal<String, LocalStorage>,
    /// While the text is shown.
    pub hide: Signal<String, LocalStorage>,
}

impl PasswordInputStrings {
    /// The names in English.
    pub fn new() -> Self {
        Self { show: "Show password".into(), hide: "Hide password".into() }
    }
}

impl Default for PasswordInputStrings {
    fn default() -> Self {
        Self::new()
    }
}

/// A secure text field bound to `value`, with an eye button at its end that reveals the text and
/// masks it again on the next press. The button is named for what a press does. Forwarded
/// attributes and handlers land on the text field, so `a11y:label` names the field and
/// `on:key_down` sees the keys typed into it.
#[component]
pub fn PasswordInput(
    /// The text.
    #[prop(into)]
    value: Binding<String>,
    /// What to show while it is empty.
    #[prop(into, default = String::new())]
    placeholder: String,
    /// Where to record the text field, to focus it.
    #[prop(optional)]
    node_ref: Option<NodeRef>,
    /// The button's names. Left out, English.
    #[prop(optional)]
    strings: Option<PasswordInputStrings>,
    /// Classes merged after the group's own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else, forwarded to the text field.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let PasswordInputStrings { show, hide } = strings.unwrap_or_default();
    let revealed = RwSignal::new_local(false);
    let secure = Signal::derive_local(move || !revealed.get());
    let field = node_ref.unwrap_or_default();

    let button_label = move || if revealed.get() { hide.get() } else { show.get() };
    let icon = Signal::derive_local(move || if revealed.get() { EYE_OFF } else { EYE });

    view! {
        InputGroup(class = "lui-password-input", class = class) {
            InputGroupInput(value = value, placeholder = placeholder, secure = secure, node_ref = field, {..attrs})
            InputGroupAddon(align = InputGroupAddonAlign::InlineEnd) {
                InputGroupButton(
                    size = InputGroupButtonSize::IconXs,
                    a11y:label = button_label,
                    on:click = move |_| revealed.set(!revealed.get_untracked())
                ) {
                    Icon(icon = icon)
                }
            }
        }
    }
}
