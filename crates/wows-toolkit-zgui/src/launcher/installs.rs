//! The installs row of the Settings drawer: the installs found and added, each with its update
//! check and download, which one the toolkit reads, and which one the default account plays.

use std::path::PathBuf;

use rust_i18n::t;
use wows_toolkit_launcher::InstallChoice;
use zgui::component;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::SpinnerProps;

use crate::launcher::display::CatalogView;
use crate::launcher::display::DownloadTarget;
use crate::launcher::display::InstallVersion;
use crate::launcher::display::InstallView;
use crate::launcher::download::DownloadStatusProps;
use crate::launcher::games::GamesListProps;
use crate::launcher::model::LauncherModel;
use crate::launcher::steam_login::WgcLoginControlProps;
use crate::services::Services;
use crate::shell::Shell;

#[component]
pub fn InstallsControl(model: LauncherModel, shell: Shell) -> impl IntoView {
    let locale = Services::current().locale;
    let installs = Memo::new(move |_| model.with(|state| InstallView::all(&state.launcher)));
    let discovering = Memo::new(move |_| model.with(|state| state.launcher.is_discovering()));

    view! {
        column(class = "wt-launcher__column wt-launcher__column--wide", attr:data-testid = "launcher-installs") {
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
            }
            if move || installs.get().is_empty() && !discovering.get() {
                text(class = "wt-launcher__dim", attr:data-testid = "launcher-no-installs") {{locale.text("ui.launcher.no_installs")}}
            }
            for install in move || installs.get(), key = |install: &InstallView| install.clone() {
                InstallRow(model = model, shell = shell, install = install)
            }
            GamesList(model = model, shell = shell)
        }
    }
}

#[component]
fn InstallRow(model: LauncherModel, shell: Shell, install: InstallView) -> impl IntoView {
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
    let toolkit_path = path.clone();
    let is_toolkit = Memo::new(move |_| model.wows_dir().is_some_and(|dir| dir == toolkit_path));
    let played_path = path.clone();
    let played = Memo::new(move |_| {
        let wows_dir = model.wows_dir();
        model
            .with(|state| {
                let settings = state.settings.as_ref()?;
                let account = settings.default_account()?;
                (account.install.resolve(wows_dir.as_deref()).as_ref() == Some(&played_path)).then_some(())
            })
            .is_some()
    });
    let has_default = Memo::new(move |_| {
        model.with(|state| state.settings.as_ref().is_some_and(|settings| settings.default_account().is_some()))
    });
    let version = version.map(|version| match version {
        InstallVersion::Known(version) => AnyView::new(view! { text {{version}} }),
        InstallVersion::ManagedBySteam => AnyView::new(view! { text {{locale.text("ui.launcher.managed_by_steam")}} }),
    });
    let shown_path = path.display().to_string();
    let login_path = path.clone();
    let path = StoredValue::new(path);

    view! {
        column(class = "wt-launcher__card", attr:data-testid = format!("launcher-install-{slot}")) {
            row(class = "wt-launcher__row") {
                text(class = "wt-launcher__strong") {{source}}
                text(class = "wt-launcher__mono wt-launcher__truncate") {{shown_path}}
                {version}
            }
            row(class = "wt-launcher__row") {
                if move || is_toolkit.get() {
                    text(class = "wt-launcher__badge", attr:data-testid = format!("launcher-toolkit-{slot}")) {{locale.text("ui.launcher.installs.toolkit")}}
                } else {}
                if move || played.get() {
                    text(class = "wt-launcher__badge", attr:data-testid = format!("launcher-played-{slot}")) {{locale.text("ui.launcher.installs.played")}}
                } else {}
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
                            {move || if has_content { locale.text("ui.launcher.installs.update")() } else { locale.text("ui.launcher.download")() }}
                        }
                        text(class = "wt-launcher__dim") {{locale.text("ui.launcher.download_hint")}}
                    }
                }
            }
            WgcLoginControl(model = model, path = login_path, test_id = format!("launcher-wgc-login-{slot}"), brief = false)
            row(class = "wt-launcher__row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    disabled = Signal::derive_local(move || is_toolkit.get()),
                    attr:data-testid = format!("launcher-use-{slot}"),
                    on:click = move |_| shell.apply_wows_dir(path.get_value().to_string_lossy().into_owned())
                ) {
                    {locale.text("ui.launcher.use_for_toolkit")}
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    disabled = Signal::derive_local(move || played.get() || !has_default.get()),
                    attr:data-testid = format!("launcher-play-from-{slot}"),
                    on:click = move |_| play_from(model, path.get_value())
                ) {
                    {locale.text("ui.launcher.installs.play_from")}
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
pub(crate) fn add_install(model: LauncherModel, shell: Shell) {
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

/// Asks where to put the newest build and downloads it there: for an install with content the
/// picker starts at the install, for one without at the default install directory.
fn download(model: LauncherModel, shell: Shell, install: PathBuf, has_content: bool) {
    if has_content {
        crate::launcher::play::start_update(shell, model, install);
        return;
    }
    let start = wgc_download::default_install_dir();
    let asked = wows_toolkit_services::dialog::pick_folder_in(&t!("ui.launcher.download_title"), &start);
    spawn_local(async move {
        let Some(dest) = shell.chosen_path(asked).await else { return };
        let started = model.run(move |launcher, _, env| (launcher.start_download(install, dest, env), false));
        if let Some(Err(err)) = started {
            shell.toasts().failed(err.to_string());
        }
    });
}

/// Makes the default account play the install at `path`: the app's game directory follows the
/// toolkit's setting, any other install is pinned.
fn play_from(model: LauncherModel, path: PathBuf) {
    let wows_dir = model.wows_dir_untracked();
    model.edit_settings(|settings| {
        let Some(key) = settings.default_account else { return };
        let choice = if wows_dir.as_deref() == Some(path.as_path()) {
            InstallChoice::Toolkit
        } else {
            InstallChoice::Path(path)
        };
        settings.set_account_install(key, choice);
    });
}
