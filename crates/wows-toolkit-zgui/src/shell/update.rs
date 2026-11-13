//! Looking for a newer release, offering it, and installing it.

use std::rc::Rc;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedSender;
use futures::future::LocalBoxFuture;
use landaire_ui::MarkdownProps;
use rootcause::hooks::builtin_hooks::report_formatter::DefaultReportFormatter;
use rust_i18n::t;
use wows_toolkit_services::update::Downloaded;
use wows_toolkit_viewmodel::update::Release;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
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
use zgui_ui::DialogFooterProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;

use crate::desktop;
use crate::http::USER_AGENT;
use crate::services::Job;
use crate::services::JobHandle;
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
        width: 680px;
        max-width: 100%;
    }
    .wt-update__version { user-select: text; }
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
    }
    .wt-update__hint { color: var(--lui-color-text-dim); }
    .wt-update__footer {
        padding-top: 12px;
        border-top: 1px solid var(--lui-color-border);
    }"
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

/// Where releases are looked up and installed from.
pub trait ReleaseSource {
    /// The newest release, when it is newer than this build.
    fn check(&self, services: Services) -> LocalBoxFuture<'static, Result<Option<Release>, rootcause::Report>>;

    /// Downloads the release archive at `asset_url` and installs it, sending how far the download
    /// has got to `progress`. A successful install starts the new build and exits this process, so
    /// this answers only with a failure.
    fn install(
        &self,
        services: Services,
        asset_url: String,
        progress: UnboundedSender<Downloaded>,
    ) -> LocalBoxFuture<'static, Result<(), rootcause::Report>>;
}

/// The releases published on GitHub.
struct PublishedReleases;

impl ReleaseSource for PublishedReleases {
    fn check(&self, services: Services) -> LocalBoxFuture<'static, Result<Option<Release>, rootcause::Report>> {
        let proxy = proxy_url(services);
        let found = services.on_tokio(async move {
            wows_toolkit_services::update::check(USER_AGENT, &proxy, env!("CARGO_PKG_VERSION")).await
        });
        Box::pin(async move { found.await? })
    }

    fn install(
        &self,
        services: Services,
        asset_url: String,
        progress: UnboundedSender<Downloaded>,
    ) -> LocalBoxFuture<'static, Result<(), rootcause::Report>> {
        let proxy = proxy_url(services);
        let runtime = services.runtime();
        Box::pin(blocking(move || {
            let Some(runtime) = runtime else {
                return Err(wows_toolkit_services::update::UpdateError::NoRuntime.into());
            };
            wows_toolkit_services::update::install(&asset_url, USER_AGENT, &proxy, &progress, &runtime)
        }))
    }
}

/// Where the update check and install are.
#[derive(Clone, Copy)]
pub struct UpdateFlow {
    status: RwSignal<UpdateStatus, LocalStorage>,
    /// The status bar's line for a check the reader asked for, held until the check answers.
    asked_job: StoredValue<Option<JobHandle>, LocalStorage>,
    source: StoredValue<Rc<dyn ReleaseSource>, LocalStorage>,
}

impl UpdateFlow {
    pub fn new() -> Self {
        Self {
            status: RwSignal::new_local(UpdateStatus::Idle),
            asked_job: StoredValue::new_local(None),
            source: StoredValue::new_local(Rc::new(PublishedReleases)),
        }
    }

    /// Looks releases up in `source` from now on.
    #[cfg(test)]
    pub fn use_source(self, source: Rc<dyn ReleaseSource>) {
        self.source.set_value(source);
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

fn describe_failure(error: &rootcause::Report) -> String {
    format!("{}", error.format_with(&DefaultReportFormatter::ASCII))
}

/// Looks for a newer release, and offers to install one.
pub fn check_for_update(services: Services, shell: Shell, asked: Asked) {
    let flow = shell.update;
    let status = flow.status.get_untracked();
    if status == UpdateStatus::Installing {
        return;
    }
    // Shown for a check by hand even when it joins the startup one, so the click is seen to work.
    if asked == Asked::ByHand && flow.asked_job.with_value(Option::is_none) {
        let job =
            services.jobs.start(Job { name: t!("ui.messages.checking_app_updates").into_owned(), progress: None });
        flow.asked_job.set_value(Some(job));
    }
    // A check by hand during the startup one takes that one over, so its answer is said.
    if let UpdateStatus::Checking(_) = status {
        if asked == Asked::ByHand {
            flow.status.set(UpdateStatus::Checking(Asked::ByHand));
        }
        return;
    }
    flow.status.set(UpdateStatus::Checking(asked));

    let found = flow.source.get_value().check(services);
    // Detached: the menu that asked closes at once, and the answer still has to be said.
    spawn_detached(async move {
        let found = found.await;
        // Read with `try_`, as the shell may be gone by the time the check answers.
        let Some(UpdateStatus::Checking(asked)) = flow.status.try_get_untracked() else { return };
        flow.status.set(UpdateStatus::Idle);
        drop(flow.asked_job.try_update_value(Option::take).flatten());

        match found {
            Ok(Some(release)) => offer_update(services, shell, release),
            Ok(None) if asked == Asked::ByHand => {
                shell.toasts().ok(t!("ui.messages.app_up_to_date").into_owned());
            }
            Ok(None) => {}
            Err(error) => {
                let reason = describe_failure(&error);
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
        let version = StoredValue::new(t!("ui.dialogs.update_message", tag = release.tag_name).into_owned());
        let notes = StoredValue::new(release.body.clone().filter(|notes| !notes.trim().is_empty()));
        let hint = StoredValue::new(if !cfg!(target_os = "windows") {
            Some(t!("ui.dialogs.update_windows_only").into_owned())
        } else if asset_url.is_none() {
            Some(t!("ui.dialogs.update_asset_pending").into_owned())
        } else {
            None
        });
        let open_link = UnsyncCallback::new(|url: String| desktop::open_url(&url));

        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(attr:data-testid = "update-offer") {
                    DialogHeader {
                        DialogTitle {{t!("ui.windows.update_available").into_owned()}}
                    }
                    column(class = "wt-update") {
                        text(class = "wt-update__version", attr:data-testid = "update-version") {{version.get_value()}}
                        // The notes are the release's own text, so they sit in a frame of their
                        // own: where they start and stop is the frame, not a guess from headings.
                        {notes.get_value().map(|notes| view! {
                            column {
                                text(class = "wt-update__notes-head") {{t!("ui.dialogs.update_notes").into_owned()}}
                                box(class = "wt-update__notes") {
                                    Markdown(source = notes, on_link = open_link, attr:data-testid = "update-notes")
                                }
                            }
                        }.into_view())}
                        {hint.get_value().map(|hint| view! { text(class = "wt-update__hint") {{hint}} }.into_view())}
                    }
                    // A rule over the buttons, so they read as the dialog's own actions rather
                    // than the end of the notes above.
                    DialogFooter(class = "wt-update__footer") {
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
    let installing = flow.source.get_value().install(services, asset_url, reports);

    spawn_detached(async move {
        let listening = async {
            while let Some(step) = progress.next().await {
                job.report(Job { name: describe_download(step), progress: None });
            }
        };
        let (installed, ()) = futures::future::join(installing, listening).await;
        drop(job);

        // `try_`, as the shell may be gone by the time the install answers.
        if flow.status.try_update(|status| *status = UpdateStatus::Idle).is_none() {
            return;
        }
        if let Err(error) = installed {
            let reason = describe_failure(&error);
            tracing::warn!("Update installation failed: {reason}");
            notices::show_error(shell, reason);
        }
    });
}

#[cfg(test)]
mod tests;
