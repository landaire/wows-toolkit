use std::ffi::OsString;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::path::Path;
use std::process::Command;
use std::ptr::null;
use std::ptr::null_mut;

use wgc_relay_protocol::LAUNCH_MARKER_ENV;
use wgc_relay_protocol::RELAY_ADDR_ENV;
use wgc_relay_protocol::RELAY_TOKEN_ENV;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::IO::CreateIoCompletionPort;
use windows_sys::Win32::System::IO::GetQueuedCompletionStatus;
use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
use windows_sys::Win32::System::JobObjects::CreateJobObjectW;
use windows_sys::Win32::System::JobObjects::JOBOBJECT_ASSOCIATE_COMPLETION_PORT;
use windows_sys::Win32::System::JobObjects::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION;
use windows_sys::Win32::System::JobObjects::JobObjectAssociateCompletionPortInformation;
use windows_sys::Win32::System::JobObjects::JobObjectBasicAccountingInformation;
use windows_sys::Win32::System::JobObjects::QueryInformationJobObject;
use windows_sys::Win32::System::JobObjects::SetInformationJobObject;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::INFINITE;

use crate::error::RelayError;

/// Starts the game and waits until it and every process it started have
/// exited. `WorldOfWarships.exe` exits as soon as it has started the client in
/// `bin64`, so waiting on it alone would end the session early. Returns the
/// game executable's exit code.
pub fn run_game(game: &Path, args: &[OsString]) -> Result<u32, RelayError> {
    let session = Session::new()?;

    let mut command = Command::new(game);
    command.args(args).env(LAUNCH_MARKER_ENV, "1").env_remove(RELAY_ADDR_ENV).env_remove(RELAY_TOKEN_ENV);
    // The stub resolves bin64 and res relative to its working directory.
    if let Some(dir) = game.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        command.current_dir(dir);
    }
    let mut child = command.spawn().map_err(|source| RelayError::Spawn { path: game.to_path_buf(), source })?;

    session.wait_until_only_relay_remains()?;
    let status = child.wait().map_err(|source| RelayError::Wait { path: game.to_path_buf(), source })?;

    // An exit code is always present on Windows.
    Ok(status.code().map_or(1, |code| code as u32))
}

/// A job object holding the relay and, by inheritance, everything it starts.
struct Session {
    job: OwnedHandle,
    port: OwnedHandle,
}

impl Session {
    fn new() -> Result<Session, RelayError> {
        // SAFETY: unnamed job with default security.
        let job = unsafe { CreateJobObjectW(null(), null()) };
        if job.is_null() {
            return Err(RelayError::last_os_error("CreateJobObjectW"));
        }
        // SAFETY: a fresh handle this session now owns.
        let job = unsafe { OwnedHandle::from_raw_handle(job) };

        // SAFETY: creates a new port not tied to any file.
        let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
        if port.is_null() {
            return Err(RelayError::last_os_error("CreateIoCompletionPort"));
        }
        // SAFETY: a fresh handle this session now owns.
        let port = unsafe { OwnedHandle::from_raw_handle(port) };

        let association =
            JOBOBJECT_ASSOCIATE_COMPLETION_PORT { CompletionKey: null_mut(), CompletionPort: port.as_raw_handle() };
        // SAFETY: the struct is live and sized as the information class expects.
        let associated = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectAssociateCompletionPortInformation,
                (&raw const association).cast(),
                size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
            )
        } != 0;
        if !associated {
            return Err(RelayError::last_os_error("SetInformationJobObject"));
        }

        // SAFETY: the pseudo handle of this process and an owned job handle.
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), GetCurrentProcess()) } == 0 {
            return Err(RelayError::last_os_error("AssignProcessToJobObject"));
        }

        Ok(Session { job, port })
    }

    fn active_processes(&self) -> Result<u32, RelayError> {
        // SAFETY: plain-old-data out struct.
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the out struct is live and sized for the information class.
        let ok = unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&raw mut info).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        } != 0;
        if !ok {
            return Err(RelayError::last_os_error("QueryInformationJobObject"));
        }

        Ok(info.ActiveProcesses)
    }

    /// Blocks until the relay is the job's last process. Every job message
    /// prompts a recount, which also covers processes that left the job
    /// before a message was posted.
    fn wait_until_only_relay_remains(&self) -> Result<(), RelayError> {
        loop {
            if self.active_processes()? <= 1 {
                return Ok(());
            }

            let mut message = 0u32;
            let mut key = 0usize;
            let mut overlapped = null_mut();
            // SAFETY: out slots are live; the port is owned by this session.
            let ok = unsafe {
                GetQueuedCompletionStatus(self.port.as_raw_handle(), &mut message, &mut key, &mut overlapped, INFINITE)
            } != 0;
            if !ok {
                return Err(RelayError::last_os_error("GetQueuedCompletionStatus"));
            }
        }
    }
}
