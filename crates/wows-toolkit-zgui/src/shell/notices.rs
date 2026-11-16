//! Errors worth reading in full, and how a finished job says what happened.

use rust_i18n::t;
use zgui::css;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::NodeRef;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogFooterProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;

use crate::shell::HostedDialogProps;
use crate::shell::Shell;
use crate::shell::dialogs::focus_when_bound;

const SHEET: &str = "wt-notices";

const CSS: &str = css!(
    ".wt-error {
        display: flex;
        flex-direction: column;
        gap: 8px;
        max-width: 560px;
    }
    .wt-error__lead { font-weight: 700; }
    .wt-error__text {
        max-height: 320px;
        overflow-y: auto;
        font-size: 11px;
        white-space: pre-wrap;
        user-select: text;
    }"
);

/// How long a failure may be before it is worth the error window rather than a message.
const TOO_LONG_TO_TOAST: usize = 160;

impl Shell {
    /// Says why a finished job failed: a sentence as a message, and anything longer, such as a
    /// chain of causes, in the error window, where it can be read in full and copied.
    pub fn say_failure(self, said: String) {
        if said.len() > TOO_LONG_TO_TOAST || said.lines().count() > 1 {
            show_error(self, said);
        } else {
            self.toasts().failed(said);
        }
    }
}

/// Shows an error worth reading in full, with a way to copy it.
///
/// A message is right for a sentence and wrong for a chain of causes: it goes by itself and cannot
/// be copied into a bug report.
pub fn show_error(shell: Shell, text: String) {
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let text = StoredValue::new(text.clone());
        let copy = NodeRef::new();
        focus_when_bound(copy);

        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(dismiss_on_outside_press = false, attr:data-testid = "notice-error") {
                    DialogHeader {
                        DialogTitle {{t!("ui.windows.error").into_owned()}}
                    }
                    column(class = "wt-error") {
                        text(class = "wt-error__lead") {{t!("ui.labels.error_occurred").into_owned()}}
                        text(class = "wt-error__text", attr:data-testid = "notice-error-text") {{text.get_value()}}
                    }
                    DialogFooter {
                        Button(
                            node_ref = copy,
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            attr:data-testid = "notice-error-copy",
                            on:click = move |_| shell.copy_text(text.get_value())
                        ) {
                            {t!("ui.buttons.copy").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use landaire_ui::test_support::HeadlessWindow;
    use zgui::geom::DevicePx;
    use zgui::geom::Point;

    use super::show_error;
    use crate::interaction_tests::open_bare_shell;
    use crate::interaction_tests::shell;

    fn open_error() -> HeadlessWindow {
        let mut window = open_bare_shell();
        show_error(shell(), "the first cause\nthe second cause".to_owned());
        window.advance(Duration::from_millis(400));

        window
    }

    #[test]
    fn a_press_past_the_error_leaves_it_open() {
        let mut window = open_error();

        window.click_at(Point::new(DevicePx(4.0), DevicePx(4.0)));
        window.advance(Duration::from_millis(400));

        assert!(window.by_test_id("notice-error").is_some());
    }

    #[test]
    fn the_error_opens_with_its_copy_button_taking_enter() {
        let mut window = open_error();

        let copy = window.by_test_id("notice-error-copy").expect("the copy button is shown");
        assert_eq!(window.a11y(copy).map(|state| state.focused), Some(true));
    }
}
