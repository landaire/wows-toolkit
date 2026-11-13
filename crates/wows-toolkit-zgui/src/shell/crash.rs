//! What the last run's crash left behind, and what a bug report is made of.

use rust_i18n::t;
use zgui::css;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogDescriptionProps;
use zgui_ui::DialogFooterProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;

use crate::shell::HostedDialogProps;
use crate::shell::Shell;

const SHEET: &str = "wt-crash";

/// The report is a backtrace dozens of lines long, so it scrolls inside a box of its own and the
/// dialog keeps a fixed height.
const CSS: &str = css!(
    ".wt-crash {
        display: flex;
        flex-direction: column;
        gap: 12px;
        width: 640px;
        max-width: 100%;
    }
    .wt-crash__report {
        max-height: 260px;
        overflow-y: auto;
        padding: 8px;
        border: 1px solid var(--lui-color-border);
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-widget);
        font-family: var(--lui-font-family-mono);
        font-size: 11px;
        white-space: pre-wrap;
        user-select: text;
    }"
);

/// Reports what the last run's crash left behind, once.
pub fn report_last_crash(shell: Shell) {
    let Some(report) = wows_toolkit_services::logging::take_crash_report() else { return };

    show_crash_report(shell, report);
}

/// The crash dialog, with a way to copy the report.
pub fn show_crash_report(shell: Shell, report: String) {
    shell.remember_crash(report.clone());
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let report = StoredValue::new(report.clone());

        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(attr:data-testid = "crash-report") {
                    DialogHeader {
                        DialogTitle {{t!("ui.windows.crash_detected").into_owned()}}
                        DialogDescription {{t!("ui.dialogs.crash_message").into_owned()}}
                    }
                    column(class = "wt-crash") {
                        text(class = "wt-crash__report", attr:data-testid = "crash-report-scroll") {{report.with_value(|report| report.trim().to_owned())}}
                    }
                    DialogFooter {
                        Button(
                            size = ButtonSize::Sm,
                            variant = ButtonVariant::Outline,
                            on:click = move |_| handle.close()
                        ) {
                            {t!("ui.buttons.cancel").into_owned()}
                        }
                        Button(
                            size = ButtonSize::Sm,
                            attr:data-testid = "crash-report-copy",
                            on:click = move |_| {
                                shell.copy_text(report.get_value());
                                shell.toasts().ok(t!("ui.stats.copied").into_owned());
                                handle.close();
                            }
                        ) {
                            {t!("ui.buttons.copy").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

/// Puts the small support summary on the clipboard.
pub fn copy_bug_report_info(shell: Shell) {
    let services = shell.services();
    // The saved commit arrives with the settings.
    let Some(constants_commit) = services.settings.read_untracked(|settings| settings.constants_commit.clone()) else {
        return;
    };

    let loaded_game_version = services
        .game_data
        .cache_untracked()
        .and_then(|cache| cache.latest_loaded_version())
        .map(wows_toolkit_viewmodel::bug_report::full_game_version);
    let storage_dir = wows_toolkit_config::storage_dir();
    let downloaded_constants = wows_toolkit_viewmodel::bug_report::latest_downloaded_constants(storage_dir.as_deref());
    let report = wows_toolkit_viewmodel::bug_report::BugReportInfo {
        constants_fetch: wows_toolkit_viewmodel::bug_report::constants_fetch_status(),
        application_version: env!("CARGO_PKG_VERSION").to_owned(),
        loaded_game_version,
        downloaded_constants,
        constants_commit,
        recent_crash: shell.recent_crash(),
    }
    .discord_code_block();

    shell.copy_text(report);
    shell.toasts().ok(t!("ui.messages.bug_report_info_copied").into_owned());
}

/// Puts the newest log file on the clipboard, for a bug report.
pub fn copy_latest_log(shell: Shell) {
    let Some(path) = wows_toolkit_services::logging::newest_log() else {
        shell.toasts().warn(t!("ui.messages.log_not_found").into_owned());
        return;
    };

    match std::fs::read_to_string(&path) {
        Ok(contents) => {
            let bytes = contents.len() / 1024;
            let named = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            shell.copy_text(contents);
            shell.toasts().ok(t!("ui.messages.log_copied", bytes = bytes, file = named).into_owned());
        }
        Err(err) => {
            shell.toasts().failed(t!("ui.messages.log_copy_failed", error = err).into_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use zgui::reactive::prelude::GetUntracked;

    use super::show_crash_report;
    use crate::interaction_tests::REVIEW_SIZE;
    use crate::interaction_tests::open_bare_shell;
    use crate::interaction_tests::shell;

    /// The report box's `max-height`.
    const REPORT_HEIGHT: f32 = 260.0;

    #[test]
    fn a_long_crash_report_scrolls_inside_the_dialog() {
        let mut window = open_bare_shell();
        let report: String = (0..200).map(|line| format!("  {line}: frame::in::the::backtrace\n")).collect();

        show_crash_report(shell(), report);
        window.advance(Duration::from_millis(400));

        let details = window.by_test_id("crash-report-scroll").expect("the report is shown");
        let details = window.bounds(details).expect("the report has a box");
        let zoom = shell().appearance.zoom.get_untracked().get();
        assert!(
            details.size.height.0 <= REPORT_HEIGHT * zoom + 0.5,
            "the report box is held to its height, got {details:?}"
        );
        let bottom = details.origin.y.0 + details.size.height.0;
        assert!(bottom < REVIEW_SIZE.height, "and the dialog ends inside the window");
    }
}
