//! What is running in the background, for the status bar to name.

use std::num::NonZeroU64;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Update;

/// How far a job has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: NonZeroU64,
}

impl Progress {
    /// `None` for a total of zero: nothing is known yet about how far along it is.
    pub fn new(done: u64, total: u64) -> Option<Self> {
        Some(Self { done, total: NonZeroU64::new(total)? })
    }
}

/// One piece of background work, as the status bar names it.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    /// Already translated.
    pub name: String,
    pub progress: Option<Progress>,
}

/// Names one entry in [`Jobs`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JobId(u64);

impl JobId {
    /// Drawn from a process-wide counter, so a job started after the window owning [`Jobs`] is
    /// gone reads no disposed state.
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);

        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Everything running, oldest first.
#[derive(Clone, Copy)]
pub struct Jobs {
    running: RwSignal<Vec<(JobId, Job)>, LocalStorage>,
}

impl Jobs {
    pub(super) fn new() -> Self {
        Self { running: RwSignal::new_local(Vec::new()) }
    }

    /// Puts `job` on the status bar until the returned handle is dropped.
    ///
    /// Hold the handle in the task doing the work: a task cancelled with its view drops it, so work
    /// whose owner is gone leaves the bar with it.
    #[must_use = "dropping the handle takes the job off the status bar"]
    pub fn start(self, job: Job) -> JobHandle {
        let id = JobId::next();
        // `update` does nothing once the window owning the bar is gone.
        self.running.update(|running| running.push((id, job)));

        JobHandle { jobs: self, id }
    }

    /// What is running, oldest first.
    pub fn running(self) -> Vec<Job> {
        self.running.get().into_iter().map(|(_, job)| job).collect()
    }
}

/// Keeps one job on the status bar.
pub struct JobHandle {
    jobs: Jobs,
    id: JobId,
}

impl JobHandle {
    /// Replaces what the job says.
    pub fn report(&self, job: Job) {
        let id = self.id;
        self.jobs.running.update(|running| {
            if let Some((_, shown)) = running.iter_mut().find(|(running, _)| *running == id) {
                *shown = job;
            }
        });
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        let id = self.id;
        // `try_` because the handle can outlive the window whose services it reported to.
        self.jobs.running.try_update(|running| running.retain(|(running, _)| *running != id));
    }
}

#[cfg(test)]
mod tests {
    use zgui::reactive::Mounted;

    use super::*;

    fn export() -> Job {
        Job { name: "export".to_string(), progress: None }
    }

    /// A detached replay export starts its job once the save dialog returns, which can be after the
    /// main window owning the status bar is gone.
    #[test]
    fn a_job_started_after_the_bar_is_gone_does_nothing() {
        zgui::reactive::install().expect("the reactive runtime installs");
        let main = Mounted::new();
        let jobs = main.with(Jobs::new);
        let first = jobs.start(export());
        assert_eq!(jobs.running(), [export()]);
        drop(first);

        main.unmount();
        let handle = jobs.start(export());
        handle.report(Job { name: "export".to_string(), progress: Progress::new(1, 2) });
        drop(handle);
    }
}
