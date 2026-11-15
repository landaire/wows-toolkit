use std::ffi::OsString;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tracing::debug;
use tracing::warn;
use wgc_relay_protocol::RELAY_ADDR_ENV;
use wgc_relay_protocol::RELAY_TOKEN_ENV;
use wgc_relay_protocol::RelayToken;
use wgc_relay_protocol::TOKEN_LEN;

use crate::error::IpcError;
use crate::responder::IpcResponder;
use crate::server::serve_stream;

/// The loopback listener `wgc-relay.exe` connects back to, one TCP connection
/// per pipe connection.
pub struct RelayListener {
    listener: TcpListener,
    addr: SocketAddr,
    token: RelayToken,
}

impl RelayListener {
    pub async fn bind() -> Result<RelayListener, IpcError> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let token = RelayToken::from_bytes(rand::random());

        Ok(RelayListener { listener, addr, token })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Environment for the relay process. Wine passes it through to Windows.
    pub fn relay_env(&self) -> Vec<(OsString, OsString)> {
        vec![
            (RELAY_ADDR_ENV.into(), self.addr.to_string().into()),
            (RELAY_TOKEN_ENV.into(), self.token.to_hex().into()),
        ]
    }

    /// Accepts relay connections until dropped. Dropping it also ends the
    /// connections it serves.
    pub async fn serve(self, responder: Arc<dyn IpcResponder>) -> Result<(), IpcError> {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                accepted = self.listener.accept() => {
                    let (stream, peer) = accepted?;
                    let token = self.token.clone();
                    let responder = Arc::clone(&responder);
                    connections.spawn(async move {
                        if let Err(err) = serve_connection(stream, peer, &token, responder.as_ref()).await {
                            warn!(%err, "relay connection failed");
                        }
                    });
                }
                Some(_) = connections.join_next() => {}
            }
        }
    }
}

async fn serve_connection(
    mut stream: TcpStream,
    peer: SocketAddr,
    token: &RelayToken,
    responder: &dyn IpcResponder,
) -> Result<(), IpcError> {
    // The relay sends the token immediately; a silent peer is not the relay.
    const TOKEN_TIMEOUT: Duration = Duration::from_secs(5);

    let mut presented = [0u8; TOKEN_LEN];
    tokio::time::timeout(TOKEN_TIMEOUT, stream.read_exact(&mut presented))
        .await
        .map_err(|_| IpcError::TokenTimeout { peer })??;
    if !token.matches(&presented) {
        return Err(IpcError::TokenMismatch { peer });
    }

    debug!(%peer, "relay connected");
    stream.set_nodelay(true)?;

    serve_stream(stream, responder).await
}
