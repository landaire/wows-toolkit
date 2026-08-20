//! Launch WoWs installs.

use std::path::PathBuf;

use thiserror::Error;

/// Whether this launch is normal online play or offline replay playback.
#[derive(Debug, Clone)]
pub enum LaunchMode {
    Online,
    Replay { path: PathBuf },
}

/// Session state for the active account, consulted by the online-launch gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStatus {
    /// A valid session; `expires_at` is a unix timestamp in seconds.
    Valid { expires_at: u64 },
    /// A session existed but has expired; re-authentication is required.
    Expired,
    /// An account is configured but has no token.
    Missing,
    /// No account is configured yet (the slice-1 state).
    NotConfigured,
}

#[derive(Debug, Default, Clone)]
pub struct LaunchArgs {
    pub extra: Vec<String>,
    /// Ordered so it is deterministic and serde-friendly.
    pub env: Vec<(String, String)>,
}

pub fn build_args(mode: &LaunchMode, args: &LaunchArgs) -> Vec<String> {
    let mut out = Vec::new();
    if let LaunchMode::Replay { path } = mode {
        out.push(path.to_string_lossy().into_owned());
    }
    out.extend(args.extra.iter().cloned());
    out
}

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("game executable not found at {path}")]
    ExeNotFound { path: String },
    #[error("no Proton prefix configured for this install")]
    ProtonPrefixNotConfigured,
    #[error("no Proton installation found")]
    ProtonNotFound,
    #[error("a valid session is required to launch online; please re-authenticate")]
    AuthenticationRequired,
    #[error("failed to spawn game process: {detail}")]
    SpawnFailed { detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn build_args_puts_replay_first() {
        let mode = LaunchMode::Replay { path: PathBuf::from("/r/a.wowsreplay") };
        let args = LaunchArgs { extra: vec!["-foo".into()], ..Default::default() };
        assert_eq!(build_args(&mode, &args), vec!["/r/a.wowsreplay".to_string(), "-foo".to_string()]);
    }

    #[test]
    fn build_args_online_is_extra_only() {
        assert!(build_args(&LaunchMode::Online, &LaunchArgs::default()).is_empty());
    }
}
