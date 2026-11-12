//! A debug-mode JSON payload in the side panel: the replay's metadata, its battle results, or one
//! player's metadata.
//!
//! Shown as monospace lines in a scroll area. zgui has no read-only code editor, so the JSON is
//! not highlighted as the GPUI editor highlights it; the copy button puts it on the clipboard.

use std::rc::Rc;

use rust_i18n::t;
use zgui::component;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::ScrollAreaProps;

use crate::icons::AppIconProps;
use crate::shell::Shell;

/// One payload, a line per element so a long payload is laid out as rows.
#[component]
pub fn RawJsonPanel(content: Rc<str>) -> impl IntoView {
    let shell = Shell::current();
    let lines: Vec<AnyView> = content
        .lines()
        .map(|line| {
            let line = line.to_owned();
            AnyView::new(view! { text(class = "wt-rp-json__line") {{line}} })
        })
        .collect();

    view! {
        column(class = "wt-rp-json", attr:data-testid = "replay-raw-json") {
            row(class = "wt-rp-chat__toolbar") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "raw-json-copy",
                    on:click = move |_| shell.copy_text(content.to_string())
                ) {
                    AppIcon(glyph = wows_toolkit_icons::Glyph::Copy)
                    text {{t!("ui.buttons.copy").into_owned()}}
                }
            }
            ScrollArea(class = "wt-rp-json__body") {
                column(class = "wt-rp-json__lines") {{lines}}
            }
        }
    }
}
