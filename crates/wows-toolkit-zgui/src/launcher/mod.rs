//! The game launcher, Linux only and behind an opt-in: the Play control at the end of the tab
//! strip, its account popover, the sign-in, setup, opt-in and game settings dialogs, the Settings
//! drawer's launcher rows and the palette's launcher entries. The state and background work are
//! shared with the other front ends through `wows_toolkit_launcher`; the model lives at shell
//! scope so discovery, update checks and downloads run whichever tab is open.

mod account;
mod display;
mod download;
pub mod drawer;
mod game_settings;
mod games;
mod installs;
mod model;
mod opt_in;
mod options;
mod palette;
mod play;
mod popover;
mod realm;
pub mod replay;
mod setup;
#[cfg(test)]
pub mod testing;
#[cfg(test)]
pub(crate) mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use wows_toolkit_launcher::LauncherEvents;
use wows_toolkit_launcher::LauncherSettings;
use zgui::css;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::task::spawn_local;

use crate::services::Job;
use crate::services::JobHandle;
use crate::services::Jobs;
use crate::services::Progress;
use crate::services::Services;
use crate::shell::Shell;

use self::display::DownloadJob;

pub use self::model::LauncherModel;
pub use self::play::PlayControlProps;

const SHEET: &str = "wt-launcher";

const CSS: &str = css!(
    ".wt-launcher__column {
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
    .wt-launcher__heading {
        font-size: var(--zui-type-size-sm);
        font-weight: var(--zui-type-weight-bold);
        color: var(--lui-color-text-bright);
    }
    .wt-launcher__link {
        font-size: var(--zui-type-size-sm);
        color: var(--lui-color-accent);
        text-decoration: underline;
    }
    .wt-launcher__keyring {
        padding: 6px 8px;
        border: 1px solid var(--wt-color-warn);
        border-radius: 4px;
    }
    .wt-launcher__mono { font-family: var(--lui-font-family-mono); }
    .wt-launcher__truncate {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-launcher__card {
        display: flex;
        flex-direction: column;
        gap: 6px;
        padding: 8px 12px;
        border: 1px solid var(--lui-color-border);
        border-radius: 6px;
        background-color: var(--lui-color-surface);
    }
    .wt-launcher__input { width: 240px; flex: none; }
    .wt-launcher__textarea.wt-launcher__textarea { min-height: 48px; }
    .wt-launcher__path-label { width: 60px; flex: none; }
    .wt-launcher__meter {
        display: flex;
        flex-direction: column;
        width: 220px;
        flex: none;
    }
    .wt-launcher__meter .zui-progress { width: 100%; }
    .wt-launcher__nowrap { flex: none; white-space: nowrap; }
    .wt-launcher__badge {
        flex: none;
        padding: 0 5px;
        border-radius: 3px;
        background-color: var(--lui-color-panel);
        border: 1px solid var(--lui-color-border);
        font-size: 11px;
        font-weight: var(--zui-type-weight-bold);
    }
    .wt-launcher .zui-spinner,
    .wt-launcher__row .zui-spinner {
        width: 12px;
        height: 12px;
        flex: none;
    }"
);

/// Starts the launcher with the shell: follows the proxy setting, reads the launcher's settings
/// row once the app's settings are loaded, applies its background work's results, keeps a status
/// bar job per running download. The work itself waits for the opt-in.
pub fn begin(shell: Shell, model: LauncherModel, events: LauncherEvents) {
    let services = shell.services();
    listen(shell, model, events);
    start(model, services);
    report_downloads(model, services.jobs);
}

/// Applies each result of the launcher's background work as it arrives, and says why a launch
/// failed once its background work gives up.
fn listen(shell: Shell, model: LauncherModel, mut events: LauncherEvents) {
    spawn_local(async move {
        while let Some(event) = events.next().await {
            model.apply(event);
            if let Some(failure) = model.take_launch_failure() {
                play::report_launch_failure(shell, &failure);
            }
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

/// Keeps one status bar job per running download.
fn report_downloads(model: LauncherModel, jobs: Jobs) {
    let running = Memo::new(move |_| model.with(|state| DownloadJob::all(&state.launcher)));
    // Outside the reactive arena: a dropped handle writes the jobs signal, which the arena must not
    // be locked for while it disposes of the shell.
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
