use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpStream;

use wgc_relay_protocol::RELAY_ADDR_ENV;
use wgc_relay_protocol::RELAY_TOKEN_ENV;
use wgc_relay_protocol::RelayToken;

use crate::error::RelayError;

/// Where the launcher listens and the token that proves this relay is its own.
#[derive(Clone)]
pub struct LinkConfig {
    addr: SocketAddr,
    token: RelayToken,
}

impl LinkConfig {
    pub fn from_env() -> Result<LinkConfig, RelayError> {
        let addr_text = std::env::var(RELAY_ADDR_ENV).map_err(|_| RelayError::MissingEnv { name: RELAY_ADDR_ENV })?;
        let addr = addr_text.parse().map_err(|_| RelayError::BadAddress { name: RELAY_ADDR_ENV, value: addr_text })?;
        let token_text =
            std::env::var(RELAY_TOKEN_ENV).map_err(|_| RelayError::MissingEnv { name: RELAY_TOKEN_ENV })?;
        let token = RelayToken::parse_hex(&token_text)?;

        Ok(LinkConfig { addr, token })
    }

    /// Opens one launcher connection and presents the token.
    pub fn connect(&self) -> Result<TcpStream, RelayError> {
        let mut stream = TcpStream::connect(self.addr).map_err(RelayError::Connect)?;
        stream.set_nodelay(true).map_err(RelayError::Connect)?;
        stream.write_all(self.token.as_bytes()).map_err(RelayError::Connect)?;

        Ok(stream)
    }
}
