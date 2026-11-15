//! The MCP server settings: whether it runs, and the address it listens on.

use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::num::NonZeroU16;

use serde::Deserialize;
use serde::Serialize;

/// The MCP server settings, stored as one row under
/// [`crate::settings::keys::MCP_SERVER`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpSettings {
    /// Whether the server is offered at all: its status bar control is shown
    /// and it can be started.
    #[serde(default)]
    pub enabled: bool,

    /// Whether an enabled server starts with the program.
    #[serde(default = "start_on_launch_default")]
    pub start_on_launch: bool,

    #[serde(default)]
    pub bind_address: McpBindAddress,

    /// Chosen once, on the first enable, so a client configured with the URL
    /// keeps working. `None` until then.
    #[serde(default)]
    pub port: Option<McpPort>,
}

impl McpSettings {
    pub const fn new() -> Self {
        Self { enabled: false, start_on_launch: true, bind_address: McpBindAddress::LOOPBACK, port: None }
    }

    /// The address to listen on, once a port is chosen.
    pub fn socket_address(&self) -> Option<SocketAddr> {
        let port = self.port?;

        Some(SocketAddr::new(self.bind_address.ip(), port.get()))
    }
}

impl Default for McpSettings {
    /// A server that was never enabled, on loopback.
    fn default() -> Self {
        Self::new()
    }
}

const fn start_on_launch_default() -> bool {
    true
}

/// The IP address the MCP server listens on. The server has no authentication, so it listens only
/// on loopback; a row holding another address is reported and the server is not started.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct McpBindAddress(IpAddr);

impl McpBindAddress {
    pub const LOOPBACK: Self = Self(IpAddr::V4(Ipv4Addr::LOCALHOST));

    /// Reads a loopback IPv4 or IPv6 address as typed, without brackets or a port.
    pub fn parse(text: &str) -> Result<Self, McpAddressError> {
        let trimmed = text.trim();
        let unbracketed = trimmed.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')).unwrap_or(trimmed);
        let address: IpAddr =
            unbracketed.parse().map_err(|_| McpAddressError::NotAnAddress { input: text.to_owned() })?;
        if !address.is_loopback() {
            return Err(McpAddressError::NotLoopback { input: text.to_owned() });
        }

        Ok(Self(address))
    }

    pub const fn ip(self) -> IpAddr {
        self.0
    }

    /// Whether only this machine can reach the server, so it may listen here.
    pub const fn is_loopback(self) -> bool {
        self.0.is_loopback()
    }
}

impl Default for McpBindAddress {
    /// Loopback, so nothing off this machine reaches the server.
    fn default() -> Self {
        Self::LOOPBACK
    }
}

impl std::fmt::Display for McpBindAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The TCP port the MCP server listens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct McpPort(NonZeroU16);

impl McpPort {
    pub const fn new(port: NonZeroU16) -> Self {
        Self(port)
    }

    pub fn parse(text: &str) -> Result<Self, McpPortError> {
        let trimmed = text.trim();
        let number: u16 = trimmed.parse().map_err(|_| McpPortError::NotAPort { input: text.to_owned() })?;

        NonZeroU16::new(number).map(Self).ok_or(McpPortError::NotAPort { input: text.to_owned() })
    }

    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

impl std::fmt::Display for McpPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum McpAddressError {
    #[error("{input:?} is not an IP address")]
    NotAnAddress { input: String },

    #[error("{input:?} is not a loopback address")]
    NotLoopback { input: String },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum McpPortError {
    #[error("{input:?} is not a port from 1 to 65535")]
    NotAPort { input: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_with_or_without_brackets() {
        assert_eq!(McpBindAddress::parse(" 127.0.0.1 ").unwrap(), McpBindAddress::LOOPBACK);
        assert_eq!(McpBindAddress::parse("[::1]").unwrap().ip(), "::1".parse::<IpAddr>().unwrap());
        assert_eq!(McpBindAddress::parse("127.0.0.2").unwrap().ip(), "127.0.0.2".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn host_names_and_ports_are_not_addresses() {
        for input in ["localhost", "127.0.0.1:8080", "", "256.0.0.1", "[::1"] {
            assert!(matches!(McpBindAddress::parse(input), Err(McpAddressError::NotAnAddress { .. })), "{input:?}");
        }
    }

    #[test]
    fn only_loopback_addresses_are_taken() {
        for input in ["0.0.0.0", "::", "192.168.1.20", "[fe80::1]"] {
            assert!(matches!(McpBindAddress::parse(input), Err(McpAddressError::NotLoopback { .. })), "{input:?}");
        }
    }

    #[test]
    fn ports_run_from_one_to_65535() {
        assert_eq!(McpPort::parse("8080").unwrap().get(), 8080);
        assert_eq!(McpPort::parse(" 65535 ").unwrap().get(), 65535);
        for input in ["0", "65536", "-1", "http", ""] {
            assert!(McpPort::parse(input).is_err(), "{input:?}");
        }
    }

    #[test]
    fn a_row_written_with_fewer_fields_reads_the_defaults() {
        let read: McpSettings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();

        assert_eq!(read, McpSettings { enabled: true, ..McpSettings::new() });
    }

    #[test]
    fn a_saved_row_reads_back() {
        let saved = McpSettings {
            enabled: true,
            start_on_launch: false,
            bind_address: McpBindAddress::parse("::1").unwrap(),
            port: Some(McpPort::parse("40123").unwrap()),
        };

        let json = serde_json::to_string(&saved).unwrap();
        assert_eq!(json, r#"{"enabled":true,"start_on_launch":false,"bind_address":"::1","port":40123}"#);
        assert_eq!(serde_json::from_str::<McpSettings>(&json).unwrap(), saved);
    }
}
