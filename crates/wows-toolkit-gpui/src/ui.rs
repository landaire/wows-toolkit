//! Small rendering helpers shared across the tabs.

use gpui_kit::*;

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
