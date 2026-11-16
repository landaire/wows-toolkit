//! What the last run's crash left behind, and what a bug report is made of.

use rust_i18n::t;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::NodeRef;
use zgui::view::install_stylesheet;
use zgui_ui::AlertDialogActionProps;
use zgui_ui::AlertDialogCancelProps;
use zgui_ui::AlertDialogContentProps;
use zgui_ui::AlertDialogDescriptionProps;
use zgui_ui::AlertDialogFooterProps;
use zgui_ui::AlertDialogHeaderProps;
use zgui_ui::AlertDialogProps;
use zgui_ui::AlertDialogTitleProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogDescriptionProps;
use zgui_ui::DialogFooterProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;

use crate::desktop;
use crate::shell::HostedDialogProps;
use crate::shell::Shell;
use crate::shell::dialogs::focus_when_bound;
use crate::shell::menu::DISCORD_URL;
use crate::shell::menu::ISSUES_URL;

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
    }
    .wt-crash__links {
        display: flex;
        gap: 8px;
    }
    .wt-crash__clear { margin-left: auto; }"
);

/// Reports what the last run's crash left behind, once.
pub fn report_last_crash(shell: Shell) {
    let Some(report) = wows_toolkit_services::logging::take_crash_report() else { return };

    show_crash_report(shell, report);
}

/// The crash dialog, with a way to copy the report, the places to report it, and a way to clear the
/// settings in case they are what crashed it.
pub fn show_crash_report(shell: Shell, report: String) {
    shell.remember_crash(report.clone());
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let report = StoredValue::new(report.clone());
        let copy = NodeRef::new();
        focus_when_bound(copy);

        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(dismiss_on_outside_press = false, attr:data-testid = "crash-report") {
                    DialogHeader {
                        DialogTitle {{t!("ui.windows.crash_detected").into_owned()}}
                        DialogDescription {{t!("ui.dialogs.crash_message").into_owned()}}
                    }
                    column(class = "wt-crash") {
                        text(class = "wt-crash__report", attr:data-testid = "crash-report-scroll") {{report.with_value(|report| report.trim().to_owned())}}
                        row(class = "wt-crash__links") {
                            Button(
                                size = ButtonSize::Sm,
                                variant = ButtonVariant::Outline,
                                attr:data-testid = "crash-report-github",
                                on:click = move |_| desktop::open_url(ISSUES_URL)
                            ) {
                                {t!("ui.buttons.github").into_owned()}
                            }
                            Button(
                                size = ButtonSize::Sm,
                                variant = ButtonVariant::Outline,
                                attr:data-testid = "crash-report-discord",
                                on:click = move |_| desktop::open_url(DISCORD_URL)
                            ) {
                                {t!("ui.buttons.discord").into_owned()}
                            }
                            Button(
                                size = ButtonSize::Sm,
                                variant = ButtonVariant::Destructive,
                                class = "wt-crash__clear",
                                attr:data-testid = "crash-report-clear-settings",
                                on:click = move |_| confirm_clear_settings(shell)
                            ) {
                                {t!("ui.buttons.clear_settings").into_owned()}
                            }
                        }
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
                            node_ref = copy,
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

/// Asks before clearing the settings, which cannot be taken back.
fn confirm_clear_settings(shell: Shell) {
    shell.dialogs.open(move |handle| {
        let open = RwSignal::new_local(true);

        AnyView::new(view! {
            AlertDialog(open = open, on_open_change = handle.on_open_change()) {
                AlertDialogContent(attr:data-testid = "crash-clear-settings-confirm") {
                    AlertDialogHeader {
                        AlertDialogTitle {{t!("ui.buttons.clear_settings").into_owned()}}
                        AlertDialogDescription {{t!("ui.dialogs.crash_clear_settings").into_owned()}}
                    }
                    AlertDialogFooter {
                        AlertDialogCancel {{t!("ui.buttons.cancel").into_owned()}}
                        AlertDialogAction(
                            variant = ButtonVariant::Destructive,
                            attr:data-testid = "crash-clear-settings-ok",
                            on:click = move |_| shell.services().settings.clear()
                        ) {
                            {t!("ui.buttons.clear_settings").into_owned()}
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

    use landaire_ui::Appearance;
    use landaire_ui::test_support::HeadlessWindow;
    use zgui::geom::DevicePx;
    use zgui::geom::Point;
    use zgui::reactive::prelude::GetUntracked;
    use zgui::vocab::Modifiers;
    use zgui::vocab::NamedKey;

    use super::show_crash_report;
    use crate::desktop::opened_for_test;
    use crate::interaction_tests::REVIEW_SIZE;
    use crate::interaction_tests::open_bare_shell;
    use crate::interaction_tests::open_shell;
    use crate::interaction_tests::review_options;
    use crate::interaction_tests::save_shot;
    use crate::interaction_tests::shell;
    use crate::services::CLEARED;
    use crate::services::saved_for_test;
    use crate::shell::menu::DISCORD_URL;
    use crate::shell::menu::ISSUES_URL;

    fn settle(window: &mut HeadlessWindow) {
        window.advance(Duration::from_millis(400));
    }

    fn open_crash_report() -> HeadlessWindow {
        let mut window = open_bare_shell();
        show_crash_report(shell(), "thread 'main' panicked at src/main.rs:1:1".to_owned());
        settle(&mut window);

        window
    }

    fn press(window: &mut HeadlessWindow, id: &str) {
        window.click(window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"))).expect("the button is pressed");
        settle(window);
    }

    fn cleared() -> bool {
        saved_for_test().iter().any(|(key, _)| *key == CLEARED)
    }

    #[test]
    fn a_press_past_the_crash_report_leaves_it_open() {
        let mut window = open_crash_report();

        window.click_at(Point::new(DevicePx(4.0), DevicePx(4.0)));
        settle(&mut window);

        assert!(window.by_test_id("crash-report").is_some());
    }

    #[test]
    fn enter_copies_the_crash_report() {
        let mut window = open_crash_report();

        window.press_key(NamedKey::Enter, Modifiers::NONE);
        settle(&mut window);

        assert!(window.by_test_id("crash-report").is_none(), "copying closes the report");
        let toasts = window.texts_by_class("zui-toast").concat();
        assert!(toasts.contains(rust_i18n::t!("ui.stats.copied").as_ref()), "{toasts}");
    }

    #[test]
    fn the_crash_report_links_to_github_and_discord() {
        let mut window = open_crash_report();

        press(&mut window, "crash-report-github");
        press(&mut window, "crash-report-discord");

        assert_eq!(opened_for_test(), [ISSUES_URL, DISCORD_URL]);
    }

    #[test]
    fn clearing_the_settings_from_the_crash_report_asks_first() {
        let mut window = open_crash_report();

        press(&mut window, "crash-report-clear-settings");
        assert!(window.by_test_id("crash-clear-settings-confirm").is_some(), "the clear is confirmed first");
        assert!(!cleared(), "nothing is cleared before the answer");

        press(&mut window, "crash-clear-settings-ok");
        assert!(cleared());
        assert!(window.by_test_id("crash-clear-settings-confirm").is_none());
    }

    /// Draws the crash report over the shell, in Dark and Light.
    #[test]
    #[ignore = "draws on the graphics device; run by hand to review the crash report"]
    fn the_crash_report_renders_in_dark_and_light() {
        for (appearance, name) in [(Appearance::Dark, "dark"), (Appearance::Light, "light")] {
            let mut window = open_shell(review_options(appearance));
            show_crash_report(shell(), "thread 'main' panicked at src/main.rs:1:1".to_owned());
            settle(&mut window);

            println!("{}", save_shot(&mut window, &format!("crash-report-{name}")).display());
        }
    }

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
