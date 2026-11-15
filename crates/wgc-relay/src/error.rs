use std::path::PathBuf;

use thiserror::Error;
use wgc_relay_protocol::FrameError;
use wgc_relay_protocol::TokenParseError;

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("environment variable {name} is missing; the launcher sets it")]
    MissingEnv { name: &'static str },
    #[error("{name} is not a socket address: {value:?}")]
    BadAddress { name: &'static str, value: String },
    #[error("relay token is malformed: {0}")]
    BadToken(#[from] TokenParseError),
    #[error("usage: wgc-relay.exe <game exe> [args]")]
    MissingGameArgument,
    #[error("{call} failed with Windows error {code}")]
    Win32 { call: &'static str, code: u32 },
    #[error("starting {path} failed: {source}")]
    Spawn { path: PathBuf, source: std::io::Error },
    #[error("waiting for {path} failed: {source}")]
    Wait { path: PathBuf, source: std::io::Error },
    #[error("launcher link failed: {0}")]
    Link(#[from] FrameError),
    #[error("connecting to the launcher failed: {0}")]
    Connect(std::io::Error),
}

impl RelayError {
    /// Captures `GetLastError` for the Win32 call that just failed.
    pub fn last_os_error(call: &'static str) -> RelayError {
        // SAFETY: reads thread-local state only.
        let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };

        RelayError::Win32 { call, code }
    }
}
