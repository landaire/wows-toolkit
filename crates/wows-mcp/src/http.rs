use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::rt::TokioTimer;
use hyper_util::server::conn::auto;
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::server::Handler;

/// Path clients are told to use. The service answers on every path.
const ENDPOINT_PATH: &str = "/mcp";

/// Host names a loopback client may send. DNS rebinding makes a browser send
/// an attacker's host name to a loopback address, so only these and the bound
/// address itself are accepted.
const LOOPBACK_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// Browser origins served from this machine. Any other origin is a web page
/// attempting a cross-site request.
const LOOPBACK_ORIGINS: [&str; 6] = [
    "http://localhost:*",
    "http://127.0.0.1:*",
    "http://[::1]:*",
    "https://localhost:*",
    "https://127.0.0.1:*",
    "https://[::1]:*",
];

/// The bound address of an HTTP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpEndpoint {
    address: SocketAddr,
}

impl HttpEndpoint {
    pub fn new(address: SocketAddr) -> Self {
        Self { address }
    }

    pub fn address(self) -> SocketAddr {
        self.address
    }

    /// The URL an MCP client on this machine connects to. An unspecified
    /// bind address is reached through loopback.
    pub fn url(self) -> String {
        let ip = match self.address.ip() {
            IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
            ip => ip,
        };

        format!("http://{}{ENDPOINT_PATH}", SocketAddr::new(ip, self.address.port()))
    }

    /// `Host` header values the server accepts.
    pub fn allowed_hosts(self) -> Vec<String> {
        let mut hosts: Vec<String> = LOOPBACK_HOSTS.iter().map(|host| (*host).to_owned()).collect();
        hosts.push(self.address.to_string());

        hosts
    }

    /// `Origin` header values the server accepts. Requests without `Origin`
    /// come from non-browser clients and are accepted.
    pub fn allowed_origins() -> Vec<String> {
        LOOPBACK_ORIGINS.iter().map(|origin| (*origin).to_owned()).collect()
    }

    fn config(self, sessions: CancellationToken) -> StreamableHttpServerConfig {
        let mut config = StreamableHttpServerConfig::default()
            .with_allowed_hosts(self.allowed_hosts())
            .with_allowed_origins(Self::allowed_origins())
            .enforce_origin_validation();
        config.cancellation_token = sessions;

        config
    }
}

/// Serves `handler` on `listener` until `shutdown` is cancelled, then closes
/// every session and waits a bounded time for open connections to finish.
pub(crate) async fn serve(
    handler: Handler,
    listener: TcpListener,
    endpoint: HttpEndpoint,
    shutdown: CancellationToken,
) {
    // Bounds descriptors and tasks a local process can make the server hold.
    const MAX_CONNECTIONS: usize = 64;
    // A connection that never finishes its headers would hold a slot.
    const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
    const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
    // Keeps a persistent accept failure such as descriptor exhaustion from
    // spinning.
    const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

    let sessions = shutdown.child_token();
    let service: StreamableHttpService<Handler, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        endpoint.config(sessions.clone()),
    );
    // HTTP/1 only: protocol detection waits for a first byte without a timeout, so an idle socket
    // would hold a connection slot forever.
    let mut builder = auto::Builder::new(TokioExecutor::new()).http1_only();
    builder.http1().timer(TokioTimer::new()).header_read_timeout(HEADER_READ_TIMEOUT);
    let graceful = GracefulShutdown::new();
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    tracing::info!(address = %endpoint.address(), "serving MCP over streamable HTTP");

    loop {
        let permit = tokio::select! {
            () = shutdown.cancelled() => break,
            permit = Arc::clone(&permits).acquire_owned() => permit.expect("the connection semaphore is never closed"),
        };
        let accepted = tokio::select! {
            () = shutdown.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let (stream, peer) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, "failed to accept a connection");
                tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                continue;
            }
        };

        let connection =
            builder.serve_connection(TokioIo::new(stream), TowerToHyperService::new(service.clone())).into_owned();
        let connection = graceful.watch(connection);
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(%peer, %error, "connection ended with an error");
            }
            drop(permit);
        });
    }

    drop(listener);
    sessions.cancel();
    if tokio::time::timeout(DRAIN_TIMEOUT, graceful.shutdown()).await.is_err() {
        tracing::warn!("MCP connections still open after shutdown; abandoning them");
    }
    tracing::info!(address = %endpoint.address(), "MCP server stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_reaches_unspecified_addresses_through_loopback() {
        let v4 = HttpEndpoint::new("0.0.0.0:4000".parse().unwrap());
        let v6 = HttpEndpoint::new("[::]:4000".parse().unwrap());
        let lan = HttpEndpoint::new("192.168.1.20:4000".parse().unwrap());

        assert_eq!(v4.url(), "http://127.0.0.1:4000/mcp");
        assert_eq!(v6.url(), "http://[::1]:4000/mcp");
        assert_eq!(lan.url(), "http://192.168.1.20:4000/mcp");
    }
}
