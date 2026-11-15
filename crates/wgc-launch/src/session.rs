//! A running game session: the runner child plus every process that
//! inherited the session marker. Wine reparents processes and Proton may
//! run them in a container, so the process tree alone does not find them.

use std::process::Child;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;

use crate::error::KillError;
use crate::error::WaitError;
use crate::overlay::FuseMount;

/// Present in the environment of every process of a toolkit-launched
/// session. The value names the install (or the prefix setup run).
pub const SESSION_ENV: &str = "WOWS_TOOLKIT_GAME_SESSION";

const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u32);

/// Identifies one session's processes by their `SESSION_ENV` value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A spawned session. Call `wait` to reap the runner and release the overlay.
#[derive(Debug)]
pub struct GameProcess {
    child: Child,
    handle: GameHandle,
    mount: Option<FuseMount>,
}

impl GameProcess {
    pub(crate) fn new(child: Child, session: SessionId, mount: Option<FuseMount>) -> Self {
        let handle = GameHandle { session: Arc::new(session), runner: Some(ProcessId(child.id())) };
        Self { child, handle, mount }
    }

    pub fn handle(&self) -> GameHandle {
        self.handle.clone()
    }

    /// Blocks until the runner and every session process have exited, then
    /// unmounts a fuse overlay. The session is over whichever way it returns.
    /// Without Linux process tracking this waits for the runner only.
    pub fn wait(mut self) -> Result<GameExit, WaitError> {
        let runner_status = self.child.wait().map_err(WaitError::Runner)?;
        if cfg!(target_os = "linux") {
            self.handle.wait_exit().map_err(WaitError::Scan)?;
        }

        if let Some(mount) = self.mount.take() {
            mount.unmount().map_err(WaitError::Unmount)?;
        }

        Ok(GameExit { runner_status })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameExit {
    /// The runner's status. Proton reports the first Windows process's code.
    pub runner_status: ExitStatus,
}

/// A cloneable view of a session for status queries and kill. Queries scan
/// `/proc` and fail with `ErrorKind::Unsupported` on other platforms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameHandle {
    session: Arc<SessionId>,
    runner: Option<ProcessId>,
}

impl GameHandle {
    /// A session left running by an earlier toolkit process, if any.
    pub fn find_running() -> std::io::Result<Option<GameHandle>> {
        let found = scan::session_processes(None)?.into_iter().next();
        Ok(found.map(|process| GameHandle { session: Arc::new(process.session), runner: None }))
    }

    pub fn session(&self) -> &SessionId {
        &self.session
    }

    /// The direct child (runner or bwrap). `None` for an adopted session.
    pub fn runner_pid(&self) -> Option<ProcessId> {
        self.runner
    }

    /// The game client's PID: `WorldOfWarships64.exe`, else the launcher stub.
    pub fn game_pid(&self) -> std::io::Result<Option<ProcessId>> {
        let processes = scan::session_processes(Some(&self.session))?;
        let pid_named = |exe: &str| {
            processes
                .iter()
                .find(|process| process.exe_name.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(exe)))
        };

        Ok(pid_named("WorldOfWarships64.exe").or_else(|| pid_named("WorldOfWarships.exe")).map(|process| process.pid))
    }

    pub fn is_running(&self) -> std::io::Result<bool> {
        Ok(!scan::session_processes(Some(&self.session))?.is_empty())
    }

    /// Blocks until no session process remains.
    pub fn wait_exit(&self) -> std::io::Result<()> {
        while self.is_running()? {
            std::thread::sleep(POLL_INTERVAL);
        }

        Ok(())
    }

    /// Kills every session process. Wine processes ignore polite requests
    /// from outside the prefix, so this sends SIGKILL.
    pub fn kill(&self) -> Result<(), KillError> {
        const ATTEMPTS: usize = 20;

        for _ in 0..ATTEMPTS {
            let processes = scan::session_processes(Some(&self.session)).map_err(KillError::Scan)?;
            if processes.is_empty() {
                return Ok(());
            }
            for process in &processes {
                scan::kill(process.pid);
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        let survivors = scan::session_processes(Some(&self.session)).map_err(KillError::Scan)?;
        if survivors.is_empty() {
            return Ok(());
        }

        Err(KillError::Survivors { pids: survivors.into_iter().map(|process| process.pid).collect() })
    }
}

pub(crate) fn running_session_pids() -> std::io::Result<Vec<ProcessId>> {
    Ok(scan::session_processes(None)?.into_iter().map(|process| process.pid).collect())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug)]
struct SessionProcess {
    pid: ProcessId,
    session: SessionId,
    /// Final component of argv[0] in either path style.
    exe_name: Option<String>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn exe_name(argv0: &[u8]) -> Option<String> {
    let name = argv0.rsplit(|byte| *byte == b'/' || *byte == b'\\').next()?;
    (!name.is_empty()).then(|| String::from_utf8_lossy(name).into_owned())
}

#[cfg(target_os = "linux")]
mod scan {
    use std::os::unix::fs::MetadataExt;

    use super::*;

    /// Processes owned by this user whose environment carries the marker,
    /// optionally for one session only.
    pub(super) fn session_processes(only: Option<&SessionId>) -> std::io::Result<Vec<SessionProcess>> {
        let uid = rustix::process::getuid().as_raw();
        let own_pid = std::process::id();
        let mut prefix = super::SESSION_ENV.as_bytes().to_vec();
        prefix.push(b'=');

        let mut found = Vec::new();
        for entry in std::fs::read_dir("/proc")?.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else {
                continue;
            };
            if pid == own_pid || !entry.metadata().is_ok_and(|metadata| metadata.uid() == uid) {
                continue;
            }

            // Processes exit between listing and reading; skip them.
            let Ok(environ) = std::fs::read(entry.path().join("environ")) else {
                continue;
            };
            let Some(value) = environ.split(|byte| *byte == 0).find_map(|var| var.strip_prefix(prefix.as_slice()))
            else {
                continue;
            };
            let session = SessionId(String::from_utf8_lossy(value).into_owned());
            if only.is_some_and(|wanted| *wanted != session) {
                continue;
            }

            let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
            let exe_name = cmdline.split(|byte| *byte == 0).next().and_then(super::exe_name);
            found.push(SessionProcess { pid: ProcessId(pid), session, exe_name });
        }

        Ok(found)
    }

    pub(super) fn kill(pid: ProcessId) {
        let Some(raw) = i32::try_from(pid.0).ok().and_then(rustix::process::Pid::from_raw) else {
            return;
        };
        // The process may already be gone.
        let _ = rustix::process::kill_process(raw, rustix::process::Signal::KILL);
    }
}

/// Session tracking reads `/proc`.
#[cfg(not(target_os = "linux"))]
mod scan {
    use super::*;

    pub(super) fn session_processes(_only: Option<&SessionId>) -> std::io::Result<Vec<SessionProcess>> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    pub(super) fn kill(_pid: ProcessId) {}
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn exe_name_handles_windows_and_unix_paths() {
        assert_eq!(exe_name(br"Z:\games\bin64\WorldOfWarships64.exe").as_deref(), Some("WorldOfWarships64.exe"));
        assert_eq!(exe_name(b"/usr/bin/wineserver").as_deref(), Some("wineserver"));
        assert_eq!(exe_name(b""), None);
    }

    #[test]
    fn sessions_are_found_by_marker_and_killed() {
        let session = SessionId::new(format!("test-{}", std::process::id()));
        let child = std::process::Command::new("sleep").arg("60").env(SESSION_ENV, session.as_str()).spawn().unwrap();
        let process = GameProcess::new(child, session, None);
        let handle = process.handle();

        assert!(handle.is_running().unwrap());
        handle.kill().unwrap();
        let exit = process.wait().unwrap();
        assert!(!exit.runner_status.success());
        assert!(!handle.is_running().unwrap());
    }
}
