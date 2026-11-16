//! A worker thread that answers requests in the order they are made, supervised by the
//! [`Watchdog`].
//!
//! A request is queued and answered through a future, so a caller on a UI thread never blocks on
//! the worker or joins it. When the thread stops, the watchdog answers the request it stopped on
//! through [`Work::stopped`] and starts another thread, which takes the requests still waiting.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::task::Context;
use std::task::Poll;
use std::thread::JoinHandle;
use std::time::Duration;

use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;

use super::WorkerStop;
use super::watchdog::Heartbeat;
use super::watchdog::Registration;
use super::watchdog::Supervised;
use super::watchdog::Watchdog;
use super::watchdog::WatchdogError;
use super::watchdog::WorkerSpec;

/// What a [`SupervisedQueue`] does with each request.
pub trait Work: Send + Sync + 'static {
    type Request: Send + Sync + 'static;
    type Response: Send + 'static;
    /// What [`Activity`] names a request by.
    type Subject: Clone + Send + 'static;

    fn subject(&self, request: &Self::Request) -> Self::Subject;

    /// Answers `request`, on the worker thread. A panic here stops the thread.
    fn run(&self, request: &Self::Request) -> Self::Response;

    /// The answer to `request`, which the worker thread stopped on, or which was waiting when
    /// the watchdog gave the worker up.
    fn stopped(&self, request: &Self::Request, stop: &WorkerStop) -> Self::Response;
}

/// What a queue holds: the request being answered, then those waiting, oldest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity<S> {
    pub working: Option<S>,
    pub waiting: Vec<S>,
}

impl<S> Activity<S> {
    /// Every request held, the one being answered first.
    pub fn subjects(&self) -> impl Iterator<Item = &S> {
        self.working.iter().chain(&self.waiting)
    }

    pub fn is_idle(&self) -> bool {
        self.working.is_none() && self.waiting.is_empty()
    }
}

/// The queue was shut down before it answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the worker was shut down before it answered")]
pub struct QueueClosed;

/// A handle on a supervised queue. The worker ends once every handle is dropped and the request
/// being answered is done; requests still waiting are answered with [`QueueClosed`].
pub struct SupervisedQueue<W: Work> {
    owner: Arc<Owner<W>>,
}

impl<W: Work> Clone for SupervisedQueue<W> {
    fn clone(&self) -> Self {
        Self { owner: Arc::clone(&self.owner) }
    }
}

impl<W: Work> SupervisedQueue<W> {
    /// Starts the worker thread under `watchdog`.
    pub fn start(watchdog: &Watchdog, spec: WorkerSpec, work: W) -> Result<Self, WatchdogError> {
        let shared = Arc::new(Shared {
            name: spec.name.to_string(),
            work,
            state: Mutex::new(State {
                waiting: VecDeque::new(),
                working: None,
                closed: false,
                given_up: None,
                watchers: Vec::new(),
            }),
            arrived: Condvar::new(),
        });
        let registration = watchdog.register(spec, Arc::new(Supervisor { shared: Arc::clone(&shared) }))?;

        Ok(Self { owner: Arc::new(Owner { registration: Some(registration), shared }) })
    }

    /// Queues `request`. The answer arrives through the returned future. While the watchdog has
    /// given the worker up, the request is answered at once through [`Work::stopped`].
    pub fn request(&self, request: W::Request) -> Reply<W::Response> {
        let shared = &self.owner.shared;
        let (reply, answer) = oneshot::channel();
        let mut state = shared.lock();
        // A closed queue drops `reply`, which answers the request with `QueueClosed`.
        if state.closed {
            return Reply { answer };
        }
        if let Some(stop) = state.given_up.clone() {
            drop(state);
            let _ = reply.send(shared.work.stopped(&request, &stop));
            return Reply { answer };
        }
        state.waiting.push_back(Pending { request: Arc::new(request), reply });
        shared.publish(&mut state);
        shared.arrived.notify_one();

        Reply { answer }
    }

    /// What the queue holds now, then again each time that changes.
    pub fn activity(&self) -> UnboundedReceiver<Activity<W::Subject>> {
        let shared = &self.owner.shared;
        let (watcher, activity) = futures::channel::mpsc::unbounded();
        let mut state = shared.lock();
        let _ = watcher.unbounded_send(shared.snapshot(&state));
        state.watchers.push(watcher);

        activity
    }
}

/// The answer to one request.
#[must_use = "a reply does nothing unless it is awaited"]
pub struct Reply<T> {
    answer: oneshot::Receiver<T>,
}

impl<T> Future for Reply<T> {
    type Output = Result<T, QueueClosed>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.answer).poll(cx).map(|answer| answer.map_err(|oneshot::Canceled| QueueClosed))
    }
}

/// Closes the queue when the last handle goes.
struct Owner<W: Work> {
    /// `None` once dropped, which is done before the worker is told to end so its end is not
    /// taken for a stop.
    registration: Option<Registration>,
    shared: Arc<Shared<W>>,
}

impl<W: Work> Drop for Owner<W> {
    fn drop(&mut self) {
        drop(self.registration.take());

        let mut state = self.shared.lock();
        state.closed = true;
        state.waiting.clear();
        self.shared.publish(&mut state);
        self.shared.arrived.notify_all();
    }
}

/// The queue as the watchdog sees it.
struct Supervisor<W: Work> {
    shared: Arc<Shared<W>>,
}

impl<W: Work> Supervised for Supervisor<W> {
    fn start(&self, heartbeat: Heartbeat) -> std::io::Result<JoinHandle<()>> {
        let shared = Arc::clone(&self.shared);
        std::thread::Builder::new().name(self.shared.name.clone()).spawn(move || serve(&shared, &heartbeat))
    }

    fn stopped(&self, stop: &WorkerStop) {
        let abandoned = {
            let mut state = self.shared.lock();
            let abandoned = state.working.take();
            self.shared.publish(&mut state);
            abandoned
        };
        if let Some(abandoned) = abandoned {
            let _ = abandoned.reply.send(self.shared.work.stopped(&abandoned.request, stop));
        }
    }

    fn gave_up(&self, stop: &WorkerStop) {
        let waiting = {
            let mut state = self.shared.lock();
            state.given_up = Some(stop.clone());
            let waiting = std::mem::take(&mut state.waiting);
            self.shared.publish(&mut state);
            waiting
        };
        for pending in waiting {
            let _ = pending.reply.send(self.shared.work.stopped(&pending.request, stop));
        }
    }
}

struct Shared<W: Work> {
    name: String,
    work: W,
    state: Mutex<State<W>>,
    arrived: Condvar,
}

impl<W: Work> Shared<W> {
    /// No thread panics while holding the lock, so a poisoned lock still holds a whole state.
    fn lock(&self) -> MutexGuard<'_, State<W>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn snapshot(&self, state: &State<W>) -> Activity<W::Subject> {
        Activity {
            working: state.working.as_ref().map(|pending| self.work.subject(&pending.request)),
            waiting: state.waiting.iter().map(|pending| self.work.subject(&pending.request)).collect(),
        }
    }

    fn publish(&self, state: &mut State<W>) {
        let activity = self.snapshot(state);
        state.watchers.retain(|watcher| watcher.unbounded_send(activity.clone()).is_ok());
    }
}

struct State<W: Work> {
    waiting: VecDeque<Pending<W>>,
    working: Option<Pending<W>>,
    closed: bool,
    /// Why the watchdog gave the worker up, until a restart starts it again.
    given_up: Option<WorkerStop>,
    watchers: Vec<UnboundedSender<Activity<W::Subject>>>,
}

struct Pending<W: Work> {
    request: Arc<W::Request>,
    reply: oneshot::Sender<W::Response>,
}

/// Answers requests until the queue is closed and nothing waits, beating `heartbeat` while idle
/// and before each request. A request whose caller stopped waiting is dropped unanswered.
fn serve<W: Work>(shared: &Shared<W>, heartbeat: &Heartbeat) {
    const IDLE_BEAT: Duration = Duration::from_millis(500);

    shared.lock().given_up = None;
    loop {
        let request = {
            let mut state = shared.lock();
            loop {
                heartbeat.beat();
                let held = state.waiting.len();
                state.waiting.retain(|pending| !pending.reply.is_canceled());
                let dropped = state.waiting.len() != held;
                if let Some(next) = state.waiting.pop_front() {
                    let request = Arc::clone(&next.request);
                    state.working = Some(next);
                    shared.publish(&mut state);
                    break request;
                }
                if dropped {
                    shared.publish(&mut state);
                }
                if state.closed {
                    return;
                }
                state = shared.arrived.wait_timeout(state, IDLE_BEAT).unwrap_or_else(PoisonError::into_inner).0;
            }
        };

        let response = shared.work.run(&request);

        let mut state = shared.lock();
        if let Some(done) = state.working.take() {
            // The caller may have stopped waiting, which is theirs to do.
            let _ = done.reply.send(response);
        }
        shared.publish(&mut state);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::mpsc::Receiver;
    use std::sync::mpsc::SyncSender;

    use futures::StreamExt;
    use futures::executor::block_on;

    use super::*;
    use crate::supervision::watchdog::WorkerName;

    /// Doubles a number, noting each it is asked. Panics on 13, once let through.
    struct Doubling {
        gate: Mutex<Receiver<()>>,
        ran: Mutex<Vec<u32>>,
    }

    impl Work for Doubling {
        type Request = u32;
        type Response = Result<u32, WorkerStop>;
        type Subject = u32;

        fn subject(&self, request: &u32) -> u32 {
            *request
        }

        fn run(&self, request: &u32) -> Result<u32, WorkerStop> {
            self.ran.lock().unwrap_or_else(PoisonError::into_inner).push(*request);
            if *request == 13 {
                let _ = self.gate.lock().unwrap_or_else(PoisonError::into_inner).recv();
                panic!("unlucky");
            }
            Ok(request * 2)
        }

        fn stopped(&self, _request: &u32, stop: &WorkerStop) -> Result<u32, WorkerStop> {
            Err(stop.clone())
        }
    }

    fn doubling() -> (Watchdog, SupervisedQueue<Doubling>, SyncSender<()>) {
        let watchdog = Watchdog::start_with_period(Duration::from_millis(20)).expect("the watchdog starts");
        let (open, gate) = mpsc::sync_channel(1);
        let spec = WorkerSpec { name: WorkerName::new("test-doubling"), stall_after: None };
        let doubling = Doubling { gate: Mutex::new(gate), ran: Mutex::new(Vec::new()) };
        let queue = SupervisedQueue::start(&watchdog, spec, doubling).expect("it starts");

        (watchdog, queue, open)
    }

    #[test]
    fn requests_are_answered_in_order_and_the_activity_goes_idle() {
        let (_watchdog, queue, _open) = doubling();
        let mut activity = queue.activity();
        assert!(block_on(activity.next()).expect("a first report").is_idle());

        let replies: Vec<Reply<Result<u32, WorkerStop>>> = (1..=3).map(|n| queue.request(n)).collect();
        let answered: Vec<Result<u32, WorkerStop>> =
            replies.into_iter().map(|reply| block_on(reply).expect("the worker answers")).collect();

        assert_eq!(answered, [Ok(2), Ok(4), Ok(6)]);
        let mut held = Vec::new();
        while let Some(report) = block_on(activity.next()) {
            if report.is_idle() {
                break;
            }
            held.push(report.subjects().count());
        }
        assert!(held.contains(&1), "the activity named what it held: {held:?}");
    }

    /// A worker thread that panics is started again by the watchdog. The request it stopped on
    /// is answered with the panic, and those waiting behind it are answered by the new thread.
    #[test]
    fn a_stopped_worker_is_started_again_and_the_waiting_requests_are_answered() {
        let (_watchdog, queue, open) = doubling();
        let unlucky = queue.request(13);
        let waiting: Vec<Reply<Result<u32, WorkerStop>>> = [1, 2].into_iter().map(|n| queue.request(n)).collect();
        open.send(()).expect("the gate opens");

        let stopped = block_on(unlucky).expect("the watchdog answers the request the worker stopped on");
        let Err(WorkerStop::Panicked(report)) = stopped else {
            panic!("the request fails with the panic: {stopped:?}")
        };
        assert!(report.to_string().contains("unlucky"), "the report names the panic: {report}");
        let answered: Vec<Result<u32, WorkerStop>> =
            waiting.into_iter().map(|reply| block_on(reply).expect("the new worker answers")).collect();
        assert_eq!(answered, [Ok(2), Ok(4)]);
        assert_eq!(block_on(queue.request(5)).expect("it keeps answering"), Ok(10));
    }

    /// Requests still waiting when the last handle goes are answered as closed.
    #[test]
    fn dropping_the_queue_closes_what_waits() {
        let (_watchdog, queue, open) = doubling();
        let blocked = queue.request(13);
        let waiting = queue.request(1);
        drop(queue);

        assert_eq!(block_on(waiting), Err(QueueClosed));
        open.send(()).expect("the gate opens");
        drop(blocked);
    }

    /// Once the watchdog gives the worker up, a request is answered at once with why, rather than
    /// waiting for a worker that is not coming.
    #[test]
    fn a_request_after_the_worker_was_given_up_is_answered_at_once() {
        let (_watchdog, queue, _open) = doubling();
        Supervisor { shared: Arc::clone(&queue.owner.shared) }.gave_up(&WorkerStop::Exited);

        assert_eq!(block_on(queue.request(1)), Ok(Err(WorkerStop::Exited)));
    }

    /// A request whose caller stopped waiting is not run.
    #[test]
    fn a_request_nobody_waits_for_is_dropped() {
        let (_watchdog, queue, open) = doubling();
        let unlucky = queue.request(13);
        drop(queue.request(1));
        let kept = queue.request(2);
        open.send(()).expect("the gate opens");

        assert!(block_on(unlucky).expect("the stop is answered").is_err());
        assert_eq!(block_on(kept).expect("the new worker answers"), Ok(4));
        let ran = queue.owner.shared.work.ran.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert_eq!(ran, [13, 2], "the dropped request was not run");
    }
}
