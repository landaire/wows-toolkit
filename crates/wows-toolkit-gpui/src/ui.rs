//! Small rendering helpers shared across the tabs.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::h_flex;
use gpui_kit::component::list::ListItem;
use gpui_kit::*;

/// The background for row `index` of a list or table, or `None` for the rows
/// that keep the panel's own.
///
/// Every other row is lifted, starting with the first, which is what egui
/// stripes (`egui_extras::Table`, `row_index.is_multiple_of(2)`) at the
/// strength egui lifts them by (see `theme::Palette::striped`). A long table
/// of numbers is read across a row, and the lift is what keeps an eye on one.
pub fn stripe(index: usize, cx: &App) -> Option<Hsla> {
    index.is_multiple_of(2).then(|| cx.theme().table_even)
}

/// Draws the indent guides for a tree row at `depth`, so a deep row reads as
/// belonging to the group above it rather than floating at an arbitrary
/// offset.
///
/// One hairline per level, at the left edge of the space that level indents
/// by, matching the connector lines `egui_ltreeview` draws.
pub fn indent_guides(depth: usize, indent: Pixels, cx: &App) -> impl IntoElement {
    let color = cx.theme().border.opacity(0.7);
    h_flex().flex_none().children((0..depth).map(move |_| {
        // The line sits at the right edge of each level's indent, which is
        // where the next level's rows begin.
        div().w(indent).h_full().flex_none().border_r_1().border_color(color)
    }))
}

/// The metrics every tree row is drawn at.
///
/// `ListItem` renders at `text_base` (a full rem, 16px) inside its own
/// `py_1 px_3`, while the app's body text is `theme.font_size` (12.5px). A
/// tree left at those defaults draws a quarter larger than the panel around
/// it and a third taller than it needs to be, which is what made the
/// listings look padded out. The component applies its own style last, so
/// setting these on the item wins.
pub fn tree_row(item: ListItem, cx: &App) -> ListItem {
    item.text_size(cx.theme().font_size).py_0().px_1()
}

/// Wraps a control that shows a selected state so the selection is announced.
///
/// `gpui-component`'s `Button::selected` is presentation only -- it styles the
/// button and deliberately sets no aria state -- so a row of selectable
/// buttons reads to assistive technology, and to the UI tests, as a row of
/// plain buttons with no indication of which one is active. Wrapping them
/// carries that state without giving up the button's look.
pub fn selectable(id: impl Into<ElementId>, selected: bool, control: impl IntoElement) -> impl IntoElement {
    div().id(id).test_support().aria_selected(selected).child(control)
}

/// A vertical rule between two groups of controls on one row.
///
/// The egui toolbars separate their groups with `ui.separator()`; without it
/// a row of buttons reads as one undifferentiated strip.
pub fn rule_v(cx: &App) -> impl IntoElement {
    div().flex_none().w(px(1.)).h(px(16.)).mx_1().bg(cx.theme().border)
}
