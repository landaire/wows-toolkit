use nsh_core::prelude::*;

/// A text button whose background marks the selected choice.
pub fn button(
    label: &'static str,
    selected: impl Fn() -> bool + Copy + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .app_class(move || {
            Some(
                if selected() {
                    "px-3 py-1.5 rounded-(--radius) border border-primary bg-primary text-primary-foreground"
                } else {
                    "px-3 py-1.5 rounded-(--radius) border border-border bg-secondary text-secondary-foreground \
                     hover:bg-accent hover:text-accent-foreground"
                }
                .into(),
            )
        })
        .cursor_pointer()
        .on_press(on_press)
        .child(text(label))
}
