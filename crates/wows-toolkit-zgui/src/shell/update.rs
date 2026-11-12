//! Looking for a newer release, offering it, and installing it.

use futures::StreamExt as _;
use rootcause::hooks::builtin_hooks::report_formatter::DefaultReportFormatter;
use rust_i18n::t;
use wows_toolkit_services::update::Downloaded;
use wows_toolkit_viewmodel::update::Release;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::task::blocking;
use zgui::task::spawn_detached;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogDescriptionProps;
use zgui_ui::DialogFooterProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;

use crate::desktop;
use crate::http::USER_AGENT;
use crate::services::Job;
use crate::services::Services;
use crate::shell::HostedDialogProps;
use crate::shell::Shell;
use crate::shell::notices;

const SHEET: &str = "wt-update";

const CSS: &str = css!(
    ".wt-update {
        display: flex;
        flex-direction: column;
        gap: 12px;
        width: 640px;
        max-width: 100%;
    }
    .wt-update__notes-head {
        color: var(--lui-color-text-dim);
        font-size: 11px;
        font-weight: 600;
    }
    .wt-update__notes {
        max-height: 420px;
        overflow-y: auto;
        padding: 12px;
        border: 1px solid var(--lui-color-border);
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-widget);
        white-space: pre-wrap;
    }
    .wt-update__hint { color: var(--lui-color-text-dim); }"
);

/// Who asked for the update check.
///
/// A startup check that finds nothing says nothing; one the reader asked for says so, because
/// silence would read as a check that never ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    AtStartup,
    ByHand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateStatus {
    Idle,
    Checking(Asked),
    Installing,
}

/// Where the update check and install are.
#[derive(Clone, Copy)]
pub struct UpdateFlow {
    status: RwSignal<UpdateStatus, LocalStorage>,
}

impl UpdateFlow {
    pub fn new() -> Self {
        Self { status: RwSignal::new_local(UpdateStatus::Idle) }
    }
}

impl Default for UpdateFlow {
    fn default() -> Self {
        Self::new()
    }
}

/// The proxy the settings name. Empty is no proxy, as the stored setting represents it, and is
/// also what a check made before the settings are read goes through.
fn proxy_url(services: Services) -> String {
    services.settings.read_untracked(|settings| settings.proxy_url.clone()).unwrap_or_default()
}

/// How far the download has got, in the reader's own terms.
fn describe_download(step: Downloaded) -> String {
    let read = humansize::format_size(step.read, humansize::BINARY);
    match step.total {
        Some(total) => {
            let total = humansize::format_size(total, humansize::BINARY);
            t!("ui.messages.update_downloading", read = read, total = total).into_owned()
        }
        // A server that did not say how much there is leaves only how much has arrived.
        None => t!("ui.messages.update_downloading_unknown", read = read).into_owned(),
    }
}

/// Looks for a newer release, and offers to install one.
pub fn check_for_update(services: Services, shell: Shell, asked: Asked) {
    let flow = shell.update;
    match flow.status.get_untracked() {
        UpdateStatus::Installing => return,
        // A check by hand during the startup one takes that one over, so its answer is said.
        UpdateStatus::Checking(_) => {
            if asked == Asked::ByHand {
                flow.status.set(UpdateStatus::Checking(Asked::ByHand));
            }
            return;
        }
        UpdateStatus::Idle => flow.status.set(UpdateStatus::Checking(asked)),
    }

    let job = (asked == Asked::ByHand).then(|| {
        services.jobs.start(Job { name: t!("ui.messages.checking_app_updates").into_owned(), progress: None })
    });
    let proxy = proxy_url(services);
    let found = services.on_tokio(async move {
        wows_toolkit_services::update::check(USER_AGENT, &proxy, env!("CARGO_PKG_VERSION")).await
    });

    // Detached: the menu that asked closes at once, and the answer still has to be said.
    spawn_detached(async move {
        let found = found.await;
        drop(job);
        let UpdateStatus::Checking(asked) = flow.status.get_untracked() else { return };
        flow.status.set(UpdateStatus::Idle);

        let found = match found {
            Ok(found) => found.map_err(|error| format!("{}", error.format_with(&DefaultReportFormatter::ASCII))),
            Err(error) => Err(error.to_string()),
        };
        match found {
            Ok(Some(release)) => offer_update(services, shell, release),
            Ok(None) if asked == Asked::ByHand => {
                shell.toasts().ok(t!("ui.messages.app_up_to_date").into_owned());
            }
            Ok(None) => {}
            Err(reason) => {
                tracing::warn!("Update check failed: {reason}");
                if asked == Asked::ByHand {
                    notices::show_error(shell, format!("{}\n\n{reason}", t!("ui.messages.update_check_failed")));
                }
            }
        }
    });
}

/// Offers `release`, and installs it if the reader says so.
fn offer_update(services: Services, shell: Shell, release: Release) {
    let asset_url = release.asset_for_windows().map(|asset| asset.browser_download_url.clone());
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let release_url = StoredValue::new(release.html_url.clone());
        let install_url = StoredValue::new(asset_url.clone());
        let can_install = cfg!(target_os = "windows") && asset_url.is_some();
        let description = StoredValue::new(t!("ui.dialogs.update_message", tag = release.tag_name).into_owned());
        let notes = StoredValue::new(release.body.clone().filter(|notes| !notes.trim().is_empty()));
        let hint = StoredValue::new(if !cfg!(target_os = "windows") {
            Some(t!("ui.dialogs.update_windows_only").into_owned())
        } else if asset_url.is_none() {
            Some(t!("ui.dialogs.update_asset_pending").into_owned())
        } else {
            None
        });

        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(attr:data-testid = "update-offer") {
                    DialogHeader {
                        DialogTitle {{t!("ui.windows.update_available").into_owned()}}
                        DialogDescription {{description.get_value()}}
                    }
                    column(class = "wt-update") {
                        {notes.get_value().map(|notes| view! {
                            column {
                                text(class = "wt-update__notes-head") {{t!("ui.dialogs.update_notes").into_owned()}}
                                text(class = "wt-update__notes", attr:data-testid = "update-notes") {{notes}}
                            }
                        }.into_view())}
                        {hint.get_value().map(|hint| view! { text(class = "wt-update__hint") {{hint}} }.into_view())}
                    }
                    DialogFooter {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            attr:data-testid = "update-view-release",
                            on:click = move |_| desktop::open_url(&release_url.get_value())
                        ) {
                            {t!("ui.buttons.view_release").into_owned()}
                        }
                        {can_install.then(|| {
                            view! {
                                Button(
                                    size = ButtonSize::Sm,
                                    attr:data-testid = "update-install",
                                    on:click = move |_| {
                                        let Some(url) = install_url.get_value() else { return };
                                        handle.close();
                                        install_update(services, shell, url);
                                    }
                                ) {
                                    {t!("ui.buttons.install_update").into_owned()}
                                }
                            }.into_view()
                        })}
                        Button(
                            variant = ButtonVariant::Ghost,
                            size = ButtonSize::Sm,
                            attr:data-testid = "update-close",
                            on:click = move |_| handle.close()
                        ) {
                            {t!("ui.buttons.cancel").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

/// Downloads and installs the release at `asset_url`, saying how far the download has got. A
/// successful install starts the new build and exits this process.
fn install_update(services: Services, shell: Shell, asset_url: String) {
    let flow = shell.update;
    if flow.status.get_untracked() != UpdateStatus::Idle {
        return;
    }
    flow.status.set(UpdateStatus::Installing);

    let job = services.jobs.start(Job { name: t!("ui.messages.update_starting").into_owned(), progress: None });
    let (reports, mut progress) = futures::channel::mpsc::unbounded::<Downloaded>();
    let proxy = proxy_url(services);
    let runtime = services.runtime();

    spawn_detached(async move {
        let installing = blocking(move || {
            let Some(runtime) = runtime else {
                return Err(wows_toolkit_services::update::UpdateError::NoRuntime.into());
            };
            wows_toolkit_services::update::install(&asset_url, USER_AGENT, &proxy, &reports, &runtime)
        });
        let listening = async {
            while let Some(step) = progress.next().await {
                job.report(Job { name: describe_download(step), progress: None });
            }
        };
        let (installed, ()) = futures::future::join(installing, listening).await;

        flow.status.set(UpdateStatus::Idle);
        if let Err(error) = installed {
            let reason = format!("{}", error.format_with(&DefaultReportFormatter::ASCII));
            tracing::warn!("Update installation failed: {reason}");
            notices::show_error(shell, reason);
        }
    });
}
