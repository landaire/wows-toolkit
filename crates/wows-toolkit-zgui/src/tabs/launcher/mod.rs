//! The Launcher tab: discover, launch and manage WoWs installs, sign in, and install a game fresh.
//! The state and background work are shared with the other front ends through
//! `wows_toolkit_launcher`; this is the view over them.

mod account;
mod display;
mod download;
mod games;
mod installs;
mod model;
mod options;
#[cfg(test)]
pub mod testing;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use landaire_ui::LoadingPlacement;
use landaire_ui::LoadingProps;
use wows_toolkit_launcher::LauncherEvents;
use wows_toolkit_launcher::LauncherSettings;
use zgui::component;
use zgui::css;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::Children;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;

use crate::services::Job;
use crate::services::JobHandle;
use crate::services::Jobs;
use crate::services::Progress;
use crate::services::Services;
use crate::shell::Shell;
use crate::tabs::launcher::account::AccountSectionProps;
use crate::tabs::launcher::display::DownloadJob;
use crate::tabs::launcher::games::GamesSectionProps;
use crate::tabs::launcher::installs::InstallsSectionProps;
use crate::tabs::launcher::model::LauncherModel;
use crate::tabs::launcher::options::OptionsSectionProps;

const SHEET: &str = "wt-launcher";

const CSS: &str = css!(
    ".wt-launcher {
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
    }
    .wt-launcher__page {
        display: flex;
        flex-direction: column;
        gap: 24px;
        padding: 16px;
    }
    .wt-launcher__section {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .wt-launcher__heading {
        font-size: var(--zui-type-size-sm);
        font-weight: var(--zui-type-weight-bold);
        color: var(--lui-color-text-bright);
    }
    .wt-launcher__card {
        display: flex;
        flex-direction: column;
        gap: 8px;
        padding: 12px;
        border: 1px solid var(--lui-color-border);
        border-radius: 6px;
        background-color: var(--lui-color-surface);
    }
    .wt-launcher__column {
        display: flex;
        flex-direction: column;
        gap: 4px;
        min-width: 0;
    }
    .wt-launcher__column--wide { gap: 8px; }
    .wt-launcher__row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .wt-launcher__row--tight { gap: 4px; }
    .wt-launcher__spacer { flex: 1 1 auto; }
    .wt-launcher__text { font-size: var(--zui-type-size-sm); }
    .wt-launcher__dim {
        font-size: var(--zui-type-size-sm);
        color: var(--wt-color-text-dim);
    }
    .wt-launcher__warn {
        font-size: var(--zui-type-size-sm);
        color: var(--wt-color-warn);
    }
    .wt-launcher__error {
        font-size: var(--zui-type-size-sm);
        color: var(--wt-color-error);
    }
    .wt-launcher__ok {
        font-size: var(--zui-type-size-sm);
        color: var(--wt-color-ok);
    }
    .wt-launcher__strong { font-weight: var(--zui-type-weight-bold); }
    .wt-launcher__mono { font-family: var(--lui-font-family-mono); }
    .wt-launcher__truncate {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-launcher__row > .wt-launcher__input {
        width: 240px;
        flex: none;
    }
    .wt-launcher__textarea { min-height: 52px; }
    .wt-launcher__path-label { width: 60px; flex: none; }
    .wt-launcher__meter {
        display: flex;
        flex-direction: column;
        width: 220px;
        flex: none;
    }
    .wt-launcher__meter .zui-progress { width: 100%; }
    .wt-launcher__nowrap { flex: none; white-space: nowrap; }
    .wt-launcher .zui-spinner {
        width: 12px;
        height: 12px;
        flex: none;
    }"
);

/// The tab. Its settings row is read once the app's settings have opened the database.
#[component]
pub fn LauncherTab() -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let shell = Shell::current();
    let (model, events) = LauncherModel::new(services);

    listen(model, events);
    start(model, services);
    check_games(model);
    report_downloads(model, services.jobs);
    #[cfg(test)]
    testing::mounted(model);

    let locale = services.locale;
    let loaded = Memo::new(move |_| model.with(|state| state.settings.is_some()));

    view! {
        scroll(class = "wt-launcher", attr:data-testid = "launcher-tab") {
            if move || loaded.get() {
                column(class = "wt-launcher__page") {
                    AccountSection(model = model)
                    InstallsSection(model = model, shell = shell)
                    OptionsSection(model = model, shell = shell)
                    GamesSection(model = model, shell = shell)
                }
            } else {
                Loading(
                    label = Signal::derive_local(locale.text("ui.launcher.loading")),
                    placement = LoadingPlacement::Top,
                    attr:data-testid = "launcher-loading"
                )
            }
        }
    }
}

/// A titled card.
#[component]
fn Section(
    /// The translation key of the title.
    heading: &'static str,
    children: Children,
) -> impl IntoView {
    let locale = Services::current().locale;

    view! {
        column(class = "wt-launcher__section") {
            text(class = "wt-launcher__heading") {{locale.text(heading)}}
            column(class = "wt-launcher__card") {
                {children.into_view_once()}
            }
        }
    }
}

/// Applies each result of the launcher's background work as it arrives.
fn listen(model: LauncherModel, mut events: LauncherEvents) {
    spawn_local(async move {
        while let Some(event) = events.next().await {
            model.apply(event);
        }
    });
}

/// Follows the app's proxy setting, and reads the launcher's own settings row the first time the
/// app's settings are loaded.
fn start(model: LauncherModel, services: Services) {
    let requested = StoredValue::new(false);
    let following = RenderEffect::new(move |_| {
        let Some(proxy) = services.settings.read(|settings| settings.proxy_url.clone()) else { return };
        model.set_proxy(&proxy);

        #[cfg(test)]
        if testing::held() {
            return;
        }
        if requested.get_value() {
            return;
        }
        let Some(pool) = services.settings.pool() else { return };
        requested.set_value(true);

        let read = services.on_tokio(async move {
            wows_toolkit_config::queries::get_setting::<LauncherSettings>(&pool, wows_toolkit_launcher::SETTINGS_KEY)
                .await
        });
        spawn_local(async move {
            // No row is a launcher never configured, for which the defaults are right; an
            // unreadable row is logged by `get_setting` and starts from the defaults too, as the
            // other front ends do.
            let settings = read.await.ok().flatten().unwrap_or_default();
            model.adopt(settings);
        });
    });
    on_cleanup_local(move || drop(following));
}

/// Asks what each known game would install, once the settings are read. A game whose check
/// failed is asked again only by its retry button.
fn check_games(model: LauncherModel) {
    let unchecked =
        Memo::new(move |_| model.with(|state| state.settings.is_some() && !state.launcher.available_games_checked()));
    let checking = RenderEffect::new(move |_| {
        if unchecked.get() {
            model.op(|launcher, _, env| {
                launcher.start_available_games_check(env);
                false
            });
        }
    });
    on_cleanup_local(move || drop(checking));
}

/// Keeps one status bar job per running download.
fn report_downloads(model: LauncherModel, jobs: Jobs) {
    let running = Memo::new(move |_| model.with(|state| DownloadJob::all(&state.launcher)));
    // Outside the reactive arena: a dropped handle writes the jobs signal, which the arena must not
    // be locked for while it disposes of the tab.
    let handles: Rc<RefCell<HashMap<PathBuf, JobHandle>>> = Rc::default();
    let reported = Rc::clone(&handles);
    let reporting = RenderEffect::new(move |_| {
        let running = running.get();
        let mut handles = reported.borrow_mut();
        handles.retain(|key, _| running.iter().any(|download| download.key == *key));
        for download in running {
            let job = download_job(&download);
            match handles.get(&download.key) {
                Some(handle) => handle.report(job),
                None => {
                    handles.insert(download.key, jobs.start(job));
                }
            }
        }
    });
    on_cleanup_local(move || {
        drop(reporting);
        let finished = std::mem::take(&mut *handles.borrow_mut());
        drop(finished);
    });
}

/// What the status bar says of `download`: what it installs and its phase, with the share done
/// counted in MiB once the size is known. The total rounds up, so a size under one MiB still counts.
fn download_job(download: &DownloadJob) -> Job {
    const MIB: u64 = 1 << 20;

    let progress = download
        .progress
        .transferred
        .and_then(|transferred| Progress::new(transferred.done / MIB, transferred.total.get().div_ceil(MIB)));

    Job { name: format!("{}: {}", download.subject, download.progress.phase), progress }
}
