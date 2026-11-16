//! The installs section: the installs found and added, each with its update check, its download,
//! and the ways to launch it or use it for the rest of the app.

use std::path::PathBuf;

use rust_i18n::t;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::LabelProps;
use zgui_ui::SpinnerProps;

use crate::services::Services;
use crate::shell::Shell;
use crate::tabs::launcher::SectionProps;
use crate::tabs::launcher::display::CatalogView;
use crate::tabs::launcher::display::DownloadTarget;
use crate::tabs::launcher::display::InstallVersion;
use crate::tabs::launcher::display::InstallView;
use crate::tabs::launcher::download::DownloadStatusProps;
use crate::tabs::launcher::model::LauncherModel;
use crate::tabs::launcher::options::autodetect_proton;

#[component]
pub fn InstallsSection(model: LauncherModel, shell: Shell) -> impl IntoView {
    let locale = Services::current().locale;
    // Applies to the next launch only.
    let safe_mode = RwSignal::new_local(Checked::No);
    let safe_mode_box = NodeRef::new();
    let safe_mode_name = NodeRef::new();
    let installs = Memo::new(move |_| model.with(|state| InstallView::all(&state.launcher)));
    let discovering = Memo::new(move |_| model.with(|state| state.launcher.is_discovering()));

    view! {
        Section(heading = "ui.launcher.installs_heading") {
            row(class = "wt-launcher__row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "launcher-rescan",
                    on:click = move |_| model.op(|launcher, settings, env| {
                        launcher.start_install_discovery(settings, env);
                        false
                    })
                ) {
                    {locale.text("ui.launcher.rescan")}
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "launcher-add-install",
                    on:click = move |_| add_install(model, shell)
                ) {
                    {locale.text("ui.launcher.add_install")}
                }
                if move || discovering.get() {
                    Spinner(attr:data-testid = "launcher-discovering")
                }
                box(class = "wt-launcher__spacer")
                row(class = "wt-launcher__row wt-launcher__row--tight") {
                    Checkbox(checked = safe_mode, node_ref = safe_mode_box, labelled_by = safe_mode_name, attr:data-testid = "launcher-safe-mode")
                    Label(
                        node_ref = safe_mode_name,
                        control = safe_mode_box,
                        attr:data-testid = "launcher-safe-mode-label",
                        on:click = move |_| safe_mode.set(Checked::from(!safe_mode.get_untracked().is_checked()))
                    ) {
                        {locale.text("ui.launcher.safe_mode")}
                    }
                }
            }
            if move || installs.get().is_empty() && !discovering.get() {
                text(class = "wt-launcher__dim", attr:data-testid = "launcher-no-installs") {{locale.text("ui.launcher.no_installs")}}
            }
            for install in move || installs.get(), key = |install: &InstallView| install.clone() {
                InstallRow(model = model, shell = shell, install = install, safe_mode = safe_mode)
            }
        }
    }
}

#[component]
fn InstallRow(
    model: LauncherModel,
    shell: Shell,
    install: InstallView,
    safe_mode: RwSignal<Checked, LocalStorage>,
) -> impl IntoView {
    let locale = Services::current().locale;
    let InstallView { slot, path, source, read_only, version, has_content } = install;
    let target = DownloadTarget::Install(slot);
    let catalog_path = path.clone();
    let catalog = Memo::new(move |_| model.with(|state| CatalogView::of(state.launcher.catalog_status(&catalog_path))));
    let offer_path = path.clone();
    let offer_download = Memo::new(move |_| {
        model.with(|state| {
            let active = state.launcher.download_state(&offer_path).is_some_and(|state| state.is_active());
            state.launcher.has_downloads(&offer_path) && !active
        })
    });
    let proton_path = path.clone();
    let needs_proton = Memo::new(move |_| {
        model.with(|state| {
            let Some(settings) = &state.settings else { return false };
            let Some(install) = state.launcher.installs().iter().find(|install| install.path == proton_path) else {
                return false;
            };
            wows_toolkit_launcher::proton::needs_setup(install, settings)
        })
    });
    let version = version.map(|version| match version {
        InstallVersion::Known(version) => AnyView::new(view! { text {{version}} }),
        InstallVersion::ManagedBySteam => AnyView::new(view! { text {{locale.text("ui.launcher.managed_by_steam")}} }),
    });
    let shown_path = path.display().to_string();
    let path = StoredValue::new(path);

    view! {
        column(class = "wt-launcher__card", attr:data-testid = format!("launcher-install-{slot}")) {
            row(class = "wt-launcher__row") {
                text(class = "wt-launcher__strong") {{source}}
                text(class = "wt-launcher__mono wt-launcher__truncate") {{shown_path}}
                {version}
            }
            if move || !read_only {
                row(class = "wt-launcher__row") {
                    CatalogStatus(catalog = catalog)
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::Sm,
                        attr:data-testid = format!("launcher-recheck-{slot}"),
                        on:click = move |_| {
                            let path = path.get_value();
                            model.op(move |launcher, _, env| {
                                launcher.recheck_catalog(&path, env);
                                false
                            });
                        }
                    ) {
                        {locale.text("ui.launcher.recheck")}
                    }
                }
                DownloadStatus(model = model, shell = shell, target = target)
                if move || offer_download.get() {
                    row(class = "wt-launcher__row") {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            attr:data-testid = format!("launcher-download-{slot}"),
                            on:click = move |_| download(model, shell, path.get_value(), has_content)
                        ) {
                            {locale.text("ui.launcher.download")}
                        }
                        text(class = "wt-launcher__dim") {{locale.text("ui.launcher.download_hint")}}
                    }
                }
            }
            if move || needs_proton.get() {
                row(class = "wt-launcher__row") {
                    text(class = "wt-launcher__warn") {{locale.text("ui.launcher.proton.needed")}}
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        attr:data-testid = format!("launcher-set-up-proton-{slot}"),
                        on:click = move |_| autodetect_proton(model, shell)
                    ) {
                        {locale.text("ui.launcher.proton.set_up")}
                    }
                }
            }
            row(class = "wt-launcher__row") {
                Button(
                    size = ButtonSize::Sm,
                    attr:data-testid = format!("launcher-launch-{slot}"),
                    on:click = move |_| launch(model, shell, &path.get_value(), safe_mode.get_untracked() == Checked::Yes)
                ) {
                    {locale.text("ui.launcher.launch")}
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = format!("launcher-use-{slot}"),
                    on:click = move |_| shell.apply_wows_dir(path.get_value().to_string_lossy().into_owned())
                ) {
                    {locale.text("ui.launcher.use_for_toolkit")}
                }
            }
        }
    }
}

/// What an install's update check found.
#[component]
fn CatalogStatus(catalog: Memo<CatalogView>) -> impl IntoView {
    let locale = Services::current().locale;

    view! {
        {move || match catalog.get() {
            CatalogView::Checking => AnyView::new(view! {
                row(class = "wt-launcher__row wt-launcher__row--tight") {
                    Spinner()
                    text(class = "wt-launcher__dim") {{locale.text("ui.launcher.checking_updates")}}
                }
            }),
            CatalogView::Found { updates, not_installed } => {
                let headline = if updates.is_empty() {
                    AnyView::new(view! { text(class = "wt-launcher__text") {{locale.text("ui.launcher.up_to_date")}} })
                } else {
                    let parts = updates.join(", ");
                    AnyView::new(view! {
                        text(class = "wt-launcher__warn") {{move || {
                            locale.track();
                            t!("ui.launcher.update_available", parts = parts.as_str()).into_owned()
                        }}}
                    })
                };
                let missing = (!not_installed.is_empty()).then(|| {
                    let parts = not_installed.join(", ");
                    view! {
                        text(class = "wt-launcher__dim") {{move || {
                            locale.track();
                            t!("ui.launcher.not_installed", parts = parts.as_str()).into_owned()
                        }}}
                    }
                });
                AnyView::new(view! {
                    column(class = "wt-launcher__column", attr:data-testid = "launcher-catalog") {
                        {headline}
                        {missing}
                    }
                })
            }
            CatalogView::Failed(detail) => AnyView::new(view! {
                text(class = "wt-launcher__dim") {{move || {
                    locale.track();
                    t!("ui.launcher.check_failed", error = detail.as_str()).into_owned()
                }}}
            }),
            CatalogView::Nothing => AnyView::new(view! { box() }),
        }}
    }
}

/// Asks for an install directory, adds it, and looks for installs again.
fn add_install(model: LauncherModel, shell: Shell) {
    let asked = wows_toolkit_services::dialog::pick_folder(&t!("ui.launcher.add_install_title"));
    spawn_local(async move {
        let Some(dir) = shell.chosen_path(asked).await else { return };
        model.op(move |launcher, settings, env| {
            settings.manual_installs.push(dir);
            launcher.start_install_discovery(settings, env);
            true
        });
    });
}

/// Asks where to put the newest build of the install at `install`, and downloads it there.
fn download(model: LauncherModel, shell: Shell, install: PathBuf, has_content: bool) {
    // An install with no detected version has no content on disk yet, so the picker starts at the
    // platform default.
    let start = if has_content { install.clone() } else { wgc_download::default_install_dir() };
    let asked = wows_toolkit_services::dialog::pick_folder_in(&t!("ui.launcher.download_title"), &start);
    spawn_local(async move {
        let Some(dest) = shell.chosen_path(asked).await else { return };
        let started = model.run(move |launcher, _, env| (launcher.start_download(install, dest, env), false));
        if let Some(Err(err)) = started {
            shell.toasts().failed(err.to_string());
        }
    });
}

/// Starts the install at `path` for online play.
fn launch(model: LauncherModel, shell: Shell, path: &std::path::Path, safe_mode: bool) {
    let Some(install) = model.install(path) else { return };
    let launched = model.with_untracked(|state| {
        let settings = state.settings.as_ref()?;
        Some(state.launcher.launch(&install, settings, safe_mode, |command| {
            wows_toolkit_services::child_process::prepare(command);
        }))
    });

    if let Some(Err(err)) = launched {
        shell.toasts().failed(t!("ui.launcher.launch_failed", error = err.to_string()).into_owned());
    }
}
