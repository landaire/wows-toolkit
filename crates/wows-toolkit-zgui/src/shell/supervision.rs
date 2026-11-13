//! The status bar's view of the supervised workers: the replays being read, and any worker that
//! is restarting, not responding or stopped. Also keeps the watchdog itself running.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use futures::StreamExt;
use rust_i18n::t;
use wows_toolkit_replay_data::loader::ReplayLoader;
use wows_toolkit_services::supervision::queue::Activity;
use wows_toolkit_services::supervision::watchdog::WorkerHealth;
use wows_toolkit_services::supervision::watchdog::WorkerName;
use wows_toolkit_services::supervision::watchdog::WorkerReport;
use zgui::reactive::on_cleanup_local;
use zgui::task::spawn_local;
use zgui::view::Timers;

use crate::services::Job;
use crate::services::JobHandle;
use crate::services::Services;

/// Starts the watchdog's keeper and the status bar's reports, for the life of the caller.
pub fn begin(services: Services) {
    // The watchdog checks every second; a stopped one is noticed within two.
    const KEEP: Duration = Duration::from_secs(2);

    let Some(watchdog) = services.watchdog() else { return };

    let keeper = Timers::current().map(|timers| {
        let kept = watchdog.clone();
        timers.set_interval(KEEP, move || {
            kept.ensure_running();
        })
    });
    on_cleanup_local(move || drop(keeper));

    let mut reports = watchdog.reports();
    spawn_local(async move {
        let mut shown: HashMap<WorkerName, JobHandle> = HashMap::new();
        while let Some(workers) = reports.next().await {
            show_workers(services, &mut shown, &workers);
        }
    });

    let Some(loader) = services.supervision.replay_loader() else { return };
    let mut activity = loader.activity();
    spawn_local(async move {
        let mut reading: Option<JobHandle> = None;
        while let Some(held) = activity.next().await {
            show_reading(services, &mut reading, &held);
        }
    });
}

/// The name a worker goes by in the status bar.
pub fn worker_title(name: &WorkerName) -> String {
    match name.as_str() {
        ReplayLoader::WORKER => t!("ui.app.worker_replay_loader").into_owned(),
        other => other.to_owned(),
    }
}

/// One job per worker that is not running normally.
fn show_workers(services: Services, shown: &mut HashMap<WorkerName, JobHandle>, workers: &[WorkerReport]) {
    shown.retain(|name, _| workers.iter().any(|worker| &worker.name == name && worker.health != WorkerHealth::Running));
    for worker in workers {
        let title = worker_title(&worker.name);
        let said = match &worker.health {
            WorkerHealth::Running => continue,
            WorkerHealth::Stalled => t!("ui.app.worker_stalled", worker = title),
            WorkerHealth::Restarting { .. } => t!("ui.app.worker_restarting", worker = title),
            WorkerHealth::GaveUp { .. } => t!("ui.app.worker_stopped", worker = title),
        };
        let job = Job { name: said.into_owned(), progress: None };
        match shown.get(&worker.name) {
            Some(handle) => handle.report(job),
            None => {
                shown.insert(worker.name.clone(), services.jobs.start(job));
            }
        }
    }
}

/// One job while the loader holds replays: the one it reads, or how many it holds.
fn show_reading(services: Services, reading: &mut Option<JobHandle>, held: &Activity<PathBuf>) {
    let subjects: Vec<&PathBuf> = held.subjects().collect();
    let said = match subjects.as_slice() {
        [] => {
            *reading = None;
            return;
        }
        [one] => t!("ui.app.reading_replay", replay = one.file_stem().unwrap_or(one.as_os_str()).to_string_lossy()),
        many => t!("ui.app.reading_replays", count = many.len()),
    };
    let job = Job { name: said.into_owned(), progress: None };
    match reading {
        Some(handle) => handle.report(job),
        None => *reading = Some(services.jobs.start(job)),
    }
}
