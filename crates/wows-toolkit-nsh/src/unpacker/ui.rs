//! Controls the Unpacker's panes share, styled with the app's theme tokens.

use nsh_core::element::Input;
use nsh_core::element::input;
use nsh_core::prelude::*;

use crate::icons::Icon;
use crate::icons::icon;

/// How a button reads against its surroundings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    Primary,
    Secondary,
    Ghost,
}

impl ButtonStyle {
    fn class(self) -> &'static str {
        match self {
            Self::Primary => {
                "min-h-7 px-3 rounded-(--radius) border border-border bg-primary text-primary-foreground \
                 items-center justify-center hover:opacity-85 active:opacity-70 disabled:opacity-45"
            }
            Self::Secondary => {
                "min-h-7 px-3 rounded-(--radius) border border-border bg-secondary text-secondary-foreground \
                 items-center justify-center hover:opacity-85 active:opacity-70 disabled:opacity-45"
            }
            Self::Ghost => {
                "min-h-6 px-2 rounded-(--radius) text-foreground items-center justify-center \
                 hover:bg-accent active:opacity-70 disabled:opacity-45"
            }
        }
    }
}

/// A button showing `label`; the caller adds `on_press`, `disabled` and an
/// accessibility label where the text alone is ambiguous.
pub fn button(label: impl IntoReactive<SharedString>, style: ButtonStyle) -> View {
    view()
        .accessible(true)
        .role(AccessibilityRole::Button)
        .app_class(class(style.class()))
        .child(text(label).app_class(class("text-xs")))
}

/// A ghost button showing `glyph`, named and hinted by `label`.
pub fn icon_button(glyph: Icon, label: String) -> View {
    view()
        .accessible(true)
        .role(AccessibilityRole::Button)
        .label(label.clone())
        .tooltip(tip(label))
        .app_class(class(ButtonStyle::Ghost.class()))
        .child(dim_icon(glyph))
}

/// `glyph` at the controls' icon size, in the dim text color.
pub fn dim_icon(glyph: Icon) -> Svg {
    icon(glyph, px(14.0)).app_class(class("text-muted-foreground"))
}

/// A one-line text field bound to `value`.
pub fn text_field(value: Signal<String>, placeholder: String, label: String) -> Input {
    input()
        .label(label)
        .value(move || value.get())
        .placeholder(placeholder)
        .on_change(move |text: &str| value.set(text.to_owned()))
        .app_class(class("min-h-7 px-2 rounded-(--radius) border border-input bg-background text-xs text-foreground"))
}

/// Dim secondary text.
pub fn caption(content: impl IntoReactive<SharedString>) -> Text {
    text(content).app_class(class("text-xs text-muted-foreground"))
}

/// A hover hint, drawn as a small popover.
pub fn tip(content: impl IntoReactive<SharedString>) -> Text {
    text(content).app_class(class(
        "px-2 py-1 rounded-(--radius) border border-border bg-popover text-xs text-popover-foreground",
    ))
}

/// A status line in the middle of an empty pane.
pub fn status(content: impl IntoReactive<SharedString>) -> AnyElement {
    view()
        .app_class(class("flex-1 items-center justify-center p-6"))
        .child(text(content).app_class(class("text-sm text-muted-foreground")))
        .into_element()
}

/// Tailwind classes as `app_class` takes them.
pub fn class(names: &'static str) -> Option<SharedString> {
    Some(SharedString::from(names))
}
