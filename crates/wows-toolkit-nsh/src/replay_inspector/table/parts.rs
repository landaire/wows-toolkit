//! Small pieces the table's cells, details and menus share.

use nsh_base::menu_item;
use nsh_base::menu_separator;
use nsh_core::prelude::*;

use crate::icons::Icon;
use crate::icons::icon;

/// Tailwind classes as `app_class` takes them.
pub fn class(names: &'static str) -> Option<SharedString> {
    Some(SharedString::from(names))
}

/// A hover hint.
pub fn tip(content: impl Into<SharedString>) -> AnyElement {
    text(content.into())
        .app_class(class(
            "px-2 py-1 rounded-(--radius) border border-border bg-popover text-xs text-popover-foreground",
        ))
        .into_element()
}

/// A hover hint of several lines in the monospace font, so padded
/// breakdown columns line up.
pub fn tip_lines(content: &str) -> AnyElement {
    view()
        .app_class(class("px-2 py-1 rounded-(--radius) border border-border bg-popover"))
        .children(
            content
                .split('\n')
                .map(|line| {
                    text(line.to_owned())
                        .app_class(class("font-mono text-xs text-popover-foreground"))
                        .into_element()
                })
                .collect::<Vec<_>>(),
        )
        .into_element()
}

/// Detail text.
pub fn detail_text(content: impl Into<SharedString>) -> Text {
    text(content.into()).app_class(class("text-xs text-foreground"))
}

/// A detail text in the dim tone.
pub fn dim_text(content: impl Into<SharedString>) -> Text {
    text(content.into()).app_class(class("text-xs text-muted-foreground"))
}

pub fn section_heading(content: impl Into<SharedString>) -> AnyElement {
    text(content.into()).app_class(class("text-xs font-bold text-foreground")).into_element()
}

pub fn separator() -> AnyElement {
    view().app_class(class("h-px w-full bg-border")).into_element()
}

/// A small pressable glyph, named for assistive technologies.
pub fn glyph_button(mark: Icon, label: impl Into<SharedString>) -> View {
    view()
        .accessible(true)
        .role(AccessibilityRole::Button)
        .label(label.into())
        .app_class(class(
            "flex-none items-center justify-center w-5 h-5 rounded-(--radius) text-muted-foreground \
             hover:bg-accent hover:text-foreground active:opacity-70",
        ))
        .child(icon(mark, px(12.0)))
}

/// The classes a row menu's popup draws with.
pub const MENU_POPUP: &str = "min-w-56 p-1 rounded-(--radius) border border-border bg-popover";

/// One menu item: an optional glyph and its text.
pub fn menu_entry(label: impl Into<SharedString>, mark: Option<Icon>, on_select: impl Fn() + 'static) -> AnyElement {
    let label = label.into();
    let mark = match mark {
        Some(mark) => icon(mark, px(14.0)).app_class(class("flex-none text-popover-foreground")).into_element(),
        None => view().app_class(class("w-3.5 flex-none")).into_element(),
    };

    menu_item()
        .accessibility_label(label.clone())
        .app_class(class("flex-row items-center gap-2 min-h-7 px-2 rounded-(--radius) hover:bg-accent"))
        .on_select(on_select)
        .child(mark)
        .child(text(label).app_class(class("text-xs text-popover-foreground")))
        .into_element()
}

pub fn menu_rule() -> AnyElement {
    menu_separator().app_class(class("h-px my-1 bg-border")).into_element()
}
