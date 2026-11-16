//! The watchdog over the app's long-lived worker threads, kept running from a timer on the main
//! thread, and the workers it reports as not running for the status bar.

use std::time::Duration;

use futures::StreamExt;
use gpui_kit::App;
use gpui_kit::Global;
use rust_i18n::t;
use wows_toolkit_replay_data::live_match::LivePoll;
use wows_toolkit_services::supervision::watchdog::Watchdog;
use wows_toolkit_services::supervision::watchdog::WatchdogError;
use wows_toolkit_services::supervision::watchdog::WorkerHealth;
use wows_toolkit_services::supervision::watchdog::WorkerName;
use wows_toolkit_services::supervision::watchdog::WorkerReport;

use crate::status_bar::Job;

struct Supervision {
    watchdog: Watchdog,
    /// The watchdog's latest report.
    workers: Vec<WorkerReport>,
}

impl Global for Supervision {}

/// Starts the watchdog. Call once, during app startup, before any worker is started.
pub fn init(cx: &mut App) -> Result<(), WatchdogError> {
    // The watchdog checks every second, so one that stopped is noticed within two.
    const KEEP: Duration = Duration::from_secs(1);

    let watchdog = Watchdog::start()?;
    let mut reports = watchdog.reports();
    cx.set_global(Supervision { watchdog: watchdog.clone(), workers: Vec::new() });

    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(KEEP).await;
            watchdog.ensure_running();
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        while let Some(workers) = reports.next().await {
            cx.update(|cx| {
                cx.global_mut::<Supervision>().workers = workers;
                cx.refresh_windows();
            });
        }
    })
    .detach();

    Ok(())
}

/// The watchdog to register a worker with. `None` when it did not start, and then nothing is
/// watched.
pub fn watchdog(cx: &App) -> Option<Watchdog> {
    cx.try_global::<Supervision>().map(|supervision| supervision.watchdog.clone())
}

/// One status bar job per worker that is not running normally.
pub fn worker_jobs(cx: &App) -> Vec<Job> {
    let Some(supervision) = cx.try_global::<Supervision>() else { return Vec::new() };

    supervision
        .workers
        .iter()
        .filter_map(|worker| {
            let title = worker_title(&worker.name);
            let said = match &worker.health {
                WorkerHealth::Running => return None,
                WorkerHealth::Stalled => t!("ui.app.worker_stalled", worker = title),
                WorkerHealth::Restarting { .. } => t!("ui.app.worker_restarting", worker = title),
                WorkerHealth::GaveUp { .. } => t!("ui.app.worker_stopped", worker = title),
            };
            Some(Job { name: said.into_owned().into(), progress: None })
        })
        .collect()
}

/// The name a worker goes by in the status bar.
fn worker_title(name: &WorkerName) -> String {
    match name.as_str() {
        LivePoll::WORKER => t!("ui.app.worker_live_match").into_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread::JoinHandle;

    use gpui_kit::TestAppContext;
    use wows_toolkit_services::supervision::WorkerStop;
    use wows_toolkit_services::supervision::watchdog::Heartbeat;
    use wows_toolkit_services::supervision::watchdog::Supervised;
    use wows_toolkit_services::supervision::watchdog::WorkerSpec;

    use super::*;

    /// A worker whose thread ends as soon as it starts.
    struct Ending;

    impl Supervised for Ending {
        fn start(&self, _heartbeat: Heartbeat) -> std::io::Result<JoinHandle<()>> {
            std::thread::Builder::new().name("ending".to_owned()).spawn(|| {})
        }

        fn stopped(&self, _stop: &WorkerStop) {}

        fn gave_up(&self, _stop: &WorkerStop) {}
    }

    /// A worker the watchdog finds stopped is named on the status bar while it waits to restart.
    #[gpui_kit::test]
    async fn a_stopped_worker_is_named_on_the_status_bar(cx: &mut TestAppContext) {
        // The watchdog and the worker are real threads.
        cx.executor().allow_parking();
        cx.update(|cx| init(cx).expect("the watchdog starts"));
        let watchdog = cx.update(|cx| watchdog(cx)).expect("the watchdog is installed");
        let spec = WorkerSpec { name: WorkerName::new("ending"), stall_after: None };
        let _registration = watchdog.register(spec, Arc::new(Ending)).expect("the worker starts");

        let said = t!("ui.app.worker_restarting", worker = "ending").into_owned();
        while !cx.update(|cx| worker_jobs(cx)).iter().any(|job| job.name.as_ref() == said) {
            cx.executor().timer(Duration::from_millis(20)).await;
        }
    }
}
