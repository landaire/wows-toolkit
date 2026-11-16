//! What is running in the background, for the status bar to name.

use std::num::NonZeroU64;

use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;

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

/// Everything running, oldest first.
#[derive(Clone, Copy)]
pub struct Jobs {
    running: RwSignal<Vec<(JobId, Job)>, LocalStorage>,
    next: StoredValue<u64>,
}

impl Jobs {
    pub(super) fn new() -> Self {
        Self { running: RwSignal::new_local(Vec::new()), next: StoredValue::new(0) }
    }

    /// Puts `job` on the status bar until the returned handle is dropped.
    ///
    /// Hold the handle in the task doing the work: a task cancelled with its view drops it, so work
    /// whose owner is gone leaves the bar with it.
    #[must_use = "dropping the handle takes the job off the status bar"]
    pub fn start(self, job: Job) -> JobHandle {
        let id = JobId(self.next.get_value());
        self.next.update_value(|next| *next += 1);
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
