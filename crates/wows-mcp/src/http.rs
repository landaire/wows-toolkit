use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto;
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::server::Handler;

/// Path clients are told to use. The service answers on every path.
const ENDPOINT_PATH: &str = "/mcp";

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

    fn config(self, sessions: CancellationToken) -> StreamableHttpServerConfig {
        let mut config = StreamableHttpServerConfig::default();
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
    let builder = auto::Builder::new(TokioExecutor::new());
    let graceful = GracefulShutdown::new();
    tracing::info!(address = %endpoint.address(), "serving MCP over streamable HTTP");

    loop {
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
