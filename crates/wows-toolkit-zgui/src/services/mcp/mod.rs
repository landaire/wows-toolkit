//! The in-app MCP server: whether it serves, the context its tools read, and the thread it runs on.
//!
//! [`Mcp`] owns at most one running server. Starting replaces the running one; its thread is
//! waited for before the new one binds, so a restart on the same port does not collide with
//! itself.

pub mod tools;
mod worker;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;
use tokio::sync::watch;
use wows_mcp::HttpEndpoint;
use wows_query::GameData;
use wows_toolkit_services::supervision::watchdog::Watchdog;
use wows_toolkit_services::supervision::watchdog::WatchdogError;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;

use crate::services::mcp::tools::ToolContext;
use crate::services::mcp::worker::Generation;
use crate::services::mcp::worker::McpLaunch;
use crate::services::mcp::worker::McpWorker;

pub use crate::services::mcp::worker::McpEvent;
pub use crate::services::mcp::worker::StartFailure;

/// What the server is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum McpStatus {
    Stopped,
    /// Its thread is binding the address.
    Starting {
        address: SocketAddr,
    },
    Running {
        endpoint: HttpEndpoint,
    },
    Failed {
        failure: StartFailure,
    },
}

impl McpStatus {
    /// Whether a server is started or starting, which a toggle stops.
    pub const fn is_on(&self) -> bool {
        matches!(self, Self::Starting { .. } | Self::Running { .. })
    }
}

/// The server's state, as one handle.
#[derive(Clone, Copy)]
pub struct Mcp {
    status: RwSignal<McpStatus, LocalStorage>,
    worker: StoredValue<Option<McpWorker>, LocalStorage>,
    /// The generation the next start takes.
    next: StoredValue<Generation>,
    context: StoredValue<watch::Sender<ToolContext>>,
    /// The app's loaded build as the game data tools read it.
    game_data: StoredValue<watch::Sender<Option<Arc<GameData>>>>,
    events: StoredValue<UnboundedSender<McpEvent>>,
    /// Taken once by the shell, which reports what the server's thread says.
    receiver: StoredValue<Option<UnboundedReceiver<McpEvent>>, LocalStorage>,
}

impl Mcp {
    pub fn new() -> Self {
        let (events, receiver) = futures::channel::mpsc::unbounded();
        let (context, _) = watch::channel(ToolContext::default());
        let (game_data, _) = watch::channel(None);

        Self {
            status: RwSignal::new_local(McpStatus::Stopped),
            worker: StoredValue::new_local(None),
            next: StoredValue::new(Generation::FIRST),
            context: StoredValue::new(context),
            game_data: StoredValue::new(game_data),
            events: StoredValue::new(events),
            receiver: StoredValue::new_local(Some(receiver)),
        }
    }

    /// What the server is doing, tracked.
    pub fn status(self) -> McpStatus {
        self.status.get()
    }

    pub fn status_untracked(self) -> McpStatus {
        self.status.get_untracked()
    }

    /// Replaces what the tools read. A running server sees it on its next call.
    pub fn publish(self, context: ToolContext) {
        self.context.with_value(|sender| {
            sender.send_replace(context);
        });
    }

    /// The build the game data tools read now.
    pub fn shared_build(self) -> Option<Arc<GameData>> {
        self.game_data.with_value(|sender| sender.borrow().clone())
    }

    /// Replaces the build the game data tools read.
    pub fn share_build(self, build: Option<Arc<GameData>>) {
        self.game_data.with_value(|sender| {
            sender.send_replace(build);
        });
    }

    /// The events of every server started from here. `None` once taken.
    pub fn take_events(self) -> Option<UnboundedReceiver<McpEvent>> {
        self.receiver.try_update_value(Option::take).flatten()
    }

    /// Stops any running server and starts one on `address`.
    pub fn start(self, watchdog: &Watchdog, address: SocketAddr) -> Result<(), WatchdogError> {
        let replaced = self.worker.try_update_value(Option::take).flatten();
        let replaces = replaced.as_ref().map(McpWorker::finished);
        drop(replaced);

        let generation = self.next.get_value();
        self.next.set_value(generation.next());
        let launch = McpLaunch {
            generation,
            address,
            game_data: self.game_data.with_value(watch::Sender::subscribe),
            context: self.context.with_value(watch::Sender::subscribe),
            events: self.events.with_value(UnboundedSender::clone),
            replaces,
        };
        match McpWorker::start(watchdog, launch) {
            Ok(worker) => {
                self.worker.set_value(Some(worker));
                self.status.set(McpStatus::Starting { address });
                Ok(())
            }
            Err(error) => {
                self.status.set(McpStatus::Stopped);
                Err(error)
            }
        }
    }

    /// Asks the running server to stop, and returns at once.
    pub fn stop(self) {
        drop(self.worker.try_update_value(Option::take).flatten());
        self.status.try_set(McpStatus::Stopped);
    }

    /// Stops the running server and waits up to `timeout` for its thread to end, so open
    /// connections are closed before the process exits.
    pub fn shut_down(self, timeout: Duration) {
        let Some(worker) = self.worker.try_update_value(Option::take).flatten() else {
            return;
        };

        let finished = worker.finished();
        drop(worker);
        if !finished.wait(timeout) {
            tracing::warn!("the MCP server did not stop within {timeout:?}");
        }
    }

    /// Takes in `event` from the server's thread. The event back when it is about the running
    /// server, `None` when that server has since been replaced or stopped.
    pub fn receive(self, event: McpEvent) -> Option<McpEvent> {
        let current = self.worker.with_value(|worker| worker.as_ref().map(McpWorker::generation));
        if current != Some(event.generation()) {
            return None;
        }

        let status = match &event {
            McpEvent::Started { endpoint, .. } => McpStatus::Running { endpoint: *endpoint },
            McpEvent::Failed { failure, .. } => McpStatus::Failed { failure: failure.clone() },
            McpEvent::Stopped { .. } => McpStatus::Stopped,
        };
        if !status.is_on() {
            self.worker.set_value(None);
        }
        self.status.set(status);

        Some(event)
    }
}

impl Default for Mcp {
    fn default() -> Self {
        Self::new()
    }
}
