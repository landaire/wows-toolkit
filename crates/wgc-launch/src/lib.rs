//! Launch WoWs installs.

use std::path::PathBuf;

use thiserror::Error;
use wgc_install::GameInstall;

pub mod proton;

use crate::proton::ProtonConfig;

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

/// Online launches require a valid or not-yet-configured session; replay
/// launches are never gated.
pub fn gate(mode: &LaunchMode, status: TokenStatus) -> Result<(), LaunchError> {
    match mode {
        LaunchMode::Replay { .. } => Ok(()),
        LaunchMode::Online => match status {
            TokenStatus::Valid { .. } | TokenStatus::NotConfigured => Ok(()),
            TokenStatus::Expired | TokenStatus::Missing => Err(LaunchError::AuthenticationRequired),
        },
    }
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

    /// Spawn the game. Online launches are gated on `status` first. On Linux a
    /// non-Steam install needs `proton`; a Steam install launches through Steam.
    pub fn spawn(
        &self,
        proton: Option<&ProtonConfig>,
        status: TokenStatus,
    ) -> Result<Option<std::process::Child>, LaunchError> {
        gate(&self.mode, status)?;
        #[cfg(not(target_os = "linux"))]
        {
            let _ = proton;
            tracing::info!("Launching {} {}", self.exe.display(), self.args.join(" "));
            return self
                .native_command()
                .spawn()
                .map(Some)
                .map_err(|e| LaunchError::SpawnFailed { detail: e.to_string() });
        }
        #[cfg(target_os = "linux")]
        {
            use wgc_install::InstallSource;
            if matches!(self.install.source, InstallSource::Steam { .. }) {
                tracing::info!("Launching via Steam: -applaunch 552990 {}", self.args.join(" "));
                let mut cmd = std::process::Command::new("steam");
                cmd.arg("-applaunch").arg("552990");
                cmd.args(&self.args);
                cmd.envs(self.env.iter().map(|(k, v)| (k.clone(), v.clone())));
                return cmd.spawn().map(Some).map_err(|e| LaunchError::SpawnFailed { detail: e.to_string() });
            }
            let proton = proton.ok_or(LaunchError::ProtonPrefixNotConfigured)?;
            if !proton.proton_binary.exists() {
                return Err(LaunchError::ProtonNotFound);
            }
            tracing::info!(
                "Launching via Proton: {} run {} {}",
                proton.proton_binary.display(),
                self.exe.display(),
                self.args.join(" ")
            );
            let mut cmd = std::process::Command::new(&proton.proton_binary);
            cmd.arg("run").arg(&self.exe).args(&self.args);
            cmd.current_dir(&self.install.path);
            // User env first, then the required compat vars, so a user override
            // cannot break the Proton runtime.
            cmd.envs(self.env.iter().map(|(k, v)| (k.clone(), v.clone())));
            cmd.env("STEAM_COMPAT_DATA_PATH", &proton.compat_prefix);
            // The compatdata prefix may live in a secondary Steam library, so its
            // grandparent isn't reliably the Steam client root; prefer the real
            // Steam install and only fall back to the heuristic if it's not found.
            match crate::proton::find_steam_root()
                .or_else(|| proton.compat_prefix.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()))
            {
                Some(client_install_path) => {
                    cmd.env("STEAM_COMPAT_CLIENT_INSTALL_PATH", client_install_path);
                }
                None => {
                    tracing::warn!(
                        "could not determine Steam client install path from compat_prefix {}; STEAM_COMPAT_CLIENT_INSTALL_PATH not set",
                        proton.compat_prefix.display()
                    );
                }
            }
            cmd.spawn().map(Some).map_err(|e| LaunchError::SpawnFailed { detail: e.to_string() })
        }
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

    #[test]
    fn replay_launch_is_never_gated() {
        let mode = LaunchMode::Replay { path: PathBuf::from("/r/a.wowsreplay") };
        assert!(gate(&mode, TokenStatus::Expired).is_ok());
        assert!(gate(&mode, TokenStatus::Missing).is_ok());
    }

    #[test]
    fn online_launch_refused_when_expired_or_missing() {
        assert!(matches!(gate(&LaunchMode::Online, TokenStatus::Expired), Err(LaunchError::AuthenticationRequired)));
        assert!(matches!(gate(&LaunchMode::Online, TokenStatus::Missing), Err(LaunchError::AuthenticationRequired)));
    }

    #[test]
    fn online_launch_allowed_when_valid_or_not_configured() {
        assert!(gate(&LaunchMode::Online, TokenStatus::Valid { expires_at: 0 }).is_ok());
        assert!(gate(&LaunchMode::Online, TokenStatus::NotConfigured).is_ok());
    }
}
