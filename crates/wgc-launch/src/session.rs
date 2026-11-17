//! A running game session: the runner child plus every process that
//! inherited the session marker. Wine reparents processes and Proton may
//! run them in a container, so the process tree alone does not find them.

use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;

use crate::error::KillError;
use crate::error::WaitError;
use crate::lock::SessionLock;
use crate::overlay::FuseMount;
use crate::wineserver::PrefixServer;

/// Present in the environment of every process of a toolkit-launched
/// session. The value names the install (or the prefix setup run).
pub const SESSION_ENV: &str = "WOWS_TOOLKIT_GAME_SESSION";

/// The install root a session runs, beside `SESSION_ENV`, so a later toolkit
/// process can tell which install a session it did not start uses.
pub const SESSION_INSTALL_ENV: &str = "WOWS_TOOLKIT_GAME_INSTALL";

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const CLIENT_EXE: &str = "WorldOfWarships64.exe";
const STUB_EXE: &str = "WorldOfWarships.exe";

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

/// One process of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMember {
    pub pid: ProcessId,
    /// Final component of argv[0] in either path style.
    pub exe_name: Option<String>,
}

/// What every process of a session carries in its environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionMarker {
    pub(crate) id: SessionId,
    /// `None` for prefix setup, which runs no install.
    pub(crate) install: Option<PathBuf>,
}

/// A spawned session. Call `wait` to reap the runner and release the overlay
/// and the session lock.
#[derive(Debug)]
pub struct GameProcess {
    child: Child,
    handle: GameHandle,
    mount: Option<FuseMount>,
    lock: SessionLock,
}

impl GameProcess {
    pub(crate) fn new(
        child: Child,
        marker: SessionMarker,
        prefix_server: Option<PrefixServer>,
        mount: Option<FuseMount>,
        lock: SessionLock,
    ) -> Self {
        let handle = GameHandle::from_marker(marker, Some(ProcessId(child.id())), prefix_server);
        Self { child, handle, mount, lock }
    }

    pub fn handle(&self) -> GameHandle {
        self.handle.clone()
    }

    /// Blocks until the runner and every session process have exited, then
    /// unmounts a fuse overlay and releases the session lock. The session is
    /// over whichever way it returns. Without Linux process tracking this
    /// waits for the runner only.
    pub fn wait(self) -> Result<GameExit, WaitError> {
        let GameProcess { mut child, handle, mount, lock } = self;

        // Ended on a failed wait as well: the session is over either way, and nothing else
        // would end the prefix.
        let runner_status = child.wait();
        if let Some(server) = &handle.prefix_server {
            server.end();
        }
        let runner_status = runner_status.map_err(WaitError::Runner)?;
        handle.wait_exit();

        if let Some(mount) = mount {
            mount.unmount().map_err(WaitError::Unmount)?;
        }
        drop(lock);

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
    install: Option<Arc<Path>>,
    runner: Option<ProcessId>,
    /// Ends the prefix's Wine processes with the session. `None` for an
    /// adopted session and for a session that leaves them to Wine.
    prefix_server: Option<PrefixServer>,
}

impl GameHandle {
    /// The session this process started with `marker`, through `runner` when it keeps the child.
    pub(crate) fn from_marker(
        marker: SessionMarker,
        runner: Option<ProcessId>,
        prefix_server: Option<PrefixServer>,
    ) -> GameHandle {
        GameHandle { session: Arc::new(marker.id), install: marker.install.map(Arc::from), runner, prefix_server }
    }

    /// A session left running by an earlier toolkit process, if any.
    pub fn find_running() -> std::io::Result<Option<GameHandle>> {
        Ok(adopted(&scan::session_processes(None)?))
    }

    pub fn session(&self) -> &SessionId {
        &self.session
    }

    /// The install root the session runs. `None` for an adopted session
    /// whose processes do not name one.
    pub fn install(&self) -> Option<&Path> {
        self.install.as_deref()
    }

    /// The direct child (runner or bwrap). `None` for an adopted session.
    pub fn runner_pid(&self) -> Option<ProcessId> {
        self.runner
    }

    /// The game client's PID: `WorldOfWarships64.exe`, else the launcher stub.
    pub fn game_pid(&self) -> std::io::Result<Option<ProcessId>> {
        let processes = scan::session_processes(Some(&self.session))?;

        Ok(pid_named(&processes, CLIENT_EXE).or_else(|| pid_named(&processes, STUB_EXE)))
    }

    /// The PID of `WorldOfWarships64.exe`, which the stub starts once the
    /// client is cleared to run.
    pub fn client_pid(&self) -> std::io::Result<Option<ProcessId>> {
        let processes = scan::session_processes(Some(&self.session))?;

        Ok(pid_named(&processes, CLIENT_EXE))
    }

    pub fn is_running(&self) -> std::io::Result<bool> {
        Ok(!scan::session_processes(Some(&self.session))?.is_empty())
    }

    /// Every process of the session, the runner, Wine's and the game's.
    pub fn members(&self) -> std::io::Result<Vec<SessionMember>> {
        let processes = scan::session_processes(Some(&self.session))?;

        Ok(processes
            .into_iter()
            .map(|process| SessionMember { pid: process.pid, exe_name: process.exe_name })
            .collect())
    }

    /// Blocks until no session process remains. A failed scan is retried,
    /// since ending early would release the session while it may still run.
    /// Without Linux process tracking this returns at once.
    pub fn wait_exit(&self) {
        const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

        if !cfg!(target_os = "linux") {
            return;
        }

        let mut retry_delay = POLL_INTERVAL;
        loop {
            match self.is_running() {
                Ok(false) => return,
                Ok(true) => {
                    retry_delay = POLL_INTERVAL;
                    std::thread::sleep(POLL_INTERVAL);
                }
                Err(err) => {
                    tracing::warn!(
                        "could not list the processes of game session {}; retrying in {retry_delay:?}: {err}",
                        self.session.as_str()
                    );
                    std::thread::sleep(retry_delay);
                    retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY);
                }
            }
        }
    }

    /// Kills every session process: first the prefix's Wine processes
    /// through its wineserver, when the session ends them, then the rest
    /// with SIGKILL. Wine processes ignore polite requests from outside the
    /// prefix.
    pub fn kill(&self) -> Result<(), KillError> {
        const ATTEMPTS: usize = 20;

        if let Some(server) = &self.prefix_server {
            server.end();
        }

        for _ in 0..ATTEMPTS {
            let processes = scan::session_processes(Some(&self.session)).map_err(KillError::Scan)?;
            if processes.is_empty() {
                return Ok(());
            }
            for process in &processes {
                scan::kill(process).map_err(|source| KillError::Signal { pid: process.pid, source })?;
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
    /// Opened before the environment was read, so it names the process the
    /// marker came from or one that has exited, never a later holder of `pid`.
    pidfd: scan::ProcessFd,
    session: SessionId,
    /// The value of `SESSION_INSTALL_ENV`.
    install: Option<PathBuf>,
    /// Final component of argv[0] in either path style.
    exe_name: Option<String>,
}

/// A handle for the session of the first of `processes`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn adopted(processes: &[SessionProcess]) -> Option<GameHandle> {
    let first = processes.first()?;
    let install =
        processes.iter().filter(|process| process.session == first.session).find_map(|process| process.install.clone());

    Some(GameHandle {
        session: Arc::new(first.session.clone()),
        install: install.map(Arc::from),
        runner: None,
        prefix_server: None,
    })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn exe_name(argv0: &[u8]) -> Option<String> {
    let name = argv0.rsplit(|byte| *byte == b'/' || *byte == b'\\').next()?;
    (!name.is_empty()).then(|| String::from_utf8_lossy(name).into_owned())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn pid_named(processes: &[SessionProcess], exe: &str) -> Option<ProcessId> {
    processes
        .iter()
        .find(|process| process.exe_name.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(exe)))
        .map(|process| process.pid)
}

#[cfg(target_os = "linux")]
mod scan {
    use std::ffi::OsStr;
    use std::os::fd::OwnedFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    use rustix::io::Errno;
    use rustix::process::Pid;
    use rustix::process::PidfdFlags;
    use rustix::process::Signal;

    use super::*;

    pub(super) type ProcessFd = OwnedFd;

    /// Processes owned by this user whose environment carries the marker,
    /// optionally for one session only.
    pub(super) fn session_processes(only: Option<&SessionId>) -> std::io::Result<Vec<SessionProcess>> {
        let uid = rustix::process::getuid().as_raw();
        let own_pid = rustix::process::getpid();
        let mut session_prefix = super::SESSION_ENV.as_bytes().to_vec();
        session_prefix.push(b'=');
        let mut install_prefix = super::SESSION_INSTALL_ENV.as_bytes().to_vec();
        install_prefix.push(b'=');

        let mut found = Vec::new();
        for entry in std::fs::read_dir("/proc")? {
            let entry = entry?;
            let Some((id, pid)) = process_ids(&entry.file_name()) else {
                continue;
            };
            if pid == own_pid {
                continue;
            }

            let path = entry.path();
            let Some(metadata) = unless_gone(entry.metadata())? else {
                continue;
            };
            if metadata.uid() != uid {
                continue;
            }

            let pidfd = rustix::process::pidfd_open(pid, PidfdFlags::empty()).map_err(std::io::Error::from);
            let Some(pidfd) = unless_gone(pidfd)? else {
                continue;
            };
            let Some(environ) = unless_gone_or_foreign(std::fs::read(path.join("environ")))? else {
                continue;
            };
            let mut session = None;
            let mut install = None;
            for var in environ.split(|byte| *byte == 0) {
                if let Some(value) = var.strip_prefix(session_prefix.as_slice()) {
                    session = Some(SessionId(String::from_utf8_lossy(value).into_owned()));
                } else if let Some(value) = var.strip_prefix(install_prefix.as_slice()) {
                    install = Some(PathBuf::from(OsStr::from_bytes(value)));
                }
            }
            let Some(session) = session else {
                continue;
            };
            if only.is_some_and(|wanted| *wanted != session) {
                continue;
            }

            let Some(cmdline) = unless_gone_or_foreign(std::fs::read(path.join("cmdline")))? else {
                continue;
            };
            let exe_name = cmdline.split(|byte| *byte == 0).next().and_then(super::exe_name);
            found.push(SessionProcess { pid: id, pidfd, session, install, exe_name });
        }

        Ok(found)
    }

    pub(super) fn kill(process: &SessionProcess) -> std::io::Result<()> {
        match rustix::process::pidfd_send_signal(&process.pidfd, Signal::KILL) {
            // The process has already exited.
            Ok(()) | Err(Errno::SRCH) => Ok(()),
            Err(errno) => Err(errno.into()),
        }
    }

    /// The PID a `/proc` entry names, if it names a process.
    fn process_ids(name: &OsStr) -> Option<(ProcessId, Pid)> {
        let id = name.to_str()?.parse::<u32>().ok()?;
        let pid = Pid::from_raw(i32::try_from(id).ok()?)?;

        Some((ProcessId(id), pid))
    }

    /// `None` when the process exited while it was being read.
    fn unless_gone<T>(result: std::io::Result<T>) -> std::io::Result<Option<T>> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(err)
                if err.kind() == std::io::ErrorKind::NotFound
                    || err.raw_os_error() == Some(Errno::SRCH.raw_os_error()) =>
            {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    /// [`unless_gone`], and `None` for a process whose files the kernel withholds. It withholds
    /// them only from a process with other credentials, which no session process has.
    fn unless_gone_or_foreign<T>(result: std::io::Result<T>) -> std::io::Result<Option<T>> {
        match result {
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => Ok(None),
            result => unless_gone(result),
        }
    }
}

/// Session tracking reads `/proc`.
#[cfg(not(target_os = "linux"))]
mod scan {
    use super::*;

    /// Never constructed: scans fail on these platforms.
    #[derive(Debug)]
    pub(super) enum ProcessFd {}

    pub(super) fn session_processes(_only: Option<&SessionId>) -> std::io::Result<Vec<SessionProcess>> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    pub(super) fn kill(process: &SessionProcess) -> std::io::Result<()> {
        match process.pidfd {}
    }
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
        let tmp = tempfile::tempdir().unwrap();
        let lock = SessionLock::lock(&crate::dirs::RuntimeDirs::new(tmp.path())).unwrap();
        let install = PathBuf::from("/games/World_of_Warships");
        let marker = SessionMarker {
            id: SessionId::new(format!("test-{}", std::process::id())),
            install: Some(install.clone()),
        };
        let child = std::process::Command::new("sleep")
            .arg("60")
            .env(SESSION_ENV, marker.id.as_str())
            .env(SESSION_INSTALL_ENV, &install)
            .spawn()
            .unwrap();
        let process = GameProcess::new(child, marker.clone(), None, None, lock);
        let handle = process.handle();

        assert!(handle.is_running().unwrap());
        let found = adopted(&scan::session_processes(Some(&marker.id)).unwrap()).unwrap();
        assert_eq!(found.session(), &marker.id);
        assert_eq!(found.install(), Some(install.as_path()));

        handle.kill().unwrap();
        let exit = process.wait().unwrap();
        assert!(!exit.runner_status.success());
        assert!(!handle.is_running().unwrap());
    }
}
