//! Who made this, what it is built on, and where to look next.

use rust_i18n::t;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui_ui::AlertDialogActionProps;
use zgui_ui::AlertDialogCancelProps;
use zgui_ui::AlertDialogContentProps;
use zgui_ui::AlertDialogDescriptionProps;
use zgui_ui::AlertDialogFooterProps;
use zgui_ui::AlertDialogHeaderProps;
use zgui_ui::AlertDialogProps;
use zgui_ui::AlertDialogTitleProps;

use crate::desktop;
use crate::shell::Shell;

/// What the PR figures are credited to.
const PR_INFO_URL: &str = "https://wows-numbers.com/personal/rating";
pub const PROJECT_URL: &str = "https://github.com/landaire/wows-toolkit";

/// The lines of the egui About window, with this build's version at the top: a bug report that
/// names a build is worth more than one that does not.
fn about_lines() -> Vec<String> {
    vec![
        format!("{} v{}", wows_toolkit_config::APP_NAME, env!("CARGO_PKG_VERSION")),
        t!("ui.labels.made_by").into_owned(),
        t!("ui.labels.credits").into_owned(),
        format!("{} {PR_INFO_URL}", t!("ui.labels.pr_credits")),
        PROJECT_URL.to_owned(),
    ]
}

/// Puts the About dialog up.
pub fn show_about(shell: Shell) {
    shell.dialogs.open(move |handle| {
        let open = RwSignal::new_local(true);
        let lines = StoredValue::new(about_lines());

        AnyView::new(view! {
            AlertDialog(open = open, on_open_change = handle.on_open_change()) {
                AlertDialogContent(attr:data-testid = "about-dialog") {
                    AlertDialogHeader {
                        AlertDialogTitle {{t!("ui.windows.about").into_owned()}}
                        for line in move || lines.get_value(), key = |line: &String| line.clone() {
                            AlertDialogDescription {{line}}
                        }
                    }
                    AlertDialogFooter {
                        AlertDialogCancel {{t!("ui.buttons.cancel").into_owned()}}
                        AlertDialogAction(on:click = move |_| desktop::open_url(PROJECT_URL)) {
                            {t!("ui.buttons.view_github").into_owned()}
                        }
                    }
                }
            }
        })
    });
}
