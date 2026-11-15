//! The notice the user accepts before the launcher turns on.

use rust_i18n::t;
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

use crate::launcher::CSS;
use crate::launcher::SHEET;
use crate::launcher::model::LauncherModel;
use crate::shell::HostedDialogProps;
use crate::shell::Shell;

/// Shows the notice. Continue turns the launcher on and saves that; Cancel, or closing the
/// dialog, leaves it off.
pub fn ask_to_enable(shell: Shell, model: LauncherModel) {
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(attr:data-testid = "launcher-opt-in") {
                    DialogHeader {
                        DialogTitle {{t!("ui.launcher.opt_in.title").into_owned()}}
                    }
                    column(class = "wt-launcher__column wt-launcher__column--wide wt-launcher__notice") {
                        text(class = "wt-launcher__notice-account", attr:data-testid = "launcher-opt-in-account") {{t!("ui.launcher.opt_in.notice_account").into_owned()}}
                        text(attr:data-testid = "launcher-opt-in-reports") {{t!("ui.launcher.opt_in.notice_reports").into_owned()}}
                    }
                    DialogFooter {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            attr:data-testid = "launcher-opt-in-cancel",
                            on:click = move |_| handle.close()
                        ) {
                            {t!("ui.launcher.opt_in.cancel").into_owned()}
                        }
                        Button(
                            size = ButtonSize::Sm,
                            attr:data-testid = "launcher-opt-in-continue",
                            on:click = move |_| {
                                model.enable();
                                handle.close();
                            }
                        ) {
                            {t!("ui.launcher.opt_in.continue").into_owned()}
                        }
                    }
                }
            }
        })
    });
}
