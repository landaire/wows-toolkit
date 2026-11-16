//! Long-lived workers that can stop on their own: why one stopped, and when to
//! start it again.
//!
//! A worker on a thread of its own takes nothing else down when it dies, so the
//! feature it serves stops with no sign unless its owner is told.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::panic::PanicHookInfo;
use std::time::Duration;
use std::time::Instant;

pub mod queue;
pub mod watchdog;

/// A panic, as far as it could be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanicReport {
    /// `None` for a payload that is not a string.
    message: Option<String>,
    /// `None` when the panic hook did not record the panic.
    location: Option<String>,
}

impl PanicReport {
    /// The panic caught with `payload` on this thread.
    pub fn caught(payload: &(dyn Any + Send)) -> Self {
        Self::take_recorded().unwrap_or_else(|| Self { message: payload_message(payload), location: None })
    }

    /// The panic unwinding this thread, for a drop guard that has no payload
    /// to read.
    pub fn unwinding() -> Self {
        Self::take_recorded().unwrap_or(Self { message: None, location: None })
    }

    fn take_recorded() -> Option<Self> {
        LAST_PANIC.try_with(|last| last.borrow_mut().take()).ok().flatten()
    }
}

impl fmt::Display for PanicReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.message, &self.location) {
            (Some(message), Some(location)) => write!(f, "{message} (at {location})"),
            (Some(message), None) => f.write_str(message),
            (None, Some(location)) => write!(f, "panic at {location}"),
            (None, None) => f.write_str("panic with no message"),
        }
    }
}

/// Why a worker thread stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkerStop {
    Panicked(PanicReport),
    /// The thread ended without a panic while it was still wanted.
    Exited,
}

impl fmt::Display for WorkerStop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Panicked(report) => write!(f, "it panicked: {report}"),
            Self::Exited => f.write_str("its thread ended"),
        }
    }
}

/// Counts the restarts of a stopped worker, from one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RestartAttempt(u32);

impl RestartAttempt {
    pub const FIRST: Self = Self(1);

    pub fn get(self) -> u32 {
        self.0
    }
}

/// What follows a worker stopping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextStep {
    /// Started again once `delay` has passed.
    Retry { attempt: RestartAttempt, delay: Duration },
    /// Every restart has been tried; only the reader starts it again.
    GaveUp,
}

impl NextStep {
    /// Whether the reader is told of the stop: the first of a run of failures
    /// and the one that ends the retries are said, the restarts between are
    /// only logged.
    pub fn is_announced(self) -> bool {
        match self {
            Self::Retry { attempt, .. } => attempt == RestartAttempt::FIRST,
            Self::GaveUp => true,
        }
    }
}

/// When a stopped worker is started again: 1 s, 5 s and 30 s after successive
/// stops, then not until the reader asks.
///
/// A passing fault (a directory locked by a game update, a network share
/// reconnecting) is ridden out within a minute, while a lasting one is tried
/// three times rather than forever. A worker that ran for [`Self::STEADY`]
/// before it stopped starts the count again.
#[derive(Debug)]
pub struct RestartPolicy {
    /// Stops since the worker last ran steadily.
    failures: u32,
    started_at: Option<Instant>,
}

impl RestartPolicy {
    const DELAYS: [Duration; 3] = [Duration::from_secs(1), Duration::from_secs(5), Duration::from_secs(30)];
    /// How many restarts are tried before only the reader starts it again.
    pub const ATTEMPTS: RestartAttempt = RestartAttempt(Self::DELAYS.len() as u32);
    const STEADY: Duration = Duration::from_secs(5 * 60);

    pub fn new() -> Self {
        Self { failures: 0, started_at: None }
    }

    /// The worker is running from `now`.
    pub fn started(&mut self, now: Instant) {
        self.started_at = Some(now);
    }

    /// The worker stopped at `now`, or a restart failed to start it.
    pub fn stopped(&mut self, now: Instant) -> NextStep {
        let steady = self.started_at.take().is_some_and(|at| now.saturating_duration_since(at) >= Self::STEADY);
        if steady {
            self.failures = 0;
        }
        self.failures = self.failures.saturating_add(1);

        let index = self.failures as usize - 1;
        match Self::DELAYS.get(index) {
            Some(delay) => NextStep::Retry { attempt: RestartAttempt(self.failures), delay: *delay },
            None => NextStep::GaveUp,
        }
    }

    /// Starts the count again, for a restart the reader asked for or a
    /// worker ended on purpose.
    pub fn reset(&mut self) {
        self.failures = 0;
        self.started_at = None;
    }
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    /// The last panic on this thread, for whatever catches it or is dropped
    /// by it to read.
    static LAST_PANIC: RefCell<Option<PanicReport>> = const { RefCell::new(None) };
}

/// Notes a panic for [`PanicReport`] to read on the panicking thread. Called
/// from the panic hook, which is the only place its location is known.
pub fn record_panic(info: &PanicHookInfo<'_>) {
    let report =
        PanicReport { message: payload_message(info.payload()), location: info.location().map(ToString::to_string) };
    // The thread's storage is gone when the panic is in its teardown.
    let _ = LAST_PANIC.try_with(|last| *last.borrow_mut() = Some(report));
}

/// The text `panic!` and `expect` put in a payload.
fn payload_message(payload: &(dyn Any + Send)) -> Option<String> {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return Some((*message).to_owned());
    }

    payload.downcast_ref::<String>().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn retry(attempt: u32, seconds: u64) -> NextStep {
        NextStep::Retry { attempt: RestartAttempt(attempt), delay: Duration::from_secs(seconds) }
    }

    /// Successive stops back off and then stop retrying; only the first and
    /// the last of the run are said.
    #[test]
    fn restarts_back_off_and_then_give_up() {
        let mut policy = RestartPolicy::new();
        let start = Instant::now();

        let steps: Vec<NextStep> = (0..5).map(|n| policy.stopped(start + SECOND * n)).collect();

        let gave_up = NextStep::GaveUp;
        assert_eq!(steps, vec![retry(1, 1), retry(2, 5), retry(3, 30), gave_up, gave_up]);
        let announced: Vec<bool> = steps.iter().map(|step| step.is_announced()).collect();
        assert_eq!(announced, vec![true, false, false, true, true]);
    }

    /// A restart that runs only briefly before stopping again counts towards
    /// the limit; one that runs steadily starts the count again.
    #[test]
    fn a_steady_run_starts_the_count_again() {
        let mut policy = RestartPolicy::new();
        let start = Instant::now();
        assert_eq!(policy.stopped(start), retry(1, 1));

        policy.started(start + SECOND);
        assert_eq!(policy.stopped(start + SECOND * 2), retry(2, 5), "a brief run is the same run of failures");

        policy.started(start + SECOND * 10);
        let later = start + SECOND * 10 + RestartPolicy::STEADY;
        assert_eq!(policy.stopped(later), retry(1, 1), "a steady run is not");
    }

    /// A restart the reader asks for after the retries are exhausted gets the
    /// full set of retries again.
    #[test]
    fn a_reset_starts_the_count_again() {
        let mut policy = RestartPolicy::new();
        let start = Instant::now();
        for n in 0..4 {
            policy.stopped(start + SECOND * n);
        }

        policy.reset();

        assert_eq!(policy.stopped(start + SECOND * 5), retry(1, 1));
    }

    /// A panic caught without the hook having run still says its message.
    #[test]
    fn a_caught_panic_reads_its_payload() {
        let payload = std::panic::catch_unwind(|| std::panic::panic_any(String::from("the queue overflowed")))
            .expect_err("the closure panics");
        LAST_PANIC.with(|last| last.borrow_mut().take());

        let report = PanicReport::caught(payload.as_ref());

        assert_eq!(report.to_string(), "the queue overflowed");
    }
}
