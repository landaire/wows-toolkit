use nsh_core::prelude::*;

/// A text button whose background marks the selected choice.
pub fn button(
    label: &'static str,
    selected: impl Fn() -> bool + Copy + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .px(px(12.0))
        .py(px(6.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(rgb(0x5a6b7d))
        .bg(move || if selected() { rgb(0x2f6fb3) } else { rgb(0x1e2a36) })
        .cursor_pointer()
        .on_press(on_press)
        .child(text(label).text_color(rgb(0xffffff)))
}
