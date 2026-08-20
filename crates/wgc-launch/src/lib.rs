//! Launch WoWs installs.

use std::path::PathBuf;

use thiserror::Error;
use wgc_install::GameInstall;

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

/// The WoWs executable inside an install directory.
pub fn game_exe(install: &GameInstall) -> PathBuf {
    install.path.join("WorldOfWarships.exe")
}

#[derive(Debug)]
pub struct ResolvedLaunch<'a> {
    pub install: &'a GameInstall,
    exe: PathBuf,
    mode: LaunchMode,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

/// Validate the exe exists and precompute args. The online-launch gate and
/// Proton availability (Linux) are checked at spawn time, not here, because they
/// depend on runtime session state and settings.
pub fn resolve<'a>(
    install: &'a GameInstall,
    mode: LaunchMode,
    args: &LaunchArgs,
) -> Result<ResolvedLaunch<'a>, LaunchError> {
    let exe = game_exe(install);
    if !exe.exists() {
        return Err(LaunchError::ExeNotFound { path: exe.display().to_string() });
    }
    let rendered = build_args(&mode, args);
    Ok(ResolvedLaunch { install, exe, mode, args: rendered, env: args.env.clone() })
}

impl<'a> ResolvedLaunch<'a> {
    /// An un-spawned native command (Windows, or the fallback path), cwd set to
    /// the install dir, user env applied. The caller decorates (e.g. process
    /// hardening) then spawns.
    pub fn native_command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.args(&self.args);
        cmd.current_dir(&self.install.path);
        cmd.envs(self.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        cmd
    }
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

    #[test]
    fn resolve_errors_when_exe_missing() {
        let install = GameInstall::manual(PathBuf::from("/no/such/dir"));
        let err = resolve(&install, LaunchMode::Online, &LaunchArgs::default()).unwrap_err();
        assert!(matches!(err, LaunchError::ExeNotFound { .. }));
    }

    #[test]
    fn resolve_succeeds_when_exe_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("WorldOfWarships.exe"), b"stub").unwrap();
        let install = GameInstall::manual(dir.path().to_path_buf());
        let resolved = resolve(&install, LaunchMode::Online, &LaunchArgs::default()).expect("resolved");
        let cmd = resolved.native_command();
        assert_eq!(cmd.get_current_dir(), Some(dir.path()));
    }

    #[test]
    fn native_command_applies_user_env_and_args() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("WorldOfWarships.exe"), b"stub").unwrap();
        let install = GameInstall::manual(dir.path().to_path_buf());
        let args = LaunchArgs { extra: vec!["--safe".into()], env: vec![("WOWS_SAFE".into(), "1".into())] };
        let resolved = resolve(&install, LaunchMode::Online, &args).expect("resolved");
        let cmd = resolved.native_command();
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(
            envs.iter().any(|(k, v)| *k == std::ffi::OsStr::new("WOWS_SAFE") && *v == Some(std::ffi::OsStr::new("1")))
        );
        let args: Vec<_> = cmd.get_args().collect();
        assert!(args.iter().any(|a| *a == std::ffi::OsStr::new("--safe")));
    }
}
