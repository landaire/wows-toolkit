use std::path::PathBuf;

use thiserror::Error;

use crate::prefix::RelayBinary;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CodecError {
    #[error("message ends at byte {offset} with {needed} more bytes needed")]
    Truncated { offset: usize, needed: u64 },
    #[error("message magic is {found:#06x}, expected {expected:#06x}")]
    BadMagic { found: u16, expected: u16 },
    #[error("protocol version is {found}, expected {expected}")]
    UnsupportedProtocol { found: u32, expected: u32 },
    #[error("command {index} is a null pointer")]
    NullCommand { index: u64 },
    #[error("command {index} refers to polymorphic type id {id}, which the message never registered")]
    UnregisteredType { index: u64, id: u32 },
    #[error("command type {name:?} is not modeled")]
    UnknownCommand { name: String },
    #[error("wide string at byte {offset} is not valid UTF-16")]
    InvalidUtf16 { offset: usize },
    #[error("narrow string at byte {offset} is not valid UTF-8")]
    InvalidUtf8 { offset: usize },
    #[error("{remaining} bytes follow the last command")]
    TrailingBytes { remaining: usize },
    #[error("{command} appears with class versions {first} and {second} in one message")]
    ConflictingClassVersion { command: &'static str, first: u32, second: u32 },
    #[error("{command} field {field} does not match class version {version}")]
    VersionedField { command: &'static str, field: &'static str, version: u32 },
}

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("relay link I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("relay link framing failed: {0}")]
    Frame(#[from] wgc_relay_protocol::FrameError),
    #[error("relay connection from {peer} presented a wrong token")]
    TokenMismatch { peer: std::net::SocketAddr },
    #[error("relay connection from {peer} sent no token in time")]
    TokenTimeout { peer: std::net::SocketAddr },
}

#[derive(Debug, Error)]
pub enum PrefixError {
    #[error("{binary} is missing from {dir}; it ships beside the toolkit, so reinstall the toolkit")]
    BinaryMissing { binary: RelayBinary, dir: PathBuf },
    #[error("reading the Buck resource database {path} failed: {source}")]
    ResourceDatabase { path: PathBuf, source: serde_json::Error },
    #[error("cannot locate the toolkit executable: {0}")]
    CurrentExe(std::io::Error),
    #[error("writing {path} failed: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("reading {path} failed: {source}")]
    Read { path: PathBuf, source: std::io::Error },
}
