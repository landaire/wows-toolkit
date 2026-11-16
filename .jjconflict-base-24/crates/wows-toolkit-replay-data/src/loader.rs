//! The replay loader: one supervised thread that reads the replays a front end opens, in the
//! order asked.
//!
//! Opening a replay only queues a [`LoadRequest`]; the answer arrives through a future, so the
//! front end never blocks on the loader or joins it. A panic in one parse is that replay's
//! [`ReplayLoadError::Panicked`]. Anything that stops the thread anyway is the watchdog's: it
//! answers the replay being read with [`ReplayLoadError::LoaderStopped`] and starts another
//! thread, which reads the replays still waiting.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::channel::mpsc::UnboundedReceiver;
use wows_toolkit_services::supervision::WorkerStop;
use wows_toolkit_services::supervision::queue::Activity;
use wows_toolkit_services::supervision::queue::QueueClosed;
use wows_toolkit_services::supervision::queue::SupervisedQueue;
use wows_toolkit_services::supervision::queue::Work;
use wows_toolkit_services::supervision::watchdog::Watchdog;
use wows_toolkit_services::supervision::watchdog::WatchdogError;
use wows_toolkit_services::supervision::watchdog::WorkerName;
use wows_toolkit_services::supervision::watchdog::WorkerSpec;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;

use crate::error::ReplayLoadError;
use crate::load::GameDataCache;
use crate::load::ParsedReplay;
use crate::load::parse_replay_with_alts;

/// One replay to read, with the other recordings of its battle read alongside it.
pub struct LoadRequest {
    pub path: PathBuf,
    pub alts: Vec<PathBuf>,
    pub game_data: GameDataCache,
    pub personal_rating: Option<Arc<PersonalRatingData>>,
}

/// A handle on the replay loader. Cheap to clone; the loader ends when the last handle goes.
#[derive(Clone)]
pub struct ReplayLoader {
    queue: SupervisedQueue<ReadReplays>,
}

impl ReplayLoader {
    /// The loader's name under the watchdog and on its thread.
    pub const WORKER: &str = "replay-loader";

    /// Starts the loader's thread under `watchdog`.
    pub fn start(watchdog: &Watchdog) -> Result<Self, WatchdogError> {
        // A long replay reads in seconds; a minute without a beat is a loader that is stuck.
        const STALL_AFTER: Duration = Duration::from_secs(60);

        let spec = WorkerSpec { name: WorkerName::new(Self::WORKER), stall_after: Some(STALL_AFTER) };
        let queue = SupervisedQueue::start(watchdog, spec, ReadReplays)?;

        Ok(Self { queue })
    }

    /// Queues `request`. The returned future answers with the read.
    pub fn read(
        &self,
        request: LoadRequest,
    ) -> impl Future<Output = Result<ParsedReplay, ReplayLoadError>> + Send + use<> {
        let path = request.path.clone();
        let reply = self.queue.request(request);

        async move { reply.await.unwrap_or_else(|QueueClosed| Err(ReplayLoadError::LoaderGone { path })) }
    }

    /// The replays being read and waiting now, then again each time that changes.
    pub fn activity(&self) -> UnboundedReceiver<Activity<PathBuf>> {
        self.queue.activity()
    }
}

struct ReadReplays;

impl Work for ReadReplays {
    type Request = LoadRequest;
    type Response = Result<ParsedReplay, ReplayLoadError>;
    type Subject = PathBuf;

    fn subject(&self, request: &LoadRequest) -> PathBuf {
        request.path.clone()
    }

    fn run(&self, request: &LoadRequest) -> Self::Response {
        #[cfg(any(test, feature = "test-support"))]
        crate::simulated_panic::stop_loader_if_asked(&request.path);

        parse_replay_with_alts(&request.path, &request.alts, &request.game_data, request.personal_rating.as_deref())
    }

    fn stopped(&self, request: &LoadRequest, stop: &WorkerStop) -> Self::Response {
        Err(ReplayLoadError::LoaderStopped { path: request.path.clone(), stop: stop.clone() })
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;

    fn loader() -> (Watchdog, ReplayLoader, tempfile::TempDir) {
        let watchdog = Watchdog::start_with_period(Duration::from_millis(20)).expect("the watchdog starts");
        let loader = ReplayLoader::start(&watchdog).expect("the loader starts");

        (watchdog, loader, tempfile::tempdir().expect("a temporary directory"))
    }

    fn request(dir: &tempfile::TempDir, name: &str) -> LoadRequest {
        let path = dir.path().join(name);
        std::fs::write(&path, b"not a replay").expect("the file is written");

        LoadRequest {
            path,
            alts: Vec::new(),
            game_data: GameDataCache::new(dir.path().to_path_buf()),
            personal_rating: None,
        }
    }

    /// A parse that panics is that replay's error, and the loader goes on reading.
    #[test]
    fn a_panicking_parse_is_its_replays_error_and_the_loader_goes_on() {
        let (_watchdog, loader, dir) = loader();
        let faulting = request(&dir, "faulting.wowsreplay");
        crate::simulated_panic::panic_on_parse(&faulting.path);

        let read = block_on(loader.read(faulting));
        assert!(
            matches!(read, Err(ReplayLoadError::Panicked { .. })),
            "the panic is the read's error: {:?}",
            read.err()
        );

        let next = block_on(loader.read(request(&dir, "next.wowsreplay")));
        assert!(matches!(next, Err(ReplayLoadError::Parse(_))), "the next replay is read: {:?}", next.err());
    }

    /// A loader thread that stops is started again: the replay it was reading fails, and the
    /// replays waiting behind it are read by the new thread.
    #[test]
    fn a_stopped_loader_fails_its_replay_and_reads_the_rest() {
        let (_watchdog, loader, dir) = loader();
        let stopping = request(&dir, "stopping.wowsreplay");
        let stopping_path = stopping.path.clone();
        crate::simulated_panic::stop_loader_on(&stopping_path);

        let stopped = loader.read(stopping);
        let waiting = loader.read(request(&dir, "waiting.wowsreplay"));

        match block_on(stopped) {
            Err(ReplayLoadError::LoaderStopped { path, stop: WorkerStop::Panicked(report) }) => {
                assert_eq!(path, stopping_path);
                assert!(report.to_string().contains("a simulated loader fault"), "the stop names the panic: {report}");
            }
            other => panic!("the replay being read fails with the stop: {:?}", other.err()),
        }
        let read = block_on(waiting);
        assert!(matches!(read, Err(ReplayLoadError::Parse(_))), "the waiting replay is read: {:?}", read.err());
    }
}
