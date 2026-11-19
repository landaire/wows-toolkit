//! Moving the focus to an element once it exists.

use zgui::reactive::RenderEffect;
use zgui::reactive::on_cleanup_local;
use zgui::view::NodeRef;

/// What [`focus_when`] does with a text field's text as it moves the focus there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusText {
    /// Leaves the text and the caret as they are.
    Keep,
    /// Selects all of it, so what is typed next replaces it.
    SelectAll,
}

/// Focuses `element` as soon as it is built, when `wanted` says so, and treats its text as `text`
/// says. `wanted` is read again whenever what it reads changes. `taken` runs each time the focus
/// moves, for the caller to clear its request.
///
/// Call it from the component that builds `element`. The focus stays where it is once the
/// component is gone.
pub fn focus_when(element: NodeRef, wanted: impl Fn() -> bool + 'static, text: FocusText, taken: impl Fn() + 'static) {
    // A handle binds as its element is built, which is after the component's body runs, so the
    // effect reads the handle to run again once it is bound.
    let moving = RenderEffect::new(move |_| {
        let bound = element.get().is_some();
        if !bound || !wanted() {
            return;
        }

        element.focus();
        if text == FocusText::SelectAll {
            element.select_all();
        }
        taken();
    });
    on_cleanup_local(move || drop(moving));
}
