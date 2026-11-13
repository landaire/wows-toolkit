//! One thread that checks every critical worker on a fixed period: it starts a stopped worker
//! again by that worker's [`RestartPolicy`], and reports one whose heartbeat has gone quiet.
//!
//! A thread cannot be stopped from outside, so a quiet worker is only reported, and its state
//! goes back to running when the heartbeat returns. The watchdog beats too: whoever owns it calls
//! [`Watchdog::ensure_running`] from a timer of its own and so starts the watchdog again if it
//! stopped.

use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::sync::Weak;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;

use super::NextStep;
use super::PanicReport;
use super::RestartAttempt;
use super::RestartPolicy;
use super::WorkerStop;

/// The stable name of a supervised worker, which its thread also carries.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkerName(String);

impl WorkerName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// When a worker last showed it was making progress. Cheap to clone; the worker keeps one and
/// beats it, the watchdog reads it.
#[derive(Clone, Debug)]
pub struct Heartbeat {
    epoch: Instant,
    /// Milliseconds from `epoch` to the last beat.
    last: Arc<AtomicU64>,
}

impl Heartbeat {
    fn new(epoch: Instant) -> Self {
        let heartbeat = Self { epoch, last: Arc::new(AtomicU64::new(0)) };
        heartbeat.beat();

        heartbeat
    }

    pub fn beat(&self) {
        let elapsed = u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.last.store(elapsed, Ordering::Relaxed);
    }

    /// How long it has been quiet at `now`.
    fn quiet_at(&self, now: Instant) -> Duration {
        let last = self.epoch + Duration::from_millis(self.last.load(Ordering::Relaxed));
        now.saturating_duration_since(last)
    }
}

/// What the watchdog starts again, and tells when it stops.
pub trait Supervised: Send + Sync + 'static {
    /// Starts the worker's thread, which beats `heartbeat` while it makes progress. Called at
    /// registration and for every restart.
    fn start(&self, heartbeat: Heartbeat) -> std::io::Result<JoinHandle<()>>;

    /// The thread stopped with `stop`. Fails the work it held.
    fn stopped(&self, stop: &WorkerStop);

    /// No restart is left; the worker stays stopped until [`Watchdog::restart`]. Fails the work
    /// waiting for it.
    fn gave_up(&self, stop: &WorkerStop);
}

/// How a worker is supervised.
#[derive(Clone, Debug)]
pub struct WorkerSpec {
    pub name: WorkerName,
    /// How long its heartbeat may be quiet before it is reported stalled. `None` for a worker
    /// that waits without beating.
    pub stall_after: Option<Duration>,
}

/// How a supervised worker is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkerHealth {
    Running,
    /// Its heartbeat has been quiet past its limit.
    Stalled,
    /// Stopped with `stop`, and started again after a delay.
    Restarting {
        attempt: RestartAttempt,
        stop: WorkerStop,
    },
    /// Stopped with `stop`, and not started again until asked.
    GaveUp {
        stop: WorkerStop,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerReport {
    pub name: WorkerName,
    pub health: WorkerHealth,
}

#[derive(Debug, thiserror::Error)]
pub enum WatchdogError {
    #[error("the {name} thread could not be started: {source}")]
    Spawn {
        name: WorkerName,
        #[source]
        source: std::io::Error,
    },
}

/// A handle on the watchdog. The watchdog thread ends once every handle is dropped.
#[derive(Clone)]
pub struct Watchdog {
    shared: Arc<Shared>,
}

impl Watchdog {
    const PERIOD: Duration = Duration::from_secs(1);

    /// Starts the watchdog thread, checking every second.
    pub fn start() -> Result<Self, WatchdogError> {
        Self::start_with_period(Self::PERIOD)
    }

    /// Starts the watchdog thread, checking every `period`.
    pub fn start_with_period(period: Duration) -> Result<Self, WatchdogError> {
        let epoch = Instant::now();
        let shared = Arc::new(Shared {
            period,
            epoch,
            beat: Heartbeat::new(epoch),
            state: Mutex::new(State { next_id: 0, workers: Vec::new(), watchers: Vec::new() }),
            thread: Mutex::new(None),
            #[cfg(any(test, feature = "test-support"))]
            stop_asked: std::sync::atomic::AtomicBool::new(false),
        });
        let thread = spawn_watchdog(&shared)?;
        *lock(&shared.thread) = Some(thread);

        Ok(Self { shared })
    }

    /// Starts `supervised` and watches it until the returned registration is dropped. Drop the
    /// registration before telling the worker to end, or the end is taken for a stop.
    pub fn register(&self, spec: WorkerSpec, supervised: Arc<dyn Supervised>) -> Result<Registration, WatchdogError> {
        let heartbeat = Heartbeat::new(self.shared.epoch);
        let thread = supervised
            .start(heartbeat.clone())
            .map_err(|source| WatchdogError::Spawn { name: spec.name.clone(), source })?;
        let mut policy = RestartPolicy::new();
        policy.started(Instant::now());

        let mut state = self.shared.lock();
        let id = state.next_id;
        state.next_id += 1;
        state.workers.push(Entry {
            id,
            spec,
            supervised,
            heartbeat,
            thread: Some(thread),
            policy,
            health: WorkerHealth::Running,
            restart_at: None,
        });
        self.shared.publish(&mut state);

        Ok(Registration { id, shared: Arc::downgrade(&self.shared) })
    }

    /// Starts a worker that gave up again, with its full set of restarts.
    pub fn restart(&self, name: &WorkerName) {
        let mut state = self.shared.lock();
        let now = Instant::now();
        for entry in state.workers.iter_mut().filter(|entry| &entry.spec.name == name) {
            if matches!(entry.health, WorkerHealth::GaveUp { .. }) {
                entry.policy.reset();
                entry.restart_at = Some(now);
            }
        }
    }

    /// Every worker's health now, then again each time one changes.
    pub fn reports(&self) -> UnboundedReceiver<Vec<WorkerReport>> {
        let (watcher, reports) = futures::channel::mpsc::unbounded();
        let mut state = self.shared.lock();
        let _ = watcher.unbounded_send(state.reports());
        state.watchers.push(watcher);

        reports
    }

    /// Starts the watchdog thread again if it has ended. Returns whether it did. Call from a
    /// timer the watchdog does not run.
    pub fn ensure_running(&self) -> bool {
        let mut thread = lock(&self.shared.thread);
        if thread.as_ref().is_some_and(|thread| !thread.is_finished()) {
            if self.shared.beat.quiet_at(Instant::now()) > self.shared.period * 5 {
                tracing::warn!("the watchdog thread is not responding");
            }
            return false;
        }

        if let Some(ended) = thread.take() {
            match ended.join() {
                Ok(()) => tracing::error!("the watchdog thread ended; starting another"),
                Err(payload) => {
                    let report = PanicReport::caught(payload.as_ref());
                    tracing::error!(%report, "the watchdog thread panicked; starting another");
                }
            }
        }
        match spawn_watchdog(&self.shared) {
            Ok(started) => {
                *thread = Some(started);
                true
            }
            Err(err) => {
                tracing::error!(%err, "the watchdog thread could not be started again");
                false
            }
        }
    }

    /// Makes the watchdog thread panic at its next check, standing in for a fault in it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stop_for_test(&self) {
        self.shared.stop_asked.store(true, Ordering::Relaxed);
    }

    /// Checks every worker once, at `now`.
    fn check(shared: &Shared, now: Instant) {
        let mut dead = Vec::new();
        let mut due = Vec::new();
        {
            let mut state = shared.lock();
            let mut changed = false;
            for entry in &mut state.workers {
                match entry.thread.as_ref().map(JoinHandle::is_finished) {
                    Some(true) => {
                        if let Some(thread) = entry.thread.take() {
                            dead.push((entry.id, thread, Arc::clone(&entry.supervised)));
                        }
                    }
                    Some(false) => changed |= entry.check_heartbeat(now),
                    None if entry.restart_at.is_some_and(|at| at <= now) => {
                        entry.restart_at = None;
                        due.push((entry.id, Arc::clone(&entry.supervised), entry.heartbeat.clone()));
                    }
                    // Given up, or waiting out its restart delay.
                    None => {}
                }
            }
            if changed {
                shared.publish(&mut state);
            }
        }

        // The worker's own calls run outside the lock: they take the worker's locks, which its
        // owner may hold while it drops a registration.
        for (id, thread, supervised) in dead {
            let stop = match thread.join() {
                Ok(()) => WorkerStop::Exited,
                Err(payload) => WorkerStop::Panicked(PanicReport::caught(payload.as_ref())),
            };
            supervised.stopped(&stop);
            let gave_up = shared.with_entry(id, |entry| {
                let next = entry.policy.stopped(now);
                tracing::error!(worker = %entry.spec.name, %stop, ?next, "a supervised worker stopped");
                match next {
                    NextStep::Retry { attempt, delay } => {
                        entry.health = WorkerHealth::Restarting { attempt, stop: stop.clone() };
                        entry.restart_at = Some(now + delay);
                        false
                    }
                    NextStep::GaveUp => {
                        entry.health = WorkerHealth::GaveUp { stop: stop.clone() };
                        true
                    }
                }
            });
            if gave_up == Some(true) {
                supervised.gave_up(&stop);
            }
        }

        for (id, supervised, heartbeat) in due {
            heartbeat.beat();
            let started = supervised.start(heartbeat);
            let gave_up = shared.with_entry(id, |entry| match started {
                Ok(thread) => {
                    tracing::info!(worker = %entry.spec.name, "a supervised worker was started again");
                    entry.thread = Some(thread);
                    entry.policy.started(now);
                    entry.health = WorkerHealth::Running;
                    None
                }
                Err(err) => {
                    tracing::error!(worker = %entry.spec.name, %err, "a supervised worker could not be started again");
                    let stop = match &entry.health {
                        WorkerHealth::Restarting { stop, .. } | WorkerHealth::GaveUp { stop } => stop.clone(),
                        WorkerHealth::Running | WorkerHealth::Stalled => WorkerStop::Exited,
                    };
                    match entry.policy.stopped(now) {
                        NextStep::Retry { attempt, delay } => {
                            entry.health = WorkerHealth::Restarting { attempt, stop };
                            entry.restart_at = Some(now + delay);
                            None
                        }
                        NextStep::GaveUp => {
                            entry.health = WorkerHealth::GaveUp { stop: stop.clone() };
                            Some(stop)
                        }
                    }
                }
            });
            if let Some(stop) = gave_up.flatten() {
                supervised.gave_up(&stop);
            }
        }
    }
}

/// Keeps a worker watched. Dropping it stops the watching; the worker's thread is not joined.
pub struct Registration {
    id: u64,
    shared: Weak<Shared>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        let Some(shared) = self.shared.upgrade() else { return };
        let mut state = shared.lock();
        state.workers.retain(|entry| entry.id != self.id);
        shared.publish(&mut state);
    }
}

struct Shared {
    period: Duration,
    epoch: Instant,
    /// The watchdog thread's own heartbeat.
    beat: Heartbeat,
    state: Mutex<State>,
    thread: Mutex<Option<JoinHandle<()>>>,
    #[cfg(any(test, feature = "test-support"))]
    stop_asked: std::sync::atomic::AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn publish(&self, state: &mut State) {
        let reports = state.reports();
        state.watchers.retain(|watcher| watcher.unbounded_send(reports.clone()).is_ok());
    }

    /// Runs `change` on the worker `id` while it is still registered, and publishes the result.
    fn with_entry<T>(&self, id: u64, change: impl FnOnce(&mut Entry) -> T) -> Option<T> {
        let mut state = self.lock();
        let changed = state.workers.iter_mut().find(|entry| entry.id == id).map(change);
        self.publish(&mut state);

        changed
    }
}

struct State {
    next_id: u64,
    workers: Vec<Entry>,
    watchers: Vec<UnboundedSender<Vec<WorkerReport>>>,
}

impl State {
    fn reports(&self) -> Vec<WorkerReport> {
        self.workers
            .iter()
            .map(|entry| WorkerReport { name: entry.spec.name.clone(), health: entry.health.clone() })
            .collect()
    }
}

struct Entry {
    id: u64,
    spec: WorkerSpec,
    supervised: Arc<dyn Supervised>,
    heartbeat: Heartbeat,
    /// `None` while stopped.
    thread: Option<JoinHandle<()>>,
    policy: RestartPolicy,
    health: WorkerHealth,
    /// When a stopped worker is started again; `None` while running or given up.
    restart_at: Option<Instant>,
}

impl Entry {
    /// Marks a running worker stalled or recovered by its heartbeat. Returns whether it changed.
    fn check_heartbeat(&mut self, now: Instant) -> bool {
        let Some(limit) = self.spec.stall_after else { return false };

        let quiet = self.heartbeat.quiet_at(now) > limit;
        match (&self.health, quiet) {
            (WorkerHealth::Running, true) => {
                tracing::warn!(worker = %self.spec.name, "a supervised worker is not responding");
                self.health = WorkerHealth::Stalled;
                true
            }
            (WorkerHealth::Stalled, false) => {
                tracing::info!(worker = %self.spec.name, "a supervised worker is responding again");
                self.health = WorkerHealth::Running;
                true
            }
            _ => false,
        }
    }
}

/// A lock no holder panics under, so a poisoned one still guards a whole value.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn spawn_watchdog(shared: &Arc<Shared>) -> Result<JoinHandle<()>, WatchdogError> {
    let name = WorkerName::new("watchdog");
    let watched = Arc::downgrade(shared);
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || watch(&watched))
        .map_err(|source| WatchdogError::Spawn { name, source })
}

/// Checks every worker each period until the last handle is gone.
fn watch(shared: &Weak<Shared>) {
    loop {
        let Some(shared) = shared.upgrade() else { return };
        shared.beat.beat();
        #[cfg(any(test, feature = "test-support"))]
        if shared.stop_asked.swap(false, Ordering::Relaxed) {
            panic!("a simulated watchdog fault");
        }
        Watchdog::check(&shared, Instant::now());
        let period = shared.period;
        drop(shared);

        std::thread::sleep(period);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    use futures::StreamExt;
    use futures::executor::block_on;

    use super::*;

    const PERIOD: Duration = Duration::from_millis(20);

    /// A worker that runs until told to panic or to go quiet.
    #[derive(Default)]
    struct Toy {
        starts: AtomicUsize,
        stops: Mutex<Vec<WorkerStop>>,
        gave_up: AtomicBool,
        /// Makes every later start fail, as when the system refuses a thread.
        refuse_starts: AtomicBool,
        /// Sends to the running thread: `true` panics it, `false` silences its heartbeat.
        orders: Mutex<Option<mpsc::Sender<bool>>>,
    }

    impl Toy {
        fn order(&self, panic: bool) {
            let orders = lock(&self.orders);
            orders.as_ref().expect("the worker runs").send(panic).expect("the worker takes orders");
        }
    }

    impl Supervised for Toy {
        fn start(&self, heartbeat: Heartbeat) -> std::io::Result<JoinHandle<()>> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            if self.refuse_starts.load(Ordering::SeqCst) {
                return Err(std::io::Error::other("no thread for the toy"));
            }
            let (orders, taken) = mpsc::channel();
            *lock(&self.orders) = Some(orders);
            std::thread::Builder::new().name("toy".to_owned()).spawn(move || {
                let mut beating = true;
                loop {
                    if beating {
                        heartbeat.beat();
                    }
                    match taken.recv_timeout(Duration::from_millis(5)) {
                        Ok(true) => panic!("the toy broke"),
                        Ok(false) => beating = !beating,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
            })
        }

        fn stopped(&self, stop: &WorkerStop) {
            lock(&self.stops).push(stop.clone());
        }

        fn gave_up(&self, _stop: &WorkerStop) {
            self.gave_up.store(true, Ordering::SeqCst);
        }
    }

    fn spec(stall_after: Option<Duration>) -> WorkerSpec {
        WorkerSpec { name: WorkerName::new("toy"), stall_after }
    }

    /// Waits for a report that `wanted` accepts.
    fn wait_for(reports: &mut UnboundedReceiver<Vec<WorkerReport>>, wanted: impl Fn(&WorkerHealth) -> bool) {
        loop {
            let report = block_on(reports.next()).expect("the watchdog reports");
            if report.iter().any(|report| wanted(&report.health)) {
                return;
            }
        }
    }

    /// A stopped worker is found on a check, its work failed with the panic, and it is started
    /// again after the policy's first delay.
    #[test]
    fn a_dead_worker_is_found_and_started_again() {
        let watchdog = Watchdog::start_with_period(PERIOD).expect("the watchdog starts");
        let toy = Arc::new(Toy::default());
        let _registration = watchdog.register(spec(None), Arc::clone(&toy) as Arc<dyn Supervised>).expect("it starts");
        let mut reports = watchdog.reports();

        toy.order(true);
        wait_for(
            &mut reports,
            |health| matches!(health, WorkerHealth::Restarting { attempt, .. } if attempt.get() == 1),
        );
        let stops = lock(&toy.stops).clone();
        let [WorkerStop::Panicked(report)] = stops.as_slice() else { panic!("one stop, a panic: {stops:?}") };
        assert!(report.to_string().contains("the toy broke"), "the stop names the panic: {report}");

        wait_for(&mut reports, |health| *health == WorkerHealth::Running);
        assert_eq!(toy.starts.load(Ordering::SeqCst), 2, "the worker was started again");
    }

    /// A worker that stops more often than its policy allows is given up on, and starts again
    /// when asked.
    #[test]
    fn a_worker_that_keeps_stopping_is_given_up_until_asked() {
        let watchdog = Watchdog::start_with_period(PERIOD).expect("the watchdog starts");
        let toy = Arc::new(Toy::default());
        let _registration = watchdog.register(spec(None), Arc::clone(&toy) as Arc<dyn Supervised>).expect("it starts");
        let shared = &watchdog.shared;
        let mut reports = watchdog.reports();

        // Stops past every delay of the policy, without waiting for them.
        for _ in 0..=RestartPolicy::ATTEMPTS.get() {
            toy.order(true);
            wait_for(&mut reports, |health| !matches!(health, WorkerHealth::Running));
            shared.with_entry(0, |entry| entry.restart_at = entry.restart_at.map(|_| Instant::now()));
            wait_for(&mut reports, |health| matches!(health, WorkerHealth::Running | WorkerHealth::GaveUp { .. }));
        }
        assert!(toy.gave_up.load(Ordering::SeqCst), "the worker was given up on");

        watchdog.restart(&WorkerName::new("toy"));
        wait_for(&mut reports, |health| *health == WorkerHealth::Running);
    }

    /// A worker whose thread cannot be started again is given up on once its restarts run out,
    /// and the work waiting for it is failed.
    #[test]
    fn a_worker_that_cannot_be_started_again_is_given_up() {
        let watchdog = Watchdog::start_with_period(PERIOD).expect("the watchdog starts");
        let toy = Arc::new(Toy::default());
        let _registration = watchdog.register(spec(None), Arc::clone(&toy) as Arc<dyn Supervised>).expect("it starts");
        let shared = &watchdog.shared;
        let mut reports = watchdog.reports();

        toy.refuse_starts.store(true, Ordering::SeqCst);
        toy.order(true);
        wait_for(&mut reports, |health| !matches!(health, WorkerHealth::Running));
        // Each refused start waits out the policy's next delay, which is skipped here.
        for _ in 0..200 {
            let given_up = shared.with_entry(0, |entry| match entry.health {
                WorkerHealth::GaveUp { .. } => true,
                _ => {
                    entry.restart_at = entry.restart_at.map(|_| Instant::now());
                    false
                }
            });
            if given_up == Some(true) {
                break;
            }
            std::thread::sleep(PERIOD);
        }

        assert!(toy.gave_up.load(Ordering::SeqCst), "the worker was given up on and its work failed");
    }

    /// A quiet heartbeat is reported as a stall, which clears when it beats again.
    #[test]
    fn a_quiet_worker_is_reported_stalled_until_it_beats() {
        let watchdog = Watchdog::start_with_period(PERIOD).expect("the watchdog starts");
        let toy = Arc::new(Toy::default());
        let registration = watchdog.register(spec(Some(PERIOD * 3)), Arc::clone(&toy) as Arc<dyn Supervised>);
        let _registration = registration.expect("it starts");
        let mut reports = watchdog.reports();

        toy.order(false);
        wait_for(&mut reports, |health| *health == WorkerHealth::Stalled);
        toy.order(false);
        wait_for(&mut reports, |health| *health == WorkerHealth::Running);
        assert_eq!(toy.starts.load(Ordering::SeqCst), 1, "a stalled worker is not started again");
    }

    /// A watchdog thread that stopped is started again by its owner, and goes on finding stopped
    /// workers.
    #[test]
    fn a_stopped_watchdog_is_started_again_by_its_owner() {
        let watchdog = Watchdog::start_with_period(PERIOD).expect("the watchdog starts");
        let toy = Arc::new(Toy::default());
        let _registration = watchdog.register(spec(None), Arc::clone(&toy) as Arc<dyn Supervised>).expect("it starts");
        let mut reports = watchdog.reports();

        watchdog.stop_for_test();
        while !lock(&watchdog.shared.thread).as_ref().is_some_and(JoinHandle::is_finished) {
            std::thread::sleep(PERIOD);
        }
        assert!(watchdog.ensure_running(), "the owner starts it again");
        assert!(!watchdog.ensure_running(), "and only once");

        toy.order(true);
        wait_for(&mut reports, |health| matches!(health, WorkerHealth::Restarting { .. }));
    }
}
