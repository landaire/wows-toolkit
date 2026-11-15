//! The MCP server's thread: a tokio runtime of its own serving one bound address, under the
//! watchdog, which starts it again if it panics.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::thread::JoinHandle;
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use wows_mcp::BuildError;
use wows_mcp::GameDataSource;
use wows_mcp::HttpEndpoint;
use wows_mcp::McpServer;
use wows_mcp::ReplayFiles;
use wows_mcp::Transport;
use wows_query::GameData;
use wows_toolkit_services::supervision::WorkerStop;
use wows_toolkit_services::supervision::watchdog::Heartbeat;
use wows_toolkit_services::supervision::watchdog::Registration;
use wows_toolkit_services::supervision::watchdog::Supervised;
use wows_toolkit_services::supervision::watchdog::Watchdog;
use wows_toolkit_services::supervision::watchdog::WatchdogError;
use wows_toolkit_services::supervision::watchdog::WorkerName;
use wows_toolkit_services::supervision::watchdog::WorkerSpec;

use crate::services::mcp::tools::ReplayTools;
use crate::services::mcp::tools::ToolContext;

/// Which start of the server an event is about, so one from a server already replaced is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Generation(u64);

impl Generation {
    pub const FIRST: Self = Self(0);

    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// What one start of the server serves.
pub struct McpLaunch {
    pub generation: Generation,
    pub address: SocketAddr,
    /// The app's loaded build, indexed for the game data tools. `None` while the app has none.
    pub game_data: watch::Receiver<Option<Arc<GameData>>>,
    pub context: watch::Receiver<ToolContext>,
    pub events: UnboundedSender<McpEvent>,
    /// The server this one replaces. It is waited for, so its listener is closed before this one
    /// binds the same address.
    pub replaces: Option<Arc<Finished>>,
}

/// What the server's thread reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum McpEvent {
    Started {
        generation: Generation,
        endpoint: HttpEndpoint,
    },
    Failed {
        generation: Generation,
        failure: StartFailure,
    },
    /// A started server stopped serving.
    Stopped {
        generation: Generation,
    },
}

impl McpEvent {
    pub const fn generation(&self) -> Generation {
        match self {
            Self::Started { generation, .. } | Self::Failed { generation, .. } | Self::Stopped { generation } => {
                *generation
            }
        }
    }
}

/// Why the server is not serving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartFailure {
    /// Another program holds the address.
    PortInUse {
        address: SocketAddr,
    },
    Listen {
        address: SocketAddr,
        reason: String,
    },
    /// The tokio runtime or the thread could not be started.
    Runtime {
        reason: String,
    },
    /// The watchdog stopped restarting a server that kept failing.
    GaveUp {
        reason: String,
    },
}

/// Set when a server's thread has ended, panic included.
#[derive(Default)]
pub struct Finished {
    done: Mutex<bool>,
    ended: Condvar,
}

impl Finished {
    fn set(&self) {
        *self.done.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.ended.notify_all();
    }

    /// Waits up to `timeout` for the thread to end. Whether it did.
    pub fn wait(&self, timeout: Duration) -> bool {
        let done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        let (done, _) =
            self.ended.wait_timeout_while(done, timeout, |done| !*done).unwrap_or_else(PoisonError::into_inner);

        *done
    }
}

/// The running server. Dropping it asks the server to stop and returns at once.
pub struct McpWorker {
    /// `None` once dropped, which is done before the server is told to stop so its end is not
    /// taken for a crash.
    registration: Option<Registration>,
    shared: Arc<WorkerShared>,
}

impl McpWorker {
    /// The server's name under the watchdog and on its thread.
    pub const WORKER: &str = "mcp-server";

    pub fn start(watchdog: &Watchdog, launch: McpLaunch) -> Result<Self, WatchdogError> {
        // The runtime beats every five seconds; half a minute without one is a runtime whose
        // workers are all blocked.
        const STALL_AFTER: Duration = Duration::from_secs(30);

        let shared = Arc::new(WorkerShared {
            launch,
            shutdown: CancellationToken::new(),
            finished: Arc::new(Finished::default()),
        });
        let spec = WorkerSpec { name: WorkerName::new(Self::WORKER), stall_after: Some(STALL_AFTER) };
        let registration = watchdog.register(spec, Arc::new(McpSupervisor { shared: Arc::clone(&shared) }))?;

        Ok(Self { registration: Some(registration), shared })
    }

    pub fn generation(&self) -> Generation {
        self.shared.launch.generation
    }

    /// What a successor waits on before it binds.
    pub fn finished(&self) -> Arc<Finished> {
        Arc::clone(&self.shared.finished)
    }
}

impl Drop for McpWorker {
    fn drop(&mut self) {
        drop(self.registration.take());
        self.shared.shutdown.cancel();
    }
}

struct WorkerShared {
    launch: McpLaunch,
    shutdown: CancellationToken,
    finished: Arc<Finished>,
}

impl WorkerShared {
    fn report(&self, event: McpEvent) {
        // The receiver is gone once the window has closed, and nothing is left to tell.
        let _ = self.launch.events.unbounded_send(event);
    }
}

/// The server as the watchdog sees it.
struct McpSupervisor {
    shared: Arc<WorkerShared>,
}

impl Supervised for McpSupervisor {
    fn start(&self, heartbeat: Heartbeat) -> std::io::Result<JoinHandle<()>> {
        let shared = Arc::clone(&self.shared);
        std::thread::Builder::new().name(McpWorker::WORKER.to_owned()).spawn(move || {
            let finished = FinishOnDrop(Arc::clone(&shared.finished));
            serve(&shared, heartbeat);
            drop(finished);
        })
    }

    /// The next thread binds again; the watchdog's report says it restarted.
    fn stopped(&self, _stop: &WorkerStop) {}

    fn gave_up(&self, stop: &WorkerStop) {
        let generation = self.shared.launch.generation;
        self.shared.report(McpEvent::Failed { generation, failure: StartFailure::GaveUp { reason: stop.to_string() } });
    }
}

/// Marks the thread ended however it ends.
struct FinishOnDrop(Arc<Finished>);

impl Drop for FinishOnDrop {
    fn drop(&mut self) {
        self.0.set();
    }
}

/// Builds the runtime and serves until asked to stop.
fn serve(shared: &WorkerShared, heartbeat: Heartbeat) {
    // The predecessor's drain is bounded at five seconds in wows-mcp.
    const PREDECESSOR_WAIT: Duration = Duration::from_secs(7);
    const RUNTIME_DRAIN: Duration = Duration::from_secs(2);

    let launch = &shared.launch;
    if let Some(replaced) = &launch.replaces
        && !replaced.wait(PREDECESSOR_WAIT)
    {
        tracing::warn!("the previous MCP server has not stopped; binding anyway");
    }

    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("mcp-runtime").enable_all().build();
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(error) => {
            shared.report(McpEvent::Failed {
                generation: launch.generation,
                failure: StartFailure::Runtime { reason: error.to_string() },
            });
            return;
        }
    };

    runtime.block_on(async {
        // Proves the runtime still schedules work; a stall is reported on the status bar.
        const BEAT: Duration = Duration::from_secs(5);

        let beating = tokio::spawn(async move {
            loop {
                heartbeat.beat();
                tokio::time::sleep(BEAT).await;
            }
        });

        match build(launch).await {
            Ok(server) => {
                let endpoint = server.http_endpoint().expect("an HTTP server has an endpoint");
                shared.report(McpEvent::Started { generation: launch.generation, endpoint });
                if let Err(report) = server.run(shared.shutdown.clone()).await {
                    tracing::error!(%report, "the MCP server failed");
                }
                shared.report(McpEvent::Stopped { generation: launch.generation });
            }
            Err(failure) => {
                shared.report(McpEvent::Failed { generation: launch.generation, failure });
                // Held until stopped, so the watchdog does not start a server that cannot bind.
                shared.shutdown.cancelled().await;
            }
        }

        beating.abort();
    });
    runtime.shutdown_timeout(RUNTIME_DRAIN);
}

async fn build(launch: &McpLaunch) -> Result<McpServer, StartFailure> {
    let tools = ReplayTools::new(launch.context.clone());
    // Replays are reached through the replay tools, which confine paths to the replay folders;
    // replay_summary reads only what those folders hold now too.
    let context = launch.context.clone();
    let roots = ReplayFiles::Current(Arc::new(move || context.borrow().replay_roots()));
    let builder = McpServer::builder(Transport::Http(launch.address))
        .tool_set(Arc::new(tools))
        .game_data(GameDataSource::Shared(launch.game_data.clone()), roots);

    builder.build().await.map_err(|report| {
        let address = launch.address;
        match report.current_context() {
            BuildError::Listen { source, .. } if source.kind() == std::io::ErrorKind::AddrInUse => {
                StartFailure::PortInUse { address }
            }
            BuildError::Listen { source, .. } => StartFailure::Listen { address, reason: source.to_string() },
            BuildError::DuplicateTool { name } => {
                StartFailure::Runtime { reason: format!("two tool sets both provide {name}") }
            }
        }
    })
}
