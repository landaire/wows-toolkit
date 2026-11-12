//! Errors worth reading in full, and how a finished job says what happened.

use rust_i18n::t;
use zgui::css;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::view;
use zgui::view::AnyView;
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

/// Shows an error worth reading in full, with a way to copy it.
///
/// A message is right for a sentence and wrong for a chain of causes: it goes by itself and cannot
/// be copied into a bug report.
pub fn show_error(shell: Shell, text: String) {
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let text = StoredValue::new(text.clone());

        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(attr:data-testid = "notice-error") {
                    DialogHeader {
                        DialogTitle {{t!("ui.windows.error").into_owned()}}
                    }
                    column(class = "wt-error") {
                        text(class = "wt-error__lead") {{t!("ui.labels.error_occurred").into_owned()}}
                        text(class = "wt-error__text", attr:data-testid = "notice-error-text") {{text.get_value()}}
                    }
                    DialogFooter {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
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
