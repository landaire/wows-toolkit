//! A download's progress and its controls, or how it ended.

use std::path::PathBuf;

use rust_i18n::t;
use zgui::component;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::ProgressProps;
use zgui_ui::SpinnerProps;

use crate::services::Services;
use crate::shell::Shell;
use crate::tabs::launcher::display::DownloadStage;
use crate::tabs::launcher::display::DownloadTarget;
use crate::tabs::launcher::display::RunningDownload;
use crate::tabs::launcher::model::LauncherModel;

/// The status line of `target`'s download. Shows nothing until one is started.
#[component]
pub fn DownloadStatus(model: LauncherModel, shell: Shell, target: DownloadTarget) -> impl IntoView {
    let stage = Memo::new(move |_| {
        model.with(|state| {
            target.find(&state.launcher).map(|(key, state)| (key.to_path_buf(), DownloadStage::of(state)))
        })
    });

    view! {
        {move || stage.get().map(|(key, stage)| stage_view(model, shell, target, key, stage))}
    }
}

fn stage_view(
    model: LauncherModel,
    shell: Shell,
    target: DownloadTarget,
    key: PathBuf,
    stage: DownloadStage,
) -> AnyView {
    let locale = Services::current().locale;
    let slot = target.slot();

    match stage {
        DownloadStage::Running => AnyView::new(view! { Running(model = model, target = target, key = key) }),
        DownloadStage::Paused => AnyView::new(view! {
            row(class = "wt-launcher__row") {
                text(class = "wt-launcher__dim") {{locale.text("ui.launcher.paused")}}
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = format!("launcher-resume-{slot}"),
                    on:click = move |_| {
                        let key = key.clone();
                        let resumed = model.run(move |launcher, _, env| (launcher.resume_download(&key, env), false));
                        if let Some(Err(err)) = resumed {
                            shell.toasts().failed(err.to_string());
                        }
                    }
                ) {
                    {locale.text("ui.launcher.resume")}
                }
            }
        }),
        DownloadStage::Installed { skipped_deltas: 0 } => AnyView::new(view! {
            text(class = "wt-launcher__ok") {{locale.text("ui.launcher.installed")}}
        }),
        DownloadStage::Installed { skipped_deltas } => AnyView::new(view! {
            text(class = "wt-launcher__warn") {{move || {
                locale.track();
                t!("ui.launcher.installed_skipped", count = skipped_deltas).into_owned()
            }}}
        }),
        DownloadStage::Failed(detail) => AnyView::new(view! {
            text(class = "wt-launcher__error") {{move || {
                locale.track();
                t!("ui.launcher.download_failed", error = detail.as_str()).into_owned()
            }}}
        }),
        DownloadStage::Cancelled => AnyView::new(view! {
            text(class = "wt-launcher__dim") {{locale.text("ui.launcher.cancelled")}}
        }),
    }
}

/// A running download: a bar of the share fetched once the size is known, pause and cancel, and
/// the phase and file it is on.
#[component]
fn Running(model: LauncherModel, target: DownloadTarget, key: PathBuf) -> impl IntoView {
    let locale = Services::current().locale;
    let slot = target.slot();
    let running = Memo::new(move |_| {
        model.with(|state| target.find(&state.launcher).and_then(|(_, state)| RunningDownload::of(state)))
    });
    let transferred = move || running.get().and_then(|running| running.transferred);
    let percent = Signal::derive_local(move || transferred().map(|transferred| transferred.percent()));
    let line = move || running.get().map(|running| format!("{}: {}", running.phase, running.current));
    let pause_key = key.clone();
    let progress_id = format!("launcher-progress-{slot}");

    view! {
        column(class = "wt-launcher__column") {
            row(class = "wt-launcher__row") {
                if move || transferred().is_some() {
                    row(class = "wt-launcher__row") {
                        box(class = "wt-launcher__meter") {
                            Progress(value = percent, attr:data-testid = progress_id.clone())
                        }
                        text(class = "wt-launcher__text wt-launcher__nowrap") {{move || transferred().map(|transferred| transferred.said())}}
                    }
                } else {
                    Spinner()
                }
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::Sm,
                    attr:data-testid = format!("launcher-pause-{slot}"),
                    on:click = move |_| model.pause_download(&pause_key)
                ) {
                    {locale.text("ui.launcher.pause")}
                }
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::Sm,
                    attr:data-testid = format!("launcher-cancel-{slot}"),
                    on:click = move |_| model.cancel_download(&key)
                ) {
                    {locale.text("ui.launcher.cancel")}
                }
            }
            text(class = "wt-launcher__text wt-launcher__truncate") {{line}}
        }
    }
}
