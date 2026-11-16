//! The MCP server as the reader controls it: the settings that start and place it, the status bar
//! toggle, and the messages that say where it serves or why it does not.

use std::net::SocketAddr;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use landaire_ui::Notice;
use landaire_ui::ToastKey;
use rust_i18n::t;
use wows_query::GameData;
use wows_query::data::SharedCatalog;
use wows_toolkit_viewmodel::mcp::McpAddressError;
use wows_toolkit_viewmodel::mcp::McpBindAddress;
use wows_toolkit_viewmodel::mcp::McpPort;
use wows_toolkit_viewmodel::mcp::McpSettings;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::settings_drawer::Setting;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::UpdateValue;
use zgui::task::Task;
use zgui::task::blocking;
use zgui::task::spawn_local;

use crate::services::GameDataState;
use crate::services::mcp::McpEvent;
use crate::services::mcp::McpStatus;
use crate::services::mcp::StartFailure;
use crate::services::mcp::tools::ToolContext;
use crate::shell::Shell;

/// The key every message about the server is shown under, so a newer one replaces an older one.
const MCP_TOAST: &str = "mcp-server";

/// Typed text a bind address or port field did not take, kept so its row says why.
#[derive(Clone, Copy)]
pub struct McpLink {
    address_refused: RwSignal<Option<McpAddressError>, LocalStorage>,
    port_refused: RwSignal<Option<String>, LocalStorage>,
}

impl McpLink {
    pub fn new() -> Self {
        Self { address_refused: RwSignal::new_local(None), port_refused: RwSignal::new_local(None) }
    }

    /// Why the bind address text last typed was refused, tracked. `None` once a valid one is
    /// entered.
    pub fn address_refused(self) -> Option<McpAddressError> {
        self.address_refused.get()
    }

    /// The port text last refused, tracked.
    pub fn port_refused(self) -> Option<String> {
        self.port_refused.get()
    }
}

impl Default for McpLink {
    fn default() -> Self {
        Self::new()
    }
}

/// Why no port could be chosen for a first start.
#[derive(Debug, thiserror::Error)]
#[error("no free port on {address}: {source}")]
pub struct PortChoiceError {
    address: McpBindAddress,
    #[source]
    source: std::io::Error,
}

impl Shell {
    /// Reports the server's events, keeps its tools' context current, and stops it with the
    /// window. Called once, from the shell's body.
    pub fn begin_mcp(self) {
        let services = self.services();
        let mcp = services.mcp;

        if let Some(mut events) = mcp.take_events() {
            spawn_local(async move {
                while let Some(event) = events.next().await {
                    if self.is_unmounted() {
                        return;
                    }
                    if let Some(event) = mcp.receive(event) {
                        self.announce_mcp(&event);
                    }
                }
            });
        }

        let publishing = RenderEffect::new(move |_| {
            let wows_dir = services.settings.read(|settings| settings.wows_dir.clone()).filter(|dir| !dir.is_empty());
            let cache_dir = services.settings.read(|settings| settings.game_data_cache_dir.clone());
            mcp.publish(ToolContext {
                wows_dir: wows_dir.map(PathBuf::from),
                game_data: services.game_data.cache(),
                dump_base: cache_dir.as_deref().and_then(wows_toolkit_services::game_data_cache::base),
                index: services.settings.pool(),
            });
        });

        let sharing = share_loaded_build(self);

        on_cleanup_local(move || {
            // Bounded above the server's own five second drain of open connections.
            const SHUTDOWN_WAIT: Duration = Duration::from_secs(7);

            drop(publishing);
            drop(sharing);
            mcp.shut_down(SHUTDOWN_WAIT);
        });
    }

    /// Starts the server when the reader asked for it to start with the program. Called once the
    /// settings are read.
    pub fn start_mcp_at_launch(self) {
        let settings = self.services().settings.read_untracked(|settings| settings.mcp);
        if settings.is_some_and(|settings| settings.enabled && settings.start_on_launch) {
            self.start_mcp();
        }
    }

    /// Offers the server or withdraws it. Offering it starts it, on a port chosen now when none
    /// has been yet; withdrawing it stops it.
    pub fn set_mcp_enabled(self, on: bool) {
        let settings = self.services().settings;
        let Some(current) = settings.read_untracked(|settings| settings.mcp) else {
            return;
        };

        if !on {
            self.save_mcp(McpSettings { enabled: false, ..current });
            self.services().mcp.stop();
            self.toasts().withdraw(&ToastKey::new(MCP_TOAST));
            return;
        }

        let port = match current.port {
            Some(port) => port,
            None => match choose_free_port(current.bind_address) {
                Ok(port) => port,
                Err(error) => {
                    self.mcp_failed(error.to_string());
                    return;
                }
            },
        };
        self.save_mcp(McpSettings { enabled: true, port: Some(port), ..current });
        self.start_mcp();
    }

    pub fn set_mcp_start_on_launch(self, on: bool) {
        let Some(current) = self.services().settings.read_untracked(|settings| settings.mcp) else {
            return;
        };

        self.save_mcp(McpSettings { start_on_launch: on, ..current });
    }

    /// Adopts typed text as the bind address when it is a loopback one, restarting a running server
    /// there. Other text is kept for the row to report, and changes nothing.
    pub fn commit_mcp_address(self, text: String) {
        let Some(current) = self.services().settings.read_untracked(|settings| settings.mcp) else {
            return;
        };
        let address = match McpBindAddress::parse(&text) {
            Ok(address) => address,
            Err(error) => {
                self.mcp.address_refused.set(Some(error));
                return;
            }
        };

        self.mcp.address_refused.set(None);
        if address == current.bind_address {
            return;
        }
        self.save_mcp(McpSettings { bind_address: address, ..current });
        self.restart_mcp_if_on();
    }

    /// Adopts typed text as the port when it is one, restarting a running server there.
    pub fn commit_mcp_port(self, text: String) {
        let Some(current) = self.services().settings.read_untracked(|settings| settings.mcp) else {
            return;
        };
        let port = match McpPort::parse(&text) {
            Ok(port) => port,
            Err(_) => {
                self.mcp.port_refused.set(Some(text));
                return;
            }
        };

        self.mcp.port_refused.set(None);
        if Some(port) == current.port {
            return;
        }
        self.save_mcp(McpSettings { port: Some(port), ..current });
        self.restart_mcp_if_on();
    }

    /// Stops a started server, or starts a stopped one, leaving whether it is offered alone.
    pub fn toggle_mcp(self) {
        if self.services().mcp.status_untracked().is_on() {
            self.services().mcp.stop();
            self.toasts().withdraw(&ToastKey::new(MCP_TOAST));
        } else {
            self.start_mcp();
        }
    }

    /// Starts the server on the saved address, replacing a running one.
    pub fn start_mcp(self) {
        let services = self.services();
        let Some(settings) = services.settings.read_untracked(|settings| settings.mcp) else {
            return;
        };
        // A server is only started once offered, which chose its port.
        let Some(address) = settings.socket_address() else {
            return;
        };
        // The server has no authentication; a row holding another address is reported on its own.
        if !settings.bind_address.is_loopback() {
            let input = settings.bind_address.to_string();
            self.mcp_failed(t!("ui.settings.mcp.address_not_loopback", input = input).into_owned());
            return;
        }
        let Some(watchdog) = services.watchdog() else {
            self.mcp_failed(t!("ui.mcp.no_watchdog").into_owned());
            return;
        };

        if let Err(error) = services.mcp.start(&watchdog, address) {
            self.mcp_failed(error.to_string());
        }
    }

    fn restart_mcp_if_on(self) {
        if self.services().mcp.status_untracked().is_on() {
            self.start_mcp();
        }
    }

    fn save_mcp(self, mcp: McpSettings) {
        self.services().settings.edit(keys::MCP_SERVER, |settings| {
            settings.mcp = mcp;
            mcp
        });
    }

    /// Says what the server's thread reported about the running server.
    fn announce_mcp(self, event: &McpEvent) {
        let Some(toasts) = self.try_toasts() else { return };
        let key = ToastKey::new(MCP_TOAST);

        match event {
            McpEvent::Started { endpoint, .. } => {
                let url = endpoint.url();
                toasts.show(
                    Notice::ok(t!("ui.mcp.started", url = url).into_owned())
                        .keyed(key)
                        .action(t!("ui.mcp.copy_url").into_owned(), move || self.copy_text(url.clone())),
                );
            }
            McpEvent::Failed { failure, .. } => self.mcp_failed(failure_message(failure)),
            // Stopped by the reader, who knows; a crash is restarted by the watchdog.
            McpEvent::Stopped { .. } => {}
        }
    }

    /// Puts up a message that the server could not start, with a button to its settings.
    fn mcp_failed(self, reason: String) {
        let Some(toasts) = self.try_toasts() else { return };
        let open = self.settings_drawer;

        toasts.show(
            Notice::failed(t!("ui.mcp.start_failed", reason = reason).into_owned())
                .keyed(ToastKey::new(MCP_TOAST))
                .sticky()
                .action_keeping_open(t!("ui.settings.panel.open").into_owned(), move || {
                    open.open_at(Setting::McpPort);
                }),
        );
    }

    /// Whether the running or last failed server could not bind because its port is taken.
    pub fn mcp_port_in_use(self) -> bool {
        matches!(self.services().mcp.status(), McpStatus::Failed { failure: StartFailure::PortInUse { .. } })
    }
}

/// Hands the game data tools the build the app has loaded while a server runs, indexed over the
/// app's own provider and files so GameParams is parsed once. The index is dropped when the server
/// stops, and rebuilt when the app loads another build or rewrites its names in another language.
fn share_loaded_build(shell: Shell) -> RenderEffect<()> {
    let services = shell.services();
    let mcp = services.mcp;
    let indexing = StoredValue::new_local(None::<Task>);

    RenderEffect::new(move |_| {
        services.game_data.track_catalogs();
        let loaded = match services.game_data.state() {
            GameDataState::Ready(loaded) if mcp.status().is_on() => Some(loaded),
            _ => None,
        };
        let Some(loaded) = loaded else {
            if let Some(Some(running)) = indexing.try_update_value(Option::take) {
                running.cancel();
            }
            mcp.share_build(None);
            return;
        };
        let catalog = loaded
            .catalog_language()
            .map(|catalog| SharedCatalog { language: catalog.language, translation_epoch: catalog.translation_epoch });
        let current = mcp.shared_build();
        if current.is_some_and(|shared| {
            Arc::ptr_eq(shared.shared_provider(), loaded.provider()) && shared.shared_catalog() == catalog.as_ref()
        }) {
            return;
        }
        let Some(version) = loaded.version() else {
            mcp.share_build(None);
            return;
        };

        let source_dir =
            services.settings.read_untracked(|settings| PathBuf::from(&settings.wows_dir)).unwrap_or_default();
        let cache_root = wows_toolkit_config::storage_dir().map(|storage| storage.join("mcp_game_params"));
        let task = spawn_local(async move {
            let build = loaded.build();
            let provider = Arc::clone(loaded.provider());
            let vfs = loaded.vfs().clone();
            let indexed = blocking(move || {
                GameData::from_shared(version, build, provider, catalog, vfs, &source_dir, cache_root.as_deref())
            })
            .await;
            mcp.share_build(Some(Arc::new(indexed)));
        });
        if let Some(Some(earlier)) = indexing.try_update_value(|running| running.replace(task)) {
            earlier.cancel();
        }
    })
}

/// The reason a failure gives, in the current language.
fn failure_message(failure: &StartFailure) -> String {
    match failure {
        StartFailure::PortInUse { address } => {
            t!("ui.mcp.port_in_use", port = address.port(), address = address.ip()).into_owned()
        }
        StartFailure::Listen { reason, .. } | StartFailure::Runtime { reason } | StartFailure::GaveUp { reason } => {
            reason.clone()
        }
    }
}

/// A port nothing listens on at `address` now. Drawn below the ephemeral ranges of Linux
/// (32768 up) and Windows (49152 up), so an outgoing connection on a later start is unlikely to
/// hold it.
fn choose_free_port(address: McpBindAddress) -> Result<McpPort, PortChoiceError> {
    const PORTS: std::ops::Range<u16> = 20_000..32_768;
    const TRIES: usize = 32;

    let mut last_error = None;
    for _ in 0..TRIES {
        let drawn = std::hash::BuildHasher::hash_one(&std::collections::hash_map::RandomState::new(), ());
        let port = PORTS.start + (drawn % u64::from(PORTS.end - PORTS.start)) as u16;
        match TcpListener::bind(SocketAddr::new(address.ip(), port)) {
            Ok(_) => return Ok(McpPort::new(std::num::NonZeroU16::new(port).expect("the range starts above 0"))),
            Err(error) => last_error = Some(error),
        }
    }
    let source = last_error.unwrap_or_else(|| std::io::Error::from(std::io::ErrorKind::AddrInUse));

    Err(PortChoiceError { address, source })
}

#[cfg(test)]
mod tests;
