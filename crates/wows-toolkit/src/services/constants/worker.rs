//! The constants worker: one supervised thread that brings builds' constants up to date in the
//! order asked, so a generation never runs on the UI thread and two never run at once.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::Instant;

use futures::channel::mpsc::UnboundedSender;
use serde_json::Value;
use wows_data_mgr::constants::ConstantsFetcher;
use wows_toolkit_services::supervision::WorkerStop;
use wows_toolkit_services::supervision::queue::QueueClosed;
use wows_toolkit_services::supervision::queue::SupervisedQueue;
use wows_toolkit_services::supervision::queue::Work;
use wows_toolkit_services::supervision::watchdog::Watchdog;
use wows_toolkit_services::supervision::watchdog::WatchdogError;
use wows_toolkit_services::supervision::watchdog::WorkerName;
use wows_toolkit_services::supervision::watchdog::WorkerSpec;
use wowsdeob::constants::BuildNumber;
use wowsdeob::constants::ClientBuild;
use wowsdeob::constants::HostAccess;

use crate::services::constants::acquire::GenerationError;
use crate::services::constants::acquire::Missed;
use crate::services::constants::acquire::Outcome;
use crate::services::constants::acquire::Published;
use crate::services::constants::acquire::Sources;
use crate::services::constants::acquire::Step;
use crate::services::constants::acquire::Wanted;
use crate::services::constants::acquire::acquire;
use crate::services::constants::acquire::is_current;
use crate::services::constants::acquire::kept_or_embedded;

/// One build to bring up to date.
pub struct ConstantsRequest {
    pub wanted: Wanted,
    /// The game install, whose client the constants are generated from.
    pub install: Option<PathBuf>,
    /// Where [`Step`]s are reported. Unbounded, so a report never holds up the work.
    pub steps: UnboundedSender<Step>,
}

/// A handle on the constants worker. Cheap to clone; the worker ends when the last handle goes.
#[derive(Clone)]
pub struct ConstantsWorker {
    queue: SupervisedQueue<UpdateConstants>,
}

impl ConstantsWorker {
    /// The worker's name under the watchdog and on its thread.
    pub const WORKER: &str = "constants-worker";

    /// Starts the worker's thread under `watchdog`.
    pub fn start(watchdog: &Watchdog) -> Result<Self, WatchdogError> {
        // The worker beats between builds. One generation takes tens of seconds, so ten minutes
        // without a beat is one that is stuck.
        const STALL_AFTER: Duration = Duration::from_secs(600);

        let spec = WorkerSpec { name: WorkerName::new(Self::WORKER), stall_after: Some(STALL_AFTER) };
        let queue = SupervisedQueue::start(watchdog, spec, UpdateConstants { fetcher: Mutex::new(None) })?;

        Ok(Self { queue })
    }

    /// Queues `request`. The returned future answers with what became of the build.
    pub fn update(&self, request: ConstantsRequest) -> impl Future<Output = Outcome> + use<> {
        let build = request.wanted.build;
        let reply = self.queue.request(request);

        async move {
            reply.await.unwrap_or_else(|closed: QueueClosed| Outcome::Fallback {
                fallback: kept_or_embedded(wows_toolkit_services::constants::is_cached(build)),
                missed: Missed::WorkerStopped { reason: closed.to_string() },
            })
        }
    }
}

/// A GitHub client with the published manifest and build list it read, or why it could not be
/// made, kept between builds so a sweep asks for those once.
struct Fetcher {
    made: Instant,
    fetcher: Result<ConstantsFetcher, String>,
}

struct UpdateConstants {
    fetcher: Mutex<Option<Fetcher>>,
}

impl Work for UpdateConstants {
    type Request = ConstantsRequest;
    type Response = Outcome;
    type Subject = u32;

    fn subject(&self, request: &ConstantsRequest) -> u32 {
        request.wanted.build
    }

    fn run(&self, request: &ConstantsRequest) -> Outcome {
        let mut sources = Installed { fetcher: &self.fetcher, install: request.install.as_deref() };

        acquire(&request.wanted, &mut sources, |step| {
            let _ = request.steps.unbounded_send(step);
        })
    }

    fn stopped(&self, request: &ConstantsRequest, stop: &WorkerStop) -> Outcome {
        Outcome::Fallback {
            fallback: kept_or_embedded(wows_toolkit_services::constants::is_cached(request.wanted.build)),
            missed: Missed::WorkerStopped { reason: stop.to_string() },
        }
    }
}

/// The real sources: the shared storage cache, GitHub, and the game install.
struct Installed<'a> {
    fetcher: &'a Mutex<Option<Fetcher>>,
    install: Option<&'a Path>,
}

impl Sources for Installed<'_> {
    type Client = ClientBuild;

    fn cached(&self, build: u32) -> Option<Value> {
        read_cached(build)
    }

    fn fetch(&mut self, wanted: &Wanted) -> Result<Published, String> {
        // GitHub publishes a new build within days, so a manifest older than this is read again.
        const FRESH_FOR: Duration = Duration::from_secs(600);

        let mut held = self.fetcher.lock().unwrap_or_else(PoisonError::into_inner);
        let held = match held.take() {
            Some(fresh) if fresh.made.elapsed() <= FRESH_FOR => held.insert(fresh),
            _ => held.insert(Fetcher {
                made: Instant::now(),
                fetcher: ConstantsFetcher::new().map_err(|err| err.to_string()),
            }),
        };
        let fetcher = held.fetcher.as_ref().map_err(Clone::clone)?;

        let (constants, build) = fetcher
            .fetch(wanted.build, wanted.release.as_deref())
            .ok_or_else(|| "GitHub published nothing for it".to_owned())?;

        Ok(Published { constants, build })
    }

    /// Only an installed build with its executable: the executable tells which native modules the
    /// client links in, and without it the constants of current clients depend on modules nothing
    /// emulates. A game data dump carries the scripts alone.
    fn client(&self, build: u32) -> Option<ClientBuild> {
        let client = ClientBuild::find(self.install?, Some(BuildNumber::new(build))).ok()?;

        client.executable().is_some().then_some(client)
    }

    fn generate(&mut self, client: &ClientBuild) -> Result<Vec<u8>, GenerationError> {
        let extracted = std::panic::catch_unwind(|| wowsdeob::constants::extract(client, HostAccess::deny_all()))
            .map_err(|panic| GenerationError::Panicked { message: panic_message(panic.as_ref()) })??;

        for record in &extracted.incomplete_modules {
            if let Some(failure) = &record.failure {
                tracing::debug!(module = %record.module, %failure, "constants: a client module did not finish loading");
            }
        }

        Ok(extracted.constants?.render().into_bytes())
    }

    fn store(&mut self, build: u32, bytes: &[u8]) -> Result<(), String> {
        wows_toolkit_services::constants::write_cached(build, bytes)
    }
}

/// Whether the constants cached for `wanted` fit it. Reads a file, so not on the UI thread.
pub fn cached_current(wanted: &Wanted) -> bool {
    read_cached(wanted.build).is_some_and(|constants| is_current(&constants, wanted))
}

/// The constants cached for `build`, when a file that parses is there.
fn read_cached(build: u32) -> Option<Value> {
    let path = wows_toolkit_services::constants::cached_path(build)?;
    let bytes = std::fs::read(path).ok()?;

    serde_json::from_slice(&bytes).ok()
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        return (*message).to_owned();
    }
    if let Some(message) = panic.downcast_ref::<String>() {
        return message.clone();
    }

    "a panic without a message".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run with `WOWS_GAME_DIR=<install> cargo test -p wows-toolkit --release -- --ignored
    /// installed_client_generates`.
    #[test]
    #[ignore = "needs a World of Warships install named by WOWS_GAME_DIR"]
    fn installed_client_generates_constants_that_fit() {
        let install = PathBuf::from(std::env::var_os("WOWS_GAME_DIR").expect("WOWS_GAME_DIR names a game install"));
        let newest = ClientBuild::find(&install, None).expect("the install has a build with client scripts");
        let build = newest.build().get();
        let fetcher = Mutex::new(None);
        let sources = Installed { fetcher: &fetcher, install: Some(&install) };
        let client = sources.client(build).expect("the build has its executable");

        let mut sources = sources;
        let started = Instant::now();
        let bytes = sources.generate(&client).expect("the constants are generated");
        eprintln!("generated build {build} in {:?}", started.elapsed());

        let constants: Value = serde_json::from_slice(&bytes).expect("the constants are JSON");
        assert!(is_current(&constants, &Wanted { build, release: None }));
    }
}
